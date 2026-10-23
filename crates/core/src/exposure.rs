//! Security › Exposure: what makes this PC reachable from outside (remote desktop, open ports,
//! router port forwards, remote-access apps, unsigned autostarts) and the reversible fixes.
//!
//! Read-only audit without admin wherever Windows allows (registry, IP Helper tables, firewall
//! COM, WMI SecurityCenter2, WTS, Task Scheduler COM, WinVerifyTrust). Fixes need admin (except
//! removing a router UPnP mapping, a network call any user can make), back up the original value
//! first (`registry` backups, ids `exposure_*`) and can be reverted. Never spawns a shell; never
//! kills a process. This is an exposure audit, not an antivirus.
//!
//! Every check has its decision logic in a pure function (tested below); the readers only gather
//! raw values. Strings from the system (process, app, service, task and router names) are data.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{HWND, VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::NetworkManagement::WindowsFirewall::{
    INetFwPolicy2, IStaticPortMapping, IUPnPNAT, NET_FW_PROFILE_TYPE2, NetFwPolicy2, UPnPNAT,
};
use windows::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO, WTD_CACHE_ONLY_URL_RETRIEVAL,
    WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE, WinVerifyTrust,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Ole::IEnumVARIANT;
use windows::Win32::System::RemoteDesktop::{
    WTS_CURRENT_SERVER_HANDLE, WTS_SESSION_INFOW, WTSClientName, WTSClientProtocolType, WTSEnumerateSessionsW,
    WTSFreeMemory, WTSQuerySessionInformationW,
};
use windows::Win32::System::Services::{
    ChangeServiceConfigW, CloseServiceHandle, ControlService, ENUM_SERVICE_TYPE, OpenSCManagerW, OpenServiceW, SC_HANDLE,
    SC_MANAGER_CONNECT, SERVICE_CHANGE_CONFIG, SERVICE_CONTROL_STOP, SERVICE_ERROR, SERVICE_NO_CHANGE, SERVICE_START,
    SERVICE_START_TYPE, SERVICE_STATUS, SERVICE_STOP, StartServiceW,
};
use windows::Win32::System::TaskScheduler::{IExecAction, ITaskFolder, ITaskService, TASK_ENUM_HIDDEN, TaskScheduler};
use windows::Win32::System::Variant::{VARIANT, VT_DISPATCH};
use windows::core::{BSTR, GUID, HRESULT, Interface, PCWSTR, PWSTR};

use crate::advisor;
use crate::netconns::{self, Conn, Protocol, Scope, TcpState};
use crate::registry::{self, Backup, BackupEntry, Root, Value, wide};
use crate::tasksched::{self, Com};
use crate::{apps, boost, dns, system, wmi};

/* ---------------- model ---------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Nothing exposed.
    Ok,
    Exposed,
    /// Could not be read (router silent, WMI namespace missing, …): `details.reason` says why.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FwProfile {
    Domain,
    Private,
    Public,
}

impl FwProfile {
    pub const ALL: [FwProfile; 3] = [FwProfile::Domain, FwProfile::Private, FwProfile::Public];
    fn bit(self) -> i32 {
        match self {
            FwProfile::Domain => 1,
            FwProfile::Private => 2,
            FwProfile::Public => 4,
        }
    }
    fn tag(self) -> &'static str {
        match self {
            FwProfile::Domain => "domain",
            FwProfile::Private => "private",
            FwProfile::Public => "public",
        }
    }
}

/// A reversible change. Comes back from the UI (hostile): every variant is re-validated against
/// the live state before anything is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Fix {
    RdpDisable,
    NlaEnable,
    RaDisable,
    Smb1ServerDisable,
    FirewallOn { profile: FwProfile },
    UpnpRemove { protocol: String, external_port: u16 },
    WinrmStop,
    TaskDisable { path: String },
}

impl Fix {
    /// Backup id (`registry::safe_id`: `[a-z0-9_]`, ≤ 64).
    pub fn backup_id(&self) -> String {
        match self {
            Fix::RdpDisable => "exposure_rdp".into(),
            Fix::NlaEnable => "exposure_nla".into(),
            Fix::RaDisable => "exposure_ra".into(),
            Fix::Smb1ServerDisable => "exposure_smb1".into(),
            Fix::FirewallOn { profile } => format!("exposure_fw_{}", profile.tag()),
            Fix::UpnpRemove { protocol, external_port } => {
                format!("exposure_upnp_{}_{external_port}", if protocol.eq_ignore_ascii_case("udp") { "udp" } else { "tcp" })
            }
            Fix::WinrmStop => "exposure_winrm".into(),
            Fix::TaskDisable { path } => format!("exposure_task_{:016x}", fnv(&path.to_ascii_lowercase())),
        }
    }

    /// Removing a router mapping is a network call any user can make; the rest is HKLM/SCM/firewall.
    pub fn needs_admin(&self) -> bool {
        !matches!(self, Fix::UpnpRemove { .. } | Fix::TaskDisable { .. })
    }
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// One entry of a list check (listener, app, mapping, task…). `name` is data from the PC.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Item {
    pub name: String,
    pub path: Option<String>,
    /// Where it was found: `process`, `service`, `installed`, `task`, `rdp`, `profile`, `product`.
    pub source: Option<&'static str>,
    /// Why it is listed (`unsigned`, `user_folder`, `remote_desktop`, `rdp` port label, `off`…).
    pub reason: Option<&'static str>,
    pub port: Option<u16>,
    pub protocol: Option<&'static str>,
    /// Remote IP (inbound), client computer (session) or `ip:port` inside the LAN (UPnP).
    pub remote: Option<String>,
    pub severity: Severity,
    pub fix: Option<Fix>,
}

impl Item {
    fn new(name: impl Into<String>, severity: Severity) -> Self {
        Item { name: clean(&name.into()), path: None, source: None, reason: None, port: None, protocol: None, remote: None, severity, fix: None }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Check {
    /// `rdp`, `nla`, `remote_assistance`, `winrm`, `smb1_server`, `smb1_client`, `firewall`,
    /// `upnp`, `remote_apps`, `listeners`, `inbound`, `sessions`, `persistence`, `guest`, `antivirus`.
    pub id: &'static str,
    pub state: State,
    /// Meaningful when `state == Exposed`.
    pub severity: Severity,
    pub details: BTreeMap<&'static str, String>,
    pub items: Vec<Item>,
    pub fix: Option<Fix>,
}

impl Check {
    fn ok(id: &'static str) -> Self {
        Check { id, state: State::Ok, severity: Severity::Info, details: BTreeMap::new(), items: Vec::new(), fix: None }
    }
    fn exposed(id: &'static str, severity: Severity) -> Self {
        Check { state: State::Exposed, severity, ..Check::ok(id) }
    }
    fn unknown(id: &'static str, reason: &str) -> Self {
        let mut c = Check { state: State::Unknown, ..Check::ok(id) };
        c.details.insert("reason", reason.to_string());
        c
    }
    fn detail(mut self, k: &'static str, v: impl ToString) -> Self {
        self.details.insert(k, v.to_string());
        self
    }
    /// Exposed with the worst item severity when there are items, OK otherwise.
    fn from_items(id: &'static str, items: Vec<Item>) -> Self {
        match items.iter().map(|i| i.severity).max() {
            Some(s) => Check { items, ..Check::exposed(id, s) },
            None => Check::ok(id),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub checks: Vec<Check>,
    pub elevated: bool,
    /// Fixes with a backup on disk (the UI offers "undo").
    pub reverts: Vec<Fix>,
    pub took_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ExposureError {
    #[error("needs_admin")]
    NeedsAdmin,
    /// A Group Policy sets this; a local change would be overwritten.
    #[error("managed_by_policy")]
    Policy,
    /// The thing to change is not there (anymore) or was not in the last report.
    #[error("not_applicable")]
    NotApplicable,
    #[error("no_backup")]
    NoBackup,
    #[error("router_unavailable")]
    Router,
    #[error("{0}")]
    Other(String),
}

impl From<registry::RegError> for ExposureError {
    fn from(e: registry::RegError) -> Self {
        match e {
            registry::RegError::AccessDenied => ExposureError::NeedsAdmin,
            e => ExposureError::Other(e.to_string()),
        }
    }
}

const E_ACCESS_DENIED: HRESULT = HRESULT(0x8007_0005_u32 as i32);

fn win(e: windows::core::Error) -> ExposureError {
    if e.code() == E_ACCESS_DENIED { ExposureError::NeedsAdmin } else { ExposureError::Other(format!("{:?}: {}", e.code(), e.message())) }
}

/// One line, no control characters, bounded (system and router strings are hostile).
fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(160).collect::<String>().trim().to_string()
}

/* ---------------- registry decisions (pure) ---------------- */

const TS: &str = r"SYSTEM\CurrentControlSet\Control\Terminal Server";
const RDP_TCP: &str = r"SYSTEM\CurrentControlSet\Control\Terminal Server\WinStations\RDP-Tcp";
const TS_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services";
const RA: &str = r"SYSTEM\CurrentControlSet\Control\Remote Assistance";
const SMB_SERVER: &str = r"SYSTEM\CurrentControlSet\Services\LanmanServer\Parameters";
const SERVICES: &str = r"SYSTEM\CurrentControlSet\Services";

/// Raw registry values the remote-desktop checks need (`None` = value absent).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RdpRaw {
    pub deny: Option<u32>,
    pub deny_policy: Option<u32>,
    pub nla: Option<u32>,
    pub nla_policy: Option<u32>,
    pub port: Option<u32>,
}

/// RDP on/off and NLA. A Group Policy value wins over the local one; then no fix is offered.
pub(crate) fn rdp_checks(r: RdpRaw) -> (Check, Check) {
    // fDenyTSConnections absent = 1 (deny) on every edition.
    let enabled = r.deny_policy.or(r.deny) == Some(0);
    if !enabled {
        return (Check::ok("rdp"), Check::ok("nla").detail("rdp", "off"));
    }
    // UserAuthentication absent = NLA required (the default since Windows Vista SP1).
    let nla_off = r.nla_policy.or(r.nla) == Some(0);
    let port = r.port.filter(|p| (1..=65535).contains(p)).unwrap_or(3389);
    let mut rdp = Check::exposed("rdp", if nla_off { Severity::High } else { Severity::Medium }).detail("port", port);
    if port != 3389 {
        rdp = rdp.detail("custom_port", "1");
    }
    if r.deny_policy.is_some() {
        rdp = rdp.detail("policy", "1");
    } else {
        rdp.fix = Some(Fix::RdpDisable);
    }
    let nla = if nla_off {
        let mut c = Check::exposed("nla", Severity::High);
        if r.nla_policy.is_some() {
            c = c.detail("policy", "1");
        } else {
            c.fix = Some(Fix::NlaEnable);
        }
        c
    } else {
        Check::ok("nla")
    };
    (rdp, nla)
}

pub(crate) fn remote_assistance_check(allow: Option<u32>, policy: Option<u32>) -> Check {
    if policy.or(allow) != Some(1) {
        return Check::ok("remote_assistance");
    }
    let mut c = Check::exposed("remote_assistance", Severity::Low);
    if policy.is_some() {
        c = c.detail("policy", "1");
    } else {
        c.fix = Some(Fix::RaDisable);
    }
    c
}

/// SMBv1 server: the `srv` driver (installed only with the SMB 1.0 feature) not disabled and
/// `LanmanServer\Parameters\SMB1` not 0. Client: the `mrxsmb10` driver not disabled.
pub(crate) fn smb1_checks(server_driver_start: Option<u32>, server_param: Option<u32>, client_driver_start: Option<u32>) -> (Check, Check) {
    let server_on = server_driver_start.is_some_and(|s| s != 4) && server_param != Some(0);
    let server = if server_on {
        Check { fix: Some(Fix::Smb1ServerDisable), ..Check::exposed("smb1_server", Severity::High) }
    } else {
        Check::ok("smb1_server").detail("driver", if server_driver_start.is_some() { "present" } else { "absent" })
    };
    let client = if client_driver_start.is_some_and(|s| s != 4) { Check::exposed("smb1_client", Severity::Medium) } else { Check::ok("smb1_client") };
    (server, client)
}

