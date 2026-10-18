//! DNS Optimizer: rank public resolvers by measured response time (plain UDP queries, no
//! admin), show each adapter's current DNS, apply a resolver pair per adapter with
//! `SetInterfaceDnsSettings` (admin) and restore exactly what was there before (backup JSON).
//! Never `netsh`/`ipconfig`. Only catalog providers can be applied (validated here, in the core,
//! so every caller — UI, MCP, routines — goes through the same allowlist).
//! The `*InterfaceDnsSettings` APIs exist only on Windows 10 2004+: they are resolved at runtime,
//! so the app still starts on older builds and DNS changes report `unsupported_windows`.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS};
use windows::Win32::NetworkManagement::IpHelper::{
    DNS_INTERFACE_SETTINGS, DNS_INTERFACE_SETTINGS_VERSION1, DNS_SETTING_NAMESERVER, GAA_FLAG_INCLUDE_GATEWAYS,
    GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses, GetBestInterface, IF_TYPE_SOFTWARE_LOOPBACK,
    IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::{AF_INET, AF_UNSPEC, SOCKADDR_IN, SOCKET_ADDRESS};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::core::{GUID, PCWSTR, PWSTR};

use crate::registry::{self, wide};
use crate::system;

/// The resolvers the UI offers (same set as `src/data/catalogs.ts`).
pub const PROVIDERS: &[(&str, &str, [u8; 4], [u8; 4])] = &[
    ("cloudflare", "Cloudflare", [1, 1, 1, 1], [1, 0, 0, 1]),
    ("quad9", "Quad9", [9, 9, 9, 9], [149, 112, 112, 112]),
    ("google", "Google", [8, 8, 8, 8], [8, 8, 4, 4]),
    ("nextdns", "NextDNS", [45, 90, 28, 0], [45, 90, 30, 0]),
    ("opendns", "OpenDNS", [208, 67, 222, 222], [208, 67, 220, 220]),
    ("controld", "Control D", [76, 76, 2, 0], [76, 76, 10, 0]),
    ("adguard", "AdGuard", [94, 140, 14, 14], [94, 140, 15, 15]),
    ("verisign", "Verisign", [64, 6, 64, 6], [64, 6, 65, 6]),
    ("comodo", "Comodo", [8, 26, 56, 26], [8, 20, 247, 20]),
    ("neustar", "Neustar", [156, 154, 70, 5], [156, 154, 71, 5]),
];

/// Popular names (cached at every big resolver): measures resolver round-trip, not recursion.
const PROBE_NAMES: &[&str] = &["google.com", "cloudflare.com", "microsoft.com", "steampowered.com", "riotgames.com"];

#[derive(Debug, thiserror::Error)]
pub enum DnsError {
    #[error("needs_admin")]
    NeedsAdmin,
    #[error("unknown_adapter")]
    UnknownAdapter,
    #[error("invalid_address")]
    InvalidAddress,
    #[error("unknown_provider")]
    UnknownProvider,
    #[error("unsupported_windows")]
    Unsupported,
    #[error("win32 {0}")]
    Win32(u32),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/* ---------------- measuring ---------------- */

/// Minimal DNS query: header (RD set) + one A/IN question.
pub(crate) fn build_query(id: u16, name: &str) -> Vec<u8> {
    let mut q = Vec::with_capacity(32 + name.len());
    q.extend(id.to_be_bytes());
    q.extend([0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]); // flags RD, QD=1, AN/NS/AR=0
    for label in name.split('.') {
        q.push(label.len() as u8);
        q.extend(label.as_bytes());
    }
    q.extend([0, 0, 1, 0, 1]); // root, QTYPE=A, QCLASS=IN
    q
}

/// A reply to *our* id with the QR bit set and RCODE 0.
pub(crate) fn is_reply(buf: &[u8], id: u16) -> bool {
    buf.len() >= 12 && u16::from_be_bytes([buf[0], buf[1]]) == id && buf[2] & 0x80 != 0 && buf[3] & 0x0F == 0
}

/// One timed query; `None` on timeout or a bad reply.
pub fn query_ms(server: Ipv4Addr, name: &str, timeout: Duration) -> Option<f64> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.set_read_timeout(Some(timeout)).ok()?;
    // Sub-second clock bits: a fresh id per query (only matches replies; the answer itself is unused).
    let id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0x5A5A, |d| (d.subsec_nanos() >> 4) as u16);
    let q = build_query(id, name);
    let t0 = Instant::now();
    sock.send_to(&q, SocketAddrV4::new(server, 53)).ok()?;
    let mut buf = [0u8; 512];
    loop {
        let (n, from) = sock.recv_from(&mut buf).ok()?;
        if from.ip() == std::net::IpAddr::V4(server) && is_reply(&buf[..n], id) {
            return Some(t0.elapsed().as_secs_f64() * 1000.0);
        }
        if t0.elapsed() > timeout {
            return None;
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RankEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub primary: Ipv4Addr,
    pub secondary: Ipv4Addr,
    /// Median of the successful samples; None when every query failed.
    pub median_ms: Option<f64>,
    pub loss_pct: f64,
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

/// Measure every provider in parallel (`samples` queries each, rotating names) and sort fastest first.
pub fn rank(samples: u32) -> Vec<RankEntry> {
    let samples = samples.clamp(1, 10);
    let handles: Vec<_> = PROVIDERS
        .iter()
        .map(|&(id, name, p, s)| {
            std::thread::spawn(move || {
                let primary = Ipv4Addr::from(p);
                // One warm-up query (ARP/route cache), not counted.
                let _ = query_ms(primary, PROBE_NAMES[0], Duration::from_millis(800));
                let mut ok = Vec::new();
                for i in 0..samples {
                    if let Some(ms) = query_ms(primary, PROBE_NAMES[i as usize % PROBE_NAMES.len()], Duration::from_millis(1000)) {
                        ok.push(ms);
                    }
                }
                let loss_pct = 100.0 * f64::from(samples - ok.len() as u32) / f64::from(samples);
                RankEntry { id, name, primary, secondary: Ipv4Addr::from(s), median_ms: median(ok), loss_pct }
            })
        })
        .collect();
    let mut out: Vec<RankEntry> = handles.into_iter().filter_map(|h| h.join().ok()).collect();
    out.sort_by(|a, b| match (a.median_ms, b.median_ms) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.name.cmp(b.name),
    });
    out
}

/* ---------------- adapters ---------------- */

#[derive(Debug, Clone, Serialize)]
pub struct Adapter {
    /// `{GUID}` — the stable id used to apply/restore.
    pub guid: String,
    pub name: String,
    pub description: String,
    /// DNS servers in use (IPv4), whether static or from DHCP.
    pub dns: Vec<Ipv4Addr>,
    pub gateway: Option<Ipv4Addr>,
    /// Static servers configured on the interface (empty = automatic/DHCP).
    pub static_dns: Vec<Ipv4Addr>,
    /// Which provider the current servers belong to, if any.
    pub provider: Option<&'static str>,
    pub link_mbps: u64,
    /// The interface Windows routes internet traffic through (`GetBestInterface` to a public IP).
    pub primary: bool,
}

fn sockaddr_v4(sa: &SOCKET_ADDRESS) -> Option<Ipv4Addr> {
    if sa.lpSockaddr.is_null() {
        return None;
    }
    // SAFETY: lpSockaddr points to a SOCKADDR of at least iSockaddrLength bytes owned by the adapter list.
    unsafe {
        if (*sa.lpSockaddr).sa_family != AF_INET {
            return None;
        }
        let sin = &*(sa.lpSockaddr as *const SOCKADDR_IN);
        Some(Ipv4Addr::from(u32::from_be(sin.sin_addr.S_un.S_addr)))
    }
}

fn pwstr(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: NUL-terminated UTF-16 owned by the API that returned it.
    unsafe { p.to_string().unwrap_or_default() }
}

fn parse_guid(s: &str) -> Option<GUID> {
    let t = s.trim().trim_start_matches('{').trim_end_matches('}');
    GUID::try_from(t).ok()
}

fn parse_list(s: &str) -> Vec<Ipv4Addr> {
    s.split([',', ' ', ';']).filter_map(|x| x.trim().parse().ok()).collect()
}

type FnGetDns = unsafe extern "system" fn(GUID, *mut DNS_INTERFACE_SETTINGS) -> u32;
type FnSetDns = unsafe extern "system" fn(GUID, *const DNS_INTERFACE_SETTINGS) -> u32;
type FnFreeDns = unsafe extern "system" fn(*mut DNS_INTERFACE_SETTINGS);
/// What `GetProcAddress` hands back (`FARPROC`).
type Farproc = unsafe extern "system" fn() -> isize;

/// `Get/Set/FreeInterfaceDnsSettings` (iphlpapi, Windows 10 2004+), resolved once.
struct DnsApi {
    get: FnGetDns,
    set: FnSetDns,
    free: FnFreeDns,
}

static DNS_API: OnceLock<Option<DnsApi>> = OnceLock::new();

fn dns_api() -> Option<&'static DnsApi> {
    DNS_API
        .get_or_init(|| {
            let lib = wide("iphlpapi.dll");
            // SAFETY: system library; the symbols have the documented signatures above.
            unsafe {
                let h = LoadLibraryW(PCWSTR(lib.as_ptr())).ok()?;
                let get = GetProcAddress(h, windows::core::s!("GetInterfaceDnsSettings"))?;
                let set = GetProcAddress(h, windows::core::s!("SetInterfaceDnsSettings"))?;
                let free = GetProcAddress(h, windows::core::s!("FreeInterfaceDnsSettings"))?;
                Some(DnsApi {
                    get: std::mem::transmute::<Farproc, FnGetDns>(get),
                    set: std::mem::transmute::<Farproc, FnSetDns>(set),
                    free: std::mem::transmute::<Farproc, FnFreeDns>(free),
                })
            }
        })
        .as_ref()
}

/// Static servers configured on the interface (`NameServer`), empty when automatic or unsupported.
fn static_servers(guid: &GUID) -> Vec<Ipv4Addr> {
    let Some(api) = dns_api() else {
        return Vec::new();
    };
    let mut s = DNS_INTERFACE_SETTINGS { Version: DNS_INTERFACE_SETTINGS_VERSION1, ..Default::default() };
    // SAFETY: the API allocates the strings; its own free function releases them.
    unsafe {
        if (api.get)(*guid, &mut s) != ERROR_SUCCESS.0 {
            return Vec::new();
        }
        let list = parse_list(&pwstr(s.NameServer));
        (api.free)(&mut s);
        list
    }
}

pub fn provider_of(servers: &[Ipv4Addr]) -> Option<&'static str> {
    let first = servers.first()?;
    PROVIDERS.iter().find(|(_, _, p, s)| Ipv4Addr::from(*p) == *first || Ipv4Addr::from(*s) == *first).map(|(id, ..)| *id)
}

