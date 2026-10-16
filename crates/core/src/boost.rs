//! PC Boost: tweaks with `detect` / `apply` / `revert`.
//!
//! Every tweak is data (registry ops, service names, SPI parameters, a power scheme).
//! `apply` captures the exact current values into a [`Backup`] before writing; `revert`
//! restores that backup. Without a backup (the user changed things by hand before using
//! the app), turning a tweak off writes the documented Windows default and says so.
//! Nothing here spawns a process; admin-only paths return `needs_admin` instead of trying.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null_mut;

use serde::Serialize;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_NOT_ALL_ASSIGNED, ERROR_SUCCESS, GetLastError, HANDLE, HLOCAL, LUID, LocalFree,
    NTSTATUS, WIN32_ERROR,
};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::Power::{
    ACCESS_SCHEME, PowerDuplicateScheme, PowerEnumerate, PowerGetActiveScheme, PowerReadFriendlyName,
    PowerSetActiveScheme,
};
use windows::Win32::System::Services::{
    ChangeServiceConfigW, CloseServiceHandle, ControlService, ENUM_SERVICE_TYPE, OpenSCManagerW,
    OpenServiceW, QUERY_SERVICE_CONFIGW, QueryServiceConfigW, QueryServiceStatusEx, SC_HANDLE,
    SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO, SERVICE_AUTO_START, SERVICE_CHANGE_CONFIG,
    SERVICE_CONTROL_STOP, SERVICE_DEMAND_START, SERVICE_DISABLED, SERVICE_ERROR, SERVICE_NO_CHANGE,
    SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START, SERVICE_START_TYPE,
    SERVICE_STATUS, SERVICE_STATUS_PROCESS, SERVICE_STOP, StartServiceW,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::{SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETKEYBOARDDELAY, SPI_GETKEYBOARDSPEED, SPI_GETMOUSE, SPI_SETKEYBOARDDELAY, SPI_SETKEYBOARDSPEED,
    SPI_SETMOUSE, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SW_SHOWNORMAL, SYSTEM_PARAMETERS_INFO_ACTION,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::{GUID, HRESULT, PCWSTR};

use crate::registry::{self, Backup, BackupEntry, Root, Value, wide};
use crate::system;
use crate::tasksched;
use crate::visualfx::{self, Level};

/* ---------------- errors ---------------- */

#[derive(Debug, thiserror::Error)]
pub enum BoostError {
    /// Stable code: the UI maps it to "needs administrator".
    #[error("needs_admin")]
    NeedsAdmin,
    #[error("unknown_tweak")]
    UnknownTweak,
    /// The user dismissed the UAC prompt.
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Registry(#[from] registry::RegError),
    #[error("{0}")]
    VisualFx(#[from] visualfx::FxError),
    #[error("{0}")]
    Task(#[from] tasksched::TaskError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("win32: {0}")]
    Win32(String),
}

const E_ACCESS_DENIED: HRESULT = HRESULT(0x8007_0005_u32 as i32);
const E_CANCELLED: HRESULT = HRESULT(0x8007_04C7_u32 as i32);
const E_SERVICE_DOES_NOT_EXIST: HRESULT = HRESULT(0x8007_0424_u32 as i32);
const ERROR_NO_MORE_ITEMS: WIN32_ERROR = WIN32_ERROR(259);
const ERROR_ACCESS_DENIED: WIN32_ERROR = WIN32_ERROR(5);

fn map_win(e: windows::core::Error) -> BoostError {
    match e.code() {
        E_ACCESS_DENIED => BoostError::NeedsAdmin,
        E_CANCELLED => BoostError::Cancelled,
        code => BoostError::Win32(format!("{code:?}: {}", e.message())),
    }
}

fn check(e: WIN32_ERROR) -> Result<(), BoostError> {
    match e {
        ERROR_SUCCESS => Ok(()),
        ERROR_ACCESS_DENIED => Err(BoostError::NeedsAdmin),
        WIN32_ERROR(code) => Err(BoostError::Win32(format!("error {code}"))),
    }
}

/* ---------------- catalog ---------------- */

#[derive(Debug, Clone, Copy)]
enum Desired {
    Dword(u32),
    Str(&'static str),
    Absent,
}

impl Desired {
    fn value(self) -> Value {
        match self {
            Desired::Dword(d) => Value::Dword(d),
            Desired::Str(s) => Value::Str(s.to_string()),
            Desired::Absent => Value::Absent,
        }
    }
}

/// One registry value a tweak controls: `on` = tweak applied, `off` = documented Windows default
/// (used only when there is no backup to restore).
#[derive(Debug, Clone, Copy)]
struct RegOp {
    root: Root,
    path: &'static str,
    name: &'static str,
    on: Desired,
    off: Desired,
}

#[derive(Debug, Clone, Copy)]
struct Svc {
    name: &'static str,
    /// Windows default start type (SERVICE_AUTO_START = 2, SERVICE_DEMAND_START = 3).
    default_start: u32,
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Registry(&'static [RegOp]),
    Services(&'static [Svc]),
    /// `TcpAckFrequency=1`, `TCPNoDelay=1`, `TcpDelAckTicks=0` on every TCP/IP interface.
    TcpInterfaces,
    /// Mouse acceleration off (`SPI_SETMOUSE` thresholds/speed = 0).
    Mouse,
    /// Keyboard repeat delay 0 / speed 31 (`SPI_SETKEYBOARDDELAY/SPEED`).
    Keyboard,
    /// Active power scheme = Ultimate Performance (duplicated from the hidden template if needed).
    Power,
    /// The visual-effects level (see `visualfx`).
    VisualFx,
    /// Scheduled tasks (folder, name) to disable — e.g. the Compatibility Appraiser.
    Tasks(&'static [(&'static str, &'static str)]),
    /// One `Key=1;` flag inside `DirectXUserGlobalSettings` (Settings › Display › Graphics).
    DxFlag(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Performance,
    Privacy,
    Appearance,
    Network,
}

#[derive(Debug, Clone, Copy)]
pub struct Tweak {
    pub id: &'static str,
    pub admin: bool,
    pub beta: bool,
    pub category: Category,
    kind: Kind,
}

const HKCU: Root = Root::Hkcu;
const HKLM: Root = Root::Hklm;
const GAME_CONFIG: &str = r"System\GameConfigStore";
const GAME_BAR: &str = r"Software\Microsoft\GameBar";
const GAME_DVR: &str = r"Software\Microsoft\Windows\CurrentVersion\GameDVR";
const CDM: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";
const MM_PROFILE: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile";
const MM_GAMES: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games";
const TCP_INTERFACES: &str = r"SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces";

const DX_PREFS: &str = r"Software\Microsoft\DirectX\UserGpuPreferences";
const DX_GLOBAL: &str = "DirectXUserGlobalSettings";

/// `DirectXUserGlobalSettings` holds `Key=Value;` pairs shared with Auto HDR and VRR:
/// change one key, keep the others.
pub(crate) fn dx_set(current: &str, key: &str, value: Option<&str>) -> String {
    let mut out: String = current
        .split(';')
        .filter(|kv| !kv.is_empty() && kv.split('=').next() != Some(key))
        .map(|kv| format!("{kv};"))
        .collect();
    if let Some(v) = value {
        out.push_str(&format!("{key}={v};"));
    }
    out
}

pub(crate) fn dx_get<'a>(current: &'a str, key: &str) -> Option<&'a str> {
    current.split(';').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v))
}

fn dx_read() -> Result<String, BoostError> {
    Ok(match registry::read(HKCU, DX_PREFS, DX_GLOBAL)? {
        Value::Str(s) => s,
        _ => String::new(),
    })
}

fn dx_write(key: &str, value: Option<&str>) -> Result<(), BoostError> {
    let next = dx_set(&dx_read()?, key, value);
    let v = if next.is_empty() { Value::Absent } else { Value::Str(next) };
    registry::write(HKCU, DX_PREFS, DX_GLOBAL, &v)?;
    Ok(())
}

const fn op(root: Root, path: &'static str, name: &'static str, on: Desired, off: Desired) -> RegOp {
    RegOp { root, path, name, on, off }
}

const GAME_MODE: &[RegOp] = &[
    op(HKCU, GAME_BAR, "AutoGameModeEnabled", Desired::Dword(1), Desired::Absent),
    op(HKCU, GAME_BAR, "AllowAutoGameMode", Desired::Dword(1), Desired::Absent),
    op(HKCU, GAME_CONFIG, "GameDVR_Enabled", Desired::Dword(0), Desired::Dword(1)),
    op(HKCU, GAME_DVR, "AppCaptureEnabled", Desired::Dword(0), Desired::Dword(1)),
];
const INPUT_LAG: &[RegOp] = &[
    op(HKCU, GAME_CONFIG, "GameDVR_FSEBehaviorMode", Desired::Dword(2), Desired::Absent),
    op(HKCU, GAME_CONFIG, "GameDVR_HonorUserFSEBehaviorMode", Desired::Dword(1), Desired::Absent),
    op(HKCU, GAME_CONFIG, "GameDVR_FSEBehavior", Desired::Dword(2), Desired::Absent),
    op(HKCU, GAME_CONFIG, "GameDVR_DXGIHonorFSEWindowsCompatible", Desired::Dword(1), Desired::Absent),
    op(HKCU, GAME_CONFIG, "GameDVR_EFSEFeatureFlags", Desired::Dword(0), Desired::Absent),
];
const TIPS: &[RegOp] = &[
    op(HKCU, CDM, "SubscribedContent-338389Enabled", Desired::Dword(0), Desired::Dword(1)),
    op(HKCU, CDM, "SoftLandingEnabled", Desired::Dword(0), Desired::Dword(1)),
    op(HKCU, CDM, "SystemPaneSuggestionsEnabled", Desired::Dword(0), Desired::Dword(1)),
    op(HKCU, CDM, "SubscribedContent-310093Enabled", Desired::Dword(0), Desired::Dword(1)),
    op(HKCU, CDM, "SubscribedContent-353694Enabled", Desired::Dword(0), Desired::Dword(1)),
    op(HKCU, CDM, "SubscribedContent-353696Enabled", Desired::Dword(0), Desired::Dword(1)),
];
const HAGS: &[RegOp] = &[op(
    HKLM,
    r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers",
    "HwSchMode",
    Desired::Dword(2),
    Desired::Dword(1),
)];
const MMCSS: &[RegOp] = &[
    op(HKLM, MM_PROFILE, "SystemResponsiveness", Desired::Dword(10), Desired::Dword(20)),
    op(HKLM, MM_GAMES, "GPU Priority", Desired::Dword(8), Desired::Dword(8)),
    op(HKLM, MM_GAMES, "Priority", Desired::Dword(6), Desired::Dword(2)),
    op(HKLM, MM_GAMES, "Scheduling Category", Desired::Str("High"), Desired::Str("Medium")),
    op(HKLM, MM_GAMES, "SFIO Priority", Desired::Str("High"), Desired::Str("Normal")),
];
const PERF_COUNTERS: &[RegOp] = &[op(
    HKLM,
    r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Perflib",
    "Disable Performance Counters",
    Desired::Dword(1),
    Desired::Absent,
)];
const PRIORITY_SEP: &[RegOp] = &[op(
    HKLM,
    r"SYSTEM\CurrentControlSet\Control\PriorityControl",
    "Win32PrioritySeparation",
    Desired::Dword(0x26),
    Desired::Dword(2),
)];
const EXPLORER_ADV: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const SHOW_EXTENSIONS: &[RegOp] = &[op(HKCU, EXPLORER_ADV, "HideFileExt", Desired::Dword(0), Desired::Dword(1))];
const WIDGETS_OFF: &[RegOp] = &[op(HKCU, EXPLORER_ADV, "TaskbarDa", Desired::Dword(0), Desired::Dword(1))];
const SEARCH_HIGHLIGHTS_OFF: &[RegOp] = &[
    op(HKCU, r"Software\Microsoft\Windows\CurrentVersion\SearchSettings", "IsDynamicSearchBoxEnabled", Desired::Dword(0), Desired::Dword(1)),
    op(HKCU, r"Software\Microsoft\Windows\CurrentVersion\Search", "BingSearchEnabled", Desired::Dword(0), Desired::Absent),
];
const SILENT_APPS_OFF: &[RegOp] = &[
    op(HKCU, CDM, "SilentInstalledAppsEnabled", Desired::Dword(0), Desired::Dword(1)),
    op(HKCU, CDM, "PreInstalledAppsEnabled", Desired::Dword(0), Desired::Dword(1)),
    op(HKCU, CDM, "OemPreInstalledAppsEnabled", Desired::Dword(0), Desired::Dword(1)),
];
const ADVERTISING_ID_OFF: &[RegOp] = &[op(
    HKCU,
    r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo",
    "Enabled",
    Desired::Dword(0),
    Desired::Dword(1),
)];
/// Core Isolation / Memory Integrity. The only lever with a measured FPS gain (~5 %, Tom's
/// Hardware) and the only one that lowers security. Beta, custom preset only, reboot.
const HVCI_OFF: &[RegOp] = &[op(
    HKLM,
    r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity",
    "Enabled",
    Desired::Dword(0),
    Desired::Dword(1),
)];
/// Compatibility telemetry (CompatTelRunner) and CEIP run from these tasks, not from a service.
/// Task names differ by build ("… Appraiser" up to 23H2, "… Appraiser Exp" on 24H2+); missing ones are skipped.
const TASKS_COMPAT_TELEMETRY: &[(&str, &str)] = &[
    (r"\Microsoft\Windows\Application Experience", "Microsoft Compatibility Appraiser"),
    (r"\Microsoft\Windows\Application Experience", "Microsoft Compatibility Appraiser Exp"),
    (r"\Microsoft\Windows\Application Experience", "ProgramDataUpdater"),
    (r"\Microsoft\Windows\Customer Experience Improvement Program", "Consolidator"),
    (r"\Microsoft\Windows\Customer Experience Improvement Program", "UsbCeip"),
];
const TCP_OPS: [(&str, Desired, Desired); 3] = [
    ("TcpAckFrequency", Desired::Dword(1), Desired::Absent),
    ("TCPNoDelay", Desired::Dword(1), Desired::Absent),
    ("TcpDelAckTicks", Desired::Dword(0), Desired::Absent),
];

const AUTO: u32 = SERVICE_AUTO_START.0;
const MANUAL: u32 = SERVICE_DEMAND_START.0;
const SVC_SYSMAIN: &[Svc] = &[Svc { name: "SysMain", default_start: AUTO }];
const SVC_TELEMETRY: &[Svc] = &[
    Svc { name: "DiagTrack", default_start: AUTO },
    Svc { name: "dmwappushservice", default_start: MANUAL },
];
const SVC_SEARCH: &[Svc] = &[Svc { name: "WSearch", default_start: AUTO }];
const SVC_XBOX: &[Svc] = &[
    Svc { name: "XblAuthManager", default_start: MANUAL },
    Svc { name: "XblGameSave", default_start: MANUAL },
    Svc { name: "XboxNetApiSvc", default_start: MANUAL },
    Svc { name: "XboxGipSvc", default_start: MANUAL },
];
const SVC_DPS: &[Svc] = &[Svc { name: "DPS", default_start: AUTO }];
const SVC_GEOLOCATION: &[Svc] = &[Svc { name: "lfsvc", default_start: MANUAL }];

const fn tw(id: &'static str, admin: bool, beta: bool, category: Category, kind: Kind) -> Tweak {
    Tweak { id, admin, beta, category, kind }
}

use Category::{Appearance, Network, Performance, Privacy};

/// The catalog. Presets live in the UI (`src/data/catalogs.ts`); mechanics live here.
/// Evidence for the service choices: Docs/references/windows-services-research.md.
pub const TWEAKS: &[Tweak] = &[
    tw("visual_fx", false, false, Appearance, Kind::VisualFx),
    tw("game_mode", false, false, Performance, Kind::Registry(GAME_MODE)),
    tw("input_lag", false, false, Performance, Kind::Registry(INPUT_LAG)),
    tw("windowed_optimizations", false, false, Performance, Kind::DxFlag("SwapEffectUpgradeEnable")),
    tw("keyboard", false, false, Performance, Kind::Keyboard),
    tw("mouse", false, false, Performance, Kind::Mouse),
    tw("tips", false, false, Privacy, Kind::Registry(TIPS)),
    tw("show_extensions", false, false, Appearance, Kind::Registry(SHOW_EXTENSIONS)),
    tw("widgets_off", false, false, Appearance, Kind::Registry(WIDGETS_OFF)),
    tw("search_highlights_off", false, false, Privacy, Kind::Registry(SEARCH_HIGHLIGHTS_OFF)),
    tw("silent_apps_off", false, false, Privacy, Kind::Registry(SILENT_APPS_OFF)),
    tw("advertising_id_off", false, false, Privacy, Kind::Registry(ADVERTISING_ID_OFF)),
    tw("compat_telemetry_off", true, false, Privacy, Kind::Tasks(TASKS_COMPAT_TELEMETRY)),
    tw("svc_telemetry", true, false, Privacy, Kind::Services(SVC_TELEMETRY)),
    tw("svc_sysmain", true, false, Performance, Kind::Services(SVC_SYSMAIN)),
    tw("power_plan", true, false, Performance, Kind::Power),
    tw("hags", true, false, Performance, Kind::Registry(HAGS)),
    tw("mmcss", true, false, Performance, Kind::Registry(MMCSS)),
    tw("network_latency", true, false, Network, Kind::TcpInterfaces),
    tw("svc_search", true, false, Performance, Kind::Services(SVC_SEARCH)),
    tw("svc_xbox", true, false, Performance, Kind::Services(SVC_XBOX)),
    tw("svc_dps", true, false, Performance, Kind::Services(SVC_DPS)),
    tw("svc_geolocation", true, false, Privacy, Kind::Services(SVC_GEOLOCATION)),
    tw("hvci_off", true, true, Performance, Kind::Registry(HVCI_OFF)),
    tw("priority_sep", true, true, Performance, Kind::Registry(PRIORITY_SEP)),
    tw("perf_counters", true, true, Performance, Kind::Registry(PERF_COUNTERS)),
];

pub fn find(id: &str) -> Result<&'static Tweak, BoostError> {
    TWEAKS.iter().find(|t| t.id == id).ok_or(BoostError::UnknownTweak)
}

/* ---------------- state ---------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    On,
    Off,
    Partial,
    /// Could not be read, or the thing it controls does not exist on this machine.
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceState {
    pub name: String,
    /// `auto` | `manual` | `disabled` | `other`
    pub start: &'static str,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TweakState {
    pub id: &'static str,
    pub admin: bool,
    pub beta: bool,
    pub category: Category,
    pub status: Status,
    pub has_backup: bool,
    /// Visual effects only: the level the current state matches (None = custom mix).
    pub level: Option<Level>,
    /// Power plan only: the active scheme's friendly name.
    pub detail: Option<String>,
    /// Service tweaks only.
    pub services: Vec<ServiceState>,
}

fn status_of(matches: usize, total: usize) -> Status {
    if total == 0 {
        Status::Unavailable
    } else if matches == total {
        Status::On
    } else if matches == 0 {
        Status::Off
    } else {
        Status::Partial
    }
}

fn start_label(start: u32) -> &'static str {
    match SERVICE_START_TYPE(start) {
        SERVICE_AUTO_START => "auto",
        SERVICE_DEMAND_START => "manual",
        SERVICE_DISABLED => "disabled",
        _ => "other",
    }
}

pub fn detect(t: &Tweak) -> TweakState {
    let mut st = TweakState {
        id: t.id,
        admin: t.admin,
        beta: t.beta,
        category: t.category,
        status: Status::Unavailable,
        has_backup: registry::load_backup(t.id).is_some(),
        level: None,
        detail: None,
        services: Vec::new(),
    };
    match t.kind {
        Kind::Registry(ops) => {
            let mut matches = 0;
            for o in ops {
                match registry::read(o.root, o.path, o.name) {
                    Ok(v) if v == o.on.value() => matches += 1,
                    Ok(_) => {}
                    Err(_) => return st,
                }
            }
            st.status = status_of(matches, ops.len());
        }
        Kind::Services(list) => {
            let mut total = 0;
            let mut disabled = 0;
            for s in list {
                if let Ok(Some((start, running))) = svc_query(s.name) {
                    total += 1;
                    if SERVICE_START_TYPE(start) == SERVICE_DISABLED {
                        disabled += 1;
                    }
                    st.services.push(ServiceState { name: s.name.to_string(), start: start_label(start), running });
                }
            }
            st.status = status_of(disabled, total);
        }
        Kind::TcpInterfaces => {
            let Ok(ifaces) = registry::subkeys(HKLM, TCP_INTERFACES) else {
                return st;
            };
            let mut matches = 0;
            for guid in &ifaces {
                let path = format!(r"{TCP_INTERFACES}\{guid}");
                if TCP_OPS.iter().all(|(name, on, _)| registry::read(HKLM, &path, name).ok() == Some(on.value())) {
                    matches += 1;
                }
            }
            st.status = status_of(matches, ifaces.len());
        }
        Kind::Mouse => {
            if let Ok(m) = spi_get_mouse() {
                st.status = if m == [0, 0, 0] { Status::On } else { Status::Off };
            }
        }
        Kind::Keyboard => {
            if let (Ok(delay), Ok(speed)) = (spi_get_u32(SPI_GETKEYBOARDDELAY), spi_get_u32(SPI_GETKEYBOARDSPEED)) {
                st.status = if delay == 0 && speed == 31 { Status::On } else { Status::Off };
            }
        }
        Kind::Power => {
            if let Ok(active) = active_scheme() {
                let name = friendly_name(&active);
                st.status = if name == friendly_name(&ULTIMATE) { Status::On } else { Status::Off };
                st.detail = Some(name);
            }
        }
        Kind::VisualFx => {
            let items = visualfx::detect();
            let pairs: Vec<(&str, bool)> = items.iter().map(|i| (i.id, i.on)).collect();
            st.level = visualfx::level_of(&pairs);
            st.status = match st.level {
                Some(Level::Appearance) => Status::Off,
                Some(_) => Status::On,
                None => Status::Partial,
            };
        }
        Kind::Tasks(list) => {
            let mut total = 0;
            let mut disabled = 0;
            for (folder, name) in list {
                if let Ok(Some(enabled)) = tasksched::task_enabled(folder, name) {
                    total += 1;
                    if !enabled {
                        disabled += 1;
                    }
                }
            }
            st.status = status_of(disabled, total);
        }
        Kind::DxFlag(key) => {
            if let Ok(s) = dx_read() {
                st.status = if dx_get(&s, key) == Some("1") { Status::On } else { Status::Off };
            }
        }
    }
    st
}

pub fn detect_all() -> Vec<TweakState> {
    TWEAKS.iter().map(detect).collect()
}

/* ---------------- apply / revert ---------------- */

/// Turn a tweak on (with backup) or off (restore backup, else Windows default).
pub fn apply(id: &str, on: bool) -> Result<TweakState, BoostError> {
    let t = find(id)?;
    if t.admin && !system::is_elevated() {
        return Err(BoostError::NeedsAdmin);
    }
    if let Kind::VisualFx = t.kind {
        return if on { apply_visual_fx(Level::Balanced) } else { revert_visual_fx() };
    }
    if on {
        ensure_backup(t)?;
        apply_on(t)?;
    } else if let Some(b) = registry::load_backup(t.id) {
        restore_entries(&b.entries)?;
        registry::delete_backup(t.id);
    } else {
        apply_default(t)?;
    }
    // Explorer/shell settings (file extensions, widgets, search box) pick this up live.
    visualfx::broadcast_setting_change();
    Ok(detect(t))
}

pub fn apply_visual_fx(level: Level) -> Result<TweakState, BoostError> {
    let t = find("visual_fx")?;
    ensure_backup(t)?;
    for item in visualfx::ITEMS.iter() {
        let want = visualfx::desired(level, item.id);
        if visualfx::get_item(item.id).map(|cur| cur != want).unwrap_or(true) {
            visualfx::set_item(item.id, want)?;
        }
    }
    visualfx::write_setting(visualfx::setting_for(level))?;
    visualfx::broadcast_setting_change();
    Ok(detect(t))
}

pub fn revert_visual_fx() -> Result<TweakState, BoostError> {
    let t = find("visual_fx")?;
    match registry::load_backup(t.id) {
        Some(b) => {
            restore_entries(&b.entries)?;
            registry::delete_backup(t.id);
        }
        None => {
            // No backup: the only honest "off" is the dialog's "best appearance".
            for item in visualfx::ITEMS.iter() {
                visualfx::set_item(item.id, true)?;
            }
            visualfx::write_setting(1)?;
        }
    }
    visualfx::broadcast_setting_change();
    Ok(detect(t))
}

fn ensure_backup(t: &Tweak) -> Result<(), BoostError> {
    if registry::load_backup(t.id).is_some() {
        return Ok(());
    }
    let entries = capture(t)?;
    registry::save_backup(&Backup { tweak: t.id.to_string(), taken_at: registry::now_unix(), entries })?;
    Ok(())
}

fn entry(path: impl Into<String>, name: impl Into<String>, value: Value) -> BackupEntry {
    BackupEntry { root: HKCU, path: path.into(), name: name.into(), value }
}

fn capture(t: &Tweak) -> Result<Vec<BackupEntry>, BoostError> {
    Ok(match t.kind {
        Kind::Registry(ops) => ops
            .iter()
            .map(|o| Ok(BackupEntry { root: o.root, path: o.path.into(), name: o.name.into(), value: registry::read(o.root, o.path, o.name)? }))
            .collect::<Result<Vec<_>, registry::RegError>>()?,
        Kind::Services(list) => {
            let mut out = Vec::new();
            for s in list {
                if let Some((start, running)) = svc_query(s.name)? {
                    out.push(entry(format!("svc:{}", s.name), "start", Value::Dword(start)));
                    out.push(entry(format!("svc:{}", s.name), "running", Value::Dword(running as u32)));
                }
            }
            out
        }
        Kind::TcpInterfaces => {
            let mut out = Vec::new();
            for guid in registry::subkeys(HKLM, TCP_INTERFACES)? {
                let path = format!(r"{TCP_INTERFACES}\{guid}");
                for (name, _, _) in TCP_OPS {
                    out.push(BackupEntry { root: HKLM, path: path.clone(), name: name.into(), value: registry::read(HKLM, &path, name)? });
                }
            }
            out
        }
        Kind::Mouse => {
            let m = spi_get_mouse()?;
            vec![
                entry("spi:mouse", "threshold1", Value::Dword(m[0] as u32)),
                entry("spi:mouse", "threshold2", Value::Dword(m[1] as u32)),
                entry("spi:mouse", "speed", Value::Dword(m[2] as u32)),
            ]
        }
        Kind::Keyboard => vec![
            entry("spi:keyboard", "delay", Value::Dword(spi_get_u32(SPI_GETKEYBOARDDELAY)?)),
            entry("spi:keyboard", "speed", Value::Dword(spi_get_u32(SPI_GETKEYBOARDSPEED)?)),
        ],
        Kind::Power => vec![entry("power:active", "guid", Value::Str(format!("{:?}", active_scheme()?)))],
        Kind::VisualFx => {
            let mut out: Vec<BackupEntry> = visualfx::detect()
                .into_iter()
                .map(|i| entry("spi:visualfx", i.id, Value::Dword(i.on as u32)))
                .collect();
            out.push(entry(visualfx::VISUAL_FX_SETTING_PATH, visualfx::VISUAL_FX_SETTING_NAME, Value::Dword(visualfx::read_setting())));
            out
        }
        Kind::Tasks(list) => {
            let mut out = Vec::new();
            for (folder, name) in list {
                if let Some(enabled) = tasksched::task_enabled(folder, name)? {
                    out.push(entry(format!("task:{folder}|{name}"), "enabled", Value::Dword(enabled as u32)));
                }
            }
            out
        }
        // The whole string: revert restores Auto HDR / VRR flags exactly as they were.
        Kind::DxFlag(_) => vec![entry(DX_PREFS, DX_GLOBAL, registry::read(HKCU, DX_PREFS, DX_GLOBAL)?)],
    })
}

fn apply_on(t: &Tweak) -> Result<(), BoostError> {
    match t.kind {
        Kind::Registry(ops) => {
            for o in ops {
                registry::write(o.root, o.path, o.name, &o.on.value())?;
            }
        }
        Kind::Services(list) => {
            for s in list {
                if svc_query(s.name)?.is_some() {
                    svc_set_start(s.name, SERVICE_DISABLED.0)?;
                    svc_stop(s.name)?;
                }
            }
        }
        Kind::TcpInterfaces => {
            for guid in registry::subkeys(HKLM, TCP_INTERFACES)? {
                let path = format!(r"{TCP_INTERFACES}\{guid}");
                for (name, on, _) in TCP_OPS {
                    registry::write(HKLM, &path, name, &on.value())?;
                }
            }
        }
        Kind::Mouse => spi_set_mouse([0, 0, 0])?,
        Kind::Keyboard => {
            spi_set_u32(SPI_SETKEYBOARDDELAY, 0)?;
            spi_set_u32(SPI_SETKEYBOARDSPEED, 31)?;
        }
        Kind::Power => {
            let g = ensure_ultimate_scheme()?;
            set_active_scheme(&g)?;
        }
        Kind::Tasks(list) => {
            for (folder, name) in list {
                tasksched::set_task_enabled(folder, name, false)?;
            }
        }
        Kind::DxFlag(key) => dx_write(key, Some("1"))?,
        Kind::VisualFx => unreachable!("handled by apply_visual_fx"),
    }
    Ok(())
}

/// No backup available: write the documented Windows defaults.
fn apply_default(t: &Tweak) -> Result<(), BoostError> {
    match t.kind {
        Kind::Registry(ops) => {
            for o in ops {
                registry::write(o.root, o.path, o.name, &o.off.value())?;
            }
        }
        Kind::Services(list) => {
            for s in list {
                if svc_query(s.name)?.is_some() {
                    svc_set_start(s.name, s.default_start)?;
                }
            }
        }
        Kind::TcpInterfaces => {
            for guid in registry::subkeys(HKLM, TCP_INTERFACES)? {
                let path = format!(r"{TCP_INTERFACES}\{guid}");
                for (name, _, off) in TCP_OPS {
                    registry::write(HKLM, &path, name, &off.value())?;
                }
            }
        }
        Kind::Mouse => spi_set_mouse([6, 10, 1])?,
        Kind::Keyboard => {
            spi_set_u32(SPI_SETKEYBOARDDELAY, 1)?;
            spi_set_u32(SPI_SETKEYBOARDSPEED, 31)?;
        }
        Kind::Power => set_active_scheme(&BALANCED)?,
        Kind::Tasks(list) => {
            for (folder, name) in list {
                tasksched::set_task_enabled(folder, name, true)?;
            }
        }
        Kind::DxFlag(key) => dx_write(key, None)?,
        Kind::VisualFx => unreachable!("handled by revert_visual_fx"),
    }
    Ok(())
}

/// Restore captured entries in order. Pseudo-paths route to SPI / SCM / power.
fn restore_entries(entries: &[BackupEntry]) -> Result<(), BoostError> {
    let mut mouse: Option<[i32; 3]> = None;
    for e in entries {
        let dword = match e.value {
            Value::Dword(d) => Some(d),
            _ => None,
        };
        match e.path.as_str() {
            "spi:mouse" => {
                let m = mouse.get_or_insert([6, 10, 1]);
                let idx = match e.name.as_str() {
                    "threshold1" => 0,
                    "threshold2" => 1,
                    _ => 2,
                };
                m[idx] = dword.unwrap_or(0) as i32;
            }
            "spi:keyboard" => {
                let action = if e.name == "delay" { SPI_SETKEYBOARDDELAY } else { SPI_SETKEYBOARDSPEED };
                spi_set_u32(action, dword.unwrap_or(1))?;
            }
            "spi:visualfx" => visualfx::set_item(&e.name, dword.unwrap_or(1) != 0)?,
            "power:active" => {
                if let Value::Str(s) = &e.value
                    && let Ok(g) = GUID::try_from(s.as_str())
                {
                    set_active_scheme(&g)?;
                }
            }
            p if p.starts_with("task:") => {
                if let Some((folder, name)) = p[5..].split_once('|') {
                    tasksched::set_task_enabled(folder, name, dword.unwrap_or(1) != 0)?;
                }
            }
            p if p.starts_with("svc:") => {
                let name = &p[4..];
                if e.name == "start" {
                    svc_set_start(name, dword.unwrap_or(MANUAL))?;
                } else if dword == Some(1) {
                    svc_start(name)?;
                }
            }
            _ => registry::write(e.root, &e.path, &e.name, &e.value)?,
        }
    }
    if let Some(m) = mouse {
        spi_set_mouse(m)?;
    }
    Ok(())
}

/* ---------------- SPI helpers (mouse / keyboard) ---------------- */

const SPI_FLAGS: SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS =
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(SPIF_UPDATEINIFILE.0 | SPIF_SENDCHANGE.0);

fn spi_get_mouse() -> Result<[i32; 3], BoostError> {
    let mut m = [0i32; 3];
    // SAFETY: SPI_GETMOUSE fills an int[3] (threshold1, threshold2, acceleration).
    unsafe { SystemParametersInfoW(SPI_GETMOUSE, 0, Some(m.as_mut_ptr().cast::<c_void>()), Default::default()) }
        .map_err(map_win)?;
    Ok(m)
}

fn spi_set_mouse(mut m: [i32; 3]) -> Result<(), BoostError> {
    // SAFETY: SPI_SETMOUSE reads an int[3].
    unsafe { SystemParametersInfoW(SPI_SETMOUSE, 0, Some(m.as_mut_ptr().cast::<c_void>()), SPI_FLAGS) }.map_err(map_win)
}

fn spi_get_u32(action: SYSTEM_PARAMETERS_INFO_ACTION) -> Result<u32, BoostError> {
    let mut v = 0u32;
    // SAFETY: these GET actions write a DWORD through pvParam.
    unsafe { SystemParametersInfoW(action, 0, Some((&mut v as *mut u32).cast::<c_void>()), Default::default()) }
        .map_err(map_win)?;
    Ok(v)
}

fn spi_set_u32(action: SYSTEM_PARAMETERS_INFO_ACTION, v: u32) -> Result<(), BoostError> {
    // SAFETY: these SET actions take the value in uiParam.
    unsafe { SystemParametersInfoW(action, v, None, SPI_FLAGS) }.map_err(map_win)
}

/* ---------------- services (SCM API, no sc.exe / net.exe) ---------------- */

struct Sc(SC_HANDLE);

impl Drop for Sc {
    fn drop(&mut self) {
        // SAFETY: handle opened by OpenSCManagerW/OpenServiceW, closed once.
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

fn open_scm() -> Result<Sc, BoostError> {
    // SAFETY: local machine, default database.
    unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) }.map(Sc).map_err(map_win)
}

/// `Ok(None)` when the service is not installed on this machine.
fn open_svc(scm: &Sc, name: &str, access: u32) -> Result<Option<Sc>, BoostError> {
    let w = wide(name);
    // SAFETY: valid SCM handle and null-terminated name.
    match unsafe { OpenServiceW(scm.0, PCWSTR(w.as_ptr()), access) } {
        Ok(h) => Ok(Some(Sc(h))),
        Err(e) if e.code() == E_SERVICE_DOES_NOT_EXIST => Ok(None),
        Err(e) => Err(map_win(e)),
    }
}

/// (start type, running) or `None` if the service does not exist. Works without admin.
pub fn svc_query(name: &str) -> Result<Option<(u32, bool)>, BoostError> {
    let scm = open_scm()?;
    let Some(h) = open_svc(&scm, name, SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS)? else {
        return Ok(None);
    };
    let mut needed = 0u32;
    // SAFETY: size probe; the call fails with ERROR_INSUFFICIENT_BUFFER and sets `needed`.
    let _ = unsafe { QueryServiceConfigW(h.0, None, 0, &mut needed) };
    let mut buf = vec![0u64; (needed as usize).div_ceil(8) + 1];
    // SAFETY: buffer is `needed` bytes (rounded up) and 8-byte aligned for QUERY_SERVICE_CONFIGW.
    unsafe { QueryServiceConfigW(h.0, Some(buf.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>()), needed, &mut needed) }
        .map_err(map_win)?;
    // SAFETY: the call filled a QUERY_SERVICE_CONFIGW at the start of the buffer.
    let start = unsafe { (*buf.as_ptr().cast::<QUERY_SERVICE_CONFIGW>()).dwStartType.0 };

    let mut status = [0u8; size_of::<SERVICE_STATUS_PROCESS>()];
    let mut needed = 0u32;
    // SAFETY: buffer sized for SERVICE_STATUS_PROCESS.
    unsafe { QueryServiceStatusEx(h.0, SC_STATUS_PROCESS_INFO, Some(&mut status), &mut needed) }.map_err(map_win)?;
    // SAFETY: filled by the call above.
    let state = unsafe { std::ptr::read_unaligned(status.as_ptr().cast::<SERVICE_STATUS_PROCESS>()).dwCurrentState };
    Ok(Some((start, state == SERVICE_RUNNING)))
}

fn svc_set_start(name: &str, start: u32) -> Result<(), BoostError> {
    let scm = open_scm()?;
    let Some(h) = open_svc(&scm, name, SERVICE_CHANGE_CONFIG)? else {
        return Ok(());
    };
    // SAFETY: SERVICE_NO_CHANGE keeps every other field; null strings mean "unchanged".
    unsafe {
        ChangeServiceConfigW(
            h.0,
            ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
            SERVICE_START_TYPE(start),
            SERVICE_ERROR(SERVICE_NO_CHANGE),
            PCWSTR::null(),
            PCWSTR::null(),
            None,
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
            PCWSTR::null(),
        )
    }
    .map_err(map_win)
}

fn svc_stop(name: &str) -> Result<(), BoostError> {
    let scm = open_scm()?;
    let Some(h) = open_svc(&scm, name, SERVICE_STOP)? else {
        return Ok(());
    };
    let mut st = SERVICE_STATUS::default();
    // SAFETY: valid handle; status out-param is a plain struct.
    match unsafe { ControlService(h.0, SERVICE_CONTROL_STOP, &mut st) } {
        Ok(()) => Ok(()),
        // 1062 = not active, 1052 = control not accepted (already stopping): both are fine.
        Err(e) if matches!(e.code().0 as u32, 0x8007_0426 | 0x8007_041C) => Ok(()),
        Err(e) => Err(map_win(e)),
    }
}

fn svc_start(name: &str) -> Result<(), BoostError> {
    let scm = open_scm()?;
    let Some(h) = open_svc(&scm, name, SERVICE_START)? else {
        return Ok(());
    };
    // SAFETY: valid handle, no arguments.
    match unsafe { StartServiceW(h.0, None) } {
        Ok(()) => Ok(()),
        // 1056 = already running.
        Err(e) if e.code().0 as u32 == 0x8007_0420 => Ok(()),
        Err(e) => Err(map_win(e)),
    }
}

/* ---------------- power schemes ---------------- */

const ULTIMATE: GUID = GUID::from_u128(0xe9a42b02_d5df_448d_aa00_03f14749eb61);
const BALANCED: GUID = GUID::from_u128(0x381b4222_f694_41f0_9685_ff5bb260df2e);

fn take_local_guid(p: *mut GUID) -> Option<GUID> {
    if p.is_null() {
        return None;
    }
    // SAFETY: `p` was LocalAlloc'd by the power API and is freed exactly once here.
    unsafe {
        let g = *p;
        let _ = LocalFree(Some(HLOCAL(p.cast::<c_void>())));
        Some(g)
    }
}

pub fn active_scheme() -> Result<GUID, BoostError> {
    let mut p: *mut GUID = null_mut();
    // SAFETY: out-param receives a LocalAlloc'd GUID.
    check(unsafe { PowerGetActiveScheme(None, &mut p) })?;
    take_local_guid(p).ok_or_else(|| BoostError::Win32("no active scheme".into()))
}

pub fn friendly_name(g: &GUID) -> String {
    let mut size = 0u32;
    // SAFETY: size probe with a null buffer.
    let _ = unsafe { PowerReadFriendlyName(None, Some(g), None, None, None, &mut size) };
    if size == 0 {
        return String::new();
    }
    let mut buf = vec![0u8; size as usize];
    // SAFETY: buffer of `size` bytes receives a null-terminated UTF-16 string.
    if unsafe { PowerReadFriendlyName(None, Some(g), None, None, Some(buf.as_mut_ptr()), &mut size) } != ERROR_SUCCESS {
        return String::new();
    }
    registry::utf16z(&buf)
}

fn set_active_scheme(g: &GUID) -> Result<(), BoostError> {
    // SAFETY: valid GUID pointer.
    check(unsafe { PowerSetActiveScheme(None, Some(g)) })
}

fn enumerate_schemes() -> Vec<GUID> {
    let mut out = Vec::new();
    for i in 0.. {
        let mut g = GUID::zeroed();
        let mut size = size_of::<GUID>() as u32;
        // SAFETY: 16-byte buffer for one GUID per index.
        let e = unsafe { PowerEnumerate(None, None, None, ACCESS_SCHEME, i, Some((&mut g as *mut GUID).cast::<u8>()), &mut size) };
        if e == ERROR_NO_MORE_ITEMS || e != ERROR_SUCCESS {
            break;
        }
        out.push(g);
    }
    out
}

/// A visible scheme named like the hidden "Ultimate Performance" template, duplicated if missing.
fn ensure_ultimate_scheme() -> Result<GUID, BoostError> {
    let name = friendly_name(&ULTIMATE);
    if let Some(g) = enumerate_schemes().into_iter().find(|g| friendly_name(g) == name) {
        return Ok(g);
    }
    let mut p: *mut GUID = null_mut();
    // SAFETY: duplicates the template into a new LocalAlloc'd GUID.
    check(unsafe { PowerDuplicateScheme(None, &ULTIMATE, &mut p) })?;
    take_local_guid(p).ok_or_else(|| BoostError::Win32("duplicate scheme".into()))
}

/* ---------------- memory: purge the standby list ---------------- */

// `NtSetSystemInformation` is not in the `windows` crate; ntdll exports it.
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtSetSystemInformation(class: i32, info: *mut c_void, len: u32) -> NTSTATUS;
}

const SYSTEM_MEMORY_LIST_INFORMATION: i32 = 80;
const MEMORY_PURGE_STANDBY_LIST: i32 = 4;

#[derive(Debug, Clone, Serialize)]
pub struct PurgeResult {
    pub before_avail: u64,
    pub after_avail: u64,
    pub freed_bytes: u64,
}

fn enable_privilege(name: &str) -> Result<(), BoostError> {
    // SAFETY: standard token privilege dance; every handle is closed.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token).map_err(map_win)?;
        let mut luid = LUID::default();
        let w = wide(name);
        if let Err(e) = LookupPrivilegeValueW(PCWSTR::null(), PCWSTR(w.as_ptr()), &mut luid) {
            let _ = CloseHandle(token);
            return Err(map_win(e));
        }
        let tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }],
        };
        let r = AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None);
        let last = GetLastError();
        let _ = CloseHandle(token);
        r.map_err(map_win)?;
        if last == ERROR_NOT_ALL_ASSIGNED {
            return Err(BoostError::NeedsAdmin);
        }
        Ok(())
    }
}

