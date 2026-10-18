//! Power plans, like Settings > System > Power: list, switch, create "Maximum performance",
//! duplicate, rename and delete the user's own plans; read/write a short allowlist of settings
//! for plugged in (AC) and battery (DC). Native powrprof, never `powercfg`.
//! Hostile caller: plans must exist in the live list, settings must be in [`SETTINGS`] and
//! values in their range — nothing arbitrary reaches the power API.

use std::ptr::null_mut;

use serde::Serialize;
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Power::{
    GetSystemPowerStatus, PowerDeleteScheme, PowerDuplicateScheme, PowerReadACValueIndex, PowerReadDCValueIndex,
    PowerSetActiveScheme, PowerWriteACValueIndex, PowerWriteDCValueIndex, PowerWriteFriendlyName, SYSTEM_POWER_STATUS,
};
use windows::core::GUID;

use crate::boost;

/// Windows' own plans: never deleted by G2 Boost.
const BALANCED: GUID = GUID::from_u128(0x381b4222_f694_41f0_9685_ff5bb260df2e);
const HIGH_PERFORMANCE: GUID = GUID::from_u128(0x8c5e7fda_e8bf_4a96_9a85_a6e23a8c635c);
const POWER_SAVER: GUID = GUID::from_u128(0xa1841308_3541_4fab_bc81_f71556f20b4a);
const BUILTIN: [GUID; 3] = [BALANCED, HIGH_PERFORMANCE, POWER_SAVER];

const SUB_VIDEO: GUID = GUID::from_u128(0x7516b95f_f776_4464_8c53_06167f40cc99);
const SUB_SLEEP: GUID = GUID::from_u128(0x238c9fa8_0aad_41ed_83f4_97be242c8f20);
const SUB_DISK: GUID = GUID::from_u128(0x0012ee47_9041_4b5d_9b77_535fba8b1442);
const SUB_USB: GUID = GUID::from_u128(0x2a737441_1930_4402_8d77_b2bebba308a3);
const SUB_PCIE: GUID = GUID::from_u128(0x501a4d13_42af_4429_9fd1_a8218c268e20);
const SUB_PROCESSOR: GUID = GUID::from_u128(0x54533251_82be_4824_96c1_47b60b740d00);

#[derive(Debug, thiserror::Error)]
pub enum PowerError {
    #[error("needs_admin")]
    NeedsAdmin,
    #[error("unknown_plan")]
    UnknownPlan,
    #[error("unknown_setting")]
    UnknownSetting,
    #[error("invalid_value")]
    InvalidValue,
    #[error("invalid_name")]
    InvalidName,
    /// Windows' own plans and the active plan cannot be deleted.
    #[error("protected_plan")]
    Protected,
    #[error("win32: {0}")]
    Win32(String),
}

fn check(e: u32) -> Result<(), PowerError> {
    match WIN32_ERROR(e) {
        ERROR_SUCCESS => Ok(()),
        ERROR_ACCESS_DENIED => Err(PowerError::NeedsAdmin),
        WIN32_ERROR(code) => Err(PowerError::Win32(format!("error {code}"))),
    }
}

impl From<boost::BoostError> for PowerError {
    fn from(e: boost::BoostError) -> Self {
        match e {
            boost::BoostError::NeedsAdmin => PowerError::NeedsAdmin,
            other => PowerError::Win32(other.to_string()),
        }
    }
}

fn guid_str(g: &GUID) -> String {
    format!("{g:?}").to_ascii_lowercase()
}

/* ---------------- plans ---------------- */

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub guid: String,
    pub name: String,
    pub active: bool,
    /// Balanced / High performance / Power saver: Windows' own, never deleted here.
    pub builtin: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plans {
    pub plans: Vec<Plan>,
    /// A battery is present (laptop): the UI shows the "on battery" column too.
    pub has_battery: bool,
}

