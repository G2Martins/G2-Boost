//! In-game overlay: its settings (validated here, the file and the UI are hostile), the global
//! hotkey (RegisterHotKey on a dedicated thread with its own message loop, no plugin), where the
//! window goes (a corner of the monitor showing the game) and the Win32 calls that show it
//! without ever taking focus. The window itself is a Tauri webview made by the shell; the FPS
//! comes from `fps.rs`, the rest from `monitor.rs`.

use std::ffi::c_void;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromWindow};
use windows::Win32::System::Threading::{GetCurrentThreadId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetClassNameW, GetForegroundWindow, GetMessageW, GetWindowLongPtrW, GetWindowRect,
    GetWindowThreadProcessId, HWND_TOPMOST, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SW_HIDE,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos, ShowWindow, WM_HOTKEY,
    WM_QUIT, WM_USER, WS_EX_APPWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

/* ---------------- settings ---------------- */

/// What the overlay may show, in display order.
pub const METRICS: &[&str] = &[
    "game", "fps", "frame_time", "low1", "cpu_pct", "cpu_mhz", "cpu_temp", "gpu_pct", "gpu_temp", "gpu_mhz", "ram", "vram",
];
pub const CORNERS: &[&str] = &["top_left", "top_right", "bottom_left", "bottom_right"];
/// `bar` = one compact row, `list` = one line per group.
pub const STYLES: &[&str] = &["bar", "list"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct OverlaySettings {
    /// The hotkey is registered (and the overlay can appear) only while this is on.
    pub enabled: bool,
    /// From the safe list (`parse_hotkey`), canonical form like `Ctrl+Shift+F8`.
    pub hotkey: String,
    pub corner: String,
    /// Subset of [`METRICS`], kept in that order.
    pub metrics: Vec<String>,
    pub style: String,
    /// Background plate opacity, 0–100 %. The text stays opaque.
    pub opacity: u32,
    /// Scale, 75–150 %.
    pub size: u32,
}

impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            hotkey: "F7".into(),
            corner: "top_left".into(),
            metrics: ["fps", "frame_time", "low1", "cpu_pct", "gpu_pct", "gpu_temp", "ram"].map(String::from).to_vec(),
            style: "bar".into(),
            opacity: 70,
            size: 100,
        }
    }
}

/// Known values only, canonical order, bounded numbers. Anything else falls back to the default.
pub fn sanitize(s: OverlaySettings) -> OverlaySettings {
    let d = OverlaySettings::default();
    let pick = |v: String, list: &[&str], def: String| if list.contains(&v.as_str()) { v } else { def };
    let metrics: Vec<String> =
        METRICS.iter().filter(|m| s.metrics.iter().any(|x| x == *m)).map(|m| (*m).to_string()).collect();
    OverlaySettings {
        enabled: s.enabled,
        hotkey: canonical_hotkey(&s.hotkey).unwrap_or(d.hotkey),
        corner: pick(s.corner, CORNERS, d.corner),
        metrics: if metrics.is_empty() { d.metrics } else { metrics },
        style: pick(s.style, STYLES, d.style),
        opacity: s.opacity.min(100),
        size: s.size.clamp(75, 150),
    }
}

/* ---------------- hotkey ---------------- */

const MODS: &[(&str, HOT_KEY_MODIFIERS)] = &[("Ctrl", MOD_CONTROL), ("Alt", MOD_ALT), ("Shift", MOD_SHIFT)];
/// The safe list: keys games and typing rarely need. Never F12 (reserved for debuggers), the
/// Windows key, letters, digits, Esc, Tab or Enter.
pub const KEYS: &[(&str, u32)] = &[
    ("F1", 0x70), ("F2", 0x71), ("F3", 0x72), ("F4", 0x73), ("F5", 0x74), ("F6", 0x75), ("F7", 0x76), ("F8", 0x77),
    ("F9", 0x78), ("F10", 0x79), ("F11", 0x7A), ("Insert", 0x2D), ("Home", 0x24), ("End", 0x23), ("PageUp", 0x21),
    ("PageDown", 0x22), ("Pause", 0x13), ("ScrollLock", 0x91),
];
const VK_F4: u32 = 0x73;
const HOTKEY_ID: i32 = 0x6732; // any id; one hotkey per thread
const ERROR_HOTKEY_ALREADY_REGISTERED: u32 = 1409;

