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
    CloseHandle, ERROR_NOT_ALL_ASSIGNED, ERROR_SUCCESS, GetLastError, HANDLE, HLOCAL, LPARAM, LUID, LocalFree,
    NTSTATUS, WIN32_ERROR, WPARAM,
};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, IF_TYPE_IEEE80211, MIB_IF_TABLE2};
use windows::Win32::System::Power::{
    ACCESS_SCHEME, PowerDuplicateScheme, PowerEnumerate, PowerGetActiveScheme, PowerReadACDefaultIndex,
    PowerReadACValueIndex, PowerReadDCDefaultIndex, PowerReadDCValueIndex, PowerReadFriendlyName, PowerReadValueMax,
    PowerReadValueMin, PowerSetActiveScheme, PowerWriteACValueIndex, PowerWriteDCValueIndex,
};
use windows::Win32::System::Services::{
    ChangeServiceConfigW, CloseServiceHandle, ControlService, ENUM_SERVICE_TYPE, OpenSCManagerW,
    OpenServiceW, QUERY_SERVICE_CONFIGW, QueryServiceConfigW, QueryServiceStatusEx, SC_HANDLE,
    SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO, SERVICE_AUTO_START, SERVICE_CHANGE_CONFIG,
    SERVICE_CONTROL_STOP, SERVICE_DEMAND_START, SERVICE_DISABLED, SERVICE_ERROR, SERVICE_NO_CHANGE,
    SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START, SERVICE_START_TYPE,
    SERVICE_STATUS, SERVICE_STATUS_PROCESS, SERVICE_STOP, StartServiceW, DeleteService,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::{SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_BROADCAST, SMTO_ABORTIFHUNG, SPI_GETFILTERKEYS, SPI_GETKEYBOARDDELAY, SPI_GETKEYBOARDSPEED, SPI_GETMOUSE,
    SPI_GETSTICKYKEYS, SPI_GETTOGGLEKEYS, SPI_SETFILTERKEYS, SPI_SETKEYBOARDDELAY, SPI_SETKEYBOARDSPEED,
    SPI_SETLANGTOGGLE, SPI_SETMOUSE, SPI_SETSTICKYKEYS, SPI_SETTOGGLEKEYS, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE,
    SW_SHOWNORMAL, SYSTEM_PARAMETERS_INFO_ACTION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SendMessageTimeoutW,
    SystemParametersInfoW, WM_SETTINGCHANGE,
};
use windows::core::{GUID, HRESULT, PCWSTR};

use crate::registry::{self, Backup, BackupEntry, Root, Value, wide};
use crate::system;
use crate::tasksched;
use crate::visualfx::{self, Level};
use crate::winfeature;

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
    /// Denied even with administrator: Windows' own protection of some settings (UCPD, e.g. the
    /// Widgets policy on 24H2+) or an antivirus. Nothing stays changed (partial writes go back).
    #[error("blocked")]
    Blocked,
    #[error("{0}")]
    Registry(#[from] registry::RegError),
    #[error("{0}")]
    VisualFx(#[from] visualfx::FxError),
    #[error("{0}")]
    Task(#[from] tasksched::TaskError),
    #[error("{0}")]
    Feature(#[from] winfeature::FeatureError),
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

#[derive(Debug, Clone, Copy, PartialEq)]
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
    /// The value missing means the same as `on` (Windows' own default is already "on", e.g. Game Mode).
    absent_is_on: bool,
    /// Non-zero: "on" is any DWORD with this bit set (Windows also writes its own variants), not only `on`.
    bit: u32,
}

#[derive(Debug, Clone, Copy)]
struct Svc {
    name: &'static str,
    /// Windows default start type (SERVICE_AUTO_START = 2, SERVICE_DEMAND_START = 3).
    default_start: u32,
    /// Start type the tweak sets: Disabled (4), or Manual (3) for "start only when asked".
    /// A service already stricter (Disabled when the target is Manual) is left alone and counts as on.
    target: u32,
}

/// One power setting on the active plan (`None` = that side is not touched).
#[derive(Debug, Clone, Copy)]
struct PowerOp {
    subgroup: GUID,
    setting: GUID,
    ac: Option<u32>,
    dc: Option<u32>,
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
    /// A Windows optional feature turned off through DismApi (name must be in `winfeature::ALLOWED`).
    Feature(&'static str),
    /// Settings of the active power plan.
    PowerSetting(&'static [PowerOp]),
    /// The Sticky / Filter / Toggle Keys shortcuts (Shift ×5, right Shift 8 s, Num Lock 5 s) off;
    /// the features themselves stay as the user set them.
    AccessKeys,
    /// HKCU key `key\sub` with an empty default value, present only while the tweak is on
    /// (Windows 11's classic context menu is the empty `InprocServer32` of a CLSID).
    EmptyKey { key: &'static str, sub: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Performance,
    Privacy,
    Appearance,
    Network,
    Gaming,
    Ai,
    System,
}

#[derive(Debug, Clone, Copy)]
pub struct Tweak {
    pub id: &'static str,
    pub admin: bool,
    pub beta: bool,
    pub category: Category,
    kind: Kind,
    /// Only on PCs where this holds (Windows 11, Office installed, a Wi-Fi adapter); otherwise
    /// the tweak is "unavailable" and nothing is shown as pending.
    gate: Option<fn() -> bool>,
}

impl Tweak {
    const fn gated(self, gate: fn() -> bool) -> Tweak {
        Tweak { gate: Some(gate), ..self }
    }
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

pub(crate) const DX_PREFS: &str = r"Software\Microsoft\DirectX\UserGpuPreferences";
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
    RegOp { root, path, name, on, off, absent_is_on: false, bit: 0 }
}

/// A value whose absence already means "on" in Windows (its default).
const fn op_default_on(root: Root, path: &'static str, name: &'static str, on: Desired, off: Desired) -> RegOp {
    RegOp { root, path, name, on, off, absent_is_on: true, bit: 0 }
}

/// A DWORD that counts as "on" whenever `bit` is set (writes `on`).
const fn op_bit(root: Root, path: &'static str, name: &'static str, on: u32, bit: u32, off: u32) -> RegOp {
    RegOp { root, path, name, on: Desired::Dword(on), off: Desired::Dword(off), absent_is_on: false, bit }
}

/// Policy value: set while the tweak is on, deleted otherwise (no policy = Windows default).
const fn policy(root: Root, path: &'static str, name: &'static str, on: u32) -> RegOp {
    op(root, path, name, Desired::Dword(on), Desired::Absent)
}

impl RegOp {
    fn is_on(&self, v: &Value) -> bool {
        match (self.bit, v) {
            (0, v) => *v == self.on.value(),
            (bit, Value::Dword(d)) => d & bit != 0,
            _ => false,
        }
    }
}

impl Svc {
    /// Start type already at (or stricter than) the target: Auto 2 < Manual 3 < Disabled 4.
    fn reached(&self, start: u32) -> bool {
        (self.target..=DISABLED).contains(&start)
    }
}

const GAME_MODE: &[RegOp] = &[
    // Game Mode is on by default in Windows 10/11: missing values mean "on", only an explicit 0 is off.
    op_default_on(HKCU, GAME_BAR, "AutoGameModeEnabled", Desired::Dword(1), Desired::Absent),
    op_default_on(HKCU, GAME_BAR, "AllowAutoGameMode", Desired::Dword(1), Desired::Absent),
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
/// The idle-time WinSAT run (Meu PC still runs WinSAT on demand through its COM API).
const TASKS_WINSAT: &[(&str, &str)] = &[(r"\Microsoft\Windows\Maintenance", "WinSAT")];
/// Reports sent to Microsoft: disk diagnostics, feedback prompts (Siuf) and queued error reports.
/// Never ScheduledDefrag (SSD TRIM), UpdateOrchestrator or Defender tasks.
const TASKS_FEEDBACK: &[(&str, &str)] = &[
    (r"\Microsoft\Windows\DiskDiagnostic", "Microsoft-Windows-DiskDiagnosticDataCollector"),
    (r"\Microsoft\Windows\Feedback\Siuf", "DmClient"),
    (r"\Microsoft\Windows\Feedback\Siuf", "DmClientOnScenarioDownload"),
    (r"\Microsoft\Windows\Windows Error Reporting", "QueueReporting"),
];
/* ---- round 9: debloat (sources: Docs/references/debloat-sources.md) ---- */

const EDGE: &str = r"SOFTWARE\Policies\Microsoft\Edge";
const EDGE_BACKGROUND_OFF: &[RegOp] = &[policy(HKLM, EDGE, "StartupBoostEnabled", 0), policy(HKLM, EDGE, "BackgroundModeEnabled", 0)];
const EDGE_AI_OFF: &[RegOp] = &[
    policy(HKLM, EDGE, "HubsSidebarEnabled", 0),
    policy(HKLM, EDGE, "CopilotPageContext", 0),
    policy(HKLM, EDGE, "ComposeInlineEnabled", 0),
    policy(HKLM, EDGE, "NewTabPageBingChatEnabled", 0),
    // 1 = do not download the on-device AI model.
    policy(HKLM, EDGE, "GenAILocalFoundationalModelSettings", 1),
];
/// Windows 11 Widgets (`Dsh`) and Windows 10 News and interests (`Windows Feeds`).
const WIDGETS_POLICY_OFF: &[RegOp] = &[
    policy(HKLM, r"SOFTWARE\Policies\Microsoft\Dsh", "AllowNewsAndInterests", 0),
    policy(HKLM, r"SOFTWARE\Policies\Microsoft\Windows\Windows Feeds", "EnableFeeds", 0),
];
const TEAMS_CHAT_OFF: &[RegOp] = &[op(HKCU, EXPLORER_ADV, "TaskbarMn", Desired::Dword(0), Desired::Dword(1))];
const CLOUD_CONTENT: &str = r"SOFTWARE\Policies\Microsoft\Windows\CloudContent";
const CLOUD_CONTENT_USER: &str = r"Software\Policies\Microsoft\Windows\CloudContent";
/// Fully honored on Enterprise/Education; Home/Pro honor part (the HKCU `silent_apps_off` covers them).
const CONSUMER_FEATURES_OFF: &[RegOp] = &[
    policy(HKLM, CLOUD_CONTENT, "DisableWindowsConsumerFeatures", 1),
    policy(HKLM, CLOUD_CONTENT, "DisableConsumerAccountStateContent", 1),
    policy(HKLM, CLOUD_CONTENT, "DisableCloudOptimizedContent", 1),
];
/// 0 = the lowest level the edition accepts (Security on Enterprise/Education, Required on Home/Pro).
const TELEMETRY_MIN: &[RegOp] = &[policy(HKLM, r"SOFTWARE\Policies\Microsoft\Windows\DataCollection", "AllowTelemetry", 0)];
/// The policy is per user only (Policy CSP: no device scope); the Store Copilot app ignores it.
const COPILOT_OFF: &[RegOp] = &[
    policy(HKCU, r"Software\Policies\Microsoft\Windows\WindowsCopilot", "TurnOffWindowsCopilot", 1),
    policy(HKCU, EXPLORER_ADV, "ShowCopilotButton", 0),
];
const WINDOWS_AI: &str = r"SOFTWARE\Policies\Microsoft\Windows\WindowsAI";
const WINDOWS_AI_USER: &str = r"Software\Policies\Microsoft\Windows\WindowsAI";
const RECALL_OFF: &[RegOp] = &[policy(HKCU, WINDOWS_AI_USER, "DisableAIDataAnalysis", 1), policy(HKLM, WINDOWS_AI, "DisableAIDataAnalysis", 1)];
const CLICK_TO_DO_OFF: &[RegOp] = &[policy(HKCU, WINDOWS_AI_USER, "DisableClickToDo", 1), policy(HKLM, WINDOWS_AI, "DisableClickToDo", 1)];
const PAINT: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Paint";
const PAINT_AI_OFF: &[RegOp] = &[
    policy(HKLM, PAINT, "DisableCocreator", 1),
    policy(HKLM, PAINT, "DisableGenerativeFill", 1),
    policy(HKLM, PAINT, "DisableImageCreator", 1),
];
const NOTEPAD_AI_OFF: &[RegOp] = &[policy(HKLM, r"SOFTWARE\Policies\WindowsNotepad", "DisableAIFeatures", 1)];
/// 0 = HTTP only: updates still download from Microsoft, nothing is shared with other PCs.
const DELIVERY_P2P_OFF: &[RegOp] = &[policy(HKLM, r"SOFTWARE\Policies\Microsoft\Windows\DeliveryOptimization", "DODownloadMode", 0)];
const WINDOWS_SEARCH: &str = r"SOFTWARE\Policies\Microsoft\Windows\Windows Search";
const START_WEB_SEARCH_OFF: &[RegOp] = &[
    policy(HKCU, r"Software\Policies\Microsoft\Windows\Explorer", "DisableSearchBoxSuggestions", 1),
    policy(HKLM, WINDOWS_SEARCH, "DisableWebSearch", 1),
    policy(HKLM, WINDOWS_SEARCH, "ConnectedSearchUseWeb", 0),
];
const ACCOUNT_NOTIFICATIONS_OFF: &[RegOp] = &[
    policy(HKCU, EXPLORER_ADV, "Start_AccountNotifications", 0),
    policy(HKCU, EXPLORER_ADV, "Start_IrisRecommendations", 0),
    policy(HKCU, r"Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement", "ScoobeSystemSettingEnabled", 0),
];
/// The HKCU values work on every edition; the two policies only on Enterprise/Education.
const SPOTLIGHT_OFF: &[RegOp] = &[
    op(HKCU, CDM, "RotatingLockScreenEnabled", Desired::Dword(0), Desired::Dword(1)),
    op(HKCU, CDM, "RotatingLockScreenOverlayEnabled", Desired::Dword(0), Desired::Dword(1)),
    op(HKCU, CDM, "SubscribedContent-338387Enabled", Desired::Dword(0), Desired::Dword(1)),
    policy(HKCU, CLOUD_CONTENT_USER, "DisableWindowsSpotlightFeatures", 1),
    policy(HKCU, CLOUD_CONTENT_USER, "DisableSpotlightCollectionOnDesktop", 1),
];
/// Microsoft 365 Apps diagnostic data level: 3 = Neither (required service data still flows).
const OFFICE_TELEMETRY_OFF: &[RegOp] =
    &[policy(HKCU, r"Software\Policies\Microsoft\Office\Common\ClientTelemetry", "SendTelemetry", 3)];
/// NVIDIA's telemetry reporters, in the root task folder (older drivers; recent ones no longer
/// install them, then the tweak shows as unavailable).
const TASKS_NVIDIA_TELEMETRY: &[(&str, &str)] = &[
    (r"\", "NvTmMon_{B2FE1952-0186-46C3-BAEC-A80AA35AC5B8}"),
    (r"\", "NvTmRep_{B2FE1952-0186-46C3-BAEC-A80AA35AC5B8}"),
    (r"\", "NvTmRepOnLogon_{B2FE1952-0186-46C3-BAEC-A80AA35AC5B8}"),
    (r"\", "NvTmRep_CrashReport1_{B2FE1952-0186-46C3-BAEC-A80AA35AC5B8}"),
    (r"\", "NvTmRep_CrashReport2_{B2FE1952-0186-46C3-BAEC-A80AA35AC5B8}"),
    (r"\", "NvTmRep_CrashReport3_{B2FE1952-0186-46C3-BAEC-A80AA35AC5B8}"),
    (r"\", "NvTmRep_CrashReport4_{B2FE1952-0186-46C3-BAEC-A80AA35AC5B8}"),
];

/* ---- round 9: Windows ---- */

/// 0x80000001 = last-access updates off, set by the user. Windows' own "system managed, off"
/// (0x80000003) has the same low bit and counts as on.
const NTFS_LAST_ACCESS_OFF: &[RegOp] =
    &[op_bit(HKLM, r"SYSTEM\CurrentControlSet\Control\FileSystem", "NtfsDisableLastAccessUpdate", 0x8000_0001, 1, 0x8000_0002)];
/// `01` missing = Storage Sense never turned on.
const STORAGE_SENSE_OFF: &[RegOp] = &[op_default_on(
    HKCU,
    r"Software\Microsoft\Windows\CurrentVersion\StorageSense\Parameters\StoragePolicy",
    "01",
    Desired::Dword(0),
    Desired::Absent,
)];
const FAST_STARTUP_OFF: &[RegOp] =
    &[op(HKLM, r"SYSTEM\CurrentControlSet\Control\Session Manager\Power", "HiberbootEnabled", Desired::Dword(0), Desired::Dword(1))];
const NETWORK_THROTTLING_OFF: &[RegOp] =
    &[op(HKLM, MM_PROFILE, "NetworkThrottlingIndex", Desired::Dword(0xFFFF_FFFF), Desired::Dword(10))];
const TRANSPARENCY_OFF: &[RegOp] = &[op(
    HKCU,
    r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
    "EnableTransparency",
    Desired::Dword(0),
    Desired::Dword(1),
)];
const TASKBAR_LEFT: &[RegOp] = &[policy(HKCU, EXPLORER_ADV, "TaskbarAl", 0)];
const NOTIFICATIONS_OFF: &[RegOp] =
    &[op(HKCU, r"Software\Microsoft\Windows\CurrentVersion\PushNotifications", "ToastEnabled", Desired::Dword(0), Desired::Dword(1))];
const ERROR_REPORTING_OFF: &[RegOp] = &[policy(HKLM, r"SOFTWARE\Microsoft\Windows\Windows Error Reporting", "Disabled", 1)];
/// "3" = no shortcut for switching input languages ("1" = left Alt+Shift, Windows' default).
const LANG_TOGGLE: &str = r"Keyboard Layout\Toggle";
const ALT_SHIFT_OFF: &[RegOp] = &[
    op(HKCU, LANG_TOGGLE, "Language Hotkey", Desired::Str("3"), Desired::Str("1")),
    op(HKCU, LANG_TOGGLE, "Hotkey", Desired::Str("3"), Desired::Str("1")),
];
const NARRATOR_SHORTCUT_OFF: &[RegOp] = &[policy(HKCU, r"Software\Microsoft\Narrator\NoRoam", "WinEnterLaunchEnabled", 0)];
const CLASSIC_MENU_KEY: &str = r"Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}";

const SUB_WIFI: GUID = GUID::from_u128(0x19cbb8fa_5279_450e_9fac_8a3d5fedd0c1);
const SUB_PROCESSOR: GUID = GUID::from_u128(0x54533251_82be_4824_96c1_47b60b740d00);
/// Wireless adapter "Power saving mode": 0 = maximum performance (plugged in and on battery).
const WIFI_POWER_SAVING_OFF: &[PowerOp] =
    &[PowerOp { subgroup: SUB_WIFI, setting: GUID::from_u128(0x12bbebe6_58d6_4636_95bb_3217ef867c1a), ac: Some(0), dc: Some(0) }];
/// Processor `CPMINCORES` = 100 % plugged in: no parked cores. Battery left as it is.
const CORE_PARKING_OFF: &[PowerOp] =
    &[PowerOp { subgroup: SUB_PROCESSOR, setting: GUID::from_u128(0x0cc5b647_c1df_4637_891a_dec35c318583), ac: Some(100), dc: None }];

const TCP_OPS: [(&str, Desired, Desired); 3] = [
    ("TcpAckFrequency", Desired::Dword(1), Desired::Absent),
    ("TCPNoDelay", Desired::Dword(1), Desired::Absent),
    ("TcpDelAckTicks", Desired::Dword(0), Desired::Absent),
];

const AUTO: u32 = SERVICE_AUTO_START.0;
const MANUAL: u32 = SERVICE_DEMAND_START.0;
const DISABLED: u32 = SERVICE_DISABLED.0;

/// Service turned off (Disabled + stopped).
const fn off(name: &'static str, default_start: u32) -> Svc {
    Svc { name, default_start, target: DISABLED }
}

const SVC_SYSMAIN: &[Svc] = &[off("SysMain", AUTO)];
const SVC_TELEMETRY: &[Svc] = &[off("DiagTrack", AUTO), off("dmwappushservice", MANUAL)];
/// Gentler than `svc_telemetry`: DiagTrack no longer starts with Windows, but whatever needs it
/// can still start it.
const SVC_DIAGTRACK_MANUAL: &[Svc] = &[Svc { name: "DiagTrack", default_start: AUTO, target: MANUAL }];
const SVC_SEARCH: &[Svc] = &[off("WSearch", AUTO)];
const SVC_XBOX: &[Svc] =
    &[off("XblAuthManager", MANUAL), off("XblGameSave", MANUAL), off("XboxNetApiSvc", MANUAL), off("XboxGipSvc", MANUAL)];
const SVC_DPS: &[Svc] = &[off("DPS", AUTO)];
const SVC_GEOLOCATION: &[Svc] = &[off("lfsvc", MANUAL)];
/// Print Spooler: recommended off only on a PC without real printers (`recommend.rs`).
const SVC_SPOOLER: &[Svc] = &[off("Spooler", AUTO)];

const fn tw(id: &'static str, admin: bool, beta: bool, category: Category, kind: Kind) -> Tweak {
    Tweak { id, admin, beta, category, kind, gate: None }
}

/* ---- gates: where a tweak means something ---- */

fn build() -> u32 {
    match registry::read(HKLM, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "CurrentBuildNumber") {
        Ok(Value::Str(b)) => b.parse().unwrap_or(0),
        _ => 0,
    }
}

pub(crate) fn windows11() -> bool {
    build() >= 22_000
}

/// The taskbar Chat (Teams) button: Windows 11 21H2/22H2 only (23H2 removed it).
fn chat_button() -> bool {
    (22_000..22_631).contains(&build())
}

/// Microsoft 365 Apps / Office 2016+ (Click-to-Run or MSI).
pub(crate) fn office() -> bool {
    matches!(registry::read(HKLM, r"SOFTWARE\Microsoft\Office\ClickToRun\Configuration", "Platform"), Ok(Value::Str(_)))
        || matches!(registry::read(HKLM, r"SOFTWARE\Microsoft\Office\16.0\Common\InstallRoot", "Path"), Ok(Value::Str(_)))
}

/// A Wi-Fi adapter (any IEEE 802.11 interface, connected or not).
pub(crate) fn wifi() -> bool {
    let mut table: *mut MIB_IF_TABLE2 = null_mut();
    // SAFETY: GetIfTable2 allocates the table; FreeMibTable releases it once.
    unsafe {
        if GetIfTable2(&mut table) != ERROR_SUCCESS || table.is_null() {
            return false;
        }
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        let found = rows.iter().any(|r| r.Type == IF_TYPE_IEEE80211);
        FreeMibTable(table.cast::<c_void>());
        found
    }
}

use Category::{Ai, Appearance, Gaming, Network, Performance, Privacy, System};

/// The catalog. Presets live in the UI (`src/data/catalogs.ts`); mechanics live here.
/// Evidence for the service choices: Docs/references/windows-services-research.md.
pub const TWEAKS: &[Tweak] = &[
    tw("visual_fx", false, false, Appearance, Kind::VisualFx),
    tw("game_mode", false, false, Gaming, Kind::Registry(GAME_MODE)),
    tw("input_lag", false, false, Gaming, Kind::Registry(INPUT_LAG)),
    tw("windowed_optimizations", false, false, Gaming, Kind::DxFlag("SwapEffectUpgradeEnable")),
    tw("keyboard", false, false, Gaming, Kind::Keyboard),
    tw("mouse", false, false, Gaming, Kind::Mouse),
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
    tw("hags", true, false, Gaming, Kind::Registry(HAGS)),
    tw("mmcss", true, false, Gaming, Kind::Registry(MMCSS)),
    tw("network_latency", true, false, Network, Kind::TcpInterfaces),
    tw("svc_search", true, false, Performance, Kind::Services(SVC_SEARCH)),
    tw("svc_xbox", true, false, Performance, Kind::Services(SVC_XBOX)),
    tw("svc_dps", true, false, Performance, Kind::Services(SVC_DPS)),
    tw("svc_geolocation", true, false, Privacy, Kind::Services(SVC_GEOLOCATION)),
    tw("hvci_off", true, true, Performance, Kind::Registry(HVCI_OFF)),
    tw("priority_sep", true, true, Gaming, Kind::Registry(PRIORITY_SEP)),
    tw("perf_counters", true, true, Performance, Kind::Registry(PERF_COUNTERS)),
    tw("winsat_task_off", true, false, Performance, Kind::Tasks(TASKS_WINSAT)),
    tw("feedback_tasks_off", true, false, Privacy, Kind::Tasks(TASKS_FEEDBACK)),
    tw("svc_spooler", true, false, Performance, Kind::Services(SVC_SPOOLER)),
    // Round 9: debloat (the Debloat page groups these by level; levels live in src/data/catalogs.ts).
    tw("edge_background_off", true, false, Performance, Kind::Registry(EDGE_BACKGROUND_OFF)),
    tw("widgets_policy_off", true, false, Appearance, Kind::Registry(WIDGETS_POLICY_OFF)),
    tw("teams_chat_off", false, false, Appearance, Kind::Registry(TEAMS_CHAT_OFF)).gated(chat_button),
    tw("consumer_features_off", true, false, Privacy, Kind::Registry(CONSUMER_FEATURES_OFF)),
    tw("telemetry_min", true, false, Privacy, Kind::Registry(TELEMETRY_MIN)),
    tw("diagtrack_manual", true, false, Privacy, Kind::Services(SVC_DIAGTRACK_MANUAL)),
    tw("copilot_off", true, false, Ai, Kind::Registry(COPILOT_OFF)),
    tw("recall_off", true, false, Ai, Kind::Registry(RECALL_OFF)),
    tw("click_to_do_off", true, false, Ai, Kind::Registry(CLICK_TO_DO_OFF)),
    tw("recall_feature_off", true, false, Ai, Kind::Feature("Recall")),
    tw("paint_ai_off", true, false, Ai, Kind::Registry(PAINT_AI_OFF)),
    tw("notepad_ai_off", true, false, Ai, Kind::Registry(NOTEPAD_AI_OFF)),
    tw("edge_ai_off", true, false, Ai, Kind::Registry(EDGE_AI_OFF)),
    tw("delivery_p2p_off", true, false, Network, Kind::Registry(DELIVERY_P2P_OFF)),
    tw("start_web_search_off", true, false, Privacy, Kind::Registry(START_WEB_SEARCH_OFF)),
    tw("account_notifications_off", false, false, Privacy, Kind::Registry(ACCOUNT_NOTIFICATIONS_OFF)),
    tw("spotlight_off", true, false, Privacy, Kind::Registry(SPOTLIGHT_OFF)),
    tw("office_telemetry_off", true, false, Privacy, Kind::Registry(OFFICE_TELEMETRY_OFF)).gated(office),
    tw("nvidia_telemetry_off", true, false, Privacy, Kind::Tasks(TASKS_NVIDIA_TELEMETRY)),
    // Round 9: Windows.
    tw("ntfs_last_access_off", true, false, Performance, Kind::Registry(NTFS_LAST_ACCESS_OFF)),
    tw("storage_sense_off", false, false, System, Kind::Registry(STORAGE_SENSE_OFF)),
    tw("fast_startup_off", true, false, System, Kind::Registry(FAST_STARTUP_OFF)),
    tw("network_throttling_off", true, false, Network, Kind::Registry(NETWORK_THROTTLING_OFF)),
    tw("wifi_power_saving_off", true, false, Network, Kind::PowerSetting(WIFI_POWER_SAVING_OFF)).gated(wifi),
    tw("core_parking_off", true, false, Performance, Kind::PowerSetting(CORE_PARKING_OFF)),
    tw("transparency_off", false, false, Appearance, Kind::Registry(TRANSPARENCY_OFF)),
    tw("classic_context_menu", false, false, Appearance, Kind::EmptyKey { key: CLASSIC_MENU_KEY, sub: "InprocServer32" })
        .gated(windows11),
    tw("taskbar_left", false, false, Appearance, Kind::Registry(TASKBAR_LEFT)).gated(windows11),
    tw("notifications_off", false, false, System, Kind::Registry(NOTIFICATIONS_OFF)),
    tw("error_reporting_off", true, false, Privacy, Kind::Registry(ERROR_REPORTING_OFF)),
    tw("alt_shift_off", false, false, Gaming, Kind::Registry(ALT_SHIFT_OFF)),
    tw("narrator_shortcut_off", false, false, Gaming, Kind::Registry(NARRATOR_SHORTCUT_OFF)),
    tw("accessibility_shortcuts_off", false, false, Gaming, Kind::AccessKeys),
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

/// An Ultimate Performance copy, whoever made it: our copy carries the template's localized name,
/// other tools name theirs "Ultimate Performance (…)".
fn is_ultimate_name(name: &str, template: &str) -> bool {
    let (n, t) = (name.to_lowercase(), template.to_lowercase());
    (!t.is_empty() && n.contains(&t)) || n.contains("ultimate performance")
}

/// Where the registry is only a request, show what Windows is running now.
/// `detail = "reboot_pending"` when the request and the running state differ.
fn live_state(id: &str, any_absent: bool, st: &mut TweakState) {
    let live = match id {
        "hags" => match crate::hardware::hags_state() {
            Some((false, _)) => {
                st.status = Status::Unavailable; // GPU/driver without HAGS
                return;
            }
            Some((true, enabled)) => Some(enabled),
            None => None,
        },
        // `hvci_off` is "on" when Memory Integrity is NOT running.
        "hvci_off" => crate::advisor::hvci_running().map(|running| !running),
        _ => return,
    };
    let Some(live_on) = live else {
        return;
    };
    let live_status = if live_on { Status::On } else { Status::Off };
    if any_absent {
        st.status = live_status; // never configured: the driver/OS default is the truth
    } else if st.status != live_status {
        st.detail = Some("reboot_pending".into());
    }
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
    if t.gate.is_some_and(|g| !g()) {
        st.detail = Some("not_present".into());
        return st;
    }
    match t.kind {
        Kind::Registry(ops) => {
            let (mut matches, mut total, mut any_absent) = (0, 0, false);
            for o in ops {
                // Same value in both states (MMCSS "GPU Priority" 8): says nothing either way.
                if o.on == o.off {
                    continue;
                }
                let v = match registry::read(o.root, o.path, o.name) {
                    Ok(v) => v,
                    Err(_) => return st,
                };
                any_absent |= v == Value::Absent;
                // Windows default already "on" and nothing set (or set on): neutral, like a stock PC.
                if o.absent_is_on && (v == Value::Absent || v == o.on.value()) {
                    continue;
                }
                total += 1;
                if o.is_on(&v) {
                    matches += 1;
                }
            }
            st.status = if total == 0 && ops.iter().any(|o| o.absent_is_on) { Status::On } else { status_of(matches, total) };
            live_state(t.id, any_absent, &mut st);
        }
        Kind::Services(list) => {
            let mut total = 0;
            let mut done = 0;
            for s in list {
                if let Ok(Some((start, running))) = svc_query(s.name) {
                    total += 1;
                    if s.reached(start) {
                        done += 1;
                    }
                    st.services.push(ServiceState { name: s.name.to_string(), start: start_label(start), running });
                }
            }
            st.status = status_of(done, total);
        }
        Kind::Feature(name) => match winfeature::state(name) {
            Ok(s) => {
                use winfeature::State as F;
                st.status = match s {
                    F::Disabled | F::DisablePending => Status::On,
                    F::Enabled | F::EnablePending => Status::Off,
                    F::Partial => Status::Partial,
                    F::Absent => Status::Unavailable,
                };
                if matches!(s, F::DisablePending | F::EnablePending) {
                    st.detail = Some("reboot_pending".into());
                }
            }
            // Reading needs administrator (or the servicing slot is busy): unknown, not "off".
            Err(e) => st.detail = Some(e.to_string()),
        },
        // Some PCs declare a range that excludes the target (core parking reads 0..0 on CPUs Windows
        // manages itself): writing would fail, so the tweak means nothing here.
        Kind::PowerSetting(ops) if ops.iter().any(|o| [o.ac, o.dc].into_iter().flatten().any(|v| !power_in_range(o, v))) => {
            st.status = Status::Unavailable;
        }
        Kind::PowerSetting(ops) => {
            if let Ok(scheme) = active_scheme() {
                let (mut matches, mut total) = (0, 0);
                for o in ops {
                    for (want, have) in [(o.ac, power_read(&scheme, o, true)), (o.dc, power_read(&scheme, o, false))] {
                        if let (Some(w), Some(h)) = (want, have) {
                            total += 1;
                            matches += usize::from(w == h);
                        }
                    }
                }
                st.status = status_of(matches, total);
            }
        }
        Kind::AccessKeys => {
            if let Ok(keys) = access_keys_get() {
                st.status = if keys.iter().all(|(_, flags)| flags & HOTKEY_ACTIVE == 0) { Status::On } else { Status::Off };
            }
        }
        Kind::EmptyKey { key, sub } => {
            if let Ok(v) = registry::read(HKCU, &format!(r"{key}\{sub}"), "") {
                st.status = match v {
                    Value::Str(s) if s.is_empty() => Status::On,
                    Value::Absent => Status::Off,
                    _ => Status::Partial,
                };
            }
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
                st.status = if is_ultimate_name(&name, &friendly_name(&ULTIMATE)) { Status::On } else { Status::Off };
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

/* ---------------- "Ver o que muda": read from the tweak's own definition ---------------- */

/// One thing turning a tweak on changes. `kind`: `registry` | `service` | `task` | `power` |
/// `plan` | `feature` | `system` | `visual_fx`. Technical names stay as Windows spells them.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Change {
    pub kind: &'static str,
    /// `HKLM\…\Name` (`@` = the key's default value), service or task name, power setting
    /// `subgroup\setting` GUIDs, optional feature name, or the system call.
    pub target: String,
    /// What it becomes (`DWORD` as a number, strings quoted, service start type); `None` = removed.
    pub value: Option<String>,
}

fn root_name(r: Root) -> &'static str {
    match r {
        Root::Hkcu => "HKCU",
        Root::Hklm => "HKLM",
    }
}

fn shown(d: Desired) -> Option<String> {
    match d {
        Desired::Dword(n) if n > 0xFFFF => Some(format!("{n:#010x}")),
        Desired::Dword(n) => Some(n.to_string()),
        Desired::Str(s) => Some(format!("\"{s}\"")),
        Desired::Absent => None,
    }
}

pub fn changes(id: &str) -> Result<Vec<Change>, BoostError> {
    let t = find(id)?;
    let reg = |root: Root, path: &str, name: &str, value: Option<String>| Change {
        kind: "registry",
        target: format!(r"{}\{path}\{}", root_name(root), if name.is_empty() { "@" } else { name }),
        value,
    };
    let sys = |target: &str, value: &str| Change { kind: "system", target: target.into(), value: Some(value.into()) };
    let guid = |g: &GUID| format!("{g:?}").to_ascii_lowercase();
    Ok(match t.kind {
        Kind::Registry(ops) => ops.iter().map(|o| reg(o.root, o.path, o.name, shown(o.on))).collect(),
        Kind::Services(list) => {
            list.iter().map(|s| Change { kind: "service", target: s.name.into(), value: Some(start_label(s.target).into()) }).collect()
        }
        Kind::Tasks(list) => list
            .iter()
            .map(|(f, n)| Change { kind: "task", target: format!(r"{}\{n}", f.trim_end_matches('\\')), value: Some("disabled".into()) })
            .collect(),
        Kind::TcpInterfaces => TCP_OPS.iter().map(|(n, on, _)| reg(HKLM, &format!(r"{TCP_INTERFACES}\{{*}}"), n, shown(*on))).collect(),
        Kind::Mouse => vec![sys("SPI_SETMOUSE", "0, 0, 0")],
        Kind::Keyboard => vec![sys("SPI_SETKEYBOARDDELAY", "0"), sys("SPI_SETKEYBOARDSPEED", "31")],
        Kind::Power => vec![Change { kind: "plan", target: guid(&ULTIMATE), value: None }],
        Kind::VisualFx => vec![Change {
            kind: "visual_fx",
            target: format!(r"HKCU\{}\{}", visualfx::VISUAL_FX_SETTING_PATH, visualfx::VISUAL_FX_SETTING_NAME),
            value: Some("3".into()),
        }],
        Kind::DxFlag(key) => vec![reg(HKCU, DX_PREFS, DX_GLOBAL, Some(format!("{key}=1;")))],
        Kind::Feature(name) => vec![Change { kind: "feature", target: name.into(), value: Some("disabled".into()) }],
        Kind::PowerSetting(ops) => ops
            .iter()
            .map(|o| {
                let sides: Vec<String> = [("AC", o.ac), ("DC", o.dc)].iter().filter_map(|(s, v)| v.map(|v| format!("{s} {v}"))).collect();
                Change { kind: "power", target: format!(r"{}\{}", guid(&o.subgroup), guid(&o.setting)), value: Some(sides.join(" · ")) }
            })
            .collect(),
        Kind::AccessKeys => vec![
            sys("SPI_SETSTICKYKEYS · SKF_HOTKEYACTIVE", "0"),
            sys("SPI_SETFILTERKEYS · FKF_HOTKEYACTIVE", "0"),
            sys("SPI_SETTOGGLEKEYS · TKF_HOTKEYACTIVE", "0"),
        ],
        Kind::EmptyKey { key, sub } => vec![reg(HKCU, &format!(r"{key}\{sub}"), "", Some("\"\"".into()))],
    })
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
    // The DirectX settings string is shared with Auto HDR / VRR; edit only this tweak's own key,
    // both on and off, instead of capturing/replaying the whole value.
    if let Kind::DxFlag(key) = t.kind {
        dx_write(key, on.then_some("1"))?;
        visualfx::broadcast_setting_change();
        return Ok(detect(t));
    }
    if on {
        ensure_backup(t)?;
        if let Err(e) = apply_on(t) {
            // One key of several can be denied: put back what was written so it's all or nothing.
            if let Some(b) = registry::load_backup(t.id) {
                let _ = restore_entries(t, &b.entries);
                registry::delete_backup(t.id);
            }
            return Err(match e {
                BoostError::Registry(registry::RegError::AccessDenied) if system::is_elevated() => BoostError::Blocked,
                other => other,
            });
        }
    } else if let Some(b) = registry::load_backup(t.id) {
        restore_entries(t, &b.entries)?;
        registry::delete_backup(t.id);
    } else {
        apply_default(t)?;
    }
    refresh(t.id);
    Ok(detect(t))
}

/// Nudge the shell so the change shows without signing out (what Settings does after a change).
fn refresh(id: &str) {
    match id {
        "taskbar_left" | "teams_chat_off" | "widgets_off" | "copilot_off" => broadcast(Some("TraySettings")),
        "transparency_off" => broadcast(Some("ImmersiveColorSet")),
        // Re-read `Keyboard Layout\Toggle` now instead of at the next sign-in.
        // SAFETY: SPI_SETLANGTOGGLE takes no parameters.
        "alt_shift_off" => unsafe {
            let _ = SystemParametersInfoW(SPI_SETLANGTOGGLE, 0, None, SPI_FLAGS);
        },
        _ => {}
    }
    // Explorer/shell settings (file extensions, widgets, search box) pick this up live.
    visualfx::broadcast_setting_change();
}

fn broadcast(area: Option<&str>) {
    let w = area.map(wide);
    let lparam = LPARAM(w.as_ref().map_or(0, |w| w.as_ptr() as isize));
    // SAFETY: broadcast with a bounded timeout; the string outlives the call.
    unsafe {
        let _ = SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, WPARAM(0), lparam, SMTO_ABORTIFHUNG, 1000, None);
    }
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
            restore_entries(t, &b.entries)?;
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
        Kind::Feature(name) => {
            let on = matches!(winfeature::state(name)?, winfeature::State::Enabled | winfeature::State::EnablePending);
            vec![entry(format!("feature:{name}"), "enabled", Value::Dword(on as u32))]
        }
        Kind::PowerSetting(ops) => {
            // The plan changed now: revert writes back to it even if the user switches plans later.
            let scheme = active_scheme()?;
            let mut out = vec![entry("powerset:scheme", "guid", Value::Str(format!("{scheme:?}")))];
            for o in ops {
                for (side, want) in [("ac", o.ac), ("dc", o.dc)] {
                    if want.is_some()
                        && let Some(v) = power_read(&scheme, o, side == "ac")
                    {
                        out.push(entry(power_path(o), side, Value::Dword(v)));
                    }
                }
            }
            out
        }
        Kind::AccessKeys => access_keys_get()?.iter().map(|(name, flags)| entry("spi:accesskeys", *name, Value::Dword(*flags))).collect(),
        Kind::EmptyKey { key, sub } => {
            let path = format!(r"{key}\{sub}");
            vec![
                // Did the key exist before? Revert deletes it only if we created it.
                entry(format!("regkey:{key}"), "existed", Value::Dword(key_exists(key)? as u32)),
                entry(path.clone(), "", registry::read(HKCU, &path, "")?),
            ]
        }
    })
}

/// Pseudo-path of one power setting in a backup: `powerset:{subgroup}|{setting}`.
fn power_path(o: &PowerOp) -> String {
    format!("powerset:{:?}|{:?}", o.subgroup, o.setting)
}

/// `key` exists under HKCU (an empty key and a missing one read the same through values).
fn key_exists(key: &str) -> Result<bool, BoostError> {
    let (parent, leaf) = key.rsplit_once('\\').ok_or_else(|| BoostError::Win32("bad key".into()))?;
    Ok(registry::subkeys(HKCU, parent)?.iter().any(|k| k.eq_ignore_ascii_case(leaf)))
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
                let Some((start, _)) = svc_query(s.name)? else {
                    continue;
                };
                // Never loosen: a service already Disabled stays Disabled under a Manual target.
                if !s.reached(start) {
                    svc_set_start(s.name, s.target)?;
                }
                if s.target == DISABLED {
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
        Kind::Feature(name) => {
            winfeature::set(name, false)?;
        }
        Kind::PowerSetting(ops) => {
            let scheme = active_scheme()?;
            for o in ops {
                power_write(&scheme, o, o.ac, o.dc)?;
            }
            set_active_scheme(&scheme)?; // the active plan picks up writes only when re-applied
        }
        Kind::AccessKeys => {
            for (name, flags) in access_keys_get()? {
                access_key_set(name, hotkey_off(flags))?;
            }
        }
        Kind::EmptyKey { key, sub } => registry::write(HKCU, &format!(r"{key}\{sub}"), "", &Value::Str(String::new()))?,
        Kind::VisualFx => unreachable!("handled by apply_visual_fx"),
    }
    Ok(())
}

/// Clear the "shortcut turns it on" bit; everything else the user set stays.
fn hotkey_off(flags: u32) -> u32 {
    flags & !HOTKEY_ACTIVE
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
        Kind::Feature(name) => {
            winfeature::set(name, true)?;
        }
        Kind::PowerSetting(ops) => {
            // Windows' own default for the setting (the Balanced personality), never a guess.
            let scheme = active_scheme()?;
            for o in ops {
                let (ac, dc) = power_default(o);
                power_write(&scheme, o, o.ac.and(ac), o.dc.and(dc))?;
            }
            set_active_scheme(&scheme)?;
        }
        Kind::AccessKeys => {
            for (name, flags) in access_keys_get()? {
                access_key_set(name, flags | HOTKEY_ACTIVE)?;
            }
        }
        Kind::EmptyKey { key, .. } => registry::delete_tree(HKCU, key)?,
        Kind::VisualFx => unreachable!("handled by revert_visual_fx"),
    }
    Ok(())
}

/// Restore captured entries in order. Pseudo-paths route to SPI / SCM / power / DISM.
/// The backup file is user-writable (hostile): only entries this tweak itself would capture
/// (same root, path and name as a fresh capture) are replayed; anything else is ignored.
fn restore_entries(t: &Tweak, entries: &[BackupEntry]) -> Result<(), BoostError> {
    let own = capture(t)?;
    let entries: Vec<&BackupEntry> =
        entries.iter().filter(|e| own.iter().any(|o| o.root == e.root && o.path == e.path && o.name == e.name)).collect();
    let scheme = entries.iter().find(|e| e.path == "powerset:scheme").and_then(|e| match &e.value {
        Value::Str(s) => GUID::try_from(s.as_str()).ok().filter(|g| enumerate_schemes().contains(g)),
        _ => None,
    });
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
            p if p.starts_with("feature:") => {
                winfeature::set(&p[8..], dword == Some(1))?;
            }
            "powerset:scheme" => {}
            p if p.starts_with("powerset:") => {
                let op = match t.kind {
                    Kind::PowerSetting(ops) => ops.iter().find(|o| power_path(o) == p),
                    _ => None,
                };
                if let (Some(o), Some(g), Some(v)) = (op, scheme, dword) {
                    let ac = e.name == "ac";
                    power_write(&g, o, ac.then_some(v), (!ac).then_some(v))?;
                    if active_scheme().ok() == Some(g) {
                        set_active_scheme(&g)?;
                    }
                }
            }
            "spi:accesskeys" => {
                if let Some(flags) = dword {
                    access_key_set(&e.name, flags)?;
                }
            }
            p if p.starts_with("regkey:") => {
                if dword == Some(0) {
                    registry::delete_tree(HKCU, &p[7..])?; // created by the tweak: remove it whole
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

/* ---------------- accessibility shortcuts (SPI) ---------------- */

/// `SKF_/FKF_/TKF_HOTKEYACTIVE`: the keyboard shortcut that turns the feature on.
const HOTKEY_ACTIVE: u32 = 0x4;
/// (name, get, set, struct size). STICKYKEYS and TOGGLEKEYS are `{cbSize, dwFlags}`; FILTERKEYS
/// adds four timing DWORDs, which are read and written back untouched.
const ACCESS_KEYS: [(&str, SYSTEM_PARAMETERS_INFO_ACTION, SYSTEM_PARAMETERS_INFO_ACTION, u32); 3] = [
    ("sticky", SPI_GETSTICKYKEYS, SPI_SETSTICKYKEYS, 8),
    ("filter", SPI_GETFILTERKEYS, SPI_SETFILTERKEYS, 24),
    ("toggle", SPI_GETTOGGLEKEYS, SPI_SETTOGGLEKEYS, 8),
];

fn access_key_read(get: SYSTEM_PARAMETERS_INFO_ACTION, size: u32) -> Result<[u32; 6], BoostError> {
    let mut s = [0u32; 6];
    s[0] = size;
    // SAFETY: the buffer is at least `size` bytes and starts with cbSize, as the struct does.
    unsafe { SystemParametersInfoW(get, size, Some(s.as_mut_ptr().cast::<c_void>()), Default::default()) }.map_err(map_win)?;
    Ok(s)
}

/// Current flags of the three features.
fn access_keys_get() -> Result<Vec<(&'static str, u32)>, BoostError> {
    ACCESS_KEYS.iter().map(|(name, get, _, size)| Ok((*name, access_key_read(*get, *size)?[1]))).collect()
}

fn access_key_set(name: &str, flags: u32) -> Result<(), BoostError> {
    let Some((_, get, set, size)) = ACCESS_KEYS.iter().find(|k| k.0 == name) else {
        return Ok(()); // unknown name (hostile backup): nothing to do
    };
    let mut s = access_key_read(*get, *size)?;
    s[1] = flags;
    // SAFETY: same struct as read, only dwFlags changed.
    unsafe { SystemParametersInfoW(*set, *size, Some(s.as_mut_ptr().cast::<c_void>()), SPI_FLAGS) }.map_err(map_win)
}

/* ---------------- power settings on a plan ---------------- */

fn power_read(scheme: &GUID, o: &PowerOp, ac: bool) -> Option<u32> {
    let mut v = 0u32;
    // SAFETY: GUID pointers outlive the call; plain out-param. A setting the system does not
    // expose (no Wi-Fi subgroup, say) fails and reads as None.
    let ok = unsafe {
        if ac {
            PowerReadACValueIndex(None, Some(scheme), Some(&o.subgroup), Some(&o.setting), &mut v) == ERROR_SUCCESS
        } else {
            PowerReadDCValueIndex(None, Some(scheme), Some(&o.subgroup), Some(&o.setting), &mut v) == ERROR_SUCCESS.0
        }
    };
    ok.then_some(v)
}

/// The value lies in the range this PC declares for the setting (outside it the write fails with
/// ERROR_INVALID_DATA).
fn power_in_range(o: &PowerOp, v: u32) -> bool {
    let (mut min, mut max) = (0u32, 0u32);
    // SAFETY: GUID pointers outlive the calls; the out-params are plain u32.
    unsafe {
        PowerReadValueMin(None, Some(&o.subgroup), Some(&o.setting), &mut min) == ERROR_SUCCESS
            && PowerReadValueMax(None, Some(&o.subgroup), Some(&o.setting), &mut max) == ERROR_SUCCESS
            && (min..=max).contains(&v)
    }
}

fn power_write(scheme: &GUID, o: &PowerOp, ac: Option<u32>, dc: Option<u32>) -> Result<(), BoostError> {
    // SAFETY: GUID pointers outlive the calls.
    unsafe {
        if let Some(v) = ac {
            check(PowerWriteACValueIndex(None, scheme, Some(&o.subgroup), Some(&o.setting), v))?;
        }
        if let Some(v) = dc {
            check(WIN32_ERROR(PowerWriteDCValueIndex(None, scheme, Some(&o.subgroup), Some(&o.setting), v)))?;
        }
    }
    Ok(())
}

/// Windows' default (AC, DC) for the setting under the Balanced personality.
fn power_default(o: &PowerOp) -> (Option<u32>, Option<u32>) {
    let (mut ac, mut dc) = (0u32, 0u32);
    // SAFETY: GUID pointers outlive the calls; plain out-params.
    unsafe {
        let a = PowerReadACDefaultIndex(None, &BALANCED, Some(&o.subgroup), &o.setting, &mut ac);
        let d = PowerReadDCDefaultIndex(None, &BALANCED, Some(&o.subgroup), &o.setting, &mut dc);
        ((a == ERROR_SUCCESS.0).then_some(ac), (d == ERROR_SUCCESS.0).then_some(dc))
    }
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

/// Delete a leftover service whose program no longer exists (admin). Re-checked here: a service
/// whose binary is still on disk is never deleted.
pub(crate) fn delete_orphan_service(name: &str) -> Result<(), BoostError> {
    const DELETE: u32 = 0x0001_0000;
    let scm = open_scm()?;
    let Some(h) = open_svc(&scm, name, SERVICE_QUERY_CONFIG | SERVICE_STOP | DELETE)? else {
        return Ok(());
    };
    let mut needed = 0u32;
    // SAFETY: size probe, then a buffer of `needed` bytes (8-byte aligned) for the config.
    let _ = unsafe { QueryServiceConfigW(h.0, None, 0, &mut needed) };
    let mut buf = vec![0u64; (needed as usize).div_ceil(8) + 1];
    unsafe { QueryServiceConfigW(h.0, Some(buf.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>()), needed, &mut needed) }
        .map_err(map_win)?;
    // SAFETY: filled by the call above; the string lives inside `buf`.
    let image = unsafe { (*buf.as_ptr().cast::<QUERY_SERVICE_CONFIGW>()).lpBinaryPathName.to_string() }.unwrap_or_default();
    let exe = crate::apps::service_exe(&image);
    if exe.is_empty() || std::path::Path::new(&exe).exists() {
        return Err(BoostError::Win32("service binary still present".into()));
    }
    let mut st = SERVICE_STATUS::default();
    // SAFETY: valid handle; stopping a service whose program is gone usually fails — ignored.
    unsafe {
        let _ = ControlService(h.0, SERVICE_CONTROL_STOP, &mut st);
        DeleteService(h.0).map_err(map_win)
    }
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

pub(crate) fn take_local_guid(p: *mut GUID) -> Option<GUID> {
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

pub(crate) fn enumerate_schemes() -> Vec<GUID> {
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
pub(crate) fn ensure_ultimate_scheme() -> Result<GUID, BoostError> {
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

pub(crate) fn enable_privilege(name: &str) -> Result<(), BoostError> {
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

    /// Elevated: flip each round-7 task tweak and flip it back; the end state must match the start.
    /// `--ignored round7_tasks_roundtrip_elevated` in an elevated shell with `G2BOOST_DATA_DIR` set.
    #[test]
    #[ignore]
    fn round7_tasks_roundtrip_elevated() {
        if !crate::system::is_elevated() {
            println!("skip: not elevated");
            return;
        }
        for id in ["winsat_task_off", "feedback_tasks_off"] {
            let start = detect(TWEAKS.iter().find(|t| t.id == id).unwrap()).status;
            if start == Status::Partial {
                println!("{id}: partial, left alone");
                continue;
            }
            let on = start == Status::On;
            assert_eq!(apply(id, !on).unwrap().status, if on { Status::Off } else { Status::On }, "{id} flip");
            assert_eq!(apply(id, on).unwrap().status, start, "{id} back");
            println!("{id}: {start:?} -> flipped -> back ok");
        }
    }

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
    fn absent_default_on_and_neutral_ops_do_not_skew_status() {
        const P: &str = r"Software\G2 Boost\Test\boost_default";
        const OPS: &[RegOp] = &[
            op_default_on(HKCU, P, "mode", Desired::Dword(1), Desired::Absent),
            op(HKCU, P, "same", Desired::Dword(8), Desired::Dword(8)),
            op(HKCU, P, "dvr", Desired::Dword(0), Desired::Dword(1)),
        ];
        const T: Tweak = tw("unit_test_default_on", false, false, Performance, Kind::Registry(OPS));
        for n in ["mode", "same", "dvr"] {
            registry::write(HKCU, P, n, &Value::Absent).unwrap();
        }
        assert_eq!(detect(&T).status, Status::Off, "stock PC: only dvr counts");
        registry::write(HKCU, P, "dvr", &Value::Dword(0)).unwrap();
        assert_eq!(detect(&T).status, Status::On, "mode absent = Windows default on");
        registry::write(HKCU, P, "mode", &Value::Dword(0)).unwrap();
        assert_eq!(detect(&T).status, Status::Partial, "explicit 0 turns mode off");
        for n in ["mode", "dvr"] {
            registry::write(HKCU, P, n, &Value::Absent).unwrap();
        }

        assert!(is_ultimate_name("Ultimate Performance (ExitLag)", "Desempenho Máximo"));
        assert!(is_ultimate_name("Desempenho Máximo", "Desempenho Máximo"));
        assert!(!is_ultimate_name("Alto desempenho", "Desempenho Máximo"));
        assert!(!is_ultimate_name("Equilibrado", ""));
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

        // A tampered backup: entries this tweak never captures are ignored on restore.
        let mut tampered = b.entries.clone();
        tampered.push(BackupEntry { root: HKCU, path: TEST_PATH.into(), name: "planted".into(), value: Value::Dword(1) });
        tampered.push(BackupEntry { root: HKCU, path: r"Software\G2 Boost\Test\elsewhere".into(), name: "alpha".into(), value: Value::Dword(1) });
        restore_entries(&TEST_TWEAK, &tampered).unwrap();
        assert_eq!(registry::read(HKCU, TEST_PATH, "planted").unwrap(), Value::Absent, "not ours: not written");
        assert_eq!(registry::read(HKCU, r"Software\G2 Boost\Test\elsewhere", "alpha").unwrap(), Value::Absent);
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

    /// The UI catalog (`src/data/catalogs.ts`) and both dictionaries follow this catalog: same ids,
    /// same category and admin flag, name/description/warning in pt-BR and en.
    #[test]
    fn catalog_matches_the_ui_and_the_dictionaries() {
        let file = include_str!("../../../src/data/catalogs.ts");
        let pt = include_str!("../../../src/locales/pt-BR.ts");
        let en = include_str!("../../../src/locales/en.ts");
        let from = file.find("export const TWEAKS").expect("TWEAKS in catalogs.ts");
        let ui = &file[from..from + file[from..].find("\n];").expect("end of TWEAKS")];
        let mut ui_ids: Vec<&str> = ui.split("id: \"").skip(1).map(|s| &s[..s.find('"').unwrap()]).collect();
        let mut ids: Vec<&str> = TWEAKS.iter().map(|t| t.id).collect();
        ui_ids.sort_unstable();
        ids.sort_unstable();
        assert_eq!(ui_ids, ids, "the UI and the core list the same tweaks");
        let mut seen = std::collections::HashSet::new();
        for t in TWEAKS {
            assert!(seen.insert(t.id), "duplicate id {}", t.id);
            assert!(registry::safe_id(t.id), "{}", t.id);
            let at = ui.find(&format!("id: \"{}\",", t.id)).unwrap_or_else(|| panic!("{} missing in catalogs.ts", t.id));
            let item = &ui[at..at + ui[at..].find('}').expect("entry ends")];
            let category = serde_json::to_value(t.category).unwrap();
            assert!(item.contains(&format!("category: \"{}\"", category.as_str().unwrap())), "{}: category differs in the UI", t.id);
            assert_eq!(item.contains("admin: true"), t.admin, "{}: admin differs in the UI", t.id);
            assert_eq!(item.contains("beta: true"), t.beta, "{}: beta differs in the UI", t.id);
            for dict in [pt, en] {
                for key in [format!("\"boost.tweak.{}\":", t.id), format!("\"boost.tweak.{}.desc\":", t.id), format!("\"boost.tweak.{}.warn\":", t.id)] {
                    assert!(dict.contains(&key), "{key} missing in a dictionary");
                }
            }
        }
        for c in [Performance, Privacy, Appearance, Network, Gaming, Ai, System] {
            let key = format!("\"boost.category.{}\":", serde_json::to_value(c).unwrap().as_str().unwrap());
            assert!(pt.contains(&key) && en.contains(&key), "{key}");
        }
    }

    /// What the product promises never to touch: Windows Update, Defender, firewall, UAC, Prefetch.
    #[test]
    fn nothing_forbidden_in_the_catalog() {
        const FORBIDDEN: &[&str] = &[
            "windows defender", "spynet", "mpengine", r"\policies\system", "enablelua", "consentprompt", "windowsupdate",
            r"\au\", "noautoupdate", "auoptions", "firewall", "prefetch", "securityhealth", "updateorchestrator",
            "scheduleddefrag", "waasmedic",
        ];
        const SERVICES: &[&str] =
            &["windefend", "wuauserv", "usosvc", "waasmedicsvc", "mpssvc", "bfe", "securityhealthservice", "sense", "wdnissvc", "wscsvc", "bits", "trustedinstaller"];
        for t in TWEAKS {
            for c in changes(t.id).unwrap() {
                let target = c.target.to_lowercase();
                assert!(!FORBIDDEN.iter().any(|f| target.contains(f)), "{}: {}", t.id, c.target);
                if c.kind == "service" {
                    assert!(!SERVICES.contains(&target.as_str()), "{}: service {}", t.id, c.target);
                }
            }
        }
    }

    #[test]
    fn every_tweak_says_what_it_changes() {
        for t in TWEAKS {
            let list = changes(t.id).unwrap();
            assert!(!list.is_empty(), "{}", t.id);
            for c in &list {
                assert!(["registry", "service", "task", "power", "plan", "feature", "system", "visual_fx"].contains(&c.kind), "{}", c.kind);
                if c.kind == "registry" {
                    assert!(c.target.starts_with(r"HKCU\") || c.target.starts_with(r"HKLM\"), "{}", c.target);
                }
                if c.kind == "feature" {
                    assert!(winfeature::ALLOWED.contains(&c.target.as_str()), "{}", c.target);
                }
            }
        }
        let copilot = changes("copilot_off").unwrap();
        assert!(copilot.contains(&Change {
            kind: "registry",
            target: r"HKCU\Software\Policies\Microsoft\Windows\WindowsCopilot\TurnOffWindowsCopilot".into(),
            value: Some("1".into()),
        }));
        assert_eq!(changes("diagtrack_manual").unwrap()[0].value.as_deref(), Some("manual"));
        assert_eq!(changes("classic_context_menu").unwrap()[0].target, format!(r"HKCU\{CLASSIC_MENU_KEY}\InprocServer32\@"));
        assert!(changes("../x").is_err());
    }

    #[test]
    fn service_targets_never_loosen() {
        let manual = Svc { name: "x", default_start: AUTO, target: MANUAL };
        let disabled = off("x", AUTO);
        assert!(!manual.reached(AUTO) && manual.reached(MANUAL) && manual.reached(DISABLED));
        assert!(!disabled.reached(MANUAL) && disabled.reached(DISABLED));
        assert!(!manual.reached(0) && !manual.reached(1), "boot/system drivers are never a match");
        assert_eq!(hotkey_off(0x7E), 0x7A, "only the shortcut bit goes");
        assert_eq!(hotkey_off(0), 0);
        let ntfs = NTFS_LAST_ACCESS_OFF[0];
        for on in [0x8000_0001, 0x8000_0003, 1] {
            assert!(ntfs.is_on(&Value::Dword(on)), "{on:#x}");
        }
        for off in [0x8000_0002, 0x8000_0000, 0] {
            assert!(!ntfs.is_on(&Value::Dword(off)), "{off:#x}");
        }
        assert!(!ntfs.is_on(&Value::Absent) && !ntfs.is_on(&Value::Str("1".into())));
    }

    #[test]
    fn gated_tweaks_are_unavailable_where_they_mean_nothing() {
        fn never() -> bool {
            false
        }
        const T: Tweak = tw("unit_test_gated", false, false, System, Kind::Registry(TEST_OPS)).gated(never);
        let st = detect(&T);
        assert_eq!((st.status, st.detail.as_deref()), (Status::Unavailable, Some("not_present")));
        let _ = (wifi(), office(), windows11(), chat_button()); // live probes answer without panicking
    }

    /// The classic-menu mechanism on a throwaway HKCU key: created by the tweak → removed whole by
    /// the revert; already there → only its value goes back.
    #[test]
    fn empty_key_is_removed_only_if_we_created_it() {
        // SAFETY: same override every data-dir test in this crate uses (process-wide env var).
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
        const KEY: &str = r"Software\G2 Boost\Test\emptykey\{00000000-0000-0000-0000-00000000c1a5}";
        const T: Tweak = tw("unit_test_emptykey", false, false, Appearance, Kind::EmptyKey { key: KEY, sub: "InprocServer32" });
        let sub = format!(r"{KEY}\InprocServer32");
        registry::delete_tree(HKCU, KEY).unwrap();
        registry::delete_backup(T.id);
        assert_eq!(detect(&T).status, Status::Off);

        ensure_backup(&T).unwrap();
        apply_on(&T).unwrap();
        assert_eq!(detect(&T).status, Status::On);
        assert_eq!(registry::read(HKCU, &sub, "").unwrap(), Value::Str(String::new()));
        restore_entries(&T, &registry::load_backup(T.id).unwrap().entries).unwrap();
        registry::delete_backup(T.id);
        assert!(!key_exists(KEY).unwrap(), "created by us: gone");

        registry::write(HKCU, &sub, "", &Value::Str("keep".into())).unwrap();
        ensure_backup(&T).unwrap();
        apply_on(&T).unwrap();
        restore_entries(&T, &registry::load_backup(T.id).unwrap().entries).unwrap();
        registry::delete_backup(T.id);
        assert_eq!(registry::read(HKCU, &sub, "").unwrap(), Value::Str("keep".into()), "the user's key stays");
        registry::delete_tree(HKCU, r"Software\G2 Boost\Test\emptykey").unwrap();
    }

    /// Elevated: flip every round-9 tweak that writes the registry, power settings or SPI and flip
    /// it back; the end state must match the start. Skips the DISM feature (minutes, reboot).
    /// `--ignored round9_roundtrip_elevated` in an elevated shell with `G2BOOST_DATA_DIR` set.
    #[test]
    #[ignore]
    fn round9_roundtrip_elevated() {
        if !crate::system::is_elevated() {
            println!("skip: not elevated");
            return;
        }
        let start = TWEAKS.iter().position(|t| t.id == "edge_background_off").unwrap();
        for t in &TWEAKS[start..] {
            let before = detect(t).status;
            if matches!(t.kind, Kind::Feature(_)) || matches!(before, Status::Partial | Status::Unavailable) {
                println!("{}: {before:?}, left alone", t.id);
                continue;
            }
            let on = before == Status::On;
            let flipped = match apply(t.id, !on) {
                Err(BoostError::Blocked) => {
                    // Windows (UCPD) or the antivirus guards this key: the state must be untouched.
                    assert_eq!(detect(t).status, before, "{} after blocked", t.id);
                    println!("{}: blocked by Windows/antivirus, unchanged", t.id);
                    continue;
                }
                r => r.unwrap(),
            };
            assert_eq!(flipped.status, if on { Status::Off } else { Status::On }, "{} flip", t.id);
            assert_eq!(apply(t.id, on).unwrap().status, before, "{} back", t.id);
            println!("{}: {before:?} -> flipped -> back ok", t.id);
        }
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
