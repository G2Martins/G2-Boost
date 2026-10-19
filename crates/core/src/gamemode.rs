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
}

impl Default for Options {
    fn default() -> Self {
        Self { power_plan: true, high_priority: true, purge_ram: true, pause_routines: true }
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
    /// Why an option did not apply (`needs_admin`, `plan_failed`, `priority_denied`).
    pub skipped: Vec<String>,
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
    save(&s);
    s
}

/// Undo the session (if any): previous power plan back, normal priority if the game still runs.
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