/// Interface index of the route to the internet (what multi-NIC/VPN machines actually use).
fn best_if_index() -> Option<u32> {
    let mut idx = 0u32;
    // 8.8.8.8 in network byte order; only the routing table is consulted, nothing is sent.
    let dest = u32::from_ne_bytes([8, 8, 8, 8]);
    // SAFETY: plain out-parameter.
    (unsafe { GetBestInterface(dest, &mut idx) } == ERROR_SUCCESS.0).then_some(idx)
}

/// Connected, non-loopback adapters with their DNS and default gateway; the routed one first.
pub fn adapters() -> Vec<Adapter> {
    let best = best_if_index();
    let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST;
    let mut size = 16 * 1024u32;
    let mut buf: Vec<u64>;
    loop {
        buf = vec![0u64; (size as usize).div_ceil(8)];
        // SAFETY: buffer of `size` bytes, 8-byte aligned; the API fills a linked list inside it.
        let rc = unsafe { GetAdaptersAddresses(u32::from(AF_UNSPEC.0), flags, None, Some(buf.as_mut_ptr().cast()), &mut size) };
        if rc == ERROR_BUFFER_OVERFLOW.0 {
            continue;
        }
        if rc != ERROR_SUCCESS.0 {
            return Vec::new();
        }
        break;
    }
    let mut out = Vec::new();
    let mut p = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    // SAFETY: walking the list the API wrote into `buf` (alive for the whole loop).
    unsafe {
        while !p.is_null() {
            let a = &*p;
            p = a.Next;
            if a.OperStatus != IfOperStatusUp || a.IfType == IF_TYPE_SOFTWARE_LOOPBACK {
                continue;
            }
            let guid_str = if a.AdapterName.is_null() { String::new() } else { a.AdapterName.to_string().unwrap_or_default() };
            let mut dns = Vec::new();
            let mut d = a.FirstDnsServerAddress;
            while !d.is_null() {
                if let Some(ip) = sockaddr_v4(&(*d).Address) {
                    dns.push(ip);
                }
                d = (*d).Next;
            }
            let mut gateway = None;
            let mut g = a.FirstGatewayAddress;
            while !g.is_null() && gateway.is_none() {
                gateway = sockaddr_v4(&(*g).Address);
                g = (*g).Next;
            }
            // Skip virtual adapters without a gateway (VPN loopbacks, Hyper-V switches) unless they carry DNS.
            if gateway.is_none() && dns.is_empty() {
                continue;
            }
            let static_dns = parse_guid(&guid_str).map(|g| static_servers(&g)).unwrap_or_default();
            let primary = best.is_some_and(|b| b == a.Anonymous1.Anonymous.IfIndex);
            out.push(Adapter {
                primary,
                provider: provider_of(&dns),
                guid: guid_str,
                name: pwstr(a.FriendlyName),
                description: pwstr(a.Description),
                dns,
                gateway,
                static_dns,
                link_mbps: a.TransmitLinkSpeed / 1_000_000,
            });
        }
    }
    // The routed interface first, then anything with a default gateway.
    out.sort_by_key(|a| (!a.primary, a.gateway.is_none()));
    out
}