/* ---------------- ports and addresses (pure) ---------------- */

/// Risk and a short protocol label of a listening port (labels are protocol names, not apps).
pub(crate) fn port_risk(port: u16) -> (Severity, Option<&'static str>) {
    match port {
        21 => (Severity::High, Some("ftp")),
        23 | 2323 => (Severity::High, Some("telnet")),
        135 => (Severity::High, Some("rpc")),
        139 => (Severity::High, Some("netbios")),
        445 => (Severity::High, Some("smb")),
        3389 => (Severity::High, Some("rdp")),
        5555 => (Severity::High, Some("adb")),
        5900..=5902 => (Severity::High, Some("vnc")),
        5985 | 5986 => (Severity::High, Some("winrm")),
        22 => (Severity::Medium, Some("ssh")),
        1433 => (Severity::Medium, Some("sql")),
        3306 => (Severity::Medium, Some("mysql")),
        5432 => (Severity::Medium, Some("postgres")),
        6379 => (Severity::Medium, Some("redis")),
        9200 => (Severity::Medium, Some("search")),
        11211 => (Severity::Medium, Some("cache")),
        27017 => (Severity::Medium, Some("mongo")),
        _ => (Severity::Info, None),
    }
}

/// `true` for the kernel, svchost & co.: anything under `%windir%` (lowercase) or pid 0/4.
fn is_system_proc(c: &Conn, windir: &str) -> bool {
    c.pid <= 4 || (!c.exe_path.is_empty() && under(&c.exe_path.to_ascii_lowercase(), windir))
}

fn under(path_l: &str, dir_l: &str) -> bool {
    !dir_l.is_empty() && path_l.len() > dir_l.len() && path_l.starts_with(dir_l) && path_l.as_bytes()[dir_l.len()] == b'\\'
}

/// Ports Windows' own services listen on (RPC, NetBIOS, SMB, RDP, WinRM): a process we cannot
/// open that listens there is a protected system service (the RDP/SMB/WinRM checks cover them).
const WINDOWS_PORTS: &[u16] = &[135, 139, 445, 3389, 5985, 5986];

/// TCP listeners reachable from another machine (not loopback) owned by non-system programs.
/// A process whose path cannot be read only counts on a risky port that is not Windows' own.
pub(crate) fn exposed_listeners(conns: &[Conn], windir: &str) -> Vec<Item> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for c in conns {
        if c.protocol != Protocol::Tcp || c.state != Some(TcpState::Listen) || netconns::remote_scope(c.local_addr) == Scope::Loopback {
            continue;
        }
        if is_system_proc(c, windir) {
            continue;
        }
        let (sev, label) = port_risk(c.local_port);
        if c.exe_path.is_empty() && (sev < Severity::Medium || WINDOWS_PORTS.contains(&c.local_port)) {
            continue;
        }
        if !seen.insert((c.process.to_ascii_lowercase(), c.local_port)) {
            continue;
        }
        let mut it = Item::new(&c.process, sev);
        it.path = (!c.exe_path.is_empty()).then(|| c.exe_path.clone());
        it.source = Some("process");
        it.reason = label;
        it.port = Some(c.local_port);
        it.protocol = Some("tcp");
        out.push(it);
    }
    out.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.port.cmp(&b.port)));
    out
}

/// Established TCP whose local port is one this PC listens on (not loopback) and whose remote
/// end is a public address: someone on the internet connected in.
pub(crate) fn inbound_public(conns: &[Conn]) -> Vec<Item> {
    let listening: HashSet<u16> = conns
        .iter()
        .filter(|c| c.protocol == Protocol::Tcp && c.state == Some(TcpState::Listen) && netconns::remote_scope(c.local_addr) != Scope::Loopback)
        .map(|c| c.local_port)
        .collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for c in conns {
        if c.protocol != Protocol::Tcp || c.state != Some(TcpState::Established) || c.remote_scope != Scope::Public || !listening.contains(&c.local_port) {
            continue;
        }
        let Some(remote) = c.remote_addr else { continue };
        if !seen.insert((c.process.to_ascii_lowercase(), c.local_port, remote)) {
            continue;
        }
        let (sev, label) = port_risk(c.local_port);
        let mut it = Item::new(&c.process, if sev == Severity::High { Severity::High } else { Severity::Medium });
        it.path = (!c.exe_path.is_empty()).then(|| c.exe_path.clone());
        it.source = Some("process");
        it.reason = label;
        it.port = Some(c.local_port);
        it.protocol = Some("tcp");
        it.remote = Some(remote.to_string());
        out.push(it);
        if out.len() >= 50 {
            break;
        }
    }
    out
}

/* ---------------- firewall (pure) ---------------- */

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Firewall {
    /// `NET_FW_PROFILE2_*` bits of the networks in use.
    pub current: i32,
    /// domain, private, public.
    pub enabled: [Option<bool>; 3],
}

/// Off on the profile in use = high (medium when another firewall is registered and on); off on a
/// profile not in use = medium/low.
pub(crate) fn firewall_check(fw: Option<Firewall>, third_party_on: Option<&str>) -> Check {
    let Some(fw) = fw else { return Check::unknown("firewall", "unreadable") };
    let mut items = Vec::new();
    for (i, p) in FwProfile::ALL.iter().enumerate() {
        if fw.enabled[i] != Some(false) {
            continue;
        }
        let current = fw.current & p.bit() != 0;
        let sev = match (current, third_party_on.is_some()) {
            (true, false) => Severity::High,
            (true, true) | (false, false) => Severity::Medium,
            (false, true) => Severity::Low,
        };
        let mut it = Item::new(p.tag(), sev);
        it.source = Some(if current { "current" } else { "profile" });
        it.reason = Some("off");
        it.fix = Some(Fix::FirewallOn { profile: *p });
        items.push(it);
    }
    let fix = items.iter().find(|i| i.source == Some("current")).and_then(|i| i.fix.clone());
    let mut c = Check::from_items("firewall", items);
    c.fix = fix;
    if let Some(n) = third_party_on {
        c = c.detail("third_party", clean(n));
    }
    c
}

/* ---------------- antivirus (pure) ---------------- */

/// `productState` of `root\SecurityCenter2`: bits 12–15 = scanner (1 on, 0 off, 2 snoozed,
/// 3 expired), bits 4–7 = definitions (0 up to date, 1 out of date).
pub(crate) fn product_state(state: i64) -> (&'static str, bool) {
    let scanner = match (state >> 12) & 0xF {
        1 => "on",
        2 => "snoozed",
        3 => "expired",
        _ => "off",
    };
    (scanner, (state >> 4) & 0xF == 0)
}

pub(crate) fn antivirus_check(products: Option<Vec<(String, i64)>>) -> Check {
    let Some(products) = products else { return Check::unknown("antivirus", "unreadable") };
    let mut items = Vec::new();
    let mut any_on = false;
    let mut on_but_old = false;
    for (name, st) in &products {
        let (scanner, fresh) = product_state(*st);
        let on = scanner == "on";
        any_on |= on;
        on_but_old |= on && !fresh;
        let mut it = Item::new(name.as_str(), if on && fresh { Severity::Info } else { Severity::Low });
        it.source = Some("product");
        it.reason = Some(if on && !fresh { "outdated" } else { scanner });
        // One product can register several entries with the same name.
        if !items.iter().any(|x: &Item| x.name == it.name && x.reason == it.reason) {
            items.push(it);
        }
    }
    let mut c = if !any_on {
        Check::exposed("antivirus", Severity::High)
    } else if on_but_old && !products.iter().any(|(_, s)| product_state(*s) == ("on", true)) {
        Check::exposed("antivirus", Severity::Medium)
    } else {
        Check::ok("antivirus")
    };
    c.items = items;
    c
}

/* ---------------- sessions, guest (pure) ---------------- */

#[derive(Debug, Clone)]
pub(crate) struct SessionRow {
    pub id: u32,
    /// `WTSClientProtocolType`: 0 console, 1 ICA, 2 RDP.
    pub protocol: u16,
    /// `WTS_CONNECTSTATE_CLASS`: 0 active, 1 connected, 4 disconnected, 6 listen…
    pub state: i32,
    pub client: String,
}

pub(crate) fn remote_sessions(rows: &[SessionRow]) -> Vec<Item> {
    rows.iter()
        .filter(|r| r.protocol == 2 && matches!(r.state, 0 | 1 | 4))
        .map(|r| {
            let live = r.state != 4;
            let mut it = Item::new(if r.client.is_empty() { format!("#{}", r.id) } else { r.client.clone() }, if live { Severity::High } else { Severity::Medium });
            it.source = Some("rdp");
            it.reason = Some(if live { "active" } else { "disconnected" });
            it
        })
        .collect()
}

/// The built-in guest account is the local one whose SID ends in the well-known RID 501.
pub(crate) fn guest_enabled(rows: &[(String, bool)]) -> Option<bool> {
    rows.iter().find(|(sid, _)| sid.starts_with("S-1-5-21-") && sid.ends_with("-501")).map(|(_, disabled)| !disabled)
}

/* ---------------- UPnP (pure) ---------------- */

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mapping {
    pub protocol: String,
    pub external_port: u16,
    pub internal_port: u16,
    pub internal_client: String,
    pub enabled: bool,
    pub description: String,
}

/// Mappings whose internal client is one of this PC's addresses (router data is hostile: the
/// client must parse as an IP, the protocol must be TCP or UDP).
pub(crate) fn mappings_to_me(all: &[Mapping], mine: &HashSet<IpAddr>) -> Vec<Mapping> {
    all.iter()
        .filter(|m| matches!(m.protocol.as_str(), "TCP" | "UDP") && m.external_port > 0)
        .filter(|m| m.internal_client.trim().parse::<IpAddr>().is_ok_and(|ip| mine.contains(&ip)))
        .cloned()
        .collect()
}

pub(crate) fn upnp_check(r: &Result<Vec<Mapping>, &'static str>, mine: &HashSet<IpAddr>) -> Check {
    match r {
        Err(why) => Check::unknown("upnp", why),
        Ok(all) => {
            let items = mappings_to_me(all, mine)
                .into_iter()
                .map(|m| {
                    let (risk, label) = port_risk(m.internal_port);
                    let mut it = Item::new(if m.description.is_empty() { format!("{} {}", m.protocol, m.external_port) } else { m.description.clone() }, if risk == Severity::High { Severity::High } else { Severity::Medium });
                    it.source = Some("router");
                    it.reason = label;
                    it.port = Some(m.external_port);
                    it.protocol = Some(if m.protocol == "UDP" { "udp" } else { "tcp" });
                    it.remote = Some(format!("{}:{}", m.internal_client.trim(), m.internal_port));
                    it.fix = Some(Fix::UpnpRemove { protocol: m.protocol.clone(), external_port: m.external_port });
                    it
                })
                .collect();
            Check::from_items("upnp", items).detail("router_mappings", all.len())
        }
    }
}

/* ---------------- remote-access software (data + pure match) ---------------- */

struct Known {
    /// `remote_desktop`, `rmm` (remote management agent), `vnc`, `tunnel`.
    kind: &'static str,
    /// Lowercase exe file names.
    exes: &'static [&'static str],
    /// Lowercase service key prefixes.
    services: &'static [&'static str],
    /// Lowercase fragments of the Uninstall `DisplayName` (empty = only reported while running).
    names: &'static [&'static str],
}

