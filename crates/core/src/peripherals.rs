//! Mouse and keyboard settings: pointer speed, wheel lines, double-click time, key repeat delay
//! and rate. Native `SystemParametersInfo` only (`SPIF_UPDATEINIFILE | SPIF_SENDCHANGE`, like the
//! Control Panel). The first change to a setting saves the value it had (`backups/peripherals.json`);
//! "back to the original" writes it back. Callers are hostile: values are clamped to the ranges the
//! Control Panel itself offers, and the backup file is re-validated on load.
//!
//! Mouse acceleration and the "fast keyboard" preset stay in `boost` (tweaks `mouse`, `keyboard`).

use std::ffi::c_void;

use serde::{Deserialize, Serialize};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETKEYBOARDDELAY, SPI_GETKEYBOARDSPEED, SPI_GETMOUSESPEED, SPI_GETWHEELSCROLLLINES, SPI_SETDOUBLECLICKTIME,
    SPI_SETKEYBOARDDELAY, SPI_SETKEYBOARDSPEED, SPI_SETMOUSESPEED, SPI_SETWHEELSCROLLLINES, SPIF_SENDCHANGE,
    SPIF_UPDATEINIFILE, SYSTEM_PARAMETERS_INFO_ACTION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};

use crate::registry::{self, Backup, BackupEntry, Root, Value};

// `GetDoubleClickTime` has no SPI getter. user32 is always loaded; declared here instead of enabling
// one more `windows` feature for a single call (same approach as `NtSetSystemInformation` in `boost`).
#[link(name = "user32")]
unsafe extern "system" {
    fn GetDoubleClickTime() -> u32;
}