pub fn plans() -> Plans {
    let active = boost::active_scheme().ok();
    let plans = boost::enumerate_schemes()
        .into_iter()
        .map(|g| Plan { guid: guid_str(&g), name: boost::friendly_name(&g), active: Some(g) == active, builtin: BUILTIN.contains(&g) })
        .collect();
    Plans { plans, has_battery: has_battery() }
}

fn has_battery() -> bool {
    let mut s = SYSTEM_POWER_STATUS::default();
    // SAFETY: plain out-struct. BatteryFlag 128 = no system battery, 255 = unknown.
    unsafe { GetSystemPowerStatus(&mut s) }.is_ok() && s.BatteryFlag != 128 && s.BatteryFlag != 255
}

/// The plan `guid` names, if it is in the live list.
fn listed(guid: &str) -> Result<GUID, PowerError> {
    let t = guid.trim().trim_start_matches('{').trim_end_matches('}');
    let g = GUID::try_from(t).map_err(|_| PowerError::UnknownPlan)?;
    if boost::enumerate_schemes().contains(&g) { Ok(g) } else { Err(PowerError::UnknownPlan) }
}

pub fn set_active(guid: &str) -> Result<(), PowerError> {
    let g = listed(guid)?;
    // SAFETY: valid GUID pointer.
    check(unsafe { PowerSetActiveScheme(None, Some(&g)) }.0)
}

/// The visible copy of the hidden "Ultimate Performance" template (created once).
pub fn create_max_performance() -> Result<String, PowerError> {
    Ok(guid_str(&boost::ensure_ultimate_scheme()?))
}

/// 1–64 visible characters; trimmed.
pub(crate) fn valid_name(name: &str) -> Option<&str> {
    let n = name.trim();
    (!n.is_empty() && n.chars().count() <= 64 && !n.chars().any(char::is_control)).then_some(n)
}

fn write_name(g: &GUID, name: &str) -> Result<(), PowerError> {
    let bytes: Vec<u8> = name.encode_utf16().chain(std::iter::once(0)).flat_map(u16::to_le_bytes).collect();
    // SAFETY: NUL-terminated UTF-16 buffer, length in bytes.
    check(unsafe { PowerWriteFriendlyName(None, g, None, None, &bytes) }.0)
}

/// Copy of `guid` with every setting, named `name`.
pub fn duplicate(guid: &str, name: &str) -> Result<String, PowerError> {
    let name = valid_name(name).ok_or(PowerError::InvalidName)?;
    let src = listed(guid)?;
    let mut p: *mut GUID = null_mut();
    // SAFETY: the API LocalAllocs the new GUID; take_local_guid frees it.
    check(unsafe { PowerDuplicateScheme(None, &src, &mut p) }.0)?;
    let g = boost::take_local_guid(p).ok_or_else(|| PowerError::Win32("duplicate".into()))?;
    write_name(&g, name)?;
    Ok(guid_str(&g))
}

pub fn rename(guid: &str, name: &str) -> Result<(), PowerError> {
    let name = valid_name(name).ok_or(PowerError::InvalidName)?;
    write_name(&listed(guid)?, name)
}

/// Delete a user plan. Windows' own plans and the active one are refused.
pub fn delete(guid: &str) -> Result<(), PowerError> {
    let g = listed(guid)?;
    if BUILTIN.contains(&g) || boost::active_scheme().ok() == Some(g) {
        return Err(PowerError::Protected);
    }
    // SAFETY: valid GUID pointer.
    check(unsafe { PowerDeleteScheme(None, &g) }.0)
}

