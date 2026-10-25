//! Windows optional features through `DismApi.dll` (never `DISM.exe`): read one feature's state
//! and turn it off or back on in the running Windows. Admin only; one servicing job at a time
//! (the slot shared with `repair` and `volcache`). Only names in [`ALLOWED`] reach the API.

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};
use windows::core::{HRESULT, PCWSTR, s, w};

use crate::{registry, repair, system};

/// Optional features G2 Boost may switch. `Recall`: Windows 11 24H2 and newer.
pub const ALLOWED: &[&str] = &["Recall"];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FeatureError {
    #[error("needs_admin")]
    NeedsAdmin,
    /// A repair or a system-files cleanup holds the servicing slot.
    #[error("busy")]
    Busy,
    #[error("unknown_feature")]
    Unknown,
    #[error("dism {0:#010x}")]
    Dism(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Enabled,
    Disabled,
    /// Changed, waiting for a restart.
    EnablePending,
    DisablePending,
    Partial,
    /// This Windows does not have the feature.
    Absent,
}

/// `DismPackageFeatureState` → [`State`].
pub(crate) fn from_dism(v: i32) -> State {
    match v {
        1 => State::DisablePending,
        2 | 3 => State::Disabled, // staged, or removed with its payload
        4 => State::Enabled,
        5 => State::EnablePending,
        7 => State::Partial,
        _ => State::Absent, // not present, superseded
    }
}

const DISMAPI_E_UNKNOWN_FEATURE: u32 = 0x800F_080C;
/// `ERROR_SUCCESS_REBOOT_REQUIRED`, as an HRESULT and raw: done, takes effect after a restart.
const REBOOT_REQUIRED: [u32; 2] = [0x8007_0BC2, 0x0000_0BC2];
/// `DISM_ONLINE_IMAGE`: the running Windows.
const DISM_ONLINE: PCWSTR = w!("DISM_{53BFAE52-B167-4E2F-A258-0A37B57FF845}");

type FarProc = unsafe extern "system" fn() -> isize;
type Open = unsafe extern "system" fn(PCWSTR, PCWSTR, PCWSTR, *mut u32) -> HRESULT;
type Close = unsafe extern "system" fn(u32) -> HRESULT;
type Info = unsafe extern "system" fn(u32, PCWSTR, PCWSTR, i32, *mut *mut c_void) -> HRESULT;
type Disable = unsafe extern "system" fn(u32, PCWSTR, PCWSTR, i32, *mut c_void, *const c_void, *mut c_void) -> HRESULT;
type Enable = unsafe extern "system" fn(
    u32,
    PCWSTR,
    PCWSTR,
    i32,
    i32,
    *const PCWSTR,
    u32,
    i32,
    *mut c_void,
    *const c_void,
    *mut c_void,
) -> HRESULT;
type Delete = unsafe extern "system" fn(*mut c_void) -> HRESULT;

#[derive(Clone, Copy)]
struct Api {
    open: Open,
    close: Close,
    info: Info,
    disable: Disable,
    enable: Enable,
    delete: Delete,
}

static API: OnceLock<Api> = OnceLock::new();

fn api() -> Result<Api, FeatureError> {
    if let Some(a) = API.get() {
        return Ok(*a);
    }
    // DismInitialize is once per process: `repair` owns it.
    repair::dism_ready().map_err(FeatureError::Dism)?;
    // SAFETY: System32-only load of the documented DISM API (already loaded by `repair`); the
    // exports have the signatures from dismapi.h (WINAPI = system ABI).
    unsafe {
        let lib = LoadLibraryExW(w!("DismApi.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)
            .map_err(|e| FeatureError::Dism(e.code().0 as u32))?;
        let get = |n| GetProcAddress(lib, n).ok_or(FeatureError::Dism(0x8007_007F)); // ERROR_PROC_NOT_FOUND
        let a = Api {
            open: std::mem::transmute::<FarProc, Open>(get(s!("DismOpenSession"))?),
            close: std::mem::transmute::<FarProc, Close>(get(s!("DismCloseSession"))?),
            info: std::mem::transmute::<FarProc, Info>(get(s!("DismGetFeatureInfo"))?),
            disable: std::mem::transmute::<FarProc, Disable>(get(s!("DismDisableFeature"))?),
            enable: std::mem::transmute::<FarProc, Enable>(get(s!("DismEnableFeature"))?),
            delete: std::mem::transmute::<FarProc, Delete>(get(s!("DismDelete"))?),
        };
        Ok(*API.get_or_init(|| a))
    }
}

/// The name as UTF-16, only if it is in [`ALLOWED`] (callers are hostile).
pub(crate) fn checked(name: &str) -> Result<Vec<u16>, FeatureError> {
    if ALLOWED.contains(&name) { Ok(registry::wide(name)) } else { Err(FeatureError::Unknown) }
}

/// `f` with a session on the running Windows, holding the servicing slot.
fn with_session<T>(f: impl FnOnce(&Api, u32) -> Result<T, FeatureError>) -> Result<T, FeatureError> {
    if !system::is_elevated() {
        return Err(FeatureError::NeedsAdmin);
    }
    let _slot = repair::exclusive().ok_or(FeatureError::Busy)?;
    let api = api()?;
    let mut session = 0u32;
    // SAFETY: online image, no windows dir / system drive overrides; out param is a u32.
    let hr = unsafe { (api.open)(DISM_ONLINE, PCWSTR::null(), PCWSTR::null(), &mut session) };
    if hr.is_err() {
        return Err(FeatureError::Dism(hr.0 as u32));
    }
    let out = f(&api, session);
    // SAFETY: the session opened above, closed once.
    unsafe {
        let _ = (api.close)(session);
    }
    out
}

// ponytail: answers are cached for the process (a DISM session costs seconds and detect runs
// often); `set` drops the entry. A change made outside G2 Boost shows after a restart of the app.
static CACHE: Mutex<Vec<(&'static str, State)>> = Mutex::new(Vec::new());

fn cached(name: &str) -> Option<State> {
    CACHE.lock().unwrap_or_else(|p| p.into_inner()).iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

fn remember(name: &str, s: Option<State>) {
    let mut c = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    c.retain(|(n, _)| *n != name);
    if let (Some(s), Some(n)) = (s, ALLOWED.iter().find(|a| **a == name)) {
        c.push((n, s));
    }
}

/// The feature's state (admin; `Busy` while a repair runs).
pub fn state(name: &str) -> Result<State, FeatureError> {
    let w = checked(name)?;
    if let Some(s) = cached(name) {
        return Ok(s);
    }
    let s = with_session(|api, session| {
        let mut info: *mut c_void = null_mut();
        // SAFETY: valid session and NUL-terminated name; no package identifier.
        let hr = unsafe { (api.info)(session, PCWSTR(w.as_ptr()), PCWSTR::null(), 0, &mut info) };
        if hr.0 as u32 == DISMAPI_E_UNKNOWN_FEATURE {
            return Ok(State::Absent);
        }
        if hr.is_err() || info.is_null() {
            return Err(FeatureError::Dism(hr.0 as u32));
        }
        // SAFETY: DismFeatureInfo (pack 1) starts with the name pointer; the state (an enum, i32)
        // follows at offset 8 on 64-bit. DismDelete frees the struct once.
        let v = unsafe { info.cast::<u8>().add(8).cast::<i32>().read_unaligned() };
        unsafe {
            let _ = (api.delete)(info);
        }
        Ok(from_dism(v))
    })?;
    remember(name, Some(s));
    Ok(s)
}

/// Turn the feature on or off (keeps the payload, so turning it back on works offline).
/// Minutes at most; the result usually waits for a restart.
pub fn set(name: &str, enabled: bool) -> Result<State, FeatureError> {
    let w = checked(name)?;
    remember(name, None);
    with_session(|api, session| {
        let name = PCWSTR(w.as_ptr());
        // SAFETY: valid session and name; no package name, no cancel event, no progress callback.
        let hr = unsafe {
            if enabled {
                (api.enable)(session, name, PCWSTR::null(), 0, 0, std::ptr::null(), 0, 0, null_mut(), std::ptr::null(), null_mut())
            } else {
                (api.disable)(session, name, PCWSTR::null(), 0, null_mut(), std::ptr::null(), null_mut())
            }
        };
        if hr.is_ok() || REBOOT_REQUIRED.contains(&(hr.0 as u32)) {
            Ok(())
        } else if hr.0 as u32 == DISMAPI_E_UNKNOWN_FEATURE {
            Err(FeatureError::Unknown)
        } else {
            Err(FeatureError::Dism(hr.0 as u32))
        }
    })?;
    state(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_allowlisted_names_reach_dism() {
        for bad in ["", "recall", "Recall ", "Microsoft-Windows-Subsystem-Linux", "..\\Recall", "Recall\0", "NetFx3"] {
            assert_eq!(checked(bad), Err(FeatureError::Unknown), "{bad:?}");
        }
        assert!(checked("Recall").is_ok());
        assert_eq!(set("TelnetClient", true), Err(FeatureError::Unknown), "refused before any admin check or DISM call");
        assert_eq!(from_dism(2), State::Disabled);
        assert_eq!(from_dism(3), State::Disabled);
        assert_eq!(from_dism(4), State::Enabled);
        assert_eq!(from_dism(1), State::DisablePending);
        assert_eq!(from_dism(5), State::EnablePending);
        assert_eq!(from_dism(0), State::Absent);
        assert_eq!(from_dism(-7), State::Absent);
    }

    #[test]
    fn unelevated_state_is_refused() {
        if system::is_elevated() {
            return;
        }
        assert_eq!(state("Recall"), Err(FeatureError::NeedsAdmin));
    }

    /// Elevated, read-only: the Recall state on this PC (Absent on Windows 10 and older 11).
    /// `cargo test -p g2boost-core -- --ignored recall_state_elevated` in an elevated shell.
    #[test]
    #[ignore]
    fn recall_state_elevated() {
        println!("Recall: {:?}", state("Recall"));
    }
}
