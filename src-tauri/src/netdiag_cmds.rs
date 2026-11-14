//! Commands of Rede › Diagnóstico ("my internet is slow"): what this PC adds to the picture and
//! the fixes that run here. The speed test runs in the webview; Wi-Fi, devices, DNS ranking and
//! per-app usage reuse their own commands.

use g2boost_core::{dns, nicpower, system};

#[derive(serde::Serialize)]
pub struct PcView {
    /// The adapter internet traffic goes through (`None` = offline).
    adapter: Option<dns::Adapter>,
    /// Descriptions of the VPN / tunnel adapters that are up.
    vpns: Vec<String>,
    nic_power: Option<nicpower::NicPower>,
    /// A NIC power backup exists (the Undo button).
    nic_backup: bool,
    elevated: bool,
}

#[tauri::command]
pub async fn netdiag_pc() -> Result<PcView, String> {
    super::blocking(|| -> Result<PcView, String> {
        let all = dns::adapters();
        let adapter = all.iter().find(|a| a.primary).cloned();
        let nic_power = adapter.as_ref().and_then(|a| nicpower::for_adapter(&a.guid));
        let vpns = all.iter().filter(|a| a.vpn).map(|a| a.description.clone()).collect();
        Ok(PcView { adapter, vpns, nic_power, nic_backup: nicpower::has_backup(), elevated: system::is_elevated() })
    })
    .await
}

/// The primary adapter only: the UI never names one.
#[tauri::command]
pub async fn nic_power_off() -> Result<nicpower::NicPower, String> {
    super::blocking(|| {
        let a = dns::adapters().into_iter().find(|a| a.primary).ok_or(nicpower::NicError::NotFound)?;
        nicpower::turn_off(&a.guid)
    })
    .await
}

#[tauri::command]
pub async fn nic_power_restore() -> Result<(), String> {
    super::blocking(nicpower::restore).await
}

/// Open the router's admin page: `http://<gateway>/` of the primary adapter, a private IPv4
/// only (the caller passes nothing). The user types the router password there, not here.
#[tauri::command]
pub fn router_open() -> Result<String, String> {
    let gw = dns::adapters().into_iter().find(|a| a.primary).and_then(|a| a.gateway).filter(|ip| ip.is_private()).ok_or("no_gateway")?;
    tauri_plugin_opener::open_url(format!("http://{gw}/"), None::<&str>).map_err(|e| e.to_string())?;
    Ok(gw.to_string())
}
