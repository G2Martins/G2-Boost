//! Hardware / OS profile ("Meu PC"). Read-only and without admin: registry, SMBIOS
//! (`GetSystemFirmwareTable`), DXGI, storage IOCTLs, TPM base services, Secure Boot state.
//! The SSD/HDD answer here also drives the SysMain recommendation.

use std::ffi::c_void;
use std::mem::size_of;

use serde::Serialize;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1};
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
    ComputerNameDnsHostname, GetComputerNameExW, GetLogicalProcessorInformationEx, GetPhysicallyInstalledSystemMemory,
    GetSystemFirmwareTable, GetTickCount64, RSMB, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};
use windows::Win32::System::Threading::{ALL_PROCESSOR_GROUPS, GetActiveProcessorCount};
use windows::Win32::System::TpmBaseServices::{TPM_DEVICE_INFO, Tbsi_GetDeviceInfo};
use windows::core::{PCWSTR, PWSTR};

use crate::registry::{self, Root, Value, wide};
use crate::system;

#[derive(Debug, Clone, Serialize)]
pub struct Gpu {
    pub name: String,
    pub vram_bytes: u64,
    /// `nvidia` | `amd` | `intel` | `other`
    pub vendor: &'static str,
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

#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    pub computer: String,
    /// "Windows 11 Pro 24H2 (build 26200.1234)"
    pub os: String,
    pub cpu: String,
    pub cores_physical: u32,
    pub cores_logical: u32,
    pub ram_bytes: u64,
    pub uptime_secs: u64,
    pub gpus: Vec<Gpu>,
    pub board: Option<String>,
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
    Profile {
        computer: computer_name(),
        os: os_string(),
        cpu: cpu_name(),
        cores_physical: physical_cores(),
        cores_logical: unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) },
        ram_bytes: installed_ram(),
        uptime_secs: unsafe { GetTickCount64() } / 1000,
        gpus: gpus(),
        board: smbios.board,
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
    reg_str(Root::Hklm, r"HARDWARE\DESCRIPTION\System\CentralProcessor\0", "ProcessorNameString")
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_else(|| "CPU".into())
}

fn physical_cores() -> u32 {
    let mut len = 0u32;
    // SAFETY: size probe (fails with ERROR_INSUFFICIENT_BUFFER and fills `len`).
    let _ = unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, None, &mut len) };
    if len == 0 {
        return 0;
    }
    let mut buf = vec![0u8; len as usize];
    // SAFETY: buffer of `len` bytes, 8-byte aligned enough for the variable-size records we walk by `Size`.
    if unsafe { GetLogicalProcessorInformationEx(RelationProcessorCore, Some(buf.as_mut_ptr().cast()), &mut len) }.is_err() {
        return 0;
    }
    let mut cores = 0u32;
    let mut pos = 0usize;
    while pos + 8 <= len as usize {
        // SAFETY: each record starts with Relationship (u32) + Size (u32); we only read those.
        let rec = unsafe { std::ptr::read_unaligned(buf.as_ptr().add(pos).cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>()) };
        if rec.Relationship == RelationProcessorCore {
            cores += 1;
        }
        if rec.Size == 0 {
            break;
        }
        pos += rec.Size as usize;
    }
    cores
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

fn gpus() -> Vec<Gpu> {
    let mut out = Vec::new();
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
            let gpu = Gpu { name: String::from_utf16_lossy(&desc.Description[..end]), vram_bytes: desc.DedicatedVideoMemory as u64, vendor };
            // DXGI can list the same card twice (one entry per LUID alias); keep one per name+VRAM.
            if !out.iter().any(|g: &Gpu| g.name == gpu.name && g.vram_bytes == gpu.vram_bytes) {
                out.push(gpu);
            }
        }
    }
    out
}

/* ---------------- SMBIOS ---------------- */

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Smbios {
    pub system: Option<String>,
    pub board: Option<String>,
    pub chassis: Option<u8>,
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
        let s = |idx: u8| -> Option<String> {
            if idx == 0 { None } else { strings.get(idx as usize - 1).filter(|v| !v.is_empty()).cloned() }
        };
        let field = |off: usize| table.get(pos + off).copied().unwrap_or(0);
        match kind {
            1 if len >= 6 => {
                let v = [s(field(4)), s(field(5))].into_iter().flatten().collect::<Vec<_>>().join(" ");
                if !v.is_empty() {
                    info.system = Some(v);
                }
            }
            2 if len >= 6 => {
                let v = [s(field(4)), s(field(5))].into_iter().flatten().collect::<Vec<_>>().join(" ");
                if !v.is_empty() {
                    info.board = Some(v);
                }
            }
            3 if len >= 6 => info.chassis = Some(field(5) & 0x7F),
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

    fn smbios_blob() -> Vec<u8> {
        // RawSMBIOSData header (8 bytes) + Type 1 + Type 2 + Type 3 + Type 127.
        let mut v = vec![0u8, 3, 4, 0, 0, 0, 0, 0];
        // Type 1 (System): length 27 in real tables; 8 is enough for our reader (manufacturer @4, product @5).
        v.extend([1, 8, 1, 0, 1, 2, 0, 0]);
        v.extend(b"Gigabyte\0X870\0\0");
        // Type 2 (Baseboard)
        v.extend([2, 8, 2, 0, 1, 2, 0, 0]);
        v.extend(b"Gigabyte Technology Co., Ltd.\0X870 AORUS ELITE\0\0");
        // Type 3 (Chassis): type @5 = 3 (desktop)
        v.extend([3, 8, 3, 0, 1, 3, 0, 0]);
        v.extend(b"Default string\0\0");
        v.extend([127, 4, 4, 0]);
        v.extend([0, 0]);
        v
    }

    #[test]
    fn smbios_parser_reads_system_board_and_chassis() {
        let s = parse_smbios(&smbios_blob());
        assert_eq!(s.system.as_deref(), Some("Gigabyte X870"));
        assert_eq!(s.board.as_deref(), Some("Gigabyte Technology Co., Ltd. X870 AORUS ELITE"));
        assert_eq!(s.chassis, Some(3));
        assert_eq!(parse_smbios(&[0u8; 4]), Smbios::default());
        assert!(!is_laptop(Some(3)));
        assert!(is_laptop(Some(10)));
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
    }
}
