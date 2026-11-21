//! Startup apps ("msconfig" done safely): list what starts with Windows and turn items off/on
//! exactly like Task Manager does — through the `StartupApproved` flags. Nothing is deleted,
//! the Run entry stays where it was, and turning it back on restores it. HKCU items need no
//! admin; machine-wide (HKLM / common Startup folder) items need elevation.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::registry::{self, Root, Value};
use crate::system;

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_WOW: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Location {
    /// HKCU Run (this user).
    UserRun,
    /// HKLM Run (all users, 64-bit view).
    MachineRun,
    /// HKLM WOW6432Node Run (all users, 32-bit apps).
    MachineRun32,
    /// %APPDATA%\…\Startup folder (this user).
    UserFolder,
    /// %ProgramData%\…\StartUp folder (all users).
    CommonFolder,
}

impl Location {
    fn approved(self) -> (Root, String) {
        match self {
            Location::UserRun => (Root::Hkcu, format!(r"{APPROVED}\Run")),
            Location::MachineRun => (Root::Hklm, format!(r"{APPROVED}\Run")),
            Location::MachineRun32 => (Root::Hklm, format!(r"{APPROVED}\Run32")),
            Location::UserFolder => (Root::Hkcu, format!(r"{APPROVED}\StartupFolder")),
            Location::CommonFolder => (Root::Hklm, format!(r"{APPROVED}\StartupFolder")),
        }
    }

    pub fn needs_admin(self) -> bool {
        !matches!(self, Location::UserRun | Location::UserFolder)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StartupItem {
    /// Run value name or shortcut file name — the key used by `StartupApproved`.
    pub name: String,
    /// Command line (Run) or shortcut path (folder). Data, never executed by us.
    pub command: String,
    pub location: Location,
    pub enabled: bool,
    pub needs_admin: bool,
    /// Security components (Windows Security tray): shown, never turned off by G2 Boost.
    pub protected: bool,
    /// The program it starts (shortcut target resolved), when it exists: its icon, and the link
    /// between this item and the process using RAM. `None` = unknown or gone.
    pub exe: Option<String>,
}

/// Security software (Windows Security tray + common third-party antivirus), matched lowercase on
/// the Run value name or command. Turning these off hides threat notifications or the AV's own
/// UI — out of scope for an optimizer, whoever asks (UI or AI).
const SECURITY_MARKERS: &[&str] = &[
    "securityhealth", "windows defender", "msascuil", "kaspersky", r"\avp.exe", "avpui.exe", "avast", r"\avg\",
    "avgui.exe", "bitdefender", "bdagent", r"\eset\", "egui.exe", "norton", "mcafee", "malwarebytes", "mbam", "sophos",
    "trend micro", "f-secure", "webroot", "avira", "comodo", "zonealarm", "panda security", "emsisoft", "drweb",
    "dr.web", "g data",
];

fn is_protected(name: &str, command: &str) -> bool {
    is_security_software(name) || is_security_software(command)
}

/// Name, command line or path that belongs to security software (also used by the process list).
pub(crate) fn is_security_software(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    SECURITY_MARKERS.iter().any(|m| t.contains(m))
}

/// Task Manager's encoding: first byte even (0x02/0x06) = enabled, odd (0x03/0x07) = disabled.
pub(crate) fn approved_enabled(v: &Value) -> bool {
    match v {
        Value::Binary(b) => b.first().is_none_or(|x| x % 2 == 0),
        _ => true,
    }
}

/// 12 bytes: flag + 3 zero bytes + FILETIME (time it was disabled; zero when enabled).
pub(crate) fn approved_blob(enabled: bool, now_filetime: u64) -> Vec<u8> {
    let mut v = vec![if enabled { 0x02 } else { 0x03 }, 0, 0, 0];
    v.extend(if enabled { 0u64 } else { now_filetime }.to_le_bytes());
    v
}

fn filetime_now() -> u64 {
    registry::unix_to_filetime(registry::now_unix())
}

fn state(location: Location, name: &str) -> bool {
    let (root, path) = location.approved();
    registry::read(root, &path, name).map(|v| approved_enabled(&v)).unwrap_or(true)
}

fn run_items(root: Root, path: &str, location: Location, out: &mut Vec<StartupItem>) {
    for name in registry::value_names(root, path).unwrap_or_default() {
        if name.is_empty() {
            continue;
        }
        let command = match registry::read(root, path, &name) {
            Ok(Value::Str(s)) => s,
            _ => continue,
        };
        out.push(StartupItem {
            enabled: state(location, &name),
            needs_admin: location.needs_admin(),
            protected: is_protected(&name, &command),
            exe: program_of(&command).and_then(|p| existing(&updater_target(&p, &command).unwrap_or(p))),
            name,
            command,
            location,
        });
    }
}

fn folder(location: Location) -> Option<PathBuf> {
    let (var, rel) = match location {
        Location::UserFolder => ("APPDATA", r"Microsoft\Windows\Start Menu\Programs\Startup"),
        Location::CommonFolder => ("ProgramData", r"Microsoft\Windows\Start Menu\Programs\StartUp"),
        _ => return None,
    };
    std::env::var_os(var).map(|b| PathBuf::from(b).join(rel))
}

fn folder_items(location: Location, out: &mut Vec<StartupItem>) {
    let Some(dir) = folder(location) else {
        return;
    };
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.eq_ignore_ascii_case("desktop.ini") || e.path().is_dir() {
            continue;
        }
        let command = e.path().to_string_lossy().into_owned();
        let lower = name.to_ascii_lowercase();
        let exe = if lower.ends_with(".lnk") {
            crate::apps::lnk_target(&e.path()).and_then(|t| existing(&t.to_string_lossy()))
        } else if lower.ends_with(".exe") {
            existing(&command)
        } else {
            None
        };
        out.push(StartupItem {
            enabled: state(location, &name),
            needs_admin: location.needs_admin(),
            protected: is_protected(&name, &command),
            exe,
            command,
            name,
            location,
        });
    }
}

/// Program of a Run command line: quoted path, or up to the `.exe` that ends it; `%VARS%`
/// expanded; a bare name (`rundll32.exe x`) is System32's. Pure text, nothing is run or opened.
pub(crate) fn program_of(command: &str) -> Option<String> {
    let expanded = crate::apps::expand_env(command.trim());
    let (prog, _) = crate::apps::parse_command(&expanded)?;
    if prog.contains('\\') {
        return Some(prog);
    }
    Some(format!(r"{}\System32\{prog}", system::system_root()))
}

/// Apps that start through their updater (`<dir>\Update.exe --processStart Discord.exe`): the
/// program the user knows lives in the newest `<dir>\app-<version>\` (the one that shows up running
/// and has the app's icon), else the stub next to the updater. `None` = not that pattern.
fn updater_target(prog: &str, command: &str) -> Option<String> {
    let p = std::path::Path::new(prog);
    if !p.file_name()?.to_str()?.eq_ignore_ascii_case("update.exe") {
        return None;
    }
    let (_, args) = crate::apps::parse_command(&crate::apps::expand_env(command.trim()))?;
    let mut it = args.split_whitespace();
    it.find(|a| a.eq_ignore_ascii_case("--processStart"))?;
    let name = it.next()?.trim_matches('"');
    // A bare file name only: anything with a path or that is not an .exe is not followed.
    if name.contains(['\\', '/', ':']) || name.contains("..") || !name.to_ascii_lowercase().ends_with(".exe") {
        return None;
    }
    let dir = p.parent()?;
    let newest = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().to_ascii_lowercase().starts_with("app-"))
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path().join(name))))
        .filter(|(_, x)| x.is_file())
        .max_by_key(|(t, _)| *t)
        .map(|(_, x)| x);
    Some(newest.unwrap_or_else(|| dir.join(name)).to_string_lossy().into_owned())
}

