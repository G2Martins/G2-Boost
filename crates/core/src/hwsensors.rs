//! Hardware sensor tree for the "Monitor de hardware" page: device → group → sensor, one
//! snapshot per second. Everything is read in user mode, no driver of ours:
//! PDH (CPU per thread and clock, RAPL power, disks), NVML (NVIDIA GPU), `GetPerformanceInfo`
//! (memory), `GetIfTable2` (network), SMART through `smart.rs` (disk temperature) and, when the
//! user already runs HWiNFO / LibreHardwareMonitor, every reading they publish (temperatures,
//! fans, voltages) grouped by their own device names.
//!
//! CPU temperature exists only through those tools (owner's decision: no PawnIO, no driver).
//!
//! The tree is built by pure functions over plain numbers (tested); `Sampler` only gathers.
//! Labels of built-in nodes are i18n keys (`hw.d.<key>`, `hw.g.<key>`, `hw.s.<key>`) so the UI
//! owns the wording; names that come from the machine or from an external tool stay as data.

use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, IF_TYPE_SOFTWARE_LOOPBACK, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::{IfOperStatusUp, MediaConnectStateConnected};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Performance::{PDH_HQUERY, PdhCloseQuery, PdhCollectQueryData, PdhOpenQueryW};
use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};
use windows::core::{PCWSTR, w};

use crate::hardware;
use crate::monitor::{Counter, add_counter, values};
use crate::nvml;
use crate::registry::{self, Root, Value, now_unix};
use crate::sensors::{self, Kind, Reading, Source};
use crate::smart::{self, DriveHealth};

/* ---------------- model ---------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Cpu,
    Gpu,
    Memory,
    Disk,
    Network,
    /// Published by HWiNFO / LibreHardwareMonitor under their own device names.
    External,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sensor {
    /// Stable across snapshots (the UI keeps min/max by it): `cpu/load/thread3`, `gpu0/temp/gpu_core`.
    pub id: String,
    /// English fallback for built-in sensors; the tool's own label for external readings.
    pub label: String,
    /// i18n key suffix (`hw.s.<key>`) for built-in sensors; `None` = show `label` as is.
    pub key: Option<&'static str>,
    /// Index of numbered sensors (thread 3 → `n = 3`), the `{n}` of the translated label.
    pub n: Option<u32>,
    pub value: f64,
    /// `%`, `°C`, `MHz`, `W`, `V`, `RPM`, `B`, `B/s`, `Mb/s` for built-ins; the tool's unit otherwise.
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Group {
    pub id: String,
    pub label: String,
    pub key: Option<&'static str>,
    pub sensors: Vec<Sensor>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Device {
    pub id: String,
    /// The machine's own name (CPU model, disk model, adapter alias) or the tool's device name.
    pub label: String,
    pub key: Option<&'static str>,
    pub n: Option<u32>,
    pub kind: DeviceKind,
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    /// 1, 2, 3… per sampler run.
    pub seq: u32,
    pub ts: u64,
    pub devices: Vec<Device>,
    /// Sensors in the whole tree.
    pub count: u32,
    /// Which tool feeds the external devices (None = none running).
    pub external: Option<Source>,
    /// A CPU temperature is among the readings (the UI explains how to get one when not).
    pub cpu_temp: bool,
}

/// Every i18n key the built-in tree can carry (checked against both locales by a test).
pub const DEVICE_KEYS: &[&str] = &["cpu", "gpu", "memory", "disk", "other"];
pub const GROUP_KEYS: &[&str] = &[
    "load", "clock", "power", "temp", "volt", "fan", "usage", "current", "other", "vram", "physical", "commit", "activity", "traffic",
    "link",
];
pub const SENSOR_KEYS: &[&str] = &[
    "total", "thread", "pkg", "pp0", "pp1", "dram", "gpu_core", "gpu", "mem_ctrl", "clock_core", "clock_mem", "clock_video", "draw",
    "limit", "limit_pct", "vram_used", "vram_total", "vram_pct", "fan", "mem_used_pct", "mem_used", "mem_avail", "mem_cache",
    "commit", "commit_limit", "commit_pct", "active", "read", "write", "disk_temp", "down", "up", "link",
];

/* ---------------- raw inputs (plain numbers) ---------------- */

