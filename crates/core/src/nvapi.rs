//! NVIDIA driver 3D settings (the driver profile database, "DRS") through NVAPI.
//!
//! `nvapi64.dll` ships with the driver; it is loaded from System32 only and every function is
//! resolved through `nvapi_QueryInterface` by id. Interface ids, struct layouts, setting ids and
//! values come from NVIDIA's public SDK (github.com/NVIDIA/nvapi, MIT, commit 70d337d,
//! 2026-09-18: `nvapi_interface.h`, `nvapi.h`, `NvApiDriverSettings.h`). The two "ultra low
//! latency" ids are not in the public header; see Docs/boost/gpu.md for how they were checked.
//!
//! Hostile callers: the UI and MCP name a setting by our own id and an option key; only the
//! allowlist below reaches `SetSetting`. The previous value of every DRS id is saved before the
//! first change (JSON in the data dir, validated on read); revert restores it or deletes the
//! user value (driver default / NVIDIA's predefined game value).

use std::ffi::c_void;
use std::mem::size_of;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};
use windows::core::{PCWSTR, s};

use crate::registry::{self, wide};

/* ---------------- catalog (the allowlist) ---------------- */

/// One DRS dword: id, the driver's default when no profile sets it, and the values the public
/// header allows (restoring a backup outside this range is refused).
#[derive(Clone, Copy)]
struct Drs {
    id: u32,
    default: u32,
    valid: fn(u32) -> bool,
}

const PRERENDERLIMIT: Drs = Drs { id: 0x007B_A09E, default: 0, valid: |v| v <= 0xff };
// Not in the public header (see module doc): "Ultra Low Latency - CPL State" 0 off / 1 on /
// 2 ultra, and "Ultra Low Latency - Enabled" 0 / 1.
const ULL_CPL_STATE: Drs = Drs { id: 0x0005_F543, default: 0, valid: |v| v <= 2 };
const ULL_ENABLED: Drs = Drs { id: 0x1083_5000, default: 0, valid: |v| v <= 1 };
const PREFERRED_PSTATE: Drs = Drs { id: 0x1057_EB71, default: 5, valid: |v| v <= 5 };
const FRL_FPS: Drs = Drs { id: 0x1083_5002, default: 0, valid: |v| v <= FPS_MAX };
const VSYNCMODE: Drs = Drs {
    id: 0x00A8_79CF,
    default: VSYNC_APP,
    valid: |v| {
        [VSYNC_APP, VSYNC_OFF, VSYNC_ON, 0x3261_0244, 0x7127_1021, 0x1324_5256, VSYNC_FAST].contains(&v)
    },
};
const SHADER_CACHE_SIZE: Drs = Drs { id: 0x00AC_8497, default: 0x4000, valid: |_| true };
const QUALITY_ENHANCEMENTS: Drs = Drs {
    id: 0x00CE_2691,
    default: 0,
    valid: |v| [TEX_HIGH_QUALITY, 0, TEX_PERFORMANCE, TEX_HIGH_PERFORMANCE].contains(&v),
};
const OGL_THREAD_CONTROL: Drs = Drs { id: 0x20C1_221E, default: 0, valid: |v| v <= 2 };

const VSYNC_APP: u32 = 0x6092_5292;
const VSYNC_OFF: u32 = 0x0841_6747;
const VSYNC_ON: u32 = 0x4781_4940;
const VSYNC_FAST: u32 = 0x1888_8888;
const TEX_HIGH_QUALITY: u32 = 0xffff_fff6;
const TEX_PERFORMANCE: u32 = 0x0000_000a;
const TEX_HIGH_PERFORMANCE: u32 = 0x0000_0014;

/// Frame cap range the UI accepts (0 = off). The header allows up to 1023.
pub const FPS_MIN: u32 = 20;
pub const FPS_MAX: u32 = 1000;

enum Kind {
    /// Named options; each lists one value per DRS id of the setting, in order.
    Choice(&'static [(&'static str, &'static [u32])]),
    /// Frame cap: 0 = off, else `FPS_MIN..=FPS_MAX`.
    Fps,
}

type Detect = fn(&[u32]) -> Option<&'static str>;

struct Def {
    id: &'static str,
    drs: &'static [Drs],
    kind: Kind,
    /// Option key suggested for games; `None` = depends on the monitor or the game.
    recommended: Option<&'static str>,
    /// Other values that already count as good ("apply recommended" never downgrades them).
    fine: &'static [&'static str],
    /// Custom reading for composite settings (default: exact match of every value).
    detect: Option<Detect>,
}

/// Low Latency Mode in the control panel writes three ids. Read it by the strongest switch so a
/// value set elsewhere (old "max pre-rendered frames 1") still reads as On.
fn detect_low_latency(v: &[u32]) -> Option<&'static str> {
    match (v[0], v[1], v[2]) {
        (_, _, 1) => Some("ultra"),
        (1, _, _) => Some("on"),
        (0, 0, 0) => Some("off"),
        _ => None,
    }
}

