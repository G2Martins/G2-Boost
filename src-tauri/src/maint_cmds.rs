//! Round 9 · Manutenção+ (weekly review, full repair, quick fixes) and Segurança › Antivírus:
//! thin IPC over `repair::run_plan`, `troubleshoot` and `antivirus`. No logic here.

use g2boost_core::{antivirus, repair, troubleshoot};
use tauri::{AppHandle, Emitter};

use crate::blocking;

/// Weekly review or full repair, run to the end (the UI confirmed first). Admin only; one
/// servicing job at a time; Stop = `repair_cancel`. `label` names the restore point.
#[tauri::command]
pub async fn maint_plan_run(app: AppHandle, plan: repair::Plan, label: String) -> Result<repair::PlanOutcome, String> {
    let label: String = label.chars().filter(|c| !c.is_control()).take(120).collect();
    blocking(move || {
        repair::run_plan(plan, &label, &|p| {
            let _ = app.emit("maintenance:plan-progress", p);
        })
    })
    .await
}

#[tauri::command]
pub fn maint_plan_status() -> Option<repair::PlanRunning> {
    repair::plan_status()
}

#[tauri::command]
pub fn maint_plan_history() -> Vec<repair::PlanOutcome> {
    repair::plan_history()
}

/// Local print queues and the Spooler state (no admin): the preview before "Destravar".
#[tauri::command]
pub async fn maint_print_status() -> Result<troubleshoot::PrintStatus, String> {
    blocking(|| Ok::<_, String>(troubleshoot::print_status())).await
}

/// One quick fix (Windows Update reset, print queue, audio). Admin; the UI confirmed first.
#[tauri::command]
pub async fn maint_fix_run(fix: troubleshoot::Fix) -> Result<troubleshoot::Outcome, String> {
    blocking(move || troubleshoot::run(fix)).await
}

#[tauri::command]
pub fn maint_fix_history() -> Vec<troubleshoot::Outcome> {
    troubleshoot::history()
}

#[tauri::command]
pub async fn av_status() -> Result<antivirus::Status, String> {
    blocking(|| Ok::<_, String>(antivirus::status())).await
}

/// Add this exe to Defender's exclusions (the core re-checks the rule; admin).
#[tauri::command]
pub async fn av_exclude() -> Result<antivirus::Status, String> {
    blocking(antivirus::exclude).await
}

#[tauri::command]
pub async fn av_unexclude() -> Result<antivirus::Status, String> {
    blocking(antivirus::unexclude).await
}

#[tauri::command]
pub fn av_reveal_exe() -> Result<(), String> {
    antivirus::reveal_exe().map_err(|e| e.to_string())
}
