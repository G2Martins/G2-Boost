//! The 17 checkboxes of "Performance Options → Visual Effects", driven the same way the
//! dialog is: `HKLM\...\Explorer\VisualEffects\<item>` says whether an item is a
//! `SystemParametersInfo` action (SPIActionGet/Set) or an HKCU value (RegPath). That
//! metadata was read on the dev machine on 2026-10-14 and is frozen here as data.
//! Everything is per-user: no admin, no logoff, immediate effect (`SPIF_SENDCHANGE`).

use std::ffi::c_void;
use std::mem::size_of;

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::core::BOOL;
use windows::Win32::UI::WindowsAndMessaging::{
    ANIMATIONINFO, HWND_BROADCAST, SMTO_ABORTIFHUNG, SPI_GETANIMATION, SPI_SETANIMATION, SPIF_SENDCHANGE,
    SPIF_UPDATEINIFILE, SYSTEM_PARAMETERS_INFO_ACTION, SendMessageTimeoutW, SystemParametersInfoW, WM_SETTINGCHANGE,
};

use crate::registry::{self, Root, Value};

const EXPLORER_ADV: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const DWM: &str = r"Software\Microsoft\Windows\DWM";
/// `VisualFXSetting`: 0 = let Windows choose, 1 = best appearance, 2 = best performance, 3 = custom.
pub const VISUAL_FX_SETTING_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects";
pub const VISUAL_FX_SETTING_NAME: &str = "VisualFXSetting";

#[derive(Debug, Clone, Copy)]
enum Kind {
    /// GET writes a BOOL through pvParam; SET passes the BOOL *as* pvParam (0x1000-range actions).
    SpiBool { get: u32, set: u32 },
    /// GET writes a BOOL through pvParam; SET passes the BOOL in uiParam (DragFullWindows, FontSmoothing).
    SpiUiParam { get: u32, set: u32 },
    /// SPI_GET/SETANIMATION with ANIMATIONINFO (minimize/maximize animation).
    SpiAnimation,
    /// HKCU DWORD. `absent_is_on`: value missing means the effect is on. `inverted`: 1 means off.
    Reg { path: &'static str, name: &'static str, absent_is_on: bool, inverted: bool },
}

#[derive(Debug, Clone, Copy)]
pub struct Item {
    pub id: &'static str,
    kind: Kind,
}

