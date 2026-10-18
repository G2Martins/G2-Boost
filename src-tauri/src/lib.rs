//! Tauri shell: thin IPC commands over `g2boost_core`. No business logic here.

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use std::sync::atomic::AtomicU32;

use g2boost_core::boost::{self, PurgeResult, TweakState};
use g2boost_core::{advisor, apps, claude_connect, context, dns, firmware, power, procs, registry, secrets, startup};
use g2boost_core::cleanup::{self, ConfirmToken, ExecItem, PreviewItem, RunRecord, TokenStore};
use g2boost_core::disk::{self, Node, NodeView, ScanProgress, ScanSummary};
use g2boost_core::hardware::{self, Profile};
use g2boost_core::monitor::Monitor;
use g2boost_core::network::{self, Hop, LatencyStats, Probe};
use g2boost_core::recommend::{self, Facts, Recommendation, Score};
use g2boost_core::restore::{self, CreateOutcome, RestoreStatus};
use g2boost_core::routines::{self, Clock, Game, GameWatcher, RoutineState};
use g2boost_core::settings::{self, Schedule, Settings};
use g2boost_core::system::{self, DriveInfo, MemoryStatus};
use g2boost_core::tasksched;
use g2boost_core::visualfx::Level;
use g2boost_core::winsat::{self, WinSat};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

/// Routes the UI knows. Anything else coming from `--route` / `G2BOOST_START_ROUTE` is dropped.
const ROUTES: &[&str] = &["dashboard", "disk", "cleanup", "apps", "network", "boost", "ai", "settings"];

/// `--route <name>` on the command line (set when we relaunch ourselves elevated) wins over
/// the `G2BOOST_START_ROUTE` env var (dev/screenshot aid).
fn start_route() -> Option<String> {
    let mut args = std::env::args().skip(1);
    let mut from_cli = None;
    while let Some(a) = args.next() {
        if a == "--route" {
            from_cli = args.next();
        }
    }
    from_cli
        .or_else(|| std::env::var("G2BOOST_START_ROUTE").ok())
        .filter(|r| ROUTES.contains(&r.as_str()))
}

#[derive(serde::Serialize)]
struct AppInfo {
    version: &'static str,
    core_version: &'static str,
    os: String,
    elevated: bool,
    /// Windows account name, for the dashboard greeting.
    user: String,
    /// Dev aids: `G2BOOST_START_ROUTE=network` / `G2BOOST_START_TAB=tools` open the app there
    /// (screenshots, manual tests). Harmless if unset.
    start_route: Option<String>,
    start_tab: Option<String>,
    /// `G2BOOST_AUTORUN=1`: pages kick off their main action on mount (screenshot/manual QA aid).
    autorun: bool,
    /// `G2BOOST_SCAN_PATH`: root the disk page scans on autorun (defaults to the first drive).
    scan_path: Option<String>,
    /// This executable (shown in the MCP registration hint).
    exe: String,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        core_version: g2boost_core::VERSION,
        os: system::os_version(),
        elevated: system::is_elevated(),
        user: std::env::var("USERNAME").unwrap_or_default(),
        start_route: start_route(),
        start_tab: std::env::var("G2BOOST_START_TAB").ok(),
        autorun: std::env::var("G2BOOST_AUTORUN").is_ok_and(|v| v == "1"),
        scan_path: std::env::var("G2BOOST_SCAN_PATH").ok(),
        exe: std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
    }
}

#[tauri::command]
fn list_drives() -> Vec<DriveInfo> {
    system::list_drives()
}

#[tauri::command]
fn memory_status() -> Option<MemoryStatus> {
    system::memory_status()
}

/* ---------------- boost ---------------- */

#[tauri::command]
async fn boost_state() -> Vec<TweakState> {
    tauri::async_runtime::spawn_blocking(boost::detect_all).await.unwrap_or_default()
}

