//! Routines: when is the scheduled cleanup due, and did a game just start?
//! Pure decisions here; the Tauri shell owns the timer thread and the side effects.

use serde::{Deserialize, Serialize};
use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::registry::{self, Root, Value};
use crate::settings::Schedule;
use crate::system;

/* ---------------- schedule ---------------- */

/// Local wall-clock snapshot (`GetLocalTime`), plus the unix timestamp of the same instant.
#[derive(Debug, Clone, Copy)]
pub struct Clock {
    pub unix: u64,
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl Clock {
    pub fn now() -> Self {
        // SAFETY: GetLocalTime only writes the SYSTEMTIME out-param.
        let st = unsafe { GetLocalTime() };
        Self {
            unix: registry::now_unix(),
            weekday: st.wDayOfWeek as u8,
            hour: st.wHour as u8,
            minute: st.wMinute as u8,
            second: st.wSecond as u8,
        }
    }

    /// Unix time of `hour:00:00`, `days_back` days before today (0 = today).
    fn slot(&self, hour: u8, days_back: u64) -> u64 {
        let since_midnight = u64::from(self.hour) * 3600 + u64::from(self.minute) * 60 + u64::from(self.second);
        (self.unix.saturating_sub(since_midnight) + u64::from(hour) * 3600).saturating_sub(days_back * 86_400)
    }
}

/// Is a routine run due now? `last_run` = unix time of the previous routine run,
/// `ran_since_start` = whether this process already ran its at-logon cleanup. A daily or weekly
/// slot missed while the PC was off runs at the next chance (any run before the latest slot =
/// due); never ran = wait for the slot itself.
pub fn due(schedule: &Schedule, clock: Clock, last_run: Option<u64>, ran_since_start: bool) -> bool {
    match *schedule {
        Schedule::Off => false,
        Schedule::AtLogon => !ran_since_start,
        Schedule::EveryHours { hours } => {
            last_run.is_none_or(|l| clock.unix.saturating_sub(l) >= u64::from(hours.max(1)) * 3600)
        }
        Schedule::Daily { hour } => {
            let passed = clock.hour >= hour;
            last_run.map_or(passed, |l| l < clock.slot(hour, if passed { 0 } else { 1 }))
        }
        Schedule::Weekly { weekday, hour } => {
            let mut back = (7 + u64::from(clock.weekday % 7) - u64::from(weekday % 7)) % 7;
            if back == 0 && clock.hour < hour {
                back = 7;
            }
            last_run.map_or(back == 0, |l| l < clock.slot(hour, back))
        }
    }
}

/* ---------------- routine state (last run) ---------------- */

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RoutineState {
    pub last_run: Option<u64>,
}

fn state_file() -> std::path::PathBuf {
    registry::data_dir().join("routine.json")
}

pub fn load_state() -> RoutineState {
    std::fs::read_to_string(state_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_state(s: &RoutineState) {
    if let Some(dir) = state_file().parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(state_file(), serde_json::to_string(s).unwrap_or_default());
}

/* ---------------- game detection ---------------- */

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Game {
    pub name: String,
    /// `steam` (RunningAppID), `exe` (user list), `steamapps` (process under steamapps\common).
    pub source: &'static str,
}

const STEAM: &str = r"Software\Valve\Steam";

/// Steam writes `HKCU\Software\Valve\Steam\RunningAppID` while a game runs (0 otherwise).
fn steam_running() -> Option<Game> {
    match registry::read(Root::Hkcu, STEAM, "RunningAppID") {
        Ok(Value::Dword(id)) if id != 0 => {
            let name = match registry::read(Root::Hkcu, &format!(r"{STEAM}\Apps\{id}"), "Name") {
                Ok(Value::Str(n)) if !n.is_empty() => n,
                _ => format!("Steam app {id}"),
            };
            Some(Game { name, source: "steam" })
        }
        _ => None,
    }
}

/// Match running process paths against the user's exe list and Steam's install layout.
pub fn match_processes(paths: &[String], exes: &[String]) -> Option<Game> {
    for p in paths {
        let lower = p.to_ascii_lowercase();
        let file = lower.rsplit('\\').next().unwrap_or("").to_string();
        if exes.contains(&file) {
            return Some(Game { name: file, source: "exe" });
        }
        if lower.contains(r"\steamapps\common\") && !file.is_empty() {
            return Some(Game { name: file, source: "steamapps" });
        }
    }
    None
}

pub fn detect_game(exes: &[String]) -> Option<Game> {
    steam_running().or_else(|| match_processes(&system::running_process_paths(), exes))
}

/// Edge detector: fires once when the machine goes from "no game" to "game". The first tick
/// only records the baseline, so a game already running when the app starts never fires.
#[derive(Debug, Default)]
pub struct GameWatcher {
    primed: bool,
    current: Option<Game>,
}

impl GameWatcher {
    pub fn current(&self) -> Option<&Game> {
        self.current.as_ref()
    }

    /// Feed the latest detection; returns the game that just started, if any.
    pub fn observe(&mut self, now: Option<Game>) -> Option<Game> {
        let started = self.primed && self.current.is_none() && now.is_some();
        self.primed = true;
        self.current = now;
        if started { self.current.clone() } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(weekday: u8, hour: u8, minute: u8) -> Clock {
        // 2026-10-14 is a Wednesday (3); midnight local = some fixed unix base.
        let base = 1_800_000_000u64 - 1_800_000_000 % 86_400;
        Clock { unix: base + u64::from(hour) * 3600 + u64::from(minute) * 60, weekday, hour, minute, second: 0 }
    }

    #[test]
    fn schedule_due_rules() {
        assert!(!due(&Schedule::Off, clock(3, 12, 0), None, false));
        assert!(due(&Schedule::AtLogon, clock(3, 12, 0), None, false));
        assert!(!due(&Schedule::AtLogon, clock(3, 12, 0), None, true));

        let c = clock(3, 12, 0);
        assert!(due(&Schedule::EveryHours { hours: 2 }, c, None, false));
        assert!(!due(&Schedule::EveryHours { hours: 2 }, c, Some(c.unix - 3600), false));
        assert!(due(&Schedule::EveryHours { hours: 2 }, c, Some(c.unix - 7200), false));

        let daily = Schedule::Daily { hour: 9 };
        assert!(!due(&daily, clock(3, 8, 59), None, false), "before the slot");
        assert!(due(&daily, clock(3, 9, 0), None, false));
        assert!(!due(&daily, clock(3, 9, 30), Some(clock(3, 9, 5).unix), false), "already ran today");
        assert!(due(&daily, clock(3, 9, 30), Some(clock(3, 9, 5).unix - 86_400), false), "ran yesterday");

        let weekly = Schedule::Weekly { weekday: 3, hour: 20 };
        assert!(!due(&weekly, clock(2, 21, 0), None, false), "wrong day");
        assert!(due(&weekly, clock(3, 20, 0), None, false));
        assert!(!due(&weekly, clock(3, 21, 0), Some(clock(3, 20, 1).unix), false));
    }

    #[test]
    fn a_slot_missed_while_the_pc_was_off_runs_at_the_next_chance() {
        const DAY: u64 = 86_400;
        // Daily at 9: the PC was off yesterday at 9; today at 8 the run from two days ago is stale.
        let daily = Schedule::Daily { hour: 9 };
        let yesterday_9 = clock(3, 9, 5).unix - DAY;
        assert!(due(&daily, clock(3, 8, 0), Some(yesterday_9 - DAY), false), "yesterday's slot missed");
        assert!(!due(&daily, clock(3, 8, 0), Some(yesterday_9), false), "ran yesterday: wait for 9");
        // Weekly on Wednesday (3) at 20: Thursday morning, the PC was off on Wednesday night.
        let weekly = Schedule::Weekly { weekday: 3, hour: 20 };
        let last_wed = clock(3, 20, 5).unix - DAY; // the clocks share one date: Thursday's "yesterday"
        assert!(due(&weekly, clock(4, 10, 0), Some(last_wed - 7 * DAY), false), "Wednesday's slot missed");
        assert!(!due(&weekly, clock(4, 10, 0), Some(last_wed), false), "ran on Wednesday");
        assert!(!due(&weekly, clock(4, 10, 0), None, false), "never ran: waits for the day");
        // Wednesday before 20: the slot to compare is last week's.
        let week_ago = clock(3, 20, 5).unix - 7 * DAY;
        assert!(!due(&weekly, clock(3, 10, 0), Some(week_ago), false));
        assert!(due(&weekly, clock(3, 10, 0), Some(week_ago - 7 * DAY), false), "last week's slot missed");
        // Sunday (0) slot seen from Saturday (6): six days back.
        let sunday = Schedule::Weekly { weekday: 0, hour: 9 };
        assert!(due(&sunday, clock(6, 12, 0), Some(clock(6, 9, 0).unix - 7 * DAY), false));
        assert!(!due(&sunday, clock(6, 12, 0), Some(clock(6, 9, 5).unix - 6 * DAY), false));
    }

    #[test]
    fn game_matching_and_edge_detection() {
        let paths = vec![
            r"C:\Windows\explorer.exe".to_string(),
            r"D:\SteamLibrary\steamapps\common\Counter-Strike Global Offensive\game\bin\win64\cs2.exe".to_string(),
        ];
        assert_eq!(match_processes(&paths, &[]).unwrap(), Game { name: "cs2.exe".into(), source: "steamapps" });
        assert_eq!(match_processes(&paths, &["explorer.exe".into()]).unwrap().source, "exe");
        assert!(match_processes(&[r"C:\Program Files\app\app.exe".to_string()], &[]).is_none());

        let mut w = GameWatcher::default();
        let g = Game { name: "cs2.exe".into(), source: "exe" };
        assert!(w.observe(Some(g.clone())).is_none(), "already running at startup: baseline only");
        assert!(w.observe(None).is_none());
        assert_eq!(w.observe(Some(g.clone())), Some(g.clone()), "fresh start fires");
        assert!(w.observe(Some(g)).is_none(), "still running: no repeat");
        let _ = Clock::now();
        let _ = detect_game(&[]);
    }
}
