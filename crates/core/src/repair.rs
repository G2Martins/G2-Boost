//! Windows repair: DISM (image health) through `DismApi.dll`, never `DISM.exe`; SFC and MRT
//! through `sfc.exe` / `MRT.exe` under the exception in `Docs/core/global-rules.md` §1.3:
//! absolute `System32` path built here (never the PATH), fixed arguments, no shell, no window,
//! only on a user click with the app elevated, one at a time. Results come from Windows' own
//! logs (`CBS.log`, `mrt.log`), parsed by pure functions.
//!
//! One servicing job at a time for the whole app: [`exclusive`] is shared with the Disk Cleanup
//! handlers (`volcache`), which touch the same component store. [`cancel`] stops whatever runs.

use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, TryLockError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};
use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemDirectoryW, GetWindowsDirectoryW};
use windows::Win32::System::Threading::{CreateEventW, ResetEvent, SetEvent};
use windows::core::{HRESULT, PCWSTR, s, w};

use crate::registry;
use crate::system;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const HISTORY_MAX: usize = 50;
const LOG_CAP: u64 = 64 << 20;
/// `ERROR_TIMEOUT`: recorded as the code when a tool ran past its limit and was stopped.
const TIMEOUT_CODE: u32 = 1460;
const E_CANCELLED: u32 = 0x8007_04C7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    /// `DismCheckImageHealth(ScanImage = TRUE)` (= `/ScanHealth`).
    DismScan,
    /// `DismRestoreImageHealth` from Windows Update (= `/RestoreHealth`).
    DismRestore,
    Sfc,
    MrtQuick,
    MrtFull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Healthy,
    /// DISM: corruption found, `RestoreHealth` can fix it.
    Repairable,
    /// DISM: corruption that cannot be fixed online; SFC: files it could not repair.
    NonRepairable,
    /// SFC: corrupted files found and repaired.
    Repaired,
    /// MRT: no infection found.
    Clean,
    /// MRT: infection found (see `threats`).
    Threats,
    Failed,
    Cancelled,
    /// The tool finished but its log had nothing for this run (rotated or never written).
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    pub tool: Tool,
    pub at: u64,
    pub duration_ms: u64,
    pub verdict: Verdict,
    /// HRESULT (DISM) or exit code (SFC/MRT). 1460 = stopped after the time limit.
    pub code: u32,
    /// SFC: distinct files repaired / not repaired (from `CBS.log`).
    pub repaired: u32,
    pub unrepaired: u32,
    /// MRT: threat names from `mrt.log` (never file paths).
    pub threats: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RepairError {
    #[error("needs_admin")]
    NeedsAdmin,
    /// Another repair or a system-files cleanup is running.
    #[error("busy")]
    Busy,
    /// MRT arrives through Windows Update; not on this PC.
    #[error("mrt_missing")]
    MrtMissing,
    #[error("io: {0}")]
    Io(String),
}

/* ---------------- one job at a time + cancel ---------------- */

static EXCLUSIVE: Mutex<()> = Mutex::new(());
static CANCEL: AtomicBool = AtomicBool::new(false);
/// DISM's cancel event (created once, never closed, so `cancel` can always signal it).
static DISM_EVENT: AtomicIsize = AtomicIsize::new(0);
static RUNNING: Mutex<Option<(Tool, Option<u32>)>> = Mutex::new(None);

/// The app-wide servicing slot. `None` = something else is running. Clears a stale cancel.
pub(crate) fn exclusive() -> Option<MutexGuard<'static, ()>> {
    let g = match EXCLUSIVE.try_lock() {
        Ok(g) => g,
        Err(TryLockError::Poisoned(p)) => p.into_inner(),
        Err(TryLockError::WouldBlock) => return None,
    };
    CANCEL.store(false, Ordering::SeqCst);
    Some(g)
}

