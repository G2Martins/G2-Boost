//! Local advisor: deterministic findings (upgrades, temperatures, config, gaming tier) from
//! the hardware profile, the last monitor sample, WinSAT and Windows security state. Works
//! without any AI; the AI assistant and the MCP server get the same findings as context.
//! Every finding is a code + numbers; the UI owns the wording (i18n `advisor.<id>.*`).

use std::collections::BTreeMap;

use serde::Serialize;

use crate::firmware::{self, Firmware};
use crate::hardware::Profile;
use crate::monitor::Sample;
use crate::sensors;
use crate::system;
use crate::winsat::WinSat;
use crate::wmi;

/// Temperature limits under load (°C). The single source: the advisor findings use them, and the
/// context pack hands them to the in-app AI and the MCP client (`thermal_limits_c`).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Thermal {
    pub cpu_warn: f64,
    pub cpu_critical: f64,
    pub gpu_warn: u32,
    pub gpu_critical: u32,
}

pub const THERMAL: Thermal = Thermal { cpu_warn: 88.0, cpu_critical: 95.0, gpu_warn: 83, gpu_critical: 90 };

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warn,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Costs nothing (BIOS/Windows setting).
    FreeFix,
    /// Hardware purchase.
    Upgrade,
    Temperature,
    Security,
    Maintenance,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Finding {
    pub id: &'static str,
    pub severity: Severity,
    pub kind: Kind,
    /// Values interpolated into the i18n text (`{rated}`, `{free_gb}` …).
    pub params: BTreeMap<&'static str, String>,
}

/// Coarse gaming class from the GPU model (heuristic, no FPS numbers — those come from sourced
/// benchmarks through the assistant). `entry` < `mainstream` < `high` < `enthusiast`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Integrated,
    Entry,
    Mainstream,
    High,
    Enthusiast,
}

#[derive(Debug, Clone, Serialize)]
pub struct GamingClass {
    pub tier: Option<Tier>,
    /// "1080p" / "1440p" / "4k": highest resolution the tier usually drives at high settings.
    pub target: Option<&'static str>,
    /// `cpu` when the CPU is clearly the weaker side (few cores / old gen), else `gpu` or None.
    pub likely_bottleneck: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub gaming: GamingClass,
}

/// Extra facts the rules need that are not in `Profile`.
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub system_free_bytes: Option<u64>,
    pub system_total_bytes: Option<u64>,
    /// Core Isolation / Memory Integrity running.
    pub hvci_running: Option<bool>,
    pub sample: Option<Sample>,
    pub winsat: Option<WinSat>,
    /// Days since the GPU driver date (when known).
    pub gpu_driver_age_days: Option<u32>,
    pub firmware: Option<Firmware>,
    /// CPU temperature from HWiNFO / LibreHardwareMonitor when one of them is running.
    pub cpu_temp_c: Option<f64>,
    /// Remote access and exposure (cheap subset, `exposure::quick`).
    pub exposure: Option<crate::exposure::Report>,
    /// Link power saving of the adapter carrying internet traffic.
    pub nic_power: Option<crate::nicpower::NicPower>,
    /// Firmware/Windows requirements of the installed games' anti-cheats.
    pub anticheat: Option<crate::anticheat::Readiness>,
}

/// Gather everything the rules need for this machine (no admin, ~1 s).
pub fn env(p: &Profile, sample: Option<Sample>, winsat: Option<WinSat>) -> Env {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let sys = system::list_drives().into_iter().find(|d| d.letter.eq_ignore_ascii_case(&drive));
    let gpu_driver = p.gpus.first().and_then(|g| g.driver.clone());
    let cpu_temp_c = sample.as_ref().and_then(|s| s.cpu_temp_c).or_else(|| sensors::read().and_then(|x| x.cpu_temp_c));
    let fw = firmware::read(p);
    Env {
        system_free_bytes: sys.as_ref().map(|d| d.free_bytes),
        system_total_bytes: sys.as_ref().map(|d| d.total_bytes),
        hvci_running: fw.hvci_running, // same Win32_DeviceGuard read as `hvci_running()`
        sample,
        winsat,
        gpu_driver_age_days: gpu_driver.as_deref().and_then(driver_age_days),
        anticheat: Some(crate::anticheat::readiness(p, &fw)),
        firmware: Some(fw),
        cpu_temp_c,
        exposure: Some(crate::exposure::quick()),
        nic_power: crate::nicpower::primary(),
    }
}

