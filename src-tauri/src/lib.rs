//! Tauri shell: thin IPC commands over `g2boost_core`. No business logic here.

use std::net::Ipv4Addr;

use g2boost_core::network::{self, Hop, LatencyStats, Probe};
use g2boost_core::system::{self, DriveInfo, MemoryStatus};

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
                std::thread::sleep(std::time::Duration::from_millis(u64::from(interval)));
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
        .invoke_handler(tauri::generate_handler![
            app_info,
            list_drives,
            memory_status,
            ping_host,
            trace_host
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
