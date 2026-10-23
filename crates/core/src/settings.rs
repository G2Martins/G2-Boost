//! User settings for the routines (`%LOCALAPPDATA%\G2 Boost\settings.json`).
//! Everything is validated on load and before save: the file is hostile input too.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::cleanup;
use crate::registry;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Schedule {
    Off,
    /// Once per app start (the app starts with Windows → once per logon).
    AtLogon,
    EveryHours { hours: u32 },
    Daily { hour: u8 },
    /// `weekday`: 0 = Sunday … 6 = Saturday (SYSTEMTIME convention).
    Weekly { weekday: u8, hour: u8 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    /// Closing the window hides to the tray while any routine is active.
    pub close_to_tray: bool,
    pub schedule: Schedule,
    /// Catalog ids the routine cleans. Shaders and the Recycle Bin are opt-in on purpose.
    pub routine_categories: Vec<String>,
    /// Purge the standby list when a game starts (needs the app elevated).
    pub game_ram_clean: bool,
    /// Extra game executables (lowercase `name.exe`) besides Steam detection.
    pub game_exes: Vec<String>,
    /// Create a System Restore point before applying a preset (when elevated).
    pub restore_before_preset: bool,
    /// App names (as in the installed list) the user protected from the uninstaller.
    pub protected_apps: Vec<String>,
    /// Luli may search the web in every conversation (off = she asks per conversation).
    pub ai_web_always: bool,
    /// The user's own instructions for Luli (tone, level, preferences), sent with every question.
    pub luli_instructions: String,
    /// Games (ids from `games::installed`) with Game Mode on.
    pub game_mode_games: Vec<String>,
    /// What Game Mode does while one of those games runs.
    pub game_mode: crate::gamemode::Options,
    /// Fixed priority per executable, applied by the routine thread (never realtime).
    pub priority_rules: Vec<crate::prio::Rule>,
    /// Per-game options (CPU cores while in Game Mode, dedicated GPU), by game id. Changed only
    /// through `game_tweaks_set` in the shell, which also writes the GPU preference.
    pub game_tweaks: BTreeMap<String, crate::gamemode::GameTweaks>,
    /// Background check for new exposure (public listener, inbound connection, remote-access
    /// app, router port forward) every few minutes; alerts in the app.
    pub exposure_monitor: bool,
    /// Every normal launch asks UAC once and continues elevated (like game tools that require
    /// administrator). Not for `--mcp` / `--tray`; declining keeps the unelevated copy.
    pub always_admin: bool,
    /// winget ids the user left out of "update all" (validated on use by `updates::excluded_ids`).
    pub update_excluded: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            restore_before_preset: true,
            close_to_tray: true,
            schedule: Schedule::Off,
            routine_categories: cleanup::CATALOG
                .iter()
                .filter(|c| c.default_on && !c.admin)
                .map(|c| c.id.to_string())
                .collect(),
            game_ram_clean: false,
            game_exes: Vec::new(),
            protected_apps: Vec::new(),
            ai_web_always: false,
            luli_instructions: String::new(),
            game_mode_games: Vec::new(),
            game_mode: crate::gamemode::Options::default(),
            priority_rules: Vec::new(),
            game_tweaks: BTreeMap::new(),
            exposure_monitor: true,
            always_admin: false,
            update_excluded: Vec::new(),
        }
    }
}

const MAX_EXES: usize = 50;

/// Keep only what the catalog knows and what looks like a bare `name.exe`.
pub fn sanitize(mut s: Settings) -> Settings {
    let mut seen = std::collections::HashSet::new();
    s.routine_categories.retain(|id| cleanup::find(id).is_some() && seen.insert(id.clone()));
    let mut seen = std::collections::HashSet::new();
    s.game_exes = s
        .game_exes
        .iter()
        .map(|e| e.trim().to_ascii_lowercase())
        .filter(|e| {
            e.len() <= 64
                && e.len() > 4
                && e.ends_with(".exe")
                && !e.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|'])
                && seen.insert(e.clone())
        })
        .take(MAX_EXES)
        .collect();
    let mut seen = std::collections::HashSet::new();
    s.protected_apps = s
        .protected_apps
        .iter()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty() && n.chars().count() <= 200 && !n.chars().any(char::is_control) && seen.insert(n.to_lowercase()))
        .take(300)
        .collect();
    // Free text the user wrote: bounded, no control characters except line breaks.
    s.luli_instructions = s
        .luli_instructions
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(1500)
        .collect::<String>()
        .trim()
        .to_string();
    // Game ids are made by `games.rs` (`steam:730`, `app:m64:{GUID}`, `exe:name.exe`): bounded, no
    // control characters, deduplicated.
    let mut seen = std::collections::HashSet::new();
    s.game_mode_games.retain(|id| valid_game_id(id) && seen.insert(id.clone()));
    s.game_mode_games.truncate(200);
    s.game_tweaks = std::mem::take(&mut s.game_tweaks)
        .into_iter()
        .filter(|(id, t)| t.any() && valid_game_id(id))
        .take(200)
        .collect();
    s.priority_rules = crate::prio::sanitize(std::mem::take(&mut s.priority_rules));
    s.schedule = match s.schedule {
        Schedule::EveryHours { hours } => Schedule::EveryHours { hours: hours.clamp(1, 168) },
        Schedule::Daily { hour } => Schedule::Daily { hour: hour.min(23) },
        Schedule::Weekly { weekday, hour } => Schedule::Weekly { weekday: weekday.min(6), hour: hour.min(23) },
        other => other,
    };
    s
}

