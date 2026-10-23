//! AMD Radeon driver 3D settings through ADLX. **Beta: written against the SDK headers, never run
//! on an AMD card** (the development PC has an NVIDIA GPU).
//!
//! `amdadlx64.dll` ships with the AMD driver and is loaded from System32 only. The C vtables are
//! bound exactly as the official SDK declares them (github.com/GPUOpen-LibrariesAndSDKs/ADLX, MIT,
//! commit 32b5a74, 2026-09-21: `ADLX.h`, `ADLXDefines.h`, `ISystem.h`, `I3DSettings.h`,
//! `ICollections.h`, `ADLXStructures.h`). A wrong slot on an untested driver would crash the
//! process, so **every ADLX call runs in a helper process**: our own executable started with
//! `--gpu-helper <request json>` (fixed flag, no shell), which prints one JSON line and exits.
//! The parent validates the request before spawning, kills the helper after a timeout and treats
//! a crash or a malformed answer as "could not read the AMD card".

use std::ffi::{CStr, c_char, c_void};
use std::io::Read as _;
use std::mem::size_of;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::ptr::null_mut;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};
use windows::core::{PCWSTR, s};

use crate::registry::wide;

/// The command-line flag that turns `G2 Boost.exe` into the helper.
pub const HELPER_FLAG: &str = "--gpu-helper";
const HELPER_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_REQUEST: usize = 1024;
const MAX_ANSWER: u64 = 64 * 1024;
const MAX_GPUS: usize = 8;
const MAX_CHANGES: usize = 6;

