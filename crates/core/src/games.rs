//! Installed games from every launcher on this PC, read from each launcher's own files and
//! registry (no network, no launcher API, nothing started): Steam manifests, Epic manifests, GOG
//! and Ubisoft registry keys, and Uninstall entries of game publishers (EA app, Riot, Battle.net,
//! Rockstar and others), plus executables the user added. Names are data (they reach AI prompts
//! and the UI as text), never instructions. Also starts a game for "Preparar e jogar" (always as
//! the signed-in user, never with G2 Boost's administrator rights) and opens the exe picker.

use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{
    CLSCTX_ALL, CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree, IDispatch, IServiceProvider,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_DONTADDTORECENT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_PATHMUSTEXIST, FileOpenDialog,
    IFileOpenDialog, IShellBrowser, IShellDispatch2, IShellFolderViewDual, IShellItem, IShellWindows,
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHCreateItemFromParsingName, SHELLEXECUTEINFOW, SID_STopLevelBrowser,
    SIGDN_FILESYSPATH, SVGIO_BACKGROUND, SWC_DESKTOP, SWFO_NEEDDISPATCH, ShellExecuteExW, ShellWindows,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{BSTR, HRESULT, Interface, PCWSTR};

use crate::registry::{self, Root, Value, wide};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Launcher {
    Steam,
    Epic,
    Ea,
    Riot,
    Battlenet,
    Ubisoft,
    Gog,
    Rockstar,
    Other,
    /// An executable the user added by hand (settings `game_exes`).
    Custom,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct InstalledGame {
    /// `steam:730`, `epic:<AppName>`, `gog:<id>`, `ubisoft:<id>`, `app:<uninstall id>`, `exe:<name>`.
    pub id: String,
    pub name: String,
    pub launcher: Launcher,
    pub install_dir: Option<String>,
    /// The game's main executable (icon, and the name shown while it runs).
    pub main_exe: Option<String>,
    pub size_bytes: Option<u64>,
}

const MAX_NAME: usize = 120;

fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect::<String>().trim().to_string()
}

fn lower(p: &str) -> String {
    p.trim_end_matches('\\').to_ascii_lowercase()
}

/// Launchers write folders with `/` (Riot) or a trailing slash: one Windows form for matching.
fn norm_dir(p: String) -> String {
    p.replace('/', "\\").trim_end_matches('\\').to_string()
}

/* ---------------- classification of Uninstall entries ---------------- */

/// Launchers, anti-cheats, runtimes and tools that game publishers also install.
const TOOL_WORDS: &[&str] = &[
    "launcher", "client", "vanguard", "anti-cheat", "anticheat", "easyanticheat", "battleye", "redistributable", "redist",
    "ea app", "origin", "battle.net", "ubisoft connect", "uplay", "social club", "hoyoplay", "overlay", "updater", "helper",
    "service", "sdk", "driver", "runtime", "prerequisite", "directx", "physx", "vcredist", "touchup", "installer",
];

/// Which launcher installed an Uninstall entry, when it is a game (publisher + name rules).
pub(crate) fn classify(name: &str, publisher: &str) -> Option<Launcher> {
    let n = name.to_ascii_lowercase();
    if n.starts_with("steam app ") || n.is_empty() || TOOL_WORDS.iter().any(|w| n.contains(w)) {
        return None; // Steam games come from Steam's manifests; tools are not games
    }
    let p = publisher.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| p.contains(w));
    Some(if has(&["electronic arts", "ea swiss", "ea sports", "ea canada"]) || p == "ea" {
        Launcher::Ea
    } else if has(&["riot games"]) {
        Launcher::Riot
    } else if has(&["blizzard"]) {
        Launcher::Battlenet
    } else if has(&["ubisoft"]) {
        Launcher::Ubisoft
    } else if has(&["rockstar games"]) {
        Launcher::Rockstar
    } else if has(&[
        "cd projekt", "bethesda", "mojang", "bandai namco", "capcom", "sega", "square enix", "2k games", "take-two",
        "activision", "paradox interactive", "wargaming", "garena", "netease", "hoyoverse", "cognosphere", "mihoyo",
        "krafton", "nexon", "gaijin", "epic games", "kuro games", "level infinite", "smilegate", "pearl abyss",
    ]) {
        Launcher::Other
    } else {
        return None;
    })
}

/* ---------------- main executable ---------------- */

/// Executables that ship with games but are not the game.
const NOT_GAME_EXE: &[&str] = &[
    "unins", "crash", "report", "redist", "setup", "install", "helper", "update", "vc_redist", "dxsetup", "ue4prereq",
    "uesetup", "easyanticheat", "battleye", "cefprocess", "webhelper", "dotnet", "7z", "touchup", "cleanup", "physx",
    "python", "launcher", "bootstrap", "handler", "sandbox", "elevate",
];