fn valid_game_id(id: &str) -> bool {
    id.len() <= 200 && id.contains(':') && !id.chars().any(char::is_control)
}

fn file() -> PathBuf {
    registry::data_dir().join("settings.json")
}

pub fn load() -> Settings {
    std::fs::read_to_string(file())
        .ok()
        .and_then(|t| serde_json::from_str::<Settings>(&t).ok())
        .map(sanitize)
        .unwrap_or_default()
}

pub fn save(s: &Settings) -> std::io::Result<Settings> {
    let s = sanitize(s.clone());
    if let Some(dir) = file().parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(file(), serde_json::to_string_pretty(&s).expect("serializable"))?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_drops_unknown_categories_bad_exes_and_clamps() {
        let s = sanitize(Settings {
            restore_before_preset: true,
            close_to_tray: false,
            schedule: Schedule::EveryHours { hours: 0 },
            routine_categories: vec!["temp_user".into(), "nope".into(), "temp_user".into(), "../x".into()],
            game_ram_clean: true,
            ai_web_always: false,
            luli_instructions: "  seja direta\u{0}\n".into(),
            game_mode_games: vec!["steam:730".into(), "steam:730".into(), "bad\u{1}:x".into(), "noprefix".into()],
            game_mode: crate::gamemode::Options::default(),
            priority_rules: vec![],
            game_tweaks: BTreeMap::from([
                ("steam:730".to_string(), crate::gamemode::GameTweaks { free_core0: true, ..Default::default() }),
                ("steam:1".to_string(), crate::gamemode::GameTweaks::default()),
                ("bad\u{1}:x".to_string(), crate::gamemode::GameTweaks { gpu_high_perf: true, ..Default::default() }),
            ]),
            exposure_monitor: true,
            always_admin: true,
            protected_apps: vec![" Discord ".into(), "discord".into(), "".into(), "a\u{0}".into()],
            game_exes: vec![
                " CS2.exe ".into(),
                "C:\\evil\\game.exe".into(),
                "notexe".into(),
                "cs2.exe".into(),
                "a.exe".into(),
                "x".repeat(70) + ".exe",
            ],
            update_excluded: vec![],
        });
        assert_eq!(s.routine_categories, vec!["temp_user".to_string()]);
        assert_eq!(s.game_exes, vec!["cs2.exe".to_string(), "a.exe".to_string()]);
        assert_eq!(s.schedule, Schedule::EveryHours { hours: 1 });
        assert_eq!(s.protected_apps, vec!["Discord".to_string()]);
        assert_eq!(
            sanitize(Settings { schedule: Schedule::Weekly { weekday: 9, hour: 99 }, ..Default::default() }).schedule,
            Schedule::Weekly { weekday: 6, hour: 23 }
        );
        let d = Settings::default();
        assert!(d.routine_categories.contains(&"temp_user".to_string()));
        assert!(!d.routine_categories.contains(&"shaders".to_string()));
        assert!(!d.routine_categories.contains(&"recycle".to_string()));
        assert!(!d.routine_categories.contains(&"temp_windows".to_string()), "admin categories are opt-in");
        assert_eq!(s.luli_instructions, "seja direta", "trimmed, control characters removed");
        assert_eq!(s.game_mode_games, vec!["steam:730".to_string()], "deduplicated, control chars and bad ids dropped");
        assert_eq!(s.game_tweaks.keys().collect::<Vec<_>>(), vec!["steam:730"], "empty entries and bad ids dropped");
    }

    #[test]
    fn json_roundtrip_keeps_tagged_schedule() {
        let s = Settings { schedule: Schedule::Daily { hour: 7 }, ..Default::default() };
        let text = serde_json::to_string(&s).unwrap();
        assert!(text.contains("\"mode\":\"daily\""));
        assert_eq!(serde_json::from_str::<Settings>(&text).unwrap(), s);
        // Unknown/missing fields fall back to defaults instead of failing.
        let partial: Settings = serde_json::from_str(r#"{"game_ram_clean":true}"#).unwrap();
        assert!(partial.game_ram_clean && partial.close_to_tray);
        assert!(partial.exposure_monitor, "exposure monitor defaults on");
    }
}