#[derive(Debug, thiserror::Error)]
pub enum PeriphError {
    #[error("win32: {0}")]
    Win32(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// `WHEEL_PAGESCROLL`: the wheel scrolls one screen at a time (a valid value the user may already have).
pub const WHEEL_PAGE: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Setting {
    /// 1..=20, 10 = the Windows default (1:1 with acceleration off).
    MouseSpeed,
    /// Lines per wheel notch.
    WheelLines,
    /// Milliseconds between two clicks of a double-click.
    DoubleClick,
    /// Delay before a held key repeats: 0 (shortest) ..= 3 (longest).
    KbDelay,
    /// Repeat rate: 0 (slowest) ..= 31 (fastest).
    KbRate,
}

impl Setting {
    pub const ALL: [Setting; 5] =
        [Setting::MouseSpeed, Setting::WheelLines, Setting::DoubleClick, Setting::KbDelay, Setting::KbRate];

    pub const fn id(self) -> &'static str {
        match self {
            Setting::MouseSpeed => "mouse_speed",
            Setting::WheelLines => "wheel_lines",
            Setting::DoubleClick => "double_click",
            Setting::KbDelay => "kb_delay",
            Setting::KbRate => "kb_rate",
        }
    }

    fn from_id(id: &str) -> Option<Setting> {
        Setting::ALL.into_iter().find(|s| s.id() == id)
    }

    /// Inclusive range the Windows Control Panel offers.
    pub const fn range(self) -> (u32, u32) {
        match self {
            Setting::MouseSpeed => (1, 20),
            Setting::WheelLines => (1, 100),
            Setting::DoubleClick => (200, 900),
            Setting::KbDelay => (0, 3),
            Setting::KbRate => (0, 31),
        }
    }
}

/// Hostile value in, valid value out.
pub fn clamp(s: Setting, v: i64) -> u32 {
    let (lo, hi) = s.range();
    v.clamp(i64::from(lo), i64::from(hi)) as u32
}

/// What goes back from the backup file: in range, except the page-scroll wheel value Windows allows.
fn restorable(s: Setting, v: u32) -> u32 {
    if s == Setting::WheelLines && v == WHEEL_PAGE { v } else { clamp(s, i64::from(v)) }
}

/* ---------------- SPI ---------------- */

const FLAGS: SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS =
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(SPIF_UPDATEINIFILE.0 | SPIF_SENDCHANGE.0);

fn win(e: windows::core::Error) -> PeriphError {
    PeriphError::Win32(format!("{:?}", e.code()))
}

fn spi_get(action: SYSTEM_PARAMETERS_INFO_ACTION) -> Result<u32, PeriphError> {
    let mut v = 0u32;
    // SAFETY: these GET actions write one UINT through pvParam.
    unsafe { SystemParametersInfoW(action, 0, Some((&mut v as *mut u32).cast::<c_void>()), Default::default()) }
        .map_err(win)?;
    Ok(v)
}

fn spi_set(action: SYSTEM_PARAMETERS_INFO_ACTION, ui_param: u32, pv_param: Option<*mut c_void>) -> Result<(), PeriphError> {
    // SAFETY: the value rides in uiParam, or in pvParam as a plain integer (SPI_SETMOUSESPEED); nothing is dereferenced.
    unsafe { SystemParametersInfoW(action, ui_param, pv_param, FLAGS) }.map_err(win)
}

fn get(s: Setting) -> Result<u32, PeriphError> {
    match s {
        Setting::MouseSpeed => spi_get(SPI_GETMOUSESPEED),
        Setting::WheelLines => spi_get(SPI_GETWHEELSCROLLLINES),
        // SAFETY: no arguments, no side effects.
        Setting::DoubleClick => Ok(unsafe { GetDoubleClickTime() }),
        Setting::KbDelay => spi_get(SPI_GETKEYBOARDDELAY),
        Setting::KbRate => spi_get(SPI_GETKEYBOARDSPEED),
    }
}

fn put(s: Setting, v: u32) -> Result<(), PeriphError> {
    match s {
        Setting::MouseSpeed => spi_set(SPI_SETMOUSESPEED, 0, Some(v as usize as *mut c_void)),
        Setting::WheelLines => spi_set(SPI_SETWHEELSCROLLLINES, v, None),
        Setting::DoubleClick => spi_set(SPI_SETDOUBLECLICKTIME, v, None),
        Setting::KbDelay => spi_set(SPI_SETKEYBOARDDELAY, v, None),
        Setting::KbRate => spi_set(SPI_SETKEYBOARDSPEED, v, None),
    }
}

/* ---------------- backup (first value only) ---------------- */

const BACKUP_ID: &str = "peripherals";
const BACKUP_PATH: &str = "spi:peripherals";

/// The file is user-writable: unknown names, non-DWORD values and repeats are dropped, values re-clamped.
fn parse_originals(entries: &[BackupEntry]) -> Vec<(Setting, u32)> {
    let mut out: Vec<(Setting, u32)> = Vec::new();
    for e in entries {
        let (Some(s), Value::Dword(v)) = (Setting::from_id(&e.name), &e.value) else { continue };
        if e.path == BACKUP_PATH && !out.iter().any(|(x, _)| *x == s) {
            out.push((s, restorable(s, *v)));
        }
    }
    out
}

fn load_originals() -> Vec<(Setting, u32)> {
    registry::load_backup(BACKUP_ID).map(|b| parse_originals(&b.entries)).unwrap_or_default()
}

fn save_originals(list: &[(Setting, u32)]) -> Result<(), PeriphError> {
    if list.is_empty() {
        registry::delete_backup(BACKUP_ID);
        return Ok(());
    }
    let entries = list
        .iter()
        .map(|(s, v)| BackupEntry { root: Root::Hkcu, path: BACKUP_PATH.into(), name: s.id().into(), value: Value::Dword(*v) })
        .collect();
    registry::save_backup(&Backup { tweak: BACKUP_ID.into(), taken_at: registry::now_unix(), entries })?;
    Ok(())
}

/* ---------------- public API ---------------- */

#[derive(Debug, Clone, Serialize)]
pub struct Original {
    pub setting: Setting,
    pub value: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct State {
    pub mouse_speed: u32,
    /// `WHEEL_PAGE` (4294967295) = one screen per notch.
    pub wheel_lines: u32,
    pub double_click: u32,
    pub kb_delay: u32,
    pub kb_rate: u32,
    /// Saved originals that differ from what is set now (the ones "back to the original" would change).
    pub originals: Vec<Original>,
}

pub fn state() -> Result<State, PeriphError> {
    let st = State {
        mouse_speed: get(Setting::MouseSpeed)?,
        wheel_lines: get(Setting::WheelLines)?,
        double_click: get(Setting::DoubleClick)?,
        kb_delay: get(Setting::KbDelay)?,
        kb_rate: get(Setting::KbRate)?,
        originals: Vec::new(),
    };
    let current = |s: Setting| match s {
        Setting::MouseSpeed => st.mouse_speed,
        Setting::WheelLines => st.wheel_lines,
        Setting::DoubleClick => st.double_click,
        Setting::KbDelay => st.kb_delay,
        Setting::KbRate => st.kb_rate,
    };
    let originals = load_originals()
        .into_iter()
        .filter(|(s, v)| current(*s) != *v)
        .map(|(setting, value)| Original { setting, value })
        .collect();
    Ok(State { originals, ..st })
}

/// How many settings still have a saved original (for the restoration list).
pub fn originals_count() -> usize {
    load_originals().len()
}

fn history(setting: Setting, on: bool, result: &Result<State, PeriphError>) {
    let error = result.as_ref().err().map(ToString::to_string);
    crate::profiles::log(vec![crate::profiles::HistoryEntry {
        at: registry::now_unix(),
        source: "@periph".into(),
        id: format!("periph_{}", setting.id()),
        on,
        ok: error.is_none(),
        error,
    }]);
}

/// Set one value (clamped). The value it had is saved before the first change.
pub fn set(s: Setting, value: i64) -> Result<State, PeriphError> {
    let result = set_inner(s, clamp(s, value));
    history(s, true, &result);
    result
}

fn set_inner(s: Setting, v: u32) -> Result<State, PeriphError> {
    let cur = get(s)?;
    let mut originals = load_originals();
    if v != cur && !originals.iter().any(|(x, _)| *x == s) {
        originals.push((s, cur));
        save_originals(&originals)?; // backup first
    }
    put(s, v)?;
    // Back at the saved original: nothing left to undo, and the next change saves a fresh original.
    let before = originals.len();
    originals.retain(|(x, o)| !(*x == s && *o == v));
    if originals.len() != before {
        save_originals(&originals)?;
    }
    state()
}

/// Put the saved original back: one setting, or all of them (`None`).
pub fn revert(which: Option<Setting>) -> Result<State, PeriphError> {
    let targets: Vec<Setting> =
        load_originals().into_iter().map(|(s, _)| s).filter(|s| which.is_none_or(|w| w == *s)).collect();
    let result = revert_inner(which);
    for s in targets {
        history(s, false, &result);
    }
    result
}

fn revert_inner(which: Option<Setting>) -> Result<State, PeriphError> {
    let mut failed: Option<PeriphError> = None;
    let mut originals = load_originals();
    originals.retain(|(s, v)| {
        if which.is_some_and(|w| w != *s) {
            return true;
        }
        match put(*s, restorable(*s, *v)) {
            Ok(()) => false,
            Err(e) => {
                failed.get_or_insert(e);
                true
            }
        }
    });
    save_originals(&originals)?;
    match failed {
        Some(e) => Err(e),
        None => state(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// The backup file is shared by every test below that writes it.
    static LOCK: Mutex<()> = Mutex::new(());

    fn dir() {
        // SAFETY: same override every data-dir test in this crate uses (process-wide env var).
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
    }

    #[test]
    fn clamp_keeps_hostile_values_inside_the_control_panel_ranges() {
        for s in Setting::ALL {
            let (lo, hi) = s.range();
            for v in [i64::MIN, -1, 0, 1, 7, 21, 99, 1000, 5000, i64::from(u32::MAX), i64::MAX] {
                let c = clamp(s, v);
                assert!((lo..=hi).contains(&c), "{s:?} {v} -> {c}");
            }
            assert_eq!(clamp(s, i64::from(lo)), lo);
            assert_eq!(clamp(s, i64::from(hi)), hi);
        }
        assert_eq!(clamp(Setting::MouseSpeed, 10), 10, "10 is 1:1");
        assert_eq!(clamp(Setting::KbDelay, -5), 0);
        assert_eq!(clamp(Setting::KbRate, 99), 31);
        assert_eq!(clamp(Setting::DoubleClick, 10), 200);
    }

    #[test]
    fn restore_accepts_page_scroll_only_for_the_wheel() {
        assert_eq!(restorable(Setting::WheelLines, WHEEL_PAGE), WHEEL_PAGE);
        assert_eq!(restorable(Setting::MouseSpeed, WHEEL_PAGE), 20);
        assert_eq!(restorable(Setting::WheelLines, 0), 1);
        assert_eq!(restorable(Setting::WheelLines, 40_000), 100);
        assert_eq!(restorable(Setting::KbDelay, 2), 2);
    }

    #[test]
    fn backup_file_is_validated_on_load() {
        let e = |path: &str, name: &str, value: Value| BackupEntry { root: Root::Hkcu, path: path.into(), name: name.into(), value };
        let got = parse_originals(&[
            e(BACKUP_PATH, "mouse_speed", Value::Dword(99)),        // clamped to 20
            e(BACKUP_PATH, "mouse_speed", Value::Dword(3)),         // repeat: first wins
            e(BACKUP_PATH, "wheel_lines", Value::Dword(WHEEL_PAGE)), // page scroll survives
            e(BACKUP_PATH, "kb_delay", Value::Str("2".into())),     // not a DWORD
            e(BACKUP_PATH, "../evil", Value::Dword(1)),             // unknown setting
            e("spi:other", "kb_rate", Value::Dword(5)),             // foreign path
            e(BACKUP_PATH, "double_click", Value::Absent),
        ]);
        assert_eq!(got, vec![(Setting::MouseSpeed, 20), (Setting::WheelLines, WHEEL_PAGE)]);
    }

    #[test]
    fn originals_roundtrip_through_the_backup_store() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        dir();
        registry::delete_backup(BACKUP_ID);
        assert!(load_originals().is_empty());
        save_originals(&[(Setting::KbRate, 12), (Setting::DoubleClick, 450)]).unwrap();
        assert_eq!(load_originals(), vec![(Setting::KbRate, 12), (Setting::DoubleClick, 450)]);
        assert_eq!(originals_count(), 2);
        save_originals(&[]).unwrap();
        assert!(registry::load_backup(BACKUP_ID).is_none(), "an empty list removes the file");
    }

    #[test]
    fn state_reads_without_admin() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        dir();
        registry::delete_backup(BACKUP_ID);
        let st = state().unwrap();
        assert!((1..=20).contains(&st.mouse_speed), "{}", st.mouse_speed);
        assert!(st.wheel_lines >= 1);
        assert!(st.double_click >= 1);
        assert!(st.kb_delay <= 3 && st.kb_rate <= 31);
        assert!(st.originals.is_empty());
    }

    /// Changes the real pointer speed, wheel lines, double-click time and key repeat of this user,
    /// then puts every one back. Run by hand: `cargo test -p g2boost-core -- --ignored peripherals_live`.
    #[test]
    #[ignore]
    fn peripherals_live_set_and_revert() {
        /// Puts every value back even when an assertion fails halfway.
        struct Back(Vec<(Setting, u32)>);
        impl Drop for Back {
            fn drop(&mut self) {
                for (s, v) in &self.0 {
                    let _ = put(*s, *v);
                }
            }
        }
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        dir();
        registry::delete_backup(BACKUP_ID);
        let start = state().unwrap();
        let _back = Back(Setting::ALL.iter().map(|s| (*s, get(*s).unwrap())).collect());
        for s in Setting::ALL {
            let (lo, hi) = s.range();
            let cur = get(s).unwrap();
            let want = if cur == hi { lo } else { hi };
            set(s, i64::from(want)).unwrap();
            assert_eq!(get(s).unwrap(), want, "{s:?} set");
        }
        assert_eq!(state().unwrap().originals.len(), 5);
        let back = revert(None).unwrap();
        assert!(back.originals.is_empty());
        assert_eq!(
            (back.mouse_speed, back.wheel_lines, back.double_click, back.kb_delay, back.kb_rate),
            (start.mouse_speed, start.wheel_lines, start.double_click, start.kb_delay, start.kb_rate)
        );
        assert!(registry::load_backup(BACKUP_ID).is_none());
    }
}