/// `Win32_DeviceGuard.SecurityServicesRunning` contains 2 when HVCI (memory integrity) is running.
pub fn hvci_running() -> Option<bool> {
    let rows = wmi::query(r"ROOT\Microsoft\Windows\DeviceGuard", "SELECT SecurityServicesRunning FROM Win32_DeviceGuard", &["SecurityServicesRunning"]).ok()?;
    match rows.first()?.get("SecurityServicesRunning")? {
        wmi::WmiValue::IntList(v) => Some(v.contains(&2)),
        _ => None,
    }
}

/// "32.0.16.1714 (2026-07-20)" → days since 2026-07-20.
pub(crate) fn driver_age_days(driver: &str) -> Option<u32> {
    let start = driver.find('(')? + 1;
    let date = driver.get(start..start + 10)?;
    let mut it = date.split('-');
    let (y, m, d): (i64, i64, i64) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    let days = days_from_civil(y, m, d);
    let today = (crate::registry::now_unix() / 86_400) as i64;
    u32::try_from(today - days).ok()
}

/// Howard Hinnant's days-from-civil (proleptic Gregorian), days since 1970-01-01.
pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// GPU model → tier. Desktop naming only; laptop/OEM variants fall back to the nearest bucket.
pub fn gpu_tier(name: &str) -> Option<Tier> {
    let n = name.to_ascii_uppercase();
    let num = |prefix: &str| -> Option<u32> {
        let i = n.find(prefix)? + prefix.len();
        let digits: String = n[i..].trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse().ok()
    };
    if n.contains("MICROSOFT BASIC") {
        return None;
    }
    if let Some(m) = num("RTX") {
        // 2050…5090: generation = thousands, class = last two digits.
        let (generation, class) = (m / 1000, m % 100);
        let base = match class {
            0..=50 => Tier::Entry,
            51..=60 => Tier::Mainstream,
            61..=70 => Tier::High,
            _ => Tier::Enthusiast,
        };
        return Some(if generation <= 2 { step_down(base) } else { base });
    }
    if let Some(m) = num("GTX") {
        // GTX 1070/1080 and 16-series are entry-level today; anything older is below that.
        return Some(if m >= 1070 { Tier::Entry } else { Tier::Integrated });
    }
    if let Some(m) = num("RX") {
        // RX 5500…7900: generation = thousands, class = hundreds. RX 9060/9070 switched to
        // NVIDIA-style two-digit classes.
        let generation = m / 1000;
        let class = if generation >= 9 { (m % 100) / 10 } else { (m / 100) % 10 };
        let base = match class {
            0..=5 => Tier::Entry,
            6 => Tier::Mainstream,
            7 | 8 => Tier::High,
            _ => Tier::Enthusiast,
        };
        return Some(if generation <= 5 { step_down(base) } else { base });
    }
    if n.contains("ARC") {
        return Some(if n.contains("A3") || n.contains("A5") { Tier::Entry } else { Tier::Mainstream });
    }
    if n.contains("INTEL") || n.contains("RADEON(TM) GRAPHICS") || n.contains("RADEON GRAPHICS") || n.contains("VEGA") {
        return Some(Tier::Integrated);
    }
    None
}

fn step_down(t: Tier) -> Tier {
    match t {
        Tier::Enthusiast => Tier::High,
        Tier::High => Tier::Mainstream,
        Tier::Mainstream => Tier::Entry,
        _ => Tier::Integrated,
    }
}