#[derive(Debug, Clone, Copy, Default)]
pub struct ThreadRaw {
    pub load: Option<f64>,
    pub mhz: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rapl {
    Package,
    Cores,
    Graphics,
    Dram,
}

impl Rapl {
    fn key(self) -> &'static str {
        match self {
            Rapl::Package => "pkg",
            Rapl::Cores => "pp0",
            Rapl::Graphics => "pp1",
            Rapl::Dram => "dram",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Rapl::Package => "Package",
            Rapl::Cores => "Cores",
            Rapl::Graphics => "Integrated graphics",
            Rapl::Dram => "Memory (DRAM)",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CpuRaw {
    pub name: String,
    pub load: Option<f64>,
    pub mhz: Option<f64>,
    /// In running order: index = thread number shown to the user.
    pub threads: Vec<ThreadRaw>,
    pub rapl: Vec<(Rapl, f64)>,
}

#[derive(Debug, Clone, Default)]
pub struct GpuRaw {
    pub name: String,
    pub temp_c: Option<f64>,
    pub core_mhz: Option<f64>,
    pub mem_mhz: Option<f64>,
    pub video_mhz: Option<f64>,
    pub power_w: Option<f64>,
    pub power_limit_w: Option<f64>,
    pub load: Option<f64>,
    pub mem_ctrl: Option<f64>,
    pub vram_used: Option<f64>,
    pub vram_total: Option<f64>,
    pub fan_pct: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MemRaw {
    pub total: u64,
    pub avail: u64,
    pub cache: u64,
    pub commit: u64,
    pub commit_limit: u64,
}

#[derive(Debug, Clone, Default)]
pub struct DiskRaw {
    pub index: u32,
    pub name: String,
    pub active: Option<f64>,
    pub read_bps: Option<f64>,
    pub write_bps: Option<f64>,
    pub temp_c: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct NetRaw {
    /// `InterfaceIndex`.
    pub id: u32,
    pub alias: String,
    pub down_bps: Option<f64>,
    pub up_bps: Option<f64>,
    pub link_mbps: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct Parts {
    pub cpu: Option<CpuRaw>,
    pub gpu: Option<GpuRaw>,
    pub mem: Option<MemRaw>,
    pub disks: Vec<DiskRaw>,
    pub nets: Vec<NetRaw>,
}

/* ---------------- pure tree builders ---------------- */

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn clamp100(v: f64) -> f64 {
    v.clamp(0.0, 100.0)
}

/// A built-in sensor; missing and non-finite values produce `None` (the sensor is not listed).
fn sensor(gid: &str, key: &'static str, label: &str, v: Option<f64>, unit: &str) -> Option<Sensor> {
    let value = v.filter(|x| x.is_finite())?;
    Some(Sensor { id: format!("{gid}/{key}"), label: label.into(), key: Some(key), n: None, value: round3(value), unit: unit.into() })
}

fn numbered(gid: &str, key: &'static str, label: &str, n: u32, v: Option<f64>, unit: &str) -> Option<Sensor> {
    let mut s = sensor(gid, key, label, v, unit)?;
    s.id = format!("{gid}/{key}{n}");
    s.label = format!("{label} {n}");
    s.n = Some(n);
    Some(s)
}

fn group(did: &str, key: &'static str, label: &str, sensors: impl IntoIterator<Item = Option<Sensor>>) -> Option<Group> {
    let sensors: Vec<Sensor> = sensors.into_iter().flatten().collect();
    (!sensors.is_empty()).then(|| Group { id: format!("{did}/{key}"), label: label.into(), key: Some(key), sensors })
}

fn device(
    id: &str,
    label: &str,
    key: Option<&'static str>,
    n: Option<u32>,
    kind: DeviceKind,
    groups: impl IntoIterator<Item = Option<Group>>,
) -> Option<Device> {
    let groups: Vec<Group> = groups.into_iter().flatten().collect();
    (!groups.is_empty()).then(|| Device { id: id.into(), label: label.into(), key, n, kind, groups })
}

fn percent_of(part: Option<f64>, whole: Option<f64>) -> Option<f64> {
    match (part, whole) {
        (Some(p), Some(w)) if w > 0.0 => Some(clamp100(p * 100.0 / w)),
        _ => None,
    }
}

pub fn cpu_device(r: &CpuRaw) -> Option<Device> {
    const D: &str = "cpu";
    let name = r.name.trim();
    let (load_g, clock_g, power_g) = ("cpu/load", "cpu/clock", "cpu/power");
    let load = group(
        D,
        "load",
        "Load",
        std::iter::once(sensor(load_g, "total", "Total", r.load.map(clamp100), "%")).chain(
            r.threads.iter().enumerate().map(|(i, t)| numbered(load_g, "thread", "Thread", i as u32, t.load.map(clamp100), "%")),
        ),
    );
    let clock = group(
        D,
        "clock",
        "Clock",
        std::iter::once(sensor(clock_g, "total", "Total", r.mhz.filter(|m| *m > 0.0), "MHz")).chain(
            r.threads.iter().enumerate().map(|(i, t)| numbered(clock_g, "thread", "Thread", i as u32, t.mhz.filter(|m| *m > 0.0), "MHz")),
        ),
    );
    let power = group(D, "power", "Power", r.rapl.iter().map(|(d, w)| sensor(power_g, d.key(), d.label(), Some(*w), "W")));
    device(D, name, name.is_empty().then_some("cpu"), None, DeviceKind::Cpu, [load, clock, power])
}

pub fn gpu_device(r: &GpuRaw) -> Option<Device> {
    const D: &str = "gpu0";
    let name = r.name.trim();
    let temp = group(D, "temp", "Temperatures", [sensor("gpu0/temp", "gpu_core", "GPU core", r.temp_c, "°C")]);
    let load = group(
        D,
        "load",
        "Load",
        [sensor("gpu0/load", "gpu", "GPU", r.load.map(clamp100), "%"), sensor("gpu0/load", "mem_ctrl", "Memory controller", r.mem_ctrl.map(clamp100), "%")],
    );
    let clock = group(
        D,
        "clock",
        "Clocks",
        [
            sensor("gpu0/clock", "clock_core", "Graphics", r.core_mhz, "MHz"),
            sensor("gpu0/clock", "clock_mem", "Memory", r.mem_mhz, "MHz"),
            sensor("gpu0/clock", "clock_video", "Video", r.video_mhz, "MHz"),
        ],
    );
    let power = group(
        D,
        "power",
        "Power",
        [
            sensor("gpu0/power", "draw", "Power draw", r.power_w, "W"),
            sensor("gpu0/power", "limit", "Power limit", r.power_limit_w.filter(|l| *l > 0.0), "W"),
            sensor("gpu0/power", "limit_pct", "Of the limit", percent_of(r.power_w, r.power_limit_w), "%"),
        ],
    );
    let vram = group(
        D,
        "vram",
        "Video memory",
        [
            sensor("gpu0/vram", "vram_used", "In use", r.vram_used, "B"),
            sensor("gpu0/vram", "vram_total", "Total", r.vram_total.filter(|t| *t > 0.0), "B"),
            sensor("gpu0/vram", "vram_pct", "Usage", percent_of(r.vram_used, r.vram_total), "%"),
        ],
    );
    let fan = group(D, "fan", "Fan", [sensor("gpu0/fan", "fan", "Fan", r.fan_pct.map(clamp100), "%")]);
    device(D, name, name.is_empty().then_some("gpu"), None, DeviceKind::Gpu, [temp, load, clock, power, vram, fan])
}

pub fn memory_device(r: &MemRaw) -> Option<Device> {
    if r.total == 0 {
        return None;
    }
    const D: &str = "mem";
    let (total, avail) = (r.total as f64, r.avail.min(r.total) as f64);
    let physical = group(
        D,
        "physical",
        "Physical",
        [
            sensor("mem/physical", "mem_used_pct", "Usage", Some((total - avail) * 100.0 / total), "%"),
            sensor("mem/physical", "mem_used", "In use", Some(total - avail), "B"),
            sensor("mem/physical", "mem_avail", "Available", Some(avail), "B"),
            sensor("mem/physical", "mem_cache", "System cache", Some(r.cache as f64), "B"),
        ],
    );
    let commit = group(
        D,
        "commit",
        "Committed",
        [
            sensor("mem/commit", "commit", "Committed", Some(r.commit as f64), "B"),
            sensor("mem/commit", "commit_limit", "Commit limit", Some(r.commit_limit as f64).filter(|l| *l > 0.0), "B"),
            sensor("mem/commit", "commit_pct", "Of the limit", percent_of(Some(r.commit as f64), Some(r.commit_limit as f64)), "%"),
        ],
    );
    device(D, "", Some("memory"), None, DeviceKind::Memory, [physical, commit])
}

pub fn disk_device(r: &DiskRaw) -> Option<Device> {
    let id = format!("disk{}", r.index);
    let activity = group(
        &id,
        "activity",
        "Activity",
        [
            sensor(&format!("{id}/activity"), "active", "Active time", r.active.map(clamp100), "%"),
            sensor(&format!("{id}/activity"), "read", "Read", r.read_bps.map(|v| v.max(0.0)), "B/s"),
            sensor(&format!("{id}/activity"), "write", "Write", r.write_bps.map(|v| v.max(0.0)), "B/s"),
        ],
    );
    let temp = group(&id, "temp", "Temperatures", [sensor(&format!("{id}/temp"), "disk_temp", "Temperature", r.temp_c, "°C")]);
    let name = r.name.trim();
    device(&id, name, name.is_empty().then_some("disk"), Some(r.index), DeviceKind::Disk, [activity, temp])
}

pub fn net_device(r: &NetRaw) -> Option<Device> {
    let id = format!("net{}", r.id);
    let traffic = group(
        &id,
        "traffic",
        "Traffic",
        [
            sensor(&format!("{id}/traffic"), "down", "Download", r.down_bps.map(|v| v.max(0.0)), "B/s"),
            sensor(&format!("{id}/traffic"), "up", "Upload", r.up_bps.map(|v| v.max(0.0)), "B/s"),
        ],
    );
    let link = group(&id, "link", "Link", [sensor(&format!("{id}/link"), "link", "Link speed", r.link_mbps.filter(|v| *v > 0.0), "Mb/s")]);
    let alias = r.alias.trim();
    device(&id, if alias.is_empty() { "?" } else { alias }, None, None, DeviceKind::Network, [traffic, link])
}

/* ---------------- external tools (HWiNFO / LibreHardwareMonitor) ---------------- */

/// Readings kept from the tools (a full HWiNFO board can publish a thousand).
pub const MAX_EXTERNAL: usize = 700;

/// Group order of an external device and the i18n key of each.
const EXT_GROUPS: &[(&str, &str)] = &[
    ("temp", "Temperatures"),
    ("volt", "Voltages"),
    ("fan", "Fans"),
    ("power", "Power"),
    ("clock", "Clocks"),
    ("usage", "Usage"),
    ("current", "Current"),
    ("other", "Other"),
];

fn ext_group(kind: Kind, unit: &str) -> &'static str {
    match kind {
        Kind::Temperature => "temp",
        Kind::Voltage => "volt",
        Kind::Fan => "fan",
        Kind::Power => "power",
        Kind::Clock => "clock",
        Kind::Other => match unit.trim() {
            "%" => "usage",
            "A" | "mA" => "current",
            _ => "other",
        },
    }
}

/// HWiNFO writes ANSI: the degree sign of "°C" can arrive as U+FFFD. Known kinds get their unit.
fn ext_unit(kind: Kind, raw: &str) -> String {
    match kind {
        Kind::Temperature => "°C".into(),
        Kind::Voltage => "V".into(),
        Kind::Fan => "RPM".into(),
        Kind::Power => "W".into(),
        Kind::Clock => "MHz".into(),
        Kind::Other => raw.trim().replace('\u{FFFD}', "°"),
    }
}

/// `CPU Package` → `cpu-package`: ASCII only, no path separators, bounded.
fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().take(60) {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let t = out.trim_matches('-');
    if t.is_empty() { "x".into() } else { t.chars().take(40).collect() }
}

/// `base`, or `base#2`, `base#3`… when already taken.
fn unique(base: String, used: &mut HashSet<String>) -> String {
    if used.insert(base.clone()) {
        return base;
    }
    (2u32..).map(|n| format!("{base}#{n}")).find(|c| used.insert(c.clone())).unwrap_or(base)
}

/// Readings of the external tool as devices: one per device name the tool uses, in the order it
/// lists them, with groups by kind. Ids are stable between calls while the tool's order is.
pub fn external_devices(readings: &[Reading]) -> Vec<Device> {
    let mut order: Vec<&str> = Vec::new();
    let mut by_dev: HashMap<&str, Vec<&Reading>> = HashMap::new();
    for r in readings.iter().filter(|r| r.value.is_finite()).take(MAX_EXTERNAL) {
        by_dev
            .entry(r.sensor.as_str())
            .or_insert_with(|| {
                order.push(r.sensor.as_str());
                Vec::new()
            })
            .push(r);
    }
    let mut used = HashSet::new();
    let mut out = Vec::new();
    for name in order {
        let rows = &by_dev[name];
        let did = unique(format!("ext/{}", slug(name)), &mut used);
        let mut groups = Vec::new();
        for (gkey, glabel) in EXT_GROUPS {
            let gid = format!("{did}/{gkey}");
            let sensors: Vec<Sensor> = rows
                .iter()
                .filter(|r| ext_group(r.kind, &r.unit) == *gkey)
                .map(|r| Sensor {
                    id: unique(format!("{gid}/{}", slug(&r.label)), &mut used),
                    label: r.label.clone(),
                    key: None,
                    n: None,
                    value: round3(r.value),
                    unit: ext_unit(r.kind, &r.unit),
                })
                .collect();
            if !sensors.is_empty() {
                // Group keys are shared with the built-in groups (`hw.g.temp`, `hw.g.fan`…).
                let key = GROUP_KEYS.iter().copied().find(|k| k == gkey);
                groups.push(Group { id: gid, label: (*glabel).into(), key, sensors });
            }
        }
        let name = name.trim();
        out.extend(device(&did, name, name.is_empty().then_some("other"), None, DeviceKind::External, groups.into_iter().map(Some)));
    }
    out
}

/// The whole tree: built-in devices first, then what the external tool publishes.
pub fn build(parts: &Parts, ext: Option<&[Reading]>) -> Vec<Device> {
    let mut v: Vec<Device> = Vec::new();
    v.extend(parts.cpu.as_ref().and_then(cpu_device));
    v.extend(parts.gpu.as_ref().and_then(gpu_device));
    v.extend(parts.mem.as_ref().and_then(memory_device));
    v.extend(parts.disks.iter().filter_map(disk_device));
    v.extend(parts.nets.iter().filter_map(net_device));
    if let Some(r) = ext {
        v.extend(external_devices(r));
    }
    v
}

pub fn count_sensors(devices: &[Device]) -> u32 {
    devices.iter().flat_map(|d| &d.groups).map(|g| g.sensors.len() as u32).sum()
}

/* ---------------- parsing PDH instance names (hostile-safe) ---------------- */

/// `Processor Information` instances are `group,index`; the totals (`0,_Total`, `_Total`) are not threads.
pub(crate) fn parse_thread(name: &str) -> Option<(u32, u32)> {
    let (g, i) = name.split_once(',')?;
    Some((g.parse().ok()?, i.parse().ok()?))
}

/// `PhysicalDisk` instances are `<disk number> <letters>`; `_Total` is not a disk.
pub(crate) fn parse_disk_index(name: &str) -> Option<u32> {
    name.split_whitespace().next()?.parse().ok()
}

/// `Energy Meter` instances of the first package: `RAPL_Package0_PKG`, `_PP0`, `_PP1`, `_DRAM`.
pub(crate) fn rapl_domain(name: &str) -> Option<Rapl> {
    let n = name.to_ascii_lowercase();
    if !n.contains("package0") {
        return None;
    }
    [("pkg", Rapl::Package), ("pp0", Rapl::Cores), ("pp1", Rapl::Graphics), ("dram", Rapl::Dram)]
        .into_iter()
        .find_map(|(suffix, d)| n.ends_with(suffix).then_some(d))
}

/// The counter reports milliwatts.
pub(crate) fn rapl_watts(raw: f64) -> f64 {
    raw / 1000.0
}

/// Disk temperature from a SMART read: NVMe composite, else ATA attribute 194 / 190 (low byte).
pub(crate) fn drive_temp(d: &DriveHealth) -> Option<f64> {
    let t = d
        .nvme
        .and_then(|n| n.temperature_c.map(f64::from))
        .or_else(|| d.ata.iter().find(|a| a.id == 194 || a.id == 190).map(|a| (a.raw & 0xFF) as f64))?;
    (1.0..=120.0).contains(&t).then_some(t)
}

/* ---------------- NVML extras (own loader: nvml.rs stays untouched while round 9 merges) ---------------- */

#[repr(C)]
#[derive(Default)]
struct NvUtilization {
    gpu: u32,
    memory: u32,
}

#[repr(C)]
#[derive(Default)]
struct NvMemory {
    total: u64,
    free: u64,
    used: u64,
}

type FnHandle = unsafe extern "C" fn(u32, *mut *mut c_void) -> i32;
type FnUtil = unsafe extern "C" fn(*mut c_void, *mut NvUtilization) -> i32;
type FnMem = unsafe extern "C" fn(*mut c_void, *mut NvMemory) -> i32;
type FnU32 = unsafe extern "C" fn(*mut c_void, *mut u32) -> i32;
type FnName = unsafe extern "C" fn(*mut c_void, *mut u8, u32) -> i32;

// ponytail: a second small NVML loader instead of new fields on `nvml::Nvml`; fold into nvml.rs after round 9.
struct NvExtra {
    device: *mut c_void,
    util: Option<FnUtil>,
    mem: Option<FnMem>,
    fan: Option<FnU32>,
    name: Option<FnName>,
}

// SAFETY: NVML is documented thread-safe; the device handle is an opaque, process-wide token.
unsafe impl Send for NvExtra {}
unsafe impl Sync for NvExtra {}

macro_rules! sym {
    ($h:expr, $name:literal, $ty:ty) => {
        // SAFETY: the symbol has the documented NVML C signature.
        GetProcAddress($h, windows::core::s!($name)).map(|f| std::mem::transmute::<_, $ty>(f))
    };
}

impl NvExtra {
    fn load() -> Option<Self> {
        nvml::get()?; // loads and initialises nvml.dll once
        // SAFETY: the module is already loaded by `nvml::get`; symbols resolved by name with the C ABI.
        unsafe {
            let h = GetModuleHandleW(w!("nvml.dll")).ok()?;
            let by_index: FnHandle = sym!(h, "nvmlDeviceGetHandleByIndex_v2", FnHandle)?;
            let mut device = null_mut();
            if by_index(0, &mut device) != 0 {
                return None;
            }
            Some(Self {
                device,
                util: sym!(h, "nvmlDeviceGetUtilizationRates", FnUtil),
                mem: sym!(h, "nvmlDeviceGetMemoryInfo", FnMem),
                fan: sym!(h, "nvmlDeviceGetFanSpeed", FnU32),
                name: sym!(h, "nvmlDeviceGetName", FnName),
            })
        }
    }

    /// (GPU %, memory controller %).
    fn utilization(&self) -> Option<(f64, f64)> {
        let f = self.util?;
        let mut u = NvUtilization::default();
        // SAFETY: valid device handle; struct matches nvmlUtilization_t.
        (unsafe { f(self.device, &mut u) } == 0).then_some((f64::from(u.gpu), f64::from(u.memory)))
    }

    /// (used, total) bytes of video memory.
    fn memory(&self) -> Option<(u64, u64)> {
        let f = self.mem?;
        let mut m = NvMemory::default();
        // SAFETY: valid device handle; struct matches nvmlMemory_t.
        (unsafe { f(self.device, &mut m) } == 0).then_some((m.used, m.total))
    }

    fn fan_percent(&self) -> Option<f64> {
        let f = self.fan?;
        let mut v = 0u32;
        // SAFETY: valid device handle; out-param.
        (unsafe { f(self.device, &mut v) } == 0).then_some(f64::from(v))
    }

    fn name(&self) -> Option<String> {
        let f = self.name?;
        let mut buf = [0u8; 96];
        // SAFETY: buffer of 96 bytes (NVML_DEVICE_NAME_V2_BUFFER_SIZE).
        if unsafe { f(self.device, buf.as_mut_ptr(), buf.len() as u32) } != 0 {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..end]).trim().to_string()).filter(|s| !s.is_empty())
    }
}

fn nv_extra() -> Option<&'static NvExtra> {
    static X: OnceLock<Option<NvExtra>> = OnceLock::new();
    X.get_or_init(NvExtra::load).as_ref()
}

/* ---------------- gathering ---------------- */

/// Disk temperatures change slowly, a SMART read can wake a sleeping drive and opens the raw
/// device (a pattern antivirus products watch): once a minute.
const SMART_EVERY: Duration = Duration::from_secs(60);

pub struct Sampler {
    query: PDH_HQUERY,
    cpu_util: Option<Counter>,
    cpu_perf: Option<Counter>,
    cpu_freq: Option<Counter>,
    rapl: Option<Counter>,
    disk_idle: Option<Counter>,
    disk_read: Option<Counter>,
    disk_write: Option<Counter>,
    gpu_util: Option<Counter>,
    gpu_mem: Option<Counter>,
    cpu_name: String,
    /// First DXGI GPU when NVML is absent (AMD / Intel): load and memory then come from PDH.
    gpu_fallback: Option<hardware::Gpu>,
    disk_names: HashMap<u32, String>,
    net_prev: HashMap<u32, (Instant, u64, u64)>,
    smart: Arc<Mutex<HashMap<u32, f64>>>,
    smart_at: Option<Instant>,
    smart_busy: Arc<AtomicBool>,
    ext: Option<(Source, Vec<Reading>)>,
    /// External reads are skipped until this tick after a slow one (hung provider).
    ext_skip_until: u32,
    tick: u32,
}

impl Sampler {
    /// Never fails: a counter that cannot be added just leaves its sensors out.
    pub fn new() -> Self {
        let mut query = PDH_HQUERY::default();
        // SAFETY: real-time query (no data source).
        let opened = unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } == 0;
        let add = |path: &str| if opened { add_counter(query, path) } else { None };
        let cpu_name = match registry::read(Root::Hklm, r"HARDWARE\DESCRIPTION\System\CentralProcessor\0", "ProcessorNameString") {
            Ok(Value::Str(s)) => s.split_whitespace().collect::<Vec<_>>().join(" "),
            _ => String::new(),
        };
        let has_nvml = nvml::get().is_some();
        // The per-process GPU engine counters are costly to collect: only the AMD / Intel fallback needs them.
        let add_gpu = |path: &str| if has_nvml { None } else { add(path) };
        let mut s = Self {
            query,
            cpu_util: add(r"\Processor Information(*)\% Processor Utility").or_else(|| add(r"\Processor Information(*)\% Processor Time")),
            cpu_perf: add(r"\Processor Information(*)\% Processor Performance"),
            cpu_freq: add(r"\Processor Information(*)\Processor Frequency"),
            rapl: add(r"\Energy Meter(*)\Power"),
            disk_idle: add(r"\PhysicalDisk(*)\% Idle Time"),
            disk_read: add(r"\PhysicalDisk(*)\Disk Read Bytes/sec"),
            disk_write: add(r"\PhysicalDisk(*)\Disk Write Bytes/sec"),
            gpu_util: add_gpu(r"\GPU Engine(*)\Utilization Percentage"),
            gpu_mem: add_gpu(r"\GPU Adapter Memory(*)\Dedicated Usage"),
            cpu_name,
            gpu_fallback: if has_nvml { None } else { hardware::gpus().into_iter().next() },
            disk_names: hardware::disks().into_iter().map(|d| (d.index, d.model)).collect(),
            net_prev: HashMap::new(),
            smart: Arc::new(Mutex::new(HashMap::new())),
            smart_at: None,
            smart_busy: Arc::new(AtomicBool::new(false)),
            ext: None,
            ext_skip_until: 0,
            tick: 0,
        };
        // Rate counters need two collections; prime now so the first real snapshot is meaningful.
        s.collect();
        s.nets_raw();
        s
    }

