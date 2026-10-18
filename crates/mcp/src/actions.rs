//! Changes Claude may ask for through MCP. Each one is validated in the core and then shown to the
//! person at this PC in a native Windows dialog (topmost, "No" by default). The model cannot
//! click it, the text comes from G2 Boost's own data (never from the model), and a late "Yes"
//! (after the client gave up waiting) changes nothing. Admin-only actions answer `needs_admin`
//! instead of elevating.

use std::time::{Duration, Instant};

use g2boost_core::{apps, boost, cleanup, disk, dns, power, procs, restore, settings, startup, visualfx, winget};
use serde_json::{Value, json};
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::UI::WindowsAndMessaging::{
    IDYES, MB_DEFBUTTON2, MB_ICONWARNING, MB_SETFOREGROUND, MB_TASKMODAL, MB_TOPMOST, MB_YESNO, MessageBoxW,
};
use windows::core::PCWSTR;

/// Past this, a "Yes" is stale: the client has likely timed out and moved on.
const ANSWER_WINDOW: Duration = Duration::from_secs(180);
const MAX_LINES: usize = 14;

fn is_pt() -> bool {
    // SAFETY: plain query; primary language id 0x16 = Portuguese.
    (unsafe { GetUserDefaultUILanguage() } & 0x3FF) == 0x16
}

/// One printable line: no control characters and no bidi overrides that could disguise a name.
pub fn clean_line(s: &str, max: usize) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_control() || matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}') { ' ' } else { c })
        .collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > max { t.chars().take(max).collect::<String>() + "…" } else { t }
}

/// The dialog body: what will happen, the exact items, and a reminder.
pub fn dialog_text(pt: bool, action: (&str, &str), details: &[String]) -> String {
    let (head, foot, more) = if pt {
        ("O Claude pediu ao G2 Boost para:", "Só confirme se foi você quem pediu. Clicar em Não não muda nada.", "e mais")
    } else {
        ("Claude asked G2 Boost to:", "Only confirm if you asked for this. Clicking No changes nothing.", "and")
    };
    let mut lines: Vec<String> = details.iter().take(MAX_LINES).map(|d| format!("• {}", clean_line(d, 150))).collect();
    if details.len() > MAX_LINES {
        lines.push(format!("… {more} {}", details.len() - MAX_LINES));
    }
    let what = clean_line(if pt { action.0 } else { action.1 }, 200);
    format!("{head}\n\n{what}\n\n{}\n\n{foot}", lines.join("\n"))
}

/// Ask the person at this PC. Blocks until they answer.
pub fn confirm(action: (&str, &str), details: &[String]) -> bool {
    let pt = is_pt();
    let body = g2boost_core::registry::wide(&dialog_text(pt, action, details));
    let title = g2boost_core::registry::wide(if pt { "G2 Boost: confirmar pedido do Claude" } else { "G2 Boost: confirm Claude's request" });
    let t0 = Instant::now();
    // SAFETY: both strings outlive the call; no owner window (the dialog is topmost and task-modal).
    let r = unsafe {
        MessageBoxW(
            None,
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2 | MB_TOPMOST | MB_SETFOREGROUND | MB_TASKMODAL,
        )
    };
    r == IDYES && t0.elapsed() < ANSWER_WINDOW
}

fn declined() -> Value {
    json!({ "done": false, "reason": "declined_by_user", "note": "The person at the PC said no (or answered too late). Nothing changed." })
}

fn gb(b: u64) -> String {
    format!("{:.2} GB", b as f64 / 1_073_741_824.0)
}

/* ---------------- cleanup ---------------- */

pub fn run_cleanup(ids: Vec<String>) -> Value {
    let mut ids: Vec<String> = ids.into_iter().filter(|i| cleanup::find(i).is_some()).collect();
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return json!({ "error": "unknown_categories", "valid": cleanup::CATALOG.iter().map(|c| c.id).collect::<Vec<_>>() });
    }
    let preview = cleanup::preview(&ids);
    let total: u64 = preview.iter().map(|p| p.bytes).sum();
    let details: Vec<String> = preview.iter().map(|p| format!("{}: {}{}", p.id, gb(p.bytes), if p.needs_admin { " (admin)" } else { "" })).collect();
    if !confirm((&format!("limpar {} ({} medidos agora)", ids.len(), gb(total)), &format!("clean {} categories ({} measured now)", ids.len(), gb(total))), &details) {
        return declined();
    }
    json!({ "done": true, "run": cleanup::execute(&ids, "mcp", |_, _| {}) })
}

