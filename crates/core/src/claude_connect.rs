//! Register G2 Boost's MCP server (`"<this exe>" --mcp`) with the user's Claude apps, on their
//! click: Claude Desktop (standard install and the Microsoft Store/MSIX one, whose config lives
//! in `%LOCALAPPDATA%\Claude-Data\claude_*` or, before the app moved it out of the package, under
//! `%LOCALAPPDATA%\Packages\Claude_*`; Cowork runs inside Desktop) and Claude Code (user scope,
//! top-level `mcpServers` in `~/.claude.json`). Since round 11 also Cursor (`~/.cursor/mcp.json`,
//! same shape as Desktop) and VS Code (`%APPDATA%\Code\User\mcp.json`, key `servers`); a VS Code
//! file with comments is not strict JSON, so it is refused (the UI shows the snippet to paste).
//!
//! Those files belong to other apps and are hostile input: parsed strictly, never rewritten when
//! they are not a JSON object we understand, never followed through a symlink, backed up to
//! `%LOCALAPPDATA%\G2 Boost\claude-backups` before every change and written atomically (temp file
//! + rename). Only our own `g2boost` entry is ever added or removed; key order is preserved.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value, json};

/// Our key in `mcpServers`.
pub const NAME: &str = "g2boost";
const MAX_BYTES: u64 = 16 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Client {
    Desktop,
    Code,
    Cursor,
    Vscode,
}

const ALL: [Client; 4] = [Client::Desktop, Client::Code, Client::Cursor, Client::Vscode];

#[derive(Debug, Clone, Serialize)]
pub struct Target {
    pub client: Client,
    pub path: String,
    /// The config file exists (Desktop: the app folder exists, the file is created on connect).
    pub exists: bool,
    /// Our entry is there.
    pub connected: bool,
    /// …and points at this exe (false = an older install path; connecting again fixes it).
    pub points_here: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("not_installed")]
    NotInstalled,
    #[error("unreadable_config")]
    Unparseable,
    #[error("io: {0}")]
    Io(String),
}

fn io(e: std::io::Error) -> ConnectError {
    ConnectError::Io(e.to_string())
}

/// Claude Desktop config files: the standard `%APPDATA%\Claude`, the MSIX data folders in
/// `%LOCALAPPDATA%\Claude-Data` and the old in-package MSIX folders the app still reads.
fn desktop_paths() -> Vec<PathBuf> {
    let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
    desktop_paths_in(var("APPDATA"), var("LOCALAPPDATA"))
}

pub(crate) fn desktop_paths_in(appdata: Option<PathBuf>, local: Option<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(appdata) = appdata {
        let dir = appdata.join("Claude");
        if dir.is_dir() {
            out.push(dir.join("claude_desktop_config.json"));
        }
    }
    let Some(local) = local else { return out };
    // The MSIX app keeps its data outside the package since 2026-10: one folder per package
    // family, lower case (`claude_pzs8sxrjxfjjc`).
    if let Ok(rd) = std::fs::read_dir(local.join("Claude-Data")) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().to_ascii_lowercase().starts_with("claude_") && e.path().is_dir() {
                out.push(e.path().join("claude_desktop_config.json"));
            }
        }
    }
    if let Ok(rd) = std::fs::read_dir(local.join("Packages")) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with("Claude_") {
                let dir = e.path().join(r"LocalCache\Roaming\Claude");
                if dir.is_dir() && !data_moved_out(&dir) {
                    out.push(dir.join("claude_desktop_config.json"));
                }
            }
        }
    }
    out
}

/// The MSIX app moved its data out of this folder and left a `.user-data-left.<family>` marker;
/// a config written here is never read again.
fn data_moved_out(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .is_ok_and(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().starts_with(".user-data-left.")))
}

/// `~/.claude.json`, only when Claude Code already created it.
fn code_path() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("USERPROFILE")?).join(".claude.json");
    p.is_file().then_some(p)
}

/// `<dir>\<file>` when the app's folder exists (the file itself may be created on connect).
fn in_dir(base: &str, dir: &str, file: &str) -> Option<PathBuf> {
    let d = PathBuf::from(std::env::var_os(base)?).join(dir);
    d.is_dir().then(|| d.join(file))
}

fn paths(client: Client) -> Vec<PathBuf> {
    match client {
        Client::Desktop => desktop_paths(),
        Client::Code => code_path().into_iter().collect(),
        Client::Cursor => in_dir("USERPROFILE", ".cursor", "mcp.json").into_iter().collect(),
        Client::Vscode => in_dir("APPDATA", r"Code\User", "mcp.json").into_iter().collect(),
    }
}

/// The object that holds the servers.
fn key(client: Client) -> &'static str {
    if client == Client::Vscode { "servers" } else { "mcpServers" }
}

