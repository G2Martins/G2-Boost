//! What is eating CPU and RAM right now, grouped by executable (chrome.exe ×24 = 2.1 GB).
//! Read-only: G2 Boost never kills processes — closing apps or disabling their startup is the
//! user's call (the startup manager and the AI advice point there).

use std::collections::HashMap;
use std::mem::size_of;
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Foundation::{CloseHandle, FILETIME};
use windows::Win32::System::ProcessStatus::{EnumProcesses, K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::PWSTR;

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
    system: bool,
    ram: u64,
    cpu_100ns: u64,
}

fn ft(f: FILETIME) -> u64 {
    (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)
}

fn snapshot() -> HashMap<u32, Snap> {
    let mut pids = vec![0u32; 2048];
    let mut needed = 0u32;
    // SAFETY: buffer of pids.len()*4 bytes.
    if unsafe { EnumProcesses(pids.as_mut_ptr(), (pids.len() * 4) as u32, &mut needed) }.is_err() {
        return HashMap::new();
    }
    let win = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()).to_ascii_lowercase();
    let mut out = HashMap::new();
    for &pid in &pids[..(needed / 4) as usize] {
        if pid == 0 {
            continue;
        }
        // SAFETY: limited handle, closed below; out-params are plain structs.
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                continue;
            };
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let path = if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
                String::from_utf16_lossy(&buf[..len as usize])
            } else {
                String::new()
            };
            let mut pmc = PROCESS_MEMORY_COUNTERS { cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
            let ram = if K32GetProcessMemoryInfo(h, &mut pmc, pmc.cb).as_bool() { pmc.WorkingSetSize as u64 } else { 0 };
            let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
            let cpu = if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).is_ok() { ft(k) + ft(u) } else { 0 };
            let _ = CloseHandle(h);
            if path.is_empty() {
                continue;
            }
            let lower = path.to_ascii_lowercase();
            let name = lower.rsplit('\\').next().unwrap_or(&lower).to_string();
            out.insert(pid, Snap { system: lower.starts_with(&win), name, ram, cpu_100ns: cpu });
        }
    }
    out
}

/// Two snapshots `window` apart; groups sorted by RAM, top `n`.
pub fn top(window: Duration, n: usize) -> Vec<ProcGroup> {
    let a = snapshot();
    let t0 = Instant::now();
    std::thread::sleep(window);
    let b = snapshot();
    let elapsed_100ns = (t0.elapsed().as_secs_f64() * 1e7).max(1.0);
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
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
}
