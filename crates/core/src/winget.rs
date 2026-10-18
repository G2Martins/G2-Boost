//! WinGet through the official `winget.exe` (App Installer), called directly with a fixed
//! argument list: never through `cmd` or PowerShell, never with a user-built command line. The
//! COM API is out of reach for an unpackaged exe (no proxy for `IPackageManager` outside App
//! Installer's package; see `Docs/core/console.md`).
//!
//! Ids and queries are hostile input (console, MCP) and are validated before they become argv.
//! Install and upgrade change the PC: callers run them only after a confirmed preview (console:
//! single-use token; MCP: a native confirmation dialog). Every install/upgrade is logged.
//!
//! Output is a text table aligned by the header (localized; UTF-8, OEM code page as fallback): each
//! line is decoded first, then cut at the header's column starts counted in characters.

use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::Globalization::{CP_OEMCP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS, MultiByteToWideChar};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const COMMON: &[&str] = &["--source", "winget", "--accept-source-agreements", "--disable-interactivity"];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Package {
    pub id: String,
    pub name: String,
    /// Installed version (upgrade list) or catalog version (search).
    pub version: String,
    /// Newer version available (upgrade list only).
    pub available: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    pub ok: bool,
    /// winget's exit code (0 = success; `0x8A15…` = winget error).
    pub code: u32,
    /// winget's own last message line (localized), for the user.
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum WingetError {
    /// App Installer (winget) not installed for this user.
    #[error("winget_unavailable")]
    Unavailable,
    #[error("invalid_id")]
    InvalidId,
    #[error("invalid_query")]
    InvalidQuery,
    #[error("not_found")]
    NotFound,
    #[error("timeout")]
    Timeout,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// winget ids: `Publisher.App`, `Valve.Steam`, Store ids like `9NBLGGH4NNS1`.
pub fn valid_id(id: &str) -> bool {
    (2..=128).contains(&id.len())
        && !id.starts_with(['-', '.'])
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}

/// A search term: printable, single line, bounded, and never read as a flag.
pub fn valid_query(q: &str) -> bool {
    (1..=64).contains(&q.chars().count()) && !q.starts_with('-') && !q.chars().any(char::is_control)
}

/// The App Execution Alias App Installer puts on the user's PATH.
fn exe() -> Result<PathBuf, WingetError> {
    let p = PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or(WingetError::Unavailable)?)
        .join(r"Microsoft\WindowsApps\winget.exe");
    // An execution alias is a reparse point: check the link itself, not its target.
    std::fs::symlink_metadata(&p).map(|_| p).map_err(|_| WingetError::Unavailable)
}

/// Run winget with `args`, no window, stdin closed; stdout bounded; killed after `limit`.
fn run(args: &[&str], limit: Duration) -> Result<(u32, Vec<u8>), WingetError> {
    let mut child = Command::new(exe()?)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut out = child.stdout.take().ok_or(WingetError::Unavailable)?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut out).take(4 << 20).read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(WingetError::Timeout);
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let buf = reader.join().unwrap_or_default();
    Ok((status.code().unwrap_or(-1) as u32, buf))
}

/// Visible lines: progress spinners rewrite the line with `\r`; keep what was left on screen.
fn lines(raw: &[u8]) -> Vec<&[u8]> {
    raw.split(|b| *b == b'\n')
        .map(|l| {
            let l = l.strip_suffix(b"\r").unwrap_or(l);
            l.rsplit(|b| *b == b'\r').next().unwrap_or(l)
        })
        .collect()
}

/// UTF-8 when valid, else the OEM code page (older winget builds wrote that when redirected).
fn decode(line: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(line) {
        return s.to_string();
    }
    // SAFETY: size probe, then a buffer of exactly that many UTF-16 units.
    unsafe {
        let n = MultiByteToWideChar(CP_OEMCP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), line, None);
        if n <= 0 {
            return String::from_utf8_lossy(line).into_owned();
        }
        let mut w = vec![0u16; n as usize];
        MultiByteToWideChar(CP_OEMCP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), line, Some(&mut w));
        String::from_utf16_lossy(&w)
    }
}

/// The rows of winget's table, cut at the header's column starts (in characters).
pub(crate) fn table(raw: &[u8]) -> Vec<Vec<String>> {
    let ls: Vec<Vec<char>> = lines(raw).iter().map(|l| decode(l).chars().collect()).collect();
    let Some(dash) = ls.iter().position(|l| l.len() >= 10 && l.iter().all(|c| *c == '-')) else {
        return Vec::new();
    };
    let Some(header) = dash.checked_sub(1).and_then(|i| ls.get(i)) else {
        return Vec::new();
    };
    let starts: Vec<usize> =
        (0..header.len()).filter(|&i| header[i] != ' ' && (i == 0 || header[i - 1] == ' ')).collect();
    if starts.len() < 2 {
        return Vec::new();
    }
    ls[dash + 1..]
        .iter()
        .take_while(|l| !l.is_empty())
        .take(500)
        .map(|row| {
            starts
                .iter()
                .enumerate()
                .map(|(k, &s)| {
                    let e = starts.get(k + 1).copied().unwrap_or(row.len()).min(row.len());
                    if s >= e { String::new() } else { row[s..e].iter().collect::<String>().trim().to_string() }
                })
                .collect()
        })
        .collect()
}

fn packages(raw: &[u8], with_available: bool) -> Vec<Package> {
    table(raw)
        .into_iter()
        .filter(|r| r.len() >= 3 && valid_id(&r[1]))
        .map(|r| Package {
            name: r[0].clone(),
            id: r[1].clone(),
            version: r[2].clone(),
            available: with_available.then(|| r.get(3).cloned().unwrap_or_default()).filter(|v| !v.is_empty()),
        })
        .collect()
}

