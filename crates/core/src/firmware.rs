//! BIOS / firmware settings that Windows *can* tell us without a driver: boot mode (UEFI vs
//! legacy CSM), Secure Boot, TPM, virtualization enabled in firmware, Resizable BAR (from the
//! GPU's BAR1 size), memory running at its rated (XMP/EXPO) speed, BIOS age, Fast Startup and
//! hibernation. Settings with no OS-visible trace (C-states, power limits, fan curves) are not
//! readable from Windows and are reported as such.

use serde::Serialize;
use windows::Win32::System::SystemInformation::{FIRMWARE_TYPE, FirmwareTypeBios, FirmwareTypeUefi, GetFirmwareType};
use windows::Win32::System::Threading::{IsProcessorFeaturePresent, PF_VIRT_FIRMWARE_ENABLED};
use windows::core::PCWSTR;

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

/* ---------------- BIOS guide ---------------- */

/// Motherboard makers with their own menu layout; OEM desktops and laptops get generic paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Vendor {
    Asus,
    Msi,
    Gigabyte,
    Asrock,
    Generic,
}

/// From the SMBIOS baseboard string ("Micro-Star International Co., Ltd. MAG B650 TOMAHAWK").
pub(crate) fn vendor_of(board: Option<&str>, laptop: bool) -> Vendor {
    let b = board.unwrap_or_default().to_ascii_lowercase();
    if laptop {
        Vendor::Generic // laptop firmware is the OEM's own, whatever the board says
    } else if b.contains("asus") {
        Vendor::Asus
    } else if b.contains("micro-star") || b.starts_with("msi") {
        Vendor::Msi
    } else if b.contains("gigabyte") || b.contains("aorus") {
        Vendor::Gigabyte
    } else if b.contains("asrock") {
        Vendor::Asrock
    } else {
        Vendor::Generic
    }
}

/// (vendor, key to enter setup, key for the advanced mode). Shown as-is.
const KEYS: &[(Vendor, &str, Option<&str>)] = &[
    (Vendor::Asus, "Del / F2", Some("F7")),
    (Vendor::Msi, "Del", Some("F7")),
    (Vendor::Gigabyte, "Del", Some("F2")),
    (Vendor::Asrock, "F2 / Del", Some("F6")),
    (Vendor::Generic, "Del / F2 / F10 / Esc", None),
];