pub fn gaming_class(p: &Profile) -> GamingClass {
    let tier = p.gpus.iter().filter_map(|g| gpu_tier(&g.name)).max();
    let target = tier.map(|t| match t {
        Tier::Integrated | Tier::Entry => "1080p",
        Tier::Mainstream => "1080p",
        Tier::High => "1440p",
        Tier::Enthusiast => "4k",
    });
    let weak_cpu = p.cores_physical < 6 || p.cores_logical < 8;
    let likely_bottleneck = match (tier, weak_cpu) {
        (Some(t), true) if t >= Tier::Mainstream => Some("cpu"),
        (Some(_), _) => Some("gpu"),
        _ => None,
    };
    GamingClass { tier, target, likely_bottleneck }
}

fn finding(id: &'static str, severity: Severity, kind: Kind, params: &[(&'static str, String)]) -> Finding {
    Finding { id, severity, kind, params: params.iter().cloned().collect() }
}

/// All rules. Pure over (`Profile`, `Env`) so it is testable with fake machines.
pub fn analyze(p: &Profile, e: &Env) -> Report {
    let mut f = Vec::new();
    let gib = |b: u64| format!("{:.0}", b as f64 / (1u64 << 30) as f64);

    // --- memory ---
    let dimms: Vec<_> = p.memory.iter().filter(|m| m.size_bytes > 0).collect();
    if p.memory_below_rated
        && let Some(m) = dimms.iter().find(|m| m.rated_mts.is_some_and(|r| m.configured_mts > 0 && m.configured_mts < r))
    {
        f.push(finding("xmp_off", Severity::Warn, Kind::FreeFix, &[("configured", m.configured_mts.to_string()), ("rated", m.rated_mts.unwrap_or(0).to_string())]));
    }
    if dimms.len() == 1 {
        f.push(finding("single_channel", Severity::Warn, Kind::Upgrade, &[("size", gib(dimms[0].size_bytes))]));
    }
    let ram_gb = p.ram_bytes as f64 / (1u64 << 30) as f64;
    if ram_gb > 0.0 && ram_gb < 15.5 {
        f.push(finding("ram_low", if ram_gb < 7.5 { Severity::Critical } else { Severity::Warn }, Kind::Upgrade, &[("total", format!("{ram_gb:.0}"))]));
    }

    // --- storage ---
    let sys_disk = p.disks.iter().find(|d| d.system);
    if let Some(d) = sys_disk {
        if d.ssd == Some(false) {
            f.push(finding("hdd_system", Severity::Critical, Kind::Upgrade, &[("model", d.model.clone())]));
        } else if d.bus == "sata" && d.ssd == Some(true) && p.disks.iter().any(|x| x.bus == "nvme" && x.ssd != Some(false)) {
            let nvme = p.disks.iter().find(|x| x.bus == "nvme").map(|x| x.model.clone()).unwrap_or_default();
            // A small SATA SSD that is also short on room is where Windows stalls on write bursts
            // (updates, shader and browser caches) while the fast drive sits idle: say it louder.
            let tight = matches!((e.system_free_bytes, e.system_total_bytes), (Some(free), Some(total)) if total > 0 && free * 5 < total);
            f.push(finding("sata_system_with_nvme", if tight { Severity::Warn } else { Severity::Info }, Kind::FreeFix, &[("sata", d.model.clone()), ("nvme", nvme)]));
        }
    }
    if let Some(n) = e.nic_power.as_ref().filter(|n| !n.on.is_empty()) {
        f.push(finding("nic_power_saving", Severity::Info, Kind::FreeFix, &[("adapter", n.name.clone()), ("count", n.on.len().to_string())]));
    }
    if let (Some(free), Some(total)) = (e.system_free_bytes, e.system_total_bytes)
        && total > 0
    {
        let pct = free as f64 * 100.0 / total as f64;
        if pct < 15.0 || free < 20 * (1u64 << 30) {
            f.push(finding("system_disk_full", if pct < 8.0 { Severity::Critical } else { Severity::Warn }, Kind::Maintenance, &[("free_gb", gib(free)), ("pct", format!("{pct:.0}"))]));
        }
    }

    // --- GPU / display ---
    if let Some(g) = p.gpus.iter().max_by_key(|g| g.vram_bytes) {
        let vram = g.vram_bytes as f64 / (1u64 << 30) as f64;
        let max_w = p.monitors.iter().map(|m| m.width).max().unwrap_or(0);
        if vram > 0.5 && vram < 5.5 {
            f.push(finding("vram_low", Severity::Warn, Kind::Upgrade, &[("vram", format!("{vram:.0}"))]));
        } else if vram > 0.5 && vram < 11.5 && max_w >= 2560 {
            f.push(finding("vram_high_res", Severity::Info, Kind::Upgrade, &[("vram", format!("{vram:.0}")), ("width", max_w.to_string())]));
        }
        if let Some(days) = e.gpu_driver_age_days
            && days > 180
        {
            f.push(finding("gpu_driver_old", Severity::Info, Kind::Maintenance, &[("days", days.to_string())]));
        }
    }
    for m in &p.monitors {
        if m.max_refresh_hz as f32 > m.refresh_hz + 5.0 && m.refresh_hz > 0.0 {
            f.push(finding("refresh_below_max", Severity::Warn, Kind::FreeFix, &[("monitor", m.name.clone()), ("current", format!("{:.0}", m.refresh_hz)), ("max", m.max_refresh_hz.to_string())]));
        }
    }

    // --- temperatures (what we can read without a kernel driver) ---
    if let Some(s) = &e.sample
        && let Some(t) = s.gpu_temp_c
    {
        if t >= THERMAL.gpu_critical {
            f.push(finding("gpu_hot", Severity::Critical, Kind::Temperature, &[("temp", t.to_string())]));
        } else if t >= THERMAL.gpu_warn {
            f.push(finding("gpu_hot", Severity::Warn, Kind::Temperature, &[("temp", t.to_string())]));
        }
    }

    // --- security / anti-cheat ---
    if p.secure_boot == Some(false) {
        f.push(finding("secure_boot_off", Severity::Warn, Kind::Security, &[]));
    }
    if p.tpm.as_deref() != Some("2.0") {
        f.push(finding("tpm_missing", Severity::Warn, Kind::Security, &[]));
    }
    // An installed game whose anti-cheat uses memory integrity: never suggest turning it off.
    let ac = e.anticheat.as_ref();
    match (e.hvci_running, ac.and_then(|r| r.vbs_games.first())) {
        (Some(true), Some(game)) => f.push(finding("hvci_needed", Severity::Info, Kind::Security, &[("game", game.clone())])),
        (Some(true), None) => f.push(finding("hvci_on", Severity::Info, Kind::Security, &[])),
        _ => {}
    }
    if let Some(r) = ac {
        use crate::anticheat::{Check, State};
        let missing = |c: Check| r.checks.iter().any(|x| x.id == c && x.state == State::Missing);
        for t in r.titles.iter().filter(|t| t.ready == Some(false)) {
            if let Some(game) = &t.installed {
                f.push(finding("anticheat_blocked", Severity::Warn, Kind::Security, &[("game", game.clone()), ("count", t.missing.len().to_string())]));
            }
        }
        let wants_iommu = r.titles.iter().find(|t| t.installed.is_some() && t.recommended.iter().chain(t.on_demand).any(|c| *c == Check::Iommu));
        if missing(Check::Iommu)
            && let Some(game) = wants_iommu.and_then(|t| t.installed.clone())
        {
            f.push(finding("iommu_off", Severity::Info, Kind::FreeFix, &[("game", game)]));
        }
        if missing(Check::Blocklist) {
            f.push(finding("vuln_driver_blocklist_off", Severity::Warn, Kind::Security, &[]));
        }
    }

    if let Some(t) = e.cpu_temp_c {
        if t >= THERMAL.cpu_critical {
            f.push(finding("cpu_hot", Severity::Critical, Kind::Temperature, &[("temp", format!("{t:.0}"))]));
        } else if t >= THERMAL.cpu_warn {
            f.push(finding("cpu_hot", Severity::Warn, Kind::Temperature, &[("temp", format!("{t:.0}"))]));
        }
    }

    // --- firmware / BIOS ---
    if let Some(fw) = &e.firmware {
        if fw.boot_mode == Some("legacy") {
            f.push(finding("legacy_boot", Severity::Warn, Kind::Security, &[]));
        }
        // Resizable BAR matters on RTX 30+ / RX 6000+; the card reports a 256 MB BAR when it is off.
        let modern_gpu = p.gpus.iter().any(|g| {
            let n = g.name.to_ascii_uppercase();
            ["RTX 30", "RTX 40", "RTX 50", "RX 6", "RX 7", "RX 9", "ARC"].iter().any(|k| n.contains(k))
        });
        if fw.resizable_bar == Some(false) && modern_gpu {
            f.push(finding("rebar_off", Severity::Info, Kind::FreeFix, &[("bar_mb", fw.bar1_mb.unwrap_or(256).to_string())]));
        }
        if let Some(days) = fw.bios_age_days
            && days > 3 * 365
        {
            f.push(finding("bios_old", Severity::Info, Kind::Maintenance, &[("years", format!("{:.0}", days as f64 / 365.0)), ("version", fw.bios_version.clone().unwrap_or_default())]));
        }
    }

    // --- WinSAT ---
    if let Some(w) = &e.winsat {
        if w.state == 2 {
            f.push(finding("winsat_stale", Severity::Info, Kind::Maintenance, &[]));
        } else if w.state == 3 || w.base.is_none() {
            f.push(finding("winsat_never", Severity::Info, Kind::Maintenance, &[]));
        } else if let (Some(base), Some(disk)) = (w.base, w.disk)
            && disk <= base + 0.05
            && disk < 7.0
        {
            f.push(finding("winsat_disk_weakest", Severity::Info, Kind::Upgrade, &[("disk", format!("{disk:.1}"))]));
        }
    }

    if let Some(x) = &e.exposure {
        f.extend(crate::exposure::advisor_findings(x));
    }

    f.sort_by_key(|x| std::cmp::Reverse(x.severity));
    Report { findings: f, gaming: gaming_class(p) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{CpuDetail, Disk, Gpu, MemoryModule, MonitorInfo};

    fn dimm(size_gb: u64, configured: u32, rated: Option<u32>) -> MemoryModule {
        MemoryModule {
            slot: "A1".into(),
            bank: "B0".into(),
            size_bytes: size_gb << 30,
            speed_mts: 2133,
            configured_mts: configured,
            rated_mts: rated,
            kind: "DDR4".into(),
            form_factor: "DIMM".into(),
            manufacturer: "X".into(),
            part_number: "Y".into(),
        }
    }

    fn machine() -> Profile {
        Profile {
            computer: "PC".into(),
            os: "Windows 11".into(),
            cpu: "i5-10400".into(),
            cpu_detail: CpuDetail::default(),
            cores_physical: 6,
            cores_logical: 12,
            ram_bytes: 32 << 30,
            memory: vec![dimm(16, 3200, Some(3200)), dimm(16, 3200, Some(3200))],
            memory_slots: Some(4),
            memory_below_rated: false,
            uptime_secs: 10,
            gpus: vec![Gpu { name: "NVIDIA GeForce RTX 3050".into(), vram_bytes: 8 << 30, vendor: "nvidia", driver: Some("32.0.16.1714 (2026-07-20)".into()), pcie: None, power_limit_w: None }],
            monitors: vec![MonitorInfo { name: "27G2G4".into(), width: 1920, height: 1080, refresh_hz: 120.0, max_refresh_hz: 144 }],
            board: None,
            bios: None,
            system_model: None,
            disks: vec![
                Disk { index: 0, model: "KINGSTON SA400".into(), bus: "sata", size_bytes: 240 << 30, ssd: Some(true), system: true },
                Disk { index: 1, model: "XPG GAMMIX S70".into(), bus: "nvme", size_bytes: 1000 << 30, ssd: Some(true), system: false },
            ],
            system_disk_ssd: Some(true),
            secure_boot: Some(true),
            tpm: Some("2.0".into()),
            laptop: false,
            elevated: false,
        }
    }

    fn ids(r: &Report) -> Vec<&'static str> {
        r.findings.iter().map(|f| f.id).collect()
    }

    #[test]
    fn owner_machine_findings() {
        let e = Env { system_free_bytes: Some(38 << 30), system_total_bytes: Some(223 << 30), hvci_running: Some(true), ..Default::default() };
        let r = analyze(&machine(), &e);
        let got = ids(&r);
        assert!(got.contains(&"sata_system_with_nvme"));
        assert!(got.contains(&"refresh_below_max"));
        assert!(got.contains(&"hvci_on"));
        assert!(!got.contains(&"xmp_off") && !got.contains(&"single_channel") && !got.contains(&"ram_low"));
        assert!(!got.contains(&"system_disk_full"), "17 % free and 38 GB is above both thresholds");
        assert_eq!(r.gaming.tier, Some(Tier::Entry));
        assert_eq!(r.gaming.target, Some("1080p"));
        // Sorted by severity: the 144 Hz warning before infos.
        assert_eq!(r.findings[0].severity, Severity::Warn);
        let sev = |r: &Report, id: &str| r.findings.iter().find(|f| f.id == id).map(|f| f.severity);
        assert_eq!(sev(&r, "sata_system_with_nvme"), Some(Severity::Warn), "17 % free on a small SATA: louder");
        assert!(!got.contains(&"nic_power_saving"), "unknown adapter: nothing said");
        let roomy = Env { system_free_bytes: Some(100 << 30), ..e.clone() };
        assert_eq!(sev(&analyze(&machine(), &roomy), "sata_system_with_nvme"), Some(Severity::Info), "45 % free: still a tip");

        // 2026-11-14 on the same PC: 31 GB free of 223 and the Realtek's green modes on.
        let nic = crate::nicpower::NicPower { guid: "{x}".into(), name: "Realtek Gaming 2.5GbE".into(), on: vec!["EnableGreenEthernet", "PowerSavingMode", "GigaLite"] };
        let e = Env { system_free_bytes: Some(31 << 30), nic_power: Some(nic.clone()), ..e };
        let r = analyze(&machine(), &e);
        assert_eq!(sev(&r, "sata_system_with_nvme"), Some(Severity::Warn), "14 % free on the SATA: louder");
        let n = r.findings.iter().find(|f| f.id == "nic_power_saving").expect("green modes on");
        assert_eq!(n.params.get("count").map(String::as_str), Some("3"));
        let off = Env { nic_power: Some(crate::nicpower::NicPower { on: vec![], ..nic }), ..e };
        assert!(!ids(&analyze(&machine(), &off)).contains(&"nic_power_saving"), "all off: nothing to say");
    }

    #[test]
    fn weak_machine_findings() {
        let mut p = machine();
        p.memory = vec![dimm(8, 2133, Some(3200))];
        p.memory_below_rated = true;
        p.ram_bytes = 8 << 30;
        p.disks[0].ssd = Some(false);
        p.secure_boot = Some(false);
        p.tpm = None;
        p.gpus[0].vram_bytes = 4 << 30;
        let e = Env {
            system_free_bytes: Some(10 << 30),
            system_total_bytes: Some(500 << 30),
            sample: Some(Sample { gpu_temp_c: Some(91), ..Default::default() }),
            winsat: Some(WinSat { state: 2, ..Default::default() }),
            gpu_driver_age_days: Some(400),
            firmware: Some(Firmware { boot_mode: Some("legacy"), resizable_bar: Some(false), bar1_mb: Some(256), bios_age_days: Some(2000), ..Default::default() }),
            cpu_temp_c: Some(96.0),
            ..Default::default()
        };
        let got = ids(&analyze(&p, &e));
        for id in [
            "xmp_off", "single_channel", "ram_low", "hdd_system", "system_disk_full", "vram_low", "gpu_driver_old", "gpu_hot", "secure_boot_off", "tpm_missing", "winsat_stale",
            "cpu_hot", "legacy_boot", "rebar_off", "bios_old",
        ] {
            assert!(got.contains(&id), "{id} missing in {got:?}");
        }
        assert_eq!(analyze(&p, &e).findings[0].severity, Severity::Critical);
    }

    #[test]
    fn anticheat_findings() {
        use crate::anticheat::{Facts, evaluate};
        use crate::firmware::Vendor;
        // The owner's PC: memory integrity on, VT-d off, Valorant installed.
        let fw = Facts {
            tpm: Some("2.0".into()),
            secure_boot: Some(true),
            boot_mode: Some("uefi"),
            system_gpt: Some(true),
            vbs: Some(true),
            hvci: Some(true),
            iommu: Some(false),
            virtualization: true,
            build: 26_200,
            blocklist_reg: Some(1),
            ..Default::default()
        };
        let with = |f: &Facts, games: &[&'static str]| {
            let found: Vec<_> = games.iter().map(|g| (*g, g.to_uppercase())).collect();
            Env { hvci_running: f.hvci, anticheat: Some(evaluate(f, &found, Vendor::Msi, false)), ..Default::default() }
        };
        let r = analyze(&machine(), &with(&fw, &["valorant"]));
        let got = ids(&r);
        assert!(got.contains(&"hvci_needed") && !got.contains(&"hvci_on"), "never suggest turning it off: {got:?}");
        assert!(got.contains(&"iommu_off"));
        assert!(!got.contains(&"anticheat_blocked") && !got.contains(&"vuln_driver_blocklist_off"));
        let hv = r.findings.iter().find(|f| f.id == "hvci_needed").unwrap();
        assert_eq!(hv.params.get("game").map(String::as_str), Some("VALORANT"));

        // Only Battlefield 6 (no VBS use): the usual memory integrity tip, no IOMMU talk.
        let got = ids(&analyze(&machine(), &with(&fw, &["bf6"])));
        assert!(got.contains(&"hvci_on") && !got.contains(&"hvci_needed") && !got.contains(&"iommu_off"), "{got:?}");

        // Secure Boot off + blocklist off without memory integrity.
        let bad = Facts { secure_boot: Some(false), hvci: Some(false), blocklist_reg: Some(0), ..fw };
        let r = analyze(&machine(), &with(&bad, &["valorant", "cod"]));
        let blocked: Vec<_> = r.findings.iter().filter(|f| f.id == "anticheat_blocked").collect();
        assert_eq!(blocked.len(), 2);
        assert_eq!(blocked[0].params.get("count").map(String::as_str), Some("1"));
        assert!(ids(&r).contains(&"vuln_driver_blocklist_off"));
    }

    #[test]
    fn gpu_tiers() {
        assert_eq!(gpu_tier("NVIDIA GeForce RTX 3050"), Some(Tier::Entry));
        assert_eq!(gpu_tier("NVIDIA GeForce RTX 4060 Ti"), Some(Tier::Mainstream));
        assert_eq!(gpu_tier("NVIDIA GeForce RTX 5070"), Some(Tier::High));
        assert_eq!(gpu_tier("NVIDIA GeForce RTX 4090"), Some(Tier::Enthusiast));
        assert_eq!(gpu_tier("NVIDIA GeForce RTX 2080 Ti"), Some(Tier::High));
        assert_eq!(gpu_tier("NVIDIA GeForce GTX 1660 SUPER"), Some(Tier::Entry));
        assert_eq!(gpu_tier("AMD Radeon RX 7800 XT"), Some(Tier::High));
        assert_eq!(gpu_tier("AMD Radeon RX 9070 XT"), Some(Tier::High));
        assert_eq!(gpu_tier("AMD Radeon RX 6600"), Some(Tier::Mainstream));
        assert_eq!(gpu_tier("Intel(R) UHD Graphics 630"), Some(Tier::Integrated));
        assert_eq!(gpu_tier("Intel(R) Arc(TM) B580 Graphics"), Some(Tier::Mainstream));
        assert_eq!(gpu_tier("Microsoft Basic Render Driver"), None);
    }

    #[test]
    fn dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert!(driver_age_days("1.0 (2020-01-01)").is_some_and(|d| d > 2000));
        assert!(driver_age_days("no date").is_none());
    }
}
