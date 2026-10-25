//! Round-9 tools block (Docs/core/tools.md): thin IPC over `g2boost_core::pktloss`.
//! The mouse and monitor tests are pure front end and have no command here.

use g2boost_core::pktloss::{self, Profile, Report};
use tauri::{AppHandle, Emitter};

/// One burst of ICMP probes, to the end or until `packetloss_cancel`. Every finished probe is
/// an event (`packetloss:probe`); the report comes back when the run ends. The core validates
/// `host`, and `profile` is a closed word (`quick`, `standard`, `long`), so a hostile caller
/// cannot choose a packet rate.
#[tauri::command]
pub async fn packetloss_run(app: AppHandle, host: String, profile: Profile) -> Result<Report, String> {
    crate::blocking(move || {
        pktloss::run(&host, profile, &mut |ev| {
            let _ = app.emit("packetloss:probe", ev);
        })
    })
    .await
}

#[tauri::command]
pub fn packetloss_cancel() {
    pktloss::cancel();
}

/// Save the last run as CSV in Documents and show it in Explorer. Takes no data: the file is
/// what the core measured. Returns the file name.
#[tauri::command]
pub async fn packetloss_export(locale: String) -> Result<String, String> {
    crate::blocking(move || pktloss::export_csv(&locale)).await
}
