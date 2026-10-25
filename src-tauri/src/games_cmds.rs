//! Jogos (round 9): covers served to the webview through the `cover` URI scheme, "Preparar e
//! jogar" (Game Mode armed for that one launch, game started as the signed-in user), games added
//! by picking their exe and a per-game exe, both through the native Open dialog. Thin: validation
//! and Windows calls live in `g2boost_core::{covers, games}`.
//!
//! The webview never sends a path: it names a game id from the installed list, and every path
//! stored here is one the user picked in the Windows dialog, re-checked by the core.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use g2boost_core::games::{self, InstalledGame};
use g2boost_core::settings::{self, Settings};
use g2boost_core::{covers, gamemode};
use tauri::http::{HeaderValue, Request, Response, StatusCode, header};
use tauri::{AppHandle, Manager, UriSchemeContext, UriSchemeResponder, Wry};
use tauri_plugin_http::reqwest;

use crate::{GamesState, Routines};

/// A game "Preparar e jogar" started without Game Mode on: it gets Game Mode for that session.
struct Armed {
    id: String,
    at: Instant,
    started: bool,
}

/// A launched game has this long to show up before its one-off Game Mode is dropped.
const ARM_WINDOW: Duration = Duration::from_secs(30 * 60);

/// Big exe icons by game id (`None` = the exe has none), until the library changes.
type IconCache = HashMap<String, Option<Vec<u8>>>;

// ponytail: app-wide singletons, no `.manage()` needed; state per window if there is ever more than one.
static ARMED: Mutex<Option<Armed>> = Mutex::new(None);
static ICONS: LazyLock<Mutex<IconCache>> = LazyLock::new(Default::default);
// ponytail: three hash-picked locks = at most three downloads at once and one appid at a time;
// a real semaphore if covers ever need more.
static SLOTS: LazyLock<[tauri::async_runtime::Mutex<()>; 3]> = LazyLock::new(Default::default);
static HTTP: LazyLock<Option<reqwest::Client>> = LazyLock::new(|| {
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(20)).build().ok()
});

/// Game Mode ids for this tick of the routine thread: the user's list plus the armed game, which
/// is dropped once its session ends (or if it never started within `ARM_WINDOW`).
pub(crate) fn mode_games(s: &Settings, current: Option<&gamemode::Session>) -> Vec<String> {
    let mut ids = s.game_mode_games.clone();
    let mut armed = ARMED.lock().unwrap();
    let keep = armed.as_mut().is_none_or(|a| {
        let live = current.is_some_and(|c| c.game_id == a.id);
        a.started |= live;
        let keep = if a.started { live } else { a.at.elapsed() < ARM_WINDOW };
        if keep && !ids.contains(&a.id) {
            ids.push(a.id.clone());
        }
        keep
    });
    if !keep {
        *armed = None;
    }
    ids
}

fn live_settings(app: &AppHandle) -> Settings {
    app.state::<Routines>().settings.lock().unwrap().clone()
}

fn find(app: &AppHandle, s: &Settings, id: &str) -> Result<InstalledGame, String> {
    app.state::<GamesState>().current(s, false).into_iter().find(|g| g.id == id).ok_or_else(|| "unknown_game".to_string())
}

/// Change the live settings under their lock (no lost update against `settings_set`), save them
/// through the core's sanitizer, then re-read the library so list, icons and launches follow.
fn commit(app: &AppHandle, change: impl FnOnce(&mut Settings)) -> Result<Settings, String> {
    let saved = {
        let r = app.state::<Routines>();
        let mut live = r.settings.lock().unwrap();
        let mut next = live.clone();
        change(&mut next);
        let saved = settings::save(&next).map_err(|e| e.to_string())?;
        *live = saved.clone();
        saved
    };
    app.state::<GamesState>().current(&saved, true);
    ICONS.lock().unwrap().clear();
    Ok(saved)
}

