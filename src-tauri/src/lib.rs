//! Tauri shell: thin IPC commands over `g2boost_core`. No business logic here.

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use std::sync::atomic::AtomicU32;

use g2boost_core::boost::{self, PurgeResult, TweakState};
use g2boost_core::cleanup::{self, ConfirmToken, ExecItem, PreviewItem, RunRecord, TokenStore};
use g2boost_core::disk::{self, BigFile, Node, NodeView, ScanProgress, ScanSummary};
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
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

/// Routes the UI knows. Anything else coming from `--route` / `G2BOOST_START_ROUTE` is dropped.
const ROUTES: &[&str] = &["dashboard", "disk", "cleanup", "network", "boost", "settings"];

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

#[tauri::command]
fn disk_top_files(state: State<'_, DiskState>, n: u32) -> Vec<BigFile> {
    let Some(tree) = state.tree.lock().unwrap().clone() else {
        return Vec::new();
    };
    disk::top_files(&tree, (n as usize).clamp(1, 500))
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_http::init())
        .manage(DiskState::default())
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
            disk_scan_start,
            disk_scan_cancel,
            disk_summary,
            disk_tree,
            disk_top_files,
            ping_host,
            trace_host
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
