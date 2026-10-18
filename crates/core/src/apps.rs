//! Installed apps and what they leave behind — AppCleaner's idea done for Windows, clean-room
//! (Docs/references/appcleaner.md, Docs/cleanup/app-uninstall.md).
//!
//! - **List**: the Uninstall keys (HKLM 64/32-bit, HKCU). System components, updates and entries
//!   without an uninstaller are hidden; runtimes/drivers and user-marked apps are `protected`.
//! - **Uninstall**: the app's own uninstaller, validated (never a shell, never a system binary
//!   other than `msiexec /X{GUID}`) and started with `ShellExecuteExW` — visible, UAC by its manifest.
//! - **Leftovers**: fixed roots only, matched by name/publisher/exe/install folder with a
//!   confidence and a reason; every path canonicalized and guarded before it is shown or removed.
//! - **Remove**: files to the Recycle Bin; registry trees backed up (JSON) then deleted; a
//!   service only when its binary is gone. The caller passes a one-shot `ConfirmToken` first.
//!
//! Every caller is hostile: the UI only sends ids; app data lives in the shell's state and in a
//! snapshot file that is re-validated like any other input.

use std::collections::HashSet;
use std::fs;
use std::mem::size_of;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::registry::{self, Root, Value};
use crate::safety::{PathClass, SafePath};

const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL_WOW: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL_USER: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AppsError {
    #[error("unknown_app")]
    UnknownApp,
    #[error("protected_app")]
    Protected,
    /// A shell, script host or system binary: G2 Boost does not run it.
    #[error("uninstaller_blocked")]
    Blocked,
    #[error("uninstaller_missing")]
    Missing,
    #[error("needs_admin")]
    NeedsAdmin,
    /// Leftovers of an app that is still installed are a preview only: uninstall it first.
    #[error("still_installed")]
    StillInstalled,
    #[error("cancelled")]
    Cancelled,
    #[error("win32: {0}")]
    Win32(String),
}

/* ---------------- listing ---------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hive {
    Machine,
    Machine32,
    User,
    /// Store (AppX/MSIX) package of the current user; `key` is the package full name.
    Store,
}

impl Hive {
    fn root(self) -> Root {
        if self == Hive::User { Root::Hkcu } else { Root::Hklm }
    }
    fn base(self) -> &'static str {
        match self {
            Hive::Machine => UNINSTALL,
            Hive::Machine32 => UNINSTALL_WOW,
            Hive::User | Hive::Store => UNINSTALL_USER,
        }
    }
    fn tag(self) -> &'static str {
        match self {
            Hive::Machine => "m64",
            Hive::Machine32 => "m32",
            Hive::User => "u",
            Hive::Store => "s",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct App {
    /// `<hive>:<Uninstall subkey>` — the only handle the UI sends back.
    pub id: String,
    pub hive: Hive,
    pub key: String,
    pub name: String,
    pub publisher: String,
    pub version: String,
    pub install_location: Option<String>,
    /// Main executable (from `DisplayIcon`), when it names an .exe.
    pub exe: Option<String>,
    pub size_bytes: Option<u64>,
    /// `YYYYMMDD` as Windows stores it.
    pub install_date: Option<String>,
    /// Raw `UninstallString`: data, validated before anything runs.
    pub uninstall: String,
    pub msi: bool,
    pub needs_admin: bool,
    /// `runtime` (frameworks, drivers), `self` (G2 Boost), `user` (marked in settings).
    pub protected: Option<String>,
}

/// `%VAR%` expanded from this process' environment (unknown names stay as they are).
pub(crate) fn expand_env(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let Some(j) = after.find('%') else {
            out.push('%');
            rest = after;
            continue;
        };
        let var = &after[..j];
        match std::env::var(var) {
            Ok(v) if !var.is_empty() => out.push_str(&v),
            _ => {
                out.push('%');
                out.push_str(var);
                out.push('%');
            }
        }
        rest = &after[j + 1..];
    }
    out.push_str(rest);
    out
}

fn read_str(root: Root, path: &str, name: &str) -> Option<String> {
    match registry::read(root, path, name) {
        Ok(Value::Str(s)) => {
            let s = expand_env(s.trim().trim_matches('"').trim());
            (!s.is_empty()).then_some(s)
        }
        _ => None,
    }
}

fn read_dword(root: Root, path: &str, name: &str) -> Option<u32> {
    match registry::read(root, path, name) {
        Ok(Value::Dword(d)) => Some(d),
        _ => None,
    }
}

/// Frameworks, redistributables and drivers: removing them breaks other apps or hardware.
fn runtime(name: &str, publisher: &str) -> bool {
    let (n, p) = (name.to_ascii_lowercase(), publisher.to_ascii_lowercase());
    const MS: &[&str] = &[
        "visual c++", ".net", "microsoft edge", "webview2", "windows sdk", "software development kit", "directx",
        "update health", "pc health check", "windows app runtime", "vs_", "desktop runtime", "gameinput",
    ];
    const ANY: &[&str] = &["redistributable", "driver", "chipset", "firmware", "vcredist", "runtime"];
    (p.contains("microsoft") && MS.iter().any(|k| n.contains(k))) || ANY.iter().any(|k| n.contains(k))
}

fn entry(hive: Hive, key: &str, user_protected: &[String]) -> Option<App> {
    let root = hive.root();
    let path = format!(r"{}\{key}", hive.base());
    let name = read_str(root, &path, "DisplayName")?;
    if read_dword(root, &path, "SystemComponent") == Some(1) || read_str(root, &path, "ParentKeyName").is_some() {
        return None;
    }
    if read_str(root, &path, "ReleaseType").is_some_and(|r| matches!(r.as_str(), "Security Update" | "Update Rollup" | "Hotfix" | "Update")) {
        return None;
    }
    let uninstall = read_str(root, &path, "UninstallString")?;
    let publisher = read_str(root, &path, "Publisher").unwrap_or_default();
    let exe = read_str(root, &path, "DisplayIcon").and_then(|i| {
        let p = i.rsplit_once(',').filter(|(_, n)| n.trim().parse::<i32>().is_ok()).map_or(i.as_str(), |(p, _)| p).trim().trim_matches('"');
        p.to_ascii_lowercase().ends_with(".exe").then(|| p.to_string())
    });
    let protected = if name.eq_ignore_ascii_case("G2 Boost") {
        Some("self")
    } else if user_protected.iter().any(|u| u.eq_ignore_ascii_case(&name)) {
        Some("user")
    } else if crate::startup::is_security_software(&name) || crate::startup::is_security_software(&publisher) {
        Some("security")
    } else if runtime(&name, &publisher) {
        Some("runtime")
    } else {
        None
    };
    Some(App {
        id: format!("{}:{key}", hive.tag()),
        hive,
        key: key.to_string(),
        version: read_str(root, &path, "DisplayVersion").unwrap_or_default(),
        install_location: read_str(root, &path, "InstallLocation").map(|l| l.trim_end_matches('\\').to_string()).filter(|l| !l.is_empty()),
        exe,
        size_bytes: read_dword(root, &path, "EstimatedSize").map(|kb| u64::from(kb) * 1024),
        install_date: read_str(root, &path, "InstallDate"),
        msi: read_dword(root, &path, "WindowsInstaller") == Some(1),
        needs_admin: hive == Hive::Machine || hive == Hive::Machine32,
        protected: protected.map(str::to_string),
        uninstall,
        publisher,
        name,
    })
}

/// Every removable-looking entry (Uninstall keys + Store packages), deduplicated by name +
/// version, sorted by name.
pub fn list(user_protected: &[String]) -> Vec<App> {
    let mut out: Vec<App> = Vec::new();
    for hive in [Hive::Machine, Hive::Machine32, Hive::User] {
        for key in registry::subkeys(hive.root(), hive.base()).unwrap_or_default() {
            if let Some(a) = entry(hive, &key, user_protected)
                && !out.iter().any(|o| o.name.eq_ignore_ascii_case(&a.name) && o.version == a.version)
            {
                out.push(a);
            }
        }
    }
    for a in store_apps(user_protected) {
        if !out.iter().any(|o| o.name.eq_ignore_ascii_case(&a.name)) {
            out.push(a);
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Still installed? (Uninstall key / Store package exists) — polled after starting an uninstaller.
pub fn installed(app: &App) -> bool {
    if app.hive == Hive::Store {
        return store_package(&app.key).is_some();
    }
    read_str(app.hive.root(), &format!(r"{}\{}", app.hive.base(), app.key), "DisplayName").is_some()
}

/* ---------------- Store (AppX/MSIX) ---------------- */