/// Stop the running repair or system-files cleanup (DISM event, child process, handler callback).
pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
    let ev = DISM_EVENT.load(Ordering::SeqCst);
    if ev != 0 {
        // SAFETY: the event is created once and never closed.
        unsafe {
            let _ = SetEvent(HANDLE(ev as *mut c_void));
        }
    }
}

pub(crate) fn cancelled() -> bool {
    CANCEL.load(Ordering::SeqCst)
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub running: Option<Tool>,
    /// DISM reports a percentage; SFC/MRT do not (`None`).
    pub percent: Option<u32>,
    pub mrt_present: bool,
}

pub fn status() -> Status {
    let r = *RUNNING.lock().unwrap_or_else(|p| p.into_inner());
    Status { running: r.map(|x| x.0), percent: r.and_then(|x| x.1), mrt_present: mrt_exe().is_file() }
}

/* ---------------- Windows paths (from the API, not the environment) ---------------- */

fn sys_dir(f: unsafe fn(Option<&mut [u16]>) -> u32) -> PathBuf {
    let mut buf = [0u16; 260];
    // SAFETY: the buffer is MAX_PATH long; the return value is the written length.
    let n = unsafe { f(Some(&mut buf)) } as usize;
    if n == 0 || n >= buf.len() {
        return PathBuf::from(r"C:\Windows\System32"); // ponytail: never happens on a sane system
    }
    PathBuf::from(String::from_utf16_lossy(&buf[..n]))
}

pub(crate) fn system32() -> PathBuf {
    sys_dir(GetSystemDirectoryW)
}

pub(crate) fn windows_dir() -> PathBuf {
    sys_dir(GetWindowsDirectoryW)
}

fn mrt_exe() -> PathBuf {
    system32().join("MRT.exe")
}

