//! BIOS / firmware settings that Windows *can* tell us without a driver: boot mode (UEFI vs
//! legacy CSM), Secure Boot, TPM, virtualization enabled in firmware, Resizable BAR (from the
//! GPU's BAR1 size), memory running at its rated (XMP/EXPO) speed, BIOS age, Fast Startup and
//! hibernation. Settings with no OS-visible trace (C-states, power limits, fan curves) are not
//! readable from Windows and are reported as such.

use serde::Serialize;
use windows::Win32::System::SystemInformation::{FIRMWARE_TYPE, FirmwareTypeBios, FirmwareTypeUefi, GetFirmwareType};
use windows::Win32::System::Threading::{IsProcessorFeaturePresent, PF_VIRT_FIRMWARE_ENABLED};

use crate::hardware::Profile;
use crate::nvml;
use crate::registry::{self, Root, Value};

#[derive(Debug, Clone, Serialize, Default)]
pub struct Firmware {
    /// "uefi" | "legacy"
    pub boot_mode: Option<&'static str>,
    pub secure_boot: Option<bool>,
    pub tpm: Option<String>,
    /// VT-x / AMD-V switched on in the BIOS (as Windows reports it).
    pub virtualization_enabled: bool,
    /// A hypervisor is running (Hyper-V/VBS/HVCI make Windows itself a guest).
    pub hypervisor_present: bool,
    /// Resizable BAR (NVIDIA only for now; None = unknown).
    pub resizable_bar: Option<bool>,
    pub bar1_mb: Option<u64>,
    /// RAM at the speed its part number is rated for (None = cannot tell).
    pub memory_at_rated_speed: Option<bool>,
    pub bios_vendor: Option<String>,
    pub bios_version: Option<String>,
    pub bios_date: Option<String>,
    pub bios_age_days: Option<u32>,
    pub fast_startup: Option<bool>,
    pub hibernation: Option<bool>,
}

fn dword(path: &str, name: &str) -> Option<u32> {
    match registry::read(Root::Hklm, path, name) {
        Ok(Value::Dword(d)) => Some(d),
        _ => None,
    }
}

fn boot_mode() -> Option<&'static str> {
    let mut t = FIRMWARE_TYPE::default();
    // SAFETY: plain out-param.
    unsafe { GetFirmwareType(&mut t) }.ok()?;
    if t == FirmwareTypeUefi {
        Some("uefi")
    } else if t == FirmwareTypeBios {
        Some("legacy")
    } else {
        None
    }
}

#[cfg(target_arch = "x86_64")]
fn hypervisor_bit() -> bool {
    // CPUID leaf 1 is always available on x86-64; ECX bit 31 = hypervisor present.
    std::arch::x86_64::__cpuid(1).ecx & (1 << 31) != 0
}

#[cfg(not(target_arch = "x86_64"))]
fn hypervisor_bit() -> bool {
    false
}

/// SMBIOS dates are "MM/DD/YYYY".
pub(crate) fn bios_age_days(date: &str) -> Option<u32> {
    let mut it = date.trim().split('/');
    let (m, d, y): (i64, i64, i64) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || y < 1990 {
        return None;
    }
    let then = crate::advisor::days_from_civil(y, m, d);
    let today = (registry::now_unix() / 86_400) as i64;
    u32::try_from(today - then).ok()
}

pub fn read(p: &Profile) -> Firmware {
    let bar1 = p.gpus.iter().any(|g| g.vendor == "nvidia").then(nvml::get).flatten().and_then(|n| n.bar1_total_bytes());
    let rated: Vec<bool> = p
        .memory
        .iter()
        .filter(|m| m.size_bytes > 0 && m.configured_mts > 0)
        .filter_map(|m| m.rated_mts.map(|r| m.configured_mts >= r))
        .collect();
    let hypervisor_present = hypervisor_bit();
    Firmware {
        boot_mode: boot_mode(),
        secure_boot: p.secure_boot,
        tpm: p.tpm.clone(),
        // Under Hyper-V/VBS Windows is itself a guest and the flag reads false, yet a running
        // hypervisor proves VT-x/AMD-V is on in the firmware.
        // SAFETY: simple feature query.
        virtualization_enabled: hypervisor_present || unsafe { IsProcessorFeaturePresent(PF_VIRT_FIRMWARE_ENABLED) }.as_bool(),
        hypervisor_present,
        resizable_bar: bar1.map(|b| b > 300 * 1024 * 1024),
        bar1_mb: bar1.map(|b| b / (1024 * 1024)),
        memory_at_rated_speed: if rated.is_empty() { None } else { Some(rated.iter().all(|x| *x)) },
        bios_vendor: p.bios.as_ref().map(|b| b.vendor.clone()),
        bios_version: p.bios.as_ref().map(|b| b.version.clone()),
        bios_date: p.bios.as_ref().map(|b| b.date.clone()),
        bios_age_days: p.bios.as_ref().and_then(|b| bios_age_days(&b.date)),
        fast_startup: dword(r"SYSTEM\CurrentControlSet\Control\Session Manager\Power", "HiberbootEnabled").map(|v| v == 1),
        hibernation: dword(r"SYSTEM\CurrentControlSet\Control\Power", "HibernateEnabled").map(|v| v == 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bios_dates() {
        assert!(bios_age_days("07/06/2023").is_some_and(|d| d > 365));
        assert!(bios_age_days("13/01/2023").is_none());
        assert!(bios_age_days("garbage").is_none());
    }

    #[test]
    fn live_read_without_admin() {
        let f = read(&crate::hardware::profile());
        assert!(f.boot_mode.is_some(), "GetFirmwareType answers on every supported Windows");
        assert!(f.bios_version.is_some());
    }
}
