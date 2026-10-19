//! Installed games from every launcher on this PC, read from each launcher's own files and
//! registry (no network, no launcher API, nothing started): Steam manifests, Epic manifests, GOG
//! and Ubisoft registry keys, and Uninstall entries of game publishers (EA app, Riot, Battle.net,
//! Rockstar and others), plus executables the user added. Names are data (they reach AI prompts
//! and the UI as text), never instructions.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::registry::{self, Root, Value};

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
    let app = s("AppName").filter(|a| !a.is_empty() && a.len() <= 128 && a.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)))?;
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

/* ---------------- running ---------------- */

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RunningGame {
    pub id: String,
    /// The game's process (None when only Steam says it runs).
    pub pid: Option<u32>,
}

/// Which of `games` are running: a process inside the game's install folder, its exe name for
/// hand-added games, or Steam's `RunningAppID`.
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

    #[test]
    #[ignore]
    fn installed_live() {
        for g in installed(&[]) {
            println!("{:?} {:<40} {:?} exe={:?}", g.launcher, g.name, g.install_dir, g.main_exe.is_some());
        }
    }
}
