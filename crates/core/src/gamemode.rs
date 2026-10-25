//! Game Mode: when a game the user picked starts, G2 Boost switches to the Ultimate Performance
//! plan, raises the game's process to high priority, gives it the CPU cores picked in its own
//! options (CPU Sets), purges the standby list (admin only) and pauses its own scheduled cleanup;
//! when the game closes, everything goes back. The session is saved to disk before anything
//! changes, so a crash or a forced exit restores it on the next start. The per-game GPU
//! preference (`set_gpu_high_perf`) is the one persistent setting, with its own backup.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::boost::{BoostError, DX_PREFS, dx_get, dx_set};
use crate::games::{InstalledGame, RunningGame};
use crate::registry::{self, Backup, BackupEntry, Root, Value};

/// What Game Mode does; every item is undone when the game closes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Options {
    /// Ultimate Performance plan while playing (created once if missing), then the previous plan.
    pub power_plan: bool,
    /// High (never realtime) priority for the game's process.
    pub high_priority: bool,
    /// Purge the standby list when the game starts (needs the app elevated; skipped otherwise).
    pub purge_ram: bool,
    /// G2 Boost's scheduled cleanup waits until the game closes.
    pub pause_routines: bool,
    /// Mark the game's outbound traffic as high priority (QoS DSCP 46, ActiveStore: a reboot
    /// clears it). Needs the app elevated; helps only where the router honors DSCP.
    pub network_priority: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { power_plan: true, high_priority: true, purge_ram: true, pause_routines: true, network_priority: true }
    }
}

/// Per-game options (settings `game_tweaks`, keyed by game id). The CPU options apply while the
/// game runs under Game Mode and are undone when it closes; `gpu_high_perf` is a Windows setting
/// that stays (Windows reads it when the game starts) until the user turns it off.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct GameTweaks {
    /// Keep the game off physical core 0 (and its SMT sibling), where most interrupts land.
    pub free_core0: bool,
    /// One logical processor per physical core (no SMT sibling).
    pub physical_only: bool,
    /// Only the fastest cores on hybrid CPUs (P-cores).
    pub perf_cores_only: bool,
    /// Windows' per-app graphics preference "High performance" for the game's main exe.
    pub gpu_high_perf: bool,
    /// High priority for this game under Game Mode even when the global option is off.
    pub high_priority: bool,
}

impl GameTweaks {
    pub fn any(&self) -> bool {
        self.free_core0 || self.physical_only || self.perf_cores_only || self.gpu_high_perf || self.high_priority
    }
}

/// A game being played under Game Mode, and what must be undone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Session {
    pub game_id: String,
    pub game_name: String,
    pub pid: Option<u32>,
    pub install_dir: Option<String>,
    pub started: u64,
    /// Plan active before Game Mode (restored at the end).
    pub prev_plan: Option<String>,
    /// What was actually applied: `power_plan`, `priority`, `ram`.
    pub applied: Vec<String>,
    pub ram_freed: u64,
    /// Why an option did not apply (`needs_admin`, `plan_failed`, `priority_denied`,
    /// `qos_needs_admin`, `qos_failed`, `affinity_denied`).
    pub skipped: Vec<String>,
    /// QoS policy created for this session (removed at the end).
    #[serde(default)]
    pub qos_policy: Option<String>,
    /// CPU Sets given to the game (cleared at the end). Written before the change.
    #[serde(default)]
    pub affinity: Option<Vec<u32>>,
}

fn file() -> PathBuf {
    registry::data_dir().join("gamemode_session.json")
}

/// The session in progress (survives a crash), validated as hostile input.
pub fn session() -> Option<Session> {
    let s: Session = serde_json::from_str(&std::fs::read_to_string(file()).ok()?).ok()?;
    let plan_ok = s.prev_plan.as_deref().is_none_or(|g| g.len() <= 40 && g.chars().all(|c| c.is_ascii_hexdigit() || "-{}".contains(c)));
    (s.game_id.len() <= 200 && plan_ok).then_some(s)
}

fn save(s: &Session) {
    let _ = std::fs::create_dir_all(registry::data_dir());
    let _ = std::fs::write(file(), serde_json::to_string_pretty(s).unwrap_or_default());
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Keep,
    /// Index into `running`.
    Start(usize),
    Stop,
}