/* ---------------- protocol (pure, unit-tested) ---------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    AntiLag,
    Chill,
    Boost,
    Sharpening,
    EnhancedSync,
    /// Frame Rate Target Control (a frame cap).
    Frtc,
}

impl Feature {
    pub const ALL: [Feature; 6] =
        [Feature::AntiLag, Feature::Chill, Feature::Boost, Feature::Sharpening, Feature::EnhancedSync, Feature::Frtc];

    /// Suggested state for games: Anti-Lag on; Chill, Boost (lower resolution), Enhanced Sync and the
    /// frame cap off; sharpening is taste (`None`).
    pub fn recommended(self) -> Option<bool> {
        match self {
            Feature::AntiLag => Some(true),
            Feature::Chill | Feature::Boost | Feature::EnhancedSync | Feature::Frtc => Some(false),
            Feature::Sharpening => None,
        }
    }

    /// Allowed range for `value` (sharpness %, frame cap fps); `None` = toggle only.
    fn value_range(self) -> Option<(i32, i32)> {
        match self {
            Feature::Sharpening => Some((0, 100)),
            Feature::Frtc => Some((20, 1000)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cmd {
    Read,
    Set,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub feature: Feature,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub value: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub cmd: Cmd,
    /// Index in ADLX's GPU list (set only).
    #[serde(default)]
    pub gpu: u32,
    #[serde(default)]
    pub changes: Vec<Change>,
}

impl Request {
    pub fn read() -> Self {
        Self { cmd: Cmd::Read, gpu: 0, changes: Vec::new() }
    }

    /// Both sides call this: the parent before spawning, the helper before touching ADLX.
    pub fn validate(&self) -> Result<(), AmdError> {
        let bad = match self.cmd {
            Cmd::Read => self.gpu != 0 || !self.changes.is_empty(),
            Cmd::Set => {
                self.gpu as usize >= MAX_GPUS
                    || self.changes.is_empty()
                    || self.changes.len() > MAX_CHANGES
                    || self.changes.iter().enumerate().any(|(i, c)| {
                        self.changes[..i].iter().any(|o| o.feature == c.feature)
                            || (c.enabled.is_none() && c.value.is_none())
                            || match (c.value, c.feature.value_range()) {
                                (None, _) => false,
                                (Some(_), None) => true,
                                (Some(v), Some((lo, hi))) => !(lo..=hi).contains(&v),
                            }
                    })
            }
        };
        if bad { Err(AmdError::BadRequest) } else { Ok(()) }
    }

    pub fn parse(text: &str) -> Result<Self, AmdError> {
        if text.len() > MAX_REQUEST {
            return Err(AmdError::BadRequest);
        }
        let r: Request = serde_json::from_str(text).map_err(|_| AmdError::BadRequest)?;
        r.validate()?;
        Ok(r)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeatureState {
    pub feature: Feature,
    pub supported: bool,
    pub enabled: bool,
    /// Sharpness % / frame cap fps.
    #[serde(default)]
    pub value: Option<i32>,
    /// What the driver accepts for `value` (min, max).
    #[serde(default)]
    pub range: Option<(i32, i32)>,
    /// Filled by the parent from `Feature::recommended`.
    #[serde(default)]
    pub recommended: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AmdGpu {
    pub index: u32,
    pub name: String,
    pub discrete: bool,
    pub features: Vec<FeatureState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub gpus: Vec<AmdGpu>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum AmdError {
    /// No AMD driver with ADLX on this PC.
    #[error("no_amd")]
    NotFound,
    /// The helper crashed, hung or answered garbage.
    #[error("helper_failed")]
    HelperFailed,
    #[error("bad_request")]
    BadRequest,
    #[error("adlx_{0}")]
    Adlx(i32),
}

fn clean_name(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(120).collect::<String>().trim().to_string()
}

/// The helper's stdout is hostile too (it ran foreign code): bounded, shape-checked, cleaned.
pub fn parse_answer(text: &str) -> Result<Vec<AmdGpu>, AmdError> {
    let line = text.lines().find(|l| !l.trim().is_empty()).ok_or(AmdError::HelperFailed)?;
    let a: Answer = serde_json::from_str(line).map_err(|_| AmdError::HelperFailed)?;
    if !a.ok {
        return Err(match a.error.as_deref() {
            Some("no_amd") => AmdError::NotFound,
            Some(e) if e.starts_with("adlx_") => AmdError::Adlx(e[5..].parse().unwrap_or(-1)),
            _ => AmdError::HelperFailed,
        });
    }
    if a.gpus.len() > MAX_GPUS {
        return Err(AmdError::HelperFailed);
    }
    let mut gpus = a.gpus;
    for (i, g) in gpus.iter_mut().enumerate() {
        g.index = i as u32;
        g.name = clean_name(&g.name);
        g.features.truncate(Feature::ALL.len());
        for f in &mut g.features {
            f.recommended = f.feature.recommended();
            if f.range.is_some_and(|(lo, hi)| lo > hi) {
                f.range = None;
            }
        }
    }
    Ok(gpus)
}

/* ---------------- parent side ---------------- */

/// Run `req` in a helper process (our own exe, fixed flag). Read = every GPU's state; set = apply
/// the changes and return the new state.
pub fn call(req: &Request) -> Result<Vec<AmdGpu>, AmdError> {
    req.validate()?;
    let json = serde_json::to_string(req).map_err(|_| AmdError::BadRequest)?;
    let exe = std::env::current_exe().map_err(|_| AmdError::HelperFailed)?;
    let mut child = Command::new(exe)
        .args([HELPER_FLAG, &json])
        .creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW.0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| AmdError::HelperFailed)?;
    let mut out = child.stdout.take().ok_or(AmdError::HelperFailed)?;
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = (&mut out).take(MAX_ANSWER).read_to_string(&mut buf);
        buf
    });
    let deadline = Instant::now() + HELPER_TIMEOUT;
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|_| AmdError::HelperFailed)? {
            break s;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AmdError::HelperFailed);
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let text = reader.join().unwrap_or_default();
    if !status.success() {
        return Err(AmdError::HelperFailed); // crashed (access violation exits non-zero)
    }
    parse_answer(&text)
}

/* ---------------- helper side ---------------- */