static CATALOG: &[Def] = &[
    Def {
        id: "low_latency",
        drs: &[PRERENDERLIMIT, ULL_CPL_STATE, ULL_ENABLED],
        kind: Kind::Choice(&[("off", &[0, 0, 0]), ("on", &[1, 1, 0]), ("ultra", &[1, 2, 1])]),
        recommended: Some("on"),
        fine: &["ultra"],
        detect: Some(detect_low_latency),
    },
    Def {
        id: "power",
        drs: &[PREFERRED_PSTATE],
        kind: Kind::Choice(&[("normal", &[5]), ("adaptive", &[0]), ("max", &[1])]),
        recommended: Some("max"),
        fine: &[],
        detect: None,
    },
    Def { id: "fps_limit", drs: &[FRL_FPS], kind: Kind::Fps, recommended: None, fine: &[], detect: None },
    Def {
        id: "vsync",
        drs: &[VSYNCMODE],
        kind: Kind::Choice(&[("app", &[VSYNC_APP]), ("off", &[VSYNC_OFF]), ("on", &[VSYNC_ON]), ("fast", &[VSYNC_FAST])]),
        // Right answer depends on the monitor (variable refresh wants On + a cap): no default.
        recommended: None,
        fine: &[],
        detect: None,
    },
    Def {
        id: "shader_cache",
        drs: &[SHADER_CACHE_SIZE],
        // Sizes in MB as the driver stores them (100 GB read back as 0x19000 on the owner's PC);
        // "unlimited" is the all-ones value, "default" the header's default.
        kind: Kind::Choice(&[
            ("default", &[0x4000]),
            ("5gb", &[5 * 1024]),
            ("10gb", &[10 * 1024]),
            ("100gb", &[100 * 1024]),
            ("unlimited", &[0xffff_ffff]),
        ]),
        recommended: Some("10gb"),
        fine: &["default", "100gb", "unlimited"],
        detect: None,
    },
    Def {
        id: "texture_quality",
        drs: &[QUALITY_ENHANCEMENTS],
        kind: Kind::Choice(&[
            ("high_quality", &[TEX_HIGH_QUALITY]),
            ("quality", &[0]),
            ("performance", &[TEX_PERFORMANCE]),
            ("high_performance", &[TEX_HIGH_PERFORMANCE]),
        ]),
        recommended: Some("high_performance"),
        fine: &["performance"],
        detect: None,
    },
    Def {
        id: "threaded",
        drs: &[OGL_THREAD_CONTROL],
        kind: Kind::Choice(&[("auto", &[0]), ("on", &[1]), ("off", &[2])]),
        recommended: Some("auto"),
        fine: &["on"],
        detect: None,
    },
];

/// Already at the recommended value (or one that counts as good).
fn is_fine(d: &Def, value: Option<&str>) -> bool {
    value.is_some_and(|v| d.recommended == Some(v) || d.fine.contains(&v))
}

fn def(id: &str) -> Result<&'static Def, GpuError> {
    CATALOG.iter().find(|d| d.id == id).ok_or(GpuError::UnknownSetting)
}

/// The DRS writes for `value` (an option key, or a frame cap as a decimal number).
fn resolve(d: &'static Def, value: &str) -> Result<Vec<(Drs, u32)>, GpuError> {
    let values: Vec<u32> = match &d.kind {
        Kind::Choice(opts) => opts.iter().find(|(k, _)| *k == value).ok_or(GpuError::BadValue)?.1.to_vec(),
        Kind::Fps => {
            let n: u32 = value.parse().map_err(|_| GpuError::BadValue)?;
            if n != 0 && !(FPS_MIN..=FPS_MAX).contains(&n) {
                return Err(GpuError::BadValue);
            }
            vec![n]
        }
    };
    Ok(d.drs.iter().copied().zip(values).collect())
}

/// The option key for the effective values (`None` = a value none of our options use).
fn read_value(d: &Def, effective: &[u32]) -> Option<String> {
    match &d.kind {
        Kind::Fps => Some(effective[0].to_string()),
        Kind::Choice(opts) => match d.detect {
            Some(f) => f(effective).map(str::to_string),
            None => opts.iter().find(|(_, v)| *v == effective).map(|(k, _)| k.to_string()),
        },
    }
}

fn known_drs(id: u32) -> Option<Drs> {
    CATALOG.iter().flat_map(|d| d.drs.iter()).find(|x| x.id == id).copied()
}