/// Remote desktop/control tools, VNC servers, management agents and tunnels. Matching data only:
/// what the UI shows is the name found on the PC.
const KNOWN: &[Known] = &[
    Known { kind: "remote_desktop", exes: &["teamviewer.exe", "teamviewer_service.exe", "tv_w32.exe", "tv_x64.exe"], services: &["teamviewer"], names: &["teamviewer"] },
    Known { kind: "remote_desktop", exes: &["anydesk.exe"], services: &["anydesk"], names: &["anydesk"] },
    Known { kind: "remote_desktop", exes: &["rustdesk.exe"], services: &["rustdesk"], names: &["rustdesk"] },
    Known { kind: "remote_desktop", exes: &["ultraviewer_desktop.exe", "ultraviewer_service.exe"], services: &["ultraviewer"], names: &["ultraviewer"] },
    Known { kind: "remote_desktop", exes: &["remoting_host.exe", "remoting_desktop.exe"], services: &["chromoting"], names: &["chrome remote desktop host"] },
    Known { kind: "remote_desktop", exes: &["srservice.exe", "srmanager.exe", "srserver.exe"], services: &["splashtopremoteservice"], names: &["splashtop streamer"] },
    Known { kind: "remote_desktop", exes: &["parsecd.exe"], services: &["parsec"], names: &["parsec"] },
    Known { kind: "remote_desktop", exes: &["sunshine.exe"], services: &["sunshineservice"], names: &["sunshine"] },
    Known { kind: "remote_desktop", exes: &["nxserver.exe", "nxservice64.exe"], services: &["nxservice"], names: &["nomachine"] },
    Known { kind: "remote_desktop", exes: &["logmein.exe", "lmiguardiansvc.exe"], services: &["logmein"], names: &["logmein"] },
    Known { kind: "remote_desktop", exes: &["supremo.exe", "supremoservice.exe"], services: &["supremoservice"], names: &["supremo"] },
    Known { kind: "remote_desktop", exes: &["rserver3.exe"], services: &["rserver3"], names: &["radmin server"] },
    Known { kind: "remote_desktop", exes: &["rutserv.exe", "rfusclient.exe"], services: &["rmanservice"], names: &["remote utilities - host"] },
    Known { kind: "remote_desktop", exes: &["aeroadmin.exe"], services: &["aeroadmin"], names: &["aeroadmin"] },
    Known { kind: "remote_desktop", exes: &["hoptodesk.exe"], services: &["hoptodesk"], names: &["hoptodesk"] },
    Known { kind: "remote_desktop", exes: &["aa_v3.exe", "ammyy_admin.exe"], services: &["ammyy"], names: &["ammyy"] },
    Known { kind: "remote_desktop", exes: &["getscreen.exe"], services: &["getscreen"], names: &["getscreen"] },
    Known { kind: "remote_desktop", exes: &["remotepcservice.exe", "remotepcdesktop.exe"], services: &["remotepc"], names: &["remotepc"] },
    Known { kind: "remote_desktop", exes: &["za_connect.exe", "zaservice.exe"], services: &["zohoassist", "za_service"], names: &["zoho assist"] },
    Known { kind: "remote_desktop", exes: &["dwagent.exe", "dwagsvc.exe"], services: &["dwagent"], names: &["dwagent"] },
    Known { kind: "remote_desktop", exes: &["quickassist.exe"], services: &[], names: &[] },
    Known { kind: "vnc", exes: &["winvnc.exe", "winvnc4.exe", "tvnserver.exe", "vncserver.exe"], services: &["tvnserver", "uvnc_service", "vncserver", "winvnc"], names: &["tightvnc", "ultravnc", "realvnc", "tigervnc", "vnc server"] },
    Known { kind: "rmm", exes: &["screenconnect.clientservice.exe", "screenconnect.windowsclient.exe"], services: &["screenconnect client"], names: &["screenconnect client", "connectwise control"] },
    Known { kind: "rmm", exes: &["ateraagent.exe"], services: &["ateraagent"], names: &["ateraagent", "atera agent"] },
    Known { kind: "rmm", exes: &["ninjarmmagent.exe"], services: &["ninjarmmagent"], names: &["ninjarmmagent", "ninjaone agent"] },
    Known { kind: "rmm", exes: &["meshagent.exe"], services: &["mesh agent"], names: &["mesh agent"] },
    Known { kind: "rmm", exes: &["action1_agent.exe"], services: &["a1agent"], names: &["action1"] },
    Known { kind: "rmm", exes: &["ltsvc.exe", "lttray.exe"], services: &["ltservice"], names: &["connectwise automate", "labtech"] },
    Known { kind: "rmm", exes: &["agentmon.exe"], services: &["kaseya"], names: &["kaseya agent"] },
    Known { kind: "rmm", exes: &["pcmonitorsrv.exe"], services: &["pc monitor"], names: &["pulseway"] },
    Known { kind: "tunnel", exes: &["ngrok.exe"], services: &["ngrok"], names: &["ngrok"] },
    Known { kind: "tunnel", exes: &["cloudflared.exe"], services: &["cloudflared"], names: &["cloudflared"] },
    Known { kind: "tunnel", exes: &["frpc.exe", "frps.exe"], services: &["frpc"], names: &[] },
];

/// Raw sources for the remote-access match.
#[derive(Debug, Default, Clone)]
pub(crate) struct RemoteRaw {
    /// Running process image paths.
    pub processes: Vec<String>,
    /// (service key, display name, start type, exe path).
    pub services: Vec<(String, String, u32, String)>,
    /// Uninstall `DisplayName`s.
    pub installed: Vec<String>,
}

/// One item per known tool found: running > auto-start service > installed. Name = what was found.
pub(crate) fn remote_apps(raw: &RemoteRaw) -> Vec<Item> {
    let mut out = Vec::new();
    for k in KNOWN {
        let running = raw.processes.iter().find(|p| {
            let n = p.rsplit('\\').next().unwrap_or(p).to_ascii_lowercase();
            k.exes.contains(&n.as_str())
        });
        let service = raw
            .services
            .iter()
            .filter(|(key, ..)| {
                let l = key.to_ascii_lowercase();
                k.services.iter().any(|s| l.starts_with(s))
            })
            .find(|s| s.2 != 4);
        let installed = (!k.names.is_empty())
            .then(|| raw.installed.iter().find(|n| { let l = n.to_ascii_lowercase(); k.names.iter().any(|m| l.contains(m)) }))
            .flatten();
        let (name, path, source, sev) = if let Some(p) = running {
            (installed.cloned().unwrap_or_else(|| p.rsplit('\\').next().unwrap_or(p).to_string()), Some(p.clone()), "process", Severity::Medium)
        } else if let Some((key, display, start, exe)) = service {
            let sev = if *start == 2 { Severity::Low } else { Severity::Info };
            let name = installed.cloned().unwrap_or_else(|| if display.is_empty() { key.clone() } else { display.clone() });
            (name, (!exe.is_empty()).then(|| exe.clone()), "service", sev)
        } else if let Some(n) = installed {
            (n.clone(), None, "installed", Severity::Info)
        } else {
            continue;
        };
        let mut it = Item::new(name, sev);
        it.path = path;
        it.source = Some(source);
        it.reason = Some(k.kind);
        out.push(it);
    }
    out.sort_by(|a, b| b.severity.cmp(&a.severity).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

/* ---------------- persistence (pure) ---------------- */

#[derive(Debug, Clone, Default)]
pub(crate) struct Dirs {
    /// All lowercase, no trailing backslash.
    pub windir: String,
    pub temp: Vec<String>,
    pub public: String,
}

impl Dirs {
    fn live() -> Self {
        let env = |k: &str| std::env::var(k).unwrap_or_default().trim_end_matches('\\').to_ascii_lowercase();
        let windir = { let w = env("SystemRoot"); if w.is_empty() { r"c:\windows".to_string() } else { w } };
        Dirs { temp: vec![env("TEMP"), format!(r"{windir}\temp")], public: env("PUBLIC"), windir }
    }
}

/// Which user-writable folder a lowercase path lives in, if any.
pub(crate) fn user_folder(p: &str, d: &Dirs) -> Option<&'static str> {
    if d.temp.iter().any(|t| under(p, t)) || p.contains(r"\temp\") {
        Some("temp")
    } else if under(p, &d.public) || p.contains(r"\users\public\") {
        Some("public")
    } else if p.contains(r"\downloads\") {
        Some("downloads")
    } else if p.contains(r"\appdata\") {
        Some("appdata")
    } else {
        None
    }
}

/// Script hosts and loaders that run whatever their arguments point at.
const LOLBINS: &[&str] = &[
    "rundll32.exe", "regsvr32.exe", "mshta.exe", "wscript.exe", "cscript.exe", "powershell.exe", "pwsh.exe", "cmd.exe", "msiexec.exe",
];

/// Severity + reason for an autostart, `None` when it looks normal. `signed`: `Some(false)` = no
/// valid embedded signature, `None` = could not tell (file missing / unreadable).
/// Signed programs in AppData are normal (per-user updaters); signed ones in Temp, Downloads or
/// Public are not.
pub(crate) fn persistence_verdict(exe_l: &str, args_l: &str, signed: Option<bool>, d: &Dirs) -> Option<(Severity, &'static str)> {
    let folder = user_folder(exe_l, d);
    if folder.is_none() && under(exe_l, &d.windir) {
        let name = exe_l.rsplit('\\').next().unwrap_or(exe_l);
        let marker = [r"\appdata\", r"\temp\", r"\downloads\", r"\users\public\"].iter().any(|m| args_l.contains(m));
        return (LOLBINS.contains(&name) && marker).then_some((Severity::High, "script_user_folder"));
    }
    match (signed, folder) {
        (Some(false), Some(_)) => Some((Severity::High, "unsigned_user_folder")),
        (Some(false), None) => Some((Severity::Medium, "unsigned")),
        (_, Some("temp" | "downloads" | "public")) => Some((Severity::Medium, "user_folder")),
        _ => None,
    }
}

/// WinVerifyTrust result → signed? `TRUST_E_NOSIGNATURE`, unknown subject form, bad digest,
/// untrusted root and explicit distrust = not validly signed; anything else unknown.
pub(crate) fn signature_from_code(code: i32) -> Option<bool> {
    match code as u32 {
        0 => Some(true),
        0x800B_0100 | 0x800B_0003 | 0x8009_6010 | 0x800B_0109 | 0x800B_0111 | 0x800B_010C => Some(false),
        _ => None,
    }
}

/* ---------------- monitor baseline (pure) ---------------- */

/// Something the monitor can notice appearing. `key` identifies it across runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Observed {
    pub kind: &'static str,
    pub key: String,
    pub name: String,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Alert {
    /// `listener`, `inbound`, `remote_app`, `upnp`.
    pub kind: &'static str,
    pub name: String,
    pub port: Option<u16>,
    pub at: u64,
}

const BASELINE_CAP: usize = 2000;
const KINDS: &[&str] = &["listener", "inbound", "remote_app", "upnp"];

/// The baseline file is hostile input: keys must be `kind:…`, one line, bounded.
pub(crate) fn parse_baseline(text: &str) -> Option<BTreeSet<String>> {
    let v: Vec<String> = serde_json::from_str(text).ok()?;
    Some(
        v.into_iter()
            .filter(|k| k.len() <= 200 && !k.chars().any(char::is_control) && k.split_once(':').is_some_and(|(kind, rest)| KINDS.contains(&kind) && !rest.is_empty()))
            .take(BASELINE_CAP)
            .collect(),
    )
}

pub(crate) fn diff<'a>(base: &BTreeSet<String>, now: &'a [Observed]) -> Vec<&'a Observed> {
    let mut seen = HashSet::new();
    now.iter().filter(|o| !base.contains(&o.key) && seen.insert(o.key.as_str())).collect()
}

/* ---------------- readers (live) ---------------- */

fn dw(root: Root, path: &str, name: &str) -> Option<u32> {
    match registry::read(root, path, name) {
        Ok(Value::Dword(d)) => Some(d),
        _ => None,
    }
}

fn rdp_raw() -> RdpRaw {
    RdpRaw {
        deny: dw(Root::Hklm, TS, "fDenyTSConnections"),
        deny_policy: dw(Root::Hklm, TS_POLICY, "fDenyTSConnections"),
        nla: dw(Root::Hklm, RDP_TCP, "UserAuthentication"),
        nla_policy: dw(Root::Hklm, TS_POLICY, "UserAuthentication"),
        port: dw(Root::Hklm, RDP_TCP, "PortNumber"),
    }
}

