//! Game Mode: when a game the user picked starts, G2 Boost switches to the Ultimate Performance
//! plan, raises the game's process to high priority, purges the standby list (admin only) and
//! pauses its own scheduled cleanup; when the game closes, everything goes back. The session is
//! saved to disk before anything changes, so a crash or a forced exit restores it on the next
//! start. Nothing outside G2 Boost's own power-plan and priority calls is touched.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::games::{InstalledGame, RunningGame};
use crate::registry;

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
    /// `qos_needs_admin`, `qos_failed`).
    pub skipped: Vec<String>,
    /// QoS policy created for this session (removed at the end).
    #[serde(default)]
    pub qos_policy: Option<String>,
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

/// Apply Game Mode for `game` (its process `pid`, when known). The session is written before the
/// first change so it can always be undone.
pub fn start(game: &InstalledGame, pid: Option<u32>, opts: &Options) -> Session {
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
    if opts.high_priority
        && let (Some(pid), Some(dir)) = (pid, game.install_dir.as_deref())
    {
        match crate::procs::raise_priority(pid, dir) {
            Ok(()) => s.applied.push("priority".into()),
            Err(_) => s.skipped.push("priority_denied".into()), // anti-cheat protected or elevated game
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
    if let Some(policy) = &s.qos_policy {
        let _ = crate::qos::clear(policy);
    }
    let _ = std::fs::remove_file(file());
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let s = start(&game, Some(child.id()), &Options::default());
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
