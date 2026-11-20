//! Round 11 · Atividade: the lines long jobs leave in the bell when they end, and the native
//! notification for someone who is not looking (window in the tray or minimized).
//!
//! The webview renders every text (it holds the i18n) and asks `activity_os_notify`; this side
//! decides whether the person would miss it and how to reach them: a Windows toast when the
//! installer's shortcut gives us an app id, otherwise a taskbar flash (`FlashWindowEx` through
//! Tauri's `request_user_attention`).

use g2boost_core::activity::Tone;
use g2boost_core::{repair, updates, winnotify};
use tauri::{AppHandle, Emitter, Manager, State, UserAttentionType};

use crate::{Routines, notify};

#[derive(serde::Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Delivered {
    /// Turned off in Configurações.
    Off,
    /// The window is on screen: the bell and the toast already told them.
    Visible,
    Toast,
    Flash,
    /// Hidden in the tray where Windows takes no toast: the bell keeps the line.
    Missed,
}

/// Tell the person a job ended (or a line arrived) when the window is hidden or minimized. A click
/// on the toast brings the window back on `route` (only a route the UI knows).
#[tauri::command]
pub fn activity_os_notify(app: AppHandle, r: State<'_, Routines>, title: String, body: String, route: Option<String>) -> Delivered {
    if !r.settings.lock().unwrap().notify_native {
        return Delivered::Off;
    }
    let Some(w) = app.get_webview_window("main") else {
        return Delivered::Missed;
    };
    let minimized = w.is_minimized().unwrap_or(false);
    if w.is_visible().unwrap_or(true) && !minimized {
        return Delivered::Visible;
    }
    let route = route.filter(|r| crate::ROUTES.contains(&r.as_str()));
    if title.len() <= 4096 && body.len() <= 4096 && winnotify::available() {
        let back = app.clone();
        let shown = winnotify::show(&title, &body, move || {
            crate::show_main(&back);
            if let Some(r) = &route {
                let _ = back.emit("activity:open", r);
            }
        });
        if shown.is_ok() {
            return Delivered::Toast;
        }
    }
    if minimized {
        let _ = w.request_user_attention(Some(UserAttentionType::Informational));
        return Delivered::Flash;
    }
    Delivered::Missed
}

/// End of an app (winget) or Windows Update batch: one line in the bell.
pub fn updates_done(app: &AppHandle, b: &updates::Batch) {
    if b.items.is_empty() {
        return;
    }
    let ok = b.items.iter().filter(|i| i.status == "ok").count();
    let failed = b.items.iter().filter(|i| i.status == "failed").count();
    let windows = b.kind == "windows";
    let (key, tone) = if b.cancelled {
        ("activity.updates.cancelled", Tone::Info)
    } else if windows && b.reboot && ok > 0 {
        ("activity.updates.reboot", Tone::Warn)
    } else if failed > 0 {
        (if windows { "activity.updates.wu.partial" } else { "activity.updates.apps.partial" }, if ok == 0 { Tone::Err } else { Tone::Warn })
    } else {
        (if windows { "activity.updates.wu.ok" } else { "activity.updates.apps.ok" }, Tone::Ok)
    };
    notify(app, "updates", key, &[("count", &ok.to_string()), ("failed", &failed.to_string())], tone, Some("apps"));
}

/// End of the weekly review or the full repair (Manutenção).
pub fn plan_done(app: &AppHandle, o: &repair::PlanOutcome) {
    use repair::StepState as S;
    let cancelled = o.steps.iter().any(|s| s.state == S::Cancelled);
    let failed = o.steps.iter().filter(|s| s.state == S::Failed).count();
    let issues = failed + o.steps.iter().filter(|s| s.state == S::Attention).count();
    let weekly = o.plan == repair::Plan::Weekly;
    let (key, tone) = match (cancelled, issues) {
        (true, _) => ("activity.maint.cancelled", Tone::Info),
        (false, 0) => (if weekly { "activity.maint.weekly.ok" } else { "activity.maint.repair.ok" }, Tone::Ok),
        _ => (
            if weekly { "activity.maint.weekly.issues" } else { "activity.maint.repair.issues" },
            if failed > 0 { Tone::Err } else { Tone::Warn },
        ),
    };
    notify(app, "maintenance", key, &[("count", &issues.to_string())], tone, Some("maintenance"));
}