/// Entry point of `G2 Boost.exe --gpu-helper <json>`: the JSON line to print.
pub fn helper_main(arg: &str) -> String {
    let answer = match Request::parse(arg).and_then(|r| run(&r)) {
        Ok(gpus) => Answer { ok: true, error: None, gpus },
        Err(e) => Answer { ok: false, error: Some(e.to_string()), gpus: Vec::new() },
    };
    serde_json::to_string(&answer).unwrap_or_else(|_| r#"{"ok":false,"error":"helper_failed"}"#.into())
}

/// When this process was started as the helper: answer and return `true` (the caller exits).
pub fn helper_if_requested() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let Some(i) = args.iter().position(|a| a == HELPER_FLAG) else {
        return false;
    };
    use std::io::Write as _;
    let line = helper_main(args.get(i + 1).map(String::as_str).unwrap_or(""));
    let _ = writeln!(std::io::stdout(), "{line}");
    true
}

// ---- C vtables (ADLX C API; slots we never call are kept as `usize` to hold the layout) ----

type Res = i32; // ADLX_RESULT
const ADLX_OK: Res = 0;
const ADLX_ALREADY_ENABLED: Res = 1;
const ADLX_ALREADY_INITIALIZED: Res = 2;
const GPUTYPE_DISCRETE: i32 = 2;
/// SDK version the vtables below come from (ADLXVersion.h 2.0.0.125).
const SDK_FULL_VERSION: u64 = (2 << 48) | 125;

#[repr(C)]
struct Obj<V> {
    vtbl: *const V,
}

/// First slots of every reference-counted ADLX interface.
#[repr(C)]
struct Base {
    acquire: usize,
    release: unsafe extern "system" fn(*mut c_void) -> i32,
    query_interface: usize,
}

#[repr(C)]
struct SystemVtbl {
    get_hybrid_graphics_type: usize,
    get_gpus: unsafe extern "system" fn(*mut Obj<SystemVtbl>, *mut *mut Obj<GpuListVtbl>) -> Res,
    query_interface: usize,
    get_displays_services: usize,
    get_desktops_services: usize,
    get_gpus_changed_handling: usize,
    enable_log: usize,
    get_3d_settings_services: unsafe extern "system" fn(*mut Obj<SystemVtbl>, *mut *mut Obj<Services3dVtbl>) -> Res,
}

#[repr(C)]
struct GpuListVtbl {
    base: Base,
    size: unsafe extern "system" fn(*mut Obj<GpuListVtbl>) -> u32,
    empty: usize,
    begin: usize,
    end: usize,
    at: usize,
    clear: usize,
    remove_back: usize,
    add_back: usize,
    at_gpu_list: unsafe extern "system" fn(*mut Obj<GpuListVtbl>, u32, *mut *mut Obj<GpuVtbl>) -> Res,
}

#[repr(C)]
struct GpuVtbl {
    base: Base,
    vendor_id: usize,
    asic_family_type: usize,
    gpu_type: unsafe extern "system" fn(*mut Obj<GpuVtbl>, *mut i32) -> Res,
    is_external: usize,
    name: unsafe extern "system" fn(*mut Obj<GpuVtbl>, *mut *const c_char) -> Res,
}

type Getter = unsafe extern "system" fn(*mut c_void, *mut Obj<GpuVtbl>, *mut *mut c_void) -> Res;

#[repr(C)]
struct Services3dVtbl {
    base: Base,
    get_anti_lag: Getter,
    get_chill: Getter,
    get_boost: Getter,
    get_image_sharpening: Getter,
    get_enhanced_sync: Getter,
    get_wait_for_vertical_refresh: usize,
    get_frame_rate_target_control: Getter,
}

type GetBool = unsafe extern "system" fn(*mut c_void, *mut u8) -> Res;
type SetBool = unsafe extern "system" fn(*mut c_void, u8) -> Res;
type GetInt = unsafe extern "system" fn(*mut c_void, *mut i32) -> Res;
type SetInt = unsafe extern "system" fn(*mut c_void, i32) -> Res;
type GetRange = unsafe extern "system" fn(*mut c_void, *mut IntRange) -> Res;