/// (item, vendor, path on Intel, path on AMD): menu labels as the firmware shows them (English),
/// so they are not translated. Names vary a little between models and BIOS versions.
const PATHS: &[(&str, Vendor, &str, &str)] = &[
    ("csm", Vendor::Asus, "Boot › CSM (Compatibility Support Module) › Launch CSM: Disabled", "Boot › CSM (Compatibility Support Module) › Launch CSM: Disabled"),
    ("csm", Vendor::Msi, "Settings › Advanced › Windows OS Configuration › BIOS UEFI/CSM Mode: UEFI", "Settings › Advanced › Windows OS Configuration › BIOS UEFI/CSM Mode: UEFI"),
    ("csm", Vendor::Gigabyte, "Boot › CSM Support: Disabled", "Boot › CSM Support: Disabled"),
    ("csm", Vendor::Asrock, "Boot › CSM (Compatibility Support Module) › CSM: Disabled", "Boot › CSM (Compatibility Support Module) › CSM: Disabled"),
    ("csm", Vendor::Generic, "Boot › CSM / Legacy Support: Disabled (Boot Mode: UEFI)", "Boot › CSM / Legacy Support: Disabled (Boot Mode: UEFI)"),
    ("xmp", Vendor::Asus, "Ai Tweaker › Ai Overclock Tuner: XMP I", "Ai Tweaker › Ai Overclock Tuner: EXPO I / D.O.C.P."),
    ("xmp", Vendor::Msi, "OC › Extreme Memory Profile (XMP): Enabled", "OC › Extreme Memory Profile (A-XMP / EXPO): Profile 1"),
    ("xmp", Vendor::Gigabyte, "Tweaker › Extreme Memory Profile (X.M.P.): Profile1", "Tweaker › Extreme Memory Profile (X.M.P.) / EXPO: Profile1"),
    ("xmp", Vendor::Asrock, "OC Tweaker › DRAM Configuration › Load XMP Setting: XMP 1", "OC Tweaker › DRAM Profile Configuration › DRAM Profile Setting: EXPO / XMP"),
    ("xmp", Vendor::Generic, "Overclocking / Memory › XMP / EXPO / DOCP: Profile 1", "Overclocking / Memory › XMP / EXPO / DOCP: Profile 1"),
    ("rebar", Vendor::Asus, "Advanced › PCI Subsystem Settings › Above 4G Decoding: Enabled › Re-Size BAR Support: Auto", "Advanced › PCI Subsystem Settings › Above 4G Decoding: Enabled › Re-Size BAR Support: Auto"),
    ("rebar", Vendor::Msi, "Settings › Advanced › PCI Subsystem Settings › Above 4G memory: Enabled › Re-Size BAR Support: Enabled", "Settings › Advanced › PCI Subsystem Settings › Above 4G memory: Enabled › Re-Size BAR Support: Enabled"),
    ("rebar", Vendor::Gigabyte, "Settings › IO Ports › Above 4G Decoding: Enabled › Re-Size BAR Support: Auto", "Settings › IO Ports › Above 4G Decoding: Enabled › Re-Size BAR Support: Auto"),
    ("rebar", Vendor::Asrock, "Advanced › Chipset Configuration › Above 4G Decoding: Enabled › Re-Size BAR Support: Enabled", "Advanced › Chipset Configuration › Above 4G Decoding: Enabled › C.A.M. (Clever Access Memory): Enabled"),
    ("rebar", Vendor::Generic, "Advanced / PCI › Above 4G Decoding: Enabled › Resizable BAR: Enabled", "Advanced / PCI › Above 4G Decoding: Enabled › Resizable BAR / Smart Access Memory: Enabled"),
    ("secure_boot", Vendor::Asus, "Boot › Secure Boot › OS Type: Windows UEFI mode", "Boot › Secure Boot › OS Type: Windows UEFI mode"),
    ("secure_boot", Vendor::Msi, "Settings › Security › Secure Boot › Secure Boot: Enabled", "Settings › Security › Secure Boot › Secure Boot: Enabled"),
    ("secure_boot", Vendor::Gigabyte, "Boot › Secure Boot › Secure Boot: Enabled", "Boot › Secure Boot › Secure Boot: Enabled"),
    ("secure_boot", Vendor::Asrock, "Security › Secure Boot › Secure Boot: Enabled", "Security › Secure Boot › Secure Boot: Enabled"),
    ("secure_boot", Vendor::Generic, "Security / Boot › Secure Boot: Enabled", "Security / Boot › Secure Boot: Enabled"),
    ("tpm", Vendor::Asus, "Advanced › PCH-FW Configuration › PTT: Enable", "Advanced › AMD fTPM configuration › TPM Device Selection: Firmware TPM"),
    ("tpm", Vendor::Msi, "Settings › Security › Trusted Computing › Security Device Support: Enable (PTT)", "Settings › Security › Trusted Computing › AMD fTPM switch: AMD CPU fTPM"),
    ("tpm", Vendor::Gigabyte, "Settings › Miscellaneous › Intel Platform Trust Technology (PTT): Enabled", "Settings › Trusted Computing › AMD CPU fTPM: Enabled"),
    ("tpm", Vendor::Asrock, "Security › Intel Platform Trust Technology: Enabled", "Advanced › CPU Configuration › AMD fTPM switch: AMD CPU fTPM"),
    ("tpm", Vendor::Generic, "Security › TPM / PTT: Enabled", "Security › TPM / fTPM: Enabled"),
    ("bios_update", Vendor::Asus, "Tool › ASUS EZ Flash 3 Utility", "Tool › ASUS EZ Flash 3 Utility"),
    ("bios_update", Vendor::Msi, "M-FLASH", "M-FLASH"),
    ("bios_update", Vendor::Gigabyte, "Q-Flash", "Q-Flash"),
    ("bios_update", Vendor::Asrock, "Tool › Instant Flash", "Tool › Instant Flash"),
    ("bios_update", Vendor::Generic, "Tool / Utility › BIOS Flash (USB)", "Tool / Utility › BIOS Flash (USB)"),
    ("virtualization", Vendor::Asus, "Advanced › CPU Configuration › Intel (VMX) Virtualization Technology: Enabled", "Advanced › CPU Configuration › SVM Mode: Enabled"),
    ("virtualization", Vendor::Msi, "OC › CPU Features › Intel Virtualization Tech: Enabled", "OC › Advanced CPU Configuration › SVM Mode: Enabled"),
    ("virtualization", Vendor::Gigabyte, "Tweaker › Advanced CPU Settings › Intel Virtualization Technology: Enabled", "Tweaker › Advanced CPU Settings › SVM Mode: Enabled"),
    ("virtualization", Vendor::Asrock, "Advanced › CPU Configuration › Intel Virtualization Technology: Enabled", "Advanced › CPU Configuration › SVM Mode: Enabled"),
    ("virtualization", Vendor::Generic, "Advanced › CPU › Intel Virtualization Technology (VT-x): Enabled", "Advanced › CPU › SVM Mode (AMD-V): Enabled"),
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GuideItem {
    /// `csm`, `xmp`, `rebar`, `secure_boot`, `tpm`, `bios_update`, `virtualization` (i18n `bios.item.*`).
    pub id: &'static str,
    /// Where it lives in this maker's setup (firmware labels, untranslated).
    pub path: &'static str,
    /// Worth it only for some uses (virtualization: VMs, WSL, emulators).
    pub optional: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Guide {
    pub vendor: Vendor,
    pub enter_key: &'static str,
    pub advanced_key: Option<&'static str>,
    /// UEFI firmware: "Restart into the BIOS" can be offered.
    pub uefi: bool,
    /// Only what applies to this PC, in the order to do it.
    pub items: Vec<GuideItem>,
}

/// GPUs where Resizable BAR is a real setting (same list as the advisor's `rebar_off`).
fn rebar_capable(p: &Profile) -> bool {
    p.gpus.iter().any(|g| {
        let n = g.name.to_ascii_uppercase();
        ["RTX 30", "RTX 40", "RTX 50", "RX 6", "RX 7", "RX 9", "ARC"].iter().any(|k| n.contains(k))
    })
}

/// Pure: the checklist from the firmware findings, with this maker's menu paths.
pub(crate) fn guide_for(fw: &Firmware, vendor: Vendor, amd: bool, rebar_capable: bool) -> Guide {
    let path = |id: &str| {
        let row = PATHS.iter().find(|r| r.0 == id && r.1 == vendor).or_else(|| PATHS.iter().find(|r| r.0 == id && r.1 == Vendor::Generic));
        row.map_or("", |r| if amd { r.3 } else { r.2 })
    };
    let wants = [
        ("csm", fw.boot_mode == Some("legacy"), false),
        ("xmp", fw.memory_at_rated_speed == Some(false), false),
        ("rebar", fw.resizable_bar == Some(false) && rebar_capable, false),
        ("secure_boot", fw.secure_boot == Some(false), false),
        ("tpm", fw.tpm.is_none(), false),
        ("bios_update", fw.bios_age_days.is_some_and(|d| d > 3 * 365), false),
        ("virtualization", !fw.virtualization_enabled, true),
    ];
    let (_, enter_key, advanced_key) = KEYS.iter().find(|k| k.0 == vendor).copied().unwrap_or((Vendor::Generic, "Del / F2", None));
    Guide {
        vendor,
        enter_key,
        advanced_key,
        uefi: fw.boot_mode == Some("uefi"),
        items: wants.iter().filter(|w| w.1).map(|&(id, _, optional)| GuideItem { id, path: path(id), optional }).collect(),
    }
}

pub fn guide(p: &Profile, fw: &Firmware) -> Guide {
    let amd = p.cpu_detail.vendor.to_ascii_uppercase().contains("AMD");
    guide_for(fw, vendor_of(p.board.as_deref(), p.laptop), amd, rebar_capable(p))
}

/* ---------------- restart into the firmware setup ---------------- */

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FwError {
    #[error("needs_admin")]
    NeedsAdmin,
    /// Legacy BIOS, or UEFI firmware without "boot to firmware UI".
    #[error("unsupported")]
    Unsupported,
    #[error("win32: {0}")]
    Win32(String),
}

/// EFI global variable namespace (`OsIndications`, `OsIndicationsSupported`).
const EFI_GLOBAL: &str = "{8BE4DF61-93CA-11D2-AA0D-00E098032B8C}";
const BOOT_TO_FW_UI: u64 = 0x1;
/// NON_VOLATILE | BOOTSERVICE_ACCESS | RUNTIME_ACCESS, for a variable not set yet.
const NV_BS_RT: u32 = 0x7;

/// Pure: the `OsIndications` value that opens the setup on the next boot, other bits kept;
/// `None` when the firmware does not offer it.
pub(crate) fn with_boot_to_fw_ui(supported: u64, current: u64) -> Option<u64> {
    (supported & BOOT_TO_FW_UI != 0).then_some(current | BOOT_TO_FW_UI)
}

fn win32_error(code: u32) -> FwError {
    match code {
        1 => FwError::Unsupported,       // ERROR_INVALID_FUNCTION: no UEFI runtime variables
        5 | 1314 => FwError::NeedsAdmin, // access denied / privilege not held
        c => FwError::Win32(format!("error {c}")),
    }
}

/// (value, attributes) of an EFI global UINT64 variable; `None` = not set.
fn efi_read(name: &str) -> Result<Option<(u64, u32)>, FwError> {
    use windows::Win32::System::WindowsProgramming::GetFirmwareEnvironmentVariableExW;
    let (n, g) = (registry::wide(name), registry::wide(EFI_GLOBAL));
    let mut buf = [0u8; 8];
    let mut attrs = 0u32;
    // SAFETY: 8-byte buffer and its size; NUL-terminated names.
    let got = unsafe { GetFirmwareEnvironmentVariableExW(PCWSTR(n.as_ptr()), PCWSTR(g.as_ptr()), Some(buf.as_mut_ptr().cast()), 8, Some(&raw mut attrs)) };
    if got == 0 {
        // SAFETY: the calling thread's last error, right after the call.
        return match unsafe { windows::Win32::Foundation::GetLastError() }.0 {
            203 => Ok(None), // ERROR_ENVVAR_NOT_FOUND
            c => Err(win32_error(c)),
        };
    }
    Ok(Some((u64::from_le_bytes(buf), attrs)))
}

/// Write (`Some`) or delete (`None`) `OsIndications`.
fn efi_write(value: Option<u64>, attrs: u32) -> Result<(), FwError> {
    use windows::Win32::System::WindowsProgramming::SetFirmwareEnvironmentVariableExW;
    let (n, g) = (registry::wide("OsIndications"), registry::wide(EFI_GLOBAL));
    let bytes = value.unwrap_or(0).to_le_bytes();
    let (ptr, size) = if value.is_some() { (Some(bytes.as_ptr().cast()), 8) } else { (None, 0) };
    // SAFETY: 8 bytes (or none, which deletes the variable); NUL-terminated names.
    unsafe { SetFirmwareEnvironmentVariableExW(PCWSTR(n.as_ptr()), PCWSTR(g.as_ptr()), ptr, size, attrs) }.map_err(|e| win32_error((e.code().0 as u32) & 0xFFFF))
}

fn privilege(name: &str) -> Result<(), FwError> {
    crate::boost::enable_privilege(name).map_err(|e| match e {
        crate::boost::BoostError::NeedsAdmin => FwError::NeedsAdmin,
        other => FwError::Win32(other.to_string()),
    })
}

/// Restart straight into the firmware setup, as `shutdown /r /fw` does (without running it):
/// sets the "boot to firmware UI" bit of `OsIndications` (other bits kept) and restarts. Apps
/// still get the chance to save. If the restart cannot start, the variable is put back.
pub fn reboot_to_firmware() -> Result<(), FwError> {
    use windows::Win32::System::Shutdown::{InitiateShutdownW, SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_OTHER, SHTDN_REASON_MINOR_OTHER, SHUTDOWN_RESTART};
    if boot_mode() != Some("uefi") {
        return Err(FwError::Unsupported);
    }
    if !crate::system::is_elevated() {
        return Err(FwError::NeedsAdmin);
    }
    privilege("SeSystemEnvironmentPrivilege")?;
    let supported = efi_read("OsIndicationsSupported")?.map_or(0, |v| v.0);
    let current = efi_read("OsIndications")?;
    let next = with_boot_to_fw_ui(supported, current.map_or(0, |c| c.0)).ok_or(FwError::Unsupported)?;
    let attrs = current.map_or(NV_BS_RT, |c| c.1);
    efi_write(Some(next), attrs)?;
    let restart = privilege("SeShutdownPrivilege").and_then(|()| {
        // SAFETY: local restart, no message, no grace period.
        let rc = unsafe { InitiateShutdownW(PCWSTR::null(), PCWSTR::null(), 0, SHUTDOWN_RESTART, SHTDN_REASON_MAJOR_OTHER | SHTDN_REASON_MINOR_OTHER | SHTDN_REASON_FLAG_PLANNED) };
        if rc == 0 { Ok(()) } else { Err(FwError::Win32(format!("error {rc}"))) }
    });
    if restart.is_err() {
        let _ = efi_write(current.map(|c| c.0), attrs); // the next normal restart must not land in the BIOS
    }
    restart
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_to_firmware_bit_keeps_the_others() {
        assert_eq!(with_boot_to_fw_ui(0, 0), None, "firmware without the option");
        assert_eq!(with_boot_to_fw_ui(0xFE, 0x4), None);
        assert_eq!(with_boot_to_fw_ui(0x1, 0), Some(0x1));
        assert_eq!(with_boot_to_fw_ui(0x1F, 0x40), Some(0x41), "other requests kept");
        assert_eq!(with_boot_to_fw_ui(0x1, 0x1), Some(0x1), "already set");
    }

    #[test]
    fn reboot_refused_without_admin() {
        // Never reaches the restart: elevated runs skip this check on purpose.
        if !crate::system::is_elevated() {
            assert!(matches!(reboot_to_firmware(), Err(FwError::NeedsAdmin | FwError::Unsupported)));
        }
    }

    #[test]
    fn vendor_from_board() {
        assert_eq!(vendor_of(Some("ASUSTeK COMPUTER INC. ROG STRIX B650E-F GAMING WIFI"), false), Vendor::Asus);
        assert_eq!(vendor_of(Some("Micro-Star International Co., Ltd. MAG B650 TOMAHAWK WIFI (MS-7D75)"), false), Vendor::Msi);
        assert_eq!(vendor_of(Some("Gigabyte Technology Co., Ltd. X870 AORUS ELITE"), false), Vendor::Gigabyte);
        assert_eq!(vendor_of(Some("ASRock B550M Pro4"), false), Vendor::Asrock);
        assert_eq!(vendor_of(Some("Dell Inc. 0XYZ12"), false), Vendor::Generic);
        assert_eq!(vendor_of(Some("ASUSTeK COMPUTER INC. FA507NV"), true), Vendor::Generic, "laptops: OEM firmware");
        assert_eq!(vendor_of(None, false), Vendor::Generic);
    }

    #[test]
    fn every_item_has_a_path_for_every_vendor() {
        let items = ["csm", "xmp", "rebar", "secure_boot", "tpm", "bios_update", "virtualization"];
        for v in [Vendor::Asus, Vendor::Msi, Vendor::Gigabyte, Vendor::Asrock, Vendor::Generic] {
            assert!(KEYS.iter().any(|k| k.0 == v));
            for id in items {
                let row = PATHS.iter().find(|r| r.0 == id && r.1 == v).unwrap_or_else(|| panic!("{id} {v:?}"));
                assert!(!row.2.is_empty() && !row.3.is_empty());
                assert!(!row.2.contains(" - ") && !row.2.contains(" — "));
            }
        }
    }

    #[test]
    fn guide_lists_only_what_applies() {
        let healthy = Firmware {
            boot_mode: Some("uefi"),
            secure_boot: Some(true),
            tpm: Some("2.0".into()),
            virtualization_enabled: true,
            resizable_bar: Some(true),
            memory_at_rated_speed: Some(true),
            bios_age_days: Some(200),
            ..Default::default()
        };
        let g = guide_for(&healthy, Vendor::Msi, false, true);
        assert!(g.items.is_empty() && g.uefi);
        assert_eq!((g.enter_key, g.advanced_key), ("Del", Some("F7")));

        let owner = Firmware { resizable_bar: Some(false), bios_age_days: Some(1250), ..healthy.clone() };
        let g = guide_for(&owner, Vendor::Gigabyte, true, true);
        assert_eq!(g.items.iter().map(|i| i.id).collect::<Vec<_>>(), vec!["rebar", "bios_update"]);
        assert!(g.items[0].path.starts_with("Settings › IO Ports"));
        assert!(guide_for(&owner, Vendor::Gigabyte, true, false).items.iter().all(|i| i.id != "rebar"), "GTX-era card: ReBAR is not a setting");

        let old = Firmware { boot_mode: Some("legacy"), secure_boot: Some(false), tpm: None, virtualization_enabled: false, memory_at_rated_speed: Some(false), ..Default::default() };
        let g = guide_for(&old, Vendor::Asus, true, false);
        assert!(!g.uefi);
        assert_eq!(g.items.iter().map(|i| i.id).collect::<Vec<_>>(), vec!["csm", "xmp", "secure_boot", "tpm", "virtualization"]);
        assert!(g.items.last().unwrap().optional);
        assert!(g.items.iter().find(|i| i.id == "xmp").unwrap().path.contains("EXPO"), "AMD path");
        assert!(guide_for(&old, Vendor::Asus, false, false).items[1].path.contains("XMP I"), "Intel path");
    }

    #[test]
    #[ignore]
    fn guide_live() {
        let p = crate::hardware::profile();
        let g = guide(&p, &read(&p));
        println!("{:?} board={:?} enter={} advanced={:?} uefi={}", g.vendor, p.board, g.enter_key, g.advanced_key, g.uefi);
        for i in &g.items {
            println!("  {} (optional={}): {}", i.id, i.optional, i.path);
        }
    }

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