/// Pure: a session ends when its game is gone; otherwise the first running game the user turned
/// Game Mode on for starts one.
pub fn decide(current: Option<&str>, running: &[RunningGame], enabled: &[String]) -> Decision {
    match current {
        Some(id) if running.iter().any(|r| r.id == id) => Decision::Keep,
        Some(_) => Decision::Stop,
        None => running.iter().position(|r| enabled.contains(&r.id)).map_or(Decision::Keep, Decision::Start),
    }
}

fn active_plan() -> Option<String> {
    crate::power::plans().plans.into_iter().find(|p| p.active).map(|p| p.guid)
}

/// Apply Game Mode for `game` (its process `pid`, when known) with its per-game `tweaks`. The
/// session is written before the first change so it can always be undone.
pub fn start(game: &InstalledGame, pid: Option<u32>, opts: &Options, tweaks: &GameTweaks) -> Session {
    let mut s = Session {
        game_id: game.id.clone(),
        game_name: game.name.clone(),
        pid,
        install_dir: game.install_dir.clone(),
        started: registry::now_unix(),
        prev_plan: None,
        applied: Vec::new(),
        ram_freed: 0,
        skipped: Vec::new(),
        qos_policy: None,
        affinity: None,
    };
    if opts.power_plan {
        s.prev_plan = active_plan();
        save(&s);
        match crate::power::create_max_performance().and_then(|g| {
            if s.prev_plan.as_deref() != Some(g.as_str()) {
                crate::power::set_active(&g)?;
            }
            Ok(())
        }) {
            Ok(()) => s.applied.push("power_plan".into()),
            Err(_) => s.skipped.push("plan_failed".into()),
        }
    }
    if (opts.high_priority || tweaks.high_priority)
        && let (Some(pid), Some(dir)) = (pid, game.install_dir.as_deref())
    {
        match crate::procs::raise_priority(pid, dir) {
            Ok(()) => s.applied.push("priority".into()),
            Err(_) => s.skipped.push("priority_denied".into()), // anti-cheat protected or elevated game
        }
    }
    if let (Some(pid), Some(dir)) = (pid, game.install_dir.as_deref())
        && let Some(ids) = crate::cpusets::select(&crate::cpusets::topology(), tweaks)
    {
        s.affinity = Some(ids.clone());
        save(&s); // recorded first: a crash in between is still undone on the next start
        match crate::procs::set_cpu_sets(pid, dir, Some(&ids)) {
            Ok(()) => s.applied.push("affinity".into()),
            Err(_) => {
                s.affinity = None;
                s.skipped.push("affinity_denied".into()); // anti-cheat protected: not insisting
            }
        }
    }
    if opts.purge_ram {
        if crate::system::is_elevated() {
            if let Ok(p) = crate::boost::purge_standby_list() {
                s.ram_freed = p.freed_bytes;
                s.applied.push("ram".into());
            }
        } else {
            s.skipped.push("needs_admin".into());
        }
    }
    if opts.network_priority {
        // The running exe's file name (a launcher-started game can live in a subfolder).
        let exe = pid
            .and_then(|p| crate::procs::list_paths().into_iter().find(|(x, _)| *x == p).map(|(_, path)| path))
            .or_else(|| game.main_exe.clone())
            .and_then(|p| std::path::Path::new(&p).file_name().map(|n| n.to_string_lossy().into_owned()));
        match exe.map(|e| crate::qos::set_game(&e)) {
            Some(Ok(policy)) => {
                s.qos_policy = Some(policy);
                s.applied.push("qos".into());
            }
            Some(Err(crate::qos::QosError::NeedsAdmin)) => s.skipped.push("qos_needs_admin".into()),
            Some(Err(_)) | None => s.skipped.push("qos_failed".into()),
        }
    }
    save(&s);
    s
}