    fn collect(&self) {
        if self.query.0.is_null() {
            return;
        }
        // SAFETY: valid query.
        let _ = unsafe { PdhCollectQueryData(self.query) };
    }

    pub fn sample(&mut self) -> Snapshot {
        self.collect();
        self.tick = self.tick.wrapping_add(1);
        self.refresh_smart();
        self.refresh_external();
        let parts = Parts {
            cpu: Some(self.cpu_raw()),
            gpu: self.gpu_raw(),
            mem: memory_raw(),
            disks: self.disks_raw(),
            nets: self.nets_raw(),
        };
        let readings = self.ext.as_ref().map(|(_, r)| r.as_slice());
        let devices = build(&parts, readings);
        Snapshot {
            seq: self.tick,
            ts: now_unix(),
            count: count_sensors(&devices),
            external: self.ext.as_ref().map(|(s, _)| *s),
            cpu_temp: readings.is_some_and(|r| sensors::pick_cpu_temp(r).is_some()),
            devices,
        }
    }

    fn cpu_raw(&self) -> CpuRaw {
        let util = values(&self.cpu_util);
        let perf: HashMap<String, f64> = values(&self.cpu_perf).into_iter().collect();
        let freq: HashMap<String, f64> = values(&self.cpu_freq).into_iter().collect();
        // Effective clock = nominal frequency × % performance (turbo shows above the base), like Task Manager.
        let mhz = |name: &str| match (freq.get(name), perf.get(name)) {
            (Some(f), Some(p)) if *f > 0.0 && *p > 0.0 => Some(f * p / 100.0),
            _ => None,
        };
        let mut threads: Vec<((u32, u32), ThreadRaw)> =
            util.iter().filter_map(|(name, v)| Some((parse_thread(name)?, ThreadRaw { load: Some(*v), mhz: mhz(name) }))).collect();
        threads.sort_by_key(|(k, _)| *k);
        let threads: Vec<ThreadRaw> = threads.into_iter().map(|(_, t)| t).collect();
        let clocks: Vec<f64> = threads.iter().filter_map(|t| t.mhz).collect();
        let mut rapl: Vec<(Rapl, f64)> =
            values(&self.rapl).into_iter().filter_map(|(n, v)| Some((rapl_domain(&n)?, rapl_watts(v)))).filter(|(_, w)| *w > 0.0).collect();
        rapl.sort_by_key(|(d, _)| *d);
        rapl.dedup_by_key(|(d, _)| *d);
        CpuRaw {
            name: self.cpu_name.clone(),
            load: util.iter().find(|(n, _)| n == "_Total").map(|(_, v)| *v),
            mhz: mhz("_Total").or_else(|| (!clocks.is_empty()).then(|| clocks.iter().sum::<f64>() / clocks.len() as f64)),
            threads,
            rapl,
        }
    }

