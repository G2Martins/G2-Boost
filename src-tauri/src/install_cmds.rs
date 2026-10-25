//! IPC for the Apps page's **Instalar** tab (`crates/core/src/{catalog,install}.rs`). Thin on purpose:
//! the catalog is the allowlist, the confirmation token binds the exact ordered id list, and every
//! check lives in the core.

use g2boost_core::cleanup::ConfirmToken;
use g2boost_core::{catalog, install, updates};
use tauri::{AppHandle, Emitter, State};

use crate::{AppsState, Routines, blocking};

/// The whole catalog (about 170 small rows): the allowlist the tab draws and the batch resolves against.
#[tauri::command]
pub fn install_catalog() -> Vec<&'static catalog::Entry> {
    catalog::all().iter().collect()
}

/// The default set for a freshly formatted PC (the post-format wizard reads this too).
#[tauri::command]
pub fn install_essentials() -> Vec<&'static str> {
    catalog::essentials().iter().map(|e| e.id).collect()
}

/// Which catalog apps are installed (`winget list` plus the registry). `refresh` skips the
/// ten-minute cache. The installed list it reads is kept in the shell, so the tile's icon
/// (`apps_icon`) and the native uninstaller (`apps_uninstall`) find the same app by id.
#[tauri::command]
pub async fn install_status(state: State<'_, AppsState>, r: State<'_, Routines>, refresh: bool) -> Result<install::Status, String> {
    let protected = r.settings.lock().unwrap().protected_apps.clone();
    let (status, apps) = tauri::async_runtime::spawn_blocking(move || {
        let (status, apps) = install::scan(refresh, &protected);
        g2boost_core::apps::remember(&apps);
        (status, apps)
    })
    .await
    .map_err(|e| e.to_string())?;
    *state.apps.lock().unwrap() = apps;
    Ok(status)
}

/// Issued after the UI's confirmation: bound to the exact, ordered list of catalog ids.
#[tauri::command]
pub fn install_confirm(ids: Vec<String>) -> Result<ConfirmToken, String> {
    install::confirm(&ids).map_err(|e| e.to_string())
}

/// Installs the confirmed list one package at a time, emitting `install:progress` around each.
#[tauri::command]
pub async fn install_run(app: AppHandle, token: u64, ids: Vec<String>) -> Result<updates::Batch, String> {
    blocking(move || {
        install::run(token, &ids, |p| {
            let _ = app.emit("install:progress", p);
        })
    })
    .await
}

/// Stop after the package being installed now.
#[tauri::command]
pub fn install_cancel() {
    install::cancel();
}

/// This tab's own runs, oldest first.
#[tauri::command]
pub fn install_history() -> Vec<updates::Batch> {
    install::history()
}