fn entry(client: Client, exe: &Path) -> Value {
    let command = exe.to_string_lossy();
    match client {
        Client::Desktop | Client::Cursor => json!({ "command": command, "args": ["--mcp"] }),
        Client::Vscode => json!({ "type": "stdio", "command": command, "args": ["--mcp"] }),
        // The shape `claude mcp add-json` writes.
        Client::Code => json!({ "type": "stdio", "command": command, "args": ["--mcp"], "env": {} }),
    }
}

/// The file as a JSON object; `Ok(None)` when it does not exist.
fn read_object(path: &Path) -> Result<Option<Map<String, Value>>, ConnectError> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io(e)),
    };
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_BYTES {
        return Err(ConnectError::Unparseable);
    }
    let text = std::fs::read_to_string(path).map_err(|_| ConnectError::Unparseable)?;
    // An empty file is a fresh config (Claude Desktop creates one sometimes).
    if text.trim().is_empty() {
        return Ok(Some(Map::new()));
    }
    match serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
        Ok(Value::Object(m)) => Ok(Some(m)),
        _ => Err(ConnectError::Unparseable),
    }
}

fn servers<'a>(root: &'a Map<String, Value>, key: &str) -> Result<Option<&'a Map<String, Value>>, ConnectError> {
    match root.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(m)) => Ok(Some(m)),
        Some(_) => Err(ConnectError::Unparseable),
    }
}

fn same_exe(v: &Value, exe: &Path) -> bool {
    v.get("command").and_then(Value::as_str).is_some_and(|c| c.eq_ignore_ascii_case(&exe.to_string_lossy()))
}

/// (connected, points_here) for one file.
pub fn status_file(path: &Path, client: Client, exe: &Path) -> (bool, bool) {
    match read_object(path) {
        Ok(Some(root)) => match servers(&root, key(client)) {
            Ok(Some(s)) => s.get(NAME).map_or((false, false), |e| (true, same_exe(e, exe))),
            _ => (false, false),
        },
        _ => (false, false),
    }
}

/// Copy the original to the backup folder before touching it.
fn backup(path: &Path) -> Result<(), ConnectError> {
    let dir = crate::registry::data_dir().join("claude-backups");
    std::fs::create_dir_all(&dir).map_err(io)?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let parent = path.parent().and_then(Path::file_name).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = format!("{parent}-{stem}-{}.json", crate::registry::now_unix());
    std::fs::copy(path, dir.join(name)).map_err(io)?;
    Ok(())
}

fn write_atomic(path: &Path, root: &Map<String, Value>) -> Result<(), ConnectError> {
    let text = serde_json::to_string_pretty(root).map_err(|e| ConnectError::Io(e.to_string()))?;
    let tmp = path.with_extension("json.g2boost-tmp");
    std::fs::write(&tmp, text + "\n").map_err(io)?;
    // `rename` replaces the target on Windows (MoveFileExW + REPLACE_EXISTING).
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io(e)
    })
}

/// Add (or repoint) our entry in one file. `create` = a missing file may be created (Desktop).
pub fn connect_file(path: &Path, client: Client, exe: &Path, create: bool) -> Result<(), ConnectError> {
    let existing = read_object(path)?;
    let existed = existing.is_some();
    let mut root = match existing {
        Some(r) => r,
        None if create => Map::new(),
        None => return Err(ConnectError::NotInstalled),
    };
    let want = entry(client, exe);
    if servers(&root, key(client))?.and_then(|s| s.get(NAME)) == Some(&want) {
        return Ok(()); // already exactly this
    }
    let servers = root.entry(key(client)).or_insert_with(|| Value::Object(Map::new()));
    if servers.is_null() {
        *servers = Value::Object(Map::new());
    }
    let Value::Object(servers) = servers else {
        return Err(ConnectError::Unparseable);
    };
    servers.insert(NAME.into(), want);
    if existed {
        backup(path)?;
    }
    write_atomic(path, &root)
}

/// Remove our entry from one file (nothing else). Missing file or entry = nothing to do.
pub fn disconnect_file(path: &Path, client: Client) -> Result<(), ConnectError> {
    let Some(mut root) = read_object(path)? else {
        return Ok(());
    };
    if servers(&root, key(client))?.is_none_or(|s| !s.contains_key(NAME)) {
        return Ok(());
    }
    if let Some(Value::Object(s)) = root.get_mut(key(client)) {
        s.shift_remove(NAME);
    }
    backup(path)?;
    write_atomic(path, &root)
}