/// Store apps the user should not remove from here (the Store itself, winget, Windows Security).
const STORE_PROTECTED: &[&str] = &[
    "microsoft.windowsstore", "microsoft.desktopappinstaller", "microsoft.storepurchaseapp", "microsoft.sechealthui",
    "microsoft.windows.", "microsoft.ui.xaml", "microsoft.vclibs", "microsoft.net.", "microsoft.services.store",
    "microsoft.winappruntime", "microsoftwindows.",
];

fn package_manager() -> Option<windows::Management::Deployment::PackageManager> {
    windows::Management::Deployment::PackageManager::new().ok()
}

/// The current user's Store packages a person would call "apps" (no frameworks, resources,
/// bundles, stubs or Windows' own system packages).
fn store_apps(user_protected: &[String]) -> Vec<App> {
    use windows::ApplicationModel::PackageSignatureKind;
    let Some(pm) = package_manager() else {
        return Vec::new();
    };
    let Ok(pkgs) = pm.FindPackagesByUserSecurityId(&windows::core::HSTRING::new()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for p in pkgs {
        let skip = p.IsFramework().unwrap_or(true)
            || p.IsResourcePackage().unwrap_or(true)
            || p.IsBundle().unwrap_or(true)
            || p.IsStub().unwrap_or(false)
            || p.SignatureKind().map(|k| k == PackageSignatureKind::System).unwrap_or(true);
        if skip {
            continue;
        }
        let Ok(id) = p.Id() else {
            continue;
        };
        let (Ok(full), Ok(family)) = (id.FullName(), id.FamilyName()) else {
            continue;
        };
        let (full, family) = (full.to_string(), family.to_string());
        let name = p.DisplayName().map(|n| n.to_string()).ok().filter(|n| !n.is_empty() && !n.starts_with("ms-resource:")).unwrap_or_else(|| family.clone());
        let version = id.Version().map(|v| format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision)).unwrap_or_default();
        let fl = family.to_ascii_lowercase();
        let protected = if STORE_PROTECTED.iter().any(|s| fl.starts_with(s)) {
            Some("runtime")
        } else if user_protected.iter().any(|u| u.eq_ignore_ascii_case(&name)) {
            Some("user")
        } else {
            None
        };
        out.push(App {
            id: format!("s:{full}"),
            hive: Hive::Store,
            key: full,
            name,
            publisher: p.PublisherDisplayName().map(|s| s.to_string()).unwrap_or_default(),
            version,
            // Never offered as a leftover: WindowsApps belongs to the Store servicing stack.
            install_location: None,
            exe: None,
            size_bytes: None,
            install_date: None,
            // Store apps keep their family name here (no UninstallString); validated before use.
            uninstall: family,
            msi: false,
            needs_admin: false,
            protected: protected.map(str::to_string),
        });
    }
    out
}

