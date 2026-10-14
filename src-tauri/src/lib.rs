//! Tauri shell: thin IPC commands over `g2boost_core`. No business logic here.

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use g2boost_core::disk::{self, BigFile, Node, NodeView, ScanProgress, ScanSummary};
use g2boost_core::network::{self, Hop, LatencyStats, Probe};
use g2boost_core::system::{self, DriveInfo, MemoryStatus};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(serde::Serialize)]
struct AppInfo {
    version: &'static str,
    core_version: &'static str,
    os: String,
    elevated: bool,
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
        start_route: std::env::var("G2BOOST_START_ROUTE").ok(),
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
        .invoke_handler(tauri::generate_handler![
            app_info,
            list_drives,
            memory_status,
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