/// `ADLX_IntRange`.
#[repr(C)]
#[derive(Default)]
struct IntRange {
    min: i32,
    max: i32,
    step: i32,
}

/// IADLX3DAntiLag, IADLX3DEnhancedSync.
#[repr(C)]
struct SimpleVtbl {
    base: Base,
    is_supported: GetBool,
    is_enabled: GetBool,
    set_enabled: SetBool,
}

/// IADLX3DBoost, IADLX3DImageSharpening, IADLX3DFrameRateTargetControl: same slot order
/// (supported, enabled, range, value, set enabled, set value).
#[repr(C)]
struct RangedVtbl {
    base: Base,
    is_supported: GetBool,
    is_enabled: GetBool,
    get_range: GetRange,
    get_value: GetInt,
    set_enabled: SetBool,
    set_value: SetInt,
}

/// IADLX3DChill.
#[repr(C)]
struct ChillVtbl {
    base: Base,
    is_supported: GetBool,
    is_enabled: GetBool,
    get_fps_range: GetRange,
    get_min_fps: GetInt,
    get_max_fps: GetInt,
    set_enabled: SetBool,
}

const _: () = {
    let p = size_of::<usize>();
    assert!(size_of::<SystemVtbl>() == 8 * p);
    assert!(size_of::<GpuListVtbl>() == 12 * p);
    assert!(size_of::<GpuVtbl>() == 8 * p); // ... Name is slot 7
    assert!(size_of::<Services3dVtbl>() == 10 * p); // ... GetFrameRateTargetControl is slot 9
    assert!(size_of::<SimpleVtbl>() == 6 * p);
    assert!(size_of::<RangedVtbl>() == 9 * p);
    assert!(size_of::<ChillVtbl>() == 9 * p);
};

fn ok(r: Res) -> Result<(), AmdError> {
    if matches!(r, ADLX_OK | ADLX_ALREADY_ENABLED | ADLX_ALREADY_INITIALIZED) { Ok(()) } else { Err(AmdError::Adlx(r)) }
}

/// Releases one reference-counted interface on drop.
struct Owned(*mut c_void);

impl Owned {
    fn new(p: *mut c_void) -> Result<Self, AmdError> {
        if p.is_null() { Err(AmdError::Adlx(-1)) } else { Ok(Self(p)) }
    }

    /// SAFETY: the caller names the vtable type this interface really has.
    unsafe fn vtbl<V>(&self) -> &V {
        unsafe { &*(*(self.0 as *mut Obj<V>)).vtbl }
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: every interface we own starts with IADLXInterface (Acquire, Release, QI).
        unsafe { (self.vtbl::<Base>().release)(self.0) };
    }
}

fn run(req: &Request) -> Result<Vec<AmdGpu>, AmdError> {
    let name = wide("amdadlx64.dll");
    // SAFETY: System32-only load of AMD's ADLX; every pointer below comes from ADLX and is used
    // with the vtable layout the SDK headers declare for that interface.
    unsafe {
        let h = LoadLibraryExW(PCWSTR(name.as_ptr()), None, LOAD_LIBRARY_SEARCH_SYSTEM32).map_err(|_| AmdError::NotFound)?;
        type QueryFull = unsafe extern "C" fn(*mut u64) -> Res;
        type Init = unsafe extern "C" fn(u64, *mut *mut Obj<SystemVtbl>) -> Res;
        type Init2 = unsafe extern "C" fn(u64, *mut *mut Obj<SystemVtbl>, *mut *mut c_void) -> Res;
        type Terminate = unsafe extern "C" fn() -> Res;
        type Proc = unsafe extern "system" fn() -> isize; // FARPROC
        let terminate: Terminate =
            std::mem::transmute(GetProcAddress(h, s!("ADLXTerminate")).ok_or(AmdError::NotFound)?);
        // Ask for the older of (driver, SDK): the interfaces used here exist since ADLX 1.0.
        let mut driver_ver = SDK_FULL_VERSION;
        if let Some(f) = GetProcAddress(h, s!("ADLXQueryFullVersion")) {
            let f: QueryFull = std::mem::transmute(f);
            let mut v = 0u64;
            if f(&mut v) == ADLX_OK && v != 0 {
                driver_ver = v.min(SDK_FULL_VERSION);
            }
        }
        let mut system: *mut Obj<SystemVtbl> = null_mut();
        let r = if let Some(f) = GetProcAddress(h, s!("ADLXInitialize")) {
            std::mem::transmute::<Proc, Init>(f)(driver_ver, &mut system)
        } else if let Some(f) = GetProcAddress(h, s!("ADLXInitialize2")) {
            let mut mapping = null_mut();
            std::mem::transmute::<Proc, Init2>(f)(driver_ver, &mut system, &mut mapping)
        } else {
            return Err(AmdError::NotFound);
        };
        ok(r)?;
        if system.is_null() {
            return Err(AmdError::Adlx(-1));
        }
        let result = with_system(system, req);
        terminate();
        result
    }
}

