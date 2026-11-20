//! Memória RAM (round 11): the live split of the memory and who holds it, for the RAM tab and the
//! startup tab. Thin and read-only: PDH and the process walk live in `g2boost_core::memory`;
//! ending a process and turning a startup item off go through the existing commands
//! (`procs_end`, `startup_set`), which check again in the core.

use g2boost_core::memory::{self, AppMem, Composition};

/// In use / modified / standby / free (+ compressed) right now, and whether a purge would help.
#[tauri::command]
pub async fn ram_composition() -> Option<Composition> {
    tauri::async_runtime::spawn_blocking(memory::composition).await.ok().flatten()
}

/// The apps holding the most private memory, grouped by executable (the UI polls while visible).
#[tauri::command]
pub async fn ram_apps() -> Vec<AppMem> {
    tauri::async_runtime::spawn_blocking(|| memory::top_apps(24)).await.unwrap_or_default()
}