/// Drop the standby (cached) list so games get free pages immediately. Admin only.
/// Never trims working sets (that measurably hurts, see legacy scripts).
pub fn purge_standby_list() -> Result<PurgeResult, BoostError> {
    if !system::is_elevated() {
        return Err(BoostError::NeedsAdmin);
    }
    enable_privilege("SeProfileSingleProcessPrivilege")?;
    let before = system::memory_status().map(|m| m.avail_bytes).unwrap_or(0);
    let mut cmd = MEMORY_PURGE_STANDBY_LIST;
    // SAFETY: SystemMemoryListInformation takes a 4-byte SYSTEM_MEMORY_LIST_COMMAND.
    let status = unsafe { NtSetSystemInformation(SYSTEM_MEMORY_LIST_INFORMATION, (&mut cmd as *mut i32).cast::<c_void>(), 4) };
    if status.is_err() {
        return Err(BoostError::Win32(format!("NTSTATUS {:#x}", status.0)));
    }
    std::thread::sleep(std::time::Duration::from_millis(250));
    let after = system::memory_status().map(|m| m.avail_bytes).unwrap_or(before);
    Ok(PurgeResult { before_avail: before, after_avail: after, freed_bytes: after.saturating_sub(before) })
}

/* ---------------- elevation ---------------- */