/// Order matches the dialog (pt-BR labels in the i18n dictionary under `boost.fx.<id>`).
pub const ITEMS: [Item; 17] = [
    Item { id: "combobox_animation", kind: Kind::SpiBool { get: 0x1004, set: 0x1005 } },
    Item { id: "taskbar_animations", kind: Kind::Reg { path: EXPLORER_ADV, name: "TaskbarAnimations", absent_is_on: true, inverted: false } },
    Item { id: "control_animations", kind: Kind::SpiBool { get: 0x1042, set: 0x1043 } },
    Item { id: "animate_minmax", kind: Kind::SpiAnimation },
    Item { id: "selection_fade", kind: Kind::SpiBool { get: 0x1014, set: 0x1015 } },
    Item { id: "tooltip_animation", kind: Kind::SpiBool { get: 0x1016, set: 0x1017 } },
    Item { id: "menu_animation", kind: Kind::SpiBool { get: 0x1002, set: 0x1003 } },
    Item { id: "aero_peek", kind: Kind::Reg { path: DWM, name: "EnableAeroPeek", absent_is_on: true, inverted: false } },
    Item { id: "drag_full_windows", kind: Kind::SpiUiParam { get: 38, set: 37 } },
    Item { id: "thumbnails", kind: Kind::Reg { path: EXPLORER_ADV, name: "IconsOnly", absent_is_on: true, inverted: true } },
    Item { id: "listview_alpha_select", kind: Kind::Reg { path: EXPLORER_ADV, name: "ListviewAlphaSelect", absent_is_on: true, inverted: false } },
    Item { id: "drop_shadow", kind: Kind::SpiBool { get: 0x1024, set: 0x1025 } },
    Item { id: "cursor_shadow", kind: Kind::SpiBool { get: 0x101A, set: 0x101B } },
    Item { id: "listbox_smooth_scrolling", kind: Kind::SpiBool { get: 0x1006, set: 0x1007 } },
    Item { id: "save_thumbnails", kind: Kind::Reg { path: DWM, name: "AlwaysHibernateThumbnails", absent_is_on: false, inverted: false } },
    Item { id: "font_smoothing", kind: Kind::SpiUiParam { get: 74, set: 75 } },
    Item { id: "listview_shadow", kind: Kind::Reg { path: EXPLORER_ADV, name: "ListviewShadow", absent_is_on: true, inverted: false } },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Everything on (the dialog's "best appearance").
    Appearance,
    /// Only the heavy ones off: taskbar animations, window contents while dragging,
    /// saved taskbar thumbnails, translucent selection rectangle.
    Minimal,
    /// Minimal + shadows under windows (the owner's own setup, 2026-10-14).
    Balanced,
    /// Everything off except smooth fonts (text is unreadable without them).
    Performance,
}

const MINIMAL_OFF: &[&str] = &["taskbar_animations", "drag_full_windows", "save_thumbnails", "listview_alpha_select"];
const BALANCED_OFF: &[&str] = &[
    "taskbar_animations",
    "drag_full_windows",
    "save_thumbnails",
    "listview_alpha_select",
    "drop_shadow",
];

/// Desired on/off for one item under a level.
pub fn desired(level: Level, id: &str) -> bool {
    match level {
        Level::Appearance => true,
        Level::Minimal => !MINIMAL_OFF.contains(&id),
        Level::Balanced => !BALANCED_OFF.contains(&id),
        Level::Performance => id == "font_smoothing",
    }
}

/// `VisualFXSetting` value the dialog would show after applying `level`.
pub fn setting_for(level: Level) -> u32 {
    match level {
        Level::Appearance => 1,
        _ => 3,
    }
}

/// Which level, if any, the current item states correspond to exactly.
pub fn level_of(states: &[(&str, bool)]) -> Option<Level> {
    [Level::Appearance, Level::Minimal, Level::Balanced, Level::Performance]
        .into_iter()
        .find(|lv| states.iter().all(|(id, on)| desired(*lv, id) == *on))
}

#[derive(Debug, thiserror::Error)]
pub enum FxError {
    #[error("unknown visual effect item")]
    Unknown,
    #[error("{0}")]
    Registry(#[from] registry::RegError),
    #[error("SystemParametersInfo failed: {0}")]
    Spi(String),
}

fn item(id: &str) -> Result<Item, FxError> {
    ITEMS.iter().copied().find(|i| i.id == id).ok_or(FxError::Unknown)
}

fn spi_get_bool(action: u32) -> Result<bool, FxError> {
    let mut v = BOOL(0);
    // SAFETY: pvParam points at a BOOL, which is what these GET actions write.
    unsafe {
        SystemParametersInfoW(
            SYSTEM_PARAMETERS_INFO_ACTION(action),
            0,
            Some((&mut v as *mut BOOL).cast::<c_void>()),
            Default::default(),
        )
    }
    .map_err(|e| FxError::Spi(e.to_string()))?;
    Ok(v.as_bool())
}

const FLAGS: windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS =
    windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(
        SPIF_UPDATEINIFILE.0 | SPIF_SENDCHANGE.0,
    );

pub fn get_item(id: &str) -> Result<bool, FxError> {
    match item(id)?.kind {
        Kind::SpiBool { get, .. } | Kind::SpiUiParam { get, .. } => spi_get_bool(get),
        Kind::SpiAnimation => {
            let mut ai = ANIMATIONINFO { cbSize: size_of::<ANIMATIONINFO>() as u32, iMinAnimate: 0 };
            // SAFETY: sized ANIMATIONINFO; the call fills iMinAnimate.
            unsafe {
                SystemParametersInfoW(
                    SPI_GETANIMATION,
                    ai.cbSize,
                    Some((&mut ai as *mut ANIMATIONINFO).cast::<c_void>()),
                    Default::default(),
                )
            }
            .map_err(|e| FxError::Spi(e.to_string()))?;
            Ok(ai.iMinAnimate != 0)
        }
        Kind::Reg { path, name, absent_is_on, inverted } => {
            let raw = match registry::read(Root::Hkcu, path, name)? {
                Value::Dword(d) => d != 0,
                Value::Absent => return Ok(absent_is_on),
                Value::Str(s) => s != "0",
                Value::Binary(b) => b.first().is_some_and(|x| *x != 0),
            };
            Ok(raw != inverted)
        }
    }
}

pub fn set_item(id: &str, on: bool) -> Result<(), FxError> {
    match item(id)?.kind {
        Kind::SpiBool { set, .. } => {
            // SAFETY: these SET actions take the BOOL in pvParam itself (documented), uiParam 0.
            unsafe { SystemParametersInfoW(SYSTEM_PARAMETERS_INFO_ACTION(set), 0, Some((on as usize) as *mut c_void), FLAGS) }
                .map_err(|e| FxError::Spi(e.to_string()))
        }
        Kind::SpiUiParam { set, .. } => {
            // SAFETY: BOOL travels in uiParam; pvParam unused.
            unsafe { SystemParametersInfoW(SYSTEM_PARAMETERS_INFO_ACTION(set), on as u32, None, FLAGS) }
                .map_err(|e| FxError::Spi(e.to_string()))
        }
        Kind::SpiAnimation => {
            let mut ai = ANIMATIONINFO { cbSize: size_of::<ANIMATIONINFO>() as u32, iMinAnimate: on as i32 };
            // SAFETY: sized ANIMATIONINFO read by the call.
            unsafe {
                SystemParametersInfoW(SPI_SETANIMATION, ai.cbSize, Some((&mut ai as *mut ANIMATIONINFO).cast::<c_void>()), FLAGS)
            }
            .map_err(|e| FxError::Spi(e.to_string()))
        }
        Kind::Reg { path, name, inverted, .. } => {
            let v = if on != inverted { 1 } else { 0 };
            registry::write(Root::Hkcu, path, name, &Value::Dword(v))?;
            broadcast_setting_change();
            Ok(())
        }
    }
}

/// Tell Explorer/DWM that per-user settings changed (what the dialog's OK does).
pub fn broadcast_setting_change() {
    // SAFETY: broadcast with a bounded timeout; no pointers passed.
    unsafe {
        let _ = SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, WPARAM(0), LPARAM(0), SMTO_ABORTIFHUNG, 1000, None);
    }
}

pub fn read_setting() -> u32 {
    match registry::read(Root::Hkcu, VISUAL_FX_SETTING_PATH, VISUAL_FX_SETTING_NAME) {
        Ok(Value::Dword(d)) => d,
        _ => 0,
    }
}

/// Keep the dialog consistent with what we applied.
pub fn write_setting(v: u32) -> Result<(), FxError> {
    registry::write(Root::Hkcu, VISUAL_FX_SETTING_PATH, VISUAL_FX_SETTING_NAME, &Value::Dword(v))?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct ItemState {
    pub id: &'static str,
    pub on: bool,
}

/// Current state of all items. Items that fail to read are reported as `on` (conservative).
pub fn detect() -> Vec<ItemState> {
    ITEMS.iter().map(|i| ItemState { id: i.id, on: get_item(i.id).unwrap_or(true) }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_items_readable_and_levels_consistent() {
        let states = detect();
        assert_eq!(states.len(), 17);
        for i in &ITEMS {
            get_item(i.id).unwrap_or_else(|e| panic!("{}: {e}", i.id));
        }
        // Performance keeps only font smoothing; Appearance keeps all.
        assert!(desired(Level::Performance, "font_smoothing"));
        assert!(!desired(Level::Performance, "drop_shadow"));
        assert!(ITEMS.iter().all(|i| desired(Level::Appearance, i.id)));
        // Balanced is a strict subset of Minimal.
        assert!(ITEMS.iter().all(|i| desired(Level::Balanced, i.id) || !desired(Level::Minimal, i.id) || BALANCED_OFF.contains(&i.id)));
        let all_on: Vec<(&str, bool)> = ITEMS.iter().map(|i| (i.id, true)).collect();
        assert_eq!(level_of(&all_on), Some(Level::Appearance));
        assert!(get_item("nope").is_err());
        let _ = read_setting();
    }

    /// Flips the cursor shadow and restores it. Ignored by default: it touches the live session.
    #[test]
    #[ignore]
    fn cursor_shadow_roundtrip() {
        let before = get_item("cursor_shadow").unwrap();
        set_item("cursor_shadow", !before).unwrap();
        assert_eq!(get_item("cursor_shadow").unwrap(), !before);
        set_item("cursor_shadow", before).unwrap();
        assert_eq!(get_item("cursor_shadow").unwrap(), before);
    }
}