    fn gpu_raw(&self) -> Option<GpuRaw> {
        if let Some(n) = nvml::get() {
            let x = nv_extra();
            let util = x.and_then(NvExtra::utilization);
            let mem = x.and_then(NvExtra::memory);
            return Some(GpuRaw {
                name: x.and_then(NvExtra::name).unwrap_or_default(),
                temp_c: n.temperature_c().map(f64::from),
                core_mhz: n.clock_mhz(0).map(f64::from),
                mem_mhz: n.clock_mhz(2).map(f64::from),
                video_mhz: n.clock_mhz(3).map(f64::from),
                power_w: n.power_w().map(f64::from),
                power_limit_w: n.power_limit_w().map(f64::from),
                load: util.map(|u| u.0),
                mem_ctrl: util.map(|u| u.1),
                vram_used: mem.map(|m| m.0 as f64),
                vram_total: mem.map(|m| m.1 as f64),
                fan_pct: x.and_then(NvExtra::fan_percent),
            });
        }
        let g = self.gpu_fallback.as_ref()?;
        // Like Task Manager: the busiest engine type (3D, Copy, VideoDecode…) summed over processes.
        let mut per_engine: HashMap<String, f64> = HashMap::new();
        for (name, v) in values(&self.gpu_util) {
            *per_engine.entry(name.rsplit("engtype_").next().unwrap_or("?").to_string()).or_default() += v;
        }
        let load = per_engine.values().copied().fold(0.0, f64::max);
        let used: f64 = values(&self.gpu_mem).iter().map(|(_, v)| *v).sum();
        Some(GpuRaw {
            name: g.name.clone(),
            load: Some(load),
            vram_used: Some(used),
            vram_total: Some(g.vram_bytes as f64),
            ..GpuRaw::default()
        })
    }

