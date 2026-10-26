//! Tauri shell: thin IPC commands over `g2boost_core`. No business logic here.

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use std::sync::atomic::AtomicU32;

use g2boost_core::boost::{self, PurgeResult, TweakState};
use g2boost_core::{advisor, apps, boottime, claude_connect, consent, console, context, devices, dns, dupes, firmware, gamemode, games, luli, netconns, netetw, power, prio, procs, qos, registry, secrets, smart, startup, userassist, wlan};
use g2boost_core::{repair, volcache};
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
use g2boost_core::exposure;
use g2boost_core::tasksched;
use g2boost_core::{adlx, nvapi};
use g2boost_core::updates;
use g2boost_core::visualfx::Level;
use g2boost_core::winsat::{self, WinSat};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

// r9:games mod
mod games_cmds;

// r9:tweaks mod
mod tweaks_cmds;

// r9:maint mod
mod maint_cmds;

// r9:hw mod
mod hw_cmds;

// r9:install mod
mod install_cmds;

// r9:tools mod
mod tools_cmds;

// r9:overlay mod
mod overlay_cmds;

// r9:profiles mod
mod profiles_cmds;

// r9:setup mod
mod update_cmds;


/// Routes the UI knows. Anything else coming from `--route` / `G2BOOST_START_ROUTE` is dropped.
const ROUTES: &[&str] = &[
    "dashboard", "mypc", "hwmonitor", "maintenance", "security", "setup", "debloat", "boost", "peripherals", "profiles",
    "gpu", "games", "apps", "cleanup", "disk", "network", "overlay", "mousetest", "monitortest", "packetloss", "console",
    "ai", "settings",
];

/// `--route <name>` on the command line (set when we relaunch ourselves elevated) wins over
/// the `G2BOOST_START_ROUTE` env var (dev/screenshot aid).
fn start_route() -> Option<String> {
    cli_arg("--route").or_else(|| std::env::var("G2BOOST_START_ROUTE").ok()).filter(|r| ROUTES.contains(&r.as_str()))
}

/// Value after `flag` on our own command line (`--route x`, `--tab y`).
fn cli_arg(flag: &str) -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == flag {
            return args.next();
        }
    }
    None
}

/// Tab ids are short lowercase words (`updates`, `mypc`); anything else is dropped.
fn valid_tab(t: &str) -> bool {
    (1..=24).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
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
    /// "Sempre abrir como administrador" on and this copy is not elevated: the UI shows its own
    /// explanation first, then asks UAC (the Windows prompt dims over that screen, not a blank desktop).
    ask_admin: bool,
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
        start_tab: cli_arg("--tab").or_else(|| std::env::var("G2BOOST_START_TAB").ok()).filter(|t| valid_tab(t)),
        autorun: std::env::var("G2BOOST_AUTORUN").is_ok_and(|v| v == "1"),
        scan_path: std::env::var("G2BOOST_SCAN_PATH").ok(),
        exe: std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        ask_admin: !system::is_elevated() && !std::env::args().any(|a| a == "--tray") && settings::load().always_admin,
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
    blocking(move || boost::apply(&id, on)).await
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
    blocking(move || boost::apply_visual_fx(level)).await
}

#[tauri::command]
async fn boost_visual_fx_revert() -> Result<TweakState, String> {
    blocking(boost::revert_visual_fx).await
}

#[tauri::command]
async fn ram_purge() -> Result<PurgeResult, String> {
    blocking(boost::purge_standby_list).await
}

/// Relaunch this exe elevated (UAC) on `route`, then quit this instance.
#[tauri::command]
fn relaunch_elevated(app: AppHandle, route: String, tab: Option<String>) -> Result<(), String> {
    if !ROUTES.contains(&route.as_str()) {
        return Err("unknown_route".into());
    }
    let mut args = vec!["--route", route.as_str()];
    if let Some(t) = tab.as_deref().filter(|t| valid_tab(t)) {
        args.extend(["--tab", t]);
    }
    boost::relaunch_elevated(&args).map_err(|e| e.to_string())?;
    app.exit(0);
    Ok(())
}

/// Core work off the async runtime; core errors become their message (what every command returns).
async fn blocking<T: Send + 'static, E: ToString>(f: impl FnOnce() -> Result<T, E> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || f().map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
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
            // Fixed 1 s cadence like Task Manager: sleep to the next deadline, not 1 s after a slow
            // sample (sensors/NVML can take hundreds of ms), so ticks don't drift to 1.1–1.8 s.
            let mut next = std::time::Instant::now();
            loop {
                next += Duration::from_secs(1);
                let now = std::time::Instant::now();
                match next.checked_duration_since(now) {
                    Some(wait) => std::thread::sleep(wait),
                    None => next = now, // fell behind (machine stalled): restart the cadence
                }
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
    blocking(move || restore::create(&d)).await
}

/* ---------------- WinSAT + report ---------------- */

#[tauri::command]
async fn winsat_last() -> Result<WinSat, String> {
    tauri::async_runtime::spawn_blocking(winsat::last).await.map_err(|e| e.to_string())?
}

/// Starts Windows' own formal assessment (admin, 2–5 min, full-screen D3D tests).
#[tauri::command]
async fn winsat_start() -> Result<(), String> {
    blocking(winsat::start_formal).await
}