/* ---------------- tweaks, visual effects ---------------- */

pub fn set_tweak(id: &str, enabled: bool) -> Value {
    let Ok(t) = boost::find(id) else {
        return json!({ "error": "unknown_tweak" });
    };
    let (pt, en) = if enabled { ("ligar o ajuste", "turn on the tweak") } else { ("desligar o ajuste (restaura o valor original)", "turn off the tweak (restores the original value)") };
    if !confirm((pt, en), &[format!("{} ({})", t.id, if t.admin { "admin" } else { "user" }), "backup antes de mudar / backup before changing".into()]) {
        return declined();
    }
    match boost::apply(t.id, enabled) {
        Ok(s) => json!({ "done": true, "state": s }),
        Err(e) => json!({ "done": false, "error": e.to_string() }),
    }
}

pub fn set_visual_effects(level: visualfx::Level) -> Value {
    let name = serde_json::to_value(level).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    if !confirm(("mudar os efeitos visuais do Windows", "change Windows visual effects"), &[format!("nivel / level: {name}")]) {
        return declined();
    }
    match boost::apply_visual_fx(level) {
        Ok(s) => json!({ "done": true, "state": s }),
        Err(e) => json!({ "done": false, "error": e.to_string() }),
    }
}

/* ---------------- startup ---------------- */

pub fn set_startup(location: startup::Location, name: &str, enabled: bool) -> Value {
    let Some(item) = startup::list().into_iter().find(|i| i.location == location && i.name == name) else {
        return json!({ "error": "unknown_startup_item" });
    };
    if item.protected {
        return json!({ "error": "protected_item" });
    }
    let (pt, en) = if enabled { ("voltar a iniciar com o Windows", "start with Windows again") } else { ("não iniciar mais com o Windows", "stop starting with Windows") };
    if !confirm((pt, en), std::slice::from_ref(&item.name)) {
        return declined();
    }
    match startup::set_enabled(location, name, enabled) {
        Ok(s) => json!({ "done": true, "item": s }),
        Err(e) => json!({ "done": false, "error": e.to_string() }),
    }
}

/* ---------------- power, DNS, memory, restore point ---------------- */

pub fn set_power_plan(guid: &str) -> Value {
    let plans = power::plans();
    let Some(p) = plans.plans.iter().find(|p| p.guid.eq_ignore_ascii_case(guid.trim_matches(['{', '}']))) else {
        return json!({ "error": "unknown_plan" });
    };
    if !confirm(("ativar o plano de energia", "switch the power plan"), std::slice::from_ref(&p.name)) {
        return declined();
    }
    match power::set_active(&p.guid) {
        Ok(()) => json!({ "done": true, "active": p.name }),
        Err(e) => json!({ "done": false, "error": e.to_string() }),
    }
}

pub fn create_max_performance_plan() -> Value {
    if !confirm(("criar e ativar uma cópia do plano Desempenho máximo", "create and activate an Ultimate Performance copy"), &["mais consumo e calor / more power and heat".into()]) {
        return declined();
    }
    match power::create_max_performance() {
        Ok(guid) => json!({ "done": true, "guid": guid }),
        Err(e) => json!({ "done": false, "error": e.to_string() }),
    }
}

pub fn set_dns(adapter: &str, provider: &str) -> Value {
    let Some(a) = dns::adapters().into_iter().find(|a| a.guid.eq_ignore_ascii_case(adapter)) else {
        return json!({ "error": "unknown_adapter" });
    };
    if !confirm(("trocar o DNS do adaptador (backup antes)", "change the adapter's DNS (backup first)"), &[a.name.clone(), format!("-> {provider}")]) {
        return declined();
    }
    match dns::apply(&a.guid, provider) {
        Ok(()) => json!({ "done": true }),
        Err(e) => json!({ "done": false, "error": e.to_string() }),
    }
}

