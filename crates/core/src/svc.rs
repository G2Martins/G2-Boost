//! Service Control Manager helpers for the Maintenance quick fixes: query, list the running
//! dependents, stop and start with a deadline. Same calls as the private helpers in `boost.rs`
//! (`OpenSCManagerW`/`OpenServiceW`/`ControlService`/`StartServiceW`); kept apart on purpose
//! (`boost` owns its tweak services and their backups, this module only restarts). Never `sc.exe`
//! or `net.exe`.
//!
//! Errors are Win32 codes (5 = access denied, 1053 = no answer in time, 1058 = disabled...).

use std::time::{Duration, Instant};

use windows::Win32::System::Services::{
    CloseServiceHandle, ControlService, ENUM_SERVICE_STATUSW, EnumDependentServicesW, OpenSCManagerW, OpenServiceW,
    QUERY_SERVICE_CONFIGW, QueryServiceConfigW, QueryServiceStatusEx, SC_HANDLE, SC_MANAGER_CONNECT,
    SC_STATUS_PROCESS_INFO, SERVICE_ACTIVE, SERVICE_CONTROL_STOP, SERVICE_DISABLED, SERVICE_ENUMERATE_DEPENDENTS,
    SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START, SERVICE_STATUS, SERVICE_STATUS_PROCESS,
    SERVICE_STOP, SERVICE_STOPPED, StartServiceW,
};
use windows::core::PCWSTR;

use crate::registry::wide;

/// `ERROR_SERVICE_REQUEST_TIMEOUT`: the service did not reach the state in time.
pub const TIMEOUT: u32 = 1053;
const SERVICE_DOES_NOT_EXIST: u32 = 1060;
const ALREADY_RUNNING: u32 = 1056;
const NOT_ACTIVE: u32 = 1062;
const CANNOT_ACCEPT_CTRL: u32 = 1061;
const MORE_DATA: u32 = 234;
/// `ERROR_PROCESS_ABORTED`: the service stopped right after the start request.
const START_FAILED: u32 = 1067;

struct Sc(SC_HANDLE);

impl Drop for Sc {
    fn drop(&mut self) {
        // SAFETY: handle opened by OpenSCManagerW/OpenServiceW, closed once.
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

fn code(e: windows::core::Error) -> u32 {
    (e.code().0 as u32) & 0xFFFF
}

fn open(name: &str, access: u32) -> Result<Option<Sc>, u32> {
    // SAFETY: local machine, default database.
    let scm = Sc(unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) }.map_err(code)?);
    let w = wide(name);
    // SAFETY: valid SCM handle and NUL-terminated name.
    match unsafe { OpenServiceW(scm.0, PCWSTR(w.as_ptr()), access) }.map_err(code) {
        Ok(h) => Ok(Some(Sc(h))),
        Err(SERVICE_DOES_NOT_EXIST) => Ok(None),
        Err(c) => Err(c),
    }
}

