//! Persistent per-executable priority rules ("always run obs64.exe at Below normal").
//! The routine thread calls `Enforcer::tick` every few seconds; each process (pid + creation
//! time) is touched at most once in its lifetime, so a game whose anti-cheat denies access is
//! never retried and a priority the user changes later by hand is respected. Never Realtime.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{
    ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, GetPriorityClass, HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS,
    NORMAL_PRIORITY_CLASS, PROCESS_CREATION_FLAGS, PROCESS_SET_INFORMATION, SetPriorityClass,
};

use crate::procs::open_exact;

const MAX_RULES: usize = 50;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
}

impl Class {
    fn win32(self) -> PROCESS_CREATION_FLAGS {
        match self {
            Class::Idle => IDLE_PRIORITY_CLASS,
            Class::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
            Class::Normal => NORMAL_PRIORITY_CLASS,
            Class::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS,
            Class::High => HIGH_PRIORITY_CLASS,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    /// Lowercase file name, e.g. `obs64.exe`.
    pub exe: String,
    pub class: Class,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    pub exe: String,
    pub pid: u32,
    pub class: Class,
    /// false = could not open/set (typically access denied by an anti-cheat or elevation).
    pub ok: bool,
}

/// Never ruled, whatever the user types.
const REFUSED: &[&str] = &[
    "csrss.exe", "smss.exe", "wininit.exe", "winlogon.exe", "services.exe", "lsass.exe", "svchost.exe", "dwm.exe",
    "explorer.exe", "audiodg.exe", "system", "registry", "msmpeng.exe",
];

fn valid_exe(e: &str) -> bool {
    (5..=64).contains(&e.len())
        && e.ends_with(".exe")
        && e.chars().all(|c| c.is_ascii_graphic() || c == ' ')
        && !e.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|'])
        && !e.starts_with([' ', '.'])
        && !REFUSED.contains(&e)
        && !crate::startup::is_security_software(e)
}

/// Keeps only safe rules: bare lowercase `name.exe`, deduplicated (last wins), at most 50.
pub fn sanitize(rules: Vec<Rule>) -> Vec<Rule> {
    let mut out: Vec<Rule> = Vec::new();
    for r in rules {
        let exe = r.exe.trim().to_ascii_lowercase();
        if !valid_exe(&exe) {
            continue;
        }
        out.retain(|o| o.exe != exe);
        out.push(Rule { exe, class: r.class });
    }
    // ponytail: keeps the last 50 survivors; the UI is expected to stop earlier.
    let skip = out.len().saturating_sub(MAX_RULES);
    out.split_off(skip)
}

/// One running process: `(pid, creation time, lowercase exe name, session, image path)`.
type Row = (u32, u64, String, u32, String);

/// Pure selection: which rows get which class. Skips pid 0/4/`me`, session 0, `%SystemRoot%`,
/// security software and anything already handled.
fn select<'a>(rules: &'a [Rule], rows: &'a [Row], seen: &HashSet<(u32, u64)>, win: &str, me: u32) -> Vec<(&'a Row, &'a Rule)> {
    rows.iter()
        .filter(|(pid, created, _, session, path)| {
            !matches!(*pid, 0 | 4)
                && *pid != me
                && *session != 0
                && !seen.contains(&(*pid, *created))
                && !path.to_ascii_lowercase().starts_with(win)
                && !crate::startup::is_security_software(path)
        })
        .filter_map(|row| rules.iter().find(|r| r.exe.eq_ignore_ascii_case(&row.2)).map(|r| (row, r)))
        .collect()
}

#[derive(Default)]
pub struct Enforcer {
    seen: HashSet<(u32, u64)>,
}

impl Enforcer {
    pub fn tick(&mut self, rules: &[Rule]) -> Vec<Applied> {
        if rules.is_empty() {
            self.seen.clear();
            return Vec::new();
        }
        let rows = crate::procs::list();
        self.run(rules, &rows, &system_root(), std::process::id(), apply_one)
    }