#[tauri::command]
async fn boost_apply(id: String, on: bool) -> Result<TweakState, String> {
    tauri::async_runtime::spawn_blocking(move || boost::apply(&id, on).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn boost_visual_fx(level: String) -> Result<TweakState, String> {
    let level = match level.as_str() {
        "appearance" => Level::Appearance,
        "minimal" => Level::Minimal,
        "balanced" => Level::Balanced,
        "performance" => Level::Performance,
        _ => return Err("unknown_level".into()),
    };
    tauri::async_runtime::spawn_blocking(move || boost::apply_visual_fx(level).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn boost_visual_fx_revert() -> Result<TweakState, String> {
    tauri::async_runtime::spawn_blocking(|| boost::revert_visual_fx().map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn ram_purge() -> Result<PurgeResult, String> {
    tauri::async_runtime::spawn_blocking(|| boost::purge_standby_list().map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

/// Relaunch this exe elevated (UAC) on `route`, then quit this instance.
#[tauri::command]
fn relaunch_elevated(app: AppHandle, route: String) -> Result<(), String> {
    if !ROUTES.contains(&route.as_str()) {
        return Err("unknown_route".into());
    }
    boost::relaunch_elevated(&["--route", &route]).map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

/* ---------------- cleanup ---------------- */

const MAX_CATEGORIES: usize = 64;

#[tauri::command]
async fn cleanup_preview(ids: Vec<String>) -> Vec<PreviewItem> {
    if ids.len() > MAX_CATEGORIES {
        return Vec::new();
    }
    tauri::async_runtime::spawn_blocking(move || cleanup::preview(&ids)).await.unwrap_or_default()
}

/// Issued only from the UI's confirmation dialog; single use, 2-minute TTL, bound to `ids`.
#[tauri::command]
fn cleanup_confirm(tokens: State<'_, TokenStore>, ids: Vec<String>) -> Result<ConfirmToken, String> {
    if ids.is_empty() || ids.len() > MAX_CATEGORIES {
        return Err("bad_selection".into());
    }
    Ok(tokens.issue(ids))
}

#[derive(serde::Serialize, Clone)]
struct CleanupProgress {
    done: usize,
    total: usize,
}

#[tauri::command]
async fn cleanup_execute(
    app: AppHandle,
    tokens: State<'_, TokenStore>,
    token: u64,
    ids: Vec<String>,
) -> Result<RunRecord, String> {
    if !tokens.consume(token, &ids) {
        return Err("bad_token".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        cleanup::execute(&ids, "manual", |done, total| {
            let _ = app.emit("cleanup:progress", CleanupProgress { done, total });
        })
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn cleanup_history() -> Vec<RunRecord> {
    cleanup::history()
}

/* ---------------- dashboard: hardware profile, recommendations, monitor, restore points ---------------- */

#[derive(Default)]
struct Dashboard {
    profile: Mutex<Option<Profile>>,
    facts: Mutex<Option<Facts>>,
    monitor_subs: AtomicU32,
    monitor_running: AtomicBool,
}

#[tauri::command]
async fn system_profile(state: State<'_, Dashboard>, refresh: bool) -> Result<Profile, String> {
    if !refresh && let Some(p) = state.profile.lock().unwrap().clone() {
        return Ok(p);
    }
    let p = tauri::async_runtime::spawn_blocking(hardware::profile).await.map_err(|e| e.to_string())?;
    *state.profile.lock().unwrap() = Some(p.clone());
    Ok(p)
}

#[derive(serde::Serialize)]
struct Recommendations {
    facts: Facts,
    items: Vec<Recommendation>,
    score: Score,
}

#[tauri::command]
async fn recommendations(state: State<'_, Dashboard>, refresh: bool) -> Result<Recommendations, String> {
    let cached = if refresh { None } else { state.facts.lock().unwrap().clone() };
    let facts = match cached {
        Some(f) => f,
        None => {
            let f = tauri::async_runtime::spawn_blocking(recommend::facts).await.map_err(|e| e.to_string())?;
            *state.facts.lock().unwrap() = Some(f.clone());
            f
        }
    };
    let items = recommend::recommend(&facts);
    let states = tauri::async_runtime::spawn_blocking(boost::detect_all).await.map_err(|e| e.to_string())?;
    let score = recommend::score(&items, &states);
    Ok(Recommendations { facts, items, score })
}

/// Reference-counted: the sampling thread lives while at least one page listens to `monitor:tick`.
#[tauri::command]
fn monitor_start(app: AppHandle, state: State<'_, Dashboard>) {
    state.monitor_subs.fetch_add(1, Ordering::SeqCst);
    if state.monitor_running.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let st = app.state::<Dashboard>();
        if let Some(mut m) = Monitor::new() {
            loop {
                std::thread::sleep(Duration::from_secs(1));
                if st.monitor_subs.load(Ordering::SeqCst) == 0 {
                    break;
                }
                let _ = app.emit("monitor:tick", m.sample());
            }
        }
        st.monitor_running.store(false, Ordering::SeqCst);
    });
}

#[tauri::command]
fn monitor_stop(state: State<'_, Dashboard>) {
    let mut cur = state.monitor_subs.load(Ordering::SeqCst);
    while cur > 0 {
        match state.monitor_subs.compare_exchange(cur, cur - 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => break,
            Err(v) => cur = v,
        }
    }
}

#[tauri::command]
async fn restore_status() -> RestoreStatus {
    tauri::async_runtime::spawn_blocking(restore::status).await.unwrap_or(RestoreStatus { enabled: None, count: 0, last: None, frequency_minutes: 1440 })
}

#[tauri::command]
async fn restore_create(description: String) -> Result<CreateOutcome, String> {
    let d: String = description.chars().filter(|c| !c.is_control()).take(120).collect();
    tauri::async_runtime::spawn_blocking(move || restore::create(&d).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

/* ---------------- WinSAT + report ---------------- */

#[tauri::command]
async fn winsat_last() -> Result<WinSat, String> {
    tauri::async_runtime::spawn_blocking(winsat::last).await.map_err(|e| e.to_string())?
}

/// Starts Windows' own formal assessment (admin, 2–5 min, full-screen D3D tests).
#[tauri::command]
async fn winsat_start() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| winsat::start_formal().map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

/// Write `Documents\G2 Boost\g2boost-<stamp>.json` (+ `.txt`) with the profile, WinSAT and tweak
/// states, then reveal it in Explorer. Returns the JSON path.
#[tauri::command]
async fn report_save() -> Result<String, String> {
    let path = tauri::async_runtime::spawn_blocking(|| -> Result<PathBuf, String> {
        let profile = hardware::profile();
        let sat = winsat::last().unwrap_or_default();
        let tweaks = boost::detect_all();
        let dir = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .ok_or("no profile dir")?
            .join("Documents")
            .join("G2 Boost");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let stamp = {
            let clock = Clock::now();
            format!("{}-{:02}{:02}{:02}", clock.unix, clock.hour, clock.minute, clock.second)
        };
        let json = dir.join(format!("g2boost-{stamp}.json"));
        let body = serde_json::json!({ "generated_unix": g2boost_core::registry::now_unix(), "profile": profile, "winsat": sat, "tweaks": tweaks });
        std::fs::write(&json, serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let mut txt = String::new();
        txt.push_str(&format!("G2 Boost report\nComputer: {}\nOS: {}\nCPU: {} ({} cores / {} threads)\nRAM: {:.1} GB\n", profile.computer, profile.os, profile.cpu, profile.cores_physical, profile.cores_logical, profile.ram_bytes as f64 / 1e9));
        for m in &profile.memory {
            if m.size_bytes > 0 {
                txt.push_str(&format!("  DIMM {}: {:.0} GB {} {} MT/s (SPD {} MT/s) {} {}\n", m.slot, m.size_bytes as f64 / (1u64 << 30) as f64, m.kind, m.configured_mts, m.speed_mts, m.manufacturer, m.part_number));
            }
        }
        for g in &profile.gpus {
            txt.push_str(&format!("GPU: {} {:.0} GB VRAM driver {} {}\n", g.name, g.vram_bytes as f64 / 1e9, g.driver.as_deref().unwrap_or("?"), g.pcie.as_deref().unwrap_or("")));
        }
        for mo in &profile.monitors {
            txt.push_str(&format!("Monitor: {} {}x{} @ {:.0} Hz\n", mo.name, mo.width, mo.height, mo.refresh_hz));
        }
        txt.push_str(&format!("Board: {}\nBIOS: {}\n", profile.board.as_deref().unwrap_or("?"), profile.bios.as_ref().map(|b| format!("{} {} {}", b.vendor, b.version, b.date)).unwrap_or_default()));
        for d in &profile.disks {
            txt.push_str(&format!("Disk {}: {} {} {:.0} GB {}{}\n", d.index, d.model, d.bus, d.size_bytes as f64 / 1e9, match d.ssd { Some(true) => "SSD", Some(false) => "HDD", None => "?" }, if d.system { " [system]" } else { "" }));
        }
        txt.push_str(&format!("Secure Boot: {:?}  TPM: {:?}\nWinSAT: base {:?} cpu {:?} memory {:?} graphics {:?} d3d {:?} disk {:?}\n", profile.secure_boot, profile.tpm, sat.base, sat.cpu, sat.memory, sat.graphics, sat.d3d, sat.disk));
        for t in &tweaks {
            txt.push_str(&format!("tweak {}: {:?}\n", t.id, t.status));
        }
        std::fs::write(dir.join(format!("g2boost-{stamp}.txt")), txt).map_err(|e| e.to_string())?;
        Ok(json)
    })
    .await
    .map_err(|e| e.to_string())??;
    let _ = tauri_plugin_opener::reveal_item_in_dir(&path);
    Ok(path.to_string_lossy().into_owned())
}

/* ---------------- advisor, games, AI context + keys ---------------- */

#[tauri::command]
async fn advisor_report(state: State<'_, Dashboard>) -> Result<advisor::Report, String> {
    let cached = state.profile.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let p = cached.unwrap_or_else(hardware::profile);
        let sat = winsat::last().ok();
        let sample = Monitor::new().map(|mut m| {
            std::thread::sleep(Duration::from_millis(800));
            m.sample()
        });
        advisor::analyze(&p, &advisor::env(&p, sample, sat))
    })
    .await
    .map_err(|e| e.to_string())
}

/// The exact JSON the assistant will send (shown to the user before sending).
#[tauri::command]
async fn ai_context(state: State<'_, Dashboard>) -> Result<serde_json::Value, String> {
    let cached = state.profile.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let sample = Monitor::new().map(|mut m| {
            std::thread::sleep(Duration::from_millis(800));
            m.sample()
        });
        context::build_with(&cached.unwrap_or_else(hardware::profile), sample)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn ai_key_status() -> std::collections::BTreeMap<&'static str, bool> {
    secrets::PROVIDERS.iter().map(|p| (*p, matches!(secrets::get(p), Ok(Some(_))))).collect()
}

#[tauri::command]
fn ai_key_set(provider: String, key: String) -> Result<(), String> {
    secrets::set(&provider, &key).map_err(|e| e.to_string())
}

#[tauri::command]
fn ai_key_delete(provider: String) -> Result<(), String> {
    secrets::delete(&provider).map_err(|e| e.to_string())
}

/// Handed to the webview only for the duration of one request (see Docs/core/ai-advisor.md §1).
#[tauri::command]
fn ai_key_get(provider: String) -> Result<String, String> {
    secrets::get(&provider).map_err(|e| e.to_string())?.ok_or_else(|| "no_key".into())
}

/* ---------------- routines (settings, autostart, scheduler, game watcher, tray) ---------------- */

const AUTOSTART_TASK: &str = "G2 Boost";

struct Routines {
    settings: Mutex<Settings>,
    state: Mutex<RoutineState>,
    game: Mutex<Option<Game>>,
    ran_since_start: AtomicBool,
    running: AtomicBool,
}

#[tauri::command]
fn settings_get(r: State<'_, Routines>) -> Settings {
    r.settings.lock().unwrap().clone()
}

fn this_exe() -> Result<std::path::PathBuf, String> {
    std::env::current_exe().map_err(|e| e.to_string())
}

/// Claude Desktop / Claude Code configs found on this PC and whether G2 Boost is registered.
#[tauri::command]
async fn claude_targets() -> Result<Vec<claude_connect::Target>, String> {
    let exe = this_exe()?;
    tauri::async_runtime::spawn_blocking(move || claude_connect::targets(&exe)).await.map_err(|e| e.to_string())
}

/// Register `"<this exe>" --mcp` with one client (the UI confirms first; the file is backed up).
#[tauri::command]
async fn claude_connect(client: claude_connect::Client) -> Result<Vec<claude_connect::Target>, String> {
    let exe = this_exe()?;
    tauri::async_runtime::spawn_blocking(move || claude_connect::connect(client, &exe))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn claude_disconnect(client: claude_connect::Client) -> Result<Vec<claude_connect::Target>, String> {
    let exe = this_exe()?;
    tauri::async_runtime::spawn_blocking(move || claude_connect::disconnect(client, &exe))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Settings come from the UI, get sanitized in the core, and only then replace the live copy.
#[tauri::command]
fn settings_set(r: State<'_, Routines>, settings: Settings) -> Result<Settings, String> {
    let saved = settings::save(&settings).map_err(|e| e.to_string())?;
    *r.settings.lock().unwrap() = saved.clone();
    Ok(saved)
}

#[derive(serde::Serialize)]
struct AutostartStatus {
    enabled: bool,
}

#[tauri::command]
fn autostart_status() -> AutostartStatus {
    AutostartStatus { enabled: tasksched::task_exists("\\", AUTOSTART_TASK) }
}

/// Logon task running this exe with `--tray`. Elevated now → "highest privileges" (routines that
/// need admin work at logon); otherwise a least-privilege task, and those routines are skipped.
#[tauri::command]
fn autostart_set(on: bool) -> Result<AutostartStatus, String> {
    if on {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        tasksched::register_logon_task(AUTOSTART_TASK, &exe, "--tray", 10, system::is_elevated())
            .map_err(|e| e.to_string())?;
    } else {
        tasksched::delete_task(AUTOSTART_TASK).map_err(|e| e.to_string())?;
    }
    Ok(autostart_status())
}

#[derive(serde::Serialize)]
struct RoutineStatus {
    last_run: Option<u64>,
    game: Option<Game>,
    elevated: bool,
    running: bool,
}

#[tauri::command]
fn routine_status(r: State<'_, Routines>) -> RoutineStatus {
    RoutineStatus {
        last_run: r.state.lock().unwrap().last_run,
        game: r.game.lock().unwrap().clone(),
        elevated: system::is_elevated(),
        running: r.running.load(Ordering::SeqCst),
    }
}

#[derive(serde::Serialize, Clone)]
struct GameEvent {
    game: Game,
    freed_bytes: u64,
    error: Option<String>,
}

fn on_game_started(app: &AppHandle, game: Game) {
    let result = boost::purge_standby_list();
    let (freed, error) = match &result {
        Ok(p) => (p.freed_bytes, None),
        Err(e) => (0, Some(e.to_string())),
    };
    cleanup::record_run(&RunRecord {
        at: g2boost_core::registry::now_unix(),
        trigger: "game".into(),
        duration_ms: 0,
        items: vec![ExecItem { id: "ram_purge".into(), bytes_freed: freed, ..Default::default() }],
    });
    let _ = app.emit("game:started", GameEvent { game, freed_bytes: freed, error });
}

/// One background thread: game watcher every 5 s, scheduler check every 60 s.
fn spawn_routines(app: AppHandle) {
    std::thread::spawn(move || {
        let mut watcher = GameWatcher::default();
        let mut tick: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_secs(5));
            tick += 1;
            let r = app.state::<Routines>();
            let s = r.settings.lock().unwrap().clone();
            if s.game_ram_clean {
                let now = routines::detect_game(&s.game_exes);
                *r.game.lock().unwrap() = now.clone();
                if let Some(g) = watcher.observe(now) {
                    on_game_started(&app, g);
                }
            } else {
                *r.game.lock().unwrap() = None;
            }
            if tick.is_multiple_of(12) && !s.routine_categories.is_empty() && !r.running.load(Ordering::SeqCst) {
                let last = r.state.lock().unwrap().last_run;
                if routines::due(&s.schedule, Clock::now(), last, r.ran_since_start.load(Ordering::SeqCst)) {
                    r.running.store(true, Ordering::SeqCst);
                    let rec = cleanup::execute(&s.routine_categories, "routine", |_, _| {});
                    r.ran_since_start.store(true, Ordering::SeqCst);
                    let st = RoutineState { last_run: Some(rec.at) };
                    routines::save_state(&st);
                    *r.state.lock().unwrap() = st;
                    r.running.store(false, Ordering::SeqCst);
                    let _ = app.emit("routine:done", rec);
                }
            }
        }
    });
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/* ---------------- disk scan ---------------- */

#[derive(Default)]
struct DiskState {
    tree: Mutex<Option<Arc<Node>>>,
    summary: Mutex<Option<ScanSummary>>,
    running: AtomicBool,
    cancel: Arc<AtomicBool>,
}

#[derive(serde::Serialize, Clone)]
struct ScanProgressEvent {
    files: u64,
    dirs: u64,
    bytes: u64,
    errors: u64,
    current: String,
}

#[tauri::command]
fn disk_scan_start(app: AppHandle, state: State<'_, DiskState>, root: String) -> Result<(), String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("scan_running".into());
    }
    let root = PathBuf::from(root);
    state.cancel.store(false, Ordering::SeqCst);
    let cancel = state.cancel.clone();
    std::thread::spawn(move || {
        let progress = Arc::new(ScanProgress::default());
        let reporter = {
            let progress = progress.clone();
            let app = app.clone();
            let done = Arc::new(AtomicBool::new(false));
            let done2 = done.clone();
            let handle = std::thread::spawn(move || {
                while !done2.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(250));
                    let ev = ScanProgressEvent {
                        files: progress.files.load(Ordering::Relaxed),
                        dirs: progress.dirs.load(Ordering::Relaxed),
                        bytes: progress.bytes.load(Ordering::Relaxed),
                        errors: progress.errors.load(Ordering::Relaxed),
                        current: progress.current.lock().map(|c| c.clone()).unwrap_or_default(),
                    };
                    let _ = app.emit("disk:progress", ev);
                }
            });
            (done, handle)
        };
        let result = disk::scan(&root, &progress, &cancel);
        reporter.0.store(true, Ordering::Relaxed);
        let _ = reporter.1.join(); // last progress tick must land before `disk:done`
        let st = app.state::<DiskState>();
        match result {
            Ok((tree, summary)) => {
                *st.tree.lock().unwrap() = Some(Arc::new(tree));
                *st.summary.lock().unwrap() = Some(summary.clone());
                let _ = app.emit("disk:done", summary);
            }
            Err(e) => {
                let _ = app.emit("disk:error", e.to_string());
            }
        }
        st.running.store(false, Ordering::SeqCst);
    });
    Ok(())
}

#[tauri::command]
fn disk_scan_cancel(state: State<'_, DiskState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn disk_summary(state: State<'_, DiskState>) -> Option<ScanSummary> {
    state.summary.lock().unwrap().clone()
}

#[tauri::command]
fn disk_tree(state: State<'_, DiskState>, ids: Vec<u32>, depth: u8, max_children: u32) -> Option<NodeView> {
    let tree = state.tree.lock().unwrap().clone()?;
    disk::view(&tree, &ids, depth.min(4), (max_children as usize).clamp(1, 400))
}


/// Filtered file list (`*.mp4 >1gb >6m`), largest first.
#[tauri::command]
async fn disk_search(state: State<'_, DiskState>, query: String, n: u32) -> Result<Option<disk::SearchResult>, String> {
    let Some(tree) = state.tree.lock().unwrap().clone() else {
        return Ok(None);
    };
    let q: String = query.chars().take(200).collect();
    tauri::async_runtime::spawn_blocking(move || {
        Some(disk::search(&tree, &disk::parse_filter(&q), g2boost_core::registry::now_unix(), (n as usize).clamp(1, 500)))
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn disk_essentials(state: State<'_, DiskState>) -> Vec<disk::Essential> {
    state.tree.lock().unwrap().clone().map(|t| disk::essentials(&t)).unwrap_or_default()
}

#[tauri::command]
async fn disk_reveal(state: State<'_, DiskState>, ids: Vec<u32>) -> Result<(), String> {
    let tree = state.tree.lock().unwrap().clone().ok_or("no_scan")?;
    let p = disk::path_of(&tree, &ids).ok_or("not_found")?;
    tauri::async_runtime::spawn_blocking(move || disk::reveal(&p).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

/// Recycle Bin for items the user picked (the UI confirmed first). The tree is taken out of
/// the state while the shell works, so other commands never wait on a Windows dialog.
#[tauri::command]
async fn disk_recycle(state: State<'_, DiskState>, selection: Vec<Vec<u32>>) -> Result<disk::RecycleOutcome, String> {
    if selection.is_empty() || selection.len() > 500 {
        return Err("bad_selection".into());
    }
    if state.running.load(Ordering::SeqCst) {
        return Err("scan_running".into());
    }
    let arc = state.tree.lock().unwrap().take().ok_or("no_scan")?;
    let (tree, res) = tauri::async_runtime::spawn_blocking(move || {
        let mut tree = Arc::try_unwrap(arc).unwrap_or_else(|a| (*a).clone());
        let res = disk::recycle(&mut tree, &selection);
        (tree, res)
    })
    .await
    .map_err(|e| e.to_string())?;
    *state.tree.lock().unwrap() = Some(Arc::new(tree));
    res.map_err(|e| e.to_string())
}

/* ---------------- network ---------------- */

#[derive(serde::Serialize)]
struct PingReport {
    host: String,
    ip: Ipv4Addr,
    probes: Vec<Probe>,
    stats: LatencyStats,
}

/// Bounds enforced here because the caller (UI or MCP) is untrusted.
fn clamp(v: u32, lo: u32, hi: u32) -> u32 {
    v.clamp(lo, hi)
}

#[tauri::command]
async fn ping_host(
    host: String,
    count: u32,
    timeout_ms: u32,
    interval_ms: u32,
) -> Result<PingReport, String> {
    let count = clamp(count, 1, 100);
    let timeout = clamp(timeout_ms, 100, 5000);
    let interval = clamp(interval_ms, 0, 2000);
    tauri::async_runtime::spawn_blocking(move || {
        let ip = network::resolve_v4(&host).map_err(|e| e.to_string())?;
        let mut probes = Vec::with_capacity(count as usize);
        for i in 0..count {
            probes.push(network::ping(ip, timeout).map_err(|e| e.to_string())?);
            if i + 1 < count && interval > 0 {
                std::thread::sleep(Duration::from_millis(u64::from(interval)));
            }
        }
        let samples: Vec<Option<f64>> = probes
            .iter()
            .map(|p| match p {
                Probe::Reply { rtt_ms, .. } => Some(f64::from(*rtt_ms)),
                _ => None,
            })
            .collect();
        Ok(PingReport { host, ip, probes, stats: network::latency_stats(&samples) })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn trace_host(
    host: String,
    max_hops: u32,
    probes: u32,
    timeout_ms: u32,
) -> Result<Vec<Hop>, String> {
    let max_hops = clamp(max_hops, 1, 64) as u8;
    let probes = clamp(probes, 1, 5) as u8;
    let timeout = clamp(timeout_ms, 100, 5000);
    tauri::async_runtime::spawn_blocking(move || {
        let ip = network::resolve_v4(&host).map_err(|e| e.to_string())?;
        network::trace_route(ip, max_hops, probes, timeout).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// TCPing: `count` timed TCP handshakes to `host:port`, reported like a ping.
#[tauri::command]
async fn tcping_host(host: String, port: u16, count: u32, timeout_ms: u32, interval_ms: u32) -> Result<PingReport, String> {
    if port == 0 {
        return Err("invalid_port".into());
    }
    let count = clamp(count, 1, 50);
    let timeout = clamp(timeout_ms, 100, 5000);
    let interval = clamp(interval_ms, 0, 2000);
    tauri::async_runtime::spawn_blocking(move || {
        let ip = network::resolve_v4(&host).map_err(|e| e.to_string())?;
        let mut probes = Vec::with_capacity(count as usize);
        for i in 0..count {
            probes.push(match network::tcping(ip, port, timeout) {
                Some(ms) => Probe::Reply { from: ip, rtt_ms: ms.round().max(0.0) as u32 },
                None => Probe::Timeout,
            });
            if i + 1 < count && interval > 0 {
                std::thread::sleep(Duration::from_millis(u64::from(interval)));
            }
        }
        let samples: Vec<Option<f64>> = probes
            .iter()
            .map(|p| match p {
                Probe::Reply { rtt_ms, .. } => Some(f64::from(*rtt_ms)),
                _ => None,
            })
            .collect();
        Ok(PingReport { host, ip, probes, stats: network::latency_stats(&samples) })
    })
    .await
    .map_err(|e| e.to_string())?
}

/* ---------------- DNS optimizer ---------------- */

#[derive(serde::Serialize)]
struct DnsAdapterView {
    #[serde(flatten)]
    adapter: dns::Adapter,
    has_backup: bool,
}

#[tauri::command]
async fn dns_adapters() -> Vec<DnsAdapterView> {
    tauri::async_runtime::spawn_blocking(|| {
        dns::adapters()
            .into_iter()
            .map(|a| DnsAdapterView { has_backup: dns::has_backup(&a.guid), adapter: a })
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
async fn dns_rank(samples: u32) -> Vec<dns::RankEntry> {
    tauri::async_runtime::spawn_blocking(move || dns::rank(samples)).await.unwrap_or_default()
}

/// Apply a catalog provider to one adapter (admin); the core validates the provider id.
#[tauri::command]
async fn dns_apply(guid: String, provider: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || dns::apply(&guid, &provider).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn dns_restore(guid: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || dns::restore(&guid).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

/* ---------------- startup apps, processes, firmware, sensors ---------------- */

#[tauri::command]
async fn startup_list() -> Vec<startup::StartupItem> {
    tauri::async_runtime::spawn_blocking(startup::list).await.unwrap_or_default()
}

/// Toggle via `StartupApproved` (Task Manager semantics); the item must exist in the live list.
#[tauri::command]
async fn startup_set(location: startup::Location, name: String, enabled: bool) -> Result<startup::StartupItem, String> {
    tauri::async_runtime::spawn_blocking(move || startup::set_enabled(location, &name, enabled).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

/* ---------------- power plans ---------------- */

#[tauri::command]
async fn power_plans() -> power::Plans {
    tauri::async_runtime::spawn_blocking(power::plans).await.unwrap_or(power::Plans { plans: vec![], has_battery: false })
}

#[tauri::command]
async fn power_activate(guid: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || power::set_active(&guid).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn power_create_max() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| power::create_max_performance().map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn power_duplicate(guid: String, name: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || power::duplicate(&guid, &name).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn power_rename(guid: String, name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || power::rename(&guid, &name).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn power_delete(guid: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || power::delete(&guid).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn power_settings(guid: String) -> Result<Vec<power::SettingValue>, String> {
    tauri::async_runtime::spawn_blocking(move || power::settings(&guid).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn power_write(guid: String, id: String, ac: Option<u32>, dc: Option<u32>) -> Result<power::SettingValue, String> {
    tauri::async_runtime::spawn_blocking(move || power::write(&guid, &id, ac, dc).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

/* ---------------- uninstaller (apps + leftovers) ---------------- */

/// The installed list, the orphan sweep and the leftovers waiting for confirmation. The UI only
/// ever sends ids back; every app and path it acts on comes from here.
#[derive(Default)]
struct AppsState {
    apps: Mutex<Vec<apps::App>>,
    orphans: Mutex<Vec<apps::App>>,
    pending: Mutex<Option<apps::Pending>>,
    /// id → PNG data URL (None = no icon), filled on demand.
    icons: Mutex<std::collections::HashMap<String, Option<String>>>,
}

impl AppsState {
    /// Installed list, then the orphan sweep, then the app whose leftovers are pending (it may
    /// have just been uninstalled and dropped from a refreshed list).
    fn find(&self, id: &str) -> Option<apps::App> {
        let hit = self.apps.lock().unwrap().iter().find(|a| a.id == id).cloned();
        hit.or_else(|| self.orphans.lock().unwrap().iter().find(|a| a.id == id).cloned())
            .or_else(|| self.pending.lock().unwrap().as_ref().filter(|p| p.app.id == id).map(|p| p.app.clone()))
    }
}

#[tauri::command]
async fn apps_list(state: State<'_, AppsState>, r: State<'_, Routines>) -> Result<Vec<apps::App>, String> {
    let protected = r.settings.lock().unwrap().protected_apps.clone();
    let list = tauri::async_runtime::spawn_blocking(move || {
        let l = apps::list(&protected);
        apps::remember(&l);
        l
    })
    .await
    .map_err(|e| e.to_string())?;
    *state.apps.lock().unwrap() = list.clone();
    Ok(list)
}

#[derive(serde::Serialize)]
struct OrphanView {
    app: apps::App,
    count: usize,
    bytes: u64,
}

/// Apps seen before that are gone now, with what they left (only those that left something).
#[tauri::command]
async fn apps_orphans(state: State<'_, AppsState>) -> Result<Vec<OrphanView>, String> {
    let current = state.apps.lock().unwrap().clone();
    let views = tauri::async_runtime::spawn_blocking(move || {
        apps::orphans(&current)
            .into_iter()
            .filter_map(|a| {
                let l = apps::leftovers(&a);
                (!l.is_empty()).then(|| OrphanView { count: l.len(), bytes: l.iter().map(|x| x.bytes).sum(), app: a })
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| e.to_string())?;
    *state.orphans.lock().unwrap() = views.iter().map(|v| v.app.clone()).collect();
    Ok(views)
}

/// Leftovers of one app; kept server-side until the user confirms (or picks another app).
#[tauri::command]
async fn apps_leftovers(state: State<'_, AppsState>, id: String) -> Result<Vec<apps::Leftover>, String> {
    let app = state.find(&id).ok_or("unknown_app")?;
    let probe = app.clone();
    let items = tauri::async_runtime::spawn_blocking(move || apps::leftovers(&probe)).await.map_err(|e| e.to_string())?;
    *state.pending.lock().unwrap() = Some(apps::Pending { app, items: items.clone() });
    Ok(items)
}

fn leftover_token_ids(app_id: &str, ids: &[u32]) -> Vec<String> {
    let mut v: Vec<String> = ids.iter().map(|i| format!("leftover:{app_id}:{i}")).collect();
    v.sort();
    v
}

/// Issued after the human confirms the leftover list in the UI.
#[tauri::command]
fn apps_confirm(state: State<'_, AppsState>, tokens: State<'_, TokenStore>, ids: Vec<u32>) -> Result<ConfirmToken, String> {
    let pending = state.pending.lock().unwrap();
    let p = pending.as_ref().ok_or("unknown_app")?;
    if ids.is_empty() || ids.iter().any(|i| !p.items.iter().any(|l| l.id == *i)) {
        return Err("unknown_item".into());
    }
    Ok(tokens.issue(leftover_token_ids(&p.app.id, &ids)))
}

#[tauri::command]
async fn apps_remove(state: State<'_, AppsState>, tokens: State<'_, TokenStore>, token: u64, ids: Vec<u32>) -> Result<apps::RemoveOutcome, String> {
    let (app, items) = {
        let pending = state.pending.lock().unwrap();
        let p = pending.as_ref().ok_or("unknown_app")?;
        if !tokens.consume(token, &leftover_token_ids(&p.app.id, &ids)) {
            return Err("bad_token".into());
        }
        (p.app.clone(), apps::pick(p, &ids))
    };
    let out = tauri::async_runtime::spawn_blocking(move || {
        let out = apps::remove(&app, &items)?;
        if apps::leftovers(&app).is_empty() {
            apps::forget(&app.id); // nothing left: drop it from the orphan sweep
        }
        Ok::<_, apps::AppsError>(out)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    *state.pending.lock().unwrap() = None;
    Ok(out)
}

/// Start the app's own uninstaller (validated in the core; visible; its UAC if any).
#[tauri::command]
async fn apps_uninstall(state: State<'_, AppsState>, id: String) -> Result<(), String> {
    let app = state.apps.lock().unwrap().iter().find(|a| a.id == id).cloned().ok_or("unknown_app")?;
    tauri::async_runtime::spawn_blocking(move || apps::uninstall(&app).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn apps_installed(state: State<'_, AppsState>, id: String) -> bool {
    state.find(&id).is_some_and(|a| apps::installed(&a))
}

/// The app's icon as a PNG data URL (cached; the UI asks per visible row).
#[tauri::command]
async fn apps_icon(state: State<'_, AppsState>, id: String) -> Result<Option<String>, String> {
    if let Some(hit) = state.icons.lock().unwrap().get(&id) {
        return Ok(hit.clone());
    }
    let Some(app) = state.find(&id) else {
        return Ok(None);
    };
    let icon = tauri::async_runtime::spawn_blocking(move || apps::icon(&app)).await.map_err(|e| e.to_string())?;
    state.icons.lock().unwrap().insert(id, icon.clone());
    Ok(icon)
}

/// A dropped `.exe`/`.lnk` → the id of the app it belongs to.
#[tauri::command]
async fn apps_resolve(state: State<'_, AppsState>, path: String) -> Result<Option<String>, String> {
    let list = state.apps.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || apps::resolve_drop(&path, &list)).await.map_err(|e| e.to_string())
}

/// What the last leftover removal can put back (files in the Recycle Bin, registry backups).
#[tauri::command]
fn apps_undo_status() -> Option<apps::UndoRecord> {
    apps::undo_status()
}

/// Restore the last removal (files from the Recycle Bin, registry trees from their backups).
#[tauri::command]
async fn apps_undo() -> Result<apps::UndoOutcome, String> {
    tauri::async_runtime::spawn_blocking(|| apps::undo().map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
}

#[derive(Clone, serde::Serialize)]
struct RemovedApp {
    id: String,
    name: String,
}

/// SmartDelete: watch the Uninstall keys; an app that disappears (removed outside G2 Boost)
/// and was seen before is announced to the UI, which offers the leftover sweep.
fn watch_uninstalls(app: AppHandle) {
    std::thread::spawn(move || {
        let ids = |l: &[apps::App]| l.iter().map(|a| a.id.clone()).collect::<std::collections::HashSet<_>>();
        let first = apps::list(&[]);
        apps::remember(&first);
        let mut known = ids(&first);
        let stop = AtomicBool::new(false);
        registry::watch_subkeys(&apps::UNINSTALL_KEYS, &stop, || {
            let now = apps::list(&[]);
            let current = ids(&now);
            for id in known.difference(&current) {
                if let Some(a) = apps::remembered(id).filter(|a| a.protected.is_none()) {
                    let _ = app.emit("apps:removed", RemovedApp { id: a.id, name: a.name });
                }
            }
            apps::remember(&now);
            known = current;
        });
    });
}

/// Add/remove an installed app (by id) to the user's protected list.
#[tauri::command]
fn apps_protect(state: State<'_, AppsState>, r: State<'_, Routines>, id: String, protect: bool) -> Result<Settings, String> {
    let name = state.apps.lock().unwrap().iter().find(|a| a.id == id).map(|a| a.name.clone()).ok_or("unknown_app")?;
    let mut s = r.settings.lock().unwrap().clone();
    s.protected_apps.retain(|n| !n.eq_ignore_ascii_case(&name));
    if protect {
        s.protected_apps.push(name);
    }
    let saved = settings::save(&s).map_err(|e| e.to_string())?;
    *r.settings.lock().unwrap() = saved.clone();
    Ok(saved)
}

/* ---------------- task manager ---------------- */

#[derive(Default)]
struct ProcSampler(Mutex<procs::Sampler>);

/// Per-process list; CPU % is the delta since the previous call (the UI polls every 2 s).
#[tauri::command]
fn procs_list(state: State<'_, ProcSampler>) -> Vec<procs::ProcInfo> {
    state.0.lock().unwrap().sample()
}

/// `created` comes back as the decimal string the list sent (u64 does not fit a JS number).
fn parse_created(created: &str) -> Result<u64, String> {
    created.parse().map_err(|_| "process_gone".to_string())
}

/// End the exact process the user confirmed (pid + creation time); the core refuses protected ones.
#[tauri::command]
async fn procs_end(pid: u32, created: String) -> Result<(), String> {
    let created = parse_created(&created)?;
    tauri::async_runtime::spawn_blocking(move || procs::end(pid, created).map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

/// Explorer on the process image (the path is resolved here from pid + creation time).
#[tauri::command]
async fn procs_reveal(pid: u32, created: String) -> Result<(), String> {
    let created = parse_created(&created)?;
    tauri::async_runtime::spawn_blocking(move || {
        let path = procs::path_of(pid, created).map_err(|e| e.to_string())?;
        disk::reveal(std::path::Path::new(&path)).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn procs_top() -> Vec<procs::ProcGroup> {
    tauri::async_runtime::spawn_blocking(|| procs::top(Duration::from_millis(1000), 15)).await.unwrap_or_default()
}

#[tauri::command]
async fn firmware_info(state: State<'_, Dashboard>) -> Result<firmware::Firmware, String> {
    let cached = state.profile.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || firmware::read(&cached.unwrap_or_else(hardware::profile)))
        .await
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // `G2 Boost.exe --mcp`: stdio MCP server for Claude Desktop / Claude Code, no window.
    // One signed binary instead of a sidecar; the server is read-only (crates/mcp).
    if std::env::args().any(|a| a == "--mcp") {
        if let Err(e) = tauri::async_runtime::block_on(g2boost_mcp::serve()) {
            eprintln!("g2boost mcp: {e}");
        }
        return;
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_http::init())
        .manage(DiskState::default())
        .manage(ProcSampler::default())
        .manage(AppsState::default())
        .manage(TokenStore::default())
        .manage(Dashboard::default())
        .manage(Routines {
            settings: Mutex::new(settings::load()),
            state: Mutex::new(routines::load_state()),
            game: Mutex::new(None),
            ran_since_start: AtomicBool::new(false),
            running: AtomicBool::new(false),
        })
        .setup(|app| {
            watch_uninstalls(app.handle().clone());
            // Tray: left click opens, menu has Open / Quit. Labels follow the default locale (pt-BR).
            let open = MenuItem::with_id(app, "open", "Abrir", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            let mut tray = TrayIconBuilder::with_id("main")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("G2 Boost")
                .on_menu_event(|app, e| match e.id().as_ref() {
                    "open" => show_main(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, e| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                        show_main(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon().cloned() {
                tray = tray.icon(icon);
            }
            tray.build(app)?;
            // Started by the logon task: live in the tray until the user asks for the window.
            if std::env::args().any(|a| a == "--tray")
                && let Some(w) = app.get_webview_window("main")
            {
                let _ = w.hide();
            }
            spawn_routines(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let r = window.state::<Routines>();
                let s = r.settings.lock().unwrap();
                if s.close_to_tray && (s.schedule != Schedule::Off || s.game_ram_clean) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            list_drives,
            memory_status,
            boost_state,
            boost_apply,
            boost_visual_fx,
            boost_visual_fx_revert,
            ram_purge,
            relaunch_elevated,
            cleanup_preview,
            cleanup_confirm,
            cleanup_execute,
            cleanup_history,
            settings_get,
            settings_set,
            autostart_status,
            autostart_set,
            routine_status,
            system_profile,
            recommendations,
            monitor_start,
            monitor_stop,
            restore_status,
            restore_create,
            winsat_last,
            winsat_start,
            report_save,
            advisor_report,
            ai_context,
            ai_key_status,
            ai_key_set,
            ai_key_delete,
            ai_key_get,
            disk_scan_start,
            disk_scan_cancel,
            disk_summary,
            disk_tree,
            disk_search,
            disk_essentials,
            disk_reveal,
            disk_recycle,
            ping_host,
            trace_host,
            tcping_host,
            dns_adapters,
            dns_rank,
            dns_apply,
            dns_restore,
            startup_list,
            startup_set,
            procs_top,
            firmware_info,
            power_plans,
            power_activate,
            power_create_max,
            power_duplicate,
            power_rename,
            power_delete,
            power_settings,
            power_write,
            procs_list,
            procs_end,
            procs_reveal,
            apps_list,
            apps_orphans,
            apps_leftovers,
            apps_confirm,
            apps_remove,
            apps_uninstall,
            apps_installed,
            apps_resolve,
            apps_protect,
            apps_undo_status,
            apps_undo,
            apps_icon,
            claude_targets,
            claude_connect,
            claude_disconnect
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