/// The largest plausible game executable under `dir` (three levels deep, bounded; the usual
/// `bin\win64` / `Binaries\Win64` folders are looked at first).
fn main_exe_in(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(u64, PathBuf)> = None;
    let mut stack = vec![(dir.to_path_buf(), 0u8)];
    for sub in [r"game\bin\win64", r"bin\win64", r"Binaries\Win64", "x64", "bin"] {
        let p = dir.join(sub);
        if p.is_dir() {
            stack.push((p, 3));
        }
    }
    let mut seen = 0usize;
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            seen += 1;
            if seen > 8000 {
                return best.map(|b| b.1); // ponytail: big installs are judged by what was seen
            }
            let Ok(ty) = e.file_type() else {
                continue;
            };
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            if ty.is_dir() && !ty.is_symlink() && depth < 3 {
                stack.push((e.path(), depth + 1));
            } else if ty.is_file() && name.ends_with(".exe") && !NOT_GAME_EXE.iter().any(|w| name.contains(w)) {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                if size >= 512 * 1024 && best.as_ref().is_none_or(|(s, _)| size > *s) {
                    best = Some((size, e.path()));
                }
            }
        }
    }
    best.map(|b| b.1)
}

/* ---------------- sources ---------------- */

fn steam_games() -> Vec<InstalledGame> {
    crate::steam::installs()
        .into_iter()
        .map(|(g, dir)| InstalledGame {
            id: format!("steam:{}", g.appid),
            name: clean(&g.name),
            launcher: Launcher::Steam,
            main_exe: dir.as_deref().and_then(main_exe_in).map(|p| p.to_string_lossy().into_owned()),
            install_dir: dir.map(|d| d.to_string_lossy().into_owned()),
            size_bytes: Some(g.size_bytes).filter(|s| *s > 0),
        })
        .collect()
}

/// An Epic `AppName` that is safe in an id and in the launcher URI: plain ASCII words, no `..`.
fn epic_app_ok(a: &str) -> bool {
    !a.is_empty()
        && a.len() <= 128
        && !a.starts_with('.')
        && !a.contains("..")
        && a.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

/// `steam:<appid>` → the appid, only in the canonical form `games.rs` writes (digits, no sign,
/// no leading zero, fits in u32).
pub fn steam_appid(id: &str) -> Option<u32> {
    let d = id.strip_prefix("steam:")?;
    let a: u32 = d.parse().ok()?;
    (a != 0 && a.to_string() == d).then_some(a)
}

/// One Epic manifest (`*.item`, JSON). Not a game (apps, incomplete installs) → `None`.
pub(crate) fn parse_epic(text: &str) -> Option<InstalledGame> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    if v.get("bIsIncompleteInstall").and_then(serde_json::Value::as_bool) == Some(true) {
        return None;
    }
    if let Some(cats) = v.get("AppCategories").and_then(|c| c.as_array())
        && !cats.iter().any(|c| c.as_str() == Some("games"))
    {
        return None;
    }
    let app = s("AppName").filter(|a| epic_app_ok(a))?;
    let dir = s("InstallLocation").filter(|d| Path::new(d).is_absolute());
    let exe = match (&dir, s("LaunchExecutable")) {
        (Some(d), Some(e)) if !e.contains("..") => Some(Path::new(d).join(e).to_string_lossy().into_owned()),
        _ => None,
    };
    Some(InstalledGame {
        id: format!("epic:{app}"),
        name: clean(&s("DisplayName").unwrap_or_else(|| app.clone())),
        launcher: Launcher::Epic,
        install_dir: dir,
        main_exe: exe,
        size_bytes: v.get("InstallSize").and_then(serde_json::Value::as_u64),
    })
}

fn epic_games() -> Vec<InstalledGame> {
    let Some(pd) = crate::cleanup::env_dir("ProgramData") else {
        return Vec::new();
    };
    let Ok(rd) = std::fs::read_dir(pd.join(r"Epic\EpicGamesLauncher\Data\Manifests")) else {
        return Vec::new();
    };
    rd.flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".item"))
        .filter(|e| e.metadata().is_ok_and(|m| m.len() < 1 << 20))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|t| parse_epic(&t))
        .collect()
}

fn reg_str(root: Root, path: &str, name: &str) -> Option<String> {
    match registry::read(root, path, name) {
        Ok(Value::Str(s)) if !s.is_empty() => Some(s),
        _ => None,
    }
}

fn gog_games() -> Vec<InstalledGame> {
    const BASE: &str = r"SOFTWARE\WOW6432Node\GOG.com\Games";
    registry::subkeys(Root::Hklm, BASE)
        .unwrap_or_default()
        .into_iter()
        .filter(|id| id.chars().all(|c| c.is_ascii_digit()) && id.len() <= 20)
        .filter_map(|id| {
            let key = format!(r"{BASE}\{id}");
            let name = reg_str(Root::Hklm, &key, "gameName")?;
            Some(InstalledGame {
                id: format!("gog:{id}"),
                name: clean(&name),
                launcher: Launcher::Gog,
                install_dir: reg_str(Root::Hklm, &key, "path"),
                main_exe: reg_str(Root::Hklm, &key, "exe"),
                size_bytes: None,
            })
        })
        .collect()
}