fn firewall_policy() -> windows::core::Result<INetFwPolicy2> {
    // SAFETY: in-proc activation of the firewall policy object on a COM-initialised thread.
    unsafe { CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER) }
}

fn firewall_read() -> Option<Firewall> {
    let _com = Com::init().ok()?;
    let p = firewall_policy().ok()?;
    let mut fw = Firewall::default();
    // SAFETY: plain property reads on a valid policy object.
    unsafe {
        fw.current = p.CurrentProfileTypes().ok()?;
        for (i, prof) in FwProfile::ALL.iter().enumerate() {
            fw.enabled[i] = p.get_FirewallEnabled(NET_FW_PROFILE_TYPE2(prof.bit())).ok().map(|b| b.as_bool());
        }
    }
    Some(fw)
}

/// (displayName, productState) rows of a SecurityCenter2 class; `None` when the namespace is
/// missing (Windows Server) or WMI fails.
fn security_center(class: &str) -> Option<Vec<(String, i64)>> {
    let rows = wmi::query_timeout(r"ROOT\SecurityCenter2", &format!("SELECT displayName, productState FROM {class}"), &["displayName", "productState"], Duration::from_secs(5)).ok()?;
    Some(
        rows.iter()
            .filter_map(|r| Some((r.get("displayName")?.as_str()?.to_string(), r.get("productState")?.as_f64()? as i64)))
            .take(16)
            .collect(),
    )
}

fn guest_read() -> Option<bool> {
    let rows = wmi::query_timeout(r"ROOT\CIMV2", "SELECT SID, Disabled FROM Win32_UserAccount WHERE LocalAccount = TRUE", &["SID", "Disabled"], Duration::from_secs(5)).ok()?;
    let rows: Vec<(String, bool)> = rows
        .iter()
        .filter_map(|r| Some((r.get("SID")?.as_str()?.to_string(), matches!(r.get("Disabled"), Some(wmi::WmiValue::Bool(true))))))
        .collect();
    guest_enabled(&rows)
}

fn sessions_read() -> Vec<SessionRow> {
    let mut out = Vec::new();
    let server = Some(WTS_CURRENT_SERVER_HANDLE);
    let mut list: *mut WTS_SESSION_INFOW = std::ptr::null_mut();
    let mut n = 0u32;
    // SAFETY: WTS allocates the array; freed with WTSFreeMemory below. Count is clamped.
    unsafe {
        if WTSEnumerateSessionsW(server, 0, 1, &mut list, &mut n).is_err() || list.is_null() {
            return out;
        }
        for s in std::slice::from_raw_parts(list, n.min(256) as usize) {
            let query = |class| -> Option<Vec<u8>> {
                let mut buf = PWSTR::null();
                let mut bytes = 0u32;
                WTSQuerySessionInformationW(server, s.SessionId, class, &mut buf, &mut bytes).ok()?;
                if buf.is_null() {
                    return None;
                }
                let v = std::slice::from_raw_parts(buf.0.cast::<u8>(), bytes.min(4096) as usize).to_vec();
                WTSFreeMemory(buf.0.cast());
                Some(v)
            };
            let protocol = query(WTSClientProtocolType).filter(|b| b.len() >= 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
            let client = query(WTSClientName).map(|b| registry::utf16z(&b)).unwrap_or_default();
            out.push(SessionRow { id: s.SessionId, protocol, state: s.State.0, client: clean(&client) });
        }
        WTSFreeMemory(list.cast());
    }
    out
}

/// Own/shared-process services (no drivers): (key, display name, start, exe path).
fn services_read() -> Vec<(String, String, u32, String)> {
    registry::subkeys(Root::Hklm, SERVICES)
        .unwrap_or_default()
        .into_iter()
        .take(4000)
        .filter_map(|svc| {
            let path = format!(r"{SERVICES}\{svc}");
            let ty = dw(Root::Hklm, &path, "Type")?;
            if ty & 0x30 == 0 {
                return None;
            }
            let start = dw(Root::Hklm, &path, "Start").unwrap_or(3);
            let image = match registry::read(Root::Hklm, &path, "ImagePath") {
                Ok(Value::Str(s)) => apps::service_exe(&apps::expand_env(&s)),
                _ => String::new(),
            };
            let display = match registry::read(Root::Hklm, &path, "DisplayName") {
                Ok(Value::Str(s)) if !s.starts_with('@') => clean(&s),
                _ => String::new(),
            };
            Some((svc, display, start, image))
        })
        .collect()
}

/// This PC's addresses (every adapter), for the UPnP filter.
fn my_ips() -> HashSet<IpAddr> {
    dns::adapters().iter().filter_map(|a| a.ipv4).map(IpAddr::V4).collect()
}

/* ---- UPnP on a worker thread: the router may never answer ---- */

static UPNP_BUSY: AtomicBool = AtomicBool::new(false);
type UpnpResult = Result<Vec<Mapping>, &'static str>;
static UPNP_LAST: Mutex<Option<(Instant, Vec<Mapping>)>> = Mutex::new(None);

/// Run `f` (which talks to the router) on its own COM thread and give up after `timeout`.
/// Only one router call at a time: a call still waiting = `busy`.
fn router_job<T: Send + 'static>(timeout: Duration, f: impl FnOnce() -> Result<T, &'static str> + Send + 'static) -> Result<T, &'static str> {
    if UPNP_BUSY.swap(true, Ordering::SeqCst) {
        return Err("busy");
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let r = match Com::init() {
            Ok(_com) => f(),
            Err(_) => Err("com"),
        };
        UPNP_BUSY.store(false, Ordering::SeqCst);
        let _ = tx.send(r);
    });
    rx.recv_timeout(timeout).unwrap_or(Err("timeout"))
}

fn upnp_collection() -> Result<windows::Win32::NetworkManagement::WindowsFirewall::IStaticPortMappingCollection, &'static str> {
    // SAFETY: in-proc NAT helper object; a router without UPnP gives an error / null collection.
    unsafe {
        let nat: IUPnPNAT = CoCreateInstance(&UPnPNAT, None, CLSCTX_INPROC_SERVER).map_err(|_| "com")?;
        nat.StaticPortMappingCollection().map_err(|_| "no_router")
    }
}

fn upnp_enumerate() -> UpnpResult {
    let coll = upnp_collection()?;
    let mut out = Vec::new();
    // SAFETY: standard IEnumVARIANT walk; each VARIANT is cleared on drop; at most 512 entries.
    unsafe {
        let en: IEnumVARIANT = coll._NewEnum().and_then(|u| u.cast()).map_err(|_| "no_router")?;
        for _ in 0..512 {
            let mut v = [VARIANT::default()];
            let mut got = 0u32;
            if en.Next(&mut v, &mut got).is_err() || got == 0 {
                break;
            }
            let inner = &v[0].Anonymous.Anonymous;
            if inner.vt != VT_DISPATCH {
                continue;
            }
            let Some(m) = (*inner.Anonymous.pdispVal).as_ref().and_then(|d| d.cast::<IStaticPortMapping>().ok()) else {
                continue;
            };
            let s = |r: windows::core::Result<BSTR>| r.map(|b| clean(&b.to_string())).unwrap_or_default();
            out.push(Mapping {
                protocol: s(m.Protocol()).to_ascii_uppercase(),
                external_port: m.ExternalPort().ok().and_then(|p| u16::try_from(p).ok()).unwrap_or(0),
                internal_port: m.InternalPort().ok().and_then(|p| u16::try_from(p).ok()).unwrap_or(0),
                internal_client: s(m.InternalClient()),
                enabled: m.Enabled().is_ok_and(|b| b.as_bool()),
                description: s(m.Description()),
            });
        }
    }
    Ok(out)
}

/// Router mappings (5 s budget); the last good answer is cached for the quick report.
pub fn upnp_mappings() -> UpnpResult {
    let r = router_job(Duration::from_secs(5), upnp_enumerate);
    if let Ok(m) = &r {
        *UPNP_LAST.lock().unwrap() = Some((Instant::now(), m.clone()));
    }
    r
}

fn upnp_cached() -> UpnpResult {
    match &*UPNP_LAST.lock().unwrap() {
        Some((at, m)) if at.elapsed() < Duration::from_secs(15 * 60) => Ok(m.clone()),
        _ => Err("not_checked"),
    }
}

/* ---- signatures (cached per path, checked in parallel) ---- */

static SIGNED: Mutex<Option<HashMap<String, Option<bool>>>> = Mutex::new(None);

fn verify_file(path: &str) -> Option<bool> {
    let w = wide(path);
    let mut file = WINTRUST_FILE_INFO { cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32, pcwszFilePath: PCWSTR(w.as_ptr()), ..Default::default() };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..Default::default()
    };
    let mut action: GUID = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: `file`/`w` outlive both calls; the state handle is released by the CLOSE call.
    let code = unsafe {
        let code = WinVerifyTrust(HWND::default(), &mut action, (&mut data as *mut WINTRUST_DATA).cast());
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(HWND::default(), &mut action, (&mut data as *mut WINTRUST_DATA).cast());
        code
    };
    // Many Windows programs outside %windir% are signed through a system catalog, not inside the file.
    if code as u32 == 0x800B_0100 && in_catalog(&w) {
        return Some(true);
    }
    signature_from_code(code)
}

/// Is the file's Authenticode hash listed in an installed system catalog (CatRoot, admin-only to add)?
fn in_catalog(w: &[u16]) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::Security::Cryptography::Catalog::{
        CryptCATAdminAcquireContext2, CryptCATAdminCalcHashFromFileHandle2, CryptCATAdminEnumCatalogFromHash,
        CryptCATAdminReleaseCatalogContext, CryptCATAdminReleaseContext,
    };
    use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, OPEN_EXISTING};
    const GENERIC_READ: u32 = 0x8000_0000;
    // SAFETY: `w` is NUL-terminated; every handle/context acquired here is released here.
    unsafe {
        let Ok(file) = CreateFileW(PCWSTR(w.as_ptr()), GENERIC_READ, FILE_SHARE_READ, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) else {
            return false;
        };
        let mut found = false;
        for alg in [windows::core::w!("SHA256"), PCWSTR::null()] {
            let mut admin = 0isize;
            if found || CryptCATAdminAcquireContext2(&mut admin, None, alg, None, None).is_err() {
                continue;
            }
            let mut len = 0u32;
            let _ = CryptCATAdminCalcHashFromFileHandle2(admin, file, &mut len, None, None);
            let mut hash = vec![0u8; len.min(64) as usize];
            if !hash.is_empty() && CryptCATAdminCalcHashFromFileHandle2(admin, file, &mut len, Some(hash.as_mut_ptr()), None).is_ok() {
                let cat = CryptCATAdminEnumCatalogFromHash(admin, &hash[..(len as usize).min(hash.len())], None, None);
                if cat != 0 {
                    found = true;
                    let _ = CryptCATAdminReleaseCatalogContext(admin, cat, 0);
                }
            }
            let _ = CryptCATAdminReleaseContext(admin, 0);
        }
        let _ = CloseHandle(file);
        found
    }
}

fn signatures(paths: &[String]) -> HashMap<String, Option<bool>> {
    let mut cache = SIGNED.lock().unwrap().take().unwrap_or_default();
    let todo: Vec<&String> = paths.iter().filter(|p| !cache.contains_key(*p)).collect();
    let found: Vec<(String, Option<bool>)> = todo
        .par_iter()
        .map(|p| ((*p).clone(), if Path::new(p.as_str()).is_file() { verify_file(p) } else { None }))
        .collect();
    cache.extend(found);
    if cache.len() > 5000 {
        cache.clear();
    }
    let out = paths.iter().filter_map(|p| cache.get(p).map(|v| (p.clone(), *v))).collect();
    *SIGNED.lock().unwrap() = Some(cache);
    out
}

/* ---- scheduled tasks (Task Scheduler COM, never schtasks.exe) ---- */

