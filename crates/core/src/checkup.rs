//! Check-up: nineteen measurements of this PC (free space, memory, drive health, security
//! switches, Windows activation and updates, crashes, startup load, drivers and firmware), each
//! with its value, the limit it is judged against and an ok / attention verdict.
//!
//! It only measures. Every row names the screen of the app that fixes it (`fix`, a route id) or
//! none when the fix lives in Windows itself. Verdicts that are thresholds on hardware (disk
//! space, XMP, Resizable BAR, BIOS and driver age) come from the advisor's findings, so the
//! check-up, the Painel and the assistant share one set of limits; the rest are judged here.
//!
//! Read-only and no admin; SMART on SATA drives needs it and the row says so. The wording is the
//! UI's (`checkup.<id>.title|ok|bad`); this module returns ids, numbers and the names the machine
//! gave (antivirus product, TPM version).

use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::advisor::{self, Env};
use crate::exposure::{self, State as ExposureState};
use crate::firmware::{self, Firmware};
use crate::hardware::Profile;
use crate::network::{self, Probe};
use crate::registry::{self, Root, Value, now_unix};
use crate::smart::{self, DriveHealth, Status};
use crate::updates::{self, WuHistoryEntry};
use crate::{startup, system, winevents, wmi};

/* ---------------- limits (the ones the advisor does not own) ---------------- */

/// Physical memory in use above this is "almost full".
pub const RAM_MAX_PCT: f64 = 85.0;
/// More enabled startup programs than this slow the boot.
pub const STARTUP_MAX: u32 = 10;
/// More application crashes than this in a week point at a faulty program or driver.
pub const CRASHES_MAX: u32 = 5;
/// Window of the crash count.
pub const CRASH_DAYS: u32 = 7;
/// Any unexpected shutdown in this many days is worth a look.
pub const SHUTDOWN_DAYS: u32 = 30;
/// Windows ships cumulative updates monthly; this long without one means it is behind.
pub const UPDATE_MAX_DAYS: u32 = 45;
/// Free space below this share (the advisor also flags under 20 GB) is "full".
pub const DISK_MIN_FREE_PCT: f64 = 15.0;

/// Windows Application id in `SoftwareLicensingProduct` (other ids are Office and add-ons).
const WINDOWS_APP_ID: &str = "55c92734-d682-4d71-983e-d6ec3f16059f";
/// Defender definition and platform updates land almost daily: they are not "Windows Update".
const DEFINITION_KBS: &[&str] = &["KB2267602", "KB4052623"];

/// Every id, in the order the page shows them.
pub const IDS: &[&str] = &[
    "disk_free", "ram_use", "disk_health", "antivirus", "firewall", "uac", "secure_boot", "tpm", "activation", "windows_update",
    "reboot", "shutdowns", "crashes", "startup", "internet", "gpu_driver", "bios_age", "xmp", "rebar",
];

/// Why a row could not be judged (`checkup.unknown.<reason>`).
pub const REASONS: &[&str] = &["unreadable", "needs_admin", "not_applicable", "no_history"];

/* ---------------- model ---------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Ok,
    Attention,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub id: &'static str,
    pub state: State,
    /// Why it could not be judged (`Unknown` only).
    pub reason: Option<&'static str>,
    /// The measure the row shows; None when it could not be read.
    pub value: Option<f64>,
    /// The line between ok and attention when the rule is a number.
    pub limit: Option<f64>,
    /// How the UI writes `value`: `pct`, `bytes`, `days`, `count`, `ms` or `bool` (no number).
    pub unit: &'static str,
    /// Words and numbers for the explanation. A name ending in `_bytes` or `_days` is a number
    /// the UI formats for the locale (a size, an age); the rest is text.
    pub params: BTreeMap<&'static str, String>,
    /// Route of the screen that fixes it; None = the fix lives in Windows itself.
    pub fix: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub ok: u32,
    pub attention: u32,
    pub unknown: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub items: Vec<Item>,
    pub summary: Summary,
    pub took_ms: u64,
    pub elevated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uac {
    Off,
    /// Administrators are not asked for consent.
    Silent,
    On,
}

/// Drive health in numbers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DriveFacts {
    pub good: u32,
    pub problem: u32,
    pub unknown: u32,
    pub needs_admin: bool,
}

/// What the measurements found. `None` = it could not be read.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// System drive: letter, free bytes, total bytes.
    pub disk: Option<(String, u64, u64)>,
    /// Physical memory: used bytes, total bytes.
    pub ram: Option<(u64, u64)>,
    pub drives: Option<DriveFacts>,
    pub antivirus: Option<bool>,
    pub antivirus_name: Option<String>,
    pub firewall: Option<bool>,
    pub uac: Option<Uac>,
    pub secure_boot: Option<bool>,
    /// TPM version when one was detected; `None` is "no TPM found".
    pub tpm: Option<String>,
    pub activated: Option<bool>,
    /// `Some(None)` = measured and unreachable; `Some(Some(ms))` = answered.
    pub internet: Option<Option<u32>>,
    pub shutdowns: Option<u32>,
    pub crashes: Option<u32>,
    pub startup_apps: Option<u32>,
    pub reboot_pending: Option<bool>,
    /// `Some(None)` = history read but no successful install on record.
    pub update_days: Option<Option<u32>>,
    pub gpu_driver_days: Option<u32>,
    pub bios_days: Option<u32>,
    pub xmp_at_rated: Option<bool>,
    pub rebar: Option<bool>,
    /// Ids of the advisor's findings for this PC (`system_disk_full`, `xmp_off`, `rebar_off`,
    /// `bios_old`, `gpu_driver_old`).
    pub findings: BTreeSet<&'static str>,
}

/* ---------------- verdicts (pure) ---------------- */