/// SAFETY: `system` is a live IADLXSystem from ADLXInitialize (not reference counted).
unsafe fn with_system(system: *mut Obj<SystemVtbl>, req: &Request) -> Result<Vec<AmdGpu>, AmdError> {
    unsafe {
        let sys = &*(*system).vtbl;
        let mut list = null_mut();
        ok((sys.get_gpus)(system, &mut list))?;
        let list = Owned::new(list.cast())?;
        let lv = list.vtbl::<GpuListVtbl>();
        let mut svc = null_mut();
        ok((sys.get_3d_settings_services)(system, &mut svc))?;
        let svc = Owned::new(svc.cast())?;
        let n = (lv.size)(list.0.cast()).min(MAX_GPUS as u32);
        let mut out = Vec::new();
        for i in 0..n {
            let mut gp = null_mut();
            if (lv.at_gpu_list)(list.0.cast(), i, &mut gp) != ADLX_OK {
                continue;
            }
            let gpu = Owned::new(gp.cast())?;
            if req.cmd == Cmd::Set && i == req.gpu {
                for c in &req.changes {
                    set_feature(&svc, &gpu, c)?;
                }
            }
            out.push(read_gpu(&svc, &gpu, i));
        }
        Ok(out)
    }
}

/// SAFETY: live services + GPU interfaces.
unsafe fn feature_obj(svc: &Owned, gpu: &Owned, f: Feature) -> Option<Owned> {
    unsafe {
        let v = svc.vtbl::<Services3dVtbl>();
        let getter = match f {
            Feature::AntiLag => v.get_anti_lag,
            Feature::Chill => v.get_chill,
            Feature::Boost => v.get_boost,
            Feature::Sharpening => v.get_image_sharpening,
            Feature::EnhancedSync => v.get_enhanced_sync,
            Feature::Frtc => v.get_frame_rate_target_control,
        };
        let mut p = null_mut();
        if getter(svc.0, gpu.0.cast(), &mut p) != ADLX_OK {
            return None;
        }
        Owned::new(p).ok()
    }
}

/// SAFETY: live services + GPU interfaces.
unsafe fn read_gpu(svc: &Owned, gpu: &Owned, index: u32) -> AmdGpu {
    unsafe {
        let gv = gpu.vtbl::<GpuVtbl>();
        let mut name: *const c_char = std::ptr::null();
        let name = if (gv.name)(gpu.0.cast(), &mut name) == ADLX_OK && !name.is_null() {
            clean_name(&CStr::from_ptr(name).to_string_lossy())
        } else {
            String::new()
        };
        let mut kind = 0i32;
        let discrete = (gv.gpu_type)(gpu.0.cast(), &mut kind) == ADLX_OK && kind == GPUTYPE_DISCRETE;
        let features = Feature::ALL.iter().map(|&f| read_feature(svc, gpu, f)).collect();
        AmdGpu { index, name, discrete, features }
    }
}