struct TaskExec {
    path: String,
    exe: String,
    args: String,
}

fn bstr_out(f: impl FnOnce(*mut BSTR) -> windows::core::Result<()>) -> String {
    let mut b = BSTR::new();
    if f(&mut b).is_err() {
        return String::new();
    }
    b.to_string()
}

fn walk_tasks(folder: &ITaskFolder, out: &mut Vec<TaskExec>, depth: u32) {
    // SAFETY: COM calls on valid objects; indexes are 1-based and bounded.
    unsafe {
        if depth > 8 || out.len() > 2000 {
            return;
        }
        if folder.Path().is_ok_and(|p| p.to_string().eq_ignore_ascii_case(r"\Microsoft")) {
            return; // Windows' own tasks
        }
        if let Ok(tasks) = folder.GetTasks(TASK_ENUM_HIDDEN.0) {
            for i in 1..=tasks.Count().unwrap_or(0).clamp(0, 1000) {
                let Ok(t) = tasks.get_Item(&VARIANT::from(i)) else { continue };
                if !t.Enabled().is_ok_and(|e| e.as_bool()) {
                    continue;
                }
                let path = t.Path().map(|p| p.to_string()).unwrap_or_default();
                let Ok(actions) = t.Definition().and_then(|d| d.Actions()) else { continue };
                let mut n = 0i32;
                if actions.Count(&mut n).is_err() {
                    continue;
                }
                for j in 1..=n.clamp(0, 32) {
                    let Ok(exec) = actions.get_Item(j).and_then(|a| a.cast::<IExecAction>()) else { continue };
                    let exe = bstr_out(|b| exec.Path(b));
                    if exe.is_empty() {
                        continue;
                    }
                    let args = bstr_out(|b| exec.Arguments(b));
                    out.push(TaskExec { path: clean(&path), exe: apps::expand_env(exe.trim().trim_matches('"')), args: apps::expand_env(&args) });
                }
            }
        }
        if let Ok(subs) = folder.GetFolders(0) {
            for i in 1..=subs.Count().unwrap_or(0).clamp(0, 500) {
                if let Ok(f) = subs.get_Item(&VARIANT::from(i)) {
                    walk_tasks(&f, out, depth + 1);
                }
            }
        }
    }
}

fn tasks_read() -> Vec<TaskExec> {
    let Ok(_com) = Com::init() else { return Vec::new() };
    let mut out = Vec::new();
    // SAFETY: local Task Scheduler, current user; objects released on drop.
    unsafe {
        let Ok(svc) = CoCreateInstance::<_, ITaskService>(&TaskScheduler, None, CLSCTX_INPROC_SERVER) else { return out };
        let none = VARIANT::default();
        if svc.Connect(&none, &none, &none, &none).is_err() {
            return out;
        }
        if let Ok(root) = svc.GetFolder(&BSTR::from(r"\")) {
            walk_tasks(&root, &mut out, 0);
        }
    }
    out
}

/// Tasks the last full report flagged (only those can be disabled from the UI) and every path
/// it showed (only those can be revealed in Explorer).
static LAST_TASKS: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static LAST_PATHS: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn persistence(services: &[(String, String, u32, String)], d: &Dirs) -> Vec<Item> {
    let tasks = tasks_read();
    // (name, source, exe, args, task path)
    let mut cands: Vec<(String, &'static str, String, String, Option<String>)> = Vec::new();
    for t in tasks {
        cands.push((t.path.clone(), "task", t.exe, t.args, Some(t.path)));
    }
    for (key, display, start, exe) in services {
        if *start == 4 || exe.is_empty() {
            continue;
        }
        cands.push((if display.is_empty() { key.clone() } else { display.clone() }, "service", exe.clone(), String::new(), None));
    }
    // Only programs outside %windir% need a signature check.
    let paths: Vec<String> = cands
        .iter()
        .map(|c| c.2.clone())
        .filter(|p| { let l = p.to_ascii_lowercase(); !under(&l, &d.windir) || user_folder(&l, d).is_some() })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let signed = signatures(&paths);
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (name, source, exe, args, task) in cands {
        let Some((sev, reason)) = persistence_verdict(&exe.to_ascii_lowercase(), &args.to_ascii_lowercase(), signed.get(&exe).copied().flatten(), d) else {
            continue;
        };
        if !seen.insert((source, name.to_ascii_lowercase())) {
            continue;
        }
        let mut it = Item::new(name, sev);
        it.path = Some(clean(&exe));
        it.source = Some(source);
        it.reason = Some(reason);
        it.fix = task.map(|path| Fix::TaskDisable { path });
        out.push(it);
    }
    out.sort_by(|a, b| b.severity.cmp(&a.severity).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out.truncate(100);
    out
}

fn winrm_check(conns: &[Conn]) -> Check {
    let svc = boost::svc_query("WinRM").ok().flatten();
    let listening = conns.iter().any(|c| {
        c.protocol == Protocol::Tcp && c.state == Some(TcpState::Listen) && matches!(c.local_port, 5985 | 5986) && netconns::remote_scope(c.local_addr) != Scope::Loopback
    });
    let running = svc.is_some_and(|(_, r)| r);
    let start = match svc.map(|(s, _)| s) {
        Some(2) => "auto",
        Some(4) => "disabled",
        Some(_) => "manual",
        None => "absent",
    };
    if !running && !listening {
        return Check::ok("winrm").detail("start", start);
    }
    let mut c = Check::exposed("winrm", if listening { Severity::Medium } else { Severity::Low }).detail("start", start);
    if listening {
        c = c.detail("listening", "1");
    }
    if running {
        c.fix = Some(Fix::WinrmStop);
    }
    c
}

/* ---------------- reports ---------------- */

fn ms(t: Instant) -> u64 {
    t.elapsed().as_millis() as u64
}

/// Everything: ~1–5 s (router answer, signature checks the first time). No admin needed.
pub fn report() -> Report {
    let t0 = Instant::now();
    let upnp = std::thread::spawn(upnp_mappings);
    let d = Dirs::live();
    let conns = netconns::connections();
    let services = services_read();
    let mut checks = base_checks(&conns);
    checks.push(Check::from_items(
        "remote_apps",
        remote_apps(&RemoteRaw {
            processes: system::running_process_paths(),
            services: services.clone(),
            installed: apps::desktop_list(&[]).into_iter().map(|a| a.name).collect(),
        }),
    ));
    checks.push(Check::from_items("listeners", exposed_listeners(&conns, &d.windir)));
    checks.push(Check::from_items("sessions", remote_sessions(&sessions_read())));
    checks.push(Check::from_items("persistence", persistence(&services, &d)));
    checks.push(match guest_read() {
        Some(true) => Check::exposed("guest", Severity::Medium),
        Some(false) => Check::ok("guest"),
        None => Check::unknown("guest", "unreadable"),
    });
    let upnp = upnp.join().unwrap_or(Err("timeout"));
    checks.push(upnp_check(&upnp, &my_ips()));
    remember(&checks);
    finish(checks, t0)
}

/// The cheap subset the advisor (Painel, Luli, MCP) uses: registry, firewall, antivirus, inbound
/// connections, and the router mappings from the last full report or monitor run (if recent).
pub fn quick() -> Report {
    let t0 = Instant::now();
    let conns = netconns::connections();
    let mut checks = base_checks(&conns);
    checks.push(upnp_check(&upnp_cached(), &my_ips()));
    finish(checks, t0)
}

fn base_checks(conns: &[Conn]) -> Vec<Check> {
    let (rdp, nla) = rdp_checks(rdp_raw());
    let (smb1s, smb1c) = smb1_checks(
        dw(Root::Hklm, &format!(r"{SERVICES}\srv"), "Start"),
        dw(Root::Hklm, SMB_SERVER, "SMB1"),
        dw(Root::Hklm, &format!(r"{SERVICES}\mrxsmb10"), "Start"),
    );
    let third_party_fw = security_center("FirewallProduct")
        .unwrap_or_default()
        .into_iter()
        .find(|(_, st)| product_state(*st).0 == "on")
        .map(|(n, _)| n);
    vec![
        rdp,
        nla,
        remote_assistance_check(dw(Root::Hklm, RA, "fAllowToGetHelp"), dw(Root::Hklm, TS_POLICY, "fAllowToGetHelp")),
        winrm_check(conns),
        smb1s,
        smb1c,
        firewall_check(firewall_read(), third_party_fw.as_deref()),
        antivirus_check(security_center("AntiVirusProduct")),
        Check::from_items("inbound", inbound_public(conns)),
    ]
}

fn finish(checks: Vec<Check>, t0: Instant) -> Report {
    Report { checks, elevated: system::is_elevated(), reverts: reverts(), took_ms: ms(t0) }
}

fn remember(checks: &[Check]) {
    let mut tasks = HashSet::new();
    let mut paths = HashSet::new();
    for it in checks.iter().flat_map(|c| &c.items) {
        if let Some(Fix::TaskDisable { path }) = &it.fix {
            tasks.insert(path.to_ascii_lowercase());
        }
        if let Some(p) = &it.path {
            paths.insert(p.to_ascii_lowercase());
        }
    }
    *LAST_TASKS.lock().unwrap() = Some(tasks);
    *LAST_PATHS.lock().unwrap() = Some(paths);
}

/// For the MCP client: no profile paths, no remote IPs or client computer names.
pub fn redacted(mut r: Report) -> Report {
    for c in &mut r.checks {
        for it in &mut c.items {
            it.path = it.path.as_deref().map(crate::winevents::redact_here);
            if matches!(c.id, "inbound" | "sessions") {
                it.remote = None;
                if c.id == "sessions" {
                    it.name = "rdp".into();
                }
            }
        }
    }
    r
}

/// Open Explorer on a program the last report showed (nothing else).
pub fn reveal(path: &str) -> Result<(), ExposureError> {
    let known = LAST_PATHS.lock().unwrap().as_ref().is_some_and(|s| s.contains(&path.to_ascii_lowercase()));
    if !known {
        return Err(ExposureError::NotApplicable);
    }
    crate::disk::reveal(Path::new(path)).map_err(|e| ExposureError::Other(e.to_string()))
}

/* ---------------- advisor ---------------- */

/// Advisor findings from an exposure report (ids `rdp_open`, `nla_off`, `firewall_off`, `smb1_on`,
/// `upnp_open`, `remote_inbound`, `av_off`; texts in i18n `advisor.<id>.*`).
pub fn advisor_findings(r: &Report) -> Vec<advisor::Finding> {
    let sev = |s: Severity| match s {
        Severity::High => advisor::Severity::Critical,
        Severity::Medium => advisor::Severity::Warn,
        _ => advisor::Severity::Info,
    };
    let mut out = Vec::new();
    for c in r.checks.iter().filter(|c| c.state == State::Exposed) {
        let (id, params): (&'static str, Vec<(&'static str, String)>) = match c.id {
            "rdp" => ("rdp_open", vec![("port", c.details.get("port").cloned().unwrap_or_default())]),
            "nla" => ("nla_off", vec![]),
            "firewall" if c.severity >= Severity::Medium => ("firewall_off", vec![("count", c.items.len().to_string())]),
            "smb1_server" => ("smb1_on", vec![]),
            "upnp" => ("upnp_open", vec![("count", c.items.len().to_string())]),
            "inbound" => ("remote_inbound", vec![("count", c.items.len().to_string())]),
            "antivirus" => ("av_off", vec![]),
            _ => continue,
        };
        out.push(advisor::Finding { id, severity: sev(c.severity), kind: advisor::Kind::Security, params: params.into_iter().collect() });
    }
    out
}

/* ---------------- fixes ---------------- */

fn reg_entry(path: &str, name: &str) -> Result<BackupEntry, ExposureError> {
    Ok(BackupEntry { root: Root::Hklm, path: path.into(), name: name.into(), value: registry::read(Root::Hklm, path, name)? })
}

fn pseudo(path: &str, name: &str, value: Value) -> BackupEntry {
    BackupEntry { root: Root::Hklm, path: path.into(), name: name.into(), value }
}

/// Save the original state once (a second apply keeps the first, real original).
fn save_backup(fix: &Fix, mut entries: Vec<BackupEntry>) -> Result<(), ExposureError> {
    let id = fix.backup_id();
    if registry::load_backup(&id).is_some() {
        return Ok(());
    }
    let json = serde_json::to_string(fix).map_err(|e| ExposureError::Other(e.to_string()))?;
    entries.push(pseudo("fix", "json", Value::Str(json)));
    registry::save_backup(&Backup { tweak: id, taken_at: registry::now_unix(), entries }).map_err(|e| ExposureError::Other(e.to_string()))
}

/// Fixes that have a backup on disk.
pub fn reverts() -> Vec<Fix> {
    let dir = registry::data_dir().join("backups");
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    rd.flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".json").filter(|n| n.starts_with("exposure_")).map(str::to_string))
        .filter_map(|id| {
            let b = registry::load_backup(&id)?;
            let fix = b.entries.iter().find(|e| e.path == "fix").and_then(|e| match &e.value {
                Value::Str(s) => serde_json::from_str::<Fix>(s).ok(),
                _ => None,
            })?;
            (fix.backup_id() == id).then_some(fix)
        })
        .take(100)
        .collect()
}