fn current_state(h: &Sc) -> Result<u32, u32> {
    let mut buf = [0u8; size_of::<SERVICE_STATUS_PROCESS>()];
    let mut needed = 0u32;
    // SAFETY: buffer sized for SERVICE_STATUS_PROCESS.
    unsafe { QueryServiceStatusEx(h.0, SC_STATUS_PROCESS_INFO, Some(&mut buf), &mut needed) }.map_err(code)?;
    // SAFETY: filled by the call above.
    Ok(unsafe { std::ptr::read_unaligned(buf.as_ptr().cast::<SERVICE_STATUS_PROCESS>()) }.dwCurrentState.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Info {
    /// Start type Disabled: never started by the fixes (the user or a tweak turned it off).
    pub disabled: bool,
    /// Anything but stopped (running, starting, stopping, paused).
    pub running: bool,
}

/// `None` when the service is not installed. Works without admin.
pub fn query(name: &str) -> Result<Option<Info>, u32> {
    let Some(h) = open(name, SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS)? else {
        return Ok(None);
    };
    let mut needed = 0u32;
    // SAFETY: size probe; fails with ERROR_INSUFFICIENT_BUFFER and sets `needed`.
    let _ = unsafe { QueryServiceConfigW(h.0, None, 0, &mut needed) };
    let mut buf = vec![0u64; (needed as usize).div_ceil(8) + 1];
    // SAFETY: 8-byte aligned buffer of at least `needed` bytes.
    unsafe { QueryServiceConfigW(h.0, Some(buf.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>()), needed, &mut needed) }.map_err(code)?;
    // SAFETY: the call filled a QUERY_SERVICE_CONFIGW at the start of the buffer.
    let start = unsafe { (*buf.as_ptr().cast::<QUERY_SERVICE_CONFIGW>()).dwStartType };
    Ok(Some(Info { disabled: start == SERVICE_DISABLED, running: current_state(&h)? != SERVICE_STOPPED.0 }))
}

/// The services that depend on `name` and are running, in the order the SCM says to stop them.
pub fn active_dependents(name: &str) -> Result<Vec<String>, u32> {
    let Some(h) = open(name, SERVICE_ENUMERATE_DEPENDENTS)? else {
        return Ok(Vec::new());
    };
    let (mut needed, mut count) = (0u32, 0u32);
    // SAFETY: size probe with no buffer.
    match unsafe { EnumDependentServicesW(h.0, SERVICE_ACTIVE, None, 0, &mut needed, &mut count) }.map_err(code) {
        Ok(()) => return Ok(Vec::new()),
        Err(MORE_DATA) => {}
        Err(c) => return Err(c),
    }
    let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
    let size = (buf.len() * 8) as u32;
    // SAFETY: 8-byte aligned buffer of `size` bytes; the API writes `count` entries plus strings.
    unsafe { EnumDependentServicesW(h.0, SERVICE_ACTIVE, Some(buf.as_mut_ptr().cast()), size, &mut needed, &mut count) }.map_err(code)?;
    let first = buf.as_ptr().cast::<ENUM_SERVICE_STATUSW>();
    let room = (size as usize) / size_of::<ENUM_SERVICE_STATUSW>();
    // SAFETY: `count` entries written at the start of `buf` (bounded by its size); the names
    // point inside `buf`, alive for this loop.
    Ok((0..(count as usize).min(room)).filter_map(|i| unsafe { (*first.add(i)).lpServiceName.to_string() }.ok()).collect())
}

/// Poll until the service is in `want` (or, when starting, stopped again = failed to start).
fn wait(h: &Sc, want: u32, limit: Duration) -> Result<(), u32> {
    let deadline = Instant::now() + limit;
    loop {
        let s = current_state(h)?;
        if s == want {
            return Ok(());
        }
        if want == SERVICE_RUNNING.0 && s == SERVICE_STOPPED.0 {
            return Err(START_FAILED);
        }
        if Instant::now() > deadline {
            return Err(TIMEOUT);
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Stop and wait until stopped. Not installed or already stopped = done.
pub fn stop(name: &str, limit: Duration) -> Result<(), u32> {
    let Some(h) = open(name, SERVICE_STOP | SERVICE_QUERY_STATUS)? else {
        return Ok(());
    };
    let mut st = SERVICE_STATUS::default();
    // SAFETY: valid handle; plain status out-param.
    match unsafe { ControlService(h.0, SERVICE_CONTROL_STOP, &mut st) }.map_err(code) {
        Ok(()) | Err(CANNOT_ACCEPT_CTRL) => wait(&h, SERVICE_STOPPED.0, limit), // stopping already: wait and see
        Err(NOT_ACTIVE) => Ok(()),
        Err(c) => Err(c),
    }
}

/// Start and wait until running. Already running = done.
pub fn start(name: &str, limit: Duration) -> Result<(), u32> {
    let Some(h) = open(name, SERVICE_START | SERVICE_QUERY_STATUS)? else {
        return Err(SERVICE_DOES_NOT_EXIST);
    };
    // SAFETY: valid handle, no arguments.
    match unsafe { StartServiceW(h.0, None) }.map_err(code) {
        Ok(()) => wait(&h, SERVICE_RUNNING.0, limit),
        Err(ALREADY_RUNNING) => Ok(()),
        Err(c) => Err(c),
    }
}

/// Stop order for `targets` (each listed dependent-first by the caller): every target's running
/// dependents before it, each service once (names compare case-insensitively). Start = reverse.
pub(crate) fn stop_order(targets: &[&str], dependents: impl Fn(&str) -> Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |n: &str| {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(n)) {
            out.push(n.to_string());
        }
    };
    for t in targets {
        for d in dependents(t) {
            push(&d);
        }
        push(t);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependents_stop_first_and_each_service_once() {
        // Real shape (this PC, 2026-10-25): Audiosrv depends on AudioEndpointBuilder.
        let deps = |n: &str| match n {
            "AudioEndpointBuilder" => vec!["Audiosrv".to_string()],
            _ => Vec::new(),
        };
        assert_eq!(stop_order(&["Audiosrv", "AudioEndpointBuilder"], deps), ["Audiosrv", "AudioEndpointBuilder"]);
        assert_eq!(stop_order(&["AudioEndpointBuilder", "Audiosrv"], deps), ["Audiosrv", "AudioEndpointBuilder"], "caller order does not matter");
        // A vendor service hanging off Audiosrv (listed by the SCM for both, case differs).
        let vendor = |n: &str| match n {
            "AudioEndpointBuilder" => vec!["VendorAudio".to_string(), "audiosrv".to_string()],
            "Audiosrv" => vec!["vendoraudio".to_string()],
            _ => Vec::new(),
        };
        let order = stop_order(&["Audiosrv", "AudioEndpointBuilder"], vendor);
        assert_eq!(order, ["vendoraudio", "Audiosrv", "AudioEndpointBuilder"]);
        let start: Vec<&String> = order.iter().rev().collect();
        assert_eq!(start, ["AudioEndpointBuilder", "Audiosrv", "vendoraudio"]);
        // Windows Update services: no links between them; a running dependent goes first.
        let wu = |n: &str| if n == "cryptsvc" { vec!["AppIDSvc".to_string()] } else { Vec::new() };
        assert_eq!(stop_order(&["wuauserv", "bits", "cryptsvc"], wu), ["wuauserv", "bits", "AppIDSvc", "cryptsvc"]);
        assert!(stop_order(&[], wu).is_empty());
    }

    #[test]
    fn query_reads_without_admin() {
        // Read-only; RpcSs exists and runs on every Windows.
        let rpc = query("RpcSs").unwrap().expect("RpcSs installed");
        assert!(rpc.running && !rpc.disabled);
        assert_eq!(query("G2BoostNoSuchService").unwrap(), None);
        assert!(active_dependents("G2BoostNoSuchService").unwrap().is_empty());
        if !crate::system::is_elevated() {
            assert_eq!(stop("Spooler", Duration::from_secs(1)), Err(5), "a standard user cannot stop services");
        }
        assert_eq!(start("G2BoostNoSuchService", Duration::from_secs(1)), Err(SERVICE_DOES_NOT_EXIST));
    }
}
