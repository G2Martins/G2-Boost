//! Register G2 Boost's MCP server (`"<this exe>" --mcp`) with the user's Claude apps, on their
//! click: Claude Desktop (standard install and the Microsoft Store/MSIX one, whose config lives
//! under `%LOCALAPPDATA%\Packages\Claude_*`; Cowork runs inside Desktop) and Claude Code (user
//! scope, top-level `mcpServers` in `~/.claude.json`).
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
}

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

/// Claude Desktop config files: the standard `%APPDATA%\Claude` and every MSIX package folder.
fn desktop_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let dir = PathBuf::from(appdata).join("Claude");
        if dir.is_dir() {
            out.push(dir.join("claude_desktop_config.json"));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA")
        && let Ok(rd) = std::fs::read_dir(PathBuf::from(local).join("Packages"))
    {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with("Claude_") {
                let dir = e.path().join(r"LocalCache\Roaming\Claude");
                if dir.is_dir() {
                    out.push(dir.join("claude_desktop_config.json"));
                }
            }
        }
    }
    out
}

/// `~/.claude.json`, only when Claude Code already created it.
fn code_path() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("USERPROFILE")?).join(".claude.json");
    p.is_file().then_some(p)
}

fn paths(client: Client) -> Vec<PathBuf> {
    match client {
        Client::Desktop => desktop_paths(),
        Client::Code => code_path().into_iter().collect(),
    }
}

fn entry(client: Client, exe: &Path) -> Value {
    let command = exe.to_string_lossy();
    match client {
        Client::Desktop => json!({ "command": command, "args": ["--mcp"] }),
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

fn servers(root: &Map<String, Value>) -> Result<Option<&Map<String, Value>>, ConnectError> {
    match root.get("mcpServers") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(m)) => Ok(Some(m)),
        Some(_) => Err(ConnectError::Unparseable),
    }
}

fn same_exe(v: &Value, exe: &Path) -> bool {
    v.get("command").and_then(Value::as_str).is_some_and(|c| c.eq_ignore_ascii_case(&exe.to_string_lossy()))
}

/// (connected, points_here) for one file.
pub fn status_file(path: &Path, exe: &Path) -> (bool, bool) {
    match read_object(path) {
        Ok(Some(root)) => match servers(&root) {
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
    if servers(&root)?.and_then(|s| s.get(NAME)) == Some(&want) {
        return Ok(()); // already exactly this
    }
    let servers = root.entry("mcpServers").or_insert_with(|| Value::Object(Map::new()));
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
pub fn disconnect_file(path: &Path) -> Result<(), ConnectError> {
    let Some(mut root) = read_object(path)? else {
        return Ok(());
    };
    if servers(&root)?.is_none_or(|s| !s.contains_key(NAME)) {
        return Ok(());
    }
    if let Some(Value::Object(s)) = root.get_mut("mcpServers") {
        s.shift_remove(NAME);
    }
    backup(path)?;
    write_atomic(path, &root)
}

/// Every config of both clients found on this PC, with its state.
pub fn targets(exe: &Path) -> Vec<Target> {
    [Client::Desktop, Client::Code]
        .into_iter()
        .flat_map(|c| {
            let found = paths(c);
            if found.is_empty() {
                // Not installed: one row so the UI can say so.
                return vec![Target { client: c, path: String::new(), exists: false, connected: false, points_here: false }];
            }
            found
                .into_iter()
                .map(|p| {
                    let (connected, points_here) = status_file(&p, exe);
                    let exists = p.is_file() || c == Client::Desktop;
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
        connect_file(p, client, exe, client == Client::Desktop)?;
    }
    Ok(targets(exe))
}

pub fn disconnect(client: Client, exe: &Path) -> Result<Vec<Target>, ConnectError> {
    for p in paths(client) {
        disconnect_file(&p)?;
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
        assert_eq!(status_file(&f, exe), (true, true));
        assert_eq!(status_file(&f, Path::new(r"D:\old\G2 Boost.exe")), (true, false));

        let before = std::fs::read_to_string(&f).unwrap();
        connect_file(&f, Client::Code, exe, false).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), before, "second connect changes nothing");

        disconnect_file(&f).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert!(v["mcpServers"].get(NAME).is_none());
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["alpha"]["nested"][1], 2);
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
            assert!(matches!(disconnect_file(&f), Err(ConnectError::Unparseable) | Ok(())));
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
        assert_eq!(status_file(&bom, exe), (true, true));
        let _ = std::fs::remove_dir_all(&d);
    }
}