struct Sc(SC_HANDLE);

impl Drop for Sc {
    fn drop(&mut self) {
        // SAFETY: handle from OpenSCManagerW/OpenServiceW, closed once.
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

fn open_service(name: &str, access: u32) -> Result<Sc, ExposureError> {
    let w = wide(name);
    // SAFETY: local SCM, valid null-terminated name; handles closed by `Sc`.
    unsafe {
        let scm = Sc(OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT).map_err(win)?);
        OpenServiceW(scm.0, PCWSTR(w.as_ptr()), access).map(Sc).map_err(win)
    }
}

fn service_set_start(name: &str, start: u32) -> Result<(), ExposureError> {
    let h = open_service(name, SERVICE_CHANGE_CONFIG)?;
    // SAFETY: SERVICE_NO_CHANGE keeps every other field; null strings mean "unchanged".
    unsafe {
        ChangeServiceConfigW(
            h.0,
            ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
            SERVICE_START_TYPE(start),
            SERVICE_ERROR(SERVICE_NO_CHANGE),
            PCWSTR::null(),
            PCWSTR::null(),
            None,
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
        )
    }
    .map_err(win)
}

fn service_stop(name: &str) -> Result<(), ExposureError> {
    let h = open_service(name, SERVICE_STOP)?;
    let mut st = SERVICE_STATUS::default();
    // SAFETY: valid handle; plain status out-param.
    match unsafe { ControlService(h.0, SERVICE_CONTROL_STOP, &mut st) } {
        Ok(()) => Ok(()),
        // 1062 not active, 1052 control not accepted (already stopping).
        Err(e) if matches!(e.code().0 as u32, 0x8007_0426 | 0x8007_041C) => Ok(()),
        Err(e) => Err(win(e)),
    }
}

fn service_start(name: &str) -> Result<(), ExposureError> {
    let h = open_service(name, SERVICE_START)?;
    // SAFETY: valid handle, no arguments.
    match unsafe { StartServiceW(h.0, None) } {
        Ok(()) => Ok(()),
        Err(e) if e.code().0 as u32 == 0x8007_0420 => Ok(()), // already running
        Err(e) => Err(win(e)),
    }
}

fn firewall_set(profile: FwProfile, on: bool) -> Result<(), ExposureError> {
    let _com = Com::init().map_err(|e| ExposureError::Other(e.to_string()))?;
    let p = firewall_policy().map_err(win)?;
    // SAFETY: valid policy object; profile is one of the three fixed bits.
    unsafe { p.put_FirewallEnabled(NET_FW_PROFILE_TYPE2(profile.bit()), if on { VARIANT_TRUE } else { VARIANT_FALSE }) }.map_err(win)
}

fn split_task(path: &str) -> (String, String) {
    match path.rsplit_once('\\') {
        Some(("", name)) => (r"\".into(), name.into()),
        Some((folder, name)) => (folder.into(), name.into()),
        None => (r"\".into(), path.into()),
    }
}

fn task_err(e: tasksched::TaskError) -> ExposureError {
    match e {
        tasksched::TaskError::NeedsAdmin => ExposureError::NeedsAdmin,
        tasksched::TaskError::NotFound => ExposureError::NotApplicable,
        e => ExposureError::Other(e.to_string()),
    }
}

fn require_admin(fix: &Fix) -> Result<(), ExposureError> {
    if fix.needs_admin() && !system::is_elevated() { Err(ExposureError::NeedsAdmin) } else { Ok(()) }
}

fn upnp_protocol(p: &str) -> Result<&'static str, ExposureError> {
    match p.to_ascii_uppercase().as_str() {
        "TCP" => Ok("TCP"),
        "UDP" => Ok("UDP"),
        _ => Err(ExposureError::NotApplicable),
    }
}

/// Apply one fix after re-validating it against the live state; the original is backed up first.
pub fn apply(fix: &Fix) -> Result<(), ExposureError> {
    require_admin(fix)?;
    match fix {
        Fix::RdpDisable => {
            let (rdp, _) = rdp_checks(rdp_raw());
            if rdp.details.contains_key("policy") {
                return Err(ExposureError::Policy);
            }
            save_backup(fix, vec![reg_entry(TS, "fDenyTSConnections")?])?;
            registry::write(Root::Hklm, TS, "fDenyTSConnections", &Value::Dword(1))?;
        }
        Fix::NlaEnable => {
            if dw(Root::Hklm, TS_POLICY, "UserAuthentication").is_some() {
                return Err(ExposureError::Policy);
            }
            save_backup(fix, vec![reg_entry(RDP_TCP, "UserAuthentication")?])?;
            registry::write(Root::Hklm, RDP_TCP, "UserAuthentication", &Value::Dword(1))?;
        }
        Fix::RaDisable => {
            if dw(Root::Hklm, TS_POLICY, "fAllowToGetHelp").is_some() {
                return Err(ExposureError::Policy);
            }
            save_backup(fix, vec![reg_entry(RA, "fAllowToGetHelp")?])?;
            registry::write(Root::Hklm, RA, "fAllowToGetHelp", &Value::Dword(0))?;
        }
        Fix::Smb1ServerDisable => {
            save_backup(fix, vec![reg_entry(SMB_SERVER, "SMB1")?])?;
            registry::write(Root::Hklm, SMB_SERVER, "SMB1", &Value::Dword(0))?;
        }
        Fix::FirewallOn { profile } => {
            let i = FwProfile::ALL.iter().position(|p| p == profile).unwrap_or(0);
            let was = firewall_read().and_then(|f| f.enabled[i]).ok_or(ExposureError::NotApplicable)?;
            if was {
                return Ok(());
            }
            save_backup(fix, vec![pseudo("fw", profile.tag(), Value::Dword(0))])?;
            firewall_set(*profile, true)?;
        }
        Fix::UpnpRemove { protocol, external_port } => {
            let proto = upnp_protocol(protocol)?;
            let all = upnp_mappings().map_err(|_| ExposureError::Router)?;
            let m = mappings_to_me(&all, &my_ips())
                .into_iter()
                .find(|m| m.protocol == proto && m.external_port == *external_port)
                .ok_or(ExposureError::NotApplicable)?;
            let json = serde_json::to_string(&m).map_err(|e| ExposureError::Other(e.to_string()))?;
            save_backup(fix, vec![pseudo("upnp", "mapping", Value::Str(json))])?;
            let ext = i32::from(*external_port);
            router_job(Duration::from_secs(8), move || {
                let coll = upnp_collection()?;
                // SAFETY: valid collection; port/protocol validated above.
                unsafe { coll.Remove(ext, &BSTR::from(proto)) }.map_err(|_| "remove_failed")
            })
            .map_err(|_| ExposureError::Router)?;
        }
        Fix::WinrmStop => {
            let (start, running) = boost::svc_query("WinRM").ok().flatten().ok_or(ExposureError::NotApplicable)?;
            save_backup(fix, vec![pseudo("svc:WinRM", "start", Value::Dword(start)), pseudo("svc:WinRM", "running", Value::Dword(u32::from(running)))])?;
            if start == 2 {
                service_set_start("WinRM", 3)?;
            }
            service_stop("WinRM")?;
        }
        Fix::TaskDisable { path } => {
            let flagged = LAST_TASKS.lock().unwrap().as_ref().is_some_and(|s| s.contains(&path.to_ascii_lowercase()));
            if !flagged || path.len() > 400 || path.chars().any(char::is_control) {
                return Err(ExposureError::NotApplicable);
            }
            let (folder, name) = split_task(path);
            if tasksched::task_enabled(&folder, &name).map_err(task_err)? != Some(true) {
                return Err(ExposureError::NotApplicable);
            }
            save_backup(fix, vec![pseudo("task", path, Value::Dword(1))])?;
            tasksched::set_task_enabled(&folder, &name, false).map_err(task_err)?;
        }
    }
    Ok(())
}

/// Put back exactly what the backup holds, then drop the backup.
pub fn revert(fix: &Fix) -> Result<(), ExposureError> {
    require_admin(fix)?;
    let id = fix.backup_id();
    let b = registry::load_backup(&id).ok_or(ExposureError::NoBackup)?;
    // The backup file is hostile input too: only the entries this fix could have written count.
    for e in &b.entries {
        match (fix, e.path.as_str()) {
            (_, "fix") => {}
            (Fix::RdpDisable, TS) | (Fix::NlaEnable, RDP_TCP) | (Fix::RaDisable, RA) | (Fix::Smb1ServerDisable, SMB_SERVER) => {
                let expected = match fix {
                    Fix::RdpDisable => "fDenyTSConnections",
                    Fix::NlaEnable => "UserAuthentication",
                    Fix::RaDisable => "fAllowToGetHelp",
                    _ => "SMB1",
                };
                if e.name == expected && matches!(e.value, Value::Dword(_) | Value::Absent) {
                    registry::write(Root::Hklm, &e.path, &e.name, &e.value)?;
                }
            }
            (Fix::FirewallOn { profile }, "fw") if e.name == profile.tag() => firewall_set(*profile, e.value == Value::Dword(1))?,
            (Fix::UpnpRemove { .. }, "upnp") => {
                let Value::Str(s) = &e.value else { continue };
                let m: Mapping = serde_json::from_str(s).map_err(|_| ExposureError::NoBackup)?;
                let proto = upnp_protocol(&m.protocol)?;
                if !m.internal_client.parse::<IpAddr>().is_ok_and(|ip| my_ips().contains(&ip)) || m.internal_port == 0 {
                    return Err(ExposureError::NotApplicable);
                }
                router_job(Duration::from_secs(8), move || {
                    let coll = upnp_collection()?;
                    // SAFETY: valid collection; values validated above.
                    unsafe {
                        coll.Add(
                            i32::from(m.external_port),
                            &BSTR::from(proto),
                            i32::from(m.internal_port),
                            &BSTR::from(m.internal_client.as_str()),
                            if m.enabled { VARIANT_TRUE } else { VARIANT_FALSE },
                            &BSTR::from(m.description.as_str()),
                        )
                    }
                    .map(|_| ())
                    .map_err(|_| "add_failed")
                })
                .map_err(|_| ExposureError::Router)?;
            }
            (Fix::WinrmStop, "svc:WinRM") => match (e.name.as_str(), &e.value) {
                ("start", Value::Dword(s)) if (2..=4).contains(s) => service_set_start("WinRM", *s)?,
                ("running", Value::Dword(1)) => service_start("WinRM")?,
                _ => {}
            },
            (Fix::TaskDisable { path }, "task") if e.name.eq_ignore_ascii_case(path) => {
                let (folder, name) = split_task(path);
                tasksched::set_task_enabled(&folder, &name, true).map_err(task_err)?;
            }
            _ => {}
        }
    }
    registry::delete_backup(&id);
    Ok(())
}

/* ---------------- monitor ---------------- */

/// The cheap subset the routine thread checks every few minutes.
fn observe() -> Vec<Observed> {
    let d = Dirs::live();
    let conns = netconns::connections();
    let mut out = Vec::new();
    for it in exposed_listeners(&conns, &d.windir) {
        out.push(Observed { kind: "listener", key: format!("listener:{}", it.name.to_ascii_lowercase()), name: it.name, port: it.port });
    }
    for it in inbound_public(&conns) {
        out.push(Observed { kind: "inbound", key: format!("inbound:{}", it.name.to_ascii_lowercase()), name: it.name, port: it.port });
    }
    for it in remote_apps(&RemoteRaw { processes: system::running_process_paths(), ..Default::default() }) {
        out.push(Observed { kind: "remote_app", key: format!("remote_app:{}", it.name.to_ascii_lowercase()), name: it.name, port: None });
    }
    if let Ok(all) = upnp_mappings() {
        for m in mappings_to_me(&all, &my_ips()) {
            out.push(Observed { kind: "upnp", key: format!("upnp:{}:{}", m.protocol, m.external_port), name: if m.description.is_empty() { m.protocol.clone() } else { m.description.clone() }, port: Some(m.external_port) });
        }
    }
    out
}

fn baseline_file() -> PathBuf {
    registry::data_dir().join("exposure_baseline.json")
}

/// One monitor pass: what is new since the baseline (nothing on the very first pass, which only
/// records the baseline). The baseline only grows, so each thing alerts once.
pub fn monitor_tick() -> Vec<Alert> {
    let now = observe();
    let file = baseline_file();
    let base = std::fs::read_to_string(&file).ok().and_then(|t| parse_baseline(&t));
    let first = base.is_none();
    let mut base = base.unwrap_or_default();
    let at = registry::now_unix();
    let fresh: Vec<Alert> = if first {
        Vec::new()
    } else {
        diff(&base, &now).into_iter().map(|o| Alert { kind: o.kind, name: o.name.clone(), port: o.port, at }).collect()
    };
    base.extend(now.into_iter().map(|o| o.key));
    while base.len() > BASELINE_CAP {
        base.pop_first();
    }
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
        let tmp = file.with_extension("json.tmp");
        let list: Vec<&String> = base.iter().collect();
        if std::fs::write(&tmp, serde_json::to_string(&list).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, &file);
        }
    }
    fresh
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ids(c: &Check) -> (&'static str, State, Severity) {
        (c.id, c.state, c.severity)
    }