/// Write `Documents\G2 Boost\g2boost-<stamp>.json` (+ `.txt`) with the profile, WinSAT and tweak
/// states, then reveal it in Explorer. Returns the JSON path.
#[tauri::command]
async fn report_save() -> Result<String, String> {
    let path = tauri::async_runtime::spawn_blocking(|| -> Result<PathBuf, String> {
        let profile = hardware::profile();
        let sat = winsat::last().unwrap_or_default();
        let tweaks = boost::detect_all();
        let dir = g2boost_core::registry::documents_g2().map_err(|e| e.to_string())?;
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
    /// Per-exe priority rules: each process is touched once (prio::Enforcer).
    prio: Mutex<prio::Enforcer>,
}

/* ---------------- program icons by path ---------------- */

/// Icons already extracted (path → PNG data URL or none). Cleared when it grows past 1000.
#[derive(Default)]
struct IconCache(Mutex<std::collections::HashMap<String, Option<String>>>);

/// A program's icon by its .exe path (processes, network usage, privacy history). The path comes
/// from the webview: only a local, existing `.exe` is read, never a UNC path (no network access,
/// no credentials sent to a share); nothing is executed.
#[tauri::command]
async fn exe_icon(cache: State<'_, IconCache>, path: String) -> Result<Option<String>, String> {
    let key = path.to_ascii_lowercase();
    if let Some(hit) = cache.0.lock().unwrap().get(&key) {
        return Ok(hit.clone());
    }
    let p = PathBuf::from(&path);
    let ok = path.len() <= 520
        && !path.starts_with("\\\\")
        && !path.starts_with("//")
        && p.is_absolute()
        && key.ends_with(".exe")
        && std::fs::metadata(&p).is_ok_and(|m| m.is_file());
    let icon = if ok {
        tauri::async_runtime::spawn_blocking(move || {
            g2boost_core::icons::png_from_resource(&p, 0).map(|png| g2boost_core::icons::data_url(&png))
        })
        .await
        .map_err(|e| e.to_string())?
    } else {
        None
    };
    let mut c = cache.0.lock().unwrap();
    if c.len() > 1000 {
        c.clear();
    }
    c.insert(key, icon.clone());
    Ok(icon)
}

/* ---------------- network usage, Wi-Fi ---------------- */

#[derive(serde::Serialize)]
struct UsageRow {
    #[serde(flatten)]
    net: netconns::ProcNet,
    /// Bytes/s since the previous call (ETW, elevated only).
    down_bps: Option<f64>,
    up_bps: Option<f64>,
}

#[derive(serde::Serialize)]
struct NetUsage {
    processes: Vec<UsageRow>,
    tcp: usize,
    udp: usize,
    /// Per-program download/upload rates are available (the ETW session runs: needs admin).
    bytes_available: bool,
}

/// The ETW session behind per-program rates: started when the usage tab asks (elevated only),
/// stopped when it closes, so no tracing runs in the background otherwise.
#[derive(Default)]
struct NetEtwState {
    monitor: Mutex<Option<netetw::Monitor>>,
    prev: Mutex<Option<(std::time::Instant, Vec<netetw::ProcBytes>)>>,
}

/// Who is using the network right now: connections per program (no admin, no DNS lookups), plus
/// download/upload rates per program when elevated.
#[tauri::command]
async fn net_usage(app: AppHandle) -> Result<NetUsage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conns = netconns::connections();
        let tcp = conns.iter().filter(|c| matches!(c.protocol, netconns::Protocol::Tcp)).count();
        let st = app.state::<NetEtwState>();
        let mut mon = st.monitor.lock().unwrap();
        if mon.is_none() && system::is_elevated() {
            *mon = netetw::Monitor::start().ok();
        }
        // Per-exe rates: pid → image path, summed by path (lowercase).
        let mut by_exe: std::collections::HashMap<String, (f64, f64)> = std::collections::HashMap::new();
        if let Some(m) = mon.as_ref() {
            let now = m.snapshot();
            let mut prev = st.prev.lock().unwrap();
            if let Some((t, p)) = prev.as_ref() {
                let paths: std::collections::HashMap<u32, String> = procs::list_paths().into_iter().collect();
                for r in netetw::rates(p, &now, t.elapsed().as_secs_f64()) {
                    if let Some(path) = paths.get(&r.pid) {
                        let e = by_exe.entry(path.to_ascii_lowercase()).or_default();
                        e.0 += r.down_bytes_per_sec;
                        e.1 += r.up_bytes_per_sec;
                    }
                }
            }
            *prev = Some((std::time::Instant::now(), now));
        }
        let available = mon.is_some();
        let processes = netconns::by_process(&conns)
            .into_iter()
            .map(|net| {
                let rate = by_exe.get(&net.exe_path.to_ascii_lowercase()).copied();
                UsageRow {
                    down_bps: available.then(|| rate.map_or(0.0, |r| r.0)),
                    up_bps: available.then(|| rate.map_or(0.0, |r| r.1)),
                    net,
                }
            })
            .collect();
        NetUsage { processes, tcp, udp: conns.len() - tcp, bytes_available: available }
    })
    .await
    .map_err(|e| e.to_string())
}

/// The usage tab closed: stop the ETW session.
#[tauri::command]
fn net_usage_stop(st: State<'_, NetEtwState>) {
    *st.monitor.lock().unwrap() = None; // Drop stops the session
    *st.prev.lock().unwrap() = None;
}

#[derive(serde::Serialize)]
struct WifiView {
    status: Option<wlan::WifiStatus>,
    issues: Vec<wlan::WifiIssue>,
    networks: Vec<wlan::WifiNetwork>,
    channels: Vec<wlan::ChannelLoad>,
    best_channel: Option<u32>,
    /// `location_denied`, `no_service` or `api_<code>`.
    error: Option<String>,
}

fn wlan_code(e: &wlan::WlanError) -> String {
    match e {
        wlan::WlanError::LocationDenied => "location_denied".into(),
        wlan::WlanError::NoService => "no_service".into(),
        wlan::WlanError::Api(c) => format!("api_{c}"),
    }
}

#[tauri::command]
async fn wifi_report(refresh: bool) -> Result<WifiView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let status = match wlan::status() {
            Ok(s) => s,
            Err(e) => {
                return WifiView { status: None, issues: vec![], networks: vec![], channels: vec![], best_channel: None, error: Some(wlan_code(&e)) };
            }
        };
        let (networks, error) = match wlan::scan(refresh) {
            Ok(n) => (n, None),
            Err(e) => (Vec::new(), Some(wlan_code(&e))),
        };
        let band = status.as_ref().and_then(|s| s.band).unwrap_or(wlan::Band::Ghz2_4);
        WifiView {
            issues: status.as_ref().map(|s| wlan::security_issues(s, &networks)).unwrap_or_default(),
            channels: wlan::congestion(&networks, band),
            best_channel: wlan::best_channel(&networks, band),
            status,
            networks,
            error,
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/* ---------------- disk health, privacy, app usage, boot ---------------- */

#[derive(serde::Serialize)]
struct DiskHealthView {
    #[serde(flatten)]
    health: smart::DriveHealth,
    /// What got worse since the last saved sample (alert).
    changes: Vec<smart::Change>,
}

/// S.M.A.R.T./NVMe health of every drive; one sample a day is kept to spot a drive getting worse.
#[tauri::command]
async fn disk_health() -> Result<Vec<DiskHealthView>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let now = g2boost_core::registry::now_unix();
        let drives = smart::health();
        let views = drives
            .iter()
            .map(|d| DiskHealthView {
                changes: smart::last_sample(&d.key).map(|prev| smart::worsened(&prev, d)).unwrap_or_default(),
                health: d.clone(),
            })
            .collect();
        smart::record(now, &drives);
        views
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct AccessView {
    #[serde(flatten)]
    record: consent::AccessRecord,
    /// Where the program is now (for its icon): the recorded path, or the same exe in a newer
    /// version folder (Discord's `app-1.0.x`).
    icon_path: Option<String>,
    /// A desktop app whose program no longer exists anywhere near: uninstalled.
    gone: bool,
}

/// The recorded exe, or the same file name in a sibling version folder (newest), if the recorded
/// one is gone. Only looks one level around the recorded path; never follows links.
fn current_exe(recorded: &str) -> Option<String> {
    let p = std::path::Path::new(recorded);
    if p.is_file() {
        return Some(recorded.to_string());
    }
    let name = p.file_name()?;
    let grand = p.parent()?.parent()?;
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for e in std::fs::read_dir(grand).ok()?.flatten().take(200) {
        if !e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink()) {
            continue;
        }
        let cand = e.path().join(name);
        if let Ok(m) = std::fs::metadata(&cand)
            && m.is_file()
        {
            let t = m.modified().unwrap_or(std::time::UNIX_EPOCH);
            if best.as_ref().is_none_or(|(bt, _)| t > *bt) {
                best = Some((t, cand));
            }
        }
    }
    best.map(|(_, p)| p.to_string_lossy().into_owned())
}