impl Item {
    fn new(id: &'static str, unit: &'static str, fix: Option<&'static str>) -> Self {
        Item { id, state: State::Unknown, reason: Some("unreadable"), value: None, limit: None, unit, params: BTreeMap::new(), fix }
    }
    fn verdict(mut self, attention: bool) -> Self {
        self.state = if attention { State::Attention } else { State::Ok };
        self.reason = None;
        self
    }
    fn unknown(mut self, reason: &'static str) -> Self {
        self.state = State::Unknown;
        self.reason = Some(reason);
        self
    }
    fn value(mut self, v: f64) -> Self {
        self.value = Some(v);
        self
    }
    fn limit(mut self, v: f64) -> Self {
        self.limit = Some(v);
        self
    }
    fn param(mut self, k: &'static str, v: impl ToString) -> Self {
        self.params.insert(k, v.to_string());
        self
    }
}

fn pct(part: u64, whole: u64) -> f64 {
    if whole == 0 { 0.0 } else { (part as f64 * 1000.0 / whole as f64).round() / 10.0 }
}

fn boolean(id: &'static str, fix: Option<&'static str>, on: Option<bool>) -> Item {
    let it = Item::new(id, "bool", fix);
    match on {
        Some(on) => it.verdict(!on).value(if on { 1.0 } else { 0.0 }),
        None => it,
    }
}