/// SAFETY: live services + GPU interfaces.
unsafe fn read_feature(svc: &Owned, gpu: &Owned, f: Feature) -> FeatureState {
    let mut st = FeatureState { feature: f, supported: false, enabled: false, value: None, range: None, recommended: None };
    unsafe {
        let Some(o) = feature_obj(svc, gpu, f) else {
            return st;
        };
        let (mut sup, mut on) = (0u8, 0u8);
        match f {
            Feature::AntiLag | Feature::EnhancedSync => {
                let v = o.vtbl::<SimpleVtbl>();
                let _ = (v.is_supported)(o.0, &mut sup);
                if sup != 0 {
                    let _ = (v.is_enabled)(o.0, &mut on);
                }
            }
            Feature::Chill => {
                let v = o.vtbl::<ChillVtbl>();
                let _ = (v.is_supported)(o.0, &mut sup);
                if sup != 0 {
                    let _ = (v.is_enabled)(o.0, &mut on);
                }
            }
            Feature::Boost | Feature::Sharpening | Feature::Frtc => {
                let v = o.vtbl::<RangedVtbl>();
                let _ = (v.is_supported)(o.0, &mut sup);
                if sup != 0 {
                    let _ = (v.is_enabled)(o.0, &mut on);
                    let mut r = IntRange::default();
                    let mut cur = 0i32;
                    if (v.get_range)(o.0, &mut r) == ADLX_OK {
                        st.range = Some((r.min, r.max));
                    }
                    if (v.get_value)(o.0, &mut cur) == ADLX_OK {
                        st.value = Some(cur);
                    }
                }
            }
        }
        st.supported = sup != 0;
        st.enabled = on != 0;
    }
    st
}

