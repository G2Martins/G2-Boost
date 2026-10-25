//! In-game overlay (r9:overlay): the `overlay` window, its global hotkey and the worker that feeds
//! it twice a second. Rules and Win32 live in `g2boost_core::{overlay, fps}`; this wires them to
//! Tauri. Docs/boost/overlay.md.
//!
//! Window: built hidden on first show (transparent, undecorated, not focusable, out of the
//! taskbar), then shown, moved and hidden only through `overlay::show`/`hide` on the main thread:
//! they never activate it, so the game keeps focus (Tauri's own `show()` would activate). It is
//! destroyed when the overlay is turned off, when the main window goes, and at exit. The ETW
//! session belongs to the worker, so it only exists while the overlay is on screen.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

use g2boost_core::monitor::{Monitor, Sample};
use g2boost_core::overlay::{self, Hotkey, OverlaySettings, Target};
use g2boost_core::settings::{self, Settings};
use g2boost_core::{fps, games, system};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const LABEL: &str = "overlay";

#[derive(Default)]
pub struct OverlayState {
    /// The registered hotkey and the spec it was registered with.
    hotkey: Mutex<Option<(String, Hotkey)>>,
    hotkey_error: Mutex<Option<String>>,
    /// Feeding thread while the overlay is on screen; dropping the sender stops it.
    worker: Mutex<Option<(mpsc::Sender<()>, JoinHandle<()>)>>,
    /// One show/hide at a time (a hotkey press and the test button can race).
    busy: Mutex<()>,
    /// The app is closing: nothing is shown again.
    closing: AtomicBool,
}

#[derive(serde::Serialize)]
pub struct OverlayStatus {
    settings: OverlaySettings,
    visible: bool,
    /// `hotkey_taken`, `invalid_hotkey` or `hotkey_error <code>` when registering failed.
    hotkey_error: Option<String>,
    elevated: bool,
}

/// What the overlay window draws (event `overlay:frame`, every 500 ms while it is on screen).
#[derive(serde::Serialize, Clone)]
struct Frame {
    config: OverlaySettings,
    game: Option<String>,
    fps: Option<fps::Stats>,
    /// `ok`, `needs_admin` or `error`.
    fps_state: &'static str,
    sys: Option<Sample>,
}

fn current(app: &AppHandle) -> OverlaySettings {
    app.state::<super::Routines>().settings.lock().unwrap().overlay.clone()
}

fn status(app: &AppHandle) -> OverlayStatus {
    let st = app.state::<OverlayState>();
    OverlayStatus {
        settings: current(app),
        visible: st.worker.lock().unwrap().is_some(),
        hotkey_error: st.hotkey_error.lock().unwrap().clone(),
        elevated: system::is_elevated(),
    }
}

#[tauri::command]
pub fn overlay_status(app: AppHandle) -> OverlayStatus {
    status(&app)
}

/// Save the overlay settings (validated by the core) and apply them. Turning the overlay off
/// releases the hotkey, the window and the FPS session.
#[tauri::command]
pub async fn overlay_set(app: AppHandle, config: OverlaySettings) -> Result<OverlayStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (was, now) = {
            let r = app.state::<super::Routines>();
            let mut cur = r.settings.lock().unwrap();
            let was = cur.overlay.enabled;
            *cur = settings::save(&Settings { overlay: config, ..cur.clone() }).map_err(|e| e.to_string())?;
            (was, cur.overlay.clone())
        };
        apply_hotkey(&app, &now);
        if was && !now.enabled {
            set_visible(&app, Some(false))?;
        }
        Ok(status(&app))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// "Testar agora": show or hide the overlay right away, over whatever is on screen.
#[tauri::command]
pub async fn overlay_test(app: AppHandle, on: bool) -> Result<OverlayStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        set_visible(&app, Some(on))?;
        Ok(status(&app))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// App start (from `setup`): state, an FPS session a crash left behind, the hotkey when on.
pub fn init(app: &AppHandle) {
    app.manage(OverlayState::default());
    if system::is_elevated() {
        fps::stop_stale();
    }
    apply_hotkey(app, &current(app));
}

/// The main window is gone or the app exits: hotkey released, worker (and its ETW session)
/// stopped, window destroyed. Never waits on the main thread (it runs there).
pub fn shutdown(app: &AppHandle) {
    let Some(st) = app.try_state::<OverlayState>() else { return };
    st.closing.store(true, Ordering::SeqCst);
    *st.hotkey.lock().unwrap() = None;
    stop_worker(&st);
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.destroy();
    }
}

/// Register, change or release the hotkey to match `cfg`.
fn apply_hotkey(app: &AppHandle, cfg: &OverlaySettings) {
    let st = app.state::<OverlayState>();
    let mut hk = st.hotkey.lock().unwrap();
    let wanted = cfg.enabled.then(|| cfg.hotkey.clone());
    if hk.as_ref().map(|(spec, _)| spec) == wanted.as_ref() {
        return;
    }
    *hk = None; // unregister first: the same key may come right back
    *st.hotkey_error.lock().unwrap() = None;
    let Some(spec) = wanted else { return };
    let handle = app.clone();
    // Runs on the hotkey thread: hand the toggle to a pool thread and return at once.
    let press = move || {
        let app = handle.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let _ = set_visible(&app, None);
        });
    };
    match Hotkey::register(&spec, press) {
        Ok(h) => *hk = Some((spec, h)),
        Err(e) => *st.hotkey_error.lock().unwrap() = Some(e.to_string()),
    }
}