/// Undo the session (if any): previous power plan back, normal priority if the game still runs.
/// The QoS policy is removed too (it lives in the ActiveStore: a reboot would clear it anyway).
pub fn stop() -> Option<Session> {
    let s = session()?;
    if let Some(plan) = &s.prev_plan
        && s.applied.iter().any(|a| a == "power_plan")
    {
        let _ = crate::power::set_active(plan);
    }
    if s.applied.iter().any(|a| a == "priority")
        && let (Some(pid), Some(dir)) = (s.pid, s.install_dir.as_deref())
    {
        let _ = crate::procs::normal_priority(pid, dir); // fails harmlessly once the game exited
    }
    if s.affinity.is_some()
        && let (Some(pid), Some(dir)) = (s.pid, s.install_dir.as_deref())
    {
        let _ = crate::procs::set_cpu_sets(pid, dir, None); // every CPU again; gone with the game anyway
    }
    if let Some(policy) = &s.qos_policy {
        let _ = crate::qos::clear(policy);
    }
    let _ = std::fs::remove_file(file());
    Some(s)
}

/* ---------------- graphics preference (persistent, per exe) ---------------- */

const GPU_KEY: &str = "GpuPreference";
const GPU_BACKUP: &str = "gpu_pref";

/// The game's main exe, when it is safe to name in `UserGpuPreferences`: an absolute local path
/// (drive letter, no UNC, no `..`), ending in `.exe`, inside the install folder when there is one,
/// and present on disk (`games::exe_ok`). Hand-added games known only by name have none.
pub fn gpu_exe(game: &InstalledGame) -> Option<String> {
    crate::games::exe_ok(game.main_exe.as_deref()?, game.install_dir.as_deref())
}

/// Set (`on`) or undo Windows' "High performance" GPU for `exe`. Only the `GpuPreference` key of
/// the value is touched (the same string can carry Auto HDR / VRR / flip-model choices); the
/// previous `GpuPreference` is kept in the `gpu_pref` backup and comes back on `off`.
pub fn set_gpu_high_perf(exe: &str, on: bool) -> Result<(), BoostError> {
    gpu_pref_write(DX_PREFS, exe, on)
}