    fn disks_raw(&self) -> Vec<DiskRaw> {
        let by_name = |c: &Option<Counter>| -> HashMap<String, f64> { values(c).into_iter().collect() };
        let (read, write) = (by_name(&self.disk_read), by_name(&self.disk_write));
        let temps = self.smart.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let mut rows: Vec<DiskRaw> = values(&self.disk_idle)
            .into_iter()
            .filter_map(|(name, idle)| {
                let index = parse_disk_index(&name)?;
                Some(DiskRaw {
                    index,
                    name: self.disk_names.get(&index).cloned().unwrap_or_default(),
                    active: Some(100.0 - idle),
                    read_bps: read.get(&name).copied(),
                    write_bps: write.get(&name).copied(),
                    temp_c: temps.get(&index).copied(),
                })
            })
            .collect();
        rows.sort_by_key(|r| r.index);
        rows.dedup_by_key(|r| r.index);
        rows
    }

    /// Hardware adapters that are connected: rates are the delta of the octet counters.
    fn nets_raw(&mut self) -> Vec<NetRaw> {
        let now = Instant::now();
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut table: *mut MIB_IF_TABLE2 = null_mut();
        // SAFETY: GetIfTable2 allocates the table; FreeMibTable releases it.
        unsafe {
            if GetIfTable2(&mut table) != ERROR_SUCCESS || table.is_null() {
                return out;
            }
            let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
            for r in rows {
                // Bit 0 of the flags is HardwareInterface: skips filter drivers, VLANs and tunnels.
                if r.OperStatus != IfOperStatusUp
                    || r.Type == IF_TYPE_SOFTWARE_LOOPBACK
                    || r.MediaConnectState != MediaConnectStateConnected
                    || r.InterfaceAndOperStatusFlags._bitfield & 1 == 0
                    || !seen.insert(r.InterfaceIndex)
                {
                    continue;
                }
                let (rx, tx) = (r.InOctets, r.OutOctets);
                let (down, up) = match self.net_prev.get(&r.InterfaceIndex) {
                    Some((t0, rx0, tx0)) => {
                        let dt = now.duration_since(*t0).as_secs_f64().max(0.001);
                        (Some(rx.saturating_sub(*rx0) as f64 / dt), Some(tx.saturating_sub(*tx0) as f64 / dt))
                    }
                    None => (None, None),
                };
                self.net_prev.insert(r.InterfaceIndex, (now, rx, tx));
                let end = r.Alias.iter().position(|&c| c == 0).unwrap_or(r.Alias.len());
                // u64::MAX = unknown speed.
                let link = r.ReceiveLinkSpeed.min(r.TransmitLinkSpeed);
                out.push(NetRaw {
                    id: r.InterfaceIndex,
                    alias: String::from_utf16_lossy(&r.Alias[..end]),
                    down_bps: down,
                    up_bps: up,
                    link_mbps: (link > 0 && link < 1_000_000_000_000).then(|| link as f64 / 1e6),
                });
            }
            FreeMibTable(table.cast::<c_void>());
        }
        self.net_prev.retain(|k, _| seen.contains(k));
        out.sort_by_key(|n| n.id);
        out
    }