    #[test]
    fn rdp_and_nla() {
        let (rdp, nla) = rdp_checks(RdpRaw::default());
        assert_eq!((rdp.state, nla.state), (State::Ok, State::Ok), "absent = denied");
        let (rdp, nla) = rdp_checks(RdpRaw { deny: Some(0), ..Default::default() });
        assert_eq!(ids(&rdp), ("rdp", State::Exposed, Severity::Medium));
        assert_eq!(rdp.fix, Some(Fix::RdpDisable));
        assert_eq!(nla.state, State::Ok, "absent UserAuthentication = NLA on");
        let (rdp, nla) = rdp_checks(RdpRaw { deny: Some(0), nla: Some(0), port: Some(50000), ..Default::default() });
        assert_eq!(rdp.severity, Severity::High);
        assert_eq!(rdp.details.get("port").map(String::as_str), Some("50000"));
        assert!(rdp.details.contains_key("custom_port"));
        assert_eq!((nla.state, nla.fix.clone()), (State::Exposed, Some(Fix::NlaEnable)));
        // Policy wins over the local value, and then there is no local fix.
        let (rdp, _) = rdp_checks(RdpRaw { deny: Some(1), deny_policy: Some(0), ..Default::default() });
        assert_eq!((rdp.state, rdp.fix), (State::Exposed, None));
        let (rdp, _) = rdp_checks(RdpRaw { deny: Some(0), deny_policy: Some(1), ..Default::default() });
        assert_eq!(rdp.state, State::Ok);
        let (_, nla) = rdp_checks(RdpRaw { deny: Some(0), nla: Some(0), nla_policy: Some(0), ..Default::default() });
        assert_eq!((nla.state, nla.fix), (State::Exposed, None));
        let (rdp, _) = rdp_checks(RdpRaw { deny: Some(0), port: Some(0), ..Default::default() });
        assert_eq!(rdp.details.get("port").map(String::as_str), Some("3389"), "invalid port falls back");
    }

    #[test]
    fn remote_assistance_and_smb1() {
        assert_eq!(remote_assistance_check(None, None).state, State::Ok);
        let c = remote_assistance_check(Some(1), None);
        assert_eq!((c.state, c.severity, c.fix), (State::Exposed, Severity::Low, Some(Fix::RaDisable)));
        assert_eq!(remote_assistance_check(Some(1), Some(0)).state, State::Ok);
        assert_eq!(remote_assistance_check(Some(0), Some(1)).fix, None);

        let (s, c) = smb1_checks(None, None, None);
        assert_eq!((s.state, c.state), (State::Ok, State::Ok));
        assert_eq!(s.details.get("driver").map(String::as_str), Some("absent"));
        let (s, c) = smb1_checks(Some(3), None, Some(2));
        assert_eq!(ids(&s), ("smb1_server", State::Exposed, Severity::High));
        assert_eq!(s.fix, Some(Fix::Smb1ServerDisable));
        assert_eq!(ids(&c), ("smb1_client", State::Exposed, Severity::Medium));
        assert_eq!(smb1_checks(Some(3), Some(0), Some(4)).0.state, State::Ok, "SMB1=0 turns the server side off");
        assert_eq!(smb1_checks(Some(4), Some(1), Some(4)).1.state, State::Ok, "disabled drivers");
    }

    fn conn(state: TcpState, local: &str, lport: u16, remote: Option<&str>, exe: &str, pid: u32) -> Conn {
        let remote_addr = remote.map(|r| r.parse().unwrap());
        Conn {
            protocol: Protocol::Tcp,
            pid,
            process: if exe.is_empty() { format!("pid {pid}") } else { exe.rsplit('\\').next().unwrap_or("").to_string() },
            exe_path: exe.to_string(),
            local_addr: local.parse().unwrap(),
            local_port: lport,
            remote_scope: remote_addr.map_or(Scope::Unspecified, netconns::remote_scope),
            remote_addr,
            remote_port: 50000,
            state: Some(state),
        }
    }

    #[test]
    fn listeners_and_inbound() {
        let win = r"c:\windows";
        let conns = vec![
            conn(TcpState::Listen, "0.0.0.0", 445, None, "", 4),
            conn(TcpState::Listen, "0.0.0.0", 135, None, r"C:\Windows\System32\svchost.exe", 900),
            conn(TcpState::Listen, "127.0.0.1", 9000, None, r"C:\Apps\dev.exe", 1200),
            conn(TcpState::Listen, "0.0.0.0", 5900, None, r"C:\Program Files\vnc\winvnc.exe", 1300),
            conn(TcpState::Listen, "::", 5900, None, r"C:\Program Files\vnc\winvnc.exe", 1300),
            conn(TcpState::Listen, "0.0.0.0", 57621, None, r"C:\Users\a\AppData\Roaming\Music\music.exe", 1400),
            conn(TcpState::Listen, "0.0.0.0", 40000, None, "", 1500),
            conn(TcpState::Listen, "0.0.0.0", 22, None, "", 1600),
            conn(TcpState::Listen, "0.0.0.0", 135, None, "", 1888),
            conn(TcpState::Established, "192.168.0.10", 5900, Some("203.0.113.7"), r"C:\Program Files\vnc\winvnc.exe", 1300),
            conn(TcpState::Established, "192.168.0.10", 5900, Some("192.168.0.20"), r"C:\Program Files\vnc\winvnc.exe", 1300),
            conn(TcpState::Established, "192.168.0.10", 51000, Some("203.0.113.9"), r"C:\Apps\browser.exe", 1700),
            conn(TcpState::Established, "127.0.0.1", 9000, Some("8.8.8.8"), r"C:\Apps\dev.exe", 1200),
        ];
        let l = exposed_listeners(&conns, win);
        let got: Vec<(String, u16, Severity)> = l.iter().map(|i| (i.name.clone(), i.port.unwrap(), i.severity)).collect();
        assert_eq!(
            got,
            vec![("winvnc.exe".into(), 5900, Severity::High), ("pid 1600".into(), 22, Severity::Medium), ("music.exe".into(), 57621, Severity::Info)],
            "system and loopback skipped, v4/v6 deduped, unknown path only on risky ports"
        );
        assert_eq!(l[0].reason, Some("vnc"));
        let i = inbound_public(&conns);
        assert_eq!(i.len(), 1, "LAN peer, outgoing and loopback-only listeners do not count");
        assert_eq!((i[0].port, i[0].remote.as_deref(), i[0].severity), (Some(5900), Some("203.0.113.7"), Severity::High));
    }

    #[test]
    fn port_risks() {
        assert_eq!(port_risk(3389), (Severity::High, Some("rdp")));
        assert_eq!(port_risk(5901).0, Severity::High);
        assert_eq!(port_risk(22).0, Severity::Medium);
        assert_eq!(port_risk(27015), (Severity::Info, None));
    }

    #[test]
    fn firewall_profiles() {
        assert_eq!(firewall_check(None, None).state, State::Unknown);
        let all_on = Firewall { current: 2, enabled: [Some(true); 3] };
        assert_eq!(firewall_check(Some(all_on), None).state, State::Ok);
        let public_off = Firewall { current: 4, enabled: [Some(true), Some(true), Some(false)] };
        let c = firewall_check(Some(public_off), None);
        assert_eq!((c.severity, c.fix.clone()), (Severity::High, Some(Fix::FirewallOn { profile: FwProfile::Public })));
        let c = firewall_check(Some(public_off), Some("Other Firewall"));
        assert_eq!(c.severity, Severity::Medium);
        assert_eq!(c.details.get("third_party").map(String::as_str), Some("Other Firewall"));
        let domain_off = Firewall { current: 2, enabled: [Some(false), Some(true), Some(true)] };
        let c = firewall_check(Some(domain_off), None);
        assert_eq!((c.severity, c.fix.is_none()), (Severity::Medium, true), "not the network in use: no headline fix");
        assert_eq!(c.items[0].fix, Some(Fix::FirewallOn { profile: FwProfile::Domain }));
    }

    #[test]
    fn antivirus_states() {
        assert_eq!(product_state(0x061100), ("on", true));
        assert_eq!(product_state(0x060100), ("off", true));
        assert_eq!(product_state(0x061110), ("on", false));
        assert_eq!(product_state(0x042000).0, "snoozed");
        assert_eq!(antivirus_check(None).state, State::Unknown);
        assert_eq!(antivirus_check(Some(vec![])).severity, Severity::High, "no antivirus at all");
        let one_on = antivirus_check(Some(vec![("Defender".into(), 0x060100), ("Other AV".into(), 0x041000), ("Other AV".into(), 0x041000)]));
        assert_eq!(one_on.items.len(), 2, "duplicate registrations folded");
        assert_eq!(one_on.state, State::Ok, "another product on is enough");
        assert_eq!(one_on.items[0].reason, Some("off"));
        let old = antivirus_check(Some(vec![("AV".into(), 0x061110)]));
        assert_eq!((old.state, old.severity, old.items[0].reason), (State::Exposed, Severity::Medium, Some("outdated")));
        assert_eq!(antivirus_check(Some(vec![("AV".into(), 0x060100)])).severity, Severity::High);
    }