/// Which apps used the camera, microphone and location, and when (read from Windows' own log).
#[tauri::command]
async fn privacy_access() -> Result<Vec<AccessView>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        consent::history()
            .into_iter()
            .map(|r| {
                let icon_path = if r.packaged { None } else { current_exe(&r.path) };
                AccessView { gone: !r.packaged && icon_path.is_none(), icon_path, record: r }
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct AppUsage {
    id: String,
    /// Last time a program inside the app's folder was started from Explorer/Start (unix s).
    last_used: Option<u64>,
}

/// Last use of each installed app, from the Explorer's launch log (UserAssist). A hint, not proof:
/// apps started by other programs or at logon are not counted.
#[tauri::command]
async fn apps_usage(state: State<'_, AppsState>) -> Result<Vec<AppUsage>, String> {
    let list = state.apps.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let usage = userassist::usage();
        list.into_iter()
            .map(|a| AppUsage {
                last_used: a.install_location.as_deref().filter(|d| d.len() > 3).and_then(|d| userassist::last_used_under(&usage, d)),
                id: a.id,
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct BootView {
    #[serde(flatten)]
    report: boottime::BootReport,
    /// The Diagnostic Policy Service is off (PC Boost tweak `svc_dps`): Windows stops measuring.
    dps_off: bool,
}

/// Boot time and what slowed it (Windows' boot performance log; needs admin to read).
#[tauri::command]
async fn boot_report() -> Result<BootView, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let report = boottime::report(30).map_err(|e| match e {
            boottime::BootError::NeedsAdmin => "needs_admin".to_string(),
            other => other.to_string(),
        })?;
        let dps_off = boost::find("svc_dps").is_ok_and(|t| matches!(boost::detect(t).status, boost::Status::On));
        Ok(BootView { report, dps_off })
    })
    .await
    .map_err(|e| e.to_string())?
}

/* ---------------- duplicates (over the last disk scan) ---------------- */

#[derive(Default)]
struct DupState {
    running: AtomicBool,
    cancel: Arc<AtomicBool>,
    result: Mutex<Option<Vec<dupes::DupGroup>>>,
}

#[derive(serde::Serialize, Clone)]
struct DupProgressEvent {
    candidates: u64,
    hashed_bytes: u64,
    errors: u64,
}

/// Look for duplicate files (≥ 1 MB) in the last disk scan; progress events, result kept here.
#[tauri::command]
fn dupes_start(app: AppHandle, disk: State<'_, DiskState>, dup: State<'_, DupState>) -> Result<(), String> {
    if disk.running.load(Ordering::SeqCst) {
        return Err("scan_running".into());
    }
    let tree = disk.tree.lock().unwrap().clone().ok_or("no_scan")?;
    let root = disk.summary.lock().unwrap().as_ref().map(|s| PathBuf::from(&s.root)).ok_or("no_scan")?;
    if dup.running.swap(true, Ordering::SeqCst) {
        return Err("dupes_running".into());
    }
    dup.cancel.store(false, Ordering::SeqCst);
    let cancel = dup.cancel.clone();
    std::thread::spawn(move || {
        let progress = Arc::new(dupes::DupProgress::default());
        let done = Arc::new(AtomicBool::new(false));
        let reporter = {
            let (progress, done, app) = (progress.clone(), done.clone(), app.clone());
            std::thread::spawn(move || {
                while !done.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(300));
                    let _ = app.emit(
                        "dupes:progress",
                        DupProgressEvent {
                            candidates: progress.candidates.load(Ordering::Relaxed),
                            hashed_bytes: progress.hashed_bytes.load(Ordering::Relaxed),
                            errors: progress.errors.load(Ordering::Relaxed),
                        },
                    );
                }
            })
        };
        let files = dupes::files_from_tree(&tree, &root, 1 << 20);
        let groups = dupes::find(files, &cancel, &progress);
        done.store(true, Ordering::Relaxed);
        let _ = reporter.join();
        let st = app.state::<DupState>();
        *st.result.lock().unwrap() = Some(groups.clone());
        st.running.store(false, Ordering::SeqCst);
        let _ = app.emit("dupes:done", groups);
    });
    Ok(())
}

#[tauri::command]
fn dupes_cancel(dup: State<'_, DupState>) {
    dup.cancel.store(true, Ordering::SeqCst);
}

/// Copies the user picked, to the Recycle Bin. Only paths that are in the last duplicate result
/// (never a path invented by the caller), then the disk map's guard (`validate_recycle`).
#[tauri::command]
async fn dupes_recycle(dup: State<'_, DupState>, paths: Vec<String>) -> Result<disk::RecycleOutcome, String> {
    let known: std::collections::HashSet<String> = dup.result.lock().unwrap().as_ref().ok_or("no_result")?.iter().flat_map(|g| g.paths.iter().cloned()).collect();
    if paths.is_empty() || paths.iter().any(|p| !known.contains(p)) {
        return Err("unknown_item".into());
    }
    // Never every copy of a group: one always stays.
    for g in dup.result.lock().unwrap().iter().flatten() {
        if g.paths.iter().all(|p| paths.contains(p)) {
            return Err("keeps_none".into());
        }
    }
    let out = tauri::async_runtime::spawn_blocking(move || {
        let checked = disk::validate_recycle(&paths).map_err(|e| e.to_string())?;
        Ok::<_, String>(disk::recycle_validated(&checked, "dupes"))
    })
    .await
    .map_err(|e| e.to_string())??;
    if let Some(groups) = dup.result.lock().unwrap().as_mut() {
        for g in groups.iter_mut() {
            g.paths.retain(|p| std::path::Path::new(p).exists());
        }
        groups.retain(|g| g.paths.len() > 1);
    }
    Ok(out)
}

/* ---------------- games + Game Mode ---------------- */

/// Installed games (refreshed every 5 min by the routine thread, or on demand), their icons, and
/// the game whose Game Mode the user ended by hand (not restarted until that game closes).
#[derive(Default)]
struct GamesState {
    list: Mutex<Vec<games::InstalledGame>>,
    refreshed: Mutex<Option<std::time::Instant>>,
    icons: Mutex<std::collections::HashMap<String, Option<String>>>,
    suppressed: Mutex<Option<String>>,
}

impl GamesState {
    fn current(&self, s: &Settings, force: bool) -> Vec<games::InstalledGame> {
        let stale = self.refreshed.lock().unwrap().is_none_or(|t| t.elapsed() > Duration::from_secs(300));
        if force || stale {
            let list = games::library(s);
            *self.list.lock().unwrap() = list;
            *self.refreshed.lock().unwrap() = Some(std::time::Instant::now());
        }
        self.list.lock().unwrap().clone()
    }
}

#[derive(serde::Serialize)]
struct GameView {
    #[serde(flatten)]
    game: games::InstalledGame,
    enabled: bool,
    running: bool,
}