/// Judge every measurement. Pure: `Facts` in, rows out, in the order of [`IDS`].
pub fn evaluate(f: &Facts) -> Vec<Item> {
    let found = |id: &str| f.findings.contains(id);
    let mut out = Vec::with_capacity(IDS.len());

    out.push(match &f.disk {
        Some((drive, free, total)) if *total > 0 => Item::new("disk_free", "pct", Some("cleanup"))
            .verdict(found("system_disk_full"))
            .value(pct(*free, *total))
            .limit(DISK_MIN_FREE_PCT)
            .param("drive", drive)
            .param("free_bytes", free)
            .param("total_bytes", total),
        _ => Item::new("disk_free", "pct", Some("cleanup")),
    });

    out.push(match f.ram {
        Some((used, total)) if total > 0 => {
            let p = pct(used, total);
            Item::new("ram_use", "pct", Some("boost"))
                .verdict(p > RAM_MAX_PCT)
                .value(p)
                .limit(RAM_MAX_PCT)
                .param("used_bytes", used)
                .param("total_bytes", total)
        }
        _ => Item::new("ram_use", "pct", Some("boost")),
    });

    out.push(match f.drives {
        Some(d) if d.problem > 0 => Item::new("disk_health", "count", Some("mypc")).verdict(true).value(f64::from(d.problem)).limit(0.0),
        Some(d) if d.good > 0 => Item::new("disk_health", "count", Some("mypc")).verdict(false).param("unread", d.unknown),
        Some(d) if d.needs_admin => Item::new("disk_health", "count", Some("mypc")).unknown("needs_admin"),
        _ => Item::new("disk_health", "count", Some("mypc")),
    });

    let mut av = boolean("antivirus", Some("security"), f.antivirus);
    if let Some(name) = f.antivirus_name.as_ref().filter(|n| !n.is_empty() && f.antivirus == Some(true)) {
        av = av.param("name", name);
    }
    out.push(av);
    out.push(boolean("firewall", Some("security"), f.firewall));

    out.push(match f.uac {
        Some(u) => Item::new("uac", "bool", None).verdict(u != Uac::On).value(if u == Uac::On { 1.0 } else { 0.0 }),
        None => Item::new("uac", "bool", None),
    });

    out.push(boolean("secure_boot", Some("mypc"), f.secure_boot));

    // No TPM found is a verdict, not a failed read: the profile cannot tell the two apart.
    let tpm_ok = f.tpm.as_deref() == Some("2.0");
    let mut tpm = Item::new("tpm", "bool", Some("mypc")).verdict(!tpm_ok).value(if tpm_ok { 1.0 } else { 0.0 });
    if let Some(v) = f.tpm.as_ref().filter(|v| !v.is_empty()) {
        tpm = tpm.param("version", v);
    }
    out.push(tpm);

    out.push(boolean("activation", None, f.activated));

    out.push(match f.update_days {
        Some(Some(d)) => Item::new("windows_update", "days", Some("apps")).verdict(d > UPDATE_MAX_DAYS).value(f64::from(d)).limit(f64::from(UPDATE_MAX_DAYS)).param("age_days", d),
        Some(None) => Item::new("windows_update", "days", Some("apps")).unknown("no_history"),
        None => Item::new("windows_update", "days", Some("apps")),
    });

    out.push(boolean("reboot", Some("apps"), f.reboot_pending.map(|p| !p)));
    // `reboot` reads as "is the PC free of a pending restart": pending = attention.

    out.push(match f.shutdowns {
        Some(n) => Item::new("shutdowns", "count", Some("maintenance")).verdict(n > 0).value(f64::from(n)).limit(0.0).param("window", SHUTDOWN_DAYS),
        None => Item::new("shutdowns", "count", Some("maintenance")),
    });
    out.push(match f.crashes {
        Some(n) => Item::new("crashes", "count", Some("maintenance")).verdict(n > CRASHES_MAX).value(f64::from(n)).limit(f64::from(CRASHES_MAX)).param("window", CRASH_DAYS),
        None => Item::new("crashes", "count", Some("maintenance")),
    });
    out.push(match f.startup_apps {
        Some(n) => Item::new("startup", "count", Some("boost")).verdict(n > STARTUP_MAX).value(f64::from(n)).limit(f64::from(STARTUP_MAX)),
        None => Item::new("startup", "count", Some("boost")),
    });

    out.push(match f.internet {
        Some(Some(ms)) => Item::new("internet", "ms", Some("network")).verdict(false).value(f64::from(ms)),
        Some(None) => Item::new("internet", "ms", Some("network")).verdict(true),
        None => Item::new("internet", "ms", Some("network")),
    });

    out.push(match f.gpu_driver_days {
        Some(d) => Item::new("gpu_driver", "days", Some("gpu")).verdict(found("gpu_driver_old")).value(f64::from(d)).param("age_days", d),
        None => Item::new("gpu_driver", "days", Some("gpu")),
    });
    out.push(match f.bios_days {
        Some(d) => Item::new("bios_age", "days", Some("mypc")).verdict(found("bios_old")).value(f64::from(d)).param("age_days", d),
        None => Item::new("bios_age", "days", Some("mypc")),
    });

    // XMP / EXPO: only the advisor's inference from the part number says "below rated".
    out.push(if found("xmp_off") {
        boolean("xmp", Some("mypc"), Some(false))
    } else {
        match f.xmp_at_rated {
            Some(true) => boolean("xmp", Some("mypc"), Some(true)),
            _ => Item::new("xmp", "bool", Some("mypc")).unknown("not_applicable"),
        }
    });

    // Resizable BAR matters only on the GPU generations the advisor lists; off elsewhere is "n/a".
    out.push(match (f.rebar, found("rebar_off")) {
        (_, true) => boolean("rebar", Some("mypc"), Some(false)),
        (Some(true), false) => boolean("rebar", Some("mypc"), Some(true)),
        (Some(false), false) => Item::new("rebar", "bool", Some("mypc")).unknown("not_applicable"),
        (None, false) => Item::new("rebar", "bool", Some("mypc")),
    });

    out
}

pub fn summarize(items: &[Item]) -> Summary {
    items.iter().fold(Summary::default(), |mut s, i| {
        match i.state {
            State::Ok => s.ok += 1,
            State::Attention => s.attention += 1,
            State::Unknown => s.unknown += 1,
        }
        s
    })
}

/// `EnableLUA` off, or administrators never asked (`ConsentPromptBehaviorAdmin` = 0). Missing
/// values mean the Windows defaults (on, prompt for consent = 5).
pub(crate) fn uac_level(enable_lua: Option<u32>, consent_admin: Option<u32>) -> Uac {
    if enable_lua.unwrap_or(1) == 0 {
        Uac::Off
    } else if consent_admin.unwrap_or(5) == 0 {
        Uac::Silent
    } else {
        Uac::On
    }
}