/// A game's executable as the driver matches it: a bare file name ending in `.exe`.
pub fn valid_exe(exe: &str) -> bool {
    (5..=128).contains(&exe.len())
        && exe.to_ascii_lowercase().ends_with(".exe")
        && !exe.starts_with('.')
        && !exe.chars().any(|c| c.is_control() || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

/* ---------------- public types ---------------- */

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum GpuError {
    #[error("no_nvidia")]
    NoNvidia,
    #[error("needs_admin")]
    NeedsAdmin,
    #[error("unknown_setting")]
    UnknownSetting,
    #[error("bad_value")]
    BadValue,
    #[error("bad_exe")]
    BadExe,
    #[error("nvapi_{0}")]
    Nvapi(i32),
    #[error("io")]
    Io,
}

/// Where a change lands: the global profile, or the profile that holds a game's executable.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Global,
    Game(String),
}

impl Target {
    pub fn game(exe: &str) -> Result<Self, GpuError> {
        if valid_exe(exe) { Ok(Self::Game(exe.to_string())) } else { Err(GpuError::BadExe) }
    }

    fn key(&self) -> String {
        match self {
            Self::Global => "global".into(),
            Self::Game(exe) => exe.to_ascii_lowercase(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DriverInfo {
    /// "617.14"
    pub version: String,
    /// Build branch, e.g. "r617_10".
    pub branch: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingView {
    pub id: &'static str,
    /// Option key, the frame cap as a number, or `None` for a value none of the options use.
    pub value: Option<String>,
    /// Raw dwords in hex, one per driver id ("0x1 0x1 0x0" for low latency On).
    pub raw: String,
    /// Option keys (empty for the frame cap).
    pub options: Vec<&'static str>,
    pub recommended: Option<&'static str>,
    /// At the recommended value or another one that counts as good.
    pub good: bool,
    /// Set by the user in this profile (not inherited, not NVIDIA's predefined value).
    pub user_set: bool,
    /// We changed it and hold the previous value.
    pub backed_up: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileView {
    /// Driver profile name; `None` when the game has no profile yet (values shown are global).
    pub profile: Option<String>,
    pub settings: Vec<SettingView>,
}

/* ---------------- NVAPI binding ---------------- */

type Handle = *mut c_void;
type Status = i32;

const USTR: usize = 2048; // NVAPI_UNICODE_STRING_MAX

const fn version<T>(v: u32) -> u32 {
    size_of::<T>() as u32 | (v << 16) // MAKE_NVAPI_VERSION
}

/// `NVDRS_SETTING_V1` (pack 4). The two unions are 4100 bytes (binary value); dword values use
/// the first element.
#[repr(C)]
struct DrsSetting {
    version: u32,
    name: [u16; USTR],
    id: u32,
    kind: u32,
    location: u32,
    is_current_predefined: u32,
    is_predefined_valid: u32,
    predefined: [u32; 1025],
    current: [u32; 1025],
}
const _: () = assert!(size_of::<DrsSetting>() == 12320);

/// `NVDRS_PROFILE_V1`.
#[repr(C)]
struct DrsProfile {
    version: u32,
    name: [u16; USTR],
    gpu_support: u32,
    is_predefined: u32,
    num_apps: u32,
    num_settings: u32,
}
const _: () = assert!(size_of::<DrsProfile>() == 4116);

/// `NVDRS_APPLICATION_V4`.
#[repr(C)]
struct DrsApplication {
    version: u32,
    is_predefined: u32,
    app_name: [u16; USTR],
    friendly_name: [u16; USTR],
    launcher: [u16; USTR],
    file_in_folder: [u16; USTR],
    flags: u32,
    command_line: [u16; USTR],
}
const _: () = assert!(size_of::<DrsApplication>() == 20492);

const OK: Status = 0;
const NVIDIA_DEVICE_NOT_FOUND: Status = -6;
const INVALID_USER_PRIVILEGE: Status = -137;
const SETTING_NOT_FOUND: Status = -160;
const PROFILE_NOT_FOUND: Status = -163;
const PROFILE_NAME_IN_USE: Status = -164;
const EXECUTABLE_NOT_FOUND: Status = -166;
const ACCESS_DENIED: Status = -175;

const LOCATION_CURRENT_PROFILE: u32 = 0;
const TYPE_DWORD: u32 = 0;

type QueryInterface = unsafe extern "C" fn(u32) -> *mut c_void;

struct Api {
    #[cfg_attr(not(test), allow(dead_code))]
    qi: QueryInterface,
    initialize: unsafe extern "C" fn() -> Status,
    unload: unsafe extern "C" fn() -> Status,
    driver_branch: unsafe extern "C" fn(*mut u32, *mut u8) -> Status,
    create_session: unsafe extern "C" fn(*mut Handle) -> Status,
    destroy_session: unsafe extern "C" fn(Handle) -> Status,
    load_settings: unsafe extern "C" fn(Handle) -> Status,
    save_settings: unsafe extern "C" fn(Handle) -> Status,
    global_profile: unsafe extern "C" fn(Handle, *mut Handle) -> Status,
    profile_info: unsafe extern "C" fn(Handle, Handle, *mut DrsProfile) -> Status,
    find_application: unsafe extern "C" fn(Handle, *const u16, *mut Handle, *mut DrsApplication) -> Status,
    find_profile: unsafe extern "C" fn(Handle, *const u16, *mut Handle) -> Status,
    create_profile: unsafe extern "C" fn(Handle, *mut DrsProfile, *mut Handle) -> Status,
    delete_profile: unsafe extern "C" fn(Handle, Handle) -> Status,
    create_application: unsafe extern "C" fn(Handle, Handle, *mut DrsApplication) -> Status,
    get_setting: unsafe extern "C" fn(Handle, Handle, u32, *mut DrsSetting) -> Status,
    set_setting: unsafe extern "C" fn(Handle, Handle, *mut DrsSetting) -> Status,
    delete_setting: unsafe extern "C" fn(Handle, Handle, u32) -> Status,
}

static API: OnceLock<Option<Api>> = OnceLock::new();
/// One DRS session at a time (the UI, the MCP and "apply recommended" never interleave writes).
static LOCK: Mutex<()> = Mutex::new(());

fn api() -> Option<&'static Api> {
    API.get_or_init(load).as_ref()
}

/// A function pointer from `nvapi_QueryInterface`, as the field's declared signature.
///
/// SAFETY: the caller pairs the interface id with the signature nvapi.h declares for it.
unsafe fn cast<T: Copy>(p: *mut c_void) -> T {
    const { assert!(size_of::<T>() == size_of::<*mut c_void>()) };
    unsafe { std::mem::transmute_copy::<*mut c_void, T>(&p) }
}

fn load() -> Option<Api> {
    let name = wide("nvapi64.dll");
    // SAFETY: System32-only load of the driver's NVAPI; `nvapi_QueryInterface(id)` returns the
    // function for an id from nvapi_interface.h, transmuted to the signature declared in nvapi.h.
    unsafe {
        let h = LoadLibraryExW(PCWSTR(name.as_ptr()), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        let qi: QueryInterface = std::mem::transmute(GetProcAddress(h, s!("nvapi_QueryInterface"))?);
        macro_rules! f {
            ($id:literal) => {{
                let p = qi($id);
                if p.is_null() {
                    return None;
                }
                cast(p)
            }};
        }
        Some(Api {
            qi,
            initialize: f!(0x0150_e828),
            unload: f!(0xd22b_dd7e),
            driver_branch: f!(0x2926_aaad),
            create_session: f!(0x0694_d52e),
            destroy_session: f!(0xdad9_cff8),
            load_settings: f!(0x375d_bd6b),
            save_settings: f!(0xfcbc_7e14),
            global_profile: f!(0x617b_ff9f),
            profile_info: f!(0x61cd_6fd6),
            find_application: f!(0xeee5_66b2),
            find_profile: f!(0x7e4a_9a0b),
            create_profile: f!(0xcc17_6068),
            delete_profile: f!(0x1709_3206),
            create_application: f!(0x4347_a9de),
            get_setting: f!(0x73bf_8338),
            set_setting: f!(0x577d_d202),
            delete_setting: f!(0xe4a2_6362),
        })
    }
}

fn check(st: Status) -> Result<(), GpuError> {
    match st {
        OK => Ok(()),
        INVALID_USER_PRIVILEGE | ACCESS_DENIED => Err(GpuError::NeedsAdmin),
        NVIDIA_DEVICE_NOT_FOUND => Err(GpuError::NoNvidia),
        e => Err(GpuError::Nvapi(e)),
    }
}

fn ustr(s: &str) -> [u16; USTR] {
    let mut buf = [0u16; USTR];
    for (d, c) in buf.iter_mut().take(USTR - 1).zip(s.encode_utf16()) {
        *d = c;
    }
    buf
}

fn from_ustr(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// `NvAPI_Initialize` … `NvAPI_Unload` (NVAPI is reference counted; every open pairs a close).
struct Init(&'static Api);

impl Init {
    fn open() -> Result<Self, GpuError> {
        let api = api().ok_or(GpuError::NoNvidia)?;
        // SAFETY: no arguments; status code only.
        check(unsafe { (api.initialize)() })?;
        Ok(Self(api))
    }
}

impl Drop for Init {
    fn drop(&mut self) {
        // SAFETY: pairs the successful Initialize above.
        unsafe { (self.0.unload)() };
    }
}

struct Session {
    init: Init,
    h: Handle,
}

/// What one DRS id holds in one profile.
struct Read {
    /// Effective value (`None` = no profile sets it, the driver default applies).
    value: Option<u32>,
    /// The user's own value in this profile (what a backup must bring back).
    user: Option<u32>,
}

impl Session {
    fn open() -> Result<Self, GpuError> {
        let init = Init::open()?;
        let mut h = null_mut();
        // SAFETY: out-param handle; destroyed in Drop.
        check(unsafe { (init.0.create_session)(&mut h) })?;
        let s = Self { init, h };
        // SAFETY: valid session.
        check(unsafe { (s.api().load_settings)(s.h) })?;
        Ok(s)
    }

    fn api(&self) -> &'static Api {
        self.init.0
    }

    fn global(&self) -> Result<Handle, GpuError> {
        let mut p = null_mut();
        // SAFETY: valid session; out-param handle.
        check(unsafe { (self.api().global_profile)(self.h, &mut p) })?;
        Ok(p)
    }

    /// The profile holding `exe`, if any.
    fn find_app(&self, exe: &str) -> Result<Option<Handle>, GpuError> {
        let name = ustr(exe);
        let mut p = null_mut();
        // SAFETY: zeroed POD with its version set; name buffer is NVAPI_UNICODE_STRING_MAX long.
        let mut app: Box<DrsApplication> = Box::new(unsafe { std::mem::zeroed() });
        app.version = version::<DrsApplication>(4);
        match unsafe { (self.api().find_application)(self.h, name.as_ptr(), &mut p, &mut *app) } {
            EXECUTABLE_NOT_FOUND | PROFILE_NOT_FOUND => Ok(None),
            st => check(st).map(|_| Some(p)),
        }
    }

    /// A profile of our own for `exe` (reused when a previous run left it), with the app in it.
    fn create_for(&self, exe: &str) -> Result<Handle, GpuError> {
        let pname = ustr(&format!("G2 Boost {exe}"));
        let mut p = null_mut();
        // SAFETY: zeroed POD with version + name set; out-param handle.
        let mut info: Box<DrsProfile> = Box::new(unsafe { std::mem::zeroed() });
        info.version = version::<DrsProfile>(1);
        info.name = pname;
        match unsafe { (self.api().create_profile)(self.h, &mut *info, &mut p) } {
            PROFILE_NAME_IN_USE => check(unsafe { (self.api().find_profile)(self.h, pname.as_ptr(), &mut p) })?,
            st => check(st)?,
        }
        // SAFETY: zeroed POD with version + app name set.
        let mut app: Box<DrsApplication> = Box::new(unsafe { std::mem::zeroed() });
        app.version = version::<DrsApplication>(4);
        app.app_name = ustr(exe);
        app.friendly_name = ustr(exe);
        check(unsafe { (self.api().create_application)(self.h, p, &mut *app) })?;
        Ok(p)
    }

    fn profile_name(&self, p: Handle) -> Option<String> {
        // SAFETY: zeroed POD with version set; valid handles.
        let mut info: Box<DrsProfile> = Box::new(unsafe { std::mem::zeroed() });
        info.version = version::<DrsProfile>(1);
        (unsafe { (self.api().profile_info)(self.h, p, &mut *info) } == OK).then(|| from_ustr(&info.name))
    }

    fn get(&self, p: Handle, id: u32) -> Result<Read, GpuError> {
        // SAFETY: zeroed POD with version set; the driver fills it.
        let mut st: Box<DrsSetting> = Box::new(unsafe { std::mem::zeroed() });
        st.version = version::<DrsSetting>(1);
        match unsafe { (self.api().get_setting)(self.h, p, id, &mut *st) } {
            SETTING_NOT_FOUND => Ok(Read { value: None, user: None }),
            s => {
                check(s)?;
                if st.kind != TYPE_DWORD {
                    return Ok(Read { value: None, user: None });
                }
                let v = st.current[0];
                let mine = st.location == LOCATION_CURRENT_PROFILE && st.is_current_predefined == 0;
                Ok(Read { value: Some(v), user: mine.then_some(v) })
            }
        }
    }

    fn set(&self, p: Handle, id: u32, value: u32) -> Result<(), GpuError> {
        // SAFETY: zeroed POD with version, id, dword type and value set.
        let mut st: Box<DrsSetting> = Box::new(unsafe { std::mem::zeroed() });
        st.version = version::<DrsSetting>(1);
        st.id = id;
        st.kind = TYPE_DWORD;
        st.current[0] = value;
        check(unsafe { (self.api().set_setting)(self.h, p, &mut *st) })
    }

    /// Drop the user value: back to the driver default or NVIDIA's predefined value.
    fn delete(&self, p: Handle, id: u32) -> Result<(), GpuError> {
        // SAFETY: valid handles.
        match unsafe { (self.api().delete_setting)(self.h, p, id) } {
            SETTING_NOT_FOUND => Ok(()),
            st => check(st),
        }
    }

    fn save(&self) -> Result<(), GpuError> {
        // SAFETY: valid session.
        check(unsafe { (self.api().save_settings)(self.h) })
    }

    fn view(&self, target: &Target, backups: &Backups) -> Result<ProfileView, GpuError> {
        let (p, profile) = match target {
            Target::Global => {
                let p = self.global()?;
                (p, self.profile_name(p))
            }
            Target::Game(exe) => match self.find_app(exe)? {
                Some(p) => (p, self.profile_name(p)),
                // No profile yet: the game runs with the global values.
                None => (self.global()?, None),
            },
        };
        let own = !matches!(target, Target::Game(_)) || profile.is_some();
        let key = target.key();
        let mut settings = Vec::with_capacity(CATALOG.len());
        for d in CATALOG {
            let (effective, user_set) = self.effective(p, d)?;
            let value = read_value(d, &effective);
            settings.push(SettingView {
                id: d.id,
                raw: effective.iter().map(|v| format!("0x{v:X}")).collect::<Vec<_>>().join(" "),
                good: is_fine(d, value.as_deref()),
                value,
                options: match &d.kind {
                    Kind::Choice(o) => o.iter().map(|(k, _)| *k).collect(),
                    Kind::Fps => Vec::new(),
                },
                recommended: d.recommended,
                user_set: own && user_set,
                backed_up: d.drs.iter().any(|x| backups.prev(&key, x.id).is_some()),
            });
        }
        Ok(ProfileView { profile, settings })
    }

    /// Effective dwords of a setting in profile `p` (defaults filled in), and whether any of
    /// them is the user's own value there.
    fn effective(&self, p: Handle, d: &Def) -> Result<(Vec<u32>, bool), GpuError> {
        let mut out = Vec::with_capacity(d.drs.len());
        let mut user = false;
        for x in d.drs {
            let r = self.get(p, x.id)?;
            out.push(r.value.unwrap_or(x.default));
            user |= r.user.is_some();
        }
        Ok((out, user))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: the session created in `open`; Init unloads after this.
        unsafe { (self.api().destroy_session)(self.h) };
    }
}

/* ---------------- backups ---------------- */

const BACKUP_FILE: &str = "gpu_settings_backup.json";
const MAX_ENTRIES: usize = 4096;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
struct Backups {
    entries: Vec<BackupEntry>,
    /// Profiles we created (game key); deleted when their last setting is reverted.
    #[serde(default)]
    created: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct BackupEntry {
    /// "global" or the game's exe in lower case.
    target: String,
    drs: u32,
    /// The user's value before our first change; `None` = there was none (delete to restore).
    prev: Option<u32>,
}

fn valid_key(k: &str) -> bool {
    k == "global" || valid_exe(k)
}

impl Backups {
    fn path() -> PathBuf {
        registry::data_dir().join(BACKUP_FILE)
    }

    /// The file is hostile: unknown ids, bad keys and out-of-range values are dropped (a dropped
    /// entry reverts to the driver default instead of an arbitrary value).
    fn parse(text: &str) -> Self {
        let Ok(mut b) = serde_json::from_str::<Backups>(text) else {
            return Self::default();
        };
        b.entries.retain(|e| {
            valid_key(&e.target) && known_drs(e.drs).is_some_and(|x| e.prev.is_none_or(|v| (x.valid)(v)))
        });
        b.entries.truncate(MAX_ENTRIES);
        b.created.retain(|k| k != "global" && valid_exe(k));
        b.created.truncate(MAX_ENTRIES);
        b
    }

    fn load() -> Self {
        std::fs::read_to_string(Self::path()).map(|t| Self::parse(&t)).unwrap_or_default()
    }

    fn save(&self) -> Result<(), GpuError> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|_| GpuError::Io)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|_| GpuError::Io)?;
        std::fs::write(path, json).map_err(|_| GpuError::Io)
    }

    fn prev(&self, key: &str, drs: u32) -> Option<&BackupEntry> {
        self.entries.iter().find(|e| e.target == key && e.drs == drs)
    }

    /// Keep the first previous value only: a second change must not overwrite the original.
    fn remember(&mut self, key: &str, drs: u32, prev: Option<u32>) {
        if self.prev(key, drs).is_none() {
            self.entries.push(BackupEntry { target: key.to_string(), drs, prev });
        }
    }

    fn take(&mut self, key: &str, drs: u32) -> Option<Option<u32>> {
        let i = self.entries.iter().position(|e| e.target == key && e.drs == drs)?;
        Some(self.entries.remove(i).prev)
    }
}

/* ---------------- operations ---------------- */

/// Driver version and branch (no DRS session needed).
pub fn driver() -> Option<DriverInfo> {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let init = Init::open().ok()?;
    let mut ver = 0u32;
    let mut branch = [0u8; 64]; // NvAPI_ShortString
    // SAFETY: out-params sized as the header declares.
    if unsafe { (init.0.driver_branch)(&mut ver, branch.as_mut_ptr()) } != OK {
        return None;
    }
    let end = branch.iter().position(|&b| b == 0).unwrap_or(branch.len());
    Some(DriverInfo {
        version: format!("{}.{:02}", ver / 100, ver % 100),
        branch: String::from_utf8_lossy(&branch[..end]).into_owned(),
    })
}

pub fn read(target: &Target) -> Result<ProfileView, GpuError> {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    Session::open()?.view(target, &Backups::load())
}

/// Write `changes` (setting, option) to the target's profile, creating the game's profile when
/// it has none. `skip_fine`: leave settings already at a good value alone.
fn write(target: &Target, changes: &[(&'static Def, String)], skip_fine: bool) -> Result<ProfileView, GpuError> {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let s = Session::open()?;
    let key = target.key();
    let before = Backups::load();
    let mut b = before.clone();
    let existing = match target {
        Target::Global => Some(s.global()?),
        Target::Game(exe) => s.find_app(exe)?,
    };
    let mut writes = Vec::new();
    for (d, v) in changes {
        if skip_fine {
            let (eff, _) = s.effective(existing.map_or_else(|| s.global(), Ok)?, d)?;
            if is_fine(d, read_value(d, &eff).as_deref()) {
                continue;
            }
        }
        writes.extend(resolve(d, v)?);
    }
    if writes.is_empty() {
        return s.view(target, &b);
    }
    let p = match (existing, target) {
        (Some(p), _) => p,
        (None, Target::Game(exe)) => {
            let p = s.create_for(exe)?;
            if !b.created.contains(&key) {
                b.created.push(key.clone());
            }
            p
        }
        (None, Target::Global) => unreachable!("the global profile always exists"),
    };
    for (x, v) in writes {
        if b.prev(&key, x.id).is_none() {
            b.remember(&key, x.id, s.get(p, x.id)?.user);
        }
        s.set(p, x.id, v)?;
    }
    // Backup first: if the save fails, put the file back as it was.
    b.save()?;
    if let Err(e) = s.save() {
        let _ = before.save();
        return Err(e);
    }
    s.view(target, &b)
}

pub fn apply(target: &Target, id: &str, value: &str) -> Result<ProfileView, GpuError> {
    let d = def(id)?;
    resolve(d, value)?; // reject before opening a session
    write(target, &[(d, value.to_string())], false)
}

/// Every recommended value that is not already good.
pub fn apply_recommended(target: &Target) -> Result<ProfileView, GpuError> {
    let changes: Vec<_> = CATALOG.iter().filter_map(|d| d.recommended.map(|r| (d, r.to_string()))).collect();
    write(target, &changes, true)
}

/// Bring setting `id` back: the saved previous value, or no user value at all.
pub fn revert(target: &Target, id: &str) -> Result<ProfileView, GpuError> {
    let d = def(id)?;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let s = Session::open()?;
    let key = target.key();
    let before = Backups::load();
    let mut b = before.clone();
    let p = match target {
        Target::Global => s.global()?,
        Target::Game(exe) => match s.find_app(exe)? {
            Some(p) => p,
            None => return s.view(target, &b),
        },
    };
    for x in d.drs {
        match b.take(&key, x.id) {
            Some(Some(v)) => s.set(p, x.id, v)?,
            _ => s.delete(p, x.id)?,
        }
    }
    // Our own profile with nothing left to restore goes away with its last setting.
    if b.created.contains(&key) && !b.entries.iter().any(|e| e.target == key) {
        // SAFETY: valid session and profile handle.
        check(unsafe { (s.api().delete_profile)(s.h, p) })?;
        b.created.retain(|k| *k != key);
    }
    b.save()?;
    if let Err(e) = s.save() {
        let _ = before.save();
        return Err(e);
    }
    s.view(target, &b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_versions_match_the_header() {
        // NVDRS_SETTING_VER / NVDRS_PROFILE_VER / NVDRS_APPLICATION_VER_V4 as the SDK computes them.
        assert_eq!(version::<DrsSetting>(1), 0x0001_3020);
        assert_eq!(version::<DrsProfile>(1), 0x0001_1014);
        assert_eq!(version::<DrsApplication>(4), 0x0004_500C);
    }

    #[test]
    fn values_resolve_only_from_the_allowlist() {
        let ll = def("low_latency").unwrap();
        let w = resolve(ll, "ultra").unwrap();
        assert_eq!(w.iter().map(|(x, v)| (x.id, *v)).collect::<Vec<_>>(), vec![(0x007B_A09E, 1), (0x0005_F543, 2), (0x1083_5000, 1)]);
        assert_eq!(resolve(ll, "turbo").err(), Some(GpuError::BadValue));
        assert_eq!(def("0x1057EB71").err(), Some(GpuError::UnknownSetting));
        let fps = def("fps_limit").unwrap();
        assert_eq!(resolve(fps, "0").unwrap()[0].1, 0);
        assert_eq!(resolve(fps, "141").unwrap()[0].1, 141);
        for bad in ["5", "1001", "-1", "1e3", "", "60 "] {
            assert_eq!(resolve(fps, bad).err(), Some(GpuError::BadValue), "{bad}");
        }
        // Every option of every setting passes its own DRS range check.
        for d in CATALOG {
            if let Kind::Choice(opts) = &d.kind {
                for (k, vals) in opts.iter() {
                    assert_eq!(vals.len(), d.drs.len(), "{} {k}", d.id);
                    assert!(d.drs.iter().zip(vals.iter()).all(|(x, v)| (x.valid)(*v)), "{} {k}", d.id);
                }
            }
            assert!(d.recommended.is_none_or(|r| resolve(d, r).is_ok()), "{}", d.id);
        }
    }

    #[test]
    fn reading_maps_driver_values_to_options() {
        let ll = def("low_latency").unwrap();
        assert_eq!(read_value(ll, &[0, 0, 0]).as_deref(), Some("off"));
        assert_eq!(read_value(ll, &[1, 1, 0]).as_deref(), Some("on"));
        assert_eq!(read_value(ll, &[1, 0, 0]).as_deref(), Some("on"));
        assert_eq!(read_value(ll, &[1, 2, 1]).as_deref(), Some("ultra"));
        assert_eq!(read_value(ll, &[3, 0, 0]), None);
        assert_eq!(read_value(def("vsync").unwrap(), &[VSYNC_APP]).as_deref(), Some("app"));
        assert_eq!(read_value(def("power").unwrap(), &[2]), None);
        assert_eq!(read_value(def("fps_limit").unwrap(), &[141]).as_deref(), Some("141"));
    }

    #[test]
    fn exe_names_are_bare_file_names() {
        for ok in ["cs2.exe", "FC27.EXE", "League of Legends.exe"] {
            assert!(valid_exe(ok), "{ok}");
        }
        for bad in ["cs2", ".exe", "..\\cs2.exe", "C:\\g\\cs2.exe", "a/b.exe", "x\u{202e}.exe".replace('\u{202e}', "\n").as_str(), "a?.exe"] {
            assert!(!valid_exe(bad), "{bad}");
        }
        assert_eq!(Target::game("cs2.exe").unwrap().key(), "cs2.exe");
        assert_eq!(Target::game("CS2.EXE").unwrap().key(), "cs2.exe");
        assert_eq!(Target::game("x"), Err(GpuError::BadExe));
    }

    #[test]
    fn hostile_backup_files_are_filtered() {
        let text = r#"{"entries":[
            {"target":"global","drs":4294967295,"prev":1},
            {"target":"global","drs":274197361,"prev":99},
            {"target":"global","drs":274197361,"prev":1},
            {"target":"..\\evil.exe","drs":8102046,"prev":1},
            {"target":"cs2.exe","drs":8102046,"prev":null}
        ],"created":["global","cs2.exe","C:\\x.exe"]}"#;
        let b = Backups::parse(text);
        assert_eq!(
            b.entries,
            vec![
                BackupEntry { target: "global".into(), drs: 0x1057_EB71, prev: Some(1) },
                BackupEntry { target: "cs2.exe".into(), drs: 0x007B_A09E, prev: None },
            ]
        );
        assert_eq!(b.created, vec!["cs2.exe".to_string()]);
        assert_eq!(Backups::parse("not json"), Backups::default());
    }

    #[test]
    fn backups_keep_the_first_previous_value() {
        let mut b = Backups::default();
        b.remember("global", 1, Some(5));
        b.remember("global", 1, Some(1));
        assert_eq!(b.prev("global", 1).unwrap().prev, Some(5));
        assert_eq!(b.take("global", 1), Some(Some(5)));
        assert_eq!(b.take("global", 1), None);
    }

    #[test]
    fn backup_file_round_trip() {
        // SAFETY: same value every data-dir test sets; see `registry::tests::test_data_dir`.
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
        let mut b = Backups::default();
        b.remember("cs2.exe", 0x1057_EB71, None);
        b.created.push("cs2.exe".into());
        b.save().unwrap();
        assert_eq!(Backups::load(), b);
        std::fs::remove_file(Backups::path()).unwrap();
    }

    /// Live, read-only: the global profile on this machine's NVIDIA driver.
    #[test]
    #[ignore]
    fn live_read_global_profile() {
        println!("driver: {:?}", driver());
        let v = read(&Target::Global).expect("DRS read");
        println!("profile: {:?}", v.profile);
        for s in &v.settings {
            println!("{:16} value={:?} raw={} user_set={} good={}", s.id, s.value, s.raw, s.user_set, s.good);
        }
        // A game's profile by exe (NVIDIA ships predefined ones); reading never creates a profile.
        for exe in ["cs2.exe", "g2boost-no-such-game.exe"] {
            let g = read(&Target::game(exe).unwrap()).expect("DRS read by exe");
            let summary: Vec<_> = g.settings.iter().map(|s| format!("{}={:?}{}", s.id, s.value, if s.user_set { "*" } else { "" })).collect();
            println!("{exe}: profile={:?} {}", g.profile, summary.join(" "));
        }
    }

    /// Live WRITE, manual only (`G2BOOST_LIVE_WRITE=1` + `--ignored`): frame cap on the global
    /// profile read → set 1000 → read → revert → read. Backups go to the test data dir.
    #[test]
    #[ignore]
    fn live_round_trip_fps_limit() {
        if std::env::var("G2BOOST_LIVE_WRITE").as_deref() != Ok("1") {
            return;
        }
        // SAFETY: same value every data-dir test sets; see `registry::tests::test_data_dir`.
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
        let fps = |v: &ProfileView| v.settings.iter().find(|s| s.id == "fps_limit").cloned().unwrap();
        let before = fps(&read(&Target::Global).unwrap());
        println!("before: {before:?}");
        let set = fps(&apply(&Target::Global, "fps_limit", "1000").expect("apply"));
        println!("set:    {set:?}");
        assert_eq!(set.value.as_deref(), Some("1000"));
        assert!(set.user_set && set.backed_up);
        let back = fps(&revert(&Target::Global, "fps_limit").expect("revert"));
        println!("after:  {back:?}");
        assert_eq!(back.value, before.value);
        assert_eq!(back.user_set, before.user_set);
        assert!(!back.backed_up);
    }

    /// Live, read-only: what the driver itself calls our ids and which values it lists.
    #[test]
    #[ignore]
    fn live_probe_names_and_values() {
        type NameFn = unsafe extern "C" fn(u32, *mut [u16; USTR]) -> Status;
        let api = api().expect("nvapi");
        let _init = Init::open().unwrap();
        // SAFETY: ids from nvapi_interface.h (GetSettingNameFromId, EnumAvailableSettingValues).
        let name_fn: NameFn = unsafe { std::mem::transmute((api.qi)(0xd61c_be6e)) };
        let values_fn: unsafe extern "C" fn(u32, *mut u32, *mut c_void) -> Status =
            unsafe { std::mem::transmute((api.qi)(0x2ec3_9f90)) };
        for d in CATALOG {
            for x in d.drs {
                let mut name = [0u16; USTR];
                let st = unsafe { name_fn(x.id, &mut name) };
                // NVDRS_SETTING_VALUES: version, count, type, default union (4100), 100 × 4100.
                let mut buf = vec![0u32; (12 + 4100 + 100 * 4100) / 4];
                let size = (buf.len() * 4) as u32;
                buf[0] = size | (1 << 16);
                let mut n = 100u32;
                let vs = unsafe { values_fn(x.id, &mut n, buf.as_mut_ptr().cast()) };
                let vals: Vec<String> = (0..n.min(100) as usize).map(|i| format!("0x{:X}", buf[3 + 1025 + i * 1025])).collect();
                println!(
                    "{} 0x{:08X} name[{st}]={:?} default=0x{:X} values[{vs}]={:?}",
                    d.id,
                    x.id,
                    from_ustr(&name),
                    buf[3],
                    vals
                );
            }
        }
    }
}