/// `"ctrl + F7"` → (MOD_CONTROL, VK_F7). Case and spaces do not matter; Ctrl/Alt/Shift at most
/// once each, then one key of [`KEYS`]. Alt+F4 is refused (it closes the game).
pub fn parse_hotkey(s: &str) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    if s.len() > 40 {
        return None;
    }
    let parts: Vec<&str> = s.split('+').map(str::trim).collect();
    let (key, mods) = parts.split_last()?;
    let vk = KEYS.iter().find(|(n, _)| n.eq_ignore_ascii_case(key))?.1;
    let mut m = HOT_KEY_MODIFIERS(0);
    for p in mods {
        let bit = MODS.iter().find(|(n, _)| n.eq_ignore_ascii_case(p))?.1;
        if m.0 & bit.0 != 0 {
            return None;
        }
        m |= bit;
    }
    if m == MOD_ALT && vk == VK_F4 {
        return None;
    }
    Some((m, vk))
}

/// The stored form: `Ctrl+Alt+Shift+Key` order with the list's spelling.
pub fn canonical_hotkey(s: &str) -> Option<String> {
    let (m, vk) = parse_hotkey(s)?;
    let mut out: Vec<&str> = MODS.iter().filter(|(_, b)| m.0 & b.0 != 0).map(|(n, _)| *n).collect();
    out.push(KEYS.iter().find(|(_, v)| *v == vk)?.0);
    Some(out.join("+"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HotkeyError {
    #[error("invalid_hotkey")]
    Invalid,
    /// Another program registered the same combination.
    #[error("hotkey_taken")]
    Taken,
    #[error("hotkey_error {0}")]
    Api(u32),
}

/// A registered global hotkey. Dropping it unregisters the key and ends its thread.
pub struct Hotkey {
    thread: u32,
    join: Option<JoinHandle<()>>,
}

impl Hotkey {
    /// `on_press` runs on the hotkey thread: keep it short (hand the work to another thread).
    pub fn register(spec: &str, on_press: impl Fn() + Send + 'static) -> Result<Hotkey, HotkeyError> {
        let (mods, vk) = parse_hotkey(spec).ok_or(HotkeyError::Invalid)?;
        let (tx, rx) = mpsc::sync_channel::<Result<u32, u32>>(1);
        let join = thread::Builder::new()
            .name("g2boost-hotkey".into())
            .spawn(move || {
                let mut msg = MSG::default();
                // SAFETY: message-queue calls on this thread only; the hotkey belongs to it.
                unsafe {
                    // Create this thread's queue now, so WM_QUIT posted by Drop always lands.
                    let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);
                    if let Err(e) = RegisterHotKey(None, HOTKEY_ID, mods | MOD_NOREPEAT, vk) {
                        let _ = tx.send(Err(e.code().0 as u32 & 0xFFFF)); // HRESULT_FROM_WIN32 → code
                        return;
                    }
                    let _ = tx.send(Ok(GetCurrentThreadId()));
                    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                        if msg.message == WM_HOTKEY {
                            on_press();
                        }
                    }
                    let _ = UnregisterHotKey(None, HOTKEY_ID);
                }
            })
            .map_err(|e| HotkeyError::Api(e.raw_os_error().unwrap_or(0) as u32))?;
        match rx.recv() {
            Ok(Ok(thread)) => Ok(Hotkey { thread, join: Some(join) }),
            Ok(Err(code)) => {
                let _ = join.join();
                Err(if code == ERROR_HOTKEY_ALREADY_REGISTERED { HotkeyError::Taken } else { HotkeyError::Api(code) })
            }
            Err(_) => {
                let _ = join.join();
                Err(HotkeyError::Api(0))
            }
        }
    }
}

impl Drop for Hotkey {
    fn drop(&mut self) {
        // SAFETY: posting to our own hotkey thread, whose queue exists since `register` returned.
        let _ = unsafe { PostThreadMessageW(self.thread, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/* ---------------- where the window goes ---------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl From<RECT> for Rect {
    fn from(r: RECT) -> Self {
        Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
    }
}

/// Window position and size in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Logical (96 DPI) size of the overlay window. The content sits in the chosen corner and the
/// rest is transparent and click-through, so this only has to be large enough.
pub fn window_size(s: &OverlaySettings) -> (u32, u32) {
    let n = s.metrics.len().max(1) as u32;
    let (w, h) = if s.style == "list" { (280, 40 + 26 * n) } else { (220 + 110 * n, 56) };
    (w * s.size / 100, h * s.size / 100)
}

/// Top-left of a `size` window in `corner` of `area`, `margin` from the edges, never outside it.
pub fn place(corner: &str, area: Rect, size: (i32, i32), margin: i32) -> (i32, i32) {
    let (w, h) = size;
    let left = area.left + margin;
    let top = area.top + margin;
    let right = (area.right - margin - w).max(area.left);
    let bottom = (area.bottom - margin - h).max(area.top);
    match corner {
        "top_right" => (right, top),
        "bottom_left" => (left, bottom),
        "bottom_right" => (right, bottom),
        _ => (left, top),
    }
}

/// The window covers the whole monitor: a borderless (or fullscreen) game.
pub fn covers(window: Rect, monitor: Rect) -> bool {
    window.left <= monitor.left && window.top <= monitor.top && window.right >= monitor.right && window.bottom >= monitor.bottom
}

/// Size for `scale` (monitor DPI / 96) and position in `area` (pure).
pub fn layout(s: &OverlaySettings, area: Rect, scale: f64) -> Placement {
    let scale = if scale.is_finite() { scale.clamp(0.5, 4.0) } else { 1.0 };
    let (lw, lh) = window_size(s);
    let w = ((f64::from(lw) * scale) as i32).min(area.right - area.left).max(1);
    let h = ((f64::from(lh) * scale) as i32).min(area.bottom - area.top).max(1);
    let (x, y) = place(&s.corner, area, (w, h), (16.0 * scale) as i32);
    Placement { x, y, w, h }
}

/// Where the overlay goes for the window `hwnd` (the game; none = primary monitor): its corner of
/// that monitor, the whole monitor when the game covers it, the work area otherwise.
pub fn placement(hwnd: Option<isize>, s: &OverlaySettings) -> Placement {
    let h = HWND(hwnd.unwrap_or(0) as *mut c_void);
    // SAFETY: monitor/window queries with sized out-structs; a stale handle only fails the call.
    unsafe {
        let mut mon = MonitorFromWindow(h, MONITOR_DEFAULTTOPRIMARY);
        if mon.0.is_null() {
            mon = MonitorFromWindow(HWND::default(), MONITOR_DEFAULTTOPRIMARY);
        }
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(mon, &mut mi).as_bool() {
            let full = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
            (mi.rcMonitor, mi.rcWork) = (full, full);
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_err() || dx == 0 {
            dx = 96;
        }
        let mut wr = RECT::default();
        let fullscreen = hwnd.is_some() && GetWindowRect(h, &mut wr).is_ok() && covers(wr.into(), mi.rcMonitor.into());
        let area = if fullscreen { mi.rcMonitor } else { mi.rcWork };
        layout(s, area.into(), f64::from(dx) / 96.0)
    }
}

/* ---------------- the game window ---------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub pid: u32,
    pub hwnd: isize,
}

/// Desktop, taskbars, Start/search and the task switcher: never the game.
const SHELL_CLASSES: &[&str] = &[
    "Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd", "Windows.UI.Core.CoreWindow",
    "XamlExplorerHostIslandWindow", "MultitaskingViewFrame", "TaskSwitcherWnd", "ForegroundStaging",
];

/// The foreground window and its process, unless it is this app or part of the Windows shell.
pub fn foreground() -> Option<Target> {
    // SAFETY: plain queries on the handle the system just returned; a stale one only fails them.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut buf = [0u16; 64];
        let n = usize::try_from(GetClassNameW(hwnd, &mut buf)).unwrap_or(0).min(buf.len());
        if SHELL_CLASSES.contains(&String::from_utf16_lossy(&buf[..n]).as_str()) {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&raw mut pid));
        (pid != 0 && pid != std::process::id()).then_some(Target { pid, hwnd: hwnd.0 as isize })
    }
}

/// Image path of a process (the overlay shows its name when it is not a known game).
pub fn image_path(pid: u32) -> Option<String> {
    // SAFETY: limited query handle, closed below.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let path = crate::system::image_path(h);
        let _ = CloseHandle(h);
        path
    }
}

/* ---------------- showing the window ---------------- */

/// Show the overlay window at `p`: topmost, click-through, out of Alt+Tab and the taskbar, and
/// never activated, so the game keeps focus and input. Call on the thread that owns the window.
pub fn show(hwnd: isize, p: &Placement) {
    let h = HWND(hwnd as *mut c_void);
    // SAFETY: style bits and position of our own window.
    unsafe {
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        let on = (WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW).0 as isize;
        let want = (ex | on) & !(WS_EX_APPWINDOW.0 as isize);
        let mut flags = SWP_NOACTIVATE | SWP_SHOWWINDOW;
        if want != ex {
            SetWindowLongPtrW(h, GWL_EXSTYLE, want);
            flags |= SWP_FRAMECHANGED;
        }
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), p.x, p.y, p.w, p.h, flags);
    }
}

pub fn hide(hwnd: isize) {
    // SAFETY: hiding our own window.
    unsafe {
        let _ = ShowWindow(HWND(hwnd as *mut c_void), SW_HIDE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_sanitize_hostile_values() {
        let s = sanitize(OverlaySettings {
            enabled: true,
            hotkey: "Win+F7".into(),
            corner: "../center".into(),
            metrics: ["ram", "nope", "fps", "fps", "cpu_temp", "<script>"].map(String::from).to_vec(),
            style: "BAR".into(),
            opacity: 9_999,
            size: 0,
        });
        assert!(s.enabled);
        assert_eq!(s.hotkey, "F7", "Win is not in the safe list");
        assert_eq!(s.corner, "top_left");
        assert_eq!(s.metrics, vec!["fps", "cpu_temp", "ram"], "known only, canonical order, no repeats");
        assert_eq!(s.style, "bar", "exact values only");
        assert_eq!((s.opacity, s.size), (100, 75));
        let s = sanitize(OverlaySettings { metrics: vec![], size: u32::MAX, hotkey: " ctrl + shift+f8 ".into(), ..Default::default() });
        assert_eq!(s.metrics, OverlaySettings::default().metrics, "an empty overlay falls back to the defaults");
        assert_eq!(s.size, 150);
        assert_eq!(s.hotkey, "Ctrl+Shift+F8");
        let d = OverlaySettings::default();
        assert_eq!(sanitize(d.clone()), d, "defaults are already clean");
        // a settings.json from another version: missing and unknown fields
        let parsed: OverlaySettings = serde_json::from_str(r#"{"enabled":true,"future":1}"#).unwrap();
        assert_eq!(sanitize(parsed), OverlaySettings { enabled: true, ..d.clone() });
        // wired into the settings file validation
        let file: crate::settings::Settings =
            serde_json::from_str(r#"{"overlay":{"corner":"middle","opacity":300,"hotkey":"F12"}}"#).unwrap();
        let s = crate::settings::sanitize(file).overlay;
        assert_eq!((s.corner.as_str(), s.opacity, s.hotkey.as_str()), ("top_left", 100, "F7"));
    }

    #[test]
    fn overlay_hotkey_safe_list() {
        let ok = [
            ("F7", "F7"),
            ("f7", "F7"),
            ("shift+ctrl+F9", "Ctrl+Shift+F9"),
            (" Alt + PageUp ", "Alt+PageUp"),
            ("Ctrl+Alt+Shift+ScrollLock", "Ctrl+Alt+Shift+ScrollLock"),
            ("Alt+Shift+F4", "Alt+Shift+F4"),
            ("Insert", "Insert"),
        ];
        for (input, want) in ok {
            assert_eq!(canonical_hotkey(input).as_deref(), Some(want), "{input}");
        }
        let (m, vk) = parse_hotkey("Ctrl+F8").unwrap();
        assert_eq!((m, vk), (MOD_CONTROL, 0x77));
        let bad = [
            "", "+", "F7+", "+F7", "Ctrl+", "F12", "Ctrl+F12", "Win+F7", "A", "Ctrl+A", "Space", "Esc", "Tab", "Enter",
            "Ctrl+Ctrl+F7", "Alt+F4", "alt+f4", "F7+Ctrl", "F7 F8", "F7+F8", "Ctrl\u{0}+F7", "Ｆ７",
        ];
        for input in bad {
            assert_eq!(parse_hotkey(input), None, "{input:?}");
        }
        assert_eq!(parse_hotkey(&format!("{}F7", "Ctrl+".repeat(10))), None, "too long");
    }

    #[test]
    fn overlay_place_corners_any_monitor() {
        let fhd = Rect { left: 0, top: 0, right: 1920, bottom: 1080 };
        let size = (200, 50);
        assert_eq!(place("top_left", fhd, size, 16), (16, 16));
        assert_eq!(place("top_right", fhd, size, 16), (1704, 16));
        assert_eq!(place("bottom_left", fhd, size, 16), (16, 1014));
        assert_eq!(place("bottom_right", fhd, size, 16), (1704, 1014));
        assert_eq!(place("nonsense", fhd, size, 16), (16, 16));
        // a monitor left of the primary (negative coordinates)
        let left = Rect { left: -2560, top: -200, right: 0, bottom: 1240 };
        assert_eq!(place("top_right", left, size, 16), (-216, -184));
        // larger than the area: pinned to its top-left, never off-screen
        let tiny = Rect { left: 100, top: 100, right: 150, bottom: 120 };
        assert_eq!(place("bottom_right", tiny, size, 16), (100, 100));
    }

    #[test]
    fn overlay_layout_scales_and_fits() {
        let fhd = Rect { left: 0, top: 0, right: 1920, bottom: 1080 };
        let s = OverlaySettings { style: "list".into(), corner: "bottom_right".into(), ..Default::default() };
        let (lw, lh) = window_size(&s);
        let p = layout(&s, fhd, 1.5);
        assert_eq!((p.w, p.h), ((lw as f64 * 1.5) as i32, (lh as f64 * 1.5) as i32));
        assert_eq!((p.x + p.w, p.y + p.h), (1920 - 24, 1080 - 24), "margin scales too");
        let all = OverlaySettings { metrics: METRICS.iter().map(|m| m.to_string()).collect(), size: 150, ..Default::default() };
        let narrow = Rect { left: 0, top: 0, right: 1280, bottom: 720 };
        let p = layout(&all, narrow, 2.0);
        assert!(p.w <= 1280 && p.x >= 0, "{p:?}");
        let p = layout(&all, narrow, f64::NAN);
        assert!(p.w > 0 && p.h > 0, "garbage scale falls back to 1");
        assert!(window_size(&OverlaySettings { size: 150, ..Default::default() }).0 > window_size(&OverlaySettings::default()).0);
    }

    #[test]
    fn overlay_covers_detects_borderless() {
        let mon = Rect { left: 0, top: 0, right: 2560, bottom: 1440 };
        assert!(covers(mon, mon));
        assert!(covers(Rect { left: -8, top: -8, right: 2568, bottom: 1448 }, mon), "maximized-style overhang");
        assert!(!covers(Rect { left: 0, top: 0, right: 1280, bottom: 720 }, mon), "a window");
    }

    #[test]
    fn overlay_hotkey_registers_and_releases() {
        let spec = "Ctrl+Alt+Shift+F11"; // a combination nobody uses
        let first = match Hotkey::register(spec, || {}) {
            Ok(h) => h,
            Err(e) => {
                println!("skipped: this session cannot register hotkeys ({e})");
                return;
            }
        };
        assert_eq!(Hotkey::register(spec, || {}).err(), Some(HotkeyError::Taken), "already ours");
        drop(first);
        assert!(Hotkey::register(spec, || {}).is_ok(), "dropping released it");
        assert_eq!(Hotkey::register("F12", || {}).err(), Some(HotkeyError::Invalid));
    }
}