fn ubisoft_games() -> Vec<InstalledGame> {
    const BASE: &str = r"SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs";
    registry::subkeys(Root::Hklm, BASE)
        .unwrap_or_default()
        .into_iter()
        .filter(|id| id.chars().all(|c| c.is_ascii_digit()) && id.len() <= 20)
        .filter_map(|id| {
            let dir = reg_str(Root::Hklm, &format!(r"{BASE}\{id}"), "InstallDir")?.replace('/', "\\");
            let name = Path::new(dir.trim_end_matches('\\')).file_name()?.to_string_lossy().into_owned();
            Some(InstalledGame {
                id: format!("ubisoft:{id}"),
                name: clean(&name),
                launcher: Launcher::Ubisoft,
                main_exe: main_exe_in(Path::new(&dir)).map(|p| p.to_string_lossy().into_owned()),
                install_dir: Some(dir),
                size_bytes: None,
            })
        })
        .collect()
}

fn publisher_games() -> Vec<InstalledGame> {
    crate::apps::desktop_list(&[])
        .into_iter()
        .filter_map(|a| {
            let launcher = classify(&a.name, &a.publisher)?;
            let dir = a.install_location.clone().filter(|d| Path::new(d).is_absolute());
            Some(InstalledGame {
                id: format!("app:{}", a.id),
                name: clean(&a.name),
                launcher,
                main_exe: a.exe.clone().or_else(|| dir.as_deref().and_then(|d| main_exe_in(Path::new(d))).map(|p| p.to_string_lossy().into_owned())),
                install_dir: dir,
                size_bytes: a.size_bytes,
            })
        })
        .collect()
}

fn custom_games(exes: &[String]) -> Vec<InstalledGame> {
    exes.iter()
        .map(|e| InstalledGame {
            id: format!("exe:{e}"),
            name: e.trim_end_matches(".exe").to_string(),
            launcher: Launcher::Custom,
            install_dir: None,
            main_exe: None,
            size_bytes: None,
        })
        .collect()
}

/// Launcher-specific sources first: an install found twice (Steam manifest + Uninstall entry)
/// keeps the first; then by name, case-insensitive.
pub(crate) fn merge(lists: Vec<Vec<InstalledGame>>) -> Vec<InstalledGame> {
    let mut dirs = HashSet::new();
    let mut names = HashSet::new();
    let mut out = Vec::new();
    for mut g in lists.into_iter().flatten() {
        g.install_dir = g.install_dir.map(norm_dir);
        g.main_exe = g.main_exe.map(|e| e.replace('/', "\\"));
        let dir_new = g.install_dir.as_deref().map(lower).is_none_or(|d| dirs.insert(d));
        if dir_new && names.insert(g.name.to_lowercase()) && !g.name.is_empty() {
            out.push(g);
        }
    }
    out.sort_by_key(|g| g.name.to_lowercase());
    out
}

/// Every installed game, plus the executables the user added (`settings.game_exes`).
pub fn installed(custom_exes: &[String]) -> Vec<InstalledGame> {
    merge(vec![steam_games(), epic_games(), gog_games(), ubisoft_games(), publisher_games(), custom_games(custom_exes)])
}

/* ---------------- executables the user picked ---------------- */

/// A local exe path that is safe to keep, show and start: drive letter + `:\` (no UNC, no
/// `\\?\`), plain components only (no `..`), no other `:` (alternate data streams), `.exe`,
/// bounded, no control characters. Pure: nothing is read from disk.
pub fn plain_exe(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() > 7
        && b.len() < 1024
        && b[0].is_ascii_alphabetic()
        && &b[1..3] == b":\\"
        && !p[2..].contains(':')
        && !p.chars().any(char::is_control)
        && p.to_ascii_lowercase().ends_with(".exe")
        && Path::new(p).components().all(|c| matches!(c, Component::Prefix(_) | Component::RootDir | Component::Normal(_)))
}

/// `plain_exe`, inside `inside` when given (the game's install folder), and present on disk.
pub fn exe_ok(p: &str, inside: Option<&str>) -> Option<String> {
    let lower = p.to_ascii_lowercase();
    let in_dir = inside.is_none_or(|d| lower.starts_with(&format!("{}\\", d.trim_end_matches('\\').to_ascii_lowercase())));
    (plain_exe(p) && in_dir && Path::new(p).is_file()).then(|| p.to_string())
}

/// `exe:<file name>` for a picked exe: the id a name-only entry of the same exe already has, so
/// its Game Mode and options carry over.
pub fn path_id(p: &str) -> Option<String> {
    Path::new(p).file_name().map(|n| format!("exe:{}", n.to_string_lossy().to_ascii_lowercase()))
}

