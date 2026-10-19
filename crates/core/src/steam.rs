//! Installed Steam games, read from Steam's own files (no network, no Steam API key):
//! `HKCU\Software\Valve\Steam\SteamPath` → `steamapps\libraryfolders.vdf` → each library's
//! `appmanifest_<appid>.acf`. Names are data (they end up in AI prompts), never instructions.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::registry::{self, Root, Value};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Game {
    pub appid: u32,
    pub name: String,
    pub size_bytes: u64,
    /// Library root (drive letter is useful: "game on the HDD").
    pub library: String,
}

/// Every `"key" "value"` pair in a VDF/ACF text (nesting ignored — enough for these files).
pub(crate) fn vdf_pairs(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split('"').collect();
        // `\t"key"\t\t"value"` splits into ["\t", key, "\t\t", value, ""].
        if parts.len() >= 5 {
            out.push((parts[1].to_string(), parts[3].replace("\\\\", "\\")));
        }
    }
    out
}

fn steam_root() -> Option<PathBuf> {
    match registry::read(Root::Hkcu, r"Software\Valve\Steam", "SteamPath") {
        Ok(Value::Str(s)) if !s.is_empty() => Some(PathBuf::from(s.replace('/', "\\"))),
        _ => None,
    }
}

fn libraries(root: &Path) -> Vec<PathBuf> {
    let mut libs = vec![root.to_path_buf()];
    if let Ok(text) = std::fs::read_to_string(root.join(r"steamapps\libraryfolders.vdf")) {
        for (k, v) in vdf_pairs(&text) {
            if k == "path" {
                let p = PathBuf::from(v);
                if !libs.iter().any(|l| l.as_os_str().eq_ignore_ascii_case(p.as_os_str())) {
                    libs.push(p);
                }
            }
        }
    }
    libs
}

pub(crate) fn parse_manifest(text: &str, library: &str) -> Option<Game> {
    let pairs = vdf_pairs(text);
    let get = |key: &str| pairs.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str());
    let appid: u32 = get("appid")?.parse().ok()?;
    let name: String = get("name")?.chars().filter(|c| !c.is_control()).take(120).collect();
    // Steamworks redistributables and tools are not games.
    if appid == 228_980 || name.is_empty() {
        return None;
    }
    Some(Game { appid, name, size_bytes: get("SizeOnDisk").and_then(|s| s.parse().ok()).unwrap_or(0), library: library.to_string() })
}

/// Each library's `steamapps\common` with the folder names its manifests claim (`installdir`).
pub fn library_folders() -> Vec<(PathBuf, Vec<String>)> {
    let Some(root) = steam_root() else {
        return Vec::new();
    };
    libraries(&root)
        .into_iter()
        .filter_map(|lib| {
            let rd = std::fs::read_dir(lib.join("steamapps")).ok()?;
            let dirs = rd
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("appmanifest_"))
                .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                .filter_map(|t| vdf_pairs(&t).into_iter().find(|(k, _)| k.eq_ignore_ascii_case("installdir")).map(|(_, v)| v))
                .collect();
            let common = lib.join(r"steamapps\common");
            common.is_dir().then_some((common, dirs))
        })
        .collect()
}

pub fn installed() -> Vec<Game> {
    let Some(root) = steam_root() else {
        return Vec::new();
    };
    let mut games = Vec::new();
    for lib in libraries(&root) {
        let Ok(rd) = std::fs::read_dir(lib.join("steamapps")) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(e.path())
                && let Some(g) = parse_manifest(&text, &lib.to_string_lossy())
            {
                games.push(g);
            }
        }
    }
    games.sort_by_key(|g| std::cmp::Reverse(g.size_bytes));
    // Same appid in two libraries (copied/moved install): keep the largest, wherever it sorts.
    let mut seen = std::collections::HashSet::new();
    games.retain(|g| seen.insert(g.appid));
    games
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_libraryfolders_and_manifest() {
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t}\n}\n";
        let paths: Vec<String> = vdf_pairs(vdf).into_iter().filter(|(k, _)| k == "path").map(|(_, v)| v).collect();
        assert_eq!(paths, vec![r"C:\Program Files (x86)\Steam".to_string(), r"D:\SteamLibrary".to_string()]);

        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"730\"\n\t\"name\"\t\t\"Counter-Strike 2\"\n\t\"SizeOnDisk\"\t\t\"64521134080\"\n}\n";
        let g = parse_manifest(acf, r"D:\SteamLibrary").unwrap();
        assert_eq!((g.appid, g.name.as_str(), g.size_bytes), (730, "Counter-Strike 2", 64_521_134_080));
        assert!(parse_manifest("\"appid\" \"228980\"\n\"name\" \"Steamworks Common Redistributables\"\n", "x").is_none());
        assert!(parse_manifest("garbage", "x").is_none());
        let _ = installed();
    }
}