/* ---------------- settings ---------------- */

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Kind {
    /// Timeout in seconds; 0 = never.
    Seconds,
    /// 0–100 %.
    Percent,
    /// One of the listed indexes (meaning lives in the UI's i18n).
    Choice { values: &'static [u32] },
}

impl Kind {
    pub(crate) fn valid(self, v: u32) -> bool {
        match self {
            Kind::Seconds => v <= 86_400,
            Kind::Percent => v <= 100,
            Kind::Choice { values } => values.contains(&v),
        }
    }
}

pub struct SettingDef {
    pub id: &'static str,
    subgroup: GUID,
    setting: GUID,
    pub kind: Kind,
}

const fn def(id: &'static str, subgroup: GUID, setting: u128, kind: Kind) -> SettingDef {
    SettingDef { id, subgroup, setting: GUID::from_u128(setting), kind }
}

/// The only settings G2 Boost reads or writes. Wake timers come from the competitor analysis
/// (Docs/references/binary-analysis.md §2.12); the rest mirror Settings / Control Panel.
pub const SETTINGS: &[SettingDef] = &[
    def("display_off", SUB_VIDEO, 0x3c0bc021_c8a8_4e07_a973_6b14cbcb2b7e, Kind::Seconds),
    def("sleep_after", SUB_SLEEP, 0x29f6c1db_86da_48c5_9fdb_f2b67b1f44da, Kind::Seconds),
    def("hibernate_after", SUB_SLEEP, 0x9d7815a6_7ee4_497e_8888_515a05f02364, Kind::Seconds),
    // 0 = off, 1 = on, 2 = important wake timers only.
    def("wake_timers", SUB_SLEEP, 0xbd3b718a_0680_4d9d_8ab2_e1d2b4ac806d, Kind::Choice { values: &[0, 1, 2] }),
    def("disk_off", SUB_DISK, 0x6738e2c4_e8a5_4a42_b16a_e040e769756e, Kind::Seconds),
    // 0 = off, 1 = on.
    def("usb_suspend", SUB_USB, 0x48e6b7a6_50f5_4782_a5d4_53bb8f07e226, Kind::Choice { values: &[0, 1] }),
    // 0 = off, 1 = moderate, 2 = maximum power savings.
    def("pcie_aspm", SUB_PCIE, 0xee12f906_d277_404b_b6da_e5fa1a576df5, Kind::Choice { values: &[0, 1, 2] }),
    def("cpu_min", SUB_PROCESSOR, 0x893dee8e_2bef_41e0_89c6_b55d0929964c, Kind::Percent),
    def("cpu_max", SUB_PROCESSOR, 0xbc5038f7_23e0_4960_96da_33abaf5935ec, Kind::Percent),
    // 0 = disabled, 1 = enabled, 2 = aggressive.
    def("cpu_boost", SUB_PROCESSOR, 0xbe337238_0d82_4146_a960_4f3749d470c7, Kind::Choice { values: &[0, 1, 2] }),
    // Minimum unparked cores (%): 100 = core parking off.
    def("core_parking_min", SUB_PROCESSOR, 0x0cc5b647_c1df_4637_891a_dec35c318583, Kind::Percent),
];

#[derive(Debug, Clone, Serialize)]
pub struct SettingValue {
    pub id: &'static str,
    pub kind: Kind,
    /// Plugged in.
    pub ac: Option<u32>,
    /// On battery.
    pub dc: Option<u32>,
}

fn read(g: &GUID, d: &SettingDef) -> SettingValue {
    let (mut ac, mut dc) = (0u32, 0u32);
    // SAFETY: GUID pointers outlive the calls; plain out-params. A setting the system does not
    // expose (no battery, no PCIe subgroup…) fails and is reported as None.
    let ac_ok = unsafe { PowerReadACValueIndex(None, Some(g), Some(&d.subgroup), Some(&d.setting), &mut ac) } == ERROR_SUCCESS;
    let dc_ok = unsafe { PowerReadDCValueIndex(None, Some(g), Some(&d.subgroup), Some(&d.setting), &mut dc) } == 0;
    SettingValue { id: d.id, kind: d.kind, ac: ac_ok.then_some(ac), dc: dc_ok.then_some(dc) }
}

/// Every allowlisted setting this system exposes for `guid`.
pub fn settings(guid: &str) -> Result<Vec<SettingValue>, PowerError> {
    let g = listed(guid)?;
    Ok(SETTINGS.iter().map(|d| read(&g, d)).filter(|v| v.ac.is_some() || v.dc.is_some()).collect())
}

/// Write AC and/or DC for one allowlisted setting; re-applies the plan when it is the active one.
pub fn write(guid: &str, id: &str, ac: Option<u32>, dc: Option<u32>) -> Result<SettingValue, PowerError> {
    let d = SETTINGS.iter().find(|d| d.id == id).ok_or(PowerError::UnknownSetting)?;
    if ac.is_none() && dc.is_none() || ac.is_some_and(|v| !d.kind.valid(v)) || dc.is_some_and(|v| !d.kind.valid(v)) {
        return Err(PowerError::InvalidValue);
    }
    let g = listed(guid)?;
    // SAFETY: GUID pointers outlive the calls.
    unsafe {
        if let Some(v) = ac {
            check(PowerWriteACValueIndex(None, &g, Some(&d.subgroup), Some(&d.setting), v).0)?;
        }
        if let Some(v) = dc {
            check(PowerWriteDCValueIndex(None, &g, Some(&d.subgroup), Some(&d.setting), v))?;
        }
        // Writes land in the stored plan; the active plan only picks them up when re-applied.
        if boost::active_scheme().ok() == Some(g) {
            check(PowerSetActiveScheme(None, Some(&g)).0)?;
        }
    }
    Ok(read(&g, d))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_is_strict() {
        assert!(Kind::Seconds.valid(0) && Kind::Seconds.valid(3600) && !Kind::Seconds.valid(90_000));
        assert!(Kind::Percent.valid(100) && !Kind::Percent.valid(101));
        let k = Kind::Choice { values: &[0, 1, 2] };
        assert!(k.valid(2) && !k.valid(3));
        assert_eq!(valid_name("  Jogos  "), Some("Jogos"));
        assert_eq!(valid_name(""), None);
        assert_eq!(valid_name("a\u{0}b"), None);
        assert_eq!(valid_name(&"x".repeat(65)), None);
        let mut ids: Vec<&str> = SETTINGS.iter().map(|d| d.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), SETTINGS.len(), "unique ids");
    }

    /// Read-only against the live system (no plan is changed).
    #[test]
    fn live_plans_and_refusals() {
        let p = plans();
        assert!(!p.plans.is_empty());
        assert_eq!(p.plans.iter().filter(|x| x.active).count(), 1, "exactly one active plan");
        let active = p.plans.iter().find(|x| x.active).unwrap().guid.clone();
        let s = settings(&active).unwrap();
        assert!(s.iter().any(|v| v.id == "display_off" && v.ac.is_some()));
        assert!(matches!(set_active("{00000000-0000-0000-0000-000000000000}"), Err(PowerError::UnknownPlan)));
        assert!(matches!(set_active("not a guid"), Err(PowerError::UnknownPlan)));
        assert!(matches!(delete(&guid_str(&BALANCED)), Err(PowerError::Protected | PowerError::UnknownPlan)));
        assert!(matches!(delete(&active), Err(PowerError::Protected)), "the active plan is never deleted");
        assert!(matches!(write(&active, "nope", Some(1), None), Err(PowerError::UnknownSetting)));
        assert!(matches!(write(&active, "cpu_max", Some(150), None), Err(PowerError::InvalidValue)));
        assert!(matches!(write(&active, "cpu_max", None, None), Err(PowerError::InvalidValue)));
        assert!(matches!(duplicate(&active, " "), Err(PowerError::InvalidName)));
    }

    /// Duplicate → write → read back → delete (changes plans for a moment; run by hand:
    /// `cargo test -p g2boost-core -- --ignored power_round_trip`).
    #[test]
    #[ignore]
    fn power_round_trip() {
        let active = plans().plans.into_iter().find(|x| x.active).unwrap().guid;
        let copy = duplicate(&active, "G2 Boost test").unwrap();
        let v = write(&copy, "display_off", Some(600), None).unwrap();
        assert_eq!(v.ac, Some(600));
        rename(&copy, "G2 Boost test 2").unwrap();
        assert!(plans().plans.iter().any(|p| p.guid == copy && p.name == "G2 Boost test 2"));
        delete(&copy).unwrap();
        assert!(!plans().plans.iter().any(|p| p.guid == copy));
    }
}