/// Search the winget catalog (name, id, moniker, tag), up to 50 results.
pub fn search(query: &str) -> Result<Vec<Package>, WingetError> {
    let q = query.trim();
    if !valid_query(q) {
        return Err(WingetError::InvalidQuery);
    }
    let mut args = vec!["search", "--query", q];
    args.extend_from_slice(COMMON);
    let (_, out) = run(&args, Duration::from_secs(90))?;
    let mut p = packages(&out, false);
    p.truncate(50);
    Ok(p)
}

/// Installed apps with a newer version in the winget catalog.
pub fn upgrades() -> Result<Vec<Package>, WingetError> {
    let mut args = vec!["upgrade"];
    args.extend_from_slice(COMMON);
    let (_, out) = run(&args, Duration::from_secs(120))?;
    Ok(packages(&out, true))
}

/// The one catalog package with exactly this id (the preview before installing).
pub fn exact(id: &str) -> Result<Package, WingetError> {
    if !valid_id(id) {
        return Err(WingetError::InvalidId);
    }
    let mut args = vec!["search", "--id", id, "--exact"];
    args.extend_from_slice(COMMON);
    let (_, out) = run(&args, Duration::from_secs(90))?;
    packages(&out, false).into_iter().find(|p| p.id.eq_ignore_ascii_case(id)).ok_or(WingetError::NotFound)
}

/// Install (`upgrade = false`) or upgrade one package, silently. Blocks until winget finishes
/// (the installer itself may ask for administrator rights). Logged in `winget-log.json`.
pub fn install(id: &str, upgrade: bool) -> Result<Outcome, WingetError> {
    if !valid_id(id) {
        return Err(WingetError::InvalidId);
    }
    let verb = if upgrade { "upgrade" } else { "install" };
    let mut args = vec![verb, "--id", id, "--exact", "--silent", "--accept-package-agreements"];
    args.extend_from_slice(COMMON);
    let (code, out) = run(&args, Duration::from_secs(45 * 60))?;
    let message = lines(&out)
        .iter()
        .rev()
        .map(|l| decode(l).trim().to_string())
        .find(|l| !l.is_empty() && !l.chars().all(|c| matches!(c, '-' | '\\' | '|' | '/' | ' ')))
        .unwrap_or_default();
    let outcome = Outcome { ok: code == 0, code, message: message.chars().take(200).collect() };
    log(verb, id, &outcome);
    Ok(outcome)
}

fn log(verb: &str, id: &str, o: &Outcome) {
    let f = crate::registry::data_dir().join("winget-log.json");
    let mut all: Vec<serde_json::Value> =
        std::fs::read_to_string(&f).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    all.push(serde_json::json!({ "at": crate::registry::now_unix(), "action": verb, "id": id, "ok": o.ok, "code": o.code }));
    let n = all.len();
    if n > 200 {
        all.drain(..n - 200);
    }
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, serde_json::to_string_pretty(&all).unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_queries_are_validated() {
        for ok in ["Valve.Steam", "Microsoft.VisualStudioCode", "9NBLGGH4NNS1", "Git.Git", "Notepad++.Notepad++"] {
            assert!(valid_id(ok), "{ok}");
        }
        for bad in ["", "a", "-x", ".x", "a b", "a;b", "a&b", "x\"y", "a\\b", "--force", &"x".repeat(129)] {
            assert!(!valid_id(bad), "{bad}");
        }
        assert!(valid_query("visual studio code"));
        assert!(!valid_query(""));
        assert!(!valid_query("--force"), "never a flag");
        assert!(!valid_query("a\nb"));
        assert!(!valid_query(&"x".repeat(65)));
    }

    #[test]
    fn parses_the_real_table_shape() {
        // Real shape (winget 1.29, pt-BR): UTF-8 header with accents, spinner rewrite, footer line.
        let raw = "   - \r   \\ \rNome            ID                  Versão       Disponível\r\n\
------------------------------------------------------------\r\n\
Claude          Anthropic.Claude    2.16120.0.0  2.19675.1\r\n\
GitHub CLI      GitHub.cli          2.101.0      2.102.0\r\n\
2 atualizações disponíveis.\r\n"
            .as_bytes()
            .to_vec();
        let p = packages(&raw, true);
        assert_eq!(p.len(), 2, "{p:?}");
        assert_eq!(p[0], Package { id: "Anthropic.Claude".into(), name: "Claude".into(), version: "2.16120.0.0".into(), available: Some("2.19675.1".into()) });
        assert_eq!(p[1].name, "GitHub CLI");
        assert!(table(b"no table here").is_empty());
        // Older builds: OEM code page (CP850 "ã" = 0xC6) still decodes to one character.
        assert_eq!(decode(b"Vers\xC6o").chars().count(), 6);
        assert!(table(b"----------\r\nx").is_empty(), "no header");
    }

    /// Talks to the real winget: run by hand (`cargo test -- --ignored winget`).
    #[test]
    #[ignore]
    fn searches_the_real_catalog() {
        let found = search("steam").expect("search");
        assert!(found.iter().any(|p| p.id == "Valve.Steam"), "{found:?}");
        assert_eq!(exact("Valve.Steam").expect("exact").id, "Valve.Steam");
        assert!(matches!(exact("No.Such.Package.G2"), Err(WingetError::NotFound)));
        println!("{:?}", upgrades().expect("upgrades"));
    }
}
