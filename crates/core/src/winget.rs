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

/// App Installer's package family: winget's alias must belong to it.
const FAMILY: &str = "Microsoft.DesktopAppInstaller_8wekyb3d8bbwe";

/// (package family, app id, target exe) of an `IO_REPARSE_TAG_APPEXECLINK` buffer as
/// `FSCTL_GET_REPARSE_POINT` returns it: tag, data length, reserved, then version 3 and
/// NUL-terminated UTF-16 strings (a fourth one, the app type, is ignored).
pub(crate) fn parse_app_exec_link(buf: &[u8]) -> Option<(String, String, String)> {
    const IO_REPARSE_TAG_APPEXECLINK: u32 = 0x8000_001B;
    let u32_at = |b: &[u8], i: usize| Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?));
    if u32_at(buf, 0)? != IO_REPARSE_TAG_APPEXECLINK {
        return None;
    }
    let len = usize::from(u16::from_le_bytes(buf.get(4..6)?.try_into().ok()?));
    let data = buf.get(8..8 + len)?;
    if u32_at(data, 0)? != 3 {
        return None;
    }
    let units: Vec<u16> = data[4..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let mut it = units.split(|&u| u == 0).map(|s| String::from_utf16(s).ok().filter(|s| !s.is_empty()));
    Some((it.next()??, it.next()??, it.next()??))
}

/// App Installer's own alias: its family and app, and a target `winget.exe` directly inside its
/// package folder under `<program_files>\WindowsApps` (only TrustedInstaller writes there).
pub(crate) fn alias_ok(link: &(String, String, String), program_files: &std::path::Path) -> bool {
    let (family, app, target) = link;
    let (t, root) = (target.to_ascii_lowercase(), format!(r"{}\windowsapps\", program_files.to_string_lossy().trim_end_matches('\\').to_ascii_lowercase()));
    let in_package = t.strip_prefix(&root).and_then(|rest| rest.split_once('\\')).is_some_and(|(dir, file)| {
        dir.starts_with("microsoft.desktopappinstaller_") && dir.ends_with("_8wekyb3d8bbwe") && file == "winget.exe"
    });
    family.eq_ignore_ascii_case(FAMILY)
        && app.to_ascii_lowercase().starts_with(&format!("{}!", FAMILY.to_ascii_lowercase()))
        && !t.contains("..")
        && !t.contains('/')
        && in_package
}

/// The App Execution Alias App Installer puts on the user's PATH, returned with a handle that keeps
/// it from being changed, renamed or deleted until winget has started. That folder is the user's
/// (any program can drop a `winget.exe` there), and G2 Boost usually runs elevated: only App
/// Installer's own alias is ever started.
fn exe() -> Result<(PathBuf, std::fs::File), WingetError> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::Win32::System::Ioctl::FSCTL_GET_REPARSE_POINT;
    const FILE_SHARE_READ: u32 = 1;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let p = PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or(WingetError::Unavailable)?)
        .join(r"Microsoft\WindowsApps\winget.exe");
    let pf = crate::registry::known_folder(&windows::Win32::UI::Shell::FOLDERID_ProgramFiles).ok_or(WingetError::Unavailable)?;
    let f = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&p)
        .map_err(|_| WingetError::Unavailable)?;
    let mut buf = vec![0u8; 16 * 1024];
    let mut n = 0u32;
    // SAFETY: valid handle owned by `f`; the output buffer and its length match.
    unsafe {
        DeviceIoControl(
            windows::Win32::Foundation::HANDLE(f.as_raw_handle()),
            FSCTL_GET_REPARSE_POINT,
            None,
            0,
            Some(buf.as_mut_ptr().cast()),
            buf.len() as u32,
            Some(&mut n),
            None,
        )
    }
    .map_err(|_| WingetError::Unavailable)?;
    buf.truncate(n as usize);
    parse_app_exec_link(&buf).filter(|l| alias_ok(l, &pf)).ok_or(WingetError::Unavailable)?;
    Ok((p, f))
}

