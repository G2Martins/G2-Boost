//! Limpeza (round 11): the smart analysis, one preview/confirm/run for catalog categories and
//! Windows' cleanup handlers together (the smart view and the detailed mode both use it), and the
//! disk map's "cleanable by Cleanup" badges. Thin: the rules live in `g2boost_core::smartclean`.
//!
//! The webview only ever sends ids (catalog / handler ids, scan-tree indices) or paths it read
//! from the scan to label them; nothing here deletes a path it was given.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use g2boost_core::cleanup::{self, ConfirmToken, PreviewItem, Progress, RunRecord, TokenStore};
use g2boost_core::smartclean::{self, Plan};
use g2boost_core::system::DriveInfo;
use g2boost_core::{activity, apps, disk, repair, volcache};
use tauri::{AppHandle, Emitter, State};

use crate::{AppsState, CLEANUP_CANCEL, DiskState, DupState, Routines, notify};

/// A manual run this long ends with a line in the bell (the user may have left the page).
const LONG_RUN_MS: u64 = 15_000;

// ponytail: app-wide singletons, no `.manage()`; one analysis at a time is all the page needs.
static ANALYZING: AtomicBool = AtomicBool::new(false);
/// Unowned folders from the last analysis, lowercased: the disk map's "no owner" badge.
static UNOWNED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Measure every source (catalog, Windows handlers, big old files, unowned folders, leftovers,
/// duplicates) with live `smart:source` events; the three buckets come back.
#[tauri::command]
pub(crate) async fn smart_analyze(app: AppHandle, apps_st: State<'_, AppsState>, r: State<'_, Routines>, dup: State<'_, DupState>) -> Result<Plan, String> {
    if ANALYZING.swap(true, Ordering::SeqCst) {
        return Err("busy".into());
    }
    let mut installed = apps_st.apps.lock().unwrap().clone();
    let protected = r.settings.lock().unwrap().protected_apps.clone();
    let dupes = dup
        .result
        .lock()
        .unwrap()
        .as_ref()
        .map(|g| (g.len() as u64, g.iter().map(|x| x.wasted).sum::<u64>()));
    CLEANUP_CANCEL.store(false, Ordering::SeqCst);
    let res = tauri::async_runtime::spawn_blocking(move || {
        if installed.is_empty() {
            installed = apps::list(&protected);
        }
        let inp = smartclean::Inputs { installed: Some(installed), dupes };
        smartclean::analyze(&inp, &CLEANUP_CANCEL, |e| {
            let _ = app.emit("smart:source", e);
        })
    })
    .await;
    ANALYZING.store(false, Ordering::SeqCst);
    let plan = res.map_err(|e| e.to_string())?;
    *UNOWNED.lock().unwrap() = plan.unowned_paths.clone();
    Ok(plan)
}

/// Stop the analysis, a preview or a run between files (what was freed stays freed).
#[tauri::command]
pub(crate) fn smart_cancel() {
    CLEANUP_CANCEL.store(true, Ordering::SeqCst);
    repair::cancel();
}

#[derive(serde::Serialize)]
pub(crate) struct SmartPreview {
    catalog: Vec<PreviewItem>,
    system: Vec<volcache::VcItem>,
}

/// Measure catalog categories and handlers in one queue (`smart:measure`).
#[tauri::command]
pub(crate) async fn smart_preview(app: AppHandle, ids: Vec<String>) -> Result<SmartPreview, String> {
    let (cat, sys) = smartclean::split_ids(&ids).ok_or("bad_selection")?;
    CLEANUP_CANCEL.store(false, Ordering::SeqCst);
    let total = ids.len();
    tauri::async_runtime::spawn_blocking(move || {
        let emit = |p: &Progress| {
            let _ = app.emit("smart:measure", p);
        };
        let catalog = if cat.is_empty() {
            Vec::new()
        } else {
            cleanup::preview_live(&cat, &CLEANUP_CANCEL, |p| emit(&smartclean::joined(&Progress::default(), p, total)))
        };
        let base = leg_end(cat.len(), catalog.iter().map(|i| (i.bytes, i.files)));
        let system = if sys.is_empty() || CLEANUP_CANCEL.load(Ordering::SeqCst) {
            Vec::new()
        } else {
            match volcache::preview(&sys, |p| emit(&smartclean::joined(&base, p, total))) {
                Ok(v) => v,
                Err(e) => sys.iter().filter_map(|i| volcache::find(i)).map(|h| busy_item(h.id, &e)).collect(),
            }
        };
        SmartPreview { catalog, system }
    })
    .await
    .map_err(|e| e.to_string())
}

fn busy_item(id: &'static str, e: &volcache::VcError) -> volcache::VcItem {
    volcache::VcItem { id, bytes: 0, available: false, needs_admin: false, error: Some(e.to_string()) }
}

/// Where the first leg ended: `n` items done with these (bytes, files).
fn leg_end(n: usize, items: impl Iterator<Item = (u64, u64)>) -> Progress {
    let (bytes, files) = items.fold((0, 0), |a, (b, f)| (a.0 + b, a.1 + f));
    Progress { done: n, total: n, current: None, bytes, files }
}

/// Issued only by the UI's confirmation; single use, 2 minutes, bound to the exact ids.
#[tauri::command]
pub(crate) fn smart_confirm(tokens: State<'_, TokenStore>, ids: Vec<String>) -> Result<ConfirmToken, String> {
    smartclean::split_ids(&ids).ok_or("bad_selection")?;
    Ok(tokens.issue(ids))
}

#[derive(serde::Serialize)]
pub(crate) struct SmartRun {
    records: Vec<RunRecord>,
    /// The system drive before and after (the "you gained" line).
    before: Option<DriveInfo>,
    after: Option<DriveInfo>,
    /// Still short on space after the run: the UI points at what else takes it.
    tight: bool,
}

/// Run a confirmed selection: catalog categories, then the handlers, one `smart:progress` queue.
/// Both legs land in the cleanup history as usual; a long run also lands in the bell.
#[tauri::command]
pub(crate) async fn smart_execute(app: AppHandle, tokens: State<'_, TokenStore>, token: u64, ids: Vec<String>) -> Result<SmartRun, String> {
    let (cat, sys) = smartclean::split_ids(&ids).ok_or("bad_selection")?;
    if !tokens.consume(token, &ids) {
        return Err("bad_token".into());
    }
    CLEANUP_CANCEL.store(false, Ordering::SeqCst);
    let total = ids.len();
    tauri::async_runtime::spawn_blocking(move || {
        let t0 = Instant::now();
        let before = smartclean::system_drive();
        let emit = |p: &Progress| {
            let _ = app.emit("smart:progress", p);
        };
        let mut records = Vec::new();
        let mut base = Progress::default();
        if !cat.is_empty() {
            let rec = cleanup::execute(&cat, "manual", &CLEANUP_CANCEL, |p| emit(&smartclean::joined(&Progress::default(), p, total)));
            base = leg_end(cat.len(), rec.items.iter().map(|i| (i.bytes_freed, i.files_deleted)));
            records.push(rec);
        }
        if !sys.is_empty() {
            if CLEANUP_CANCEL.load(Ordering::SeqCst) {
                let items = sys.iter().map(|id| cleanup::ExecItem { id: id.clone(), error: Some("cancelled".into()), ..Default::default() }).collect();
                records.push(RunRecord { at: g2boost_core::registry::now_unix(), trigger: "manual".into(), duration_ms: 0, items });
            } else {
                match volcache::execute(&sys, "manual", |p| emit(&smartclean::joined(&base, p, total))) {
                    Ok(rec) => records.push(rec),
                    Err(e) => {
                        let items = sys.iter().map(|id| cleanup::ExecItem { id: id.clone(), error: Some(e.to_string()), ..Default::default() }).collect();
                        records.push(RunRecord { at: g2boost_core::registry::now_unix(), trigger: "manual".into(), duration_ms: 0, items });
                    }
                }
            }
        }
        let freed: u64 = records.iter().flat_map(|r| &r.items).map(|i| i.bytes_freed).sum();
        emit(&Progress { done: total, total, current: None, bytes: freed, files: base.files });
        if t0.elapsed().as_millis() as u64 >= LONG_RUN_MS {
            notify(&app, "cleanup", "activity.clean.done", &[("bytes", &freed.to_string())], activity::Tone::Ok, Some("cleanup"));
        }
        let after = smartclean::system_drive();
        SmartRun { records, before, tight: after.as_ref().is_some_and(smartclean::tight), after }
    })
    .await
    .map_err(|e| e.to_string())
}

/* ---------------- disk map ↔ cleanup ---------------- */

#[derive(serde::Serialize)]
pub(crate) struct OwnerHit {
    /// Scan-tree index path (the folder on screen or one of its children).
    ids: Vec<u32>,
    #[serde(flatten)]
    owners: smartclean::Owners,
    /// Found by the last analysis as a folder no installed app owns.
    unowned: bool,
}

/// Badges for the folder on screen and its direct sub-folders: which cleanup categories own or
/// live inside them, and which ones nobody owns. Only folders with something to say come back.
#[tauri::command]
pub(crate) async fn clean_owners(disk_st: State<'_, DiskState>, ids: Vec<u32>) -> Result<Vec<OwnerHit>, String> {
    let Some(tree) = disk_st.tree.lock().unwrap().clone() else {
        return Ok(Vec::new());
    };
    let unowned = UNOWNED.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (Some(node), Some(base)) = (disk::find(&tree, &ids), disk::path_of(&tree, &ids)) else {
            return Vec::new();
        };
        let roots = smartclean::catalog_roots();
        let hit = |ids: Vec<u32>, p: &std::path::Path| {
            let owners = smartclean::owners(p, &roots);
            let lower = p.to_string_lossy().trim_end_matches('\\').to_ascii_lowercase();
            let unowned = unowned.contains(&lower);
            (!owners.inside.is_empty() || !owners.contains.is_empty() || unowned).then_some(OwnerHit { ids, owners, unowned })
        };
        let mut out: Vec<OwnerHit> = hit(ids.clone(), &base).into_iter().collect();
        for (i, c) in node.children.iter().enumerate().take(400) {
            if c.is_dir {
                let mut cid = ids.clone();
                cid.push(i as u32);
                out.extend(hit(cid, &base.join(&c.name)));
            }
        }
        out
    })
    .await
    .map_err(|e| e.to_string())
}

/// The category whose folder holds each path (largest-files list), `None` when none does.
#[tauri::command]
pub(crate) async fn clean_owner_paths(paths: Vec<String>) -> Result<Vec<Option<&'static str>>, String> {
    if paths.len() > 500 {
        return Err("bad_selection".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let roots = smartclean::catalog_roots();
        paths
            .iter()
            .map(|p| if p.len() > 1024 { None } else { smartclean::owners(std::path::Path::new(p), &roots).inside.first().copied() })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

/// Category id → the installed exe of its app (Chrome, Discord, Steam...), for its real icon.
#[tauri::command]
pub(crate) async fn clean_app_exes() -> Result<std::collections::HashMap<&'static str, String>, String> {
    tauri::async_runtime::spawn_blocking(|| smartclean::app_exes().into_iter().collect()).await.map_err(|e| e.to_string())
}
