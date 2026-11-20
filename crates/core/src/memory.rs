//! RAM: what the memory is doing right now and who holds it (Docs/boost/memory.md).
//!
//! - **Composition**, Task Manager's split, from PDH `\Memory\*` (English names, no admin): in use,
//!   modified (waiting to be written to disk), standby (cache Windows hands out at once), free,
//!   and compressed when the Memory Compression process is visible.
//! - **Who uses it**: processes grouped by executable path, private commit and working set, and
//!   which of them may be ended (the task manager's own rules, `procs::end` checks again).
//! - **Advice**: purging the standby list only helps when free memory is nearly gone *and* the
//!   cache is big; when real use is near the top a purge does nothing (close apps instead).

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use serde_json::{Value, json};
use windows::Win32::System::Performance::{PDH_HQUERY, PdhCollectQueryData, PdhOpenQueryW};
use windows::core::PCWSTR;

use crate::monitor::{Counter, add_counter, value};
use crate::procs::{self, ProcInfo};

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Composition {
    pub total: u64,
    /// Total − free − standby − modified (Task Manager's "In use"; compressed is part of it).
    pub in_use: u64,
    pub modified: u64,
    pub standby: u64,
    pub free: u64,
    /// The Memory Compression store; `None` when that process is not visible (compression off).
    pub compressed: Option<u64>,
    pub committed: u64,
    pub commit_limit: u64,
    /// `helps` (free nearly gone and a big cache: a purge hands games free pages at once),
    /// `in_use` (real use near the top: a purge frees nothing that matters, close apps),
    /// `fine` (plenty free: nothing to do).
    pub advice: &'static str,
}

/// Task Manager's arithmetic plus the advice; the PDH reads stay in [`composition`].
pub(crate) fn compose(total: u64, free: u64, standby: u64, modified: u64) -> Composition {
    let in_use = total.saturating_sub(free).saturating_sub(standby).saturating_sub(modified);
    let share = |x: u64| x as f64 / total.max(1) as f64;
    let advice = if share(in_use) > 0.85 {
        "in_use"
    } else if share(free) < 0.05 && share(standby) > 0.25 {
        "helps"
    } else {
        "fine"
    };
    Composition { total, in_use, modified, standby, free, advice, ..Default::default() }
}

struct Pdh {
    query: PDH_HQUERY,
    free: Option<Counter>,
    modified: Option<Counter>,
    standby: [Option<Counter>; 3],
    compressed: Option<Counter>,
    committed: Option<Counter>,
    limit: Option<Counter>,
}

// SAFETY: PDH handles are not tied to a thread; the mutex in `composition` serializes every use.
unsafe impl Send for Pdh {}

impl Pdh {
    fn open() -> Option<Self> {
        let mut query = PDH_HQUERY::default();
        // SAFETY: real-time query (no data source). One per process, kept for its lifetime.
        if unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } != 0 {
            return None;
        }
        let c = |path: &str| add_counter(query, path);
        Some(Pdh {
            free: c(r"\Memory\Free & Zero Page List Bytes"),
            modified: c(r"\Memory\Modified Page List Bytes"),
            standby: [
                c(r"\Memory\Standby Cache Core Bytes"),
                c(r"\Memory\Standby Cache Normal Priority Bytes"),
                c(r"\Memory\Standby Cache Reserve Bytes"),
            ],
            // ponytail: the Process object is the costly part of a collect (every process);
            // fine at one read per 2 s while the RAM tab is open.
            compressed: c(r"\Process(Memory Compression)\Working Set"),
            committed: c(r"\Memory\Committed Bytes"),
            limit: c(r"\Memory\Commit Limit"),
            query,
        })
    }
}

/// The memory split right now. `None` when PDH is unavailable (counters disabled).
pub fn composition() -> Option<Composition> {
    static PDH: Mutex<Option<Pdh>> = Mutex::new(None);
    let mut pdh = PDH.lock().unwrap_or_else(|e| e.into_inner());
    if pdh.is_none() {
        *pdh = Pdh::open();
    }
    let p = pdh.as_ref()?;
    // SAFETY: valid query; these counters are instantaneous (one collect is a full reading).
    let _ = unsafe { PdhCollectQueryData(p.query) };
    let total = crate::system::memory_status()?.total_bytes;
    let get = |c: &Option<Counter>| value(c).map(|v| v.max(0.0) as u64);
    let mut c = compose(total, get(&p.free)?, p.standby.iter().filter_map(get).sum(), get(&p.modified).unwrap_or(0));
    c.compressed = get(&p.compressed).filter(|v| *v > 0);
    c.committed = get(&p.committed).unwrap_or(0);
    c.commit_limit = get(&p.limit).unwrap_or(0);
    Some(c)
}