/// SAFETY: live services + GPU interfaces; `c` passed `Request::validate`.
unsafe fn set_feature(svc: &Owned, gpu: &Owned, c: &Change) -> Result<(), AmdError> {
    unsafe {
        let o = feature_obj(svc, gpu, c.feature).ok_or(AmdError::Adlx(-1))?;
        let mut sup = 0u8;
        match c.feature {
            Feature::AntiLag | Feature::EnhancedSync => {
                let v = o.vtbl::<SimpleVtbl>();
                ok((v.is_supported)(o.0, &mut sup))?;
                if sup == 0 {
                    return Err(AmdError::Adlx(12)); // ADLX_NOT_SUPPORTED
                }
                if let Some(e) = c.enabled {
                    ok((v.set_enabled)(o.0, e as u8))?;
                }
            }
            Feature::Chill => {
                let v = o.vtbl::<ChillVtbl>();
                ok((v.is_supported)(o.0, &mut sup))?;
                if sup == 0 {
                    return Err(AmdError::Adlx(12));
                }
                if let Some(e) = c.enabled {
                    ok((v.set_enabled)(o.0, e as u8))?;
                }
            }
            Feature::Boost | Feature::Sharpening | Feature::Frtc => {
                let v = o.vtbl::<RangedVtbl>();
                ok((v.is_supported)(o.0, &mut sup))?;
                if sup == 0 {
                    return Err(AmdError::Adlx(12));
                }
                if let Some(val) = c.value {
                    let mut r = IntRange::default();
                    ok((v.get_range)(o.0, &mut r))?;
                    if !(r.min..=r.max).contains(&val) {
                        return Err(AmdError::BadRequest);
                    }
                    ok((v.set_value)(o.0, val))?;
                }
                if let Some(e) = c.enabled {
                    ok((v.set_enabled)(o.0, e as u8))?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_validated() {
        assert_eq!(Request::parse(r#"{"cmd":"read"}"#), Ok(Request::read()));
        let ok = Request::parse(r#"{"cmd":"set","gpu":1,"changes":[{"feature":"anti_lag","enabled":true},{"feature":"frtc","value":141}]}"#)
            .unwrap();
        assert_eq!(ok.changes.len(), 2);
        for bad in [
            "",
            "{}",
            r#"{"cmd":"format_c"}"#,
            r#"{"cmd":"read","gpu":2}"#,
            r#"{"cmd":"read","extra":1}"#,
            r#"{"cmd":"set","gpu":0,"changes":[]}"#,
            r#"{"cmd":"set","gpu":9,"changes":[{"feature":"chill","enabled":false}]}"#,
            r#"{"cmd":"set","gpu":0,"changes":[{"feature":"chill"}]}"#,
            r#"{"cmd":"set","gpu":0,"changes":[{"feature":"chill","value":60}]}"#,
            r#"{"cmd":"set","gpu":0,"changes":[{"feature":"frtc","value":5}]}"#,
            r#"{"cmd":"set","gpu":0,"changes":[{"feature":"sharpening","value":101}]}"#,
            r#"{"cmd":"set","gpu":0,"changes":[{"feature":"boost","enabled":true},{"feature":"boost","enabled":false}]}"#,
            r#"{"cmd":"set","gpu":0,"changes":[{"feature":"anti_lag","enabled":true,"id":123}]}"#,
            r#"{"cmd":"set","gpu":0,"changes":[{"feature":"overclock","enabled":true}]}"#,
        ] {
            assert_eq!(Request::parse(bad), Err(AmdError::BadRequest), "{bad}");
        }
        assert_eq!(Request::parse(&format!(r#"{{"cmd":"read","pad":"{}"}}"#, "x".repeat(2000))), Err(AmdError::BadRequest));
    }

    #[test]
    fn helper_answers_are_validated() {
        let line = r#"{"ok":true,"gpus":[{"index":7,"name":"AMD Radeon RX 7800 XT\u0007","discrete":true,
            "features":[{"feature":"frtc","supported":true,"enabled":false,"value":141,"range":[30,300]},
                        {"feature":"chill","supported":true,"enabled":true,"range":[9,1]}]}]}"#
            .replace('\n', " ");
        let gpus = parse_answer(&line).unwrap();
        assert_eq!(gpus[0].index, 0);
        assert_eq!(gpus[0].name, "AMD Radeon RX 7800 XT");
        assert_eq!(gpus[0].features[0].recommended, Some(false));
        assert_eq!(gpus[0].features[0].range, Some((30, 300)));
        assert_eq!(gpus[0].features[1].range, None);
        assert_eq!(parse_answer(r#"{"ok":false,"error":"no_amd"}"#), Err(AmdError::NotFound));
        assert_eq!(parse_answer(r#"{"ok":false,"error":"adlx_12"}"#), Err(AmdError::Adlx(12)));
        assert_eq!(parse_answer(""), Err(AmdError::HelperFailed));
        assert_eq!(parse_answer("Segmentation fault"), Err(AmdError::HelperFailed));
        let many = format!(r#"{{"ok":true,"gpus":[{}]}}"#, [r#"{"index":0,"name":"x","discrete":false,"features":[]}"#; 9].join(","));
        assert_eq!(parse_answer(&many), Err(AmdError::HelperFailed));
    }

    #[test]
    fn helper_reports_missing_driver_or_bad_input() {
        // Never loads ADLX for a bad request.
        assert_eq!(helper_main("{\"cmd\":\"nope\"}"), r#"{"ok":false,"error":"bad_request","gpus":[]}"#);
        // This development PC has no AMD driver: the helper answers no_amd instead of crashing.
        if !std::path::Path::new(r"C:\Windows\System32\amdadlx64.dll").exists() {
            assert_eq!(parse_answer(&helper_main(r#"{"cmd":"read"}"#)), Err(AmdError::NotFound));
        }
    }

    #[test]
    fn recommendations_cover_toggles() {
        assert_eq!(Feature::AntiLag.recommended(), Some(true));
        assert_eq!(Feature::Sharpening.recommended(), None);
        assert!(Feature::ALL.iter().all(|f| f.value_range().is_none_or(|(lo, hi)| lo < hi)));
    }
}