#[tauri::command]
async fn games_list(app: AppHandle, refresh: bool) -> Result<Vec<GameView>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = app.state::<Routines>().settings.lock().unwrap().clone();
        let list = app.state::<GamesState>().current(&s, refresh);
        let running = games::running(&list, &procs::list_paths(), games::steam_running_app());
        list.into_iter()
            .map(|g| GameView {
                enabled: s.game_mode_games.contains(&g.id),
                running: running.iter().any(|r| r.id == g.id),
                game: g,
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn games_icon(state: State<'_, GamesState>, id: String) -> Result<Option<String>, String> {
    if let Some(hit) = state.icons.lock().unwrap().get(&id) {
        return Ok(hit.clone());
    }
    let Some(game) = state.list.lock().unwrap().iter().find(|g| g.id == id).cloned() else {
        return Ok(None);
    };
    let icon = tauri::async_runtime::spawn_blocking(move || games::icon(&game)).await.map_err(|e| e.to_string())?;
    state.icons.lock().unwrap().insert(id, icon.clone());
    Ok(icon)
}

#[derive(serde::Serialize)]
struct GameServer {
    ip: String,
    port: u16,
    connections: usize,
}

/// Internet servers the running game talks to over TCP right now (processes inside its install
/// folder). UDP game traffic has no remote address in Windows' tables, so those are not listed.
#[tauri::command]
async fn game_servers(state: State<'_, GamesState>, id: String) -> Result<Vec<GameServer>, String> {
    let Some(dir) = state.list.lock().unwrap().iter().find(|g| g.id == id).and_then(|g| g.install_dir.clone()) else {
        return Ok(Vec::new());
    };
    tauri::async_runtime::spawn_blocking(move || {
        let prefix = format!("{}\\", dir.trim_end_matches('\\').to_ascii_lowercase());
        let mut out: Vec<GameServer> = Vec::new();
        for c in netconns::connections() {
            let (Some(ip), true) = (c.remote_addr, c.exe_path.to_ascii_lowercase().starts_with(&prefix)) else {
                continue;
            };
            if !matches!(c.remote_scope, netconns::Scope::Public) || !matches!(c.state, Some(netconns::TcpState::Established)) {
                continue;
            }
            match out.iter_mut().find(|s| s.ip == ip.to_string() && s.port == c.remote_port) {
                Some(s) => s.connections += 1,
                None => out.push(GameServer { ip: ip.to_string(), port: c.remote_port, connections: 1 }),
            }
        }
        out.sort_by_key(|s| std::cmp::Reverse(s.connections));
        out.truncate(10);
        out
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn game_mode_status() -> Option<gamemode::Session> {
    gamemode::session()
}

/// End Game Mode now (everything restored); it stays off for this game until the game closes.
#[tauri::command]
fn game_mode_stop(app: AppHandle) -> Option<gamemode::Session> {
    let s = gamemode::stop()?;
    *app.state::<GamesState>().suppressed.lock().unwrap() = Some(s.game_id.clone());
    let _ = app.emit("gamemode:changed", None::<gamemode::Session>);
    Some(s)
}

/// One Game Mode step (routine thread, every 5 s). Returns true while a session is active.
fn game_mode_tick(app: &AppHandle, s: &Settings) -> bool {
    let current = gamemode::session();
    let enabled = games_cmds::mode_games(s, current.as_ref()); // + the game "Preparar e jogar" armed
    if enabled.is_empty() && current.is_none() {
        return false;
    }
    let gs = app.state::<GamesState>();
    let list = gs.current(s, false);
    let mut running = games::running(&list, &procs::list_paths(), games::steam_running_app());
    {
        let mut sup = gs.suppressed.lock().unwrap();
        if sup.as_ref().is_some_and(|id| !running.iter().any(|r| &r.id == id)) {
            *sup = None; // the game the user stopped by hand closed: next launch gets Game Mode again
        }
        if let Some(id) = sup.as_ref() {
            running.retain(|r| &r.id != id);
        }
    }
    // Turned off for that game while it runs → end the session too.
    let current_id = current.as_ref().map(|c| c.game_id.clone()).filter(|id| enabled.contains(id) || running.iter().all(|r| &r.id != id));
    let decision = if current.is_some() && current_id.is_none() {
        gamemode::Decision::Stop
    } else {
        gamemode::decide(current_id.as_deref(), &running, &enabled)
    };
    match decision {
        gamemode::Decision::Start(i) => {
            let rg = &running[i];
            if let Some(g) = list.iter().find(|g| g.id == rg.id) {
                // A game added by hand has no install folder: use the running exe's own folder, so
                // the priority check ("process inside the game's folder") still holds.
                let mut g = g.clone();
                if g.install_dir.is_none()
                    && let Some(pid) = rg.pid
                    && let Some((_, path)) = procs::list_paths().into_iter().find(|(p, _)| *p == pid)
                {
                    g.install_dir = std::path::Path::new(&path).parent().map(|d| d.to_string_lossy().into_owned());
                }
                let tweaks = s.game_tweaks.get(&g.id).copied().unwrap_or_default();
                let session = gamemode::start(&g, rg.pid, &s.game_mode, &tweaks);
                let _ = app.emit("gamemode:changed", Some(session));
                return true;
            }
            false
        }
        gamemode::Decision::Stop => {
            gamemode::stop();
            let _ = app.emit("gamemode:changed", None::<gamemode::Session>);
            false
        }
        gamemode::Decision::Keep => current.is_some(),
    }
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

/// Devices on this PC's network (sweep + neighbor table + mDNS + SSDP; no admin, no driver).
#[tauri::command]
async fn devices_scan() -> Result<devices::Scan, String> {
    tauri::async_runtime::spawn_blocking(devices::scan).await.map_err(|e| e.to_string())?.map_err(|e| e.to_string())
}

/// Risky-port check on one device of the last scan (the core refuses any other address).
#[tauri::command]
async fn devices_ports(ip: String) -> Result<Vec<devices::OpenPort>, String> {
    tauri::async_runtime::spawn_blocking(move || devices::check_ports(&ip))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn devices_known(mac: String, known: bool) -> Result<(), String> {
    devices::set_known(&mac, known).map_err(|e| e.to_string())
}

/// The Console G2 command list (names, aliases, usage) for help and completion.
#[tauri::command]
fn console_commands() -> &'static [console::Command] {
    console::COMMANDS
}

/// The user confirmed a console change (winget install/upgrade): the core checks the token.
#[tauri::command]
async fn console_confirm(token: u64, action: String, id: String) -> Result<console::Reply, String> {
    tauri::async_runtime::spawn_blocking(move || console::confirm(token, &action, &id)).await.map_err(|e| e.to_string())
}

/// Run one Console G2 line (native, read-only; the core parses and validates it).
#[tauri::command]
async fn console_run(line: String) -> Result<console::Reply, String> {
    tauri::async_runtime::spawn_blocking(move || console::run(&line)).await.map_err(|e| e.to_string())
}

/* ---------------- Luli history, memory ---------------- */

#[tauri::command]
fn luli_history() -> luli::History {
    luli::history()
}

#[tauri::command]
fn luli_get(id: String) -> Result<luli::Conversation, String> {
    luli::get(&id).map_err(|e| e.to_string())
}

#[tauri::command]
fn luli_save(conversation: luli::Conversation) -> Result<luli::Summary, String> {
    luli::save(conversation).map_err(|e| e.to_string())
}

#[tauri::command]
fn luli_rename(id: String, title: String) -> Result<(), String> {
    luli::rename(&id, &title).map_err(|e| e.to_string())
}

#[tauri::command]
fn luli_move(id: String, group: Option<String>) -> Result<(), String> {
    luli::set_group(&id, group.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
fn luli_delete(id: String) -> Result<(), String> {
    luli::delete(&id).map_err(|e| e.to_string())
}

#[tauri::command]
fn luli_clear() -> Result<(), String> {
    luli::clear().map_err(|e| e.to_string())
}

#[tauri::command]
fn luli_group_put(id: String, name: String) -> Result<Vec<luli::Group>, String> {
    luli::put_group(&id, &name).map_err(|e| e.to_string())
}

#[tauri::command]
fn luli_group_delete(id: String) -> Result<Vec<luli::Group>, String> {
    luli::delete_group(&id).map_err(|e| e.to_string())
}

#[tauri::command]
fn luli_memory() -> Vec<luli::MemoryItem> {
    luli::memory()
}

#[tauri::command]
fn luli_remember(text: String) -> Result<Vec<luli::MemoryItem>, String> {
    luli::remember(&text).map_err(|e| e.to_string())
}

#[tauri::command]
fn luli_forget(id: String) -> Result<Vec<luli::MemoryItem>, String> {
    luli::forget(&id).map_err(|e| e.to_string())
}

/// Settings come from the UI, get sanitized in the core, and only then replace the live copy.
#[tauri::command]
fn settings_set(r: State<'_, Routines>, settings: Settings) -> Result<Settings, String> {
    // Per-game options mirror the GPU preference in the registry: only `game_tweaks_set` changes
    // them (a page holding an older copy must not undo them). Picked exes: only `pick_exe` & co.
    // Profiles and the overlay: only their own commands.
    let live = r.settings.lock().unwrap().clone();
    let settings = Settings {
        game_tweaks: live.game_tweaks,
        game_paths: live.game_paths,
        game_exe_override: live.game_exe_override,
        profiles: live.profiles,
        overlay: live.overlay,
        ..settings
    };
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
            let playing = game_mode_tick(&app, &s);
            if !s.priority_rules.is_empty() {
                r.prio.lock().unwrap().tick(&s.priority_rules);
            }
            if s.game_ram_clean {
                let now = routines::detect_game(&s.game_exes);
                *r.game.lock().unwrap() = now.clone();
                if let Some(g) = watcher.observe(now)
                    && !gamemode::session().is_some_and(|x| x.applied.iter().any(|a| a == "ram"))
                {
                    on_game_started(&app, g); // Game Mode already purged for this game: once is enough
                }
            } else {
                *r.game.lock().unwrap() = None;
            }
            let paused = playing && s.game_mode.pause_routines;
            if tick.is_multiple_of(12) && !paused && !s.routine_categories.is_empty() && !r.running.load(Ordering::SeqCst) {
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
            // Exposure monitor: first pass ~10 s after start (records the baseline), then every 5 min.
            if (tick == 2 || tick.is_multiple_of(60)) && s.exposure_monitor && !paused {
                exposure_tick(&app);
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
    /// Used bytes of the volume when the root is a whole drive (0 = unknown): the percentage base.
    expected: u64,
    /// The root's direct children so far (live treemap).
    top: Vec<disk::LiveView>,
}

/// Used bytes of the drive when `root` is a drive root like `C:\` (a folder has no known total).
fn drive_used(root: &std::path::Path) -> u64 {
    let r = root.to_string_lossy();
    let r = r.trim_end_matches('\\');
    if r.len() != 2 || !r.ends_with(':') {
        return 0;
    }
    system::list_drives()
        .into_iter()
        .find(|d| d.letter.eq_ignore_ascii_case(r))
        .map_or(0, |d| d.total_bytes.saturating_sub(d.free_bytes))
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
        let expected = drive_used(&root);
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
                        expected,
                        top: disk::live_top(&progress, 60),
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
    blocking(move || disk::reveal(&p)).await
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
#[tauri::command]
async fn ping_host(
    host: String,
    count: u32,
    timeout_ms: u32,
    interval_ms: u32,
) -> Result<PingReport, String> {
    let count = count.clamp(1, 100);
    let timeout = timeout_ms.clamp(100, 5000);
    let interval = interval_ms.clamp(0, 2000);
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
    let max_hops = max_hops.clamp(1, 64) as u8;
    let probes = probes.clamp(1, 5) as u8;
    let timeout = timeout_ms.clamp(100, 5000);
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
    let count = count.clamp(1, 50);
    let timeout = timeout_ms.clamp(100, 5000);
    let interval = interval_ms.clamp(0, 2000);
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
    blocking(move || dns::apply(&guid, &provider)).await
}

#[tauri::command]
async fn dns_restore(guid: String) -> Result<(), String> {
    blocking(move || dns::restore(&guid)).await
}

/* ---------------- startup apps, processes, firmware, sensors ---------------- */

#[tauri::command]
async fn startup_list() -> Vec<startup::StartupItem> {
    tauri::async_runtime::spawn_blocking(startup::list).await.unwrap_or_default()
}

/// Toggle via `StartupApproved` (Task Manager semantics); the item must exist in the live list.
#[tauri::command]
async fn startup_set(location: startup::Location, name: String, enabled: bool) -> Result<startup::StartupItem, String> {
    blocking(move || startup::set_enabled(location, &name, enabled)).await
}

/* ---------------- power plans ---------------- */

#[tauri::command]
async fn power_plans() -> power::Plans {
    tauri::async_runtime::spawn_blocking(power::plans).await.unwrap_or(power::Plans { plans: vec![], has_battery: false })
}

#[tauri::command]
async fn power_activate(guid: String) -> Result<(), String> {
    blocking(move || power::set_active(&guid)).await
}

#[tauri::command]
async fn power_create_max() -> Result<String, String> {
    blocking(power::create_max_performance).await
}

#[tauri::command]
async fn power_duplicate(guid: String, name: String) -> Result<String, String> {
    blocking(move || power::duplicate(&guid, &name)).await
}

#[tauri::command]
async fn power_rename(guid: String, name: String) -> Result<(), String> {
    blocking(move || power::rename(&guid, &name)).await
}

#[tauri::command]
async fn power_delete(guid: String) -> Result<(), String> {
    blocking(move || power::delete(&guid)).await
}

#[tauri::command]
async fn power_settings(guid: String) -> Result<Vec<power::SettingValue>, String> {
    blocking(move || power::settings(&guid)).await
}

#[tauri::command]
async fn power_write(guid: String, id: String, ac: Option<u32>, dc: Option<u32>) -> Result<power::SettingValue, String> {
    blocking(move || power::write(&guid, &id, ac, dc)).await
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
async fn apps_leftovers(state: State<'_, AppsState>, r: State<'_, Routines>, id: String) -> Result<Vec<apps::Leftover>, String> {
    if id == apps::SWEEP_ID {
        // Folder-first sweep on every fixed drive, against the installed list (loaded if needed).
        let mut installed = state.apps.lock().unwrap().clone();
        let protected = r.settings.lock().unwrap().protected_apps.clone();
        let items = tauri::async_runtime::spawn_blocking(move || {
            if installed.is_empty() {
                installed = apps::list(&protected);
            }
            apps::unowned(&installed)
        })
        .await
        .map_err(|e| e.to_string())?;
        *state.pending.lock().unwrap() = Some(apps::Pending { app: apps::sweep_app(), items: items.clone() });
        return Ok(items);
    }
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
        if app.id == apps::SWEEP_ID {
            return Ok(apps::remove_unowned(&items, apps::SWEEP_ID));
        }
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
    blocking(move || apps::uninstall(&app)).await
}

/// Other installed apps the installer of `id` added (Parsec → its display and USB drivers).
#[tauri::command]
fn apps_companions(state: State<'_, AppsState>, id: String) -> Vec<apps::App> {
    let Some(app) = state.find(&id) else {
        return Vec::new();
    };
    let installed = state.apps.lock().unwrap().clone();
    apps::companions(&app, &installed).into_iter().filter(apps::installed).collect()
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
    blocking(move || procs::end(pid, created)).await
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

/* ---------------- system files (Disk Cleanup handlers) + repair (DISM / SFC / MRT) ---------------- */

#[derive(serde::Serialize, Clone)]
struct RepairProgress {
    tool: repair::Tool,
    percent: Option<u32>,
}

#[tauri::command]
async fn volcache_preview(app: AppHandle, ids: Vec<String>) -> Result<Vec<volcache::VcItem>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        volcache::preview(&ids, |done, total| {
            let _ = app.emit("volcache:progress", CleanupProgress { done, total });
        })
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Same single-use token store as the catalog; the `sys_*` ids never overlap catalog ids.
#[tauri::command]
fn volcache_confirm(tokens: State<'_, TokenStore>, ids: Vec<String>) -> Result<ConfirmToken, String> {
    if !volcache::valid_selection(&ids) {
        return Err("bad_selection".into());
    }
    Ok(tokens.issue(ids))
}

#[tauri::command]
async fn volcache_execute(app: AppHandle, tokens: State<'_, TokenStore>, token: u64, ids: Vec<String>) -> Result<RunRecord, String> {
    if !tokens.consume(token, &ids) {
        return Err("bad_token".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        volcache::execute(&ids, "manual", |done, total| {
            let _ = app.emit("volcache:progress", CleanupProgress { done, total });
        })
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// One repair tool, run to the end (the UI confirmed first). Admin only; one job at a time.
#[tauri::command]
async fn repair_run(app: AppHandle, tool: repair::Tool) -> Result<repair::Outcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        repair::run(tool, &|percent| {
            let _ = app.emit("maintenance:progress", RepairProgress { tool, percent });
        })
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn repair_cancel() {
    repair::cancel();
}

#[tauri::command]
fn repair_status() -> repair::Status {
    repair::status()
}

#[tauri::command]
fn repair_history() -> Vec<repair::Outcome> {
    repair::history()
}

/* ---------------- BIOS guide + per-game tweaks (round 7.1) ---------------- */

/// What to change in this PC's BIOS, with the menu paths of its motherboard maker.
#[tauri::command]
async fn bios_guide(state: State<'_, Dashboard>) -> Result<firmware::Guide, String> {
    let cached = state.profile.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let p = cached.unwrap_or_else(hardware::profile);
        firmware::guide(&p, &firmware::read(&p))
    })
    .await
    .map_err(|e| e.to_string())
}

/// Restart into the firmware setup (UEFI `OsIndications`). Only after the UI's confirmation;
/// admin-only (`needs_admin`), `unsupported` on legacy BIOS.
#[tauri::command]
async fn bios_reboot() -> Result<(), String> {
    blocking(firmware::reboot_to_firmware).await
}

#[derive(serde::Serialize)]
struct GameTweaksInfo {
    cpu: g2boost_core::cpusets::Summary,
    gpus: usize,
}

/// What the per-game options can do here (SMT, hybrid cores, how many GPUs).
#[tauri::command]
async fn game_tweaks_info(state: State<'_, Dashboard>) -> Result<GameTweaksInfo, String> {
    let cached = state.profile.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || GameTweaksInfo {
        cpu: g2boost_core::cpusets::summary(&g2boost_core::cpusets::topology()),
        gpus: cached.unwrap_or_else(hardware::profile).gpus.len(),
    })
    .await
    .map_err(|e| e.to_string())
}

/// Save one game's options. The game must be in the installed list; turning the GPU option on or
/// off writes Windows' graphics preference for its validated main exe (with backup).
#[tauri::command]
async fn game_tweaks_set(app: AppHandle, id: String, tweaks: gamemode::GameTweaks) -> Result<Settings, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let live = app.state::<Routines>().settings.lock().unwrap().clone();
        let game = app.state::<GamesState>().current(&live, false).into_iter().find(|g| g.id == id).ok_or("unknown_game")?;
        let r = app.state::<Routines>();
        let mut s = r.settings.lock().unwrap();
        let before = s.game_tweaks.get(&id).copied().unwrap_or_default();
        if tweaks.gpu_high_perf != before.gpu_high_perf {
            match gamemode::gpu_exe(&game) {
                Some(exe) => gamemode::set_gpu_high_perf(&exe, tweaks.gpu_high_perf).map_err(|e| e.to_string())?,
                None if tweaks.gpu_high_perf => return Err("no_exe".to_string()),
                None => {} // ponytail: exe gone (uninstalled/moved): nothing left to undo by name
            }
        }
        let mut next = s.clone();
        next.game_tweaks.insert(id, tweaks);
        let saved = settings::save(&next).map_err(|e| e.to_string())?;
        *s = saved.clone();
        Ok(saved)
    })
    .await
    .map_err(|e| e.to_string())?
}

/* ---------------- GPU driver settings ---------------- */

#[tauri::command]
async fn gpu_info() -> Result<g2boost_core::gpu::Overview, String> {
    tauri::async_runtime::spawn_blocking(g2boost_core::gpu::overview).await.map_err(|e| e.to_string())
}

/// `game` = an id from `games_list` (its main exe picks the driver profile); `None` = global.
/// Only games G2 Boost listed can get a profile, never an arbitrary name from the webview.
fn gpu_target(app: &AppHandle, game: Option<String>) -> Result<nvapi::Target, String> {
    let Some(id) = game else {
        return Ok(nvapi::Target::Global);
    };
    let live = app.state::<Routines>().settings.lock().unwrap().clone();
    let g = app.state::<GamesState>().current(&live, false).into_iter().find(|g| g.id == id).ok_or("unknown_game")?;
    let exe = match &g.main_exe {
        Some(p) => std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()),
        None => id.strip_prefix("exe:").map(str::to_string),
    }
    .ok_or("no_exe")?;
    nvapi::Target::game(&exe).map_err(|e| e.to_string())
}

#[tauri::command]
async fn gpu_nv_read(app: AppHandle, game: Option<String>) -> Result<nvapi::ProfileView, String> {
    blocking(move || nvapi::read(&gpu_target(&app, game)?).map_err(|e| e.to_string())).await
}

#[tauri::command]
async fn gpu_nv_apply(app: AppHandle, game: Option<String>, id: String, value: String) -> Result<nvapi::ProfileView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        nvapi::apply(&gpu_target(&app, game)?, &id, &value).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn gpu_nv_revert(app: AppHandle, game: Option<String>, id: String) -> Result<nvapi::ProfileView, String> {
    blocking(move || nvapi::revert(&gpu_target(&app, game)?, &id).map_err(|e| e.to_string())).await
}

#[tauri::command]
async fn gpu_nv_recommended(app: AppHandle, game: Option<String>) -> Result<nvapi::ProfileView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        nvapi::apply_recommended(&gpu_target(&app, game)?).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// AMD (beta): every ADLX call runs in a `--gpu-helper` child; a crash there is an error here.
#[tauri::command]
async fn gpu_amd_read() -> Result<Vec<adlx::AmdGpu>, String> {
    blocking(|| adlx::call(&adlx::Request::read())).await
}

#[tauri::command]
async fn gpu_amd_set(index: u32, changes: Vec<adlx::Change>) -> Result<Vec<adlx::AmdGpu>, String> {
    let req = adlx::Request { cmd: adlx::Cmd::Set, gpu: index, changes };
    blocking(move || adlx::call(&req)).await
}

/* ---------------- exposure (Segurança) ---------------- */

#[derive(Default)]
struct ExposureState {
    /// New things the monitor saw since the user last looked (badge on the sidebar).
    alerts: Mutex<Vec<exposure::Alert>>,
    running: AtomicBool,
}

/// One monitor pass on its own thread (the router may take seconds); never two at once.
fn exposure_tick(app: &AppHandle) {
    if app.state::<ExposureState>().running.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let fresh = exposure::monitor_tick();
        let st = app.state::<ExposureState>();
        st.running.store(false, Ordering::SeqCst);
        if fresh.is_empty() {
            return;
        }
        let mut alerts = st.alerts.lock().unwrap();
        alerts.extend(fresh);
        let skip = alerts.len().saturating_sub(50);
        alerts.drain(..skip);
        let _ = app.emit("exposure:alerts", alerts.clone());
    });
}

#[tauri::command]
async fn exposure_report() -> Result<exposure::Report, String> {
    tauri::async_runtime::spawn_blocking(exposure::report).await.map_err(|e| e.to_string())
}

/// The fix comes from the UI (hostile): the core re-validates it against the live state.
#[tauri::command]
async fn exposure_fix(fix: exposure::Fix) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || exposure::apply(&fix).map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn exposure_revert(fix: exposure::Fix) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || exposure::revert(&fix).map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
}

/// Only paths the last report showed.
#[tauri::command]
async fn exposure_reveal(path: String) -> Result<(), String> {
    if path.len() > 1024 {
        return Err("not_applicable".into());
    }
    tauri::async_runtime::spawn_blocking(move || exposure::reveal(&path).map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn exposure_alerts(state: State<'_, ExposureState>) -> Vec<exposure::Alert> {
    state.alerts.lock().unwrap().clone()
}

#[tauri::command]
fn exposure_alerts_clear(app: AppHandle, state: State<'_, ExposureState>) {
    state.alerts.lock().unwrap().clear();
    let _ = app.emit("exposure:alerts", Vec::<exposure::Alert>::new());
}

/* ---------------- updates (winget apps + Windows Update) ---------------- */

/// Apps with a newer version in winget, classified against the user's exclusions.
#[tauri::command]
async fn updates_apps(r: State<'_, Routines>) -> Result<Vec<updates::AppUpdate>, String> {
    let excluded = r.settings.lock().unwrap().update_excluded.clone();
    tauri::async_runtime::spawn_blocking(move || updates::app_updates(&excluded))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Leave a package out of "update all" (or put it back); saved in settings.
#[tauri::command]
fn updates_exclude(r: State<'_, Routines>, id: String, exclude: bool) -> Result<Vec<String>, String> {
    let mut s = r.settings.lock().unwrap().clone();
    s.update_excluded = updates::exclude(&s.update_excluded, &id, exclude).map_err(|e| e.to_string())?;
    let saved = settings::save(&s).map_err(|e| e.to_string())?;
    let list = saved.update_excluded.clone();
    *r.settings.lock().unwrap() = saved;
    Ok(list)
}

/// Issued after the UI's confirmation: bound to the exact, ordered package list.
#[tauri::command]
fn updates_apps_confirm(ids: Vec<String>) -> Result<ConfirmToken, String> {
    updates::apps_confirm(&ids).map_err(|e| e.to_string())
}

#[tauri::command]
async fn updates_apps_run(app: AppHandle, token: u64, ids: Vec<String>) -> Result<updates::Batch, String> {
    tauri::async_runtime::spawn_blocking(move || {
        updates::apps_run(token, &ids, |p| {
            let _ = app.emit("updates:apps", p);
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Stop after the package being upgraded now.
#[tauri::command]
fn updates_apps_cancel() {
    updates::apps_cancel();
}

/// Pending Windows updates; `online` asks Microsoft now, otherwise Windows' last check.
#[tauri::command]
async fn updates_wu_search(online: bool) -> Result<updates::WuScan, String> {
    tauri::async_runtime::spawn_blocking(move || updates::wu_search(online))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Cancel the Windows Update search, download or install in progress.
#[tauri::command]
fn updates_wu_cancel() {
    updates::wu_cancel();
}

#[tauri::command]
fn updates_wu_confirm(ids: Vec<String>) -> Result<ConfirmToken, String> {
    updates::wu_confirm(&ids).map_err(|e| e.to_string())
}

#[tauri::command]
async fn updates_wu_install(app: AppHandle, token: u64, ids: Vec<String>) -> Result<updates::Batch, String> {
    tauri::async_runtime::spawn_blocking(move || {
        updates::wu_install(token, &ids, move |p| {
            let _ = app.emit("updates:wu", p);
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Windows' update history and whether a restart is pending.
#[tauri::command]
async fn updates_wu_status() -> Result<updates::WuStatus, String> {
    tauri::async_runtime::spawn_blocking(|| updates::wu_status(30))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// G2 Boost's own runs (apps and Windows), oldest first.
#[tauri::command]
fn updates_history() -> Vec<updates::Batch> {
    updates::history()
}

/// Restart now (after the UI's confirmation), only when a restart is pending.
#[tauri::command]
async fn updates_reboot() -> Result<(), String> {
    blocking(updates::restart).await
}

/* ---------------- Manutenção+: disk check, optimize drives, boot options, connection repair ---------------- */

use g2boost_core::{netrepair, volumes};

#[derive(serde::Serialize, Clone)]
struct VolumeProgress {
    op: volumes::Op,
    volume: String,
    percent: Option<u32>,
}

#[tauri::command]
async fn volumes_list() -> Result<Vec<volumes::Volume>, String> {
    tauri::async_runtime::spawn_blocking(|| volumes::list().map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
}

/// One scan / spot fix / analyze / retrim / defrag, run to the end (the UI confirmed first).
/// The core re-validates `volume` and `op` against the live list. Stop = `repair_cancel`.
#[tauri::command]
async fn volumes_run(app: AppHandle, op: volumes::Op, volume: String) -> Result<volumes::Outcome, String> {
    if volume.len() > 64 {
        return Err("unknown_volume".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let id = volume.clone();
        volumes::run(op, &volume, &|percent| {
            let _ = app.emit("maintenance:volume-progress", VolumeProgress { op, volume: id.clone(), percent });
        })
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn volumes_status() -> Option<volumes::Running> {
    volumes::status()
}

#[tauri::command]
fn volumes_history() -> Vec<volumes::Outcome> {
    volumes::history()
}

/// Restart into the advanced startup options (Safe Mode, Startup Repair...). UI confirms first.
#[tauri::command]
async fn boot_options_reboot() -> Result<(), String> {
    blocking(firmware::reboot_to_boot_options).await
}

#[tauri::command]
async fn net_dhcp_adapters() -> Vec<netrepair::DhcpAdapter> {
    tauri::async_runtime::spawn_blocking(netrepair::adapters).await.unwrap_or_default()
}

/// Release + renew one adapter's lease (index from `net_dhcp_adapters`, re-checked in the core).
#[tauri::command]
async fn net_renew(index: u32) -> Result<Option<std::net::Ipv4Addr>, String> {
    blocking(move || netrepair::renew(index)).await
}

#[tauri::command]
async fn net_flush_dns() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| netrepair::flush_dns().map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn net_open_reset() -> Result<(), String> {
    netrepair::open_network_reset().map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // `G2 Boost.exe --gpu-helper <json>`: one ADLX request in a throwaway process (crash isolation).
    if adlx::helper_if_requested() {
        return;
    }
    // `G2 Boost.exe --mcp`: stdio MCP server for Claude Desktop / Claude Code, no window.
    // One signed binary instead of a sidecar; the server is read-only (crates/mcp).
    if std::env::args().any(|a| a == "--mcp") {
        if let Err(e) = tauri::async_runtime::block_on(g2boost_mcp::serve()) {
            eprintln!("g2boost mcp: {e}");
        }
        return;
    }
    // "Sempre abrir como administrador": the UI asks (AppInfo.ask_admin → admin gate screen, then
    // `relaunch_elevated`), so the UAC prompt dims over G2 Boost's own explanation. The logon task
    // (`--tray`) never prompts; declining keeps this unelevated copy (no prompt loop).
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_http::init())
        .manage(DiskState::default())
        .manage(ProcSampler::default())
        .manage(AppsState::default())
        .manage(TokenStore::default())
        .manage(Dashboard::default())
        .manage(GamesState::default())
        .register_asynchronous_uri_scheme_protocol("cover", games_cmds::cover_protocol) // r9:games covers
        .manage(DupState::default())
        .manage(IconCache::default())
        .manage(NetEtwState::default())
        .manage(ExposureState::default())
        .manage(Routines {
            settings: Mutex::new(settings::load()),
            state: Mutex::new(routines::load_state()),
            game: Mutex::new(None),
            ran_since_start: AtomicBool::new(false),
            running: AtomicBool::new(false),
            prio: Mutex::new(prio::Enforcer::default()),
        })
        .setup(|app| {
            // A Game Mode session left by a crash or a forced exit: put the power plan back first.
            gamemode::stop();
            if system::is_elevated() {
                let _ = qos::clear_all_ours(); // orphaned QoS policies (non-elevated: a reboot clears them)
            }
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
            // r9:overlay: overlay state + its global hotkey (when on); also stops an FPS session a crash left.
            overlay_cmds::init(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let r = window.state::<Routines>();
                let s = r.settings.lock().unwrap();
                if s.close_to_tray && (s.schedule != Schedule::Off || s.game_ram_clean || !s.game_mode_games.is_empty()) {
                    api.prevent_close();
                    let _ = window.hide();
                }
                // r9:overlay: with the overlay on, closing hides to the tray too (its hotkey keeps working).
                if s.close_to_tray && s.overlay.enabled && window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            // r9:overlay: the main window is gone for good: take the overlay with it so the app exits.
            if let WindowEvent::Destroyed = event
                && window.label() == "main"
            {
                overlay_cmds::shutdown(window.app_handle());
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
            apps_companions,
            luli_history,
            luli_get,
            luli_save,
            luli_rename,
            luli_move,
            luli_delete,
            luli_clear,
            luli_group_put,
            luli_group_delete,
            luli_memory,
            luli_remember,
            luli_forget,
            apps_resolve,
            apps_protect,
            apps_undo_status,
            apps_undo,
            apps_icon,
            claude_targets,
            claude_connect,
            claude_disconnect,
            console_commands,
            console_run,
            console_confirm,
            devices_scan,
            devices_ports,
            devices_known,
            games_list,
            games_icon,
            game_mode_status,
            game_mode_stop,
            game_servers,
            net_usage,
            net_usage_stop,
            exe_icon,
            wifi_report,
            disk_health,
            privacy_access,
            apps_usage,
            boot_report,
            dupes_start,
            dupes_cancel,
            dupes_recycle,
            volcache_preview,
            volcache_confirm,
            volcache_execute,
            repair_run,
            repair_cancel,
            repair_status,
            repair_history,
            bios_guide,
            bios_reboot,
            game_tweaks_info,
            game_tweaks_set,
            gpu_info,
            gpu_nv_read,
            gpu_nv_apply,
            gpu_nv_revert,
            gpu_nv_recommended,
            gpu_amd_read,
            gpu_amd_set,
            exposure_report,
            exposure_fix,
            exposure_revert,
            exposure_reveal,
            exposure_alerts,
            exposure_alerts_clear,
            updates_apps,
            updates_exclude,
            updates_apps_confirm,
            updates_apps_run,
            updates_apps_cancel,
            updates_wu_search,
            updates_wu_cancel,
            updates_wu_confirm,
            updates_wu_install,
            updates_wu_status,
            updates_history,
            updates_reboot,
            volumes_list,
            volumes_run,
            volumes_status,
            volumes_history,
            boot_options_reboot,
            net_dhcp_adapters,
            net_renew,
            net_flush_dns,
            net_open_reset,
            // r9:games
            games_cmds::game_launch,
            games_cmds::pick_exe,
            games_cmds::game_exe_reset,
            games_cmds::games_remove,

            // r9:tweaks
            tweaks_cmds::tweak_changes,
            tweaks_cmds::debloat_apps,
            tweaks_cmds::debloat_apps_confirm,
            tweaks_cmds::debloat_apps_remove,

            // r9:maint
            maint_cmds::maint_plan_run,
            maint_cmds::maint_plan_status,
            maint_cmds::maint_plan_history,
            maint_cmds::maint_print_status,
            maint_cmds::maint_wu_backups,
            maint_cmds::maint_fix_run,
            maint_cmds::maint_fix_history,
            maint_cmds::av_status,
            maint_cmds::av_exclude,
            maint_cmds::av_unexclude,
            maint_cmds::av_reveal_exe,

            // r9:hw
            hw_cmds::hw_start,
            hw_cmds::hw_stop,
            hw_cmds::hw_export_csv,
            hw_cmds::checkup_run,

            // r9:install
            install_cmds::install_catalog,
            install_cmds::install_essentials,
            install_cmds::install_status,
            install_cmds::install_confirm,
            install_cmds::install_run,
            install_cmds::install_cancel,
            install_cmds::install_history,

            // r9:tools
            tools_cmds::packetloss_run,
            tools_cmds::packetloss_cancel,
            tools_cmds::packetloss_export,

            // r9:overlay
            overlay_cmds::overlay_status,
            overlay_cmds::overlay_set,
            overlay_cmds::overlay_test,

            // r9:profiles
            profiles_cmds::profiles_list,
            profiles_cmds::profiles_save,
            profiles_cmds::profiles_delete,
            profiles_cmds::profiles_plan,
            profiles_cmds::profiles_step,
            profiles_cmds::profiles_restoration,
            profiles_cmds::profiles_history,
            profiles_cmds::profiles_export,
            profiles_cmds::profiles_files,
            profiles_cmds::profiles_open_folder,
            profiles_cmds::profiles_import,
            profiles_cmds::periph_state,
            profiles_cmds::periph_set,
            profiles_cmds::periph_revert,


            // r9:setup
            update_cmds::update_check,
            update_cmds::update_install,

        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // Quitting from the tray in the middle of a game: undo Game Mode on the way out.
            if let tauri::RunEvent::Exit = event {
                gamemode::stop();
                // r9:overlay: release the hotkey and close the FPS session on the way out.
                overlay_cmds::shutdown(_app);
            }
        });
}