/// Start a second, elevated copy of this exe through the UAC prompt (`runas` verb),
/// passing `args` through. The caller exits afterwards. Never PowerShell.
pub fn relaunch_elevated(args: &[&str]) -> Result<(), BoostError> {
    let exe = std::env::current_exe()?;
    let exe_w = wide(&exe.to_string_lossy());
    let params_w = wide(&args.join(" "));
    let verb_w = wide("runas");
    // SAFETY: zeroed struct with cbSize set; all strings outlive the call.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOASYNC;
        info.lpVerb = PCWSTR(verb_w.as_ptr());
        info.lpFile = PCWSTR(exe_w.as_ptr());
        info.lpParameters = PCWSTR(params_w.as_ptr());
        info.nShow = SW_SHOWNORMAL.0;
        ShellExecuteExW(&mut info).map_err(map_win)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PATH: &str = r"Software\G2 Boost\Test\boost";
    const TEST_OPS: &[RegOp] = &[
        op(HKCU, TEST_PATH, "alpha", Desired::Dword(1), Desired::Dword(0)),
        op(HKCU, TEST_PATH, "beta", Desired::Str("on"), Desired::Absent),
    ];
    const TEST_TWEAK: Tweak = tw("unit_test_tweak", false, false, Performance, Kind::Registry(TEST_OPS));

    #[test]
    fn dx_flags_keep_other_keys() {
        let cur = "AutoHDREnable=1;SwapEffectUpgradeEnable=0;VRROptimizeEnable=0;";
        let on = dx_set(cur, "SwapEffectUpgradeEnable", Some("1"));
        assert_eq!(on, "AutoHDREnable=1;VRROptimizeEnable=0;SwapEffectUpgradeEnable=1;");
        assert_eq!(dx_get(&on, "SwapEffectUpgradeEnable"), Some("1"));
        assert_eq!(dx_get(&on, "AutoHDREnable"), Some("1"));
        assert_eq!(dx_set(&on, "SwapEffectUpgradeEnable", None), "AutoHDREnable=1;VRROptimizeEnable=0;");
        assert_eq!(dx_set("", "SwapEffectUpgradeEnable", None), "");
        assert_eq!(dx_set("", "SwapEffectUpgradeEnable", Some("1")), "SwapEffectUpgradeEnable=1;");
        assert_eq!(dx_get("SwapEffectUpgradeEnableX=1;", "SwapEffectUpgradeEnable"), None);
    }

    #[test]
    fn detect_all_reports_every_tweak_without_admin() {
        let states = detect_all();
        assert_eq!(states.len(), TWEAKS.len());
        for s in &states {
            assert!(registry::safe_id(s.id), "{}", s.id);
        }
        // Services are queryable without admin.
        let sysmain = states.iter().find(|s| s.id == "svc_sysmain").unwrap();
        assert!(matches!(sysmain.status, Status::On | Status::Off | Status::Partial));
        assert_eq!(sysmain.services.len(), 1);
        // Power plan name is readable without admin.
        let power = states.iter().find(|s| s.id == "power_plan").unwrap();
        assert!(power.detail.as_deref().is_some_and(|n| !n.is_empty()));
        assert!(find("../x").is_err());
    }

    #[test]
    fn apply_backs_up_then_revert_restores_exact_values() {
        // SAFETY: same override every data-dir test in this crate uses (process-wide env var).
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
        // Pre-existing state: alpha = 7, beta absent.
        registry::write(HKCU, TEST_PATH, "alpha", &Value::Dword(7)).unwrap();
        registry::write(HKCU, TEST_PATH, "beta", &Value::Absent).unwrap();
        registry::delete_backup(TEST_TWEAK.id);
        assert_eq!(detect(&TEST_TWEAK).status, Status::Off);

        ensure_backup(&TEST_TWEAK).unwrap();
        apply_on(&TEST_TWEAK).unwrap();
        assert_eq!(detect(&TEST_TWEAK).status, Status::On);
        assert_eq!(registry::read(HKCU, TEST_PATH, "beta").unwrap(), Value::Str("on".into()));
        let b = registry::load_backup(TEST_TWEAK.id).expect("backup written");
        assert_eq!(b.entries[0].value, Value::Dword(7));
        assert_eq!(b.entries[1].value, Value::Absent);

        // A second apply must not overwrite the original backup.
        registry::write(HKCU, TEST_PATH, "alpha", &Value::Dword(99)).unwrap();
        ensure_backup(&TEST_TWEAK).unwrap();
        assert_eq!(registry::load_backup(TEST_TWEAK.id).unwrap().entries[0].value, Value::Dword(7));

        restore_entries(&b.entries).unwrap();
        registry::delete_backup(TEST_TWEAK.id);
        assert_eq!(registry::read(HKCU, TEST_PATH, "alpha").unwrap(), Value::Dword(7));
        assert_eq!(registry::read(HKCU, TEST_PATH, "beta").unwrap(), Value::Absent);
        assert!(registry::load_backup(TEST_TWEAK.id).is_none());

        // Partial detection.
        registry::write(HKCU, TEST_PATH, "alpha", &Value::Dword(1)).unwrap();
        assert_eq!(detect(&TEST_TWEAK).status, Status::Partial);
        apply_default(&TEST_TWEAK).unwrap();
        assert_eq!(registry::read(HKCU, TEST_PATH, "alpha").unwrap(), Value::Dword(0));
    }

    #[test]
    fn admin_tweaks_refuse_without_elevation() {
        if system::is_elevated() {
            return; // nothing to assert when the test runner is elevated
        }
        assert!(matches!(apply("hags", true), Err(BoostError::NeedsAdmin)));
        assert!(matches!(purge_standby_list(), Err(BoostError::NeedsAdmin)));
        assert!(matches!(apply("nope", true), Err(BoostError::UnknownTweak)));
    }

    #[test]
    fn power_schemes_enumerate() {
        let schemes = enumerate_schemes();
        assert!(!schemes.is_empty());
        let active = active_scheme().unwrap();
        assert!(schemes.contains(&active));
        assert!(!friendly_name(&ULTIMATE).is_empty());
        assert_eq!(GUID::try_from(format!("{active:?}").as_str()).unwrap(), active);
    }
}