/// Games added by picking their exe (`settings.game_paths`). No install folder: running = that
/// exact exe, and Game Mode uses the running exe's own folder for its checks.
fn path_games(paths: &[String]) -> Vec<InstalledGame> {
    paths
        .iter()
        .filter(|p| plain_exe(p))
        .filter_map(|p| {
            Some(InstalledGame {
                id: path_id(p)?,
                name: clean(&Path::new(p).file_stem()?.to_string_lossy()),
                launcher: Launcher::Custom,
                install_dir: None,
                main_exe: Some(p.clone()),
                size_bytes: None,
            })
        })
        .collect()
}

/// The exe the user chose per game (`settings.game_exe_override`) replaces the detected one while
/// it is still a valid exe inside the game's folder (when it has one).
pub(crate) fn apply_overrides(list: &mut [InstalledGame], chosen: &BTreeMap<String, String>) {
    for g in list {
        if let Some(exe) = chosen.get(&g.id).and_then(|e| exe_ok(e, g.install_dir.as_deref())) {
            g.main_exe = Some(exe);
        }
    }
}

/// The Jogos library: `installed` plus the games added by path, with the exe chosen per game.
pub fn library(s: &crate::settings::Settings) -> Vec<InstalledGame> {
    let mut list = merge(vec![
        steam_games(),
        epic_games(),
        gog_games(),
        ubisoft_games(),
        publisher_games(),
        path_games(&s.game_paths),
        custom_games(&s.game_exes),
    ]);
    apply_overrides(&mut list, &s.game_exe_override);
    list
}

/* ---------------- running ---------------- */

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RunningGame {
    pub id: String,
    /// The game's process (None when only Steam says it runs).
    pub pid: Option<u32>,
}

/// Which of `games` are running: a process inside the game's install folder, the exact exe (or
/// the exe name, when only the name is known) for hand-added games, or Steam's `RunningAppID`.
pub fn running(games: &[InstalledGame], procs: &[(u32, String)], steam_app: Option<u32>) -> Vec<RunningGame> {
    let procs: Vec<(u32, String)> = procs.iter().map(|(pid, p)| (*pid, p.to_ascii_lowercase())).collect();
    let mut out = Vec::new();
    for g in games {
        let pid = match (&g.install_dir, g.launcher) {
            (Some(dir), _) => {
                let d = format!("{}\\", lower(dir));
                // The biggest match is usually the game; launchers live elsewhere.
                procs.iter().find(|(_, p)| p.starts_with(&d)).map(|(pid, _)| *pid)
            }
            (None, Launcher::Custom) if g.main_exe.is_some() => {
                // A picked exe has no folder of its own to match: other games may share it.
                let exe = g.main_exe.as_deref().unwrap_or_default().to_ascii_lowercase();
                procs.iter().find(|(_, p)| *p == exe).map(|(pid, _)| *pid)
            }
            (None, Launcher::Custom) => {
                let exe = g.id.trim_start_matches("exe:");
                procs.iter().find(|(_, p)| p.rsplit('\\').next() == Some(exe)).map(|(pid, _)| *pid)
            }
            _ => None,
        };
        let steam = g.id.strip_prefix("steam:").and_then(|a| a.parse::<u32>().ok()).is_some_and(|a| Some(a) == steam_app);
        if pid.is_some() || steam {
            out.push(RunningGame { id: g.id.clone(), pid });
        }
    }
    out
}

/// Steam's own "a game is running" flag (`HKCU\Software\Valve\Steam\RunningAppID`).
pub fn steam_running_app() -> Option<u32> {
    match registry::read(Root::Hkcu, r"Software\Valve\Steam", "RunningAppID") {
        Ok(Value::Dword(id)) if id != 0 => Some(id),
        _ => None,
    }
}

/// The game's icon (its main executable's), as a PNG data URL.
pub fn icon(g: &InstalledGame) -> Option<String> {
    let exe = g.main_exe.as_deref()?;
    crate::icons::png_from_resource(Path::new(exe), 0).map(|png| crate::icons::data_url(&png))
}

/* ---------------- "Preparar e jogar" ---------------- */

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchTarget {
    /// A launcher URI: the launcher starts the game (DRM, updates, overlay stay in its hands).
    Uri(String),
    /// The game's own exe, started in its folder.
    Exe { path: String, dir: String },
}

/// What "Preparar e jogar" starts for `g` (pure): Steam and Epic through their launcher URI, the
/// rest through the main exe (the one the user chose, when there is one). `None` = nothing safe.
pub fn launch_target(g: &InstalledGame) -> Option<LaunchTarget> {
    if let Some(appid) = steam_appid(&g.id) {
        return Some(LaunchTarget::Uri(format!("steam://rungameid/{appid}")));
    }
    if let Some(app) = g.id.strip_prefix("epic:") {
        return epic_app_ok(app).then(|| LaunchTarget::Uri(format!("com.epicgames.launcher://apps/{app}?action=launch&silent=true")));
    }
    let exe = g.main_exe.as_deref().filter(|e| plain_exe(e))?;
    let dir = Path::new(exe).parent()?.to_string_lossy().into_owned();
    Some(LaunchTarget::Exe { path: exe.to_string(), dir })
}

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("no_exe")]
    NoExe,
    /// Elevated, and no desktop shell to start the game as the signed-in user.
    #[error("shell_unavailable")]
    Shell,
    #[error("launch_failed")]
    Failed,
}