/* ---------------- who uses RAM ---------------- */

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProcRef {
    pub pid: u32,
    /// Creation time, as the decimal string the task manager sends back.
    #[serde(serialize_with = "procs::u64_as_string")]
    pub created: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppMem {
    /// Executable file name, lowercase.
    pub name: String,
    /// Image path (its icon, and the link to a startup item). Never sent to an AI.
    pub path: String,
    pub count: u32,
    /// Sum of private commit: no double count across a family of processes.
    pub private_bytes: u64,
    /// Sum of working sets (shared pages counted in each process).
    pub ws_bytes: u64,
    pub system: bool,
    /// Why none of its processes may be ended (system / security / service / self).
    pub protected: Option<&'static str>,
    /// The processes that may be ended, each checked again by `procs::end`.
    pub procs: Vec<ProcRef>,
}

/// Processes grouped by executable path (case-insensitive), most private memory first, top `n`.
/// A group is protected only when every one of its processes is.
pub fn group(list: &[ProcInfo], n: usize) -> Vec<AppMem> {
    let mut by: HashMap<String, AppMem> = HashMap::new();
    for p in list {
        let g = by.entry(p.path.to_ascii_lowercase()).or_insert_with(|| AppMem {
            name: p.name.clone(),
            path: p.path.clone(),
            count: 0,
            private_bytes: 0,
            ws_bytes: 0,
            system: p.system,
            protected: p.protected,
            procs: Vec::new(),
        });
        g.count += 1;
        g.private_bytes += p.private_bytes;
        g.ws_bytes += p.ram_bytes;
        if p.protected.is_none() {
            g.protected = None;
            g.procs.push(ProcRef { pid: p.pid, created: p.created });
        }
    }
    let mut v: Vec<AppMem> = by.into_values().collect();
    v.sort_by(|a, b| b.private_bytes.cmp(&a.private_bytes).then_with(|| a.name.cmp(&b.name)));
    v.truncate(n);
    v
}

pub fn top_apps(n: usize) -> Vec<AppMem> {
    group(&procs::Sampler::default().sample(), n)
}

/// What the AI sees: the split, the advice and the heaviest apps by file name (no paths, so no
/// user name), with whether each one starts with Windows.
pub fn report(n: usize) -> Value {
    let starts: Vec<String> =
        crate::startup::list().into_iter().filter(|i| i.enabled).filter_map(|i| i.exe).map(|e| e.to_ascii_lowercase()).collect();
    let apps: Vec<Value> = top_apps(n)
        .into_iter()
        .map(|a| {
            json!({
                "name": a.name,
                "processes": a.count,
                "private_bytes": a.private_bytes,
                "working_set_bytes": a.ws_bytes,
                "windows_component": a.system,
                "starts_with_windows": starts.contains(&a.path.to_ascii_lowercase()),
            })
        })
        .collect();
    json!({ "composition": composition(), "top_apps": apps })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1 << 30;

    #[test]
    fn composition_arithmetic_and_advice() {
        let c = compose(16 * GB, GB / 2, 9 * GB, GB / 2);
        assert_eq!(c.in_use, 6 * GB);
        assert_eq!(c.advice, "helps", "free nearly gone, big cache");
        assert_eq!(compose(16 * GB, 8 * GB, 4 * GB, 0).advice, "fine");
        assert_eq!(compose(16 * GB, GB / 4, GB, GB / 4).advice, "in_use", "real use near the top: a purge does nothing");
        assert_eq!(compose(16 * GB, GB / 2, 2 * GB, 0).advice, "fine", "little cache to give back");
        let odd = compose(GB, 2 * GB, GB, GB);
        assert_eq!(odd.in_use, 0, "never underflows on inconsistent counters");
    }

    #[test]
    fn live_composition_adds_up() {
        let c = composition().expect("PDH memory counters");
        assert!(c.total > 0 && c.free + c.standby + c.modified + c.in_use <= c.total + GB, "{c:?}");
        assert!(c.committed > 0 && c.commit_limit >= c.committed);
        assert!(["helps", "in_use", "fine"].contains(&c.advice));
    }

    fn p(pid: u32, path: &str, private: u64, ws: u64, protected: Option<&'static str>) -> ProcInfo {
        ProcInfo {
            pid,
            name: path.rsplit('\\').next().unwrap().to_ascii_lowercase(),
            path: path.into(),
            ram_bytes: ws,
            private_bytes: private,
            cpu_pct: 0.0,
            created: u64::from(pid) * 10,
            system: path.to_ascii_lowercase().starts_with(r"c:\windows"),
            protected,
        }
    }

    #[test]
    fn groups_by_executable_path() {
        let list = [
            p(1, r"C:\Apps\Chrome\chrome.exe", 300, 400, None),
            p(2, r"c:\apps\chrome\CHROME.EXE", 200, 250, None),
            p(3, r"C:\Other\chrome.exe", 50, 60, None),
            p(4, r"C:\Windows\System32\svchost.exe", 900, 950, Some("system")),
            p(5, r"C:\Windows\System32\svchost.exe", 100, 120, Some("service")),
            p(6, r"C:\Games\game.exe", 10, 20, Some("service")),
            p(7, r"C:\Games\game.exe", 10, 20, None),
        ];
        let g = group(&list, 10);
        assert_eq!(g.len(), 4, "same path in any case = one app; another folder = another app");
        assert_eq!((g[0].name.as_str(), g[0].count, g[0].private_bytes), ("svchost.exe", 2, 1000), "heaviest first");
        assert_eq!(g[0].protected, Some("system"));
        assert!(g[0].procs.is_empty(), "nothing in a protected group may be ended");
        let chrome = g.iter().find(|a| a.path == r"C:\Apps\Chrome\chrome.exe").unwrap();
        assert_eq!((chrome.count, chrome.private_bytes, chrome.ws_bytes, chrome.protected), (2, 500, 650, None));
        assert_eq!(chrome.procs, vec![ProcRef { pid: 1, created: 10 }, ProcRef { pid: 2, created: 20 }]);
        let game = g.iter().find(|a| a.name == "game.exe").unwrap();
        assert_eq!((game.protected, game.procs.len()), (None, 1), "only the endable process is offered");
        assert_eq!(group(&list, 2).len(), 2);
        let json = serde_json::to_value(chrome).unwrap();
        assert_eq!(json["procs"][0]["created"], "10", "a creation time does not fit a JS number: string");
    }
}