    fn refresh_smart(&mut self) {
        if self.smart_at.is_some_and(|t| t.elapsed() < SMART_EVERY) || self.smart_busy.swap(true, Ordering::SeqCst) {
            return;
        }
        self.smart_at = Some(Instant::now());
        let (cell, busy) = (self.smart.clone(), self.smart_busy.clone());
        std::thread::spawn(move || {
            let temps: HashMap<u32, f64> = smart::health().iter().filter_map(|d| Some((d.index, drive_temp(d)?))).collect();
            *cell.lock().unwrap_or_else(|p| p.into_inner()) = temps;
            busy.store(false, Ordering::SeqCst);
        });
    }

    /// HWiNFO (shared memory) every tick; LibreHardwareMonitor (WMI, heavier) every third; nothing
    /// running is retried every fifth tick. A slow provider is left alone for 30 ticks.
    fn refresh_external(&mut self) {
        let due = match &self.ext {
            Some((Source::Hwinfo, _)) => true,
            Some((Source::LibreHardwareMonitor, _)) => self.tick.is_multiple_of(3),
            None => self.tick % 5 == 1,
        };
        if !due || self.tick < self.ext_skip_until {
            return;
        }
        let t0 = Instant::now();
        self.ext = sensors::read_all();
        if t0.elapsed() > Duration::from_millis(800) {
            self.ext_skip_until = self.tick.wrapping_add(30);
        }
    }
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        if !self.query.0.is_null() {
            // SAFETY: closes the query (and its counters) exactly once.
            unsafe {
                let _ = PdhCloseQuery(self.query);
            }
        }
    }
}