/// Run winget with `args`, no window, stdin closed; stdout bounded; killed after `limit`.
fn run(args: &[&str], limit: Duration) -> Result<(u32, Vec<u8>), WingetError> {
    let (exe, alias) = exe()?;
    let mut child = Command::new(exe)
        .args(args)
        .creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW.0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    drop(alias); // started: the checked alias may change now
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

/// One row of `winget list`: an installed app winget matched to its own catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct Listed {
    /// The catalog id. A cell cut short by the console width ends in `…`.
    pub id: String,
    /// The name Windows shows for the app (its Uninstall entry), also possibly cut with `…`.
    pub name: String,
    /// Installed version; winget prefixes inexact ones with `>`, `<` or `~` (removed here).
    pub version: String,
    /// A newer version in the catalog, when winget shows one.
    pub available: Option<String>,
}

/// Installed apps winget matched to its catalog (`winget list`), one entry per id. Read-only.
pub fn list() -> Result<Vec<Listed>, WingetError> {
    let mut args = vec!["list"];
    args.extend_from_slice(COMMON);
    let (_, out) = run(&args, Duration::from_secs(120))?;
    Ok(listed(&out))
}

/// `winget list` always shows Name, Id and Version; Available and Source appear only when some row
/// has one, and the headers are localized. So the columns are told apart by position and by what
/// they hold: a version has a digit, a source name ("winget") does not.
pub(crate) fn listed(raw: &[u8]) -> Vec<Listed> {
    let mut out: Vec<Listed> = Vec::new();
    for r in table(raw) {
        if r.len() < 3 {
            continue;
        }
        let id = r[1].trim();
        // `…` is how winget marks a cell it had to cut; the stem before it must still look like an id.
        if !valid_id(id.strip_suffix('…').unwrap_or(id)) {
            continue;
        }
        let available = r
            .get(3)
            .map(|c| c.trim())
            .filter(|c| c.chars().any(|ch| ch.is_ascii_digit()))
            .map(str::to_string);
        let row = Listed {
            id: id.to_string(),
            name: r[0].trim().to_string(),
            version: r[2].trim().trim_start_matches(['>', '<', '~', ' ']).to_string(),
            available,
        };
        // The same package can be listed more than once (one row per installed architecture).
        match out.iter_mut().find(|o| o.id.eq_ignore_ascii_case(&row.id)) {
            Some(o) => {
                if o.available.is_none() {
                    o.available = row.available;
                }
            }
            None => out.push(row),
        }
    }
    out
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

    /// A reparse buffer laid out like this PC's real alias (`fsutil reparsepoint query`, 2026-11-21).
    fn link_buffer(tag: u32, version: u32, strings: &[&str]) -> Vec<u8> {
        let mut data = version.to_le_bytes().to_vec();
        for s in strings {
            data.extend(s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes));
        }
        let mut b = tag.to_le_bytes().to_vec();
        b.extend((data.len() as u16).to_le_bytes());
        b.extend([0, 0]);
        b.extend(data);
        b
    }

    #[test]
    fn only_app_installers_own_alias_is_started() {
        const REAL: &str = r"C:\Program Files\WindowsApps\Microsoft.DesktopAppInstaller_1.29.380.0_x64__8wekyb3d8bbwe\winget.exe";
        let pf = std::path::Path::new(r"C:\Program Files");
        let app = "Microsoft.DesktopAppInstaller_8wekyb3d8bbwe!winget";
        let real = parse_app_exec_link(&link_buffer(0x8000_001B, 3, &[FAMILY, app, REAL, "0"])).unwrap();
        assert_eq!(real, (FAMILY.to_string(), app.to_string(), REAL.to_string()));
        assert!(alias_ok(&real, pf));
        assert!(alias_ok(&(FAMILY.to_lowercase(), app.to_uppercase(), REAL.to_lowercase()), pf), "any case");
        let with = |family: &str, app: &str, target: &str| (family.to_string(), app.to_string(), target.to_string());
        for (f, a, t) in [
            (FAMILY, app, r"C:\Users\me\AppData\Local\Microsoft\WindowsApps\evil.exe"),
            (FAMILY, app, r"C:\Program Files\WindowsApps\Evil.Pkg_1.0_x64__8wekyb3d8bbwe\winget.exe"),
            (FAMILY, app, r"C:\Program Files\WindowsApps\Microsoft.DesktopAppInstaller_1_x64__evilpub\winget.exe"),
            (FAMILY, app, r"C:\Program Files\WindowsApps\Microsoft.DesktopAppInstaller_1_x64__8wekyb3d8bbwe\sub\winget.exe"),
            (FAMILY, app, r"C:\Program Files\WindowsApps\Microsoft.DesktopAppInstaller_1_x64__8wekyb3d8bbwe\cmd.exe"),
            (FAMILY, app, r"C:\Program Files\WindowsApps\Microsoft.DesktopAppInstaller_..\..\..\Users\me_8wekyb3d8bbwe\winget.exe"),
            (FAMILY, app, r"C:\Program Files\WindowsApps\Microsoft.DesktopAppInstaller_x/../../e_8wekyb3d8bbwe\winget.exe"),
            (FAMILY, app, r"D:\Program Files\WindowsApps\Microsoft.DesktopAppInstaller_1_x64__8wekyb3d8bbwe\winget.exe"),
            ("Evil.Pkg_8wekyb3d8bbwe", app, REAL),
            (FAMILY, "Evil.Pkg_8wekyb3d8bbwe!winget", REAL),
        ] {
            assert!(!alias_ok(&with(f, a, t), pf), "{f} {a} {t}");
        }
        assert!(parse_app_exec_link(&link_buffer(0xA000_000C, 3, &[FAMILY, app, REAL])).is_none(), "a symlink is not an alias");
        assert!(parse_app_exec_link(&link_buffer(0x8000_001B, 2, &[FAMILY, app, REAL])).is_none());
        assert!(parse_app_exec_link(&link_buffer(0x8000_001B, 3, &[FAMILY, app])).is_none(), "no target");
        let full = link_buffer(0x8000_001B, 3, &[FAMILY, app, REAL]);
        assert!(parse_app_exec_link(&full[..full.len() - 10]).is_none(), "truncated");
        assert!(parse_app_exec_link(&[]).is_none());
    }

    /// This PC's real alias passes, and winget still starts while it is held open.
    #[test]
    fn the_real_alias_is_accepted_and_still_starts() {
        let alias = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join(r"Microsoft\WindowsApps\winget.exe");
        if std::fs::symlink_metadata(&alias).is_err() {
            return; // App Installer not installed for this user
        }
        assert!(exe().is_ok());
        assert_eq!(run(&["--version"], Duration::from_secs(30)).unwrap().0, 0);
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

    /// A table laid out the way winget does it: the header, a dash line, then the rows, each column
    /// padded to its widest cell (counted in characters). Short rows leave the last cells empty.
    fn grid(rows: &[&[&str]]) -> Vec<u8> {
        let width: Vec<usize> = (0..rows[0].len())
            .map(|c| rows.iter().map(|r| r.get(c).map_or(0, |s| s.chars().count())).max().unwrap_or(0) + 3)
            .collect();
        let line = |r: &[&str]| {
            let s: String = r.iter().enumerate().map(|(c, s)| format!("{s:<w$}", w = width[c])).collect();
            format!("{}\r\n", s.trim_end())
        };
        let mut out = line(rows[0]);
        out.push_str(&format!("{}\r\n", "-".repeat(width.iter().sum::<usize>() - 3)));
        for r in &rows[1..] {
            out.push_str(&line(r));
        }
        out.into_bytes()
    }

    #[test]
    fn list_reads_the_pt_br_table_with_an_available_column() {
        // Shape of this PC's real `winget list --source winget` (winget 1.29, pt-BR): no Source
        // column, Available only on the rows with an update, repeated rows, "> " on inexact versions.
        let raw = grid(&[
            &["Nome", "ID", "Versão", "Disponível"],
            &["Claude", "Anthropic.Claude", "3.6556.2.0"],
            &["DirectX", "Microsoft.DirectX", "9.29.1974.0"],
            &["DirectX", "Microsoft.DirectX", "9.29.1974.0"],
            &["WindowsAppRuntime.1.8", "Microsoft.WindowsAppRuntime.1.8", "> 1.8.10"],
            &["Git", "Git.Git", "2.56.0.2", "2.56.0.3"],
            &["WinRAR 7.23 (64-bit)", "RARLab.WinRAR", "7.23.0"],
        ]);
        let l = listed(&raw);
        let ids: Vec<&str> = l.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["Anthropic.Claude", "Microsoft.DirectX", "Microsoft.WindowsAppRuntime.1.8", "Git.Git", "RARLab.WinRAR"]);
        assert_eq!(l[0], Listed { id: "Anthropic.Claude".into(), name: "Claude".into(), version: "3.6556.2.0".into(), available: None });
        assert_eq!(l[2].version, "1.8.10", "the inexact-version marker is dropped");
        assert_eq!((l[3].version.as_str(), l[3].available.as_deref()), ("2.56.0.2", Some("2.56.0.3")));
        assert_eq!(l[4].name, "WinRAR 7.23 (64-bit)");
    }

    #[test]
    fn list_tells_available_from_source_by_content_in_any_language() {
        // 4 columns, the last one is Source (nothing to update): its value has no digit.
        let with_source = grid(&[
            &["Name", "Id", "Version", "Source"],
            &["Git", "Git.Git", "2.56.0.2", "winget"],
            &["Steam", "Valve.Steam", "2.10", "winget"],
        ]);
        assert!(listed(&with_source).iter().all(|r| r.available.is_none()));
        // 3 columns: nothing to update, no source column.
        let plain = grid(&[&["Name", "Id", "Version"], &["Git", "Git.Git", "2.56.0.2"]]);
        assert_eq!(listed(&plain).len(), 1);
        // 5 columns in German and French: Available is cell 3 and Source cell 4, empty cells allowed.
        for head in [["Name", "ID", "Version", "Verfügbar", "Quelle"], ["Nom", "ID", "Version", "Disponible", "Source"]] {
            let raw = grid(&[
                &head,
                &["Git", "Git.Git", "2.56.0.2", "2.56.0.3", "winget"],
                &["Steam", "Valve.Steam", "2.10", "", "winget"],
                &["Discord", "Discord.Discord", "1.0.9", "", ""],
            ]);
            let l = listed(&raw);
            assert_eq!(l.iter().map(|r| r.available.as_deref()).collect::<Vec<_>>(), [Some("2.56.0.3"), None, None], "{head:?}");
            assert_eq!(l[2].version, "1.0.9");
        }
    }

    #[test]
    fn list_survives_noise_cut_cells_and_hostile_rows() {
        let mut raw = b"   - \r   \\ \rFailed when searching source; results will not be included: msstore\r\n".to_vec();
        raw.extend(grid(&[
            &["Name", "Id", "Version"],
            &["Visual Studio Code Insid…", "Microsoft.VisualStudioCode.Insid…", "1.99.0"],
            &["bad", "evil id; calc", "1.0"],
            &["bad", "-rf", "1.0"],
            &["bad", ".hidden", "1.0"],
            &["bad", "…", "1.0"],
            &["bad", "x", "1.0"],
            &["fine", "Good.App", ""],
        ]));
        raw.extend_from_slice(b"\r\n3 packages installed.\r\n");
        let l = listed(&raw);
        assert_eq!(l.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["Microsoft.VisualStudioCode.Insid…", "Good.App"]);
        assert_eq!(l[1].version, "");
        assert!(listed(b"").is_empty() && listed(b"No installed package found.\r\n").is_empty());
        assert!(listed(&grid(&[&["Name", "Id", "Version"]])).is_empty(), "a header with no rows");
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