    /// Bookkeeping around an injectable setter (`Some(ok)` = acted, `None` = already had that class).
    fn run(&mut self, rules: &[Rule], rows: &[Row], win: &str, me: u32, mut set: impl FnMut(u32, u64, Class) -> Option<bool>) -> Vec<Applied> {
        let live: HashSet<(u32, u64)> = rows.iter().map(|r| (r.0, r.1)).collect();
        self.seen.retain(|k| live.contains(k));
        let todo = select(rules, rows, &self.seen, win, me);
        let mut out = Vec::new();
        for ((pid, created, name, _, _), rule) in todo {
            self.seen.insert((*pid, *created));
            if let Some(ok) = set(*pid, *created, rule.class) {
                out.push(Applied { exe: name.clone(), pid: *pid, class: rule.class, ok });
            }
        }
        out
    }
}

fn system_root() -> String {
    std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()).to_ascii_lowercase()
}

/// Opens the exact process and sets its class; `None` if it already has it.
fn apply_one(pid: u32, created: u64, class: Class) -> Option<bool> {
    let Ok((h, _)) = open_exact(pid, created, PROCESS_SET_INFORMATION) else {
        return Some(false);
    };
    // SAFETY: valid handle from open_exact, closed right after.
    unsafe {
        let r = if GetPriorityClass(h) == class.win32().0 { None } else { Some(SetPriorityClass(h, class.win32()).is_ok()) };
        let _ = CloseHandle(h);
        r
    }
}