/// Physical and committed memory in bytes (`GetPerformanceInfo` counts pages).
fn memory_raw() -> Option<MemRaw> {
    let mut pi = PERFORMANCE_INFORMATION { cb: size_of::<PERFORMANCE_INFORMATION>() as u32, ..Default::default() };
    // SAFETY: sized out-param.
    unsafe { GetPerformanceInfo(&mut pi, pi.cb) }.ok()?;
    let page = pi.PageSize as u64;
    Some(MemRaw {
        total: pi.PhysicalTotal as u64 * page,
        avail: pi.PhysicalAvailable as u64 * page,
        cache: pi.SystemCache as u64 * page,
        commit: pi.CommitTotal as u64 * page,
        commit_limit: pi.CommitLimit as u64 * page,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smart::{AtaAttr, NvmeHealth, Status};

    fn reading(sensor: &str, label: &str, kind: Kind, value: f64, unit: &str) -> Reading {
        Reading { sensor: sensor.into(), label: label.into(), kind, value, unit: unit.into() }
    }

    fn full_parts() -> Parts {
        Parts {
            cpu: Some(CpuRaw {
                name: "Intel(R) Core(TM) i5-10400".into(),
                load: Some(12.5),
                mhz: Some(4100.0),
                threads: vec![ThreadRaw { load: Some(10.0), mhz: Some(4000.0) }, ThreadRaw { load: Some(250.0), mhz: None }],
                rapl: vec![(Rapl::Package, 41.2), (Rapl::Cores, 30.0), (Rapl::Graphics, 0.2), (Rapl::Dram, 2.5)],
            }),
            gpu: Some(GpuRaw {
                name: "NVIDIA GeForce RTX 3050".into(),
                temp_c: Some(55.0),
                core_mhz: Some(1800.0),
                mem_mhz: Some(7000.0),
                video_mhz: Some(1500.0),
                power_w: Some(60.0),
                power_limit_w: Some(130.0),
                load: Some(40.0),
                mem_ctrl: Some(20.0),
                vram_used: Some(2.0 * 1024.0 * 1024.0 * 1024.0),
                vram_total: Some(8.0 * 1024.0 * 1024.0 * 1024.0),
                fan_pct: Some(35.0),
            }),
            mem: Some(MemRaw { total: 32 << 30, avail: 20 << 30, cache: 4 << 30, commit: 14 << 30, commit_limit: 40 << 30 }),
            disks: vec![DiskRaw { index: 0, name: "KINGSTON SA400".into(), active: Some(3.0), read_bps: Some(1e6), write_bps: Some(2e6), temp_c: Some(34.0) }],
            nets: vec![NetRaw { id: 7, alias: "Ethernet".into(), down_bps: Some(1e5), up_bps: Some(2e4), link_mbps: Some(1000.0) }],
        }
    }

    fn all_sensors(devices: &[Device]) -> Vec<&Sensor> {
        devices.iter().flat_map(|d| &d.groups).flat_map(|g| &g.sensors).collect()
    }

    #[test]
    fn pdh_instance_names_parse_and_hostile_names_do_not() {
        assert_eq!(parse_thread("0,3"), Some((0, 3)));
        assert_eq!(parse_thread("1,15"), Some((1, 15)));
        for bad in ["_Total", "0,_Total", "", ",", "a,b", "0,", ",1", "99999999999,1", "0,-1", "0 ,1"] {
            assert_eq!(parse_thread(bad), None, "{bad:?}");
        }
        assert_eq!(parse_disk_index("0 C:"), Some(0));
        assert_eq!(parse_disk_index("12 D: E:"), Some(12));
        assert_eq!(parse_disk_index("3"), Some(3));
        for bad in ["_Total", "", "   ", "C:", "-1 C:", "99999999999 C:", "x 0"] {
            assert_eq!(parse_disk_index(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn rapl_domains_of_the_first_package_only() {
        assert_eq!(rapl_domain("RAPL_Package0_PKG"), Some(Rapl::Package));
        assert_eq!(rapl_domain("rapl_package0_pp0"), Some(Rapl::Cores));
        assert_eq!(rapl_domain("RAPL_Package0_PP1"), Some(Rapl::Graphics));
        assert_eq!(rapl_domain("RAPL_Package0_DRAM"), Some(Rapl::Dram));
        assert_eq!(rapl_domain("RAPL_Package1_PKG"), None, "second socket would collide ids");
        assert_eq!(rapl_domain("something"), None);
        assert_eq!(rapl_domain(""), None);
        assert!((rapl_watts(40_000.0) - 40.0).abs() < 1e-9);
        assert!((rapl_watts(100.0) - 0.1).abs() < 1e-9, "a quiet integrated GPU is a tenth of a watt, not 100 W");
    }

    #[test]
    fn cpu_tree_has_total_and_numbered_threads_and_clamps_load() {
        let d = cpu_device(&full_parts().cpu.unwrap()).unwrap();
        assert_eq!((d.id.as_str(), d.kind, d.key), ("cpu", DeviceKind::Cpu, None));
        assert_eq!(d.label, "Intel(R) Core(TM) i5-10400");
        let ids: Vec<&str> = all_sensors(std::slice::from_ref(&d)).iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"cpu/load/total") && ids.contains(&"cpu/load/thread0") && ids.contains(&"cpu/load/thread1"));
        assert!(ids.contains(&"cpu/clock/total") && ids.contains(&"cpu/clock/thread0"));
        assert!(!ids.contains(&"cpu/clock/thread1"), "a thread without a clock has no clock sensor");
        assert!(ids.contains(&"cpu/power/pkg") && ids.contains(&"cpu/power/dram"));
        let t1 = d.groups[0].sensors.iter().find(|s| s.id == "cpu/load/thread1").unwrap();
        assert_eq!((t1.value, t1.n, t1.key), (100.0, Some(1), Some("thread")), "turbo utility above 100 is clamped");
        assert_eq!(t1.label, "Thread 1");
        // No name → the translated generic label.
        let nameless = cpu_device(&CpuRaw { load: Some(1.0), ..Default::default() }).unwrap();
        assert_eq!((nameless.label.as_str(), nameless.key), ("", Some("cpu")));
        assert!(cpu_device(&CpuRaw::default()).is_none(), "nothing measured, no device");
    }

    #[test]
    fn gpu_tree_derives_percentages_and_drops_missing_parts() {
        let d = gpu_device(&full_parts().gpu.unwrap()).unwrap();
        let find = |id: &str| all_sensors(std::slice::from_ref(&d)).into_iter().find(|s| s.id == id).map(|s| s.value);
        assert_eq!(find("gpu0/power/limit_pct"), Some(46.154));
        assert_eq!(find("gpu0/vram/vram_pct"), Some(25.0));
        assert_eq!(find("gpu0/temp/gpu_core"), Some(55.0));
        assert_eq!(find("gpu0/clock/clock_video"), Some(1500.0));
        let partial = gpu_device(&GpuRaw { name: "AMD".into(), load: Some(5.0), power_limit_w: Some(0.0), power_w: Some(10.0), ..Default::default() }).unwrap();
        let ids: Vec<&str> = all_sensors(std::slice::from_ref(&partial)).iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["gpu0/load/gpu", "gpu0/power/draw"], "zero limit gives no limit and no percentage");
        assert!(gpu_device(&GpuRaw::default()).is_none());
        let nan = gpu_device(&GpuRaw { temp_c: Some(f64::INFINITY), load: Some(f64::NAN), core_mhz: Some(1.0), ..Default::default() }).unwrap();
        assert_eq!(count_sensors(&[nan]), 1, "non-finite readings never reach the JSON");
        let pegged = gpu_device(&GpuRaw { load: Some(f64::INFINITY), ..Default::default() }).unwrap();
        assert_eq!(pegged.groups[0].sensors[0].value, 100.0, "a percentage is clamped, so infinity reads as full");
    }

    #[test]
    fn memory_disk_and_network_trees() {
        let p = full_parts();
        let m = memory_device(&p.mem.unwrap()).unwrap();
        let v = |id: &str| all_sensors(std::slice::from_ref(&m)).into_iter().find(|s| s.id == id).map(|s| s.value);
        assert_eq!(v("mem/physical/mem_used_pct"), Some(37.5));
        assert_eq!(v("mem/physical/mem_used"), Some((12u64 << 30) as f64));
        assert_eq!(v("mem/commit/commit_pct"), Some(35.0));
        assert!(memory_device(&MemRaw::default()).is_none());
        // Available above total (a torn read) never yields a negative "in use".
        let torn = memory_device(&MemRaw { total: 100, avail: 200, ..Default::default() }).unwrap();
        assert_eq!(torn.groups[0].sensors[1].value, 0.0);

        let d = disk_device(&p.disks[0]).unwrap();
        assert_eq!((d.id.as_str(), d.n, d.label.as_str()), ("disk0", Some(0), "KINGSTON SA400"));
        assert_eq!(d.groups.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), ["disk0/activity", "disk0/temp"]);
        let unnamed = disk_device(&DiskRaw { index: 4, active: Some(1.0), ..Default::default() }).unwrap();
        assert_eq!((unnamed.key, unnamed.n), (Some("disk"), Some(4)));
        assert!(disk_device(&DiskRaw { index: 1, ..Default::default() }).is_none());

        let n = net_device(&p.nets[0]).unwrap();
        assert_eq!((n.id.as_str(), n.label.as_str()), ("net7", "Ethernet"));
        let first_tick = net_device(&NetRaw { id: 9, alias: "Wi-Fi".into(), link_mbps: Some(866.0), ..Default::default() }).unwrap();
        assert_eq!(first_tick.groups.len(), 1, "no rate yet on the first sample, only the link speed");
        let neg = net_device(&NetRaw { id: 1, down_bps: Some(-5.0), ..Default::default() }).unwrap();
        assert_eq!(neg.groups[0].sensors[0].value, 0.0);
    }

    #[test]
    fn external_readings_group_by_device_and_kind_with_stable_unique_ids() {
        let rs = vec![
            reading("CPU [#0]: Intel Core i5-10400", "CPU Package", Kind::Temperature, 61.5, "�C"),
            reading("CPU [#0]: Intel Core i5-10400", "Core 0", Kind::Temperature, 58.0, "°C"),
            reading("CPU [#0]: Intel Core i5-10400", "Core 0", Kind::Temperature, 59.0, "°C"),
            reading("CPU [#0]: Intel Core i5-10400", "CPU FAN", Kind::Fan, 1200.0, "RPM"),
            reading("ASUS PRIME B460M", "+12V", Kind::Voltage, 12.096, "V"),
            reading("ASUS PRIME B460M", "Total CPU Usage", Kind::Other, 7.0, "%"),
            reading("ASUS PRIME B460M", "VCORE Current", Kind::Other, 1.5, "A"),
            reading("ASUS PRIME B460M", "Link", Kind::Other, 3.0, "x"),
            reading("", "Orphan", Kind::Power, 9.0, "W"),
            reading("ASUS PRIME B460M", "Broken", Kind::Voltage, f64::NAN, "V"),
        ];
        let devs = external_devices(&rs);
        assert_eq!(devs.iter().map(|d| d.label.as_str()).collect::<Vec<_>>(), ["CPU [#0]: Intel Core i5-10400", "ASUS PRIME B460M", ""]);
        assert!(devs.iter().all(|d| d.kind == DeviceKind::External));
        assert_eq!(devs[2].key, Some("other"), "unnamed device gets the translated generic label");
        let cpu = &devs[0];
        assert_eq!(cpu.groups.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), ["ext/cpu-0-intel-core-i5-10400/temp", "ext/cpu-0-intel-core-i5-10400/fan"]);
        assert_eq!(cpu.groups[0].key, Some("temp"));
        assert_eq!(cpu.groups[0].sensors[0].unit, "°C", "mangled degree sign is repaired by kind");
        let ids: Vec<&str> = cpu.groups[0].sensors.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids[1], "ext/cpu-0-intel-core-i5-10400/temp/core-0");
        assert_eq!(ids[2], "ext/cpu-0-intel-core-i5-10400/temp/core-0#2", "same label twice stays two sensors");
        let board = &devs[1];
        assert_eq!(board.groups.iter().map(|g| g.key.unwrap_or("?")).collect::<Vec<_>>(), ["volt", "usage", "current", "other"]);
        assert_eq!(count_sensors(&devs), 9, "the NaN reading is dropped");
        // Deterministic: the same readings give the same ids on the next second.
        assert_eq!(external_devices(&rs), devs);
    }

    #[test]
    fn external_ids_cannot_collide_or_carry_paths_and_the_list_is_capped() {
        let rs = vec![
            reading("Same", "A", Kind::Fan, 1.0, "RPM"),
            reading("Same", "A", Kind::Fan, 2.0, "RPM"),
            reading("same", "A", Kind::Fan, 3.0, "RPM"),
            reading("..\\..\\evil/x", "<b>x</b>", Kind::Fan, 4.0, "RPM"),
        ];
        let devs = external_devices(&rs);
        let mut seen = HashSet::new();
        for d in &devs {
            assert!(seen.insert(d.id.clone()), "duplicate device id {}", d.id);
            for g in &d.groups {
                assert!(seen.insert(g.id.clone()), "duplicate group id {}", g.id);
                for s in &g.sensors {
                    assert!(seen.insert(s.id.clone()), "duplicate sensor id {}", s.id);
                    assert!(s.id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'#')), "{}", s.id);
                }
            }
        }
        assert_eq!(slug(""), "x");
        assert_eq!(slug("   "), "x");
        assert_eq!(slug("Ünï CPU Core #1 (°C)"), "n-cpu-core-1-c");
        assert!(slug(&"a".repeat(500)).len() <= 40);

        let many: Vec<Reading> = (0..MAX_EXTERNAL + 300).map(|i| reading("Big", &format!("S{i}"), Kind::Fan, i as f64, "RPM")).collect();
        assert_eq!(count_sensors(&external_devices(&many)) as usize, MAX_EXTERNAL);
        assert!(external_devices(&[]).is_empty());
    }

    #[test]
    fn whole_tree_ids_are_unique_and_every_i18n_key_is_known_and_translated() {
        let rs = vec![reading("Board", "VIN", Kind::Voltage, 1.0, "V"), reading("", "x", Kind::Fan, 1.0, "RPM")];
        let devs = build(&full_parts(), Some(&rs));
        assert_eq!(devs.iter().map(|d| d.kind).collect::<Vec<_>>().first(), Some(&DeviceKind::Cpu));
        let mut ids = HashSet::new();
        let (mut dkeys, mut gkeys, mut skeys) = (HashSet::new(), HashSet::new(), HashSet::new());
        for d in &devs {
            assert!(ids.insert(d.id.as_str()), "{}", d.id);
            dkeys.extend(d.key);
            for g in &d.groups {
                assert!(ids.insert(g.id.as_str()), "{}", g.id);
                gkeys.extend(g.key);
                for s in &g.sensors {
                    assert!(ids.insert(s.id.as_str()), "{}", s.id);
                    skeys.extend(s.key);
                }
            }
        }
        // The UI asks `hw.d.*`, `hw.g.*`, `hw.s.*`: each must exist in both locales.
        let pt = include_str!("../../../src/locales/pt-BR.ts");
        let en = include_str!("../../../src/locales/en.ts");
        let check = |kind: &str, used: &HashSet<&str>, known: &[&str]| {
            for k in used {
                assert!(known.contains(k), "{kind} key {k} is not in the known list");
            }
            for k in known {
                for (lang, file) in [("pt-BR", pt), ("en", en)] {
                    assert!(file.contains(&format!("\"hw.{kind}.{k}\":")), "{lang} lacks hw.{kind}.{k}");
                }
            }
        };
        check("d", &dkeys, DEVICE_KEYS);
        check("g", &gkeys, GROUP_KEYS);
        check("s", &skeys, SENSOR_KEYS);
    }

    #[test]
    fn disk_temperature_from_nvme_or_ata_and_garbage_is_ignored() {
        let mut d = DriveHealth {
            index: 0,
            model: "X".into(),
            bus: "nvme".into(),
            key: "k".into(),
            needs_admin: false,
            nvme: None,
            ata: Vec::new(),
            status: Status::Unknown,
            findings: Vec::new(),
        };
        assert_eq!(drive_temp(&d), None);
        let attr = |id, raw| vec![AtaAttr { id, flags: 0, current: 100, worst: 100, raw }];
        d.ata = attr(194, 0x0001_0000_0000 | 41);
        assert_eq!(drive_temp(&d), Some(41.0), "only the low byte is the temperature");
        d.ata = attr(190, 300);
        assert_eq!(drive_temp(&d), Some(44.0), "300 & 0xFF");
        for garbage in [0, 200, 255] {
            d.ata = attr(194, garbage);
            assert_eq!(drive_temp(&d), None, "{garbage} is not a plausible drive temperature");
        }
        d.ata = attr(9, 41); // power-on hours, not a temperature
        assert_eq!(drive_temp(&d), None);
        d.nvme = Some(NvmeHealth {
            critical_warning: 0,
            temperature_c: Some(37),
            available_spare: 100,
            spare_threshold: 10,
            percentage_used: 1,
            data_read_bytes: 0,
            data_written_bytes: 0,
            power_cycles: 0,
            power_on_hours: 0,
            unsafe_shutdowns: 0,
            media_errors: 0,
            error_log_entries: 0,
        });
        assert_eq!(drive_temp(&d), Some(37.0));
    }

    /// Live: one second of the real machine. Read-only; run with `--ignored`.
    #[test]
    #[ignore = "live: reads this machine's counters"]
    fn live_sampler_smoke() {
        let mut s = Sampler::new();
        std::thread::sleep(Duration::from_millis(1100));
        let a = s.sample();
        std::thread::sleep(Duration::from_millis(1100));
        let b = s.sample();
        assert_eq!((a.seq, b.seq), (1, 2));
        assert!(b.count > 10, "expected CPU, memory and more, got {}", b.count);
        let ids: HashSet<&str> = b.devices.iter().flat_map(|d| &d.groups).flat_map(|g| &g.sensors).map(|s| s.id.as_str()).collect();
        assert_eq!(ids.len() as u32, b.count, "sensor ids are unique");
        assert!(b.devices.iter().any(|d| d.kind == DeviceKind::Cpu) && b.devices.iter().any(|d| d.kind == DeviceKind::Memory));
        println!("{}", serde_json::to_string_pretty(&b).unwrap());
    }
}