fn gpu_pref_write(key: &str, exe: &str, on: bool) -> Result<(), BoostError> {
    let current = match registry::read(Root::Hkcu, key, exe)? {
        Value::Str(s) => s,
        _ => String::new(),
    };
    let mut backup = registry::load_backup(GPU_BACKUP)
        .unwrap_or_else(|| Backup { tweak: GPU_BACKUP.into(), taken_at: registry::now_unix(), entries: Vec::new() });
    let pos = backup.entries.iter().position(|e| e.name.eq_ignore_ascii_case(exe));
    let next = if on {
        if pos.is_none() {
            let prev = dx_get(&current, GPU_KEY).map_or(Value::Absent, |v| Value::Str(v.to_string()));
            backup.entries.push(BackupEntry { root: Root::Hkcu, path: key.into(), name: exe.into(), value: prev });
            registry::save_backup(&backup)?;
        }
        dx_set(&current, GPU_KEY, Some("2"))
    } else {
        // The backup file is hostile input too: only a plain number goes back into the string.
        let prev = pos.map(|i| backup.entries.remove(i)).and_then(|e| match e.value {
            Value::Str(v) if !v.is_empty() && v.len() <= 2 && v.chars().all(|c| c.is_ascii_digit()) => Some(v),
            _ => None,
        });
        dx_set(&current, GPU_KEY, prev.as_deref())
    };
    registry::write(Root::Hkcu, key, exe, &if next.is_empty() { Value::Absent } else { Value::Str(next) })?;
    if !on && pos.is_some() {
        if backup.entries.is_empty() {
            registry::delete_backup(GPU_BACKUP);
        } else {
            registry::save_backup(&backup)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_exe_only_for_a_real_local_exe_inside_the_game() {
        let dir = std::env::temp_dir().join(format!("g2boost-gpuexe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("Game.exe");
        std::fs::write(&exe, b"MZ").unwrap();
        let d = dir.to_string_lossy().into_owned();
        let g = |main: &str, install: Option<&str>| InstalledGame {
            id: "steam:1".into(),
            name: "G".into(),
            launcher: crate::games::Launcher::Steam,
            install_dir: install.map(str::to_string),
            main_exe: Some(main.to_string()),
            size_bytes: None,
        };
        let e = exe.to_string_lossy().into_owned();
        assert_eq!(gpu_exe(&g(&e, Some(&d))), Some(e.clone()));
        assert_eq!(gpu_exe(&g(&e, None)), Some(e.clone()));
        assert_eq!(gpu_exe(&g(&format!(r"{d}\..\{}\Game.exe", dir.file_name().unwrap().to_string_lossy()), Some(&d))), None, "no ..");
        assert_eq!(gpu_exe(&g(&e, Some(r"C:\Other"))), None, "outside the install folder");
        assert_eq!(gpu_exe(&g(r"\\server\share\Game.exe", None)), None, "UNC");
        assert_eq!(gpu_exe(&g(&format!(r"{d}\missing.exe"), Some(&d))), None, "must exist");
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        assert_eq!(gpu_exe(&g(&format!(r"{d}\notes.txt"), Some(&d))), None, ".exe only");
        assert_eq!(gpu_exe(&InstalledGame { main_exe: None, ..g(&e, None) }), None, "hand-added games");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gpu_preference_edits_only_its_key_and_restores_it() {
        // SAFETY: same override every data-dir test in this crate uses.
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
        const KEY: &str = r"Software\G2 Boost\Test\gpu_pref";
        let exe = r"C:\Games\g2boost-test\game.exe";
        let other = r"C:\Games\g2boost-test\other.exe";
        registry::delete_backup(GPU_BACKUP);
        let read = |name: &str| registry::read(Root::Hkcu, KEY, name).unwrap();
        // A value Windows' own Settings wrote, with Auto HDR beside the GPU choice.
        registry::write(Root::Hkcu, KEY, exe, &Value::Str("GpuPreference=1;AutoHDREnable=1;".into())).unwrap();
        gpu_pref_write(KEY, exe, true).unwrap();
        assert_eq!(read(exe), Value::Str("AutoHDREnable=1;GpuPreference=2;".into()));
        gpu_pref_write(KEY, exe, true).unwrap(); // twice: the first backup is kept
        gpu_pref_write(KEY, other, true).unwrap();
        assert_eq!(read(other), Value::Str("GpuPreference=2;".into()));
        gpu_pref_write(KEY, exe, false).unwrap();
        assert_eq!(read(exe), Value::Str("AutoHDREnable=1;GpuPreference=1;".into()), "previous choice back, HDR kept");
        gpu_pref_write(KEY, other, false).unwrap();
        assert_eq!(read(other), Value::Absent, "nothing was there: value removed");
        assert!(registry::load_backup(GPU_BACKUP).is_none(), "backup gone with the last entry");
        // A tampered backup cannot smuggle extra keys into the string.
        gpu_pref_write(KEY, other, true).unwrap();
        let mut b = registry::load_backup(GPU_BACKUP).unwrap();
        b.entries[0].value = Value::Str("2;Evil=1".into());
        registry::save_backup(&b).unwrap();
        gpu_pref_write(KEY, other, false).unwrap();
        assert_eq!(read(other), Value::Absent);
        registry::write(Root::Hkcu, KEY, exe, &Value::Absent).unwrap();
    }

    fn running(ids: &[&str]) -> Vec<RunningGame> {
        ids.iter().map(|id| RunningGame { id: (*id).into(), pid: Some(1) }).collect()
    }

    #[test]
    fn decide_starts_keeps_and_stops() {
        let enabled = vec!["steam:730".to_string(), "app:x".to_string()];
        assert_eq!(decide(None, &running(&["epic:a", "steam:730"]), &enabled), Decision::Start(1));
        assert_eq!(decide(None, &running(&["epic:a"]), &enabled), Decision::Keep, "not enabled");
        assert_eq!(decide(None, &[], &enabled), Decision::Keep);
        assert_eq!(decide(Some("steam:730"), &running(&["steam:730"]), &enabled), Decision::Keep);
        assert_eq!(decide(Some("steam:730"), &running(&["app:x"]), &enabled), Decision::Stop, "its game closed");
        assert_eq!(decide(Some("steam:730"), &[], &enabled), Decision::Stop);
    }

    /// End to end on this machine (`--ignored`): a fake game (`G2BOOST_FAKE_GAME` = path of a
    /// sleeping exe) in a temp "install folder" → Game Mode on → plan switched + priority high (+ RAM
    /// purged when elevated) → stop → previous plan back. Also written to %TEMP%\g2boost-live.
    #[test]
    #[ignore]
    fn live_session_switches_plan_and_priority_then_restores() {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{GetPriorityClass, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        let mut log = String::new();
        let Some(src) = std::env::var_os("G2BOOST_FAKE_GAME").map(PathBuf::from).filter(|p| p.is_file()) else {
            println!("G2BOOST_FAKE_GAME not set: skipped");
            return;
        };
        // SAFETY: same override every data-dir test in this crate uses.
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
        let dir = std::env::temp_dir().join(format!("g2boost-fakegame-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let exe = dir.join("fakegame.exe");
        std::fs::copy(&src, &exe).unwrap();
        let mut child = std::process::Command::new(&exe).arg("120").spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(400));
        let game = InstalledGame {
            id: "exe:fakegame.exe".into(),
            name: "Fake Game".into(),
            launcher: crate::games::Launcher::Custom,
            install_dir: Some(dir.to_string_lossy().into_owned()),
            main_exe: Some(exe.to_string_lossy().into_owned()),
            size_bytes: None,
        };
        let procs = crate::procs::list_paths();
        let running = crate::games::running(std::slice::from_ref(&game), &procs, None);
        log += &format!("detected running: {running:?}\n");
        assert_eq!(running.first().and_then(|r| r.pid), Some(child.id()), "running detection by install folder");
        // Start from Balanced so the switch is visible; the user's own plan comes back at the end.
        let original = active_plan();
        const BALANCED: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
        crate::power::set_active(BALANCED).unwrap();
        let before = active_plan();
        assert_eq!(before.as_deref(), Some(BALANCED));
        let tweaks = GameTweaks { free_core0: true, ..Default::default() };
        let s = start(&game, Some(child.id()), &Options::default(), &tweaks);
        let during = active_plan();
        let class = {
            // SAFETY: query-only handle, closed below.
            let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, child.id()) }.unwrap();
            let c = unsafe { GetPriorityClass(h) };
            let _ = unsafe { CloseHandle(h) };
            c
        };
        log += &format!("before={before:?} during={during:?} applied={:?} skipped={:?} ram_freed={} priority_class=0x{class:x}\n", s.applied, s.skipped, s.ram_freed);
        assert!(s.applied.contains(&"power_plan".to_string()), "{log}");
        assert!(s.applied.contains(&"priority".to_string()), "{log}");
        if crate::cpusets::summary(&crate::cpusets::topology()).physical > 1 {
            assert!(s.applied.contains(&"affinity".to_string()) && s.affinity.is_some(), "core 0 freed\n{log}");
        }
        assert_eq!(class, 0x80, "HIGH_PRIORITY_CLASS\n{log}");
        assert_eq!(session().map(|x| x.game_id), Some("exe:fakegame.exe".into()));
        assert!(stop().is_some());
        let after = active_plan();
        log += &format!("after={after:?}\n");
        assert_ne!(during, before, "the plan switched while playing\n{log}");
        assert_eq!(after, before, "previous plan restored\n{log}");
        assert!(session().is_none());
        if let Some(o) = &original {
            crate::power::set_active(o).unwrap(); // the user's own plan, as it was
        }
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&dir);
        let out = std::env::temp_dir().join("g2boost-live");
        let _ = std::fs::create_dir_all(&out);
        let _ = std::fs::write(out.join("gamemode.txt"), format!("elevated={}\n{log}OK\n", crate::system::is_elevated()));
        println!("{log}");
    }

    #[test]
    fn session_file_is_validated_and_removed_on_stop() {
        // SAFETY: same override every data-dir test in this crate uses (process-wide env var).
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
        let mut s = Session {
            game_id: "steam:730".into(),
            game_name: "CS2".into(),
            pid: None,
            install_dir: None,
            started: 1,
            prev_plan: Some("381b4222-f694-41f0-9685-ff5bb260df2e".into()),
            applied: vec![],
            ram_freed: 0,
            skipped: vec![],
            qos_policy: None,
            affinity: Some(vec![256]),
        };
        save(&s);
        assert_eq!(session().as_ref().map(|x| x.game_id.as_str()), Some("steam:730"));
        s.prev_plan = Some("not a guid; calc.exe".into());
        save(&s);
        assert!(session().is_none(), "a tampered plan id is ignored");
        save(&Session { prev_plan: None, ..s });
        assert!(stop().is_some());
        assert!(session().is_none() && stop().is_none());
    }
}