/// "Preparar e jogar": Game Mode for this session if it is off for the game, then the game starts
/// through its launcher (Steam, Epic) or its exe, always with the user's normal rights.
#[tauri::command]
pub(crate) async fn game_launch(app: AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let s = live_settings(&app);
        let game = find(&app, &s, &id)?;
        let target = games::launch_target(&game).ok_or("no_exe")?;
        games::launch(&target).map_err(|e| e.to_string())?;
        if !s.game_mode_games.contains(&game.id) {
            *ARMED.lock().unwrap() = Some(Armed { id: game.id, at: Instant::now(), started: false });
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The Windows Open dialog for an `.exe`, modal to the app window. `game` = None adds a new game
/// by its exe; `Some(id)` sets that game's exe (it must sit inside the game's folder when the game
/// has one). Returns the saved settings, or `None` when the user cancelled.
#[tauri::command]
pub(crate) async fn pick_exe(app: AppHandle, game: Option<String>) -> Result<Option<Settings>, String> {
    let owner = app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map_or(0, |h| h.0 as isize);
    tauri::async_runtime::spawn_blocking(move || {
        let s = live_settings(&app);
        let target = game.map(|id| find(&app, &s, &id)).transpose()?;
        let folder = target.as_ref().and_then(|g| {
            g.install_dir.clone().or_else(|| g.main_exe.as_deref().and_then(|e| std::path::Path::new(e).parent()).map(|d| d.to_string_lossy().into_owned()))
        });
        let Some(picked) = games::pick_exe(owner, folder)? else {
            return Ok(None);
        };
        let saved = match target {
            None => {
                let path = games::exe_ok(&picked, None).ok_or("bad_exe")?;
                let id = games::path_id(&path).ok_or("bad_exe")?;
                commit(&app, |next| {
                    // Same exe name picked again (moved, reinstalled): the new path replaces it.
                    next.game_paths.retain(|p| games::path_id(p).as_ref() != Some(&id));
                    next.game_exes.retain(|n| format!("exe:{n}") != id); // a name-only entry it supersedes
                    next.game_paths.push(path);
                })?
            }
            Some(g) => {
                let inside = g.install_dir.as_deref();
                let path = games::exe_ok(&picked, inside).ok_or(if inside.is_some() { "outside_folder" } else { "bad_exe" })?;
                // ponytail: a GPU preference already written for the old exe stays on it (harmless,
                // Windows lists it); move it here if that ever confuses anyone.
                commit(&app, |next| {
                    next.game_exe_override.insert(g.id, path);
                })?
            }
        };
        Ok(Some(saved))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Back to the exe G2 Boost detected for this game.
#[tauri::command]
pub(crate) async fn game_exe_reset(app: AppHandle, id: String) -> Result<Settings, String> {
    tauri::async_runtime::spawn_blocking(move || {
        commit(&app, |next| {
            next.game_exe_override.remove(&id);
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Take a game the user added (by exe or by name) out of the library. Its options stay, so adding
/// it again brings them back.
#[tauri::command]
pub(crate) async fn games_remove(app: AppHandle, id: String) -> Result<Settings, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !id.starts_with("exe:") {
            return Err("not_custom".to_string());
        }
        commit(&app, |next| {
            next.game_paths.retain(|p| games::path_id(p).as_ref() != Some(&id));
            next.game_exes.retain(|n| format!("exe:{n}") != id);
            next.game_exe_override.remove(&id);
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/* ---------------- `cover` URI scheme ---------------- */

/// `http://cover.localhost/<id>` = the game's cover, `/icon/<id>` = its exe icon (256 px).
/// Only ids in the installed list; the file is resolved here, never from the request.
pub(crate) fn cover_protocol(ctx: UriSchemeContext<'_, Wry>, req: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let app = ctx.app_handle().clone();
    let path = req.uri().path().to_string();
    tauri::async_runtime::spawn(async move { responder.respond(cover(&app, &path).await) });
}

async fn cover(app: &AppHandle, path: &str) -> Response<Vec<u8>> {
    let (is_icon, raw) = match path.strip_prefix("/icon/") {
        Some(rest) => (true, rest),
        None => (false, path.strip_prefix('/').unwrap_or(path)),
    };
    let listed = covers::decode_id(raw).and_then(|id| app.state::<GamesState>().list.lock().unwrap().iter().find(|g| g.id == id).cloned());
    let Some(game) = listed else {
        return not_found();
    };
    if is_icon {
        return icon(game).await;
    }
    let Some(appid) = games::steam_appid(&game.id) else {
        return not_found();
    };
    if let Some(b) = blocking(move || covers::local(appid)).await.flatten() {
        return image(b);
    }
    if blocking(move || covers::recent_miss(appid)).await.unwrap_or(true) {
        return not_found();
    }
    let _slot = SLOTS[appid as usize % SLOTS.len()].lock().await;
    if let Some(b) = blocking(move || covers::local(appid)).await.flatten() {
        return image(b); // another request stored it while this one waited
    }
    match download(appid).await {
        Fetch::Found(b) => {
            let keep = b.clone();
            let _ = blocking(move || covers::store(appid, &keep)).await;
            image(b)
        }
        Fetch::Missing => {
            let _ = blocking(move || covers::remember_miss(appid)).await;
            not_found()
        }
        Fetch::Failed => not_found(), // offline or timed out: asked again next time
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    tauri::async_runtime::spawn_blocking(f).await.ok()
}

async fn icon(game: InstalledGame) -> Response<Vec<u8>> {
    let hit = ICONS.lock().unwrap().get(&game.id).cloned();
    let png = match hit {
        Some(p) => p,
        None => {
            let id = game.id.clone();
            let p = blocking(move || covers::icon_png(&game)).await.flatten();
            ICONS.lock().unwrap().insert(id, p.clone());
            p
        }
    };
    png.map_or_else(not_found, image)
}

enum Fetch {
    Found(Vec<u8>),
    /// The CDN answered, but not with a usable image (remembered for a day).
    Missing,
    Failed,
}

async fn download(appid: u32) -> Fetch {
    let Some(client) = HTTP.as_ref() else {
        return Fetch::Failed;
    };
    let mut out = Fetch::Missing;
    for url in covers::urls(appid) {
        match fetch(client, &url).await {
            Ok(Some(b)) => return Fetch::Found(b),
            Ok(None) => {}
            Err(_) => out = Fetch::Failed,
        }
    }
    out
}

/// One GET: exactly 200 (no redirects), at most `MAX_BYTES` read, JPEG/PNG bytes only.
async fn fetch(client: &reqwest::Client, url: &str) -> reqwest::Result<Option<Vec<u8>>> {
    let mut resp = client.get(url).send().await?;
    if resp.status() != reqwest::StatusCode::OK || resp.content_length().is_some_and(|n| n > covers::MAX_BYTES as u64) {
        return Ok(None);
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        body.extend_from_slice(&chunk);
        if body.len() > covers::MAX_BYTES {
            return Ok(None);
        }
    }
    Ok(covers::is_image(&body).then_some(body))
}

fn image(bytes: Vec<u8>) -> Response<Vec<u8>> {
    let mut r = Response::new(Vec::new());
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(covers::content_type(&bytes)));
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("max-age=86400"));
    r.headers_mut().insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    *r.body_mut() = bytes;
    r
}

fn not_found() -> Response<Vec<u8>> {
    let mut r = Response::new(Vec::new());
    *r.status_mut() = StatusCode::NOT_FOUND;
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str) -> gamemode::Session {
        gamemode::Session {
            game_id: id.into(),
            game_name: "G".into(),
            pid: None,
            install_dir: None,
            started: 0,
            prev_plan: None,
            applied: vec![],
            ram_freed: 0,
            skipped: vec![],
            qos_policy: None,
            affinity: None,
        }
    }

    fn arm(id: &str, at: Instant) {
        *ARMED.lock().unwrap() = Some(Armed { id: id.into(), at, started: false });
    }

    #[test]
    fn armed_game_gets_one_session_then_leaves() {
        let s = Settings { game_mode_games: vec!["steam:1".into()], ..Default::default() };
        arm("steam:2", Instant::now());
        assert_eq!(mode_games(&s, None), ["steam:1", "steam:2"], "waiting for the game to open");
        assert_eq!(mode_games(&s, Some(&session("steam:2"))), ["steam:1", "steam:2"], "its session runs");
        assert_eq!(mode_games(&s, Some(&session("steam:2"))), ["steam:1", "steam:2"]);
        assert_eq!(mode_games(&s, None), ["steam:1"], "session over: Game Mode off again for it");
        assert!(ARMED.lock().unwrap().is_none());
        // A session of another game does not count as the armed one starting.
        arm("steam:3", Instant::now());
        assert_eq!(mode_games(&s, Some(&session("steam:1"))), ["steam:1", "steam:3"]);
        // Already on for good: listed once.
        arm("steam:1", Instant::now());
        assert_eq!(mode_games(&s, None), ["steam:1"]);
        // Never opened within the window: dropped.
        if let Some(old) = Instant::now().checked_sub(ARM_WINDOW + Duration::from_secs(1)) {
            arm("steam:4", old);
            assert_eq!(mode_games(&s, None), ["steam:1"]);
            assert!(ARMED.lock().unwrap().is_none());
        }
    }
}