fn stop_worker(st: &OverlayState) {
    let running = st.worker.lock().unwrap().take();
    if let Some((tx, join)) = running {
        drop(tx);
        let _ = join.join();
    }
}

fn hwnd_of(w: &WebviewWindow) -> Result<isize, String> {
    w.hwnd().map(|h| h.0 as isize).map_err(|e| e.to_string())
}

/// Show (`Some(true)`), hide (`Some(false)`) or toggle (`None`) the overlay.
fn set_visible(app: &AppHandle, want: Option<bool>) -> Result<(), String> {
    let st = app.state::<OverlayState>();
    let _one = st.busy.lock().unwrap();
    let shown = st.worker.lock().unwrap().is_some();
    let on = want.unwrap_or(!shown) && !st.closing.load(Ordering::SeqCst);
    stop_worker(&st); // one feeding thread (and one ETW session) at a time
    let window = app.get_webview_window(LABEL);
    if !on {
        if let Some(w) = window {
            let hwnd = hwnd_of(&w)?;
            let _ = app.run_on_main_thread(move || overlay::hide(hwnd));
            if !current(app).enabled {
                let _ = w.destroy(); // a test with the overlay off leaves nothing behind
            }
        }
        let _ = app.emit("overlay:visible", false);
        return Ok(());
    }
    let w = match window {
        Some(w) => w,
        None => build(app)?,
    };
    let hwnd = hwnd_of(&w)?;
    // First placement right now, so the hotkey feels instant; the worker follows the game after.
    let target = overlay::foreground();
    let p = overlay::placement(target.map(|t| t.hwnd), &current(app));
    let _ = app.run_on_main_thread(move || overlay::show(hwnd, &p));
    let (tx, rx) = mpsc::channel();
    let handle = app.clone();
    let join = std::thread::Builder::new()
        .name("g2boost-overlay".into())
        .spawn(move || worker(&handle, hwnd, target, &rx))
        .map_err(|e| e.to_string())?;
    *st.worker.lock().unwrap() = Some((tx, join));
    let _ = app.emit("overlay:visible", true);
    Ok(())
}

fn build(app: &AppHandle) -> Result<WebviewWindow, String> {
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("overlay.html".into()))
        .title("G2 Boost overlay")
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .focusable(false)
        .visible(false)
        .drag_and_drop(false)
        .build()
        .map_err(|e| e.to_string())
}

/// The game's name from the library when the process is one of its games, else its exe name.
fn display_name(app: &AppHandle, pid: u32) -> Option<String> {
    let path = overlay::image_path(pid)?;
    let list = app.state::<super::GamesState>().list.lock().unwrap().clone();
    let name = games::running(&list, &[(pid, path.clone())], None)
        .first()
        .and_then(|r| list.iter().find(|g| g.id == r.id))
        .map(|g| g.name.clone())
        .or_else(|| std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned()))?;
    Some(name.chars().filter(|c| !c.is_control()).take(48).collect())
}

/// Feeds the overlay every 500 ms (FPS) and follows the game once a second (target, monitor,
/// CPU/GPU/RAM sample) until stopped. Owns the FPS session: it closes when this returns.
fn worker(app: &AppHandle, hwnd: isize, mut target: Option<Target>, stop: &mpsc::Receiver<()>) {
    // FPS needs administrator; without it the overlay still shows everything else.
    let fps_mon = if system::is_elevated() { fps::Monitor::start() } else { Err(fps::EtwError::NeedsAdmin) };
    let mut sys = Monitor::new();
    let (mut game, mut sample, mut retarget) = (None, None, true);
    for tick in 0u64.. {
        let cfg = current(app);
        if tick.is_multiple_of(2) {
            if let Some(fg) = overlay::foreground()
                && target.is_none_or(|t| t.pid != fg.pid)
            {
                target = Some(fg); // the user switched apps: count the new one from zero
                retarget = true;
            }
            if retarget && let Some(t) = target {
                if let Ok(m) = &fps_mon {
                    m.set_target(t.pid);
                }
                game = display_name(app, t.pid);
                retarget = false;
            }
            let p = overlay::placement(target.map(|t| t.hwnd), &cfg);
            let _ = app.run_on_main_thread(move || overlay::show(hwnd, &p));
            sample = sys.as_mut().map(Monitor::sample);
        }
        let (fps, fps_state) = match &fps_mon {
            Ok(m) => (m.stats(), "ok"),
            Err(fps::EtwError::NeedsAdmin) => (None, "needs_admin"),
            Err(_) => (None, "error"),
        };
        let _ = app.emit("overlay:frame", Frame { config: cfg, game: game.clone(), fps, fps_state, sys: sample.clone() });
        if !matches!(stop.recv_timeout(Duration::from_millis(500)), Err(RecvTimeoutError::Timeout)) {
            break; // sender dropped: hide or exit
        }
    }
}