/// Every config of both clients found on this PC, with its state.
pub fn targets(exe: &Path) -> Vec<Target> {
    ALL.into_iter()
        .flat_map(|c| {
            let found = paths(c);
            if found.is_empty() {
                // Not installed: one row so the UI can say so.
                return vec![Target { client: c, path: String::new(), exists: false, connected: false, points_here: false }];
            }
            found
                .into_iter()
                .map(|p| {
                    let (connected, points_here) = status_file(&p, c, exe);
                    let exists = p.is_file() || c != Client::Code;
                    Target { client: c, path: p.to_string_lossy().into_owned(), exists, connected, points_here }
                })
                .collect()
        })
        .collect()
}

/// Connect every config of `client` found on this PC.
pub fn connect(client: Client, exe: &Path) -> Result<Vec<Target>, ConnectError> {
    let found = paths(client);
    if found.is_empty() {
        return Err(ConnectError::NotInstalled);
    }
    for p in &found {
        connect_file(p, client, exe, client != Client::Code)?;
    }
    Ok(targets(exe))
}

/// Repoint our entry in `path` at `exe` when it names an exe that no longer exists (the app was
/// reinstalled to another folder, e.g. `C:\Program Files` → `G:\G2 Boost`): the connection the
/// user made keeps working. An entry naming another exe that still exists (a second install) is
/// left alone. `true` = rewritten (backed up first, like any connect).
pub fn repair_file(path: &Path, client: Client, exe: &Path) -> bool {
    let stale = match read_object(path) {
        Ok(Some(root)) => matches!(servers(&root, key(client)), Ok(Some(s)) if s.get(NAME).is_some_and(|e| {
            !same_exe(e, exe) && e.get("command").and_then(Value::as_str).is_some_and(|c| !Path::new(c).exists())
        })),
        _ => false,
    };
    stale && connect_file(path, client, exe, false).is_ok()
}

/// [`repair_file`] over every config found; how many were repointed.
pub fn repair_stale(exe: &Path) -> usize {
    ALL.into_iter().flat_map(|c| paths(c).into_iter().map(move |p| (c, p))).filter(|(c, p)| repair_file(p, *c, exe)).count()
}