/// Start `t` as the signed-in user. G2 Boost usually runs elevated and a game must not inherit
/// that: elevated, the request goes through the desktop's own Explorer (its folder view's
/// `IShellDispatch2::ShellExecute`), so it starts with the user's normal rights; never a fallback
/// to an elevated start. Not elevated, a plain `ShellExecuteExW`.
pub fn launch(t: &LaunchTarget) -> Result<(), LaunchError> {
    let (file, dir) = match t {
        LaunchTarget::Uri(u) => (u.as_str(), ""),
        LaunchTarget::Exe { path, dir } => {
            if !Path::new(path).is_file() {
                return Err(LaunchError::NoExe);
            }
            (path.as_str(), dir.as_str())
        }
    };
    let _com = crate::tasksched::Com::init().map_err(|_| LaunchError::Failed)?;
    if crate::system::is_elevated() { shell_execute_as_user(file, dir) } else { shell_execute(file, dir) }
}

fn shell_execute(file: &str, dir: &str) -> Result<(), LaunchError> {
    let (f, d, verb) = (wide(file), wide(dir), wide("open"));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(f.as_ptr()),
        lpDirectory: if dir.is_empty() { PCWSTR::null() } else { PCWSTR(d.as_ptr()) },
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: every string outlives the call; COM is initialised by the caller.
    unsafe { ShellExecuteExW(&mut info) }.map_err(|_| LaunchError::Failed)
}

/// Raymond Chen's "launch unelevated from an elevated process": desktop window → its top-level
/// browser → active view → background automation object → `Shell.Application` → ShellExecute.
fn shell_execute_as_user(file: &str, dir: &str) -> Result<(), LaunchError> {
    let shell = |_| LaunchError::Shell;
    // SAFETY: COM calls on this thread (initialised by the caller); every object is released here.
    unsafe {
        let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL).map_err(shell)?;
        let (desktop, root) = (VARIANT::from(0i32), VARIANT::default()); // CSIDL_DESKTOP
        let mut hwnd = 0i32;
        let found = windows.FindWindowSW(&desktop, &root, SWC_DESKTOP, &mut hwnd, SWFO_NEEDDISPATCH).map_err(shell)?;
        let browser: IShellBrowser = found.cast::<IServiceProvider>().map_err(shell)?.QueryService(&SID_STopLevelBrowser).map_err(shell)?;
        let view = browser.QueryActiveShellView().map_err(shell)?;
        let background: IDispatch = view.GetItemObject(SVGIO_BACKGROUND).map_err(shell)?;
        let app = background.cast::<IShellFolderViewDual>().map_err(shell)?.Application().map_err(shell)?;
        app.cast::<IShellDispatch2>()
            .map_err(shell)?
            .ShellExecute(&BSTR::from(file), &VARIANT::from(""), &VARIANT::from(dir), &VARIANT::from("open"), &VARIANT::from(SW_SHOWNORMAL.0))
            .map_err(|_| LaunchError::Failed)
    }
}