    #[test]
    fn sessions_and_guest() {
        let rows = vec![
            SessionRow { id: 0, protocol: 0, state: 4, client: String::new() },
            SessionRow { id: 1, protocol: 0, state: 0, client: String::new() },
            SessionRow { id: 2, protocol: 2, state: 0, client: "LAPTOP".into() },
            SessionRow { id: 3, protocol: 2, state: 4, client: String::new() },
            SessionRow { id: 65536, protocol: 2, state: 6, client: String::new() },
        ];
        let s = remote_sessions(&rows);
        assert_eq!(s.len(), 2, "console, services and the listener are not remote sessions");
        assert_eq!((s[0].name.as_str(), s[0].severity, s[0].reason), ("LAPTOP", Severity::High, Some("active")));
        assert_eq!((s[1].name.as_str(), s[1].reason), ("#3", Some("disconnected")));
        let accounts = vec![("S-1-5-21-1-2-3-500".to_string(), true), ("S-1-5-21-1-2-3-501".to_string(), false)];
        assert_eq!(guest_enabled(&accounts), Some(true));
        assert_eq!(guest_enabled(&accounts[..1]), None);
        assert_eq!(guest_enabled(&[("S-1-5-21-9-501".into(), true)]), Some(false));
    }

    #[test]
    fn upnp_only_mine_and_sane() {
        let me: HashSet<IpAddr> = [IpAddr::V4(Ipv4Addr::new(192, 168, 0, 10))].into();
        let m = |proto: &str, ext: u16, client: &str, int: u16| Mapping { protocol: proto.into(), external_port: ext, internal_port: int, internal_client: client.into(), enabled: true, description: String::new() };
        let all = vec![m("TCP", 3389, "192.168.0.10", 3389), m("UDP", 3074, "192.168.0.11", 3074), m("TCP", 80, "evil; host", 80), m("ICMP", 1, "192.168.0.10", 1), m("TCP", 0, "192.168.0.10", 8080), m("UDP", 27015, " 192.168.0.10 ", 27015)];
        let mine = mappings_to_me(&all, &me);
        assert_eq!(mine.iter().map(|x| x.external_port).collect::<Vec<_>>(), vec![3389, 27015]);
        let c = upnp_check(&Ok(all), &me);
        assert_eq!((c.state, c.severity, c.items.len()), (State::Exposed, Severity::High, 2));
        assert_eq!(c.items[0].fix, Some(Fix::UpnpRemove { protocol: "TCP".into(), external_port: 3389 }));
        assert_eq!(c.items[1].remote.as_deref(), Some("192.168.0.10:27015"));
        assert_eq!(upnp_check(&Err("timeout"), &me).state, State::Unknown);
        assert_eq!(upnp_check(&Ok(vec![]), &me).state, State::Ok);
    }

    #[test]
    fn remote_access_match() {
        let raw = RemoteRaw {
            processes: vec![r"C:\Program Files\AnyDesk\AnyDesk.exe".into(), r"C:\Windows\explorer.exe".into()],
            services: vec![
                ("AnyDesk".into(), "AnyDesk Service".into(), 2, String::new()),
                ("TeamViewer".into(), "TeamViewer".into(), 4, String::new()),
                ("cloudflared".into(), "Cloudflared agent".into(), 2, r"C:\cf\cloudflared.exe".into()),
            ],
            installed: vec!["TeamViewer".into(), "Some Game".into()],
        };
        let got: Vec<(String, &str, Severity)> = remote_apps(&raw).into_iter().map(|i| (i.name, i.source.unwrap(), i.severity)).collect();
        assert_eq!(
            got,
            vec![
                ("AnyDesk.exe".into(), "process", Severity::Medium),
                ("Cloudflared agent".into(), "service", Severity::Low),
                ("TeamViewer".into(), "installed", Severity::Info),
            ]
        );
        assert!(remote_apps(&RemoteRaw { installed: vec!["Quick Assist".into()], ..Default::default() }).is_empty(), "built-in helper only counts while running");
    }

    fn dirs() -> Dirs {
        Dirs { windir: r"c:\windows".into(), temp: vec![r"c:\users\a\appdata\local\temp".into(), r"c:\windows\temp".into()], public: r"c:\users\public".into() }
    }

    #[test]
    fn paths_and_persistence() {
        let d = dirs();
        assert_eq!(user_folder(r"c:\users\a\appdata\local\temp\x.exe", &d), Some("temp"));
        assert_eq!(user_folder(r"c:\users\a\appdata\roaming\app\a.exe", &d), Some("appdata"));
        assert_eq!(user_folder(r"c:\users\a\downloads\setup.exe", &d), Some("downloads"));
        assert_eq!(user_folder(r"c:\users\public\x.exe", &d), Some("public"));
        assert_eq!(user_folder(r"c:\program files\app\a.exe", &d), None);
        assert_eq!(user_folder(r"c:\windowsapps\a.exe", &d), None);

        let v = |exe: &str, args: &str, signed| persistence_verdict(exe, args, signed, &d);
        assert_eq!(v(r"c:\windows\system32\svchost.exe", "-k netsvcs", None), None);
        assert_eq!(v(r"c:\windows\system32\rundll32.exe", r"c:\users\a\appdata\roaming\x.dll,run", None), Some((Severity::High, "script_user_folder")));
        assert_eq!(v(r"c:\windows\system32\wscript.exe", r"c:\program files\app\a.vbs", None), None);
        assert_eq!(v(r"c:\users\a\appdata\roaming\x\x.exe", "", Some(false)), Some((Severity::High, "unsigned_user_folder")));
        assert_eq!(v(r"c:\program files\app\svc.exe", "", Some(false)), Some((Severity::Medium, "unsigned")));
        assert_eq!(v(r"c:\users\a\appdata\local\app\update.exe", "", Some(true)), None, "signed per-user updater");
        assert_eq!(v(r"c:\windows\temp\a.exe", "", Some(true)), Some((Severity::Medium, "user_folder")), "Windows\\Temp is not system");
        assert_eq!(v(r"c:\program files\app\svc.exe", "", None), None, "unknown signature alone is not a finding");

        assert_eq!(signature_from_code(0), Some(true));
        assert_eq!(signature_from_code(0x800B_0100_u32 as i32), Some(false));
        assert_eq!(signature_from_code(0x8009_6010_u32 as i32), Some(false), "tampered");
        assert_eq!(signature_from_code(0x8007_0002_u32 as i32), None);
    }

    #[test]
    fn baseline_is_validated_and_diff_alerts_once() {
        assert!(parse_baseline("not json").is_none());
        let b = parse_baseline(r#"["listener:a.exe","bogus:x","upnp:","inbound:b.exe\u0007","remote_app:c"]"#).unwrap();
        assert_eq!(b.into_iter().collect::<Vec<_>>(), vec!["listener:a.exe".to_string(), "remote_app:c".to_string()]);
        let big = serde_json::to_string(&(0..5000).map(|i| format!("listener:{i}")).collect::<Vec<_>>()).unwrap();
        assert_eq!(parse_baseline(&big).unwrap().len(), BASELINE_CAP);

        let base: BTreeSet<String> = ["listener:a.exe".to_string()].into();
        let o = |key: &str| Observed { kind: "listener", key: key.into(), name: key.into(), port: None };
        let now = vec![o("listener:a.exe"), o("listener:b.exe"), o("listener:b.exe")];
        let d = diff(&base, &now);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].key, "listener:b.exe");
    }

    #[test]
    fn fixes_are_strict_and_have_safe_backup_ids() {
        for f in [
            Fix::RdpDisable,
            Fix::NlaEnable,
            Fix::RaDisable,
            Fix::Smb1ServerDisable,
            Fix::FirewallOn { profile: FwProfile::Public },
            Fix::UpnpRemove { protocol: "udp".into(), external_port: 65535 },
            Fix::WinrmStop,
            Fix::TaskDisable { path: r"\..\..\evil".into() },
        ] {
            assert!(registry::safe_id(&f.backup_id()), "{f:?}");
            let json = serde_json::to_string(&f).unwrap();
            assert_eq!(serde_json::from_str::<Fix>(&json).unwrap(), f);
        }
        for bad in [
            r#"{"kind":"format_disk"}"#,
            r#"{"kind":"upnp_remove","protocol":"TCP","external_port":70000}"#,
            r#"{"kind":"firewall_on","profile":"all"}"#,
            r#"{"kind":"task_disable"}"#,
        ] {
            assert!(serde_json::from_str::<Fix>(bad).is_err(), "{bad}");
        }
        assert_eq!(split_task(r"\Updater"), (r"\".into(), "Updater".into()));
        assert_eq!(split_task(r"\Vendor\Sub\Task"), (r"\Vendor\Sub".into(), "Task".into()));
        assert!(matches!(upnp_protocol("icmp"), Err(ExposureError::NotApplicable)));
    }

    #[test]
    fn hostile_fix_requests_are_refused_without_touching_anything() {
        // A task that the last report did not flag cannot be disabled.
        let r = apply(&Fix::TaskDisable { path: r"\Microsoft\Windows\Defrag\ScheduledDefrag".into() });
        assert!(matches!(r, Err(ExposureError::NotApplicable)));
        assert!(matches!(reveal(r"C:\Windows\System32\cmd.exe"), Err(ExposureError::NotApplicable)));
        if !system::is_elevated() {
            assert!(matches!(apply(&Fix::RdpDisable), Err(ExposureError::NeedsAdmin)));
            assert!(matches!(revert(&Fix::Smb1ServerDisable), Err(ExposureError::NeedsAdmin)));
        }
    }

    #[test]
    fn advisor_mapping() {
        let mut fw = Check::exposed("firewall", Severity::High);
        fw.items.push(Item::new("public", Severity::High));
        let r = Report {
            checks: vec![
                rdp_checks(RdpRaw { deny: Some(0), nla: Some(0), ..Default::default() }).0,
                fw,
                Check::exposed("smb1_server", Severity::High),
                Check::exposed("antivirus", Severity::High),
                Check::exposed("guest", Severity::Medium),
                Check::ok("upnp"),
            ],
            elevated: false,
            reverts: vec![],
            took_ms: 0,
        };
        let f = advisor_findings(&r);
        let ids: Vec<&str> = f.iter().map(|x| x.id).collect();
        assert_eq!(ids, vec!["rdp_open", "firewall_off", "smb1_on", "av_off"]);
        assert!(f.iter().all(|x| x.kind == advisor::Kind::Security && x.severity == advisor::Severity::Critical));
        assert_eq!(f[0].params.get("port").map(String::as_str), Some("3389"));
        assert_eq!(f[1].params.get("count").map(String::as_str), Some("1"));
    }

    /// Elevated round trip on Remote Assistance: fix → 0 with a backup; revert → the original value,
    /// backup gone. `--ignored ra_fix_roundtrip_elevated` in an elevated shell with `G2BOOST_DATA_DIR` set.
    #[test]
    #[ignore]
    fn ra_fix_roundtrip_elevated() {
        if !crate::system::is_elevated() {
            println!("skip: not elevated");
            return;
        }
        let before = dw(Root::Hklm, RA, "fAllowToGetHelp");
        apply(&Fix::RaDisable).unwrap();
        assert_eq!(dw(Root::Hklm, RA, "fAllowToGetHelp"), Some(0));
        assert!(reverts().contains(&Fix::RaDisable));
        revert(&Fix::RaDisable).unwrap();
        assert_eq!(dw(Root::Hklm, RA, "fAllowToGetHelp"), before);
        assert!(!reverts().contains(&Fix::RaDisable));
        println!("remote assistance round trip ok (before = {before:?})");
    }

    /// Read-only, no admin: prints what this machine shows. `cargo test -p g2boost-core -- --ignored live_exposure --nocapture`
    #[test]
    #[ignore]
    fn live_exposure_report() {
        let r = report();
        for c in &r.checks {
            println!("{:<18} {:?} {:?} {:?} fix={:?}", c.id, c.state, c.severity, c.details, c.fix);
            for it in &c.items {
                println!("    - {} {:?} {:?} {:?} port={:?} {:?}", it.name, it.severity, it.source, it.reason, it.port, it.fix);
            }
        }
        println!("elevated={} took={} ms reverts={:?}", r.elevated, r.took_ms, r.reverts);
        let q = quick();
        println!("quick took {} ms; advisor: {:?}", q.took_ms, advisor_findings(&q).iter().map(|f| f.id).collect::<Vec<_>>());
    }
}
