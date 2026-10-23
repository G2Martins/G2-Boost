//! Hardware / OS profile ("Meu PC"). Read-only and without admin: registry, SMBIOS
//! (`GetSystemFirmwareTable`), CPUID, DXGI, storage IOCTLs, display config, TPM base
//! services, Secure Boot state, NVML (NVIDIA, user-mode). The SSD/HDD answer here also
//! drives the SysMain recommendation. Everything CPU-Z/Speccy get from a kernel driver
//! (temperatures, voltages, SPD/XMP tables, chipset registers) is deliberately absent.

use std::collections::BTreeMap;
use std::ffi::c_void;
use std::mem::size_of;

use serde::Serialize;
use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
    DISPLAYCONFIG_TARGET_DEVICE_NAME, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS,
    QueryDisplayConfig,
};
use windows::Win32::Graphics::Gdi::{DEVMODEW, ENUM_DISPLAY_SETTINGS_MODE, EnumDisplaySettingsW};
use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1};

/// Hardware-accelerated GPU scheduling as the driver reports it *now* (not the registry request):
/// `(supported, enabled)` for the first hardware adapter that answers `KMTQAITYPE_WDDM_2_7_CAPS`.
/// Differs from `HwSchMode` until the next reboot after a change.
pub fn hags_state() -> Option<(bool, bool)> {
    use windows::Wdk::Graphics::Direct3D::{
        D3DKMT_CLOSEADAPTER, D3DKMT_OPENADAPTERFROMLUID, D3DKMT_QUERYADAPTERINFO, D3DKMT_WDDM_2_7_CAPS, D3DKMTCloseAdapter,
        D3DKMTOpenAdapterFromLuid, D3DKMTQueryAdapterInfo, KMTQAITYPE_WDDM_2_7_CAPS,
    };
    // SAFETY: DXGI enumeration; each opened KMT adapter handle is closed below; caps is a plain u32 bitfield.
    unsafe {
        let factory = CreateDXGIFactory1::<IDXGIFactory1>().ok()?;
        for i in 0..8 {
            let Ok(adapter) = factory.EnumAdapters1(i) else {
                break;
            };
            let Ok(desc) = adapter.GetDesc1() else {
                continue;
            };
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let mut open = D3DKMT_OPENADAPTERFROMLUID { AdapterLuid: desc.AdapterLuid, hAdapter: 0 };
            if D3DKMTOpenAdapterFromLuid(&mut open).is_err() {
                continue;
            }
            let mut caps = D3DKMT_WDDM_2_7_CAPS::default();
            let mut q = D3DKMT_QUERYADAPTERINFO {
                hAdapter: open.hAdapter,
                Type: KMTQAITYPE_WDDM_2_7_CAPS,
                pPrivateDriverData: (&mut caps as *mut D3DKMT_WDDM_2_7_CAPS).cast(),
                PrivateDriverDataSize: std::mem::size_of::<D3DKMT_WDDM_2_7_CAPS>() as u32,
            };
            let ok = D3DKMTQueryAdapterInfo(&mut q).is_ok();
            let _ = D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: open.hAdapter });
            if ok {
                // Bits: 0 HwSchSupported, 1 HwSchEnabled, 2 HwSchEnabledByDefault.
                let bits = caps.Anonymous.Value;
                if bits & 1 != 0 {
                    return Some((true, bits & 2 != 0));
                }
            }
        }
        Some((false, false))
    }
}
use windows::Win32::Storage::FileSystem::{
    BusTypeNvme, BusTypeRAID, BusTypeSas, BusTypeSata, BusTypeUsb, CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ,
    FILE_SHARE_WRITE, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, OPEN_EXISTING, STORAGE_BUS_TYPE,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{
    DEVICE_SEEK_PENALTY_DESCRIPTOR, DISK_GEOMETRY_EX, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, IOCTL_STORAGE_QUERY_PROPERTY,
    PropertyStandardQuery, STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY, StorageDeviceProperty,
    StorageDeviceSeekPenaltyProperty, VOLUME_DISK_EXTENTS,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::{
    CacheData, CacheInstruction, ComputerNameDnsHostname, GetComputerNameExW, GetLogicalProcessorInformationEx,
    GetPhysicallyInstalledSystemMemory, GetSystemFirmwareTable, GetTickCount64, LOGICAL_PROCESSOR_RELATIONSHIP, RSMB,
    RelationCache, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};
use windows::Win32::System::Threading::{ALL_PROCESSOR_GROUPS, GetActiveProcessorCount};
use windows::Win32::System::TpmBaseServices::{TPM_DEVICE_INFO, Tbsi_GetDeviceInfo};
use windows::core::{PCWSTR, PWSTR};

use crate::nvml;
use crate::registry::{self, Root, Value, wide};
use crate::system;

#[derive(Debug, Clone, Serialize)]
pub struct Gpu {
    pub name: String,
    pub vram_bytes: u64,
    /// `nvidia` | `amd` | `intel` | `other`
    pub vendor: &'static str,
    /// Driver version (+ date) from the display class registry key, or NVML.
    pub driver: Option<String>,
    /// "PCIe 3.0 x8" (NVML only for now).
    pub pcie: Option<String>,
    /// Power limit in watts (NVML only).
    pub power_limit_w: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Disk {
    pub index: u32,
    pub model: String,
    /// `nvme` | `sata` | `usb` | `sas` | `raid` | `other`
    pub bus: &'static str,
    pub size_bytes: u64,
    /// `None` when the drive did not answer the seek-penalty query (some USB bridges).
    pub ssd: Option<bool>,
    pub system: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Bios {
    pub vendor: String,
    pub version: String,
    pub date: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MemoryModule {
    /// "DIMM_A1" (device locator) and "BANK 0" (bank locator) as the board names them.
    pub slot: String,
    pub bank: String,
    pub size_bytes: u64,
    /// SPD/JEDEC speed the firmware reports (MT/s).
    pub speed_mts: u32,
    /// Speed actually configured (MT/s; SMBIOS 3.0+, 0 when unknown).
    pub configured_mts: u32,
    /// Rated speed guessed from the part number ("CMK16GX4M1E3200C16" → 3200), if any.
    pub rated_mts: Option<u32>,
    /// "DDR4", "DDR5", "LPDDR5" …
    pub kind: String,
    pub form_factor: String,
    pub manufacturer: String,
    pub part_number: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CacheInfo {
    pub level: u8,
    /// `data` | `instruction` | `unified`
    pub kind: &'static str,
    /// Instances (e.g. 6 for "6 × 32 KB").
    pub count: u32,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CpuDetail {
    pub vendor: String,
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
    pub flags: Vec<&'static str>,
    /// Microcode revision from the registry ("0xF6").
    pub microcode: Option<String>,
    pub caches: Vec<CacheInfo>,
    pub hyperthreading: bool,
    /// VMX (Intel) / SVM (AMD) present in CPUID.
    pub virtualization: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MonitorInfo {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: f32,
    /// Highest refresh the driver offers at the current resolution (0 = unknown).
    pub max_refresh_hz: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    pub computer: String,
    /// "Windows 11 Pro 24H2 (build 26200.1234)"
    pub os: String,
    pub cpu: String,
    pub cpu_detail: CpuDetail,
    pub cores_physical: u32,
    pub cores_logical: u32,
    pub ram_bytes: u64,
    pub memory: Vec<MemoryModule>,
    pub memory_slots: Option<u16>,
    /// Modules configured below the speed guessed from their part number (XMP/EXPO likely off).
    pub memory_below_rated: bool,
    pub uptime_secs: u64,
    pub gpus: Vec<Gpu>,
    pub monitors: Vec<MonitorInfo>,
    pub board: Option<String>,
    pub bios: Option<Bios>,
    pub system_model: Option<String>,
    pub disks: Vec<Disk>,
    pub system_disk_ssd: Option<bool>,
    pub secure_boot: Option<bool>,
    /// "2.0" / "1.2"
    pub tpm: Option<String>,
    pub laptop: bool,
    pub elevated: bool,
}

pub fn profile() -> Profile {
    let smbios = read_smbios().unwrap_or_default();
    let disks = disks();
    let system_disk_ssd = disks.iter().find(|d| d.system).and_then(|d| d.ssd);
    let cores_physical = physical_cores();
    let cores_logical = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) };
    let memory_below_rated = smbios.memory.iter().any(|m| m.size_bytes > 0 && m.configured_mts > 0 && m.rated_mts.is_some_and(|r| m.configured_mts < r));
    Profile {
        computer: computer_name(),
        os: os_string(),
        cpu: cpu_name(),
        cpu_detail: cpu_detail(cores_physical, cores_logical),
        cores_physical,
        cores_logical,
        ram_bytes: installed_ram(),
        memory: smbios.memory,
        memory_slots: smbios.memory_slots,
        memory_below_rated,
        uptime_secs: unsafe { GetTickCount64() } / 1000,
        gpus: gpus(),
        monitors: monitors(),
        board: smbios.board,
        bios: smbios.bios,
        system_model: smbios.system,
        disks,
        system_disk_ssd,
        secure_boot: secure_boot(),
        tpm: tpm_version(),
        laptop: is_laptop(smbios.chassis),
        elevated: system::is_elevated(),
    }
}

fn computer_name() -> String {
    let mut buf = [0u16; 256];
    let mut len = buf.len() as u32;
    // SAFETY: buffer sized by `len`; the call writes at most that many UTF-16 units.
    match unsafe { GetComputerNameExW(ComputerNameDnsHostname, Some(PWSTR(buf.as_mut_ptr())), &mut len) } {
        Ok(()) => String::from_utf16_lossy(&buf[..len as usize]),
        Err(_) => std::env::var("COMPUTERNAME").unwrap_or_default(),
    }
}

fn reg_str(root: Root, path: &str, name: &str) -> Option<String> {
    match registry::read(root, path, name) {
        Ok(Value::Str(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}

fn reg_dword(root: Root, path: &str, name: &str) -> Option<u32> {
    match registry::read(root, path, name) {
        Ok(Value::Dword(d)) => Some(d),
        _ => None,
    }
}

const CURRENT_VERSION: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
const CPU0: &str = r"HARDWARE\DESCRIPTION\System\CentralProcessor\0";
const DISPLAY_CLASS: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

/// Edition ids as Windows spells them → what people call them.
fn edition_label(id: &str) -> String {
    match id {
        "Core" | "CoreSingleLanguage" => "Home".into(),
        "Professional" => "Pro".into(),
        "ProfessionalWorkstation" => "Pro for Workstations".into(),
        "Enterprise" => "Enterprise".into(),
        "EnterpriseS" => "Enterprise LTSC".into(),
        "IoTEnterprise" => "IoT Enterprise".into(),
        "IoTEnterpriseS" => "IoT Enterprise LTSC".into(),
        "Education" => "Education".into(),
        other => other.to_string(),
    }
}

fn os_string() -> String {
    // `ProductName` still says "Windows 10" on Windows 11, so the family comes from the build.
    let family = system::os_version();
    let family = family.split(" (").next().unwrap_or("Windows");
    let edition = reg_str(Root::Hklm, CURRENT_VERSION, "EditionID").map(|e| edition_label(&e)).unwrap_or_default();
    let display = reg_str(Root::Hklm, CURRENT_VERSION, "DisplayVersion").unwrap_or_default();
    let build = reg_str(Root::Hklm, CURRENT_VERSION, "CurrentBuild").unwrap_or_default();
    let ubr = reg_dword(Root::Hklm, CURRENT_VERSION, "UBR").map(|u| format!(".{u}")).unwrap_or_default();
    format!("{family} {edition} {display} (build {build}{ubr})").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn cpu_name() -> String {
    reg_str(Root::Hklm, CPU0, "ProcessorNameString")
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_else(|| "CPU".into())
}

/// All `GetLogicalProcessorInformationEx` records of one relationship, as raw bytes + walker.
fn logical_processor_records(relationship: LOGICAL_PROCESSOR_RELATIONSHIP) -> Vec<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX> {
    let mut len = 0u32;
    // SAFETY: size probe (fails with ERROR_INSUFFICIENT_BUFFER and fills `len`).
    let _ = unsafe { GetLogicalProcessorInformationEx(relationship, None, &mut len) };
    if len == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u8; len as usize];
    // SAFETY: buffer of `len` bytes; records are walked by their own `Size`.
    if unsafe { GetLogicalProcessorInformationEx(relationship, Some(buf.as_mut_ptr().cast()), &mut len) }.is_err() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 8 <= len as usize {
        // SAFETY: a full record (header + union) is at least `Size` bytes; we copy by value.
        let rec = unsafe { std::ptr::read_unaligned(buf.as_ptr().add(pos).cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>()) };
        if rec.Size == 0 {
            break;
        }
        out.push(rec);
        pos += rec.Size as usize;
    }
    out
}

fn physical_cores() -> u32 {
    logical_processor_records(RelationProcessorCore).iter().filter(|r| r.Relationship == RelationProcessorCore).count() as u32
}

fn caches() -> Vec<CacheInfo> {
    let mut agg: BTreeMap<(u8, &'static str), (u32, u64)> = BTreeMap::new();
    for rec in logical_processor_records(RelationCache) {
        if rec.Relationship != RelationCache {
            continue;
        }
        // SAFETY: `Cache` is the active union member for RelationCache records.
        let c = unsafe { rec.Anonymous.Cache };
        let kind = if c.Type == CacheData {
            "data"
        } else if c.Type == CacheInstruction {
            "instruction"
        } else {
            "unified"
        };
        let e = agg.entry((c.Level, kind)).or_insert((0, u64::from(c.CacheSize)));
        e.0 += 1;
    }
    agg.into_iter().map(|((level, kind), (count, size_bytes))| CacheInfo { level, kind, count, size_bytes }).collect()
}

#[cfg(target_arch = "x86_64")]
#[allow(unused_unsafe)]
fn cpuid_detail() -> (String, u32, u32, u32, Vec<&'static str>, bool, bool) {
    use std::arch::x86_64::{__cpuid, __cpuid_count};
    // SAFETY: CPUID is available on every x86-64 CPU Windows 10/11 runs on.
    let (l0, l1, l7, e1) = unsafe { (__cpuid(0), __cpuid(1), __cpuid_count(7, 0), __cpuid(0x8000_0001)) };
    let mut vendor = Vec::with_capacity(12);
    for r in [l0.ebx, l0.edx, l0.ecx] {
        vendor.extend_from_slice(&r.to_le_bytes());
    }
    let vendor = String::from_utf8_lossy(&vendor).trim().to_string();
    let eax = l1.eax;
    let (stepping, model, family, ext_model, ext_family) = (eax & 0xF, (eax >> 4) & 0xF, (eax >> 8) & 0xF, (eax >> 16) & 0xF, (eax >> 20) & 0xFF);
    let fam = if family == 0xF { family + ext_family } else { family };
    let mdl = if family == 6 || family == 0xF { (ext_model << 4) | model } else { model };
    let bit = |v: u32, b: u32| v & (1 << b) != 0;
    let table: [(&str, bool); 20] = [
        ("MMX", bit(l1.edx, 23)),
        ("SSE", bit(l1.edx, 25)),
        ("SSE2", bit(l1.edx, 26)),
        ("SSE3", bit(l1.ecx, 0)),
        ("SSSE3", bit(l1.ecx, 9)),
        ("SSE4.1", bit(l1.ecx, 19)),
        ("SSE4.2", bit(l1.ecx, 20)),
        ("FMA3", bit(l1.ecx, 12)),
        ("AES", bit(l1.ecx, 25)),
        ("AVX", bit(l1.ecx, 28)),
        ("F16C", bit(l1.ecx, 29)),
        ("AVX2", bit(l7.ebx, 5)),
        ("BMI1", bit(l7.ebx, 3)),
        ("BMI2", bit(l7.ebx, 8)),
        ("AVX-512F", bit(l7.ebx, 16)),
        ("SHA", bit(l7.ebx, 29)),
        ("x86-64", bit(e1.edx, 29)),
        ("NX", bit(e1.edx, 20)),
        ("VMX", bit(l1.ecx, 5)),
        ("SVM", bit(e1.ecx, 2)),
    ];
    let flags: Vec<&'static str> = table.iter().filter(|(_, on)| *on).map(|(n, _)| *n).collect();
    let virt = bit(l1.ecx, 5) || bit(e1.ecx, 2);
    let ht_flag = bit(l1.edx, 28);
    (vendor, fam, mdl, stepping, flags, virt, ht_flag)
}

#[cfg(not(target_arch = "x86_64"))]
fn cpuid_detail() -> (String, u32, u32, u32, Vec<&'static str>, bool, bool) {
    (String::new(), 0, 0, 0, Vec::new(), false, false)
}

fn cpu_detail(cores_physical: u32, cores_logical: u32) -> CpuDetail {
    let (vendor, family, model, stepping, flags, virtualization, ht_flag) = cpuid_detail();
    let microcode = match registry::read(Root::Hklm, CPU0, "Update Revision") {
        Ok(Value::Binary(b)) if b.len() >= 8 => Some(format!("0x{:X}", u32::from_le_bytes([b[4], b[5], b[6], b[7]]))),
        Ok(Value::Binary(b)) if b.len() >= 4 => Some(format!("0x{:X}", u32::from_le_bytes([b[0], b[1], b[2], b[3]]))),
        _ => None,
    };
    CpuDetail {
        vendor,
        family,
        model,
        stepping,
        flags,
        microcode,
        caches: caches(),
        hyperthreading: ht_flag && cores_logical > cores_physical,
        virtualization,
    }
}

fn installed_ram() -> u64 {
    let mut kb = 0u64;
    // SAFETY: plain out-param.
    if unsafe { GetPhysicallyInstalledSystemMemory(&mut kb) }.is_ok() {
        kb * 1024
    } else {
        system::memory_status().map(|m| m.total_bytes).unwrap_or(0)
    }
}

/* ---------------- GPU ---------------- */

/// "32.0.16.1714 (2026-07-20)" from the display class key whose DriverDesc matches `name`.
fn gpu_driver(name: &str) -> Option<String> {
    for sub in registry::subkeys(Root::Hklm, DISPLAY_CLASS).unwrap_or_default() {
        if !sub.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let path = format!(r"{DISPLAY_CLASS}\{sub}");
        if reg_str(Root::Hklm, &path, "DriverDesc").as_deref() != Some(name) {
            continue;
        }
        let version = reg_str(Root::Hklm, &path, "DriverVersion")?;
        // DriverDate is "M-D-YYYY".
        let date = reg_str(Root::Hklm, &path, "DriverDate").and_then(|d| {
            let mut it = d.split('-');
            let (m, day, y) = (it.next()?, it.next()?, it.next()?);
            Some(format!("{y}-{:0>2}-{:0>2}", m, day))
        });
        return Some(match date {
            Some(d) => format!("{version} ({d})"),
            None => version,
        });
    }
    None
}

pub fn gpus() -> Vec<Gpu> {
    let mut out: Vec<Gpu> = Vec::new();
    // SAFETY: DXGI factory enumeration; every interface is released by drop.
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return out;
        };
        for i in 0..8 {
            let Ok(adapter) = factory.EnumAdapters1(i) else {
                break;
            };
            let Ok(desc) = adapter.GetDesc1() else {
                continue;
            };
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let end = desc.Description.iter().position(|&c| c == 0).unwrap_or(desc.Description.len());
            let vendor = match desc.VendorId {
                0x10DE => "nvidia",
                0x1002 | 0x1022 => "amd",
                0x8086 => "intel",
                _ => "other",
            };
            let name = String::from_utf16_lossy(&desc.Description[..end]);
            // DXGI can list the same card twice (one entry per LUID alias); keep one per name+VRAM.
            if out.iter().any(|g| g.name == name && g.vram_bytes == desc.DedicatedVideoMemory as u64) {
                continue;
            }
            let nv = if vendor == "nvidia" && out.iter().all(|g| g.vendor != "nvidia") { nvml::get() } else { None };
            out.push(Gpu {
                driver: gpu_driver(&name).or_else(|| nv.and_then(|n| n.driver_version())),
                pcie: nv.and_then(|n| n.pcie()).map(|(g, w)| format!("PCIe {g}.0 x{w}")),
                power_limit_w: nv.and_then(|n| n.power_limit_w()),
                name,
                vram_bytes: desc.DedicatedVideoMemory as u64,
                vendor,
            });
        }
    }
    out
}

/* ---------------- monitors ---------------- */

fn monitors() -> Vec<MonitorInfo> {
    let mut n_paths = 0u32;
    let mut n_modes = 0u32;
    // SAFETY: size probe for the active-paths query.
    if unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut n_paths, &mut n_modes) } != ERROR_SUCCESS || n_paths == 0 {
        return Vec::new();
    }
    // SAFETY: zeroed POD arrays sized by the probe; filled by QueryDisplayConfig.
    let mut paths: Vec<DISPLAYCONFIG_PATH_INFO> = vec![unsafe { std::mem::zeroed() }; n_paths as usize];
    let mut modes: Vec<DISPLAYCONFIG_MODE_INFO> = vec![unsafe { std::mem::zeroed() }; n_modes as usize];
    let rc = unsafe { QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut n_paths, paths.as_mut_ptr(), &mut n_modes, modes.as_mut_ptr(), None) };
    if rc != ERROR_SUCCESS {
        return Vec::new();
    }
    let mut out = Vec::new();
    for p in &paths[..n_paths as usize] {
        // SAFETY: zeroed request with the header filled as the API requires.
        let mut req: DISPLAYCONFIG_TARGET_DEVICE_NAME = unsafe { std::mem::zeroed() };
        req.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
        req.header.size = size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
        req.header.adapterId = p.targetInfo.adapterId;
        req.header.id = p.targetInfo.id;
        let name = if unsafe { DisplayConfigGetDeviceInfo(&mut req.header) } == 0 {
            let end = req.monitorFriendlyDeviceName.iter().position(|&c| c == 0).unwrap_or(req.monitorFriendlyDeviceName.len());
            String::from_utf16_lossy(&req.monitorFriendlyDeviceName[..end])
        } else {
            String::new()
        };
        let (width, height) = modes[..n_modes as usize]
            .iter()
            .find(|m| m.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE && m.id == p.sourceInfo.id && m.adapterId == p.sourceInfo.adapterId)
            // SAFETY: sourceMode is the active union member for SOURCE entries.
            .map(|m| unsafe { (m.Anonymous.sourceMode.width, m.Anonymous.sourceMode.height) })
            .unwrap_or((0, 0));
        let r = p.targetInfo.refreshRate;
        let refresh_hz = if r.Denominator > 0 { r.Numerator as f32 / r.Denominator as f32 } else { 0.0 };
        let max_refresh_hz = gdi_device_name(p).map(|dev| max_refresh_at(&dev, width, height)).unwrap_or(0);
        out.push(MonitorInfo {
            name: if name.is_empty() { format!("Monitor {}", out.len() + 1) } else { name },
            width,
            height,
            refresh_hz,
            max_refresh_hz,
        });
    }
    out
}

/// `\\.\DISPLAY1` for a path's source (needed by `EnumDisplaySettingsW`).
fn gdi_device_name(p: &DISPLAYCONFIG_PATH_INFO) -> Option<Vec<u16>> {
    // SAFETY: zeroed request with the header filled as the API requires.
    let mut req: DISPLAYCONFIG_SOURCE_DEVICE_NAME = unsafe { std::mem::zeroed() };
    req.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
    req.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
    req.header.adapterId = p.sourceInfo.adapterId;
    req.header.id = p.sourceInfo.id;
    if unsafe { DisplayConfigGetDeviceInfo(&mut req.header) } != 0 {
        return None;
    }
    let end = req.viewGdiDeviceName.iter().position(|&c| c == 0).unwrap_or(req.viewGdiDeviceName.len());
    let mut v = req.viewGdiDeviceName[..end].to_vec();
    v.push(0);
    Some(v)
}

/// Highest refresh rate the driver offers at this resolution ("the monitor supports 144 Hz").
fn max_refresh_at(device: &[u16], width: u32, height: u32) -> u32 {
    let mut best = 0u32;
    for i in 0..2000u32 {
        // SAFETY: DEVMODEW with dmSize set; the call fills it for mode index `i`.
        let mut dm: DEVMODEW = unsafe { std::mem::zeroed() };
        dm.dmSize = size_of::<DEVMODEW>() as u16;
        if !unsafe { EnumDisplaySettingsW(PCWSTR(device.as_ptr()), ENUM_DISPLAY_SETTINGS_MODE(i), &mut dm) }.as_bool() {
            break;
        }
        if dm.dmPelsWidth == width && dm.dmPelsHeight == height {
            best = best.max(dm.dmDisplayFrequency);
        }
    }
    best
}

/* ---------------- SMBIOS ---------------- */

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Smbios {
    pub system: Option<String>,
    pub board: Option<String>,
    pub chassis: Option<u8>,
    pub bios: Option<Bios>,
    pub memory_slots: Option<u16>,
    pub memory: Vec<MemoryModule>,
}

fn read_smbios() -> Option<Smbios> {
    // SAFETY: size probe, then a buffer of that size.
    let size = unsafe { GetSystemFirmwareTable(RSMB, 0, None) };
    if size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    let got = unsafe { GetSystemFirmwareTable(RSMB, 0, Some(&mut buf)) };
    if got == 0 {
        return None;
    }
    Some(parse_smbios(&buf[..(got as usize).min(buf.len())]))
}

fn memory_type_label(t: u8) -> String {
    match t {
        0x18 => "DDR3",
        0x1A => "DDR4",
        0x1B => "LPDDR",
        0x1C => "LPDDR2",
        0x1D => "LPDDR3",
        0x1E => "LPDDR4",
        0x22 => "DDR5",
        0x23 => "LPDDR5",
        0x02 => "Unknown",
        _ => "DRAM",
    }
    .to_string()
}

fn form_factor_label(f: u8) -> String {
    match f {
        0x09 => "DIMM",
        0x0D => "SO-DIMM",
        0x0F => "FB-DIMM",
        0x0E => "Die",
        _ => "",
    }
    .to_string()
}

/// "CMK16GX4M1E3200C16" → 3200, "KF560C36BBEK2-32" → 5600 (two-digit hundreds → ×100). Best effort.
pub(crate) fn rated_from_part_number(part: &str) -> Option<u32> {
    let bytes = part.as_bytes();
    let mut i = 0;
    let mut best: Option<u32> = None;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let n: u32 = part[start..i].parse().unwrap_or(0);
            let candidate = match i - start {
                4 if (1600..=9000).contains(&n) => Some(n),
                // Kingston "KF432…" / "KF560…": DDR generation digit followed by speed ÷ 100.
                3 if (3..=5).contains(&(n / 100)) && (16..=90).contains(&(n % 100)) => Some((n % 100) * 100),
                _ => None,
            };
            if let Some(c) = candidate {
                best = Some(best.map_or(c, |b| b.max(c)));
            }
        } else {
            i += 1;
        }
    }
    best
}

/// `raw` is the RawSMBIOSData blob: 8-byte header, then the structure table.
pub(crate) fn parse_smbios(raw: &[u8]) -> Smbios {
    let mut info = Smbios::default();
    let Some(table) = raw.get(8..) else {
        return info;
    };
    let mut pos = 0usize;
    while pos + 4 <= table.len() {
        let kind = table[pos];
        let len = table[pos + 1] as usize;
        if len < 4 || pos + len > table.len() {
            break;
        }
        // Strings follow the formatted area, each NUL-terminated, list ends with an extra NUL.
        let mut strings: Vec<String> = Vec::new();
        let mut p = pos + len;
        loop {
            let start = p;
            while p < table.len() && table[p] != 0 {
                p += 1;
            }
            if p >= table.len() {
                break;
            }
            if p == start {
                p += 1; // the terminating double NUL
                break;
            }
            strings.push(String::from_utf8_lossy(&table[start..p]).trim().to_string());
            p += 1;
        }
        if strings.is_empty() {
            p += 1; // no strings: the section is exactly "\0\0"
        }
        let s = |idx: u8| -> Option<String> {
            if idx == 0 { None } else { strings.get(idx as usize - 1).filter(|v| !v.is_empty()).cloned() }
        };
        let b = |off: usize| if off < len { table[pos + off] } else { 0 };
        let w = |off: usize| if off + 1 < len { u16::from_le_bytes([table[pos + off], table[pos + off + 1]]) } else { 0 };
        let d = |off: usize| if off + 3 < len { u32::from_le_bytes([table[pos + off], table[pos + off + 1], table[pos + off + 2], table[pos + off + 3]]) } else { 0 };
        match kind {
            0 if len >= 9 => {
                info.bios = Some(Bios { vendor: s(b(4)).unwrap_or_default(), version: s(b(5)).unwrap_or_default(), date: s(b(8)).unwrap_or_default() });
            }
            1 if len >= 6 => {
                let v = [s(b(4)), s(b(5))].into_iter().flatten().collect::<Vec<_>>().join(" ");
                if !v.is_empty() {
                    info.system = Some(v);
                }
            }
            2 if len >= 6 => {
                let v = [s(b(4)), s(b(5))].into_iter().flatten().collect::<Vec<_>>().join(" ");
                if !v.is_empty() {
                    info.board = Some(v);
                }
            }
            3 if len >= 6 => info.chassis = Some(b(5) & 0x7F),
            16 if len >= 0x0F => {
                let n = w(0x0D);
                if n > 0 {
                    info.memory_slots = Some(info.memory_slots.unwrap_or(0) + n);
                }
            }
            17 if len >= 0x15 => {
                let raw_size = w(0x0C);
                let size_bytes = match raw_size {
                    0 | 0xFFFF => 0,
                    0x7FFF => u64::from(d(0x1C)) * 1024 * 1024,
                    v if v & 0x8000 != 0 => u64::from(v & 0x7FFF) * 1024,
                    v => u64::from(v) * 1024 * 1024,
                };
                let part_number = s(b(0x1A)).unwrap_or_default();
                info.memory.push(MemoryModule {
                    slot: s(b(0x10)).unwrap_or_default(),
                    bank: s(b(0x11)).unwrap_or_default(),
                    size_bytes,
                    speed_mts: u32::from(w(0x15)),
                    configured_mts: u32::from(w(0x20)),
                    rated_mts: rated_from_part_number(&part_number),
                    kind: memory_type_label(b(0x12)),
                    form_factor: form_factor_label(b(0x0E)),
                    manufacturer: s(b(0x17)).unwrap_or_default(),
                    part_number,
                });
            }
            127 => break,
            _ => {}
        }
        if p <= pos {
            break;
        }
        pos = p;
    }
    info
}

/// SMBIOS chassis types that mean "portable": laptop, notebook, hand held, sub notebook,
/// convertible, detachable, tablet. Falls back to "has a battery".
fn is_laptop(chassis: Option<u8>) -> bool {
    match chassis {
        Some(8..=11 | 14 | 30..=32) => true,
        Some(3..=7 | 13 | 15..=17 | 23..=24 | 35) => false,
        _ => {
            let mut st = SYSTEM_POWER_STATUS::default();
            // SAFETY: plain out-param.
            unsafe { GetSystemPowerStatus(&mut st) }.is_ok() && st.BatteryFlag != 128 && st.BatteryFlag != 255
        }
    }
}

/* ---------------- disks ---------------- */

struct Device(HANDLE);

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: handle from CreateFileW, closed once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Open a device path with no access rights: enough for the metadata IOCTLs below, no admin.
fn open_device(path: &str) -> Option<Device> {
    let w = wide(path);
    // SAFETY: valid path; 0 desired access.
    unsafe { CreateFileW(PCWSTR(w.as_ptr()), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) }
        .ok()
        .map(Device)
}

fn ioctl(dev: &Device, code: u32, input: Option<&[u8]>, out: &mut [u8]) -> Option<usize> {
    let mut ret = 0u32;
    // SAFETY: buffers are valid for the sizes passed; synchronous call.
    unsafe {
        DeviceIoControl(
            dev.0,
            code,
            input.map(|i| i.as_ptr().cast::<c_void>()),
            input.map_or(0, |i| i.len() as u32),
            Some(out.as_mut_ptr().cast::<c_void>()),
            out.len() as u32,
            Some(&mut ret),
            None,
        )
    }
    .ok()?;
    Some(ret as usize)
}

fn property_query(id: windows::Win32::System::Ioctl::STORAGE_PROPERTY_ID) -> Vec<u8> {
    let q = STORAGE_PROPERTY_QUERY { PropertyId: id, QueryType: PropertyStandardQuery, AdditionalParameters: [0] };
    // SAFETY: plain-old-data struct viewed as bytes for the IOCTL input buffer.
    unsafe { std::slice::from_raw_parts((&q as *const STORAGE_PROPERTY_QUERY).cast::<u8>(), size_of::<STORAGE_PROPERTY_QUERY>()) }.to_vec()
}

fn cstr_at(buf: &[u8], off: u32) -> Option<String> {
    let off = off as usize;
    if off == 0 || off >= buf.len() {
        return None;
    }
    let end = buf[off..].iter().position(|&b| b == 0).map_or(buf.len(), |p| off + p);
    let s = String::from_utf8_lossy(&buf[off..end]).trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn bus_label(b: STORAGE_BUS_TYPE) -> &'static str {
    [(BusTypeNvme, "nvme"), (BusTypeSata, "sata"), (BusTypeUsb, "usb"), (BusTypeSas, "sas"), (BusTypeRAID, "raid")]
        .into_iter()
        .find(|(k, _)| *k == b)
        .map_or("other", |(_, label)| label)
}

fn system_disk_number() -> Option<u32> {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let dev = open_device(&format!(r"\\.\{drive}"))?;
    let mut out = vec![0u8; 1024];
    ioctl(&dev, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, None, &mut out)?;
    // SAFETY: the IOCTL filled a VOLUME_DISK_EXTENTS at the start of the buffer.
    let ext = unsafe { std::ptr::read_unaligned(out.as_ptr().cast::<VOLUME_DISK_EXTENTS>()) };
    (ext.NumberOfDiskExtents >= 1).then_some(ext.Extents[0].DiskNumber)
}

/// Every disk a lettered volume lives on (spanned/striped volumes have several). Empty = unknown.
pub(crate) fn volume_disks(letter: char) -> Vec<u32> {
    if !letter.is_ascii_alphabetic() {
        return Vec::new();
    }
    let Some(dev) = open_device(&format!(r"\\.\{letter}:")) else {
        return Vec::new();
    };
    let mut out = vec![0u8; 1024];
    if ioctl(&dev, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, None, &mut out).is_none() {
        return Vec::new();
    }
    // VOLUME_DISK_EXTENTS: u32 count, then 8-aligned DISK_EXTENTs of 24 bytes (DiskNumber first).
    let n = u32::from_le_bytes([out[0], out[1], out[2], out[3]]) as usize;
    (0..n.min((out.len() - 8) / 24))
        .map(|i| {
            let o = 8 + i * 24;
            u32::from_le_bytes([out[o], out[o + 1], out[o + 2], out[o + 3]])
        })
        .collect()
}

pub fn disks() -> Vec<Disk> {
    let system_disk = system_disk_number();
    let mut out = Vec::new();
    for index in 0..16u32 {
        let Some(dev) = open_device(&format!(r"\\.\PhysicalDrive{index}")) else {
            continue;
        };
        let mut buf = vec![0u8; 2048];
        let Some(_) = ioctl(&dev, IOCTL_STORAGE_QUERY_PROPERTY, Some(&property_query(StorageDeviceProperty)), &mut buf) else {
            continue;
        };
        // SAFETY: STORAGE_DEVICE_DESCRIPTOR header at the start of the buffer; strings addressed by offset.
        let desc = unsafe { std::ptr::read_unaligned(buf.as_ptr().cast::<STORAGE_DEVICE_DESCRIPTOR>()) };
        let model = [cstr_at(&buf, desc.VendorIdOffset), cstr_at(&buf, desc.ProductIdOffset)]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        let mut seek = vec![0u8; 64];
        let ssd = ioctl(&dev, IOCTL_STORAGE_QUERY_PROPERTY, Some(&property_query(StorageDeviceSeekPenaltyProperty)), &mut seek).map(|_| {
            // SAFETY: filled DEVICE_SEEK_PENALTY_DESCRIPTOR.
            let d = unsafe { std::ptr::read_unaligned(seek.as_ptr().cast::<DEVICE_SEEK_PENALTY_DESCRIPTOR>()) };
            !d.IncursSeekPenalty
        });
        let mut geo = vec![0u8; 256];
        let size_bytes = ioctl(&dev, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, None, &mut geo)
            .map(|_| {
                // SAFETY: filled DISK_GEOMETRY_EX.
                let g = unsafe { std::ptr::read_unaligned(geo.as_ptr().cast::<DISK_GEOMETRY_EX>()) };
                g.DiskSize.max(0) as u64
            })
            .unwrap_or(0);
        out.push(Disk {
            index,
            model: if model.is_empty() { format!("Disk {index}") } else { model },
            bus: bus_label(desc.BusType),
            size_bytes,
            ssd,
            system: system_disk == Some(index),
        });
    }
    out
}

/* ---------------- firmware security ---------------- */

fn secure_boot() -> Option<bool> {
    reg_dword(Root::Hklm, r"SYSTEM\CurrentControlSet\Control\SecureBoot\State", "UEFISecureBootEnabled").map(|v| v == 1)
}

fn tpm_version() -> Option<String> {
    let mut info = TPM_DEVICE_INFO::default();
    // SAFETY: sized out-param; 0 = TBS_SUCCESS.
    let rc = unsafe { Tbsi_GetDeviceInfo(size_of::<TPM_DEVICE_INFO>() as u32, (&mut info as *mut TPM_DEVICE_INFO).cast::<c_void>()) };
    if rc != 0 {
        return None;
    }
    Some(match info.tpmVersion {
        2 => "2.0".into(),
        1 => "1.2".into(),
        v => format!("{v}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hags_state_answers() {
        // DXGI always lists at least the software adapter, so the query itself never fails.
        let s = hags_state();
        println!("hags (supported, enabled) = {s:?}");
        assert!(s.is_some());
    }

    fn smbios_blob() -> Vec<u8> {
        // RawSMBIOSData header (8 bytes) + Type 0 + Type 1 + Type 2 + Type 3 + Type 16 + Type 17 ×2 + Type 127.
        let mut v = vec![0u8, 3, 4, 0, 0, 0, 0, 0];
        // Type 0 (BIOS): vendor @4=1, version @5=2, date @8=3; length 24.
        let mut t0 = vec![0u8; 24];
        t0[0] = 0;
        t0[1] = 24;
        t0[4] = 1;
        t0[5] = 2;
        t0[8] = 3;
        v.extend(t0);
        v.extend(b"American Megatrends International, LLC.\x00F10\x0007/06/2023\x00\x00");
        v.extend([1, 8, 1, 0, 1, 2, 0, 0]);
        v.extend(b"Gigabyte\0X870\0\0");
        v.extend([2, 8, 2, 0, 1, 2, 0, 0]);
        v.extend(b"Gigabyte Technology Co., Ltd.\0X870 AORUS ELITE\0\0");
        v.extend([3, 8, 3, 0, 1, 3, 0, 0]);
        v.extend(b"Default string\0\0");
        // Type 16: number of devices @0x0D = 4; length 0x17.
        let mut t16 = vec![0u8; 0x17];
        t16[0] = 16;
        t16[1] = 0x17;
        t16[0x0D] = 4;
        v.extend(t16);
        v.extend(b"\0\0");
        // Type 17 ×2: 16 GB DDR4 @2133 configured 3200, Corsair CMK16GX4M1E3200C16; then an empty slot.
        let mut t17 = vec![0u8; 0x28];
        t17[0] = 17;
        t17[1] = 0x28;
        t17[0x0C] = (16384u16 & 0xFF) as u8;
        t17[0x0D] = (16384u16 >> 8) as u8;
        t17[0x0E] = 0x09;
        t17[0x10] = 1;
        t17[0x11] = 2;
        t17[0x12] = 0x1A;
        t17[0x15] = (2133u16 & 0xFF) as u8;
        t17[0x16] = (2133u16 >> 8) as u8;
        t17[0x17] = 3;
        t17[0x1A] = 4;
        t17[0x20] = (3200u16 & 0xFF) as u8;
        t17[0x21] = (3200u16 >> 8) as u8;
        v.extend(t17);
        v.extend(b"DIMM_A2\0BANK 1\0Corsair\0CMK16GX4M1E3200C16\0\0");
        let mut empty = vec![0u8; 0x28];
        empty[0] = 17;
        empty[1] = 0x28;
        empty[0x10] = 1;
        v.extend(empty);
        v.extend(b"DIMM_B1\0\0");
        v.extend([127, 4, 4, 0]);
        v.extend([0, 0]);
        v
    }

    #[test]
    fn smbios_parser_reads_everything_we_use() {
        let s = parse_smbios(&smbios_blob());
        assert_eq!(s.system.as_deref(), Some("Gigabyte X870"));
        assert_eq!(s.board.as_deref(), Some("Gigabyte Technology Co., Ltd. X870 AORUS ELITE"));
        assert_eq!(s.chassis, Some(3));
        assert_eq!(s.bios, Some(Bios { vendor: "American Megatrends International, LLC.".into(), version: "F10".into(), date: "07/06/2023".into() }));
        assert_eq!(s.memory_slots, Some(4));
        assert_eq!(s.memory.len(), 2);
        let m = &s.memory[0];
        assert_eq!((m.size_bytes, m.speed_mts, m.configured_mts, m.rated_mts), (16 << 30, 2133, 3200, Some(3200)));
        assert_eq!((m.kind.as_str(), m.form_factor.as_str(), m.slot.as_str(), m.bank.as_str()), ("DDR4", "DIMM", "DIMM_A2", "BANK 1"));
        assert_eq!(s.memory[1].size_bytes, 0, "empty slot");
        assert_eq!(parse_smbios(&[0u8; 4]), Smbios::default());
        assert!(!is_laptop(Some(3)));
        assert!(is_laptop(Some(10)));
    }

    #[test]
    fn rated_speed_from_part_numbers() {
        assert_eq!(rated_from_part_number("CMK16GX4M1E3200C16"), Some(3200));
        assert_eq!(rated_from_part_number("KF560C36BBEK2-32"), Some(6000));
        assert_eq!(rated_from_part_number("KF432C16BB/8"), Some(3200));
        assert_eq!(rated_from_part_number("F4-3600C16-16GTZNC"), Some(3600));
        assert_eq!(rated_from_part_number("HMA81GU6CJR8N-VK"), None);
        assert_eq!(rated_from_part_number(""), None);
    }

    #[test]
    fn live_profile_is_sane_without_admin() {
        let p = profile();
        assert!(!p.computer.is_empty());
        assert!(p.os.starts_with("Windows"));
        assert!(p.cores_logical >= 1 && p.cores_physical >= 1 && p.cores_physical <= p.cores_logical);
        assert!(p.ram_bytes > 1 << 30);
        assert!(!p.disks.is_empty(), "at least the system disk");
        assert!(p.disks.iter().any(|d| d.system), "system disk identified");
        assert!(p.disks.iter().all(|d| d.size_bytes > 0));
        assert!(p.system_disk_ssd.is_some());
        assert!(!p.gpus.is_empty());
        assert!(p.uptime_secs > 0);
        assert!(p.cpu_detail.flags.contains(&"SSE2"), "x86-64 always has SSE2");
        assert!(p.cpu_detail.family > 0);
        assert!(p.cpu_detail.caches.iter().any(|c| c.level == 1), "L1 reported");
        assert!(!p.monitors.is_empty() && p.monitors[0].width > 0 && p.monitors[0].refresh_hz > 10.0);
        assert!(p.memory.iter().any(|m| m.size_bytes > 0), "at least one populated DIMM");
        assert!(p.bios.as_ref().is_some_and(|b| !b.version.is_empty()));
    }
}