/// One-shot application with no memory (use `Enforcer` for the periodic loop).
pub fn apply(rules: &[Rule]) -> Vec<Applied> {
    Enforcer::default().tick(rules)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(exe: &str, class: Class) -> Rule {
        Rule { exe: exe.into(), class }
    }

    #[test]
    fn sanitize_filters_and_dedupes() {
        let out = sanitize(vec![
            r("OBS64.exe", Class::BelowNormal),
            r(r"C:\x\a.exe", Class::Idle),
            r("a/b.exe", Class::Idle),
            r("*.exe", Class::Idle),
            r("a?b.exe", Class::Idle),
            r("a.exe", Class::Idle), // 5 chars
            r("ab.exe", Class::Idle),
            r("noext", Class::Idle),
            r("svchost.exe", Class::High),
            r("explorer.exe", Class::High),
            r("MsMpEng.exe", Class::Idle),
            r("kaspersky.exe", Class::Idle),
            r("obs64.exe", Class::High),
            r(&format!("{}.exe", "a".repeat(61)), Class::Idle),
        ]);
        let names: Vec<_> = out.iter().map(|x| x.exe.as_str()).collect();
        assert_eq!(names, ["a.exe", "ab.exe", "obs64.exe"]);
        assert_eq!(out.iter().find(|x| x.exe == "obs64.exe").unwrap().class, Class::High, "last wins");
        assert!(!out.iter().any(|x| REFUSED.contains(&x.exe.as_str()) || x.exe.contains(['\\', '/', '*', '?'])));
        assert!(out.iter().all(|x| x.exe == x.exe.to_ascii_lowercase() && x.exe.len() >= 5 && x.exe.len() <= 64));
        assert!(!out.iter().any(|x| x.exe.starts_with("kaspersky")));
    }

    #[test]
    fn sanitize_caps_at_50() {
        let many: Vec<Rule> = (0..80).map(|i| r(&format!("app{i:02}.exe"), Class::Idle)).collect();
        let out = sanitize(many);
        assert_eq!(out.len(), 50);
        assert_eq!(out[0].exe, "app30.exe");
    }

    #[test]
    fn realtime_is_not_expressible() {
        assert!(serde_json::from_str::<Class>("\"realtime\"").is_err());
        assert_eq!(serde_json::from_str::<Class>("\"below_normal\"").unwrap(), Class::BelowNormal);
        assert_eq!(serde_json::to_string(&Class::AboveNormal).unwrap(), "\"above_normal\"");
    }

    #[test]
    fn class_maps_to_win32() {
        assert_eq!(Class::Idle.win32().0, 0x40);
        assert_eq!(Class::BelowNormal.win32().0, 0x4000);
        assert_eq!(Class::Normal.win32().0, 0x20);
        assert_eq!(Class::AboveNormal.win32().0, 0x8000);
        assert_eq!(Class::High.win32().0, 0x80);
    }

    fn row(pid: u32, created: u64, name: &str, session: u32, path: &str) -> Row {
        (pid, created, name.into(), session, path.into())
    }

    #[test]
    fn select_applies_protections() {
        let rules = vec![r("game.exe", Class::High), r("svc.exe", Class::Idle), r("win.exe", Class::Idle), r("av.exe", Class::Idle)];
        let rows = vec![
            row(100, 1, "game.exe", 1, r"d:\games\game.exe"),
            row(101, 1, "svc.exe", 0, r"d:\svc.exe"),
            row(102, 1, "win.exe", 1, r"c:\windows\system32\win.exe"),
            row(103, 1, "av.exe", 1, r"c:\program files\kaspersky lab\av.exe"),
            row(4, 1, "game.exe", 1, r"d:\game.exe"),
            row(7, 1, "game.exe", 1, r"d:\me\game.exe"),
            row(104, 1, "other.exe", 1, r"d:\other.exe"),
        ];
        let got = select(&rules, &rows, &HashSet::new(), r"c:\windows", 7);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0.0, 100);
    }

    #[test]
    fn enforcer_touches_each_process_once_and_forgets_dead_ones() {
        let rules = vec![r("game.exe", Class::BelowNormal)];
        let mut e = Enforcer::default();
        let rows = vec![row(100, 1, "game.exe", 1, r"d:\game.exe"), row(101, 5, "game.exe", 1, r"d:\game.exe")];
        let mut calls = Vec::new();
        let a = e.run(&rules, &rows, r"c:\windows", 1, |p, _, _| {
            calls.push(p);
            Some(p == 100) // 101 denied
        });
        assert_eq!(a.len(), 2);
        assert!(a.iter().any(|x| x.pid == 101 && !x.ok));
        // Same processes: nothing more, denied ones are not retried.
        assert!(e.run(&rules, &rows, r"c:\windows", 1, |_, _, _| panic!("retried")).is_empty());
        // Pid 100 exits and the pid is reused with a new creation time: handled again.
        let rows2 = vec![row(100, 9, "game.exe", 1, r"d:\game.exe")];
        assert_eq!(e.run(&rules, &rows2, r"c:\windows", 1, |_, _, _| Some(true)).len(), 1);
        assert_eq!(e.seen.len(), 1, "dead entries dropped");
        // Already at the wanted class: remembered but not reported.
        let rows3 = vec![row(200, 1, "game.exe", 1, r"d:\game.exe")];
        assert!(e.run(&rules, &rows3, r"c:\windows", 1, |_, _, _| None).is_empty());
        assert!(e.seen.contains(&(200, 1)));
    }

    /// Child-process body for the live test (ignored on its own).
    #[test]
    #[ignore]
    fn sleep_helper() {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }

    /// Live: sets BelowNormal on a child this test spawns, verifies it, kills it.
    #[test]
    #[ignore]
    fn live_sets_below_normal_on_own_child() {
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        let exe = std::env::current_exe().unwrap();
        let mut child = std::process::Command::new(&exe)
            .args(["--ignored", "--exact", "prio::tests::sleep_helper", "--test-threads=1"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let name = exe.file_name().unwrap().to_string_lossy().to_ascii_lowercase();
        let applied = apply(&[Rule { exe: name, class: Class::BelowNormal }]);
        // SAFETY: query-only handle on our own child, closed below.
        let cls = unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, child.id()).unwrap();
            let c = GetPriorityClass(h);
            let _ = CloseHandle(h);
            c
        };
        let _ = child.kill();
        let _ = child.wait();
        let mine = applied.iter().find(|a| a.pid == child.id()).expect("child applied");
        assert!(mine.ok);
        assert_eq!(cls, BELOW_NORMAL_PRIORITY_CLASS.0);
    }
}
