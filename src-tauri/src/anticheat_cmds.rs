//! Anti-cheat readiness (round 11): the Jogos / Meu PC panel and the re-check after a boot. Thin:
//! detection, the requirement matrix and the snapshot live in `g2boost_core::anticheat`.

use std::time::Duration;

use g2boost_core::activity::Tone;
use g2boost_core::anticheat::{self, Change};
use g2boost_core::{firmware, hardware};
use tauri::{AppHandle, State};

use crate::Dashboard;

/// Requirements of the installed games' anti-cheats and what this PC meets (read-only, no admin).
#[tauri::command]
pub(crate) async fn anticheat_readiness(state: State<'_, Dashboard>) -> Result<anticheat::Readiness, String> {
    let cached = state.profile.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let p = cached.unwrap_or_else(hardware::profile);
        anticheat::readiness(&p, &firmware::read(&p))
    })
    .await
    .map_err(|e| e.to_string())
}

/// Most lines one start raises (a BIOS reset can flip several checks at once).
const MAX_LINES: usize = 6;

/// On every start (the logon task starts the app right after a boot): compare with the last
/// start and tell what changed, e.g. "Secure Boot is on now" or "TPM gone after a BIOS update".
pub(crate) fn setup(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        // ponytail: fixed delay so the window and routines get the first seconds; no retry.
        std::thread::sleep(Duration::from_secs(20));
        for c in anticheat::recheck(&hardware::profile()).into_iter().take(MAX_LINES) {
            match c {
                Change::Check { id, on } => {
                    let id = serde_json::to_value(id).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                    let key = format!("anticheat.notify.{id}.{}", if on { "on" } else { "off" });
                    crate::notify(&app, "anticheat", &key, &[], if on { Tone::Ok } else { Tone::Warn }, Some("games"));
                }
                Change::Game { name, ready } => {
                    let (key, tone) = if ready { ("anticheat.notify.ready", Tone::Ok) } else { ("anticheat.notify.blocked", Tone::Warn) };
                    crate::notify(&app, "anticheat", key, &[("game", &name)], tone, Some("games"));
                }
            }
        }
    });
}