pub fn restore_dns(adapter: &str) -> Value {
    let Some(a) = dns::adapters().into_iter().find(|a| a.guid.eq_ignore_ascii_case(adapter)) else {
        return json!({ "error": "unknown_adapter" });
    };
    if !confirm(("voltar o DNS do adaptador ao que era antes", "restore the adapter's previous DNS"), std::slice::from_ref(&a.name)) {
        return declined();
    }
    match dns::restore(&a.guid) {
        Ok(()) => json!({ "done": true }),
        Err(e) => json!({ "done": false, "error": e.to_string() }),
    }
}

pub fn purge_standby_memory() -> Value {
    if !confirm(("liberar a memória em espera (standby list)", "free the standby memory list"), &["admin".into()]) {
        return declined();
    }
    match boost::purge_standby_list() {
        Ok(r) => json!({ "done": true, "result": r }),
        Err(e) => json!({ "done": false, "error": e.to_string() }),
    }
}

pub fn create_restore_point() -> Value {
    if !confirm(("criar um ponto de restauração do Windows", "create a Windows restore point"), &["admin".into()]) {
        return declined();
    }
    match restore::create("G2 Boost (Claude)") {
        Ok(o) => json!({ "done": true, "outcome": o }),
        Err(e) => json!({ "done": false, "error": e.to_string() }),
    }
}

/* ---------------- processes ---------------- */

pub fn processes() -> Value {
    let mut s = procs::Sampler::default();
    let _ = s.sample();
    std::thread::sleep(Duration::from_millis(1000));
    let mut list = s.sample();
    list.sort_by(|a, b| b.cpu_pct.total_cmp(&a.cpu_pct).then(b.ram_bytes.cmp(&a.ram_bytes)));
    list.truncate(60);
    json!(list)
}

pub fn end_process(pid: u32, created: u64) -> Value {
    let mut s = procs::Sampler::default();
    let Some(p) = s.sample().into_iter().find(|p| p.pid == pid && p.created == created) else {
        return json!({ "error": "process_gone" });
    };
    if p.system || p.protected.is_some() {
        return json!({ "error": "protected_process" });
    }
    if !confirm(("encerrar o processo (dados não salvos se perdem)", "end the process (unsaved data is lost)"), &[format!("{} (PID {pid})", p.name), p.path.clone()]) {
        return declined();
    }
    match procs::end(pid, created) {
        Ok(()) => json!({ "done": true }),
        Err(e) => json!({ "done": false, "error": e.to_string() }),
    }
}

/* ---------------- apps ---------------- */

fn find_app(id: &str) -> Option<apps::App> {
    let s = settings::load();
    let list = apps::list(&s.protected_apps);
    apps::remember(&list);
    list.into_iter().find(|a| a.id == id).or_else(|| apps::remembered(id))
}

pub fn uninstall_app(id: &str) -> Value {
    let Some(app) = find_app(id) else {
        return json!({ "error": "unknown_app" });
    };
    if app.protected.is_some() {
        return json!({ "error": "protected_app", "why": app.protected });
    }
    if !confirm(("abrir o desinstalador do app", "open the app's uninstaller"), &[format!("{} ({})", app.name, app.publisher)]) {
        return declined();
    }
    match apps::uninstall(&app) {
        Ok(()) => json!({ "done": true, "next": "When the app's own uninstaller finishes, call get_app_leftovers with the same id." }),
        Err(e) => json!({ "done": false, "error": e.to_string() }),
    }
}

/// What a removal would take: everything except the user's own data and low-confidence guesses.
fn removable(app: &apps::App) -> Vec<apps::Leftover> {
    apps::leftovers(app).into_iter().filter(|l| !l.precious && l.confidence != apps::Confidence::Low).collect()
}

pub fn app_leftovers(id: &str) -> Value {
    let Some(app) = find_app(id) else {
        return json!({ "error": "unknown_app" });
    };
    let all = apps::leftovers(&app);
    let s = settings::load();
    let companions: Vec<Value> = apps::companions(&app, &apps::list(&s.protected_apps))
        .iter()
        .map(|c| json!({ "id": c.id, "name": c.name, "publisher": c.publisher }))
        .collect();
    json!({
        "app": app.name,
        "companions_still_installed": companions,
        "leftovers": all.iter().map(|l| json!({ "kind": l.kind, "path": g2boost_core::winevents::redact_here(&l.path), "bytes": l.bytes, "confidence": l.confidence, "reason": l.reason, "precious": l.precious, "needs_admin": l.needs_admin })).collect::<Vec<_>>(),
        "remove_app_leftovers_takes": removable(&app).len(),
        "note": "remove_app_leftovers never takes the user's own data (precious) or low-confidence items; the app's screen can.",
    })
}