/// Local wall clock as `CBS.log` writes it: `YYYY-MM-DD HH:MM:SS`.
fn local_stamp() -> String {
    // SAFETY: GetLocalTime only fills the returned struct.
    let t = unsafe { GetLocalTime() };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/* ---------------- run ---------------- */

/// Run one tool to the end (minutes to hours). `on_progress(percent)` gets DISM's percentage;
/// SFC/MRT have none. Admin only; refuses when another servicing job runs.
pub fn run(tool: Tool, on_progress: &(dyn Fn(Option<u32>) + Sync)) -> Result<Outcome, RepairError> {
    if !system::is_elevated() {
        return Err(RepairError::NeedsAdmin);
    }
    if matches!(tool, Tool::MrtQuick | Tool::MrtFull) && !mrt_exe().is_file() {
        return Err(RepairError::MrtMissing);
    }
    let _slot = exclusive().ok_or(RepairError::Busy)?;
    set_running(Some((tool, None)));
    let progress = |p: Option<u32>| {
        set_running(Some((tool, p)));
        on_progress(p);
    };
    let t0 = Instant::now();
    let mut out = Outcome {
        tool,
        at: registry::now_unix(),
        duration_ms: 0,
        verdict: Verdict::Unknown,
        code: 0,
        repaired: 0,
        unrepaired: 0,
        threats: Vec::new(),
    };
    let res = match tool {
        Tool::DismScan => dism(false, &progress, &mut out),
        Tool::DismRestore => dism(true, &progress, &mut out),
        Tool::Sfc => sfc(&mut out),
        Tool::MrtQuick => mrt(false, &mut out),
        Tool::MrtFull => mrt(true, &mut out),
    };
    set_running(None);
    res?;
    if cancelled() {
        out.verdict = Verdict::Cancelled;
    }
    out.duration_ms = t0.elapsed().as_millis() as u64;
    record(&out);
    Ok(out)
}

fn set_running(v: Option<(Tool, Option<u32>)>) {
    *RUNNING.lock().unwrap_or_else(|p| p.into_inner()) = v;
}

/* ---------------- DISM (DismApi.dll) ---------------- */

/// `DISM_ONLINE_IMAGE`: the running Windows.
const DISM_ONLINE: PCWSTR = w!("DISM_{53BFAE52-B167-4E2F-A258-0A37B57FF845}");

type DismProgress = unsafe extern "system" fn(u32, u32, *mut c_void);
type FarProc = unsafe extern "system" fn() -> isize;
type DismInit = unsafe extern "system" fn(i32, PCWSTR, PCWSTR) -> HRESULT;
type DismOpen = unsafe extern "system" fn(PCWSTR, PCWSTR, PCWSTR, *mut u32) -> HRESULT;
type DismClose = unsafe extern "system" fn(u32) -> HRESULT;
type DismCheck = unsafe extern "system" fn(u32, i32, *mut c_void, Option<DismProgress>, *mut c_void, *mut i32) -> HRESULT;
type DismRestore =
    unsafe extern "system" fn(u32, *const PCWSTR, u32, i32, *mut c_void, Option<DismProgress>, *mut c_void) -> HRESULT;

#[derive(Clone, Copy)]
struct Dism {
    open: DismOpen,
    close: DismClose,
    check: DismCheck,
    restore: DismRestore,
}

/// Initialised once per process (`DismInitialize` is documented as once per process); kept for
/// the app's lifetime. Only a successful init is cached.
static DISM_API: OnceLock<Dism> = OnceLock::new();

fn dism_api() -> Result<Dism, u32> {
    if let Some(a) = DISM_API.get() {
        return Ok(*a);
    }
    // SAFETY: System32-only load of the documented DISM API; the exports have the signatures
    // from dismapi.h (WINAPI = system ABI).
    unsafe {
        let lib = LoadLibraryExW(w!("DismApi.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).map_err(|e| e.code().0 as u32)?;
        let get = |name: windows::core::PCSTR| GetProcAddress(lib, name).ok_or(0x8007_007F_u32); // ERROR_PROC_NOT_FOUND
        let init = std::mem::transmute::<FarProc, DismInit>(get(s!("DismInitialize"))?);
        let api = Dism {
            open: std::mem::transmute::<FarProc, DismOpen>(get(s!("DismOpenSession"))?),
            close: std::mem::transmute::<FarProc, DismClose>(get(s!("DismCloseSession"))?),
            check: std::mem::transmute::<FarProc, DismCheck>(get(s!("DismCheckImageHealth"))?),
            restore: std::mem::transmute::<FarProc, DismRestore>(get(s!("DismRestoreImageHealth"))?),
        };
        // DismLogErrorsWarningsInfo; default log (%windir%\Logs\DISM\dism.log) and scratch dir.
        let hr = init(2, PCWSTR::null(), PCWSTR::null());
        if hr.is_err() {
            return Err(hr.0 as u32);
        }
        let _ = DISM_API.set(api);
        Ok(api)
    }
}

fn dism_event() -> Option<HANDLE> {
    let mut ev = DISM_EVENT.load(Ordering::SeqCst);
    if ev == 0 {
        // SAFETY: unnamed manual-reset event, kept for the process lifetime.
        let h = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.ok()?;
        ev = h.0 as isize;
        DISM_EVENT.store(ev, Ordering::SeqCst);
    }
    let h = HANDLE(ev as *mut c_void);
    // SAFETY: valid event handle (see above).
    unsafe {
        let _ = ResetEvent(h);
    }
    if cancelled() {
        // cancel() raced in before the reset: keep it signalled.
        // SAFETY: as above.
        unsafe {
            let _ = SetEvent(h);
        }
    }
    Some(h)
}

/// DISM calls this on its own thread; `user` points at the caller's `&dyn Fn` (alive for the call).
unsafe extern "system" fn dism_progress(current: u32, total: u32, user: *mut c_void) {
    if total == 0 || user.is_null() {
        return;
    }
    // SAFETY: `user` is the `&&dyn Fn` passed by `dism` below, valid until the DISM call returns.
    let f = unsafe { &*(user as *const &(dyn Fn(Option<u32>) + Sync)) };
    f(Some(((current as u64 * 100) / total as u64).min(100) as u32));
}

fn dism(restore: bool, on_progress: &(dyn Fn(Option<u32>) + Sync), out: &mut Outcome) -> Result<(), RepairError> {
    let api = match dism_api() {
        Ok(a) => a,
        Err(code) => return fail(out, code),
    };
    let Some(ev) = dism_event() else {
        return fail(out, 0x8000_4005); // E_FAIL
    };
    let mut session = 0u32;
    // SAFETY: online image, no windows dir / system drive overrides; out param is a u32.
    let hr = unsafe { (api.open)(DISM_ONLINE, PCWSTR::null(), PCWSTR::null(), &mut session) };
    if hr.is_err() {
        return fail(out, hr.0 as u32);
    }
    let sink: &(dyn Fn(Option<u32>) + Sync) = on_progress;
    let user = &sink as *const &(dyn Fn(Option<u32>) + Sync) as *mut c_void;
    let mut state = 0i32;
    // SAFETY: valid session and event; `user` outlives the synchronous calls.
    let hr = unsafe {
        if restore {
            // No source paths: Windows Update is the source (what `/RestoreHealth` does).
            let hr = (api.restore)(session, std::ptr::null(), 0, 0, ev.0, Some(dism_progress), user);
            if hr.is_ok() {
                // Quick re-check (CheckHealth, no scan): report the state the image ended in.
                (api.check)(session, 0, ev.0, None, std::ptr::null_mut(), &mut state)
            } else {
                hr
            }
        } else {
            (api.check)(session, 1, ev.0, Some(dism_progress), user, &mut state)
        }
    };
    // SAFETY: the session opened above.
    unsafe {
        let _ = (api.close)(session);
    }
    out.code = hr.0 as u32;
    out.verdict = if cancelled() || out.code == E_CANCELLED {
        Verdict::Cancelled
    } else if hr.is_err() {
        Verdict::Failed
    } else {
        match state {
            0 => Verdict::Healthy,
            1 => Verdict::Repairable,
            _ => Verdict::NonRepairable,
        }
    };
    Ok(())
}

fn fail(out: &mut Outcome, code: u32) -> Result<(), RepairError> {
    out.code = code;
    out.verdict = if code == E_CANCELLED { Verdict::Cancelled } else { Verdict::Failed };
    Ok(())
}

/* ---------------- SFC / MRT (registered exception) ---------------- */

enum Stop {
    Cancelled,
    Timeout,
}

/// Start `exe` (absolute System32 path) with fixed `args`, no window, no stdio; poll for the
/// end, the cancel flag and the time limit (kill on either).
fn spawn_wait(exe: &Path, args: &[&str], limit: Duration) -> Result<Result<u32, Stop>, RepairError> {
    let mut child = Command::new(exe)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| RepairError::Io(e.to_string()))?;
    let deadline = Instant::now() + limit;
    loop {
        if let Some(s) = child.try_wait().map_err(|e| RepairError::Io(e.to_string()))? {
            return Ok(Ok(s.code().unwrap_or(-1) as u32));
        }
        let stop = if cancelled() {
            Some(Stop::Cancelled)
        } else if Instant::now() > deadline {
            Some(Stop::Timeout)
        } else {
            None
        };
        if let Some(stop) = stop {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(Err(stop));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn stopped(out: &mut Outcome, stop: Stop) {
    match stop {
        Stop::Cancelled => out.verdict = Verdict::Cancelled,
        Stop::Timeout => {
            out.verdict = Verdict::Failed;
            out.code = TIMEOUT_CODE;
        }
    }
}

/// Bytes of `path` from `offset` (whole file when it shrank: rotated), capped.
fn read_from(path: &Path, offset: u64) -> Vec<u8> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = if len >= offset { offset } else { 0 };
    let start = start.max(len.saturating_sub(LOG_CAP));
    let mut buf = Vec::new();
    if f.seek(SeekFrom::Start(start)).is_ok() {
        let _ = f.take(LOG_CAP).read_to_end(&mut buf);
    }
    buf
}

fn sfc(out: &mut Outcome) -> Result<(), RepairError> {
    let log = windows_dir().join(r"Logs\CBS\CBS.log");
    let since = local_stamp();
    let offset = std::fs::metadata(&log).map(|m| m.len()).unwrap_or(0);
    match spawn_wait(&system32().join("sfc.exe"), &["/scannow"], Duration::from_secs(90 * 60))? {
        Err(stop) => stopped(out, stop),
        Ok(code) => {
            out.code = code;
            let s = parse_cbs(&String::from_utf8_lossy(&read_from(&log, offset)), &since);
            out.repaired = s.repaired;
            out.unrepaired = s.unrepaired;
            out.verdict = if !s.seen {
                Verdict::Unknown
            } else if s.unrepaired > 0 {
                Verdict::NonRepairable
            } else if s.repaired > 0 {
                Verdict::Repaired
            } else {
                Verdict::Healthy
            };
        }
    }
    Ok(())
}

fn read_mrt_log(path: &Path) -> String {
    let b = read_from(path, 0);
    match b.strip_prefix(&[0xFF, 0xFE]) {
        Some(rest) => registry::utf16z(rest),
        None => String::from_utf8_lossy(&b).into_owned(),
    }
}

fn mrt(full: bool, out: &mut Outcome) -> Result<(), RepairError> {
    let log = windows_dir().join(r"debug\mrt.log");
    let before = mrt_runs(&read_mrt_log(&log));
    let args: &[&str] = if full { &["/Q", "/F"] } else { &["/Q"] };
    let limit = Duration::from_secs(if full { 8 * 3600 } else { 90 * 60 });
    match spawn_wait(&mrt_exe(), args, limit)? {
        Err(stop) => stopped(out, stop),
        Ok(code) => {
            out.code = code;
            let text = read_mrt_log(&log);
            let s = if mrt_runs(&text) > before { parse_mrt(&text) } else { MrtSummary::default() };
            out.threats = s.threats;
            // Exit codes (KB891716): 0 = nothing found, 6..=13 = infection found (removed or not).
            out.verdict = match code {
                0 => Verdict::Clean,
                6..=13 => Verdict::Threats,
                _ if s.clean => Verdict::Clean,
                _ if !out.threats.is_empty() => Verdict::Threats,
                _ => Verdict::Failed,
            };
        }
    }
    Ok(())
}

/* ---------------- log parsers (pure) ---------------- */

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct CbsSummary {
    /// Any `[SR]` line written by this run.
    pub seen: bool,
    pub repaired: u32,
    pub unrepaired: u32,
}

/// The last `"..."` of a CBS line (the file name in `[SR]` lines), else the whole line.
fn last_quoted(line: &str) -> &str {
    let mut parts = line.rsplit('"');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(_), Some(name), Some(_)) => name,
        _ => line,
    }
}

/// `[SR]` lines of `CBS.log` written at or after `since` (`YYYY-MM-DD HH:MM:SS`, local time,
/// the format CBS writes). Files are counted once; a file that was also "cannot repair" is not
/// counted as repaired.
pub(crate) fn parse_cbs(log: &str, since: &str) -> CbsSummary {
    use std::collections::HashSet;
    let (mut seen, mut fixed, mut broken) = (false, HashSet::new(), HashSet::new());
    for line in log.lines() {
        let Some(stamp) = line.get(..19) else {
            continue;
        };
        if stamp < since || !line.contains("[SR]") {
            continue;
        }
        seen = true;
        if line.contains("Cannot repair member file") || line.contains("Could not reproject corrupted file") {
            broken.insert(last_quoted(line).to_ascii_lowercase());
        } else if line.contains("Repairing corrupted file") || line.contains("Repaired file") {
            fixed.insert(last_quoted(line).to_ascii_lowercase());
        }
    }
    let repaired = fixed.difference(&broken).count() as u32;
    CbsSummary { seen, repaired, unrepaired: broken.len() as u32 }
}

const MRT_RUN: &str = "Microsoft Windows Malicious Software Removal Tool v";

fn mrt_runs(log: &str) -> usize {
    log.lines().filter(|l| l.trim_start().starts_with(MRT_RUN)).count()
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct MrtSummary {
    pub clean: bool,
    pub threats: Vec<String>,
}

/// The last run block of `mrt.log`: "No infection found." or `Found <Threat:Name> ...` lines.
/// Only the threat name is kept (the lines that follow carry file paths: never stored).
pub(crate) fn parse_mrt(log: &str) -> MrtSummary {
    let lines: Vec<&str> = log.lines().collect();
    let Some(start) = lines.iter().rposition(|l| l.trim_start().starts_with(MRT_RUN)) else {
        return MrtSummary::default();
    };
    let mut s = MrtSummary::default();
    for l in &lines[start..] {
        let l = l.trim();
        if l.starts_with("No infection found") {
            s.clean = true;
        } else if let Some(rest) = l.strip_prefix("Found ") {
            let name: String = rest
                .split([' ', ','])
                .next()
                .unwrap_or_default()
                .chars()
                .filter(|c| c.is_ascii_graphic())
                .take(100)
                .collect();
            if !name.is_empty() && !s.threats.contains(&name) && s.threats.len() < 20 {
                s.threats.push(name);
            }
        }
    }
    if !s.threats.is_empty() {
        s.clean = false;
    }
    s
}

/* ---------------- history (no paths) ---------------- */

fn history_file() -> PathBuf {
    registry::data_dir().join("repair-history.json")
}

pub fn history() -> Vec<Outcome> {
    std::fs::read_to_string(history_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn record(o: &Outcome) {
    let mut all = history();
    all.push(o.clone());
    if all.len() > HISTORY_MAX {
        let drop = all.len() - HISTORY_MAX;
        all.drain(..drop);
    }
    let f = history_file();
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(f, serde_json::to_string_pretty(&all).unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cbs_counts_this_run_only_and_each_file_once() {
        let log = "\
2026-10-22 09:59:59, Info                  CSI    00000001 [SR] Cannot repair member file [l:8]\"old.dll\" of X, version 1\r
2026-10-22 10:00:00, Info                  CSI    00000002 [SR] Verifying 100 components\r
2026-10-22 10:00:01, Info                  CBS    Not an SR line \"x.dll\"\r
2026-10-22 10:00:05, Info                  CSI    00000003 [SR] Repairing corrupted file [ml:52{26},l:50{25}]\"\\??\\C:\\Windows\\System32\"\\[l:14{7}]\"a.dll\" from store\r
2026-10-22 10:00:05, Info                  CSI    00000004 [SR] Repaired file \\SystemRoot\\WinSxS\\[l:14]\"A.DLL\" by copying from backup\r
2026-10-22 10:00:06, Info                  CSI    00000005 [SR] Cannot repair member file [l:10]\"b.dll\" of Microsoft-Windows-B, version 10.0, arch amd64\r
2026-10-22 10:00:06, Info                  CSI    00000006 [SR] Could not reproject corrupted file [ml:52{26}]\"\\??\\C:\\Windows\"\\[l:10]\"b.dll\"; source file in store is also corrupted\r
2026-10-22 10:00:07, Info                  CSI    00000007 [SR] Repairing corrupted file [l:10]\"b.dll\" from store\r
2026-10-22 10:00:09, Info                  CSI    00000008 [SR] Verify complete\r
short\r
";
        let s = parse_cbs(log, "2026-10-22 10:00:00");
        assert_eq!(s, CbsSummary { seen: true, repaired: 1, unrepaired: 1 });
        // Clean run: [SR] lines, nothing repaired.
        let clean = "2026-10-22 11:00:00, Info  CSI  00000001 [SR] Verifying 100 components\n\
2026-10-22 11:00:09, Info  CSI  00000002 [SR] Verify complete\n";
        assert_eq!(parse_cbs(clean, "2026-10-22 11:00:00"), CbsSummary { seen: true, repaired: 0, unrepaired: 0 });
        // Log rotated / nothing for this run.
        assert!(!parse_cbs(log, "2026-10-23 00:00:00").seen);
        assert!(!parse_cbs("", "2026-10-22 10:00:00").seen);
        // Multi-byte text before column 19 must not panic.
        assert!(!parse_cbs("ção ção ção ção ção ção [SR]", "2026").seen);
    }

    #[test]
    fn mrt_reads_the_last_run_and_keeps_only_threat_names() {
        // Real shape (this PC's mrt.log, 2026-10-14), plus a fabricated infected run.
        let clean = "\
---------------------------------------------------------------------------------------
Microsoft Windows Malicious Software Removal Tool v5.146, (build 5.146.26100.1003)
Started On Wed Oct 14 00:20:24 2026

Results Summary:
----------------
No infection found.
Successfully Submitted Heartbeat Report
Microsoft Windows Malicious Software Removal Tool Finished On Wed Oct 14 00:23:20 2026


Return code: 0 (0x0)
";
        let infected = "\
---------------------------------------------------------------------------------------
Microsoft Windows Malicious Software Removal Tool v5.147, (build 5.147.1.1)
Started On Thu Oct 22 10:00:00 2026

Results Summary:
----------------
Found Trojan:Win32/Fakeav!rts and Removed!
    file://C:\\Users\\someone\\AppData\\evil.exe
Found Virus:Win32/Expiro.gen!F, Not Removed.
Found Trojan:Win32/Fakeav!rts and Removed!
Microsoft Windows Malicious Software Removal Tool Finished On Thu Oct 22 10:05:00 2026

Return code: 13 (0xd)
";
        assert_eq!(mrt_runs(clean), 1);
        assert_eq!(parse_mrt(clean), MrtSummary { clean: true, threats: vec![] });
        let both = format!("{infected}{clean}");
        assert_eq!(mrt_runs(&both), 2);
        assert!(parse_mrt(&both).clean, "only the last run counts");
        let s = parse_mrt(&format!("{clean}{infected}"));
        assert!(!s.clean);
        assert_eq!(s.threats, vec!["Trojan:Win32/Fakeav!rts".to_string(), "Virus:Win32/Expiro.gen!F".to_string()]);
        assert!(s.threats.iter().all(|t| !t.contains('\\')), "no paths");
        assert_eq!(parse_mrt("garbage"), MrtSummary::default());
    }

    #[test]
    fn tool_names_round_trip_and_unknown_tools_are_refused() {
        let t: Tool = serde_json::from_str("\"mrt_full\"").unwrap();
        assert_eq!(t, Tool::MrtFull);
        assert!(serde_json::from_str::<Tool>("\"cmd\"").is_err());
        assert_eq!(serde_json::to_string(&Verdict::NonRepairable).unwrap(), "\"non_repairable\"");
    }

    #[test]
    fn system_paths_come_from_the_api() {
        let s = system32();
        assert!(s.is_absolute() && s.join("kernel32.dll").is_file(), "{}", s.display());
        assert!(windows_dir().join("System32").is_dir());
        assert_eq!(local_stamp().len(), 19);
    }

    #[test]
    fn refuses_without_elevation_and_one_job_at_a_time() {
        if !system::is_elevated() {
            assert!(matches!(run(Tool::Sfc, &|_| {}), Err(RepairError::NeedsAdmin)));
        }
        let held = exclusive().expect("free slot");
        assert!(exclusive().is_none(), "second job refused");
        drop(held);
        assert!(exclusive().is_some());
    }

    /// Elevated, read-only: DISM CheckHealth flag via the scan entry point would take minutes, so
    /// this only proves DismApi loads and initialises. Run elevated by hand:
    /// `g2boost_core-*.exe --ignored repair::tests::dism_api_initialises`
    #[test]
    #[ignore]
    fn dism_api_initialises() {
        assert!(system::is_elevated(), "run elevated");
        dism_api().expect("DismInitialize");
    }
}
