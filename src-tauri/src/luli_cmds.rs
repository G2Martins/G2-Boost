//! Conexões (round 11): Luli skills and the read-only map of the MCP servers on this PC. Thin:
//! validation lives in `g2boost_core::{skills, mcpmap}`.
//!
//! A SKILL.md or skill .zip dropped on the window: the webview never names a path. This module
//! records the dropped file from the native drag-and-drop event, tells the page, and the page
//! takes its bytes once (`skills_take_drop`); the zip is opened in the webview, only SKILL.md is
//! sent back to the core, nothing in the package ever runs.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use g2boost_core::{mcpmap, skills};
use tauri::{AppHandle, DragDropEvent, Emitter, Manager, WebviewEvent};

/// Same ceiling the webview applies to a picked file.
const MAX_DROP: u64 = 25 << 20;

// ponytail: one pending drop for the one window; a queue if several files at once ever matter.
static DROPPED: Mutex<Option<PathBuf>> = Mutex::new(None);

fn skill_file(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("zip"))
}

pub fn setup(app: &AppHandle) {
    let Some(w) = app.get_webview_window("main") else { return };
    let handle = app.clone();
    w.on_webview_event(move |e| {
        if let WebviewEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = e
            && let Some(p) = paths.first().filter(|p| skill_file(p))
        {
            *DROPPED.lock().unwrap() = Some(p.clone());
            let _ = handle.emit("skills:dropped", ());
        }
    });
}

/// The .md/.zip just dropped, once: a local regular file (no symlink, no network path), ≤ 25 MB.
#[tauri::command]
pub fn skills_take_drop() -> Result<tauri::ipc::Response, String> {
    let p = DROPPED.lock().unwrap().take().ok_or("no_drop")?;
    let meta = std::fs::symlink_metadata(&p).map_err(|_| "unreadable")?;
    if meta.file_type().is_symlink() || !meta.is_file() || p.to_string_lossy().starts_with(r"\\") {
        return Err("unreadable".into());
    }
    if meta.len() > MAX_DROP {
        return Err("too_large".into());
    }
    std::fs::read(&p).map(tauri::ipc::Response::new).map_err(|_| "unreadable".into())
}

#[tauri::command]
pub fn skills_list() -> Vec<skills::Skill> {
    skills::list()
}

/// One SKILL.md's text (pasted, picked, or taken out of a zip by the webview); added turned off.
#[tauri::command]
pub fn skills_add(text: String) -> Result<Vec<skills::Skill>, String> {
    skills::add(&text).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn skills_set(name: String, enabled: bool) -> Result<Vec<skills::Skill>, String> {
    skills::set_enabled(&name, enabled).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn skills_remove(name: String) -> Result<Vec<skills::Skill>, String> {
    skills::remove(&name).map_err(|e| e.to_string())
}

/// The MCP servers other AI apps start on this PC (read-only; see `mcpmap`).
#[tauri::command]
pub async fn mcp_map() -> Result<Vec<mcpmap::ClientMap>, String> {
    tauri::async_runtime::spawn_blocking(mcpmap::scan).await.map_err(|e| e.to_string())
}
