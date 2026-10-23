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
        out.push(StartupItem {
            enabled: state(location, &name),
            needs_admin: location.needs_admin(),
            protected: is_protected(&name, &command),
            command,
            name,
            location,
        });
    }
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
