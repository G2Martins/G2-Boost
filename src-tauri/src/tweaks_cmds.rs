//! Round 9 tweaks block: "Ver o que muda" for every tweak and the preinstalled-apps removal of
//! the Debloat page. Thin IPC over `g2boost_core`; every check lives in the core.

use g2boost_core::cleanup::ConfirmToken;
use g2boost_core::{boost, debloat};
use tauri::State;

/// What turning `id` on writes, read from the tweak's own definition.
#[tauri::command]
pub fn tweak_changes(id: String) -> Result<Vec<boost::Change>, String> {
    boost::changes(&id).map_err(|e| e.to_string())
}

/// Allowlisted preinstalled Store apps installed for this user (apps the user protected stay out).
#[tauri::command]
pub async fn debloat_apps(r: State<'_, crate::Routines>) -> Result<Vec<debloat::Preinstalled>, String> {
    let protected = r.settings.lock().unwrap().protected_apps.clone();
    tauri::async_runtime::spawn_blocking(move || debloat::installed(&protected)).await.map_err(|e| e.to_string())
}

/// Issued after the UI's confirmation, bound to the exact ids of the last listing.
#[tauri::command]
pub fn debloat_apps_confirm(ids: Vec<String>) -> Result<ConfirmToken, String> {
    debloat::confirm(&ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn debloat_apps_remove(token: u64, ids: Vec<String>) -> Result<Vec<debloat::Removed>, String> {
    crate::blocking(move || debloat::remove(token, &ids)).await
}