/// Windows is activated when any Windows license reports `Licensed` (1).
pub(crate) fn activation_state(statuses: &[i64]) -> Option<bool> {
    (!statuses.is_empty()).then(|| statuses.contains(&1))
}

/// Days since the newest successful install that is not a Defender definition update.
pub(crate) fn last_update_days(history: &[WuHistoryEntry], now: u64) -> Option<u32> {
    history
        .iter()
        .filter(|e| e.operation == "install" && e.result == "ok")
        .filter(|e| !e.kb.as_deref().is_some_and(|kb| DEFINITION_KBS.contains(&kb)))
        .map(|e| e.at)
        .max()
        .map(|at| u32::try_from(now.saturating_sub(at) / 86_400).unwrap_or(u32::MAX))
}

pub(crate) fn drive_facts(drives: &[DriveHealth]) -> DriveFacts {
    let mut f = DriveFacts::default();
    for d in drives {
        match d.status {
            Status::Good => f.good += 1,
            Status::Caution | Status::Bad => f.problem += 1,
            Status::Unknown => f.unknown += 1,
        }
        f.needs_admin |= d.needs_admin;
    }
    f
}

/* ---------------- gathering ---------------- */

fn dword(path: &str, name: &str) -> Option<Option<u32>> {
    match registry::read(Root::Hklm, path, name) {
        Ok(Value::Dword(d)) => Some(Some(d)),
        Ok(Value::Absent) => Some(None),
        _ => None,
    }
}

fn read_uac() -> Option<Uac> {
    let key = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System";
    Some(uac_level(dword(key, "EnableLUA")?, dword(key, "ConsentPromptBehaviorAdmin")?))
}

