//! Commands of the hardware monitor page and the Meu PC check-up. The sampler's state lives in
//! statics, so lib.rs only declares this module and lists the commands.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use g2boost_core::hwsensors::Sampler;
use g2boost_core::{checkup, hardware, hwcsv};
use tauri::{AppHandle, Emitter, State};

/// Pages listening to `hw:tick`; the sampling thread lives while there is at least one.
static SUBS: AtomicU32 = AtomicU32::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);

/// If the sampling thread panics, the next page that opens must be able to start a new one.
struct Unwind;

impl Drop for Unwind {
    fn drop(&mut self) {
        if std::thread::panicking() {
            RUNNING.store(false, Ordering::SeqCst);
        }
    }
}

/// Reference-counted like `monitor_start`: one snapshot of the sensor tree per second to
/// `hw:tick` while a page listens.
#[tauri::command]
pub fn hw_start(app: AppHandle) {
    SUBS.fetch_add(1, Ordering::SeqCst);
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let _unwind = Unwind;
        loop {
            let mut sampler = Sampler::new();
            // Fixed 1 s cadence: sleep to the next deadline, not 1 s after a slow sample.
            let mut next = Instant::now();
            while SUBS.load(Ordering::SeqCst) > 0 {
                next += Duration::from_secs(1);
                let now = Instant::now();
                match next.checked_duration_since(now) {
                    Some(wait) => std::thread::sleep(wait),
                    None => next = now, // fell behind (machine stalled): restart the cadence
                }
                if SUBS.load(Ordering::SeqCst) == 0 {
                    break;
                }
                let _ = app.emit("hw:tick", sampler.sample());
            }
            RUNNING.store(false, Ordering::SeqCst);
            // A page that subscribed while this thread was stopping must not be left without one.
            if SUBS.load(Ordering::SeqCst) == 0 || RUNNING.swap(true, Ordering::SeqCst) {
                return;
            }
        }
    });
}

#[tauri::command]
pub fn hw_stop() {
    let _ = SUBS.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
}

/// Write the rows the page shows (value, minimum, maximum) to `Documents` and reveal the file.
/// The core bounds and sanitises everything; the path is ours.
#[tauri::command]
pub async fn hw_export_csv(request: hwcsv::Request) -> Result<String, String> {
    let path = super::blocking(move || hwcsv::save(&request)).await?;
    let _ = tauri_plugin_opener::reveal_item_in_dir(&path);
    Ok(path.to_string_lossy().into_owned())
}

/// The check-up (about 3 s: WMI, event logs, Windows Update history, network). Read-only. Reuses
/// the hardware profile the Meu PC page already cached.
#[tauri::command]
pub async fn checkup_run(state: State<'_, super::Dashboard>) -> Result<checkup::Report, String> {
    let cached = state.profile.lock().unwrap().clone();
    let (report, profile) = tauri::async_runtime::spawn_blocking(move || {
        let p = cached.unwrap_or_else(hardware::profile);
        (checkup::run(&p), p)
    })
    .await
    .map_err(|e| e.to_string())?;
    state.profile.lock().unwrap().get_or_insert(profile);
    Ok(report)
}