/// The native Open dialog for one `.exe` (owner = the app window, so it is modal to it), on its
/// own COM STA thread; it opens in `folder` when given. `Ok(None)` = the user cancelled. The
/// caller still validates the path (`exe_ok`): the dialog only proves the user picked it.
pub fn pick_exe(owner: isize, folder: Option<String>) -> Result<Option<String>, String> {
    let hr = |e: windows::core::Error| format!("{:#010x}", e.code().0 as u32);
    std::thread::spawn(move || {
        let _com = crate::tasksched::Com::init().map_err(|e| e.to_string())?;
        // SAFETY: the standard IFileOpenDialog sequence on this STA thread; strings outlive the
        // calls, the returned path is freed with CoTaskMemFree, COM objects drop here.
        unsafe {
            let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).map_err(hr)?;
            let spec = wide("*.exe");
            dlg.SetFileTypes(&[COMDLG_FILTERSPEC { pszName: PCWSTR(spec.as_ptr()), pszSpec: PCWSTR(spec.as_ptr()) }]).map_err(hr)?;
            let opts = dlg.GetOptions().map_err(hr)?;
            dlg.SetOptions(opts | FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR | FOS_DONTADDTORECENT).map_err(hr)?;
            if let Some(f) = folder.filter(|f| Path::new(f).is_dir()) {
                let w = wide(&f);
                if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(w.as_ptr()), None) {
                    let _ = dlg.SetFolder(&item);
                }
            }
            let owner = (owner != 0).then_some(HWND(owner as *mut core::ffi::c_void));
            match dlg.Show(owner) {
                Err(e) if e.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => return Ok(None),
                r => r.map_err(hr)?,
            }
            let p = dlg.GetResult().map_err(hr)?.GetDisplayName(SIGDN_FILESYSPATH).map_err(hr)?;
            let path = p.to_string().map_err(|e| e.to_string());
            CoTaskMemFree(Some(p.0 as *const core::ffi::c_void));
            path.map(Some)
        }
    })
    .join()
    .map_err(|_| "picker".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str, name: &str, launcher: Launcher, dir: Option<&str>) -> InstalledGame {
        InstalledGame { id: id.into(), name: name.into(), launcher, install_dir: dir.map(str::to_string), main_exe: None, size_bytes: None }
    }

    #[test]
    fn publishers_and_tools() {
        assert_eq!(classify("EA SPORTS FC™ 26", "Electronic Arts"), Some(Launcher::Ea));
        assert_eq!(classify("VALORANT", "Riot Games, Inc"), Some(Launcher::Riot));
        assert_eq!(classify("Riot Vanguard", "Riot Games, Inc"), None, "anti-cheat is not a game");
        assert_eq!(classify("Riot Client", "Riot Games, Inc"), None);
        assert_eq!(classify("Battle.net", "Blizzard Entertainment"), None);
        assert_eq!(classify("Overwatch", "Blizzard Entertainment"), Some(Launcher::Battlenet));
        assert_eq!(classify("EA app", "Electronic Arts"), None);
        assert_eq!(classify("Grand Theft Auto V", "Rockstar Games"), Some(Launcher::Rockstar));
        assert_eq!(classify("Rockstar Games Launcher", "Rockstar Games"), None);
        assert_eq!(classify("Steam App 730", "Valve"), None, "Steam manifests own Steam games");
        assert_eq!(classify("7-Zip", "Igor Pavlov"), None);
        assert_eq!(classify("Genshin Impact", "COGNOSPHERE PTE. LTD."), Some(Launcher::Other));
    }

    #[test]
    fn epic_manifest() {
        let item = r#"{"DisplayName":"Fortnite","AppName":"Fortnite","InstallLocation":"D:\\Epic\\Fortnite",
            "LaunchExecutable":"FortniteGame/Binaries/Win64/FortniteLauncher.exe","InstallSize":123,"AppCategories":["public","games","applications"]}"#;
        let g = parse_epic(item).unwrap();
        assert_eq!((g.id.as_str(), g.name.as_str(), g.launcher, g.size_bytes), ("epic:Fortnite", "Fortnite", Launcher::Epic, Some(123)));
        assert!(g.main_exe.unwrap().ends_with("FortniteLauncher.exe"));
        assert!(parse_epic(r#"{"AppName":"UE","AppCategories":["engines"]}"#).is_none(), "not a game");
        assert!(parse_epic(r#"{"AppName":"X","bIsIncompleteInstall":true}"#).is_none());
        assert!(parse_epic(r#"{"AppName":"..\\evil"}"#).is_none(), "id must be plain");
        assert!(parse_epic("garbage").is_none());
        let traversal = parse_epic(r#"{"AppName":"G","InstallLocation":"D:\\G","LaunchExecutable":"..\\..\\x.exe"}"#).unwrap();
        assert!(traversal.main_exe.is_none());
    }

    #[test]
    fn merge_dedupes_by_folder_then_name() {
        let merged = merge(vec![
            vec![game("steam:1", "Game A", Launcher::Steam, Some(r"G:\Steam\common\Game A"))],
            vec![game("app:m:1", "Game A (Steam entry)", Launcher::Other, Some(r"g:\steam\common\game a\"))],
            vec![game("epic:b", "Game B", Launcher::Epic, None), game("app:m:2", "game b", Launcher::Ea, None)],
        ]);
        assert_eq!(merged.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), vec!["steam:1", "epic:b"]);
    }

    #[test]
    fn running_by_folder_exe_or_steam() {
        let games = vec![
            game("steam:730", "Counter-Strike 2", Launcher::Steam, Some(r"G:\SteamLibrary\steamapps\common\Counter-Strike Global Offensive")),
            game("app:x", "VALORANT", Launcher::Riot, Some(r"C:\Riot Games\VALORANT\live")),
            game("exe:mygame.exe", "mygame", Launcher::Custom, None),
            game("epic:Fortnite", "Fortnite", Launcher::Epic, Some(r"D:\Epic\Fortnite")),
        ];
        let procs = vec![
            (10, r"C:\Riot Games\VALORANT\live\ShooterGame\Binaries\Win64\VALORANT-Win64-Shipping.exe".to_string()),
            (11, r"C:\Games\MyGame.exe".to_string()),
            (12, r"D:\Epic\FortniteX\other.exe".to_string()),
        ];
        let r = running(&games, &procs, Some(730));
        assert_eq!(
            r,
            vec![
                RunningGame { id: "steam:730".into(), pid: None },
                RunningGame { id: "app:x".into(), pid: Some(10) },
                RunningGame { id: "exe:mygame.exe".into(), pid: Some(11) },
            ],
            "a sibling folder with the same prefix is not the game"
        );
        assert!(running(&games, &[], None).is_empty());
    }

    fn with_exe(id: &str, launcher: Launcher, dir: Option<&str>, exe: Option<&str>) -> InstalledGame {
        InstalledGame { main_exe: exe.map(str::to_string), ..game(id, "G", launcher, dir) }
    }

    #[test]
    fn steam_ids_are_canonical() {
        assert_eq!(steam_appid("steam:730"), Some(730));
        assert_eq!(steam_appid("steam:4294967295"), Some(u32::MAX));
        for bad in ["steam:", "steam:0", "steam:0730", "steam:+730", "steam: 730", "steam:730 ", "steam:73a", "steam:4294967296", "epic:730", "730", "steam:730/../1"] {
            assert_eq!(steam_appid(bad), None, "{bad}");
        }
    }

    #[test]
    fn plain_exe_rejects_hostile_paths() {
        assert!(plain_exe(r"D:\Games\Hades\Hades.exe"));
        assert!(plain_exe(r"c:\jogos\ação\Jogo.EXE"), "case and non-ASCII names are fine");
        for bad in [
            r"\\server\share\game.exe",
            r"\\?\C:\Games\game.exe",
            r"C:game.exe",
            r"C:\Games\..\Windows\System32\cmd.exe",
            r"C:\Games\game.exe:evil.exe",
            r"C:\Games\game.exe\..",
            r"C:\Games\notes.txt",
            r"C:\Games\game.exe.lnk",
            "C:\\Games\\ga\u{0}me.exe",
            "C:\\Games\\game\n.exe",
            r"Games\game.exe",
            r"1:\game.exe",
            "",
        ] {
            assert!(!plain_exe(bad), "{bad:?}");
        }
        assert!(!plain_exe(&format!(r"C:\{}.exe", "a".repeat(1100))), "oversize");
    }

    #[test]
    fn exe_ok_needs_the_file_and_the_folder() {
        let dir = std::env::temp_dir().join(format!("g2boost-exeok-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join(r"bin\Game.exe"), b"MZ").unwrap();
        let d = dir.to_string_lossy().into_owned();
        let exe = format!(r"{d}\bin\Game.exe");
        assert_eq!(exe_ok(&exe, Some(&d)).as_deref(), Some(exe.as_str()));
        assert_eq!(exe_ok(&exe, Some(&format!(r"{d}\"))).as_deref(), Some(exe.as_str()), "trailing slash on the folder");
        assert_eq!(exe_ok(&exe, None).as_deref(), Some(exe.as_str()));
        assert_eq!(exe_ok(&exe, Some(&format!("{d}x"))), None, "a sibling folder with the same prefix");
        assert_eq!(exe_ok(&format!(r"{d}\bin\Missing.exe"), Some(&d)), None, "must exist");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn launch_targets_per_launcher() {
        let steam = with_exe("steam:730", Launcher::Steam, Some(r"D:\S\cs2"), Some(r"D:\S\cs2\cs2.exe"));
        assert_eq!(launch_target(&steam), Some(LaunchTarget::Uri("steam://rungameid/730".into())));
        let epic = with_exe("epic:Fortnite", Launcher::Epic, None, None);
        assert_eq!(launch_target(&epic), Some(LaunchTarget::Uri("com.epicgames.launcher://apps/Fortnite?action=launch&silent=true".into())));
        let riot = with_exe("app:m64:x", Launcher::Riot, Some(r"C:\Riot Games\League of Legends"), Some(r"C:\Riot Games\Riot Client\RiotClientServices.exe"));
        assert_eq!(
            launch_target(&riot),
            Some(LaunchTarget::Exe { path: r"C:\Riot Games\Riot Client\RiotClientServices.exe".into(), dir: r"C:\Riot Games\Riot Client".into() })
        );
        // Hostile ids and exes never become a target.
        for app in ["epic:", "epic:..", "epic:.hidden", "epic:a..b", "epic:a/b", "epic:a?x=1", "epic:a b", "epic:a&action=uninstall"] {
            assert_eq!(launch_target(&with_exe(app, Launcher::Epic, None, None)), None, "{app}");
        }
        assert_eq!(launch_target(&with_exe("steam:0730", Launcher::Steam, None, None)), None, "not a canonical Steam id, no exe");
        assert_eq!(launch_target(&with_exe("exe:x.exe", Launcher::Custom, None, None)), None, "name only: nothing to start");
        assert_eq!(launch_target(&with_exe("gog:1", Launcher::Gog, None, Some(r"\\evil\share\x.exe"))), None);
        assert_eq!(launch_target(&with_exe("gog:1", Launcher::Gog, None, Some(r"C:\G\..\x.exe"))), None);
        assert_eq!(launch_target(&with_exe("gog:1", Launcher::Gog, None, Some(r"C:\G\setup.bat"))), None);
    }

    #[test]
    fn picked_paths_and_chosen_exes() {
        let paths = vec![r"D:\Games\Hades\Hades.exe".to_string(), r"\\nas\x.exe".to_string(), r"D:\Other\HADES.exe".to_string()];
        let picked = path_games(&paths);
        assert_eq!(picked.len(), 2, "UNC dropped");
        assert_eq!((picked[0].id.as_str(), picked[0].name.as_str(), picked[0].install_dir.as_deref()), ("exe:hades.exe", "Hades", None));
        // Same file name twice: the first wins (same id); a name-only entry with that name too.
        let merged = merge(vec![picked, custom_games(&["hades.exe".into(), "other.exe".into()])]);
        assert_eq!(merged.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(), vec!["exe:hades.exe", "exe:other.exe"]);
        assert_eq!(merged[0].main_exe.as_deref(), Some(r"D:\Games\Hades\Hades.exe"));
        // A chosen exe only replaces the detected one when it is real and inside the game's folder.
        let dir = std::env::temp_dir().join(format!("g2boost-override-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Real.exe"), b"MZ").unwrap();
        let d = dir.to_string_lossy().into_owned();
        let mut list = vec![
            with_exe("steam:1", Launcher::Steam, Some(&d), Some(&format!(r"{d}\Detected.exe"))),
            with_exe("steam:2", Launcher::Steam, Some(&d), Some(&format!(r"{d}\Detected.exe"))),
            with_exe("steam:3", Launcher::Steam, Some(r"C:\Elsewhere"), None),
        ];
        let chosen = BTreeMap::from([
            ("steam:1".to_string(), format!(r"{d}\Real.exe")),
            ("steam:2".to_string(), format!(r"{d}\Missing.exe")),
            ("steam:3".to_string(), format!(r"{d}\Real.exe")),
        ]);
        apply_overrides(&mut list, &chosen);
        assert_eq!(list[0].main_exe.as_deref(), Some(format!(r"{d}\Real.exe").as_str()));
        assert_eq!(list[1].main_exe.as_deref(), Some(format!(r"{d}\Detected.exe").as_str()), "missing file: detected kept");
        assert_eq!(list[2].main_exe, None, "outside the game's folder: ignored");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_keep_only_plain_picked_paths() {
        use crate::settings::{Settings, sanitize};
        let s = sanitize(Settings {
            game_paths: vec![
                r"D:\Games\Hades\Hades.exe".into(),
                r"d:\games\hades\hades.exe".into(),
                r"\\nas\share\x.exe".into(),
                r"C:\x\..\Windows\System32\cmd.exe".into(),
                r"C:\x\notes.txt".into(),
            ],
            game_exe_override: BTreeMap::from([
                ("steam:730".to_string(), r"D:\S\cs2\cs2.exe".to_string()),
                ("steam:1".to_string(), r"\\nas\x.exe".to_string()),
                ("noprefix".to_string(), r"D:\x.exe".to_string()),
            ]),
            ..Default::default()
        });
        assert_eq!(s.game_paths, vec![r"D:\Games\Hades\Hades.exe".to_string()], "deduplicated, UNC/traversal/non-exe dropped");
        assert_eq!(s.game_exe_override.keys().collect::<Vec<_>>(), vec!["steam:730"]);
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s, "json roundtrip");
    }

    #[test]
    fn a_picked_exe_runs_only_as_itself() {
        let games = vec![with_exe("exe:game.exe", Launcher::Custom, None, Some(r"D:\Games\A\game.exe"))];
        let procs = vec![(20, r"D:\Games\B\game.exe".to_string()), (21, r"D:\Games\A\helper.exe".to_string())];
        assert!(running(&games, &procs, None).is_empty(), "same name elsewhere, or another exe in its folder");
        let procs = vec![(22, r"d:\games\a\GAME.exe".to_string())];
        assert_eq!(running(&games, &procs, None), vec![RunningGame { id: "exe:game.exe".into(), pid: Some(22) }]);
    }

    /// Opens the real Open dialog (`cargo test -p g2boost-core -- pick_exe_live --ignored --nocapture`).
    #[test]
    #[ignore]
    fn pick_exe_live() {
        println!("{:?}", pick_exe(0, std::env::var("SystemRoot").ok()));
    }

    /// Starts Notepad through the launch path (elevated: through Explorer, so it must show up
    /// unelevated in Task Manager). `cargo test -p g2boost-core -- launch_live --ignored`.
    #[test]
    #[ignore]
    fn launch_live() {
        let win = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let t = LaunchTarget::Exe { path: format!(r"{win}\System32\notepad.exe"), dir: format!(r"{win}\System32") };
        launch(&t).unwrap();
        println!("elevated={} launched notepad", crate::system::is_elevated());
    }

    #[test]
    #[ignore]
    fn installed_live() {
        for g in installed(&[]) {
            println!("{:?} {:<40} {:?} exe={:?}", g.launcher, g.name, g.install_dir, g.main_exe.is_some());
        }
    }
}