pub fn remove_app_leftovers(id: &str) -> Value {
    let Some(app) = find_app(id) else {
        return json!({ "error": "unknown_app" });
    };
    if apps::installed(&app) {
        return json!({ "error": "still_installed", "note": "Uninstall it first (uninstall_app)." });
    }
    let items = removable(&app);
    if items.is_empty() {
        return json!({ "done": true, "removed": 0 });
    }
    let total: u64 = items.iter().map(|l| l.bytes).sum();
    let details: Vec<String> = items.iter().map(|l| g2boost_core::winevents::redact_here(&l.path)).collect();
    if !confirm((&format!("remover as sobras de {} ({}, Lixeira e backup do registro)", app.name, gb(total)), &format!("remove {}'s leftovers ({}, Recycle Bin and registry backup)", app.name, gb(total))), &details) {
        return declined();
    }
    match apps::remove(&app, &items) {
        Ok(o) => json!({ "done": true, "outcome": o }),
        Err(e) => json!({ "done": false, "error": e.to_string() }),
    }
}

/* ---------------- files ---------------- */

pub fn recycle_paths(paths: Vec<String>) -> Value {
    let checked = match disk::validate_recycle(&paths) {
        Ok(c) => c,
        Err(e) => return json!({ "error": e.to_string() }),
    };
    let details: Vec<String> = checked.iter().map(|p| g2boost_core::winevents::redact_here(&p.to_string_lossy())).collect();
    if !confirm((&format!("enviar {} item(ns) para a Lixeira", checked.len()), &format!("send {} item(s) to the Recycle Bin", checked.len())), &details) {
        return declined();
    }
    json!({ "done": true, "outcome": disk::recycle_validated(&checked, "mcp_recycle") })
}

/* ---------------- winget ---------------- */

pub fn winget_install(id: &str, upgrade: bool) -> Value {
    let pkg = match winget::exact(id) {
        Ok(p) => p,
        Err(e) => return json!({ "error": e.to_string() }),
    };
    let (pt, en) = if upgrade { ("atualizar pelo winget", "update with winget") } else { ("instalar pelo winget (fonte oficial)", "install with winget (official source)") };
    if !confirm((pt, en), &[format!("{} {} ({})", pkg.name, pkg.version, pkg.id)]) {
        return declined();
    }
    match winget::install(&pkg.id, upgrade) {
        Ok(o) => json!({ "done": o.ok, "outcome": o }),
        Err(e) => json!({ "done": false, "error": e.to_string() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_text_is_bounded_and_cannot_be_disguised() {
        let evil = "fotos\u{202E}gpj.exe\nlinha falsa\u{0}";
        assert_eq!(clean_line(evil, 100), "fotos gpj.exe linha falsa");
        let many: Vec<String> = (0..30).map(|i| format!("item {i}")).collect();
        let t = dialog_text(true, ("limpar", "clean"), &many);
        assert!(t.starts_with("O Claude pediu ao G2 Boost para:"));
        assert!(t.contains("• item 13") && !t.contains("item 14\n") && t.contains("e mais 16"));
        assert!(dialog_text(false, ("limpar", "clean"), &[]).contains("Claude asked G2 Boost to:"));
        assert!(clean_line(&"x".repeat(300), 150).chars().count() <= 151);
    }

    #[test]
    fn invalid_requests_never_reach_the_dialog() {
        // Each returns before `confirm` (which would block a test on a dialog).
        assert_eq!(run_cleanup(vec!["../../x".into()])["error"], "unknown_categories");
        assert_eq!(set_tweak("rm -rf", true)["error"], "unknown_tweak");
        assert_eq!(set_power_plan("{00000000-0000-0000-0000-000000000000}")["error"], "unknown_plan");
        assert_eq!(set_dns("{bad}", "cloudflare")["error"], "unknown_adapter");
        assert_eq!(end_process(0, 0)["error"], "process_gone");
        assert_eq!(uninstall_app("no-such-app")["error"], "unknown_app");
        assert!(recycle_paths(vec![r"C:\".into()])["error"].is_string());
        assert!(winget_install("a;b", false)["error"].is_string());
    }
}