pub fn disconnect(client: Client, exe: &Path) -> Result<Vec<Target>, ConnectError> {
    for p in paths(client) {
        disconnect_file(&p, client)?;
    }
    Ok(targets(exe))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> PathBuf {
        // SAFETY: same override every data-dir test in this crate uses (process-wide env var).
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
        let d = std::env::temp_dir().join(format!("g2boost-claude-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_moved_install_is_repointed_but_a_second_live_one_is_not() {
        let d = sandbox("repair");
        let f = d.join(".claude.json");
        let exe = d.join("G2 Boost.exe");
        std::fs::write(&exe, b"").unwrap();
        // 2026-11-14: the entry still named C:\Program Files after a reinstall on G:.
        let gone = d.join("old").join("G2 Boost.exe");
        std::fs::write(&f, json!({ "a": 1, "mcpServers": { "g2boost": { "type": "stdio", "command": gone.to_string_lossy(), "args": ["--mcp"], "env": {} } } }).to_string()).unwrap();
        assert!(repair_file(&f, Client::Code, &exe));
        assert_eq!(status_file(&f, Client::Code, &exe), (true, true));
        assert!(serde_json::from_str::<Value>(&std::fs::read_to_string(&f).unwrap()).unwrap()["a"] == 1, "the rest kept");
        assert!(!repair_file(&f, Client::Code, &exe), "already here: nothing to do");
        // Another exe that exists = a second install the user may want: untouched.
        let other = d.join("other.exe");
        std::fs::write(&other, b"").unwrap();
        connect_file(&f, Client::Code, &other, false).unwrap();
        assert!(!repair_file(&f, Client::Code, &exe));
        assert_eq!(status_file(&f, Client::Code, &exe), (true, false));
        // No entry: never added by a repair.
        let empty = d.join("empty.json");
        std::fs::write(&empty, "{}").unwrap();
        assert!(!repair_file(&empty, Client::Code, &exe));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn connect_keeps_everything_else_and_is_idempotent() {
        let d = sandbox("keep");
        let f = d.join(".claude.json");
        let original = r#"{"zeta":1,"mcpServers":{"other":{"command":"x"}},"alpha":{"nested":[1,2]}}"#;
        std::fs::write(&f, original).unwrap();
        let exe = Path::new(r"C:\Program Files\G2 Boost\G2 Boost.exe");

        connect_file(&f, Client::Code, exe, false).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["zeta", "mcpServers", "alpha"], "key order preserved");
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["mcpServers"][NAME]["type"], "stdio");
        assert_eq!(v["mcpServers"][NAME]["args"][0], "--mcp");
        assert_eq!(status_file(&f, Client::Code, exe), (true, true));
        assert_eq!(status_file(&f, Client::Code, Path::new(r"D:\old\G2 Boost.exe")), (true, false));

        let before = std::fs::read_to_string(&f).unwrap();
        connect_file(&f, Client::Code, exe, false).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), before, "second connect changes nothing");

        disconnect_file(&f, Client::Code).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert!(v["mcpServers"].get(NAME).is_none());
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["alpha"]["nested"][1], 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn vscode_and_cursor_shapes() {
        let d = sandbox("vscode");
        let exe = Path::new(r"C:\G2\G2 Boost.exe");
        let f = d.join("mcp.json");
        std::fs::write(&f, r#"{"servers":{"gh":{"type":"http","url":"https://x"}},"inputs":[]}"#).unwrap();
        connect_file(&f, Client::Vscode, exe, true).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(v["servers"][NAME]["type"], "stdio");
        assert_eq!(v["servers"]["gh"]["url"], "https://x");
        assert!(v.get("mcpServers").is_none() && v["inputs"].is_array());
        assert_eq!(status_file(&f, Client::Vscode, exe), (true, true));
        assert_eq!(status_file(&f, Client::Cursor, exe), (false, false), "Cursor reads mcpServers");
        disconnect_file(&f, Client::Vscode).unwrap();
        assert!(serde_json::from_str::<Value>(&std::fs::read_to_string(&f).unwrap()).unwrap()["servers"].get(NAME).is_none());
        // VS Code allows comments; a rewrite would drop them, so such a file is refused untouched.
        let jsonc = "// mine\n{\"servers\":{}}";
        std::fs::write(&f, jsonc).unwrap();
        assert!(matches!(connect_file(&f, Client::Vscode, exe, true), Err(ConnectError::Unparseable)));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), jsonc);
        let c = d.join("cursor.json");
        connect_file(&c, Client::Cursor, exe, true).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&c).unwrap()).unwrap();
        assert_eq!(v["mcpServers"][NAME]["args"][0], "--mcp");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn hostile_or_missing_files() {
        let d = sandbox("hostile");
        let exe = Path::new(r"C:\G2\G2 Boost.exe");
        // Not JSON / not an object / mcpServers of the wrong type: refused, file untouched.
        for (i, bad) in ["{not json", "[1,2]", r#"{"mcpServers":"x"}"#].iter().enumerate() {
            let f = d.join(format!("bad{i}.json"));
            std::fs::write(&f, bad).unwrap();
            assert!(matches!(connect_file(&f, Client::Desktop, exe, true), Err(ConnectError::Unparseable)));
            assert!(matches!(disconnect_file(&f, Client::Desktop), Err(ConnectError::Unparseable) | Ok(())));
            assert_eq!(std::fs::read_to_string(&f).unwrap(), *bad);
        }
        // Claude Code without its file: not installed. Desktop: the file is created.
        let missing = d.join("missing.json");
        assert!(matches!(connect_file(&missing, Client::Code, exe, false), Err(ConnectError::NotInstalled)));
        connect_file(&missing, Client::Desktop, exe, true).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&missing).unwrap()).unwrap();
        assert_eq!(v["mcpServers"][NAME]["command"], r"C:\G2\G2 Boost.exe");
        assert!(v["mcpServers"][NAME].get("type").is_none(), "Desktop shape");
        // BOM and an empty file are accepted.
        let bom = d.join("bom.json");
        std::fs::write(&bom, "\u{feff}{\"a\":1}").unwrap();
        connect_file(&bom, Client::Desktop, exe, true).unwrap();
        assert_eq!(status_file(&bom, Client::Desktop, exe), (true, true));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn desktop_paths_follow_the_msix_data_move() {
        let d = sandbox("paths");
        let (appdata, local) = (d.join("Roaming"), d.join("Local"));
        // Data moved out: Claude-Data has the live folder, the old package folder keeps the marker.
        let live = local.join("Claude-Data").join("claude_pzs8sxrjxfjjc");
        let left = local.join("Packages").join("Claude_pzs8sxrjxfjjc").join(r"LocalCache\Roaming\Claude");
        // Another install that never moved: still read from the package.
        let kept = local.join("Packages").join("Claude_other").join(r"LocalCache\Roaming\Claude");
        for dir in [&live, &left, &kept, &local.join("Claude-Data").join("not-claude")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(left.join(".user-data-left.claude_pzs8sxrjxfjjc"), "{}").unwrap();
        std::fs::write(local.join("Claude-Data").join("README.txt"), "x").unwrap();

        let got = desktop_paths_in(Some(appdata.clone()), Some(local.clone()));
        let want = [live.join("claude_desktop_config.json"), kept.join("claude_desktop_config.json")];
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(want.iter().all(|w| got.contains(w)), "{got:?}");

        // Standard install: %APPDATA%\Claude comes first.
        std::fs::create_dir_all(appdata.join("Claude")).unwrap();
        let got = desktop_paths_in(Some(appdata.clone()), None);
        assert_eq!(got, [appdata.join("Claude").join("claude_desktop_config.json")]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