/// The long, real path of a program that exists (8.3 names and `..` resolved), a local file only.
fn existing(path: &str) -> Option<String> {
    let real = crate::disk::strip_verbatim(&std::fs::canonicalize(crate::icons::local_exe(path)?).ok()?);
    Some(real.to_string_lossy().into_owned())
}

pub fn list() -> Vec<StartupItem> {
    let mut out = Vec::new();
    run_items(Root::Hkcu, RUN, Location::UserRun, &mut out);
    run_items(Root::Hklm, RUN, Location::MachineRun, &mut out);
    run_items(Root::Hklm, RUN_WOW, Location::MachineRun32, &mut out);
    folder_items(Location::UserFolder, &mut out);
    folder_items(Location::CommonFolder, &mut out);
    out.sort_by_key(|i| (!i.enabled, i.name.to_ascii_lowercase()));
    out
}

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error("needs_admin")]
    NeedsAdmin,
    #[error("unknown_item")]
    UnknownItem,
    #[error("protected_item")]
    Protected,
    #[error("{0}")]
    Registry(#[from] registry::RegError),
}

/// Enable/disable one item. The item must exist in the live list (hostile caller).
pub fn set_enabled(location: Location, name: &str, enabled: bool) -> Result<StartupItem, StartupError> {
    let mut item = list().into_iter().find(|i| i.location == location && i.name == name).ok_or(StartupError::UnknownItem)?;
    if item.protected && !enabled {
        return Err(StartupError::Protected);
    }
    if location.needs_admin() && !system::is_elevated() {
        return Err(StartupError::NeedsAdmin);
    }
    let (root, path) = location.approved();
    registry::write(root, &path, name, &Value::Binary(approved_blob(enabled, filetime_now()))).map_err(|e| match e {
        registry::RegError::AccessDenied => StartupError::NeedsAdmin,
        other => StartupError::Registry(other),
    })?;
    item.enabled = enabled;
    Ok(item)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_flags_match_task_manager() {
        assert!(approved_enabled(&Value::Binary(vec![0x02, 0, 0, 0])));
        assert!(approved_enabled(&Value::Binary(vec![0x06, 0, 0, 0])));
        assert!(!approved_enabled(&Value::Binary(vec![0x03, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8])));
        assert!(approved_enabled(&Value::Absent), "no flag = enabled");
        let off = approved_blob(false, 0x0102_0304_0506_0708);
        assert_eq!(off.len(), 12);
        assert_eq!(off[0], 0x03);
        assert_eq!(&off[4..], &0x0102_0304_0506_0708u64.to_le_bytes());
        assert_eq!(approved_blob(true, 99), vec![0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(!Location::UserRun.needs_admin() && Location::MachineRun.needs_admin());
        assert!(is_protected("SecurityHealth", "x") && is_protected("Y", r"%windir%\system32\SecurityHealthSystray.exe"));
        assert!(!is_protected("Discord", r"C:\x\Update.exe"));
        assert!(is_protected("AVP.KES", r"C:\Program Files (x86)\Kaspersky Lab\KES\avpui.exe"));
        assert!(is_protected("Avast", r"C:\Program Files\Avast Software\Avast\AvastUI.exe /nogui"));
        assert!(is_protected("x", r"C:\Program Files\AVG\Antivirus\AVGUI.exe"));
        assert!(!is_protected("Steam", r"C:\Program Files (x86)\Steam\steam.exe -silent"), "no false positive");
    }

    #[test]
    fn program_of_a_command_line() {
        let win = system::system_root();
        assert_eq!(program_of(r#""C:\Program Files\Steam\steam.exe" -silent"#).as_deref(), Some(r"C:\Program Files\Steam\steam.exe"));
        assert_eq!(program_of(r"C:\Program Files\App\app.exe --min").as_deref(), Some(r"C:\Program Files\App\app.exe"));
        assert!(program_of(r"%SystemRoot%\system32\SecurityHealthSystray.exe").unwrap().eq_ignore_ascii_case(&format!(r"{win}\system32\SecurityHealthSystray.exe")));
        assert_eq!(program_of("rundll32.exe x.dll,Run"), Some(format!(r"{win}\System32\rundll32.exe")));
        assert_eq!(program_of("run.cmd now"), None, "not a program G2 Boost can name");
        assert_eq!(program_of(""), None);
        // Only a real local file becomes the item's program.
        assert!(existing(&format!(r"{win}\explorer.exe")).is_some_and(|p| p.to_ascii_lowercase().ends_with(r"\explorer.exe")));
        assert_eq!(existing(r"C:\nope\nope.exe"), None);
        assert_eq!(existing(r"\\server\share\x.exe"), None);
    }

    /// Sandbox: `Update.exe --processStart App.exe` names the newest `app-*\App.exe`, never a path.
    #[test]
    fn updater_started_apps_point_at_the_real_program() {
        let root = std::env::temp_dir().join(format!("g2boost-squirrel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["app-1.0.9", "app-1.0.10"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
            std::fs::write(root.join(d).join("App.exe"), b"x").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let update = root.join("Update.exe").to_string_lossy().into_owned();
        let cmd = format!(r#""{update}" --processStart App.exe"#);
        assert_eq!(updater_target(&update, &cmd), Some(root.join(r"app-1.0.10\App.exe").to_string_lossy().into_owned()));
        // No app-* folder with it: the stub next to the updater.
        let cmd2 = format!(r#""{update}" --processStart Other.exe"#);
        assert_eq!(updater_target(&update, &cmd2), Some(root.join("Other.exe").to_string_lossy().into_owned()));
        // Hostile or other shapes are not followed.
        assert_eq!(updater_target(&update, &format!(r#""{update}" --processStart ..\..\x.exe"#)), None);
        assert_eq!(updater_target(&update, &format!(r#""{update}" --processStart C:\x.exe"#)), None);
        assert_eq!(updater_target(&update, &format!(r#""{update}" --other"#)), None);
        assert_eq!(updater_target(r"C:\App\app.exe", r#""C:\App\app.exe" --processStart App.exe"#), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Round-trips a throwaway HKCU Run entry through disable/enable and removes it.
    #[test]
    fn user_run_roundtrip() {
        let name = "G2BoostTestStartupItem";
        registry::write(Root::Hkcu, RUN, name, &Value::Str(r#""C:\nope\nope.exe" --test"#.into())).unwrap();
        let item = list().into_iter().find(|i| i.name == name).expect("listed");
        assert!(item.enabled && item.location == Location::UserRun && !item.needs_admin);
        assert!(!set_enabled(Location::UserRun, name, false).unwrap().enabled);
        assert!(!list().into_iter().find(|i| i.name == name).unwrap().enabled);
        assert!(set_enabled(Location::UserRun, name, true).unwrap().enabled);
        registry::write(Root::Hkcu, RUN, name, &Value::Absent).unwrap();
        let (root, path) = Location::UserRun.approved();
        registry::write(root, &path, name, &Value::Absent).unwrap();
        assert!(matches!(set_enabled(Location::UserRun, "does-not-exist-g2", false), Err(StartupError::UnknownItem)));
    }
}
