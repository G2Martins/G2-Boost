//! IPC for the Profiles and Mouse/keyboard pages. Thin on purpose: the core validates every input
//! (names, tweak ids, JSON, file names, setting values) and does the work.

use g2boost_core::peripherals::{self, Setting};
use g2boost_core::profiles::{self, FileEntry, HistoryEntry, Plan, Profile, ProfileError, Restoration, Step, StepResult, Target};
use g2boost_core::settings::{self, Settings};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::{Routines, blocking};

/// Change the profile list in settings.json and in the live copy together, so a page holding an
/// older copy of the settings is the only way to lose them (see `settings_set`).
fn store(r: &Routines, change: impl FnOnce(&[Profile]) -> Result<Vec<Profile>, ProfileError>) -> Result<Vec<Profile>, String> {
    let mut live = r.settings.lock().unwrap();
    let next = change(&live.profiles).map_err(|e| e.to_string())?;
    let saved = settings::save(&Settings { profiles: next, ..live.clone() }).map_err(|e| e.to_string())?;
    let out = saved.profiles.clone();
    *live = saved;
    Ok(out)
}

#[tauri::command]
pub fn profiles_list(r: State<'_, Routines>) -> Vec<Profile> {
    r.settings.lock().unwrap().profiles.clone()
}

/// Add a profile, or replace the one named `previous` (edit / rename).
#[tauri::command]
pub fn profiles_save(r: State<'_, Routines>, profile: Profile, previous: Option<String>) -> Result<Vec<Profile>, String> {
    store(&r, |list| profiles::upsert(list, profile, previous.as_deref()))
}

#[tauri::command]
pub fn profiles_delete(r: State<'_, Routines>, name: String) -> Result<Vec<Profile>, String> {
    store(&r, |list| profiles::remove(list, &name))
}

/// What applying `target` would change on this PC right now (reads only).
#[tauri::command]
pub async fn profiles_plan(target: Target) -> Result<Plan, String> {
    blocking(move || Ok::<_, String>(profiles::plan_now(&target))).await
}

/// One step of a plan, through the normal tweak flow (backup first). The UI runs the steps one by
/// one, so it can show progress and stop.
#[tauri::command]
pub async fn profiles_step(step: Step, source: String) -> Result<StepResult, String> {
    blocking(move || Ok::<_, String>(profiles::run_step(&step, &source))).await
}

#[tauri::command]
pub async fn profiles_restoration() -> Result<Restoration, String> {
    blocking(|| Ok::<_, String>(profiles::restoration())).await
}

#[tauri::command]
pub fn profiles_history() -> Vec<HistoryEntry> {
    profiles::history()
}

#[derive(Serialize)]
pub struct Exported {
    json: String,
    /// The file in `Documents\G2 Boost\Perfis`; `None` when it could not be written (the JSON is still returned).
    path: Option<String>,
}

/// Write the profile as a JSON file and show it in Explorer; the text comes back for the clipboard.
#[tauri::command]
pub async fn profiles_export(profile: Profile) -> Result<Exported, String> {
    let (json, path) = blocking(move || {
        let p = profiles::clean_profile(profile).ok_or(ProfileError::InvalidName)?;
        let json = profiles::export_json(&p);
        let file = profiles::profiles_dir().ok_or(ProfileError::NoFolder).and_then(|dir| profiles::export_file(&dir, &p));
        Ok::<_, ProfileError>((json, file.ok()))
    })
    .await?;
    if let Some(p) = &path {
        let _ = tauri_plugin_opener::reveal_item_in_dir(p);
    }
    Ok(Exported { json, path: path.map(|p| p.to_string_lossy().into_owned()) })
}

#[tauri::command]
pub async fn profiles_files() -> Result<Vec<FileEntry>, String> {
    blocking(|| Ok::<_, String>(profiles::profiles_dir().map(|d| profiles::list_files(&d)).unwrap_or_default())).await
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImportSource {
    /// A file name from `profiles_files`.
    File { file: String },
    /// Pasted JSON.
    Text { text: String },
}

/// Read, validate and add a profile (a name already in use gets "(2)").
#[tauri::command]
pub fn profiles_import(r: State<'_, Routines>, source: ImportSource) -> Result<Vec<Profile>, String> {
    let profile = match source {
        ImportSource::Text { text } => profiles::parse_import(&text),
        ImportSource::File { file } => {
            profiles::profiles_dir().ok_or(ProfileError::NoFolder).and_then(|dir| profiles::read_file(&dir, &file))
        }
    }
    .map_err(|e| e.to_string())?;
    store(&r, |list| {
        let name = profiles::unique_name(list, &profile.name);
        profiles::upsert(list, Profile { name, ..profile }, None)
    })
}

/* ---------------- mouse and keyboard ---------------- */

#[tauri::command]
pub async fn periph_state() -> Result<peripherals::State, String> {
    blocking(peripherals::state).await
}

/// The core clamps `value` to the Control Panel range.
#[tauri::command]
pub async fn periph_set(setting: Setting, value: i64) -> Result<peripherals::State, String> {
    blocking(move || peripherals::set(setting, value)).await
}

/// Put the saved original back: one setting, or all (`None`).
#[tauri::command]
pub async fn periph_revert(setting: Option<Setting>) -> Result<peripherals::State, String> {
    blocking(move || peripherals::revert(setting)).await
}
