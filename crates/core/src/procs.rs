//! Processes: what is eating CPU and RAM right now — grouped by executable for the startup
//! advice (`top`, read-only) and per process for the task manager (`Sampler`). Ending a process
//! is an explicit, confirmed user action (`end`), checked here against the exact process the
//! UI saw (pid + creation time, so a recycled pid is never hit) and refused for anything that
//! keeps Windows or its security running.

use std::collections::HashMap;
use std::mem::size_of;
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{
    GetPriorityClass, GetProcessTimes, HIGH_PRIORITY_CLASS, IsProcessCritical, NORMAL_PRIORITY_CLASS, OpenProcess,
    PROCESS_ACCESS_RIGHTS, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, PROCESS_TERMINATE,
    QueryFullProcessImageNameW, SetPriorityClass, TerminateProcess,
};
use windows::core::{BOOL, PWSTR};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProcGroup {
    /// Executable file name, lowercase.
    pub name: String,
    pub count: u32,
    /// Sum of working sets.
    pub ram_bytes: u64,
    /// Share of total CPU capacity over the sampling window (0–100).
    pub cpu_pct: f32,
    /// Lives under C:\Windows (system component: informative only).
    pub system: bool,
}

struct Snap {
    name: String,
    path: String,
    system: bool,
    ram: u64,
    cpu_100ns: u64,
    /// Creation time (FILETIME): with the pid, the identity of this exact process.
    created: u64,
    /// Terminal session; 0 = services.
    session: u32,
}

fn ft(f: FILETIME) -> u64 {
    (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)
}

fn system_root() -> String {
    std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()).to_ascii_lowercase()
}

/// Image path, CPU time and creation time of an open process.
fn query(h: HANDLE) -> (String, u64, u64) {
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: buffer sized for the call; FILETIMEs are plain out-structs.
    unsafe {
        let path = if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            String::new()
        };
        let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        let (cpu, created) = if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).is_ok() { (ft(k) + ft(u), ft(c)) } else { (0, 0) };
        (path, cpu, created)
    }
}

fn snapshot() -> HashMap<u32, Snap> {
    let win = system_root();
    let mut out = HashMap::new();
    for pid in crate::system::pids() {
        if pid == 0 {
            continue;
        }
        // SAFETY: limited handle, closed below.
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                continue;
            };
            let (path, cpu, created) = query(h);
            let mut pmc = PROCESS_MEMORY_COUNTERS { cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
            let ram = if K32GetProcessMemoryInfo(h, &mut pmc, pmc.cb).as_bool() { pmc.WorkingSetSize as u64 } else { 0 };
            let _ = CloseHandle(h);
            if path.is_empty() {
                continue;
            }
            let lower = path.to_ascii_lowercase();
            let name = lower.rsplit('\\').next().unwrap_or(&lower).to_string();
            let mut session = 0u32;
            let _ = ProcessIdToSessionId(pid, &mut session);
            out.insert(pid, Snap { system: lower.starts_with(&win), name, path, ram, cpu_100ns: cpu, created, session });
        }
    }
    out
}

fn cores() -> f64 {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64
}

/// Two snapshots `window` apart; groups sorted by RAM, top `n`.
pub fn top(window: Duration, n: usize) -> Vec<ProcGroup> {
    let a = snapshot();
    let t0 = Instant::now();
    std::thread::sleep(window);
    let b = snapshot();
    let elapsed_100ns = (t0.elapsed().as_secs_f64() * 1e7).max(1.0);
    let cores = cores();
    let mut groups: HashMap<String, ProcGroup> = HashMap::new();
    for (pid, s) in &b {
        let cpu_delta = a.get(pid).map(|p| s.cpu_100ns.saturating_sub(p.cpu_100ns)).unwrap_or(0) as f64;
        let g = groups.entry(s.name.clone()).or_insert_with(|| ProcGroup { name: s.name.clone(), count: 0, ram_bytes: 0, cpu_pct: 0.0, system: s.system });
        g.count += 1;
        g.ram_bytes += s.ram;
        g.cpu_pct += (cpu_delta / elapsed_100ns / cores * 100.0) as f32;
    }
    let mut v: Vec<ProcGroup> = groups.into_values().collect();
    v.sort_by_key(|g| std::cmp::Reverse(g.ram_bytes));
    v.truncate(n);
    for g in &mut v {
        g.cpu_pct = (g.cpu_pct * 10.0).round() / 10.0;
    }
    v
}

/* ---------------- task manager ---------------- */

#[derive(Debug, Clone, Serialize)]
pub struct ProcInfo {
    pub pid: u32,
    /// Executable file name, lowercase.
    pub name: String,
    pub path: String,
    pub ram_bytes: u64,
    /// Share of total CPU capacity since the previous sample (0–100).
    pub cpu_pct: f32,
    /// Creation time (FILETIME); the UI sends it back with the pid to act on this exact process.
    /// Serialized as a string: ~1.3e17 does not fit a JS number exactly.
    #[serde(serialize_with = "u64_as_string")]
    pub created: u64,
    pub system: bool,
    /// Why G2 Boost will not end it: system / security / service / self (`None` = may be ended after confirmation).
    pub protected: Option<&'static str>,
}