/// The two Windows servicing signals of a pending restart. (`PendingFileRenameOperations` is left
/// out on purpose: installers leave it behind without any restart being needed.)
fn reboot_pending_in_registry() -> bool {
    let has = |path: &str, name: &str| registry::subkeys(Root::Hklm, path).is_ok_and(|keys| keys.iter().any(|k| k.eq_ignore_ascii_case(name)));
    has(r"SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update", "RebootRequired")
        || has(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing", "RebootPending")
}

fn read_activation() -> Option<bool> {
    let wql = format!("SELECT LicenseStatus FROM SoftwareLicensingProduct WHERE PartialProductKey IS NOT NULL AND ApplicationId = '{WINDOWS_APP_ID}'");
    let rows = wmi::query_timeout(r"ROOT\CIMV2", &wql, &["LicenseStatus"], Duration::from_secs(8)).ok()?;
    let statuses: Vec<i64> = rows.iter().filter_map(|r| r.get("LicenseStatus")?.as_f64()).map(|v| v as i64).collect();
    activation_state(&statuses)
}

/// Round trip to a public address: two pings and one TCP connect at once, the fastest answer wins
/// (some networks drop ICMP). `Some(None)` = nothing answered.
fn read_internet() -> Option<Option<u32>> {
    let ping = |ip: Ipv4Addr| match network::ping(ip, 1500) {
        Ok(Probe::Reply { rtt_ms, .. }) => Some(rtt_ms),
        _ => None,
    };
    let tcp = || {
        let t0 = Instant::now();
        let addr = SocketAddr::from((Ipv4Addr::new(1, 1, 1, 1), 443));
        TcpStream::connect_timeout(&addr, Duration::from_millis(1500)).ok().map(|_| t0.elapsed().as_millis() as u32)
    };
    std::thread::scope(|s| {
        let a = s.spawn(|| ping(Ipv4Addr::new(1, 1, 1, 1)));
        let b = s.spawn(|| ping(Ipv4Addr::new(8, 8, 8, 8)));
        let c = s.spawn(tcp);
        Some([a.join().ok().flatten(), b.join().ok().flatten(), c.join().ok().flatten()].into_iter().flatten().min())
    })
}

/// The measurements, in parallel: the slowest (WMI license, event logs, Windows Update history)
/// sets the time, a few seconds.
pub fn gather(p: &Profile) -> Facts {
    let fw: Firmware = firmware::read(p);
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let sys = system::list_drives().into_iter().find(|d| d.letter.eq_ignore_ascii_case(&drive));
    let (exp, drives, shutdowns, crashes, wu, activated, internet) = std::thread::scope(|s| {
        let exp = s.spawn(exposure::quick);
        let drives = s.spawn(smart::health);
        let shutdowns = s.spawn(|| winevents::unexpected_shutdowns(SHUTDOWN_DAYS));
        let crashes = s.spawn(|| winevents::app_crashes(CRASH_DAYS));
        let wu = s.spawn(|| updates::wu_status(100).ok());
        let activated = s.spawn(read_activation);
        let internet = s.spawn(read_internet);
        (
            exp.join().ok(),
            drives.join().ok(),
            shutdowns.join().ok().flatten(),
            crashes.join().ok().flatten(),
            wu.join().ok().flatten(),
            activated.join().ok().flatten(),
            internet.join().ok().flatten(),
        )
    });

    let check = |id: &str| exp.as_ref().and_then(|r| r.checks.iter().find(|c| c.id == id));
    let on = |id: &str, bad: fn(&exposure::Check) -> bool| check(id).and_then(|c| (c.state != ExposureState::Unknown).then(|| !bad(c)));
    let gpu_driver_days = p.gpus.first().and_then(|g| g.driver.as_deref()).and_then(advisor::driver_age_days);
    let env = Env {
        system_free_bytes: sys.as_ref().map(|d| d.free_bytes),
        system_total_bytes: sys.as_ref().map(|d| d.total_bytes),
        gpu_driver_age_days: gpu_driver_days,
        firmware: Some(fw.clone()),
        exposure: exp.clone(),
        ..Env::default()
    };
    let findings: BTreeSet<&'static str> = advisor::analyze(p, &env).findings.iter().map(|f| f.id).collect();
    let mem = system::memory_status();
    let wu_reboot = wu.as_ref().map(|w| w.reboot_required);
    Facts {
        disk: sys.filter(|d| d.total_bytes > 0).map(|d| (d.letter, d.free_bytes, d.total_bytes)),
        ram: mem.map(|m| (m.total_bytes.saturating_sub(m.avail_bytes), m.total_bytes)),
        drives: drives.as_deref().map(drive_facts).filter(|d| d.good + d.problem + d.unknown > 0),
        antivirus: on("antivirus", |c| c.state == ExposureState::Exposed),
        antivirus_name: check("antivirus").and_then(|c| c.items.iter().find(|i| i.reason == Some("on")).map(|i| i.name.clone())),
        firewall: on("firewall", |c| c.state == ExposureState::Exposed && c.severity >= exposure::Severity::Medium),
        uac: read_uac(),
        secure_boot: p.secure_boot,
        tpm: p.tpm.clone(),
        activated,
        internet,
        shutdowns,
        crashes,
        startup_apps: Some(startup::list().iter().filter(|i| i.enabled && !i.protected).count() as u32),
        reboot_pending: Some(reboot_pending_in_registry() || wu_reboot == Some(true)),
        update_days: wu.as_ref().map(|w| last_update_days(&w.history, now_unix())),
        gpu_driver_days,
        bios_days: fw.bios_age_days,
        xmp_at_rated: fw.memory_at_rated_speed,
        rebar: fw.resizable_bar,
        findings,
    }
}

/// Measure everything and judge it.
pub fn run(p: &Profile) -> Report {
    let t0 = Instant::now();
    let items = evaluate(&gather(p));
    Report { summary: summarize(&items), items, took_ms: t0.elapsed().as_millis() as u64, elevated: system::is_elevated() }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    /// Every measurement fine.
    fn healthy() -> Facts {
        Facts {
            disk: Some(("C:".into(), 80 * GIB, 200 * GIB)),
            ram: Some((8 * GIB, 32 * GIB)),
            drives: Some(DriveFacts { good: 2, problem: 0, unknown: 0, needs_admin: false }),
            antivirus: Some(true),
            antivirus_name: Some("Windows Defender".into()),
            firewall: Some(true),
            uac: Some(Uac::On),
            secure_boot: Some(true),
            tpm: Some("2.0".into()),
            activated: Some(true),
            internet: Some(Some(12)),
            shutdowns: Some(0),
            crashes: Some(1),
            startup_apps: Some(6),
            reboot_pending: Some(false),
            update_days: Some(Some(9)),
            gpu_driver_days: Some(30),
            bios_days: Some(400),
            xmp_at_rated: Some(true),
            rebar: Some(true),
            findings: BTreeSet::new(),
        }
    }

    fn get(items: &[Item], id: &str) -> Item {
        items.iter().find(|i| i.id == id).cloned().unwrap_or_else(|| panic!("no row {id}"))
    }

    fn with(mutate: impl FnOnce(&mut Facts), id: &str) -> Item {
        let mut f = healthy();
        mutate(&mut f);
        get(&evaluate(&f), id)
    }

    #[test]
    fn a_healthy_pc_passes_every_row_in_the_documented_order() {
        let items = evaluate(&healthy());
        assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), IDS);
        assert!(items.iter().all(|i| i.state == State::Ok && i.reason.is_none()), "{items:?}");
        assert_eq!(summarize(&items), Summary { ok: 19, attention: 0, unknown: 0 });
        assert_eq!(IDS.iter().collect::<BTreeSet<_>>().len(), IDS.len(), "ids are unique");
        assert_eq!(IDS.len(), 19);
    }

    #[test]
    fn nothing_readable_is_unknown_never_a_fake_verdict() {
        let items = evaluate(&Facts::default());
        for i in &items {
            match i.id {
                // The profile cannot tell "no TPM" from "unreadable": no TPM found is attention.
                "tpm" => assert_eq!(i.state, State::Attention),
                _ => assert_eq!(i.state, State::Unknown, "{}", i.id),
            }
        }
        assert!(items.iter().filter(|i| i.state == State::Unknown).all(|i| REASONS.contains(&i.reason.unwrap())));
    }

    #[test]
    fn free_space_follows_the_advisor_and_reports_the_share() {
        let ok = with(|_| {}, "disk_free");
        assert_eq!((ok.value, ok.limit, ok.unit, ok.fix), (Some(40.0), Some(15.0), "pct", Some("cleanup")));
        assert_eq!(ok.params["drive"], "C:");
        let full = with(|f| { f.disk = Some(("D:".into(), 10 * GIB, 200 * GIB)); f.findings.insert("system_disk_full"); }, "disk_free");
        assert_eq!((full.state, full.value), (State::Attention, Some(5.0)));
        assert_eq!(full.params["free_bytes"], (10 * GIB).to_string());
        assert_eq!(with(|f| f.disk = Some(("C:".into(), 0, 0)), "disk_free").state, State::Unknown, "a 0-byte volume is not a measurement");
    }

    #[test]
    fn memory_limit_is_85_percent_inclusive() {
        let at = |used: u64| with(move |f| f.ram = Some((used, 1000)), "ram_use");
        assert_eq!(at(850).state, State::Ok, "85.0 % is still fine");
        assert_eq!(at(851).state, State::Attention);
        assert_eq!(at(0).state, State::Ok);
        assert_eq!(at(1000).value, Some(100.0));
        assert_eq!(with(|f| f.ram = Some((5, 0)), "ram_use").state, State::Unknown);
    }

    #[test]
    fn counts_have_their_own_edges() {
        let n = |id: &str, set: fn(&mut Facts, u32), v: u32| {
            let mut f = healthy();
            set(&mut f, v);
            get(&evaluate(&f), id).state
        };
        assert_eq!(n("shutdowns", |f, v| f.shutdowns = Some(v), 0), State::Ok);
        assert_eq!(n("shutdowns", |f, v| f.shutdowns = Some(v), 1), State::Attention);
        assert_eq!(n("crashes", |f, v| f.crashes = Some(v), CRASHES_MAX), State::Ok);
        assert_eq!(n("crashes", |f, v| f.crashes = Some(v), CRASHES_MAX + 1), State::Attention);
        assert_eq!(n("startup", |f, v| f.startup_apps = Some(v), STARTUP_MAX), State::Ok);
        assert_eq!(n("startup", |f, v| f.startup_apps = Some(v), STARTUP_MAX + 1), State::Attention);
        assert_eq!(n("windows_update", |f, v| f.update_days = Some(Some(v)), UPDATE_MAX_DAYS), State::Ok);
        assert_eq!(n("windows_update", |f, v| f.update_days = Some(Some(v)), UPDATE_MAX_DAYS + 1), State::Attention);
        let s = with(|f| f.shutdowns = Some(3), "shutdowns");
        assert_eq!((s.value, s.limit, s.params["window"].as_str()), (Some(3.0), Some(0.0), "30"));
        let none = with(|f| f.update_days = Some(None), "windows_update");
        assert_eq!((none.state, none.reason), (State::Unknown, Some("no_history")));
    }

    #[test]
    fn drive_health_states() {
        let d = |good, problem, unknown, needs_admin| with(move |f| f.drives = Some(DriveFacts { good, problem, unknown, needs_admin }), "disk_health");
        let fine = d(2, 0, 0, false);
        assert_eq!((fine.state, fine.value), (State::Ok, None), "no number when nothing is wrong");
        let mixed = d(1, 1, 1, true);
        assert_eq!((mixed.state, mixed.value), (State::Attention, Some(1.0)));
        let partial = d(1, 0, 1, true);
        assert_eq!((partial.state, partial.params["unread"].as_str()), (State::Ok, "1"), "a good drive plus an unreadable one is still good news");
        let blind = d(0, 0, 2, true);
        assert_eq!((blind.state, blind.reason), (State::Unknown, Some("needs_admin")));
        let blind_usb = d(0, 0, 1, false);
        assert_eq!((blind_usb.state, blind_usb.reason), (State::Unknown, Some("unreadable")));
        assert_eq!(with(|f| f.drives = None, "disk_health").state, State::Unknown);
    }

    #[test]
    fn security_switches() {
        let av = with(|f| f.antivirus = Some(false), "antivirus");
        assert_eq!((av.state, av.fix), (State::Attention, Some("security")));
        assert!(!av.params.contains_key("name"), "no product name when none is active");
        assert_eq!(with(|_| {}, "antivirus").params["name"], "Windows Defender");
        assert_eq!(with(|f| f.firewall = Some(false), "firewall").state, State::Attention);
        assert_eq!(with(|f| f.uac = Some(Uac::Off), "uac").state, State::Attention);
        assert_eq!(with(|f| f.uac = Some(Uac::Silent), "uac").state, State::Attention);
        assert_eq!(with(|_| {}, "uac").fix, None, "UAC is changed in Windows, not in the app");
        assert_eq!(with(|f| f.secure_boot = Some(false), "secure_boot").state, State::Attention);
        assert_eq!(with(|f| f.secure_boot = None, "secure_boot").state, State::Unknown);
        assert_eq!(with(|f| f.activated = Some(false), "activation").state, State::Attention);
        assert_eq!(with(|_| {}, "activation").fix, None);
        let tpm12 = with(|f| f.tpm = Some("1.2".into()), "tpm");
        assert_eq!((tpm12.state, tpm12.params["version"].as_str()), (State::Attention, "1.2"));
        assert_eq!(with(|f| f.tpm = None, "tpm").state, State::Attention);
    }

    #[test]
    fn pending_restart_and_internet() {
        assert_eq!(with(|f| f.reboot_pending = Some(true), "reboot").state, State::Attention);
        assert_eq!(with(|f| f.reboot_pending = Some(false), "reboot").state, State::Ok);
        let up = with(|_| {}, "internet");
        assert_eq!((up.state, up.value, up.fix), (State::Ok, Some(12.0), Some("network")));
        let down = with(|f| f.internet = Some(None), "internet");
        assert_eq!((down.state, down.value), (State::Attention, None));
        assert_eq!(with(|f| f.internet = None, "internet").state, State::Unknown);
    }

    #[test]
    fn firmware_rows_use_the_advisor_findings() {
        let old_driver = with(|f| { f.gpu_driver_days = Some(400); f.findings.insert("gpu_driver_old"); }, "gpu_driver");
        assert_eq!((old_driver.state, old_driver.value), (State::Attention, Some(400.0)));
        assert_eq!(with(|_| {}, "gpu_driver").state, State::Ok);
        assert_eq!(with(|f| f.gpu_driver_days = None, "gpu_driver").state, State::Unknown);
        assert_eq!(with(|f| { f.bios_days = Some(2000); f.findings.insert("bios_old"); }, "bios_age").state, State::Attention);
        assert_eq!(with(|_| {}, "bios_age").state, State::Ok);

        assert_eq!(with(|f| { f.xmp_at_rated = Some(false); f.findings.insert("xmp_off"); }, "xmp").state, State::Attention);
        assert_eq!(with(|_| {}, "xmp").state, State::Ok);
        let xmp_n_a = with(|f| f.xmp_at_rated = None, "xmp");
        assert_eq!((xmp_n_a.state, xmp_n_a.reason), (State::Unknown, Some("not_applicable")));

        assert_eq!(with(|f| { f.rebar = Some(false); f.findings.insert("rebar_off"); }, "rebar").state, State::Attention);
        let old_gpu = with(|f| f.rebar = Some(false), "rebar");
        assert_eq!((old_gpu.state, old_gpu.reason), (State::Unknown, Some("not_applicable")), "off on a GPU that gains nothing from it");
        assert_eq!(with(|f| f.rebar = None, "rebar").reason, Some("unreadable"));
        assert_eq!(with(|_| {}, "rebar").state, State::Ok);
    }

    #[test]
    fn shares_never_divide_by_zero_and_round_to_a_tenth() {
        assert_eq!(pct(5, 0), 0.0);
        assert_eq!(pct(0, 0), 0.0);
        assert_eq!(pct(1, 3), 33.3);
        assert_eq!(pct(2, 3), 66.7);
        assert_eq!(pct(u64::MAX, u64::MAX), 100.0);
    }

    #[test]
    fn summary_counts_each_state() {
        let mut f = healthy();
        f.shutdowns = Some(2);
        f.firewall = Some(false);
        f.rebar = None;
        f.xmp_at_rated = None;
        let s = summarize(&evaluate(&f));
        assert_eq!(s, Summary { ok: 15, attention: 2, unknown: 2 });
        assert_eq!(s.ok + s.attention + s.unknown, 19);
    }

    #[test]
    fn registry_and_wmi_readings_become_verdicts() {
        assert_eq!(uac_level(None, None), Uac::On, "missing values are the Windows defaults");
        assert_eq!(uac_level(Some(1), Some(5)), Uac::On);
        assert_eq!(uac_level(Some(1), Some(2)), Uac::On);
        assert_eq!(uac_level(Some(1), Some(0)), Uac::Silent);
        assert_eq!(uac_level(Some(0), Some(5)), Uac::Off);
        assert_eq!(uac_level(Some(0), Some(0)), Uac::Off);

        assert_eq!(activation_state(&[]), None);
        assert_eq!(activation_state(&[1]), Some(true));
        assert_eq!(activation_state(&[0, 1]), Some(true), "an add-on license that expired does not unlicense Windows");
        for bad in [0, 2, 3, 4, 5, 6] {
            assert_eq!(activation_state(&[bad]), Some(false), "status {bad}");
        }
    }

    fn entry(kb: Option<&str>, at: u64, op: &'static str, result: &'static str) -> WuHistoryEntry {
        WuHistoryEntry { title: "t".into(), kb: kb.map(String::from), at, operation: op, result, hresult: 0 }
    }

    #[test]
    fn last_update_ignores_definitions_uninstalls_and_failures() {
        let now = 100 * 86_400;
        let h = vec![
            entry(Some("KB2267602"), now - 3600, "install", "ok"),
            entry(Some("KB4052623"), now - 7200, "install", "ok"),
            entry(Some("KB5066835"), now - 10 * 86_400, "install", "failed"),
            entry(Some("KB5066835"), now - 12 * 86_400, "uninstall", "ok"),
            entry(Some("KB5060842"), now - 20 * 86_400, "install", "ok"),
            entry(None, now - 40 * 86_400, "install", "ok"),
        ];
        assert_eq!(last_update_days(&h, now), Some(20));
        assert_eq!(last_update_days(&h[..4], now), None, "only definitions, failures and uninstalls on record");
        assert_eq!(last_update_days(&[], now), None);
        assert_eq!(last_update_days(&[entry(None, now + 99, "install", "ok")], now), Some(0), "a clock in the future is today, not a huge number");
    }

    #[test]
    fn drive_facts_count_by_status() {
        let drive = |status, needs_admin| DriveHealth {
            index: 0,
            model: "X".into(),
            bus: "sata".into(),
            key: "k".into(),
            needs_admin,
            nvme: None,
            ata: Vec::new(),
            status,
            findings: Vec::new(),
        };
        let f = drive_facts(&[drive(Status::Good, false), drive(Status::Caution, false), drive(Status::Bad, false), drive(Status::Unknown, true)]);
        assert_eq!(f, DriveFacts { good: 1, problem: 2, unknown: 1, needs_admin: true });
        assert_eq!(drive_facts(&[]), DriveFacts::default());
    }

    #[test]
    fn every_fix_is_a_real_screen_and_every_row_has_its_texts_in_both_languages() {
        const SCREENS: &[&str] = &["cleanup", "boost", "mypc", "security", "network", "maintenance", "apps", "gpu"];
        let items = evaluate(&healthy());
        assert!(items.iter().all(|i| i.fix.is_none_or(|f| SCREENS.contains(&f))), "fix must be a route the shell knows");
        let pt = include_str!("../../../src/locales/pt-BR.ts");
        let en = include_str!("../../../src/locales/en.ts");
        for (lang, file) in [("pt-BR", pt), ("en", en)] {
            for id in IDS {
                for part in ["title", "ok", "bad"] {
                    assert!(file.contains(&format!("\"checkup.{id}.{part}\":")), "{lang} lacks checkup.{id}.{part}");
                }
            }
            for reason in REASONS {
                assert!(file.contains(&format!("\"checkup.unknown.{reason}\":")), "{lang} lacks checkup.unknown.{reason}");
            }
        }
    }

    /// Live: the whole check-up on this machine. Read-only; run with `--ignored`.
    #[test]
    #[ignore = "live: reads this machine (WMI, event logs, Windows Update history, network)"]
    fn live_run() {
        let r = run(&crate::hardware::profile());
        println!("{}", serde_json::to_string_pretty(&r).unwrap());
        assert_eq!(r.items.len(), IDS.len());
        assert_eq!(r.summary.ok + r.summary.attention + r.summary.unknown, 19);
    }
}