/// Package family names: `Name_publisherhash` (letters, digits, `.`, `-`, `_`), never a path.
/// The snapshot file is user-writable, so this is checked before it is joined to a folder.
fn valid_family(f: &str) -> bool {
    !f.is_empty() && f.len() <= 128 && !f.contains("..") && f.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// The installed package with this full name, if any.
fn store_package(full: &str) -> Option<windows::ApplicationModel::Package> {
    let pm = package_manager()?;
    let pkgs = pm.FindPackagesByUserSecurityId(&windows::core::HSTRING::new()).ok()?;
    pkgs.into_iter().find(|p| p.Id().and_then(|i| i.FullName()).is_ok_and(|n| n.to_string().eq_ignore_ascii_case(full)))
}

/// Remove a Store package for the current user (blocks until Windows finishes).
fn store_remove(full: &str) -> Result<(), AppsError> {
    store_package(full).ok_or(AppsError::UnknownApp)?;
    let pm = package_manager().ok_or_else(|| AppsError::Win32("PackageManager".into()))?;
    let op = pm.RemovePackageAsync(&windows::core::HSTRING::from(full)).map_err(|e| AppsError::Win32(e.message()))?;
    let result = op.join().map_err(|e| AppsError::Win32(e.message()))?;
    match result.ExtendedErrorCode() {
        Ok(code) if code.is_ok() => Ok(()),
        Ok(code) => Err(AppsError::Win32(format!("{code:?}"))),
        Err(e) => Err(AppsError::Win32(e.message())),
    }
}

/* ---------------- snapshot (orphan sweep) ---------------- */

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Snapshot {
    apps: Vec<App>,
}

fn snapshot_file() -> PathBuf {
    registry::data_dir().join("apps_snapshot.json")
}

/// Remember what was installed (apps that disappear later are offered for a leftover sweep).
pub fn remember(current: &[App]) {
    let mut snap: Snapshot = fs::read_to_string(snapshot_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    for a in current {
        snap.apps.retain(|s| s.id != a.id);
        snap.apps.push(a.clone());
    }
    let n = snap.apps.len();
    if n > 600 {
        snap.apps.drain(..n - 600);
    }
    if let Some(d) = snapshot_file().parent() {
        let _ = fs::create_dir_all(d);
    }
    let _ = fs::write(snapshot_file(), serde_json::to_string(&snap).unwrap_or_default());
}

/// Apps seen before that are no longer installed. The file is user-writable: entries are only
/// used to *look* for leftovers, and everything found goes through the same guards.
pub fn orphans(current: &[App]) -> Vec<App> {
    let snap: Snapshot = fs::read_to_string(snapshot_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    snap.apps.into_iter().filter(|s| s.protected.is_none() && !current.iter().any(|c| c.id == s.id) && !installed(s)).collect()
}

pub fn forget(id: &str) {
    let Ok(t) = fs::read_to_string(snapshot_file()) else {
        return;
    };
    let Ok(mut snap) = serde_json::from_str::<Snapshot>(&t) else {
        return;
    };
    snap.apps.retain(|a| a.id != id);
    let _ = fs::write(snapshot_file(), serde_json::to_string(&snap).unwrap_or_default());
}

/* ---------------- name matching ---------------- */

const STOP: &[&str] = &[
    "inc", "corp", "corporation", "llc", "ltd", "limited", "gmbh", "co", "software", "technologies", "technology", "the",
    "version", "edition", "x64", "x86", "64", "32", "bit", "64bit", "32bit", "amd64", "win64", "win32", "setup",
    "installer", "for", "windows",
];

fn is_versionish(w: &str) -> bool {
    let w = w.strip_prefix('v').unwrap_or(w);
    !w.is_empty() && w.chars().all(|c| c.is_ascii_digit())
}

/// Lowercase words without brackets, punctuation, company suffixes or a trailing version:
/// `7-Zip 23.01 (x64)` → `7 zip`, `Mozilla Firefox (x64 pt-BR)` → `mozilla firefox`.
pub(crate) fn norm(s: &str) -> String {
    norm_with(s, true)
}

/// `pop_version: false` keeps a trailing version word (folder names: `Name 2` ≠ `Name`).
fn norm_with(s: &str, pop_version: bool) -> String {
    let mut t = String::with_capacity(s.len());
    let mut depth = 0u32;
    for ch in s.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            c if depth == 0 => {
                if c.is_alphanumeric() {
                    t.extend(c.to_lowercase());
                } else {
                    t.push(' ');
                }
            }
            _ => {}
        }
    }
    let mut toks: Vec<&str> = t.split_whitespace().filter(|w| !STOP.contains(w)).collect();
    while pop_version && toks.len() > 1 && toks.last().is_some_and(|w| is_versionish(w)) {
        toks.pop();
    }
    toks.join(" ")
}

fn compact(n: &str) -> String {
    n.chars().filter(|c| !c.is_whitespace()).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

/// How an app's name, product word, exe and publisher are compared with folder/key names.
pub(crate) struct Matcher {
    norm_full: String,
    full: String,
    /// Compact name keeping its trailing version (`EA SPORTS FC 27` → `easportsfc27`).
    full_v: String,
    /// Trailing version words of the app name, when it has them (`27`, `23 01`).
    version: Option<String>,
    /// Name without the publisher's words (`Google Chrome` → `chrome`).
    product: Option<String>,
    exe: Option<String>,
    publisher: Option<String>,
}

impl Matcher {
    pub(crate) fn new(app: &App) -> Self {
        let norm_full = norm(&app.name);
        let with_v = norm_with(&app.name, false);
        let version = with_v.strip_prefix(&norm_full).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
        let pub_words: Vec<String> = norm(&app.publisher).split(' ').filter(|w| !w.is_empty()).map(str::to_string).collect();
        let product = norm_full.split(' ').filter(|w| !pub_words.iter().any(|p| p == w)).collect::<Vec<_>>().join(" ");
        let exe = app.exe.as_deref().and_then(|e| Path::new(e).file_stem()).map(|s| compact(&norm(&s.to_string_lossy())));
        Matcher {
            full: compact(&norm_full),
            full_v: compact(&with_v),
            version,
            product: Some(compact(&product)).filter(|p| p.len() >= 4 && *p != compact(&norm_full)),
            exe: exe.filter(|e| e.len() >= 4),
            publisher: Some(compact(&pub_words.join(" "))).filter(|p| p.len() >= 3),
            norm_full,
        }
    }

    /// Does a folder/key named `name` belong to the app?
    pub(crate) fn folder(&self, name: &str) -> Option<Confidence> {
        if self.full.len() < 4 {
            return None; // too generic to match by name ("Go", "App")
        }
        let n = norm_with(name, false);
        let c = compact(&n);
        if c == self.full_v || self.product.as_ref() == Some(&c) || self.exe.as_ref() == Some(&c) {
            return Some(Confidence::High);
        }
        if c == self.full {
            // The app is versioned (`FC 27`) and the folder is not: possibly shared by every version.
            return Some(Confidence::Medium);
        }
        // `Name 2`, `Name_v3`: the name followed by a version word. When the app's own name
        // carries a version (yearly games: `FC 27`), it must be the same one — `FC 26` is another app.
        let rest = n.strip_prefix(&self.norm_full).and_then(|r| r.strip_prefix(' '))?;
        let first = rest.split(' ').next().filter(|w| is_versionish(w))?;
        match &self.version {
            Some(v) if v.split(' ').next() != Some(first) => None,
            _ => Some(Confidence::Medium),
        }
    }

    pub(crate) fn is_publisher(&self, name: &str) -> bool {
        self.publisher.as_ref().is_some_and(|p| *p == compact(&norm(name)))
    }
}

/* ---------------- leftovers ---------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeftoverKind {
    Folder,
    File,
    Shortcut,
    Registry,
    Service,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// The app's install folder still exists.
    InstallFolder,
    /// A Store app's data folder (`%LOCALAPPDATA%\Packages\<family name>`).
    PackageData,
    /// A shortcut pointing into the install folder.
    ShortcutTarget,
    /// Folder/key/shortcut named like the app.
    NameMatch,
    /// Named like the app, inside a folder/key named like the publisher.
    PublisherFolder,
    CrashDump,
    /// A service whose program is inside the install folder (or named like the app) and is gone.
    ServiceBinaryGone,
}

#[derive(Debug, Clone, PartialEq)]
enum Target {
    Fs(PathBuf),
    Reg(Root, String),
    Service(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct Leftover {
    pub id: u32,
    pub kind: LeftoverKind,
    /// What the user sees: a file path, `HKCU\Software\X`, or a service name.
    pub path: String,
    pub bytes: u64,
    pub confidence: Confidence,
    pub reason: Reason,
    pub needs_admin: bool,
    #[serde(skip)]
    target: Target,
}

/// Where leftovers may live. `live()` for the real machine; tests build one over a sandbox.
pub(crate) struct Ctx {
    windows: PathBuf,
    profile: PathBuf,
    /// AppData (Roaming), LocalAppData, LocalLow, ProgramData, LocalAppData\Programs.
    data: Vec<PathBuf>,
    /// Start Menu (user, all users) and Desktop (user, public).
    shortcuts: Vec<PathBuf>,
    dumps: Option<PathBuf>,
    /// Registry bases searched for keys named like the app.
    reg: Vec<(Root, String)>,
    services: bool,
}

fn known_desktop() -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Desktop, KNOWN_FOLDER_FLAG, SHGetKnownFolderPath};
    // SAFETY: the API allocates the string; freed with CoTaskMemFree. Desktop may live in OneDrive.
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Desktop, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const std::ffi::c_void));
        s.map(PathBuf::from)
    }
}

impl Ctx {
    pub(crate) fn live() -> Self {
        let env = crate::cleanup::env_dir;
        let profile = env("USERPROFILE").unwrap_or_else(|| PathBuf::from(r"C:\Users\Default"));
        let local = env("LOCALAPPDATA");
        let roaming = env("APPDATA");
        let program_data = env("ProgramData");
        let mut data: Vec<PathBuf> = [roaming.clone(), local.clone(), Some(profile.join(r"AppData\LocalLow")), program_data.clone()].into_iter().flatten().collect();
        if let Some(l) = &local {
            data.push(l.join("Programs"));
        }
        let mut shortcuts = Vec::new();
        if let Some(r) = &roaming {
            shortcuts.push(r.join(r"Microsoft\Windows\Start Menu\Programs"));
        }
        if let Some(pd) = &program_data {
            shortcuts.push(pd.join(r"Microsoft\Windows\Start Menu\Programs"));
        }
        shortcuts.extend(known_desktop());
        if let Some(public) = env("PUBLIC") {
            shortcuts.push(public.join("Desktop"));
        }
        Ctx {
            windows: env("SystemRoot").unwrap_or_else(|| PathBuf::from(r"C:\Windows")),
            dumps: local.map(|l| l.join("CrashDumps")),
            data,
            shortcuts,
            reg: vec![(Root::Hkcu, "Software".into()), (Root::Hklm, "SOFTWARE".into()), (Root::Hklm, r"SOFTWARE\WOW6432Node".into())],
            services: true,
            profile,
        }
    }
}

/// Shared folders at the top of AppData/ProgramData that no app "owns".
const DENY_FOLDERS: &[&str] = &[
    "microsoft", "windows", "packages", "temp", "programs", "publishers", "comms", "connecteddevicesplatform",
    "d3dscache", "crashdumps", "fontcache", "history", "inetcache", "virtualstore", "package cache", "regid.1991-06.com.microsoft",
    "ssh", "desktop", "documents", "start menu", "nvidia corporation", "intel", "amd", "packagemanagement",
];
/// Shared registry keys under Software.
const DENY_KEYS: &[&str] = &[
    "microsoft", "classes", "policies", "wow6432node", "clients", "registeredapplications", "windows", "intel", "odbc",
    "partner", "wow64",
];

fn lower(p: &Path) -> String {
    p.to_string_lossy().trim_end_matches('\\').to_ascii_lowercase()
}

fn starts_with_dir(child: &str, dir: &str) -> bool {
    child == dir || child.starts_with(&format!("{dir}\\"))
}

fn is_reparse(p: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    fs::symlink_metadata(p).map(|m| m.file_attributes() & 0x400 != 0).unwrap_or(true)
}

/// The install folder may be removed only below a place where apps install (never a root).
fn install_dir_ok(l: &str, ctx: &Ctx) -> bool {
    let pf: Vec<String> = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"].iter().filter_map(|v| crate::cleanup::env_dir(v)).map(|p| lower(&p)).collect();
    let programs = ctx.data.iter().map(|d| lower(d)).filter(|d| d.ends_with(r"\programs"));
    let below = |base: &str| l.starts_with(&format!("{base}\\"));
    pf.iter().any(|b| below(b))
        || programs.clone().any(|b| below(&b))
        // Another drive: at least two levels deep (D:\Games\X, never D:\ or D:\Games).
        || (!l.starts_with(&lower(&ctx.windows)[..2]) && l.matches('\\').count() >= 2)
}

/// Final gate for any filesystem leftover (shown or removed). `p` must already exist.
pub(crate) fn fs_allowed(p: &Path, ctx: &Ctx, install_dir: bool) -> Option<PathBuf> {
    if is_reparse(p) {
        return None; // junction/symlink: never followed, never removed
    }
    let real = crate::disk::strip_verbatim(&fs::canonicalize(p).ok()?);
    let safe = SafePath::classify(&real, &ctx.windows, &ctx.profile).ok()?;
    let l = lower(&real);
    if starts_with_dir(&l, &lower(&ctx.windows)) {
        return None;
    }
    // Roots: never the base itself, the profile, the drive or the shared install folders.
    let mut roots: Vec<String> = ctx.data.iter().chain(&ctx.shortcuts).map(|b| lower(b)).collect();
    let prof = lower(&ctx.profile);
    roots.extend([prof.clone(), format!(r"{prof}\appdata"), format!(r"{prof}\documents"), format!(r"{prof}\downloads")]);
    let program_files = l.len() > 3 && matches!(&l[3..], "program files" | "program files (x86)" | "users" | "programdata");
    if l.len() <= 3 || program_files || roots.contains(&l) {
        return None;
    }
    let inside_base = ctx.data.iter().chain(&ctx.shortcuts).any(|b| l.starts_with(&format!("{}\\", lower(b))));
    let ok = match safe.class() {
        PathClass::System => inside_base, // ProgramData children only
        PathClass::UserProfile | PathClass::Other => inside_base || (install_dir && install_dir_ok(&l, ctx)),
        _ => false,
    };
    ok.then_some(real)
}

fn measure(p: &Path) -> u64 {
    if p.is_dir() {
        let mut t = crate::cleanup::Tally::default();
        crate::cleanup::measure_dir(p, &mut t);
        t.bytes
    } else {
        fs::metadata(p).map(|m| m.len()).unwrap_or(0)
    }
}

fn dirs(p: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = fs::read_dir(p) else {
        return Vec::new();
    };
    rd.flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink()))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect()
}

/// Target of a `.lnk` (shell link API; the file is read, nothing is run).
pub(crate) fn lnk_target(p: &Path) -> Option<PathBuf> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile, STGM_READ};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{Interface, PCWSTR};
    let _com = crate::tasksched::Com::init().ok()?;
    let w = registry::wide(&p.to_string_lossy());
    // SAFETY: standard IShellLink/IPersistFile sequence; buffers sized for the calls.
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        link.cast::<IPersistFile>().ok()?.Load(PCWSTR(w.as_ptr()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..end]);
        (!s.is_empty()).then(|| PathBuf::from(s))
    }
}

/// Program path of a service `ImagePath` (`"C:\x\svc.exe" -k` / `\SystemRoot\…` / plain).
pub(crate) fn service_exe(image: &str) -> String {
    let s = image.trim();
    let path = match s.strip_prefix('"') {
        Some(r) => r.split('"').next().unwrap_or(r).to_string(),
        None => {
            let l = s.to_ascii_lowercase();
            l.find(".exe").map_or(s.to_string(), |i| s[..i + 4].to_string())
        }
    };
    path.strip_prefix(r"\??\").unwrap_or(&path).to_string()
}

struct Found(Vec<Leftover>);

impl Found {
    fn push(&mut self, kind: LeftoverKind, target: Target, path: String, (confidence, reason): (Confidence, Reason), needs_admin: bool, bytes: u64) {
        if self.0.iter().any(|l| l.target == target) {
            return;
        }
        // A folder already listed covers what is inside it.
        if let Target::Fs(p) = &target
            && self.0.iter().any(|l| matches!(&l.target, Target::Fs(q) if starts_with_dir(&lower(p), &lower(q))))
        {
            return;
        }
        let id = self.0.len() as u32;
        self.0.push(Leftover { id, kind, path, bytes, confidence, reason, needs_admin, target });
    }

    fn fs(&mut self, p: &Path, ctx: &Ctx, kind: LeftoverKind, confidence: Confidence, reason: Reason, install_dir: bool) {
        if let Some(real) = fs_allowed(p, ctx, install_dir) {
            let needs_admin = !starts_with_dir(&lower(&real), &lower(&ctx.profile));
            let bytes = measure(&real);
            self.push(kind, Target::Fs(real.clone()), real.to_string_lossy().into_owned(), (confidence, reason), needs_admin, bytes);
        }
    }
}

/// Everything that looks like it belongs to `app`, most confident first.
pub fn leftovers(app: &App) -> Vec<Leftover> {
    leftovers_in(app, &Ctx::live())
}

pub(crate) fn leftovers_in(app: &App, ctx: &Ctx) -> Vec<Leftover> {
    let m = Matcher::new(app);
    let mut f = Found(Vec::new());
    let install = app.install_location.as_deref().map(PathBuf::from).filter(|p| p.is_absolute() && p.exists());

    if let Some(dir) = &install {
        f.fs(dir, ctx, LeftoverKind::Folder, Confidence::High, Reason::InstallFolder, true);
    }
    // Store app: its data folder, by exact family name (the only match inside Packages).
    if app.hive == Hive::Store
        && let Some(local) = ctx.data.get(1)
        && valid_family(&app.uninstall)
    {
        f.fs(&local.join("Packages").join(&app.uninstall), ctx, LeftoverKind::Folder, Confidence::High, Reason::PackageData, false);
    }
    if app.hive == Hive::Store {
        return finish(f.0); // name matching is for desktop apps; Store data lives in Packages only
    }
    // AppData, LocalAppData, LocalLow, ProgramData, Programs: <Name> or <Publisher>\<Name>.
    for root in &ctx.data {
        for (name, path) in dirs(root) {
            if DENY_FOLDERS.contains(&name.to_ascii_lowercase().as_str()) {
                continue;
            }
            if let Some(c) = m.folder(&name) {
                f.fs(&path, ctx, LeftoverKind::Folder, c, Reason::NameMatch, false);
            } else if m.is_publisher(&name) {
                for (sub, sp) in dirs(&path) {
                    if let Some(c) = m.folder(&sub) {
                        f.fs(&sp, ctx, LeftoverKind::Folder, c, Reason::PublisherFolder, false);
                    }
                }
            }
        }
    }
    // Start Menu / Desktop: folders named like the app, shortcuts into the install folder.
    let install_l = install.as_deref().or(app.install_location.as_deref().map(Path::new)).map(lower);
    for root in &ctx.shortcuts {
        let mut stack = vec![(root.clone(), 0u8)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(rd) = fs::read_dir(&dir) else {
                continue;
            };
            for e in rd.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().into_owned();
                let Ok(ty) = e.file_type() else {
                    continue;
                };
                if ty.is_dir() && !ty.is_symlink() {
                    if m.folder(&name) == Some(Confidence::High) {
                        f.fs(&p, ctx, LeftoverKind::Folder, Confidence::High, Reason::NameMatch, false);
                    } else if depth < 1 {
                        stack.push((p, depth + 1));
                    }
                } else if name.to_ascii_lowercase().ends_with(".lnk") {
                    let into_install = install_l.as_deref().is_some_and(|i| lnk_target(&p).is_some_and(|t| starts_with_dir(&lower(&t), i)));
                    let stem = name[..name.len() - 4].to_string();
                    if into_install {
                        f.fs(&p, ctx, LeftoverKind::Shortcut, Confidence::High, Reason::ShortcutTarget, false);
                    } else if m.folder(&stem) == Some(Confidence::High) {
                        f.fs(&p, ctx, LeftoverKind::Shortcut, Confidence::Medium, Reason::NameMatch, false);
                    }
                }
            }
        }
    }
    // Crash dumps named after the main exe.
    if let (Some(dumps), Some(exe)) = (&ctx.dumps, app.exe.as_deref().and_then(|e| Path::new(e).file_name()).map(|n| n.to_string_lossy().to_ascii_lowercase()))
        && let Ok(rd) = fs::read_dir(dumps)
    {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_ascii_lowercase();
            if n.starts_with(&format!("{exe}.")) && n.ends_with(".dmp") {
                f.fs(&e.path(), ctx, LeftoverKind::File, Confidence::High, Reason::CrashDump, false);
            }
        }
    }
    // Registry: Software\<Name> or Software\<Publisher>\<Name>.
    for (root, base) in &ctx.reg {
        let label = if *root == Root::Hkcu { "HKCU" } else { "HKLM" };
        for key in registry::subkeys(*root, base).unwrap_or_default() {
            if DENY_KEYS.contains(&key.to_ascii_lowercase().as_str()) {
                continue;
            }
            let mut hit = |path: String, c: Confidence, reason: Reason| {
                f.push(LeftoverKind::Registry, Target::Reg(*root, path.clone()), format!(r"{label}\{path}"), (c, reason), *root == Root::Hklm, 0);
            };
            if let Some(c) = m.folder(&key) {
                hit(format!(r"{base}\{key}"), c, Reason::NameMatch);
            } else if m.is_publisher(&key) {
                for sub in registry::subkeys(*root, &format!(r"{base}\{key}")).unwrap_or_default() {
                    if let Some(c) = m.folder(&sub) {
                        hit(format!(r"{base}\{key}\{sub}"), c, Reason::PublisherFolder);
                    }
                }
            }
        }
    }
    // Services (own/shared process, not drivers) whose program is gone.
    if ctx.services {
        const SERVICES: &str = r"SYSTEM\CurrentControlSet\Services";
        let win = lower(&ctx.windows);
        for svc in registry::subkeys(Root::Hklm, SERVICES).unwrap_or_default() {
            let path = format!(r"{SERVICES}\{svc}");
            if !matches!(read_dword(Root::Hklm, &path, "Type"), Some(0x10 | 0x20)) {
                continue;
            }
            let Some(image) = read_str(Root::Hklm, &path, "ImagePath") else {
                continue;
            };
            let exe = service_exe(&image);
            let el = exe.to_ascii_lowercase();
            if el.is_empty() || starts_with_dir(&el, &win) || el.starts_with(r"\systemroot") || el.starts_with("system32") {
                continue;
            }
            let ours = install_l.as_deref().is_some_and(|i| starts_with_dir(&el, i)) || m.folder(&svc) == Some(Confidence::High);
            if ours && !Path::new(&exe).exists() {
                f.push(LeftoverKind::Service, Target::Service(svc.clone()), svc, (Confidence::High, Reason::ServiceBinaryGone), true, 0);
            }
        }
    }
    finish(f.0)
}

/// Most confident first, then largest; ids renumbered in that order.
fn finish(mut v: Vec<Leftover>) -> Vec<Leftover> {
    v.sort_by(|a, b| b.confidence.cmp(&a.confidence).then(b.bytes.cmp(&a.bytes)));
    for (i, l) in v.iter_mut().enumerate() {
        l.id = i as u32;
    }
    v
}

/* ---------------- uninstall ---------------- */

/// Programs G2 Boost never starts as an "uninstaller" (shells, script hosts, LOLBins).
const BLOCKED: &[&str] = &[
    "cmd.exe", "powershell.exe", "pwsh.exe", "wscript.exe", "cscript.exe", "mshta.exe", "rundll32.exe", "regsvr32.exe",
    "reg.exe", "conhost.exe", "bitsadmin.exe", "certutil.exe", "schtasks.exe", "sc.exe", "net.exe", "wmic.exe",
    "explorer.exe", "forfiles.exe", "msbuild.exe", "installutil.exe",
];

/// `(program, arguments)` of a command line: quoted path, or up to the `.exe` that ends the token.
pub(crate) fn parse_command(cmd: &str) -> Option<(String, String)> {
    let c = cmd.trim();
    if let Some(rest) = c.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some((rest[..end].to_string(), rest[end + 1..].trim().to_string()));
    }
    let l = c.to_ascii_lowercase();
    let end = l.match_indices(".exe").map(|(i, _)| i + 4).find(|&e| l[e..].chars().next().is_none_or(char::is_whitespace))?;
    Some((c[..end].to_string(), c[end..].trim().to_string()))
}

/// `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}` in MSI arguments.
fn msi_product_code(args: &str) -> Option<String> {
    let start = args.find('{')?;
    let code = args.get(start..start + 38)?;
    let inner = &code[1..37];
    let ok = code.ends_with('}')
        && inner.char_indices().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == '-' } else { c.is_ascii_hexdigit() });
    ok.then(|| code.to_string())
}