/* ---------------- apply / restore ---------------- */

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Backup {
    /// adapter GUID → previous static NameServer string ("" = automatic).
    previous: BTreeMap<String, String>,
}

fn backup_file() -> std::path::PathBuf {
    registry::data_dir().join("dns_backup.json")
}

fn load_backup() -> Backup {
    std::fs::read_to_string(backup_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_backup(b: &Backup) -> Result<(), DnsError> {
    if let Some(d) = backup_file().parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(backup_file(), serde_json::to_string_pretty(b).unwrap_or_default())?;
    Ok(())
}

fn set_name_servers(guid: &GUID, servers: &str) -> Result<(), DnsError> {
    let api = dns_api().ok_or(DnsError::Unsupported)?;
    let mut w = wide(servers);
    let s = DNS_INTERFACE_SETTINGS {
        Version: DNS_INTERFACE_SETTINGS_VERSION1,
        Flags: u64::from(DNS_SETTING_NAMESERVER),
        NameServer: PWSTR(w.as_mut_ptr()),
        ..Default::default()
    };
    // SAFETY: settings struct and string buffer outlive the call.
    let rc = unsafe { (api.set)(*guid, &s) };
    match rc {
        0 => Ok(()),
        5 => Err(DnsError::NeedsAdmin),
        code => Err(DnsError::Win32(code)),
    }
}

fn adapter_guid(guid: &str) -> Result<GUID, DnsError> {
    // Hostile caller: the GUID must be one of the live adapters.
    if !adapters().iter().any(|a| a.guid.eq_ignore_ascii_case(guid)) {
        return Err(DnsError::UnknownAdapter);
    }
    parse_guid(guid).ok_or(DnsError::UnknownAdapter)
}

/// Point `guid` at a catalog provider (admin). Never an arbitrary address: the provider id is
/// looked up in [`PROVIDERS`] here. The previous static setting is saved once.
pub fn apply(guid: &str, provider: &str) -> Result<(), DnsError> {
    let &(_, _, p, s) = PROVIDERS.iter().find(|(id, ..)| *id == provider).ok_or(DnsError::UnknownProvider)?;
    let (primary, secondary) = (Ipv4Addr::from(p), Ipv4Addr::from(s));
    if !system::is_elevated() {
        return Err(DnsError::NeedsAdmin);
    }
    dns_api().ok_or(DnsError::Unsupported)?;
    let g = adapter_guid(guid)?;
    let mut b = load_backup();
    if !b.previous.contains_key(guid) {
        let prev = static_servers(&g).iter().map(|ip| ip.to_string()).collect::<Vec<_>>().join(",");
        b.previous.insert(guid.to_string(), prev);
        save_backup(&b)?;
    }
    set_name_servers(&g, &format!("{primary},{secondary}"))?;
    crate::cleanup::flush_dns_cache();
    Ok(())
}

/// Put back exactly what was recorded before G2 Boost first changed this adapter. With no
/// recorded value there is nothing to restore (never guess DHCP — that would drop hand-set DNS).
pub fn restore(guid: &str) -> Result<(), DnsError> {
    if !system::is_elevated() {
        return Err(DnsError::NeedsAdmin);
    }
    let g = adapter_guid(guid)?;
    let mut b = load_backup();
    let Some(prev) = b.previous.remove(guid) else {
        return Err(DnsError::UnknownAdapter);
    };
    // The backup file is user-writable (rule zero): validate every address before the admin API.
    // Empty string = the adapter was on automatic/DHCP originally, which is a valid thing to restore.
    for ip in prev.split([',', ' ']).filter(|s| !s.is_empty()) {
        let a: Ipv4Addr = ip.parse().map_err(|_| DnsError::InvalidAddress)?;
        if a.is_unspecified() || a.is_broadcast() || a.is_multicast() || a.is_loopback() {
            return Err(DnsError::InvalidAddress);
        }
    }
    set_name_servers(&g, &prev)?;
    save_backup(&b)?;
    crate::cleanup::flush_dns_cache();
    Ok(())
}

pub fn has_backup(guid: &str) -> bool {
    load_backup().previous.contains_key(guid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_packet_and_reply_check() {
        let q = build_query(0x1234, "google.com");
        assert_eq!(&q[..4], &[0x12, 0x34, 0x01, 0x00]);
        assert_eq!(q[12], 6);
        assert_eq!(&q[13..19], b"google");
        assert_eq!(&q[q.len() - 4..], &[0, 1, 0, 1]);
        let mut reply = q.clone();
        reply[2] |= 0x80;
        assert!(is_reply(&reply, 0x1234));
        assert!(!is_reply(&q, 0x1234), "query is not a reply");
        assert!(!is_reply(&reply, 0x9999), "wrong id");
        reply[3] |= 0x03;
        assert!(!is_reply(&reply, 0x1234), "NXDOMAIN rejected");
        assert_eq!(median(vec![3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(vec![]), None);
        assert_eq!(parse_list("1.1.1.1,1.0.0.1 x"), vec![Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(1, 0, 0, 1)]);
        assert_eq!(provider_of(&[Ipv4Addr::new(1, 0, 0, 1)]), Some("cloudflare"));
    }

    #[test]
    fn live_adapters_and_one_resolver() {
        let a = adapters();
        assert!(!a.is_empty(), "at least one connected adapter");
        assert!(a.iter().any(|x| x.gateway.is_some()));
        assert!(a.iter().all(|x| x.guid.starts_with('{')));
        // Network-dependent; tolerate offline CI by only asserting the type.
        let _ = query_ms(Ipv4Addr::new(1, 1, 1, 1), "google.com", Duration::from_millis(1500));
        assert!(a[0].primary || a.iter().all(|x| !x.primary), "routed adapter sorts first");
        assert!(dns_api().is_some(), "Windows 10 2004+ dev box exposes the DNS settings API");
        assert!(matches!(apply(&a[0].guid, "8.8.8.8"), Err(DnsError::UnknownProvider)), "raw IPs are not providers");
        if !system::is_elevated() {
            assert!(matches!(apply(&a[0].guid, "cloudflare"), Err(DnsError::NeedsAdmin)));
        }
        assert!(matches!(adapter_guid("{00000000-0000-0000-0000-000000000000}"), Err(DnsError::UnknownAdapter)));
    }

    /// Live ranking against the real resolvers (network). Run by hand with --ignored --nocapture.
    #[test]
    #[ignore]
    fn rank_live() {
        for e in rank(5) {
            eprintln!("{:<10} {:>15} median={:?} loss={:.0}%", e.name, e.primary, e.median_ms.map(|m| (m * 10.0).round() / 10.0), e.loss_pct);
        }
        for a in adapters() {
            eprintln!("adapter {} dns={:?} static={:?} gw={:?} provider={:?}", a.name, a.dns, a.static_dns, a.gateway, a.provider);
        }
    }
}