fn u64_as_string<S: serde::Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&v.to_string())
}

/// pid → (CPU time, creation time) at the previous sample.
type CpuTimes = HashMap<u32, (u64, u64)>;

/// Live per-process list: CPU % is the delta since the previous `sample` call (the UI polls).
#[derive(Default)]
pub struct Sampler {
    prev: Option<(Instant, CpuTimes)>,
}

/// The reason a process is off-limits, judged from what the list already knows.
fn listed_protection(pid: u32, s: &Snap) -> Option<&'static str> {
    if pid == std::process::id() {
        Some("self")
    } else if pid == 4 || s.system {
        Some("system")
    } else if crate::startup::is_security_software(&s.path) {
        Some("security")
    } else if s.session == 0 {
        Some("service")
    } else {
        None
    }
}

impl Sampler {
    pub fn sample(&mut self) -> Vec<ProcInfo> {
        let now = Instant::now();
        let snap = snapshot();
        let (elapsed_100ns, prev) = match &self.prev {
            Some((t, p)) => ((now.duration_since(*t).as_secs_f64() * 1e7).max(1.0), Some(p)),
            None => (1.0, None),
        };
        let cores = cores();
        let mut out: Vec<ProcInfo> = snap
            .iter()
            .map(|(&pid, s)| {
                // Same pid but another creation time = a recycled pid: no delta.
                let cpu = prev
                    .and_then(|p| p.get(&pid))
                    .filter(|(_, created)| *created == s.created)
                    .map_or(0.0, |(cpu, _)| s.cpu_100ns.saturating_sub(*cpu) as f64 / elapsed_100ns / cores * 100.0);
                ProcInfo {
                    pid,
                    name: s.name.clone(),
                    path: s.path.clone(),
                    ram_bytes: s.ram,
                    cpu_pct: ((cpu * 10.0).round() / 10.0).min(100.0) as f32,
                    created: s.created,
                    system: s.system,
                    protected: listed_protection(pid, s),
                }
            })
            .collect();
        out.sort_by_key(|p| std::cmp::Reverse(p.ram_bytes));
        self.prev = Some((now, snap.into_iter().map(|(pid, s)| (pid, (s.cpu_100ns, s.created))).collect()));
        out
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ProcError {
    /// Exited, or the pid now belongs to another process.
    #[error("process_gone")]
    Gone,
    /// System, critical, service (session 0) or security process — or G2 Boost itself.
    #[error("protected_process")]
    Protected,
    #[error("needs_admin")]
    NeedsAdmin,
    #[error("win32: {0}")]
    Win32(String),
}

/// Open `pid` with `access` and prove it is still the process created at `created`.
fn open_exact(pid: u32, created: u64, access: PROCESS_ACCESS_RIGHTS) -> Result<(HANDLE, String), ProcError> {
    if pid == 0 || pid == 4 || pid == std::process::id() {
        return Err(ProcError::Protected);
    }
    // SAFETY: handle closed by the caller (or below on mismatch).
    let h = unsafe { OpenProcess(access | PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.map_err(|e| {
        // Access denied = elevated/protected process; invalid parameter = gone.
        if e.code().0 as u32 == 0x8007_0005 { ProcError::NeedsAdmin } else { ProcError::Gone }
    })?;
    let (path, _, real_created) = query(h);
    if real_created != created || path.is_empty() {
        // SAFETY: handle from OpenProcess above.
        let _ = unsafe { CloseHandle(h) };
        return Err(ProcError::Gone);
    }
    Ok((h, path))
}

/// End the exact process the user confirmed. Refuses system/critical/service/security processes.
pub fn end(pid: u32, created: u64) -> Result<(), ProcError> {
    let (h, path) = open_exact(pid, created, PROCESS_TERMINATE)?;
    let result = (|| {
        let lower = path.to_ascii_lowercase();
        if lower.starts_with(&system_root()) || crate::startup::is_security_software(&lower) {
            return Err(ProcError::Protected);
        }
        let mut critical = BOOL(0);
        let mut session = 0u32;
        // SAFETY: valid handle/out-params. Critical = the system bugchecks if it dies;
        // session 0 = a service process (stopping services is not a task-manager click here).
        unsafe {
            if IsProcessCritical(h, &mut critical).is_err() || critical.as_bool() {
                return Err(ProcError::Protected);
            }
            if ProcessIdToSessionId(pid, &mut session).is_err() || session == 0 {
                return Err(ProcError::Protected);
            }
            TerminateProcess(h, 1).map_err(|e| ProcError::Win32(e.message()))
        }
    })();
    // SAFETY: handle from open_exact.
    let _ = unsafe { CloseHandle(h) };
    result
}

/// Every process this user can see, as (pid, image path); unreadable paths are left out.
pub fn list_paths() -> Vec<(u32, String)> {
    snapshot().into_iter().filter(|(_, s)| !s.path.is_empty()).map(|(pid, s)| (pid, s.path)).collect()
}

/// Raise a game to high priority (never realtime). Only a process whose image lives under
/// `inside` (the game's install folder) and that is a normal user-session process: the pid comes
/// from a detection a few seconds old and could have been reused.
pub fn raise_priority(pid: u32, inside: &str) -> Result<(), ProcError> {
    set_priority(pid, inside, HIGH_PRIORITY_CLASS)
}

/// Back to normal priority (Game Mode turned off while the game still runs).
pub fn normal_priority(pid: u32, inside: &str) -> Result<(), ProcError> {
    set_priority(pid, inside, NORMAL_PRIORITY_CLASS)
}

fn set_priority(pid: u32, inside: &str, class: windows::Win32::System::Threading::PROCESS_CREATION_FLAGS) -> Result<(), ProcError> {
    if pid == 0 || pid == 4 || pid == std::process::id() || inside.len() < 4 {
        return Err(ProcError::Protected);
    }
    // SAFETY: handle closed below.
    let h = unsafe { OpenProcess(PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.map_err(|e| {
        if e.code().0 as u32 == 0x8007_0005 { ProcError::NeedsAdmin } else { ProcError::Gone }
    })?;
    let result = (|| {
        let (path, _, _) = query(h);
        let dir = format!("{}\\", inside.trim_end_matches('\\').to_ascii_lowercase());
        if !path.to_ascii_lowercase().starts_with(&dir) {
            return Err(ProcError::Gone);
        }
        let mut session = 0u32;
        // SAFETY: valid handle and out-param.
        unsafe {
            if ProcessIdToSessionId(pid, &mut session).is_err() || session == 0 {
                return Err(ProcError::Protected);
            }
            if GetPriorityClass(h) == class.0 {
                return Ok(());
            }
            SetPriorityClass(h, class).map_err(|e| ProcError::Win32(e.message()))
        }
    })();
    // SAFETY: handle from OpenProcess above.
    let _ = unsafe { CloseHandle(h) };
    result
}

/// Image path of the exact process (for "open file location"); never a path from the caller.
pub fn path_of(pid: u32, created: u64) -> Result<String, ProcError> {
    let (h, path) = open_exact(pid, created, PROCESS_ACCESS_RIGHTS(0))?;
    // SAFETY: handle from open_exact.
    let _ = unsafe { CloseHandle(h) };
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_lists_this_process_family() {
        let v = top(Duration::from_millis(300), 200);
        assert!(!v.is_empty());
        let me = std::env::current_exe().unwrap().file_name().unwrap().to_string_lossy().to_ascii_lowercase();
        assert!(v.iter().any(|g| g.name == me), "test binary present");
        assert!(v.iter().all(|g| g.count >= 1 && g.cpu_pct >= 0.0 && g.cpu_pct <= 100.5));
        assert!(v.windows(2).all(|w| w[0].ram_bytes >= w[1].ram_bytes));
    }

    #[test]
    fn sampler_and_refusals() {
        let mut s = Sampler::default();
        let first = s.sample();
        std::thread::sleep(Duration::from_millis(200));
        let list = s.sample();
        assert!(!first.is_empty() && !list.is_empty());
        let me = list.iter().find(|p| p.pid == std::process::id()).expect("self listed");
        assert_eq!(me.protected, Some("self"), "never offers to end G2 Boost itself");
        assert!(list.iter().filter(|p| p.system).all(|p| p.protected.is_some()));
        // Self and pid 4 are refused before anything is opened; a wrong creation time is "gone".
        assert_eq!(end(std::process::id(), me.created), Err(ProcError::Protected));
        assert_eq!(end(4, 0), Err(ProcError::Protected));
        let other = list.iter().find(|p| p.protected.is_none() && p.pid != std::process::id());
        if let Some(o) = other {
            assert_eq!(end(o.pid, o.created.wrapping_add(1)), Err(ProcError::Gone), "recycled pid never hit");
            assert_eq!(path_of(o.pid, o.created.wrapping_add(1)), Err(ProcError::Gone));
        }
        // A system process (explorer/svchost live under %SystemRoot%) is refused even with its real identity.
        if let Some(sys) = list.iter().find(|p| p.system && p.name == "explorer.exe") {
            assert!(matches!(end(sys.pid, sys.created), Err(ProcError::Protected | ProcError::NeedsAdmin)));
        }
    }

    /// Child-process body for the test below (ignored on its own).
    #[test]
    #[ignore]
    fn sleep_helper() {
        std::thread::sleep(Duration::from_secs(30));
    }

    /// Real terminate through every check: this test binary relaunches itself running only
    /// `sleep_helper` (outside System32, user session, not critical) and ends that child.
    #[test]
    fn ends_a_child_process_by_identity() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "procs::tests::sleep_helper", "--test-threads=1"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        std::thread::sleep(Duration::from_millis(400));
        let info = Sampler::default().sample().into_iter().find(|p| p.pid == pid).expect("child listed");
        assert_eq!(info.protected, None);
        assert!(path_of(pid, info.created).unwrap().ends_with(".exe"));
        end(pid, info.created).unwrap();
        assert_eq!(child.wait().unwrap().code(), Some(1), "terminated with our exit code");
        assert_eq!(end(pid, info.created), Err(ProcError::Gone), "already gone");
    }
}