/// What will actually run for `app`'s uninstall, after every check.
pub(crate) fn uninstaller(app: &App, windows: &Path) -> Result<(PathBuf, String), AppsError> {
    let (exe, args) = parse_command(&app.uninstall).ok_or(AppsError::Blocked)?;
    let base = Path::new(&exe).file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if base == "msiexec.exe" || base == "msiexec" {
        // Always Windows' own msiexec, always "/X{product code}" (uninstall), whatever the entry says.
        let code = msi_product_code(&args).ok_or(AppsError::Blocked)?;
        return Ok((windows.join(r"System32\msiexec.exe"), format!("/X{code}")));
    }
    let p = PathBuf::from(&exe);
    if BLOCKED.contains(&base.as_str()) || !base.ends_with(".exe") || !p.is_absolute() {
        return Err(AppsError::Blocked);
    }
    if starts_with_dir(&lower(&p), &lower(windows)) {
        return Err(AppsError::Blocked); // system binaries are not uninstallers
    }
    if !p.is_file() {
        return Err(AppsError::Missing);
    }
    Ok((p, args))
}

/// Start the app's own uninstaller (visible; its manifest decides about UAC). Returns at once:
/// the UI polls [`installed`] to know when it is done.
pub fn uninstall(app: &App) -> Result<(), AppsError> {
    use windows::Win32::UI::Shell::{SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::PCWSTR;
    if app.protected.is_some() {
        return Err(AppsError::Protected);
    }
    if app.hive == Hive::Store {
        return store_remove(&app.key);
    }
    let windows = crate::cleanup::env_dir("SystemRoot").unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let (exe, args) = uninstaller(app, &windows)?;
    let file = registry::wide(&exe.to_string_lossy());
    let params = registry::wide(&args);
    let dir = registry::wide(&exe.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC,
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        lpDirectory: PCWSTR(dir.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: every string outlives the call; the struct is fully initialised.
    unsafe { ShellExecuteExW(&mut info) }.map_err(|e| match e.code().0 as u32 {
        0x8007_04C7 => AppsError::Cancelled, // ERROR_CANCELLED: UAC dismissed
        0x8007_0005 => AppsError::NeedsAdmin,
        _ => AppsError::Win32(e.message()),
    })
}

/* ---------------- drag & drop ---------------- */

/// The app a dropped `.exe`/`.lnk` belongs to (its exe, or the longest install folder containing it).
pub fn resolve_drop(path: &str, apps: &[App]) -> Option<String> {
    let p = Path::new(path);
    if !p.is_absolute() || !p.exists() {
        return None;
    }
    let target = if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) { lnk_target(p)? } else { p.to_path_buf() };
    let t = lower(&target);
    apps.iter()
        .filter_map(|a| {
            if a.exe.as_deref().is_some_and(|e| lower(Path::new(e)) == t) {
                return Some((usize::MAX, a));
            }
            let loc = lower(Path::new(a.install_location.as_deref()?));
            (loc.len() > 3 && starts_with_dir(&t, &loc)).then_some((loc.len(), a))
        })
        .max_by_key(|(n, _)| *n)
        .map(|(_, a)| a.id.clone())
}

/* ---------------- removal ---------------- */

#[derive(Debug, Clone, Default, Serialize)]
pub struct RemoveOutcome {
    pub recycled: u32,
    pub bytes: u64,
    pub registry: u32,
    pub services: u32,
    pub failed: u32,
    /// Skipped because they need administrator rights.
    pub needs_admin: u32,
}

fn backup_dir() -> PathBuf {
    registry::data_dir().join("uninstall_backups")
}

/// Remove the chosen leftovers (already confirmed with a token by the caller). Each one is
/// re-checked here: the filesystem may have changed since the preview. Refused while the app is
/// still installed — deleting its folder would be a forced uninstall that breaks its registration.
pub fn remove(app: &App, items: &[Leftover]) -> Result<RemoveOutcome, AppsError> {
    if installed(app) {
        return Err(AppsError::StillInstalled);
    }
    Ok(remove_in(items, &app.name, &Ctx::live(), crate::system::is_elevated()))
}

pub(crate) fn remove_in(items: &[Leftover], app_name: &str, ctx: &Ctx, elevated: bool) -> RemoveOutcome {
    let t0 = std::time::Instant::now();
    let mut out = RemoveOutcome::default();
    let mut paths: Vec<(PathBuf, u64)> = Vec::new();
    for l in items {
        if l.needs_admin && !elevated && !matches!(l.target, Target::Fs(_)) {
            out.needs_admin += 1;
            continue;
        }
        match &l.target {
            Target::Fs(p) => match fs_allowed(p, ctx, l.reason == Reason::InstallFolder) {
                Some(real) if real == *p => paths.push((real, l.bytes)),
                _ => out.failed += 1,
            },
            Target::Reg(root, path) => {
                // Only keys directly under the searched bases (or one publisher level below).
                let ok = ctx.reg.iter().any(|(r, b)| r == root && path.strip_prefix(&format!(r"{b}\")).is_some_and(|rest| !rest.is_empty() && rest.matches('\\').count() <= 1));
                let done = ok
                    && registry::export_tree(*root, path)
                        .ok()
                        .and_then(|tree| {
                            fs::create_dir_all(backup_dir()).ok()?;
                            let file = backup_dir().join(format!("{}-{}.json", registry::now_unix(), out.registry + out.failed));
                            let doc = serde_json::json!({ "app": app_name, "root": root, "path": path, "tree": tree });
                            fs::write(file, serde_json::to_string_pretty(&doc).ok()?).ok()
                        })
                        .is_some()
                    && registry::delete_tree(*root, path).is_ok();
                if done { out.registry += 1 } else { out.failed += 1 }
            }
            Target::Service(name) => {
                if crate::boost::delete_orphan_service(name).is_ok() {
                    out.services += 1;
                } else {
                    out.failed += 1;
                }
            }
        }
    }
    if !paths.is_empty() {
        let list: Vec<PathBuf> = paths.iter().map(|(p, _)| p.clone()).collect();
        crate::disk::shell_recycle(&list);
        for (p, bytes) in &paths {
            if fs::symlink_metadata(p).is_err() {
                out.recycled += 1;
                out.bytes += bytes;
            } else {
                out.failed += 1;
            }
        }
    }
    if out.recycled + out.registry + out.services > 0 {
        crate::cleanup::record_run(&crate::cleanup::RunRecord {
            at: registry::now_unix(),
            trigger: "app_leftovers".into(),
            duration_ms: t0.elapsed().as_millis() as u64,
            items: vec![crate::cleanup::ExecItem {
                id: "app_leftovers".into(),
                bytes_freed: out.bytes,
                files_deleted: u64::from(out.recycled + out.registry + out.services),
                skipped: u64::from(out.failed + out.needs_admin),
                error: None,
            }],
        });
    }
    out
}

/// What the shell keeps between "preview" and "remove": the leftovers of one app.
#[derive(Debug)]
pub struct Pending {
    pub app: App,
    pub items: Vec<Leftover>,
}

/// The pending items with these ids (unknown ids are ignored).
pub fn pick(p: &Pending, ids: &[u32]) -> Vec<Leftover> {
    let set: HashSet<u32> = ids.iter().copied().collect();
    p.items.iter().filter(|l| set.contains(&l.id)).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn app(name: &str, publisher: &str, exe: Option<&str>, install: Option<&str>) -> App {
        App {
            id: "u:test".into(),
            hive: Hive::User,
            key: "test".into(),
            name: name.into(),
            publisher: publisher.into(),
            version: "1.0".into(),
            install_location: install.map(str::to_string),
            exe: exe.map(str::to_string),
            size_bytes: None,
            install_date: None,
            uninstall: String::new(),
            msi: false,
            needs_admin: false,
            protected: None,
        }
    }

    #[test]
    fn names_normalize_and_match() {
        assert_eq!(norm("7-Zip 23.01 (x64)"), "7 zip");
        assert_eq!(norm("Mozilla Firefox (x64 pt-BR)"), "mozilla firefox");
        assert_eq!(norm("Discord Inc."), "discord");
        assert_eq!(norm("OBS Studio v30"), "obs studio");
        let m = Matcher::new(&app("Mozilla Firefox (x64 pt-BR)", "Mozilla", Some(r"C:\Program Files\Mozilla Firefox\firefox.exe"), None));
        assert_eq!(m.folder("Mozilla Firefox"), Some(Confidence::High));
        assert_eq!(m.folder("Firefox"), Some(Confidence::High), "product word without the publisher");
        assert_eq!(m.folder("Mozilla Firefox 2"), Some(Confidence::Medium));
        assert_eq!(m.folder("Mozilla Thunderbird"), None);
        let fc = Matcher::new(&app("EA SPORTS FC™ 27", "EA CANADA", None, None));
        assert_eq!(fc.folder("EA SPORTS FC 27"), Some(Confidence::High));
        assert_eq!(fc.folder("EA SPORTS FC 26"), None, "last year's game is another app");
        assert_eq!(fc.folder("EA SPORTS FC"), Some(Confidence::Medium), "shared by every year");
        assert!(m.is_publisher("Mozilla"));
        let short = Matcher::new(&app("Go", "Google", None, None));
        assert_eq!(short.folder("Go"), None, "too generic to match");
        let chrome = Matcher::new(&app("Google Chrome", "Google LLC", Some(r"C:\x\chrome.exe"), None));
        assert_eq!(chrome.folder("Chrome"), Some(Confidence::High));
        assert!(chrome.is_publisher("Google"));
        assert_eq!(chrome.folder("Google"), None, "the publisher folder itself is not the app");
    }

    #[test]
    fn uninstaller_validation() {
        let win = Path::new(r"C:\Windows");
        let mut a = app("X", "", None, None);
        for bad in [
            r"cmd.exe /c del C:\x",
            r#""C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe" -c x"#,
            r"rundll32.exe C:\x.dll,Uninstall",
            r"C:\Windows\System32\mshta.exe x",
            r#""C:\Windows\notepad.exe""#,
            "MsiExec.exe /X evil",
            "",
        ] {
            a.uninstall = bad.into();
            assert_eq!(uninstaller(&a, win), Err(AppsError::Blocked), "{bad}");
        }
        a.uninstall = "MsiExec.exe /I{12345678-ABCD-1234-ABCD-1234567890AB} /quiet".into();
        assert_eq!(
            uninstaller(&a, win).unwrap(),
            (PathBuf::from(r"C:\Windows\System32\msiexec.exe"), "/X{12345678-ABCD-1234-ABCD-1234567890AB}".into())
        );
        a.uninstall = r#""C:\Program Files\Nope\unins000.exe" /SILENT"#.into();
        assert_eq!(uninstaller(&a, win), Err(AppsError::Missing));
        let me = std::env::current_exe().unwrap();
        a.uninstall = format!(r#""{}" --uninstall"#, me.display());
        assert_eq!(uninstaller(&a, win).unwrap().1, "--uninstall");
        assert_eq!(parse_command(r"C:\Apps\x.exe.old\u.exe /S").unwrap().0, r"C:\Apps\x.exe.old\u.exe");
        a.protected = Some("runtime".into());
        assert_eq!(uninstall(&a), Err(AppsError::Protected));
    }

    #[test]
    fn runtimes_and_env() {
        assert!(runtime("Microsoft Visual C++ 2015-2022 Redistributable (x64)", "Microsoft Corporation"));
        assert!(runtime("NVIDIA Graphics Driver 560.94", "NVIDIA Corporation"));
        assert!(!runtime("Discord", "Discord Inc."));
        assert!(runtime("Microsoft GameInput", "Microsoft Corporation"));
        assert!(valid_family("Microsoft.ZuneMusic_8wekyb3d8bbwe"));
        for bad in ["", "..", r"..\Microsoft", "a/b", r"C:\x", "a b"] {
            assert!(!valid_family(bad), "{bad}");
        }
        unsafe { std::env::set_var("G2_TEST_VAR", r"C:\T") };
        assert_eq!(expand_env(r"%G2_TEST_VAR%\a %NOPE% 50%"), r"C:\T\a %NOPE% 50%");
    }

    /// Fabricated profile tree; the real machine is never scanned or touched.
    fn sandbox(tag: &str) -> (PathBuf, Ctx) {
        let root = std::env::temp_dir().join(format!("g2boost-apps-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let profile = root.join(r"Users\me");
        let roaming = profile.join(r"AppData\Roaming");
        let local = profile.join(r"AppData\Local");
        let program_data = root.join("ProgramData");
        for d in [
            roaming.join(r"Acme Widget\cache"),
            roaming.join(r"AcmeCorp\Acme Widget"),
            roaming.join(r"AcmeCorp\Other Tool"),
            roaming.join("Microsoft"),
            local.join(r"Acme Widget 2"),
            local.join(r"Programs\Acme Widget\bin"),
            local.join("CrashDumps"),
            program_data.join("Acme Widget"),
            roaming.join(r"Microsoft\Windows\Start Menu\Programs\Acme Widget"),
            root.join(r"Apps\Acme Widget\bin"),
            profile.join("Desktop"),
        ] {
            fs::create_dir_all(d).unwrap();
        }
        let mut f = fs::File::create(roaming.join(r"Acme Widget\cache\blob.bin")).unwrap();
        f.write_all(&[0u8; 2048]).unwrap();
        fs::write(local.join(r"CrashDumps\widget.exe.1234.dmp"), [0u8; 10]).unwrap();
        fs::write(local.join(r"CrashDumps\other.exe.1.dmp"), [0u8; 10]).unwrap();
        let ctx = Ctx {
            windows: PathBuf::from(r"C:\Windows"),
            profile: profile.clone(),
            data: vec![roaming.clone(), local.clone(), program_data, local.join("Programs")],
            shortcuts: vec![roaming.join(r"Microsoft\Windows\Start Menu\Programs"), profile.join("Desktop")],
            dumps: Some(local.join("CrashDumps")),
            reg: vec![],
            services: false,
        };
        (root, ctx)
    }

    #[test]
    fn finds_leftovers_with_reasons_in_a_sandbox() {
        let (root, ctx) = sandbox("find");
        let install = ctx.data[3].join("Acme Widget"); // LocalAppData\Programs (per-user install)
        let a = app("Acme Widget", "AcmeCorp", Some(r"C:\x\widget.exe"), Some(&install.to_string_lossy()));
        let found = leftovers_in(&a, &ctx);
        let by = |suffix: &str| found.iter().find(|l| l.path.to_ascii_lowercase().ends_with(&suffix.to_ascii_lowercase()));
        let roaming = by(r"Roaming\Acme Widget").expect("roaming folder");
        assert_eq!((roaming.confidence, roaming.reason, roaming.bytes), (Confidence::High, Reason::NameMatch, 2048));
        assert_eq!(by(r"AcmeCorp\Acme Widget").expect("publisher subfolder").reason, Reason::PublisherFolder);
        assert_eq!(by(r"Local\Acme Widget 2").expect("versioned folder").confidence, Confidence::Medium);
        assert!(by(r"ProgramData\Acme Widget").is_some());
        assert!(by(r"Start Menu\Programs\Acme Widget").is_some());
        assert_eq!(by("widget.exe.1234.dmp").expect("crash dump").reason, Reason::CrashDump);
        assert!(by("other.exe.1.dmp").is_none());
        assert!(by(r"AcmeCorp\Other Tool").is_none(), "another app of the same publisher stays");
        assert!(by(r"Roaming\AcmeCorp").is_none(), "the publisher folder itself stays");
        assert!(by("Microsoft").is_none());
        let inst = by(r"Programs\Acme Widget").expect("install folder");
        assert_eq!(inst.reason, Reason::InstallFolder);
        assert!(found.windows(2).all(|w| w[0].confidence >= w[1].confidence));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn guard_refuses_roots_profiles_system_and_junctions() {
        let (root, ctx) = sandbox("guard");
        let roaming = ctx.data[0].clone();
        assert!(fs_allowed(&roaming, &ctx, false).is_none(), "a base root itself");
        assert!(fs_allowed(&ctx.profile, &ctx, true).is_none(), "the profile");
        assert!(fs_allowed(Path::new(r"C:\Windows\System32"), &ctx, true).is_none());
        assert!(fs_allowed(&roaming.join("Acme Widget"), &ctx, false).is_some());
        // Install folder outside the bases: only when it is below an install place.
        assert!(fs_allowed(&root.join(r"Apps\Acme Widget"), &ctx, false).is_none(), "not an install folder");
        // A junction planted where an app folder was: never followed, never removed.
        let link = roaming.join("Acme Junction");
        crate::cleanup::tests::make_junction(&link, &std::env::temp_dir());
        assert!(fs_allowed(&link, &ctx, false).is_none());
        let a = app("Acme Junction", "", None, None);
        assert!(leftovers_in(&a, &ctx).iter().all(|l| !l.path.contains("Acme Junction")));
        fs::remove_dir(&link).unwrap();
        // An install location pointing at the profile (hostile snapshot) is refused.
        let evil = app("Me", "", None, Some(&ctx.profile.to_string_lossy()));
        assert!(leftovers_in(&evil, &ctx).iter().all(|l| l.reason != Reason::InstallFolder));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn remove_recycles_only_rechecked_paths() {
        // SAFETY: history goes to the test data dir, never the real one.
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
        let (root, ctx) = sandbox("remove");
        let a = app("Acme Widget", "AcmeCorp", None, None);
        let found = leftovers_in(&a, &ctx);
        let target = found.iter().find(|l| l.path.ends_with(r"Roaming\Acme Widget")).unwrap().clone();
        // Swap the folder for a junction after the preview (TOCTOU): refused at removal time.
        let real = PathBuf::from(&target.path);
        fs::remove_dir_all(&real).unwrap();
        crate::cleanup::tests::make_junction(&real, &std::env::temp_dir());
        let out = remove_in(std::slice::from_ref(&target), "Acme Widget", &ctx, false);
        assert_eq!((out.recycled, out.failed), (0, 1));
        assert!(std::env::temp_dir().exists(), "nothing behind the junction was touched");
        fs::remove_dir(&real).unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    /// Read-only look at real leftovers (`cargo test -p g2boost-core -- --ignored leftovers_live --nocapture`).
    #[test]
    #[ignore]
    fn leftovers_live() {
        for a in list(&[]).iter().take(40) {
            let l = leftovers(a);
            eprintln!("{} [{}] protected={:?}", a.name, a.publisher, a.protected);
            for x in l.iter().take(6) {
                eprintln!("    {:?} {:?} {:?} {} ({} B)", x.confidence, x.kind, x.reason, x.path, x.bytes);
            }
        }
    }

    #[test]
    fn live_listing_is_sane() {
        let apps = list(&[]);
        assert!(apps.iter().all(|a| !a.name.is_empty() && !a.uninstall.is_empty()));
        assert!(apps.iter().all(|a| ["m64:", "m32:", "u:", "s:"].iter().any(|p| a.id.starts_with(p))));
        let mut seen = std::collections::HashSet::new();
        assert!(apps.iter().all(|a| seen.insert((a.name.to_lowercase(), a.version.clone()))), "deduplicated");
        if let Some(a) = apps.first() {
            assert!(installed(a));
        }
        // Store packages: listed with a valid family name and no install folder to offer.
        if let Some(s) = apps.iter().find(|a| a.hive == Hive::Store) {
            assert!(valid_family(&s.uninstall) && s.install_location.is_none());
            assert!(installed(s));
        }
        assert!(apps.iter().filter(|a| a.hive == Hive::Store).all(|a| !a.uninstall.to_ascii_lowercase().starts_with("microsoft.windowsstore") || a.protected.is_some()));
        assert!(resolve_drop("relative.exe", &apps).is_none());
    }
}
