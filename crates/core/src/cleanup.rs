//! Cleanup: catalog → preview (bytes) → execute (with a one-shot `ConfirmToken`) → history.
//!
//! Only catalog paths are ever touched; every root goes through [`SafePath`]; reparse points
//! (junctions/symlinks) are never followed or deleted through; files in use are skipped and
//! counted. Special handlers: Recycle Bin (`SHEmptyRecycleBinW`), DNS cache
//! (`DnsFlushResolverCache`, loaded dynamically), component store (Windows' own
//! `StartComponentCleanup` task). No `cmd`, no `rd /s`.

use std::collections::HashSet;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Shell::{
    SHEmptyRecycleBinW, SHQUERYRBINFO, SHQueryRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND,
};
use windows::core::PCWSTR;

use crate::registry::{self, wide};
use crate::safety::{PathClass, SafePath, SafePathError, is_reparse};
use crate::system;
use crate::tasksched;

const TOKEN_TTL: Duration = Duration::from_secs(120);
const HISTORY_MAX: usize = 100;

/* ---------------- catalog ---------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Daily,
    Monthly,
    Advanced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy)]
enum Action {
    /// Delete everything under the resolved directories (never the directories themselves).
    DirContents,
    /// Delete only top-level files whose name matches one of the globs (`thumbcache_*.db`).
    TopFiles(&'static [&'static str]),
    RecycleBin,
    DnsFlush,
    ComponentCleanup,
}

#[derive(Debug, Clone, Copy)]
pub struct Category {
    pub id: &'static str,
    pub group: Group,
    pub risk: Risk,
    pub admin: bool,
    /// Exe names (lowercase) that should be closed; files they hold are skipped anyway.
    pub requires_closed: &'static [&'static str],
    pub default_on: bool,
    action: Action,
}

const fn cat(
    id: &'static str,
    group: Group,
    risk: Risk,
    admin: bool,
    requires_closed: &'static [&'static str],
    default_on: bool,
    action: Action,
) -> Category {
    Category { id, group, risk, admin, requires_closed, default_on, action }
}

/// Ported from the legacy .bat scripts (Docs/references/legacy-bat-scripts.md). `spool` is out:
/// it lives under `System32`, which `SafePath` refuses on purpose. `prefetch` is out for good.
pub const CATALOG: &[Category] = &[
    cat("temp_windows", Group::Daily, Risk::Low, true, &[], true, Action::DirContents),
    cat("temp_user", Group::Daily, Risk::Low, false, &[], true, Action::DirContents),
    cat("wer", Group::Daily, Risk::Low, true, &[], true, Action::DirContents),
    cat("shaders", Group::Daily, Risk::Medium, false, &[], false, Action::DirContents),
    cat("thumbs", Group::Daily, Risk::Low, false, &["explorer.exe"], true, Action::TopFiles(&["thumbcache_*.db", "iconcache_*.db", "IconCache.db"])),
    cat("dns_flush", Group::Daily, Risk::Low, false, &[], true, Action::DnsFlush),
    cat("recycle", Group::Daily, Risk::High, false, &[], false, Action::RecycleBin),
    cat("wu_cache", Group::Monthly, Risk::Medium, true, &[], true, Action::DirContents),
    cat("nvidia_downloads", Group::Monthly, Risk::Low, true, &["nvidia app.exe"], true, Action::DirContents),
    cat("inetcache", Group::Monthly, Risk::Low, false, &["msedge.exe"], true, Action::DirContents),
    cat("chrome_cache", Group::Monthly, Risk::Low, false, &["chrome.exe"], true, Action::DirContents),
    cat("edge_cache", Group::Monthly, Risk::Low, false, &["msedge.exe"], true, Action::DirContents),
    cat("discord_cache", Group::Monthly, Risk::Low, false, &["discord.exe"], true, Action::DirContents),
    cat("spotify_cache", Group::Monthly, Risk::Low, false, &["spotify.exe"], false, Action::DirContents),
    cat("firefox_cache", Group::Monthly, Risk::Low, false, &["firefox.exe"], true, Action::DirContents),
    cat("brave_cache", Group::Monthly, Risk::Low, false, &["brave.exe"], true, Action::DirContents),
    cat("opera_cache", Group::Monthly, Risk::Low, false, &["opera.exe"], true, Action::DirContents),
    cat("vivaldi_cache", Group::Monthly, Risk::Low, false, &["vivaldi.exe"], true, Action::DirContents),
    cat("steam_cache", Group::Monthly, Risk::Low, false, &["steam.exe", "steamwebhelper.exe"], true, Action::DirContents),
    cat("epic_cache", Group::Monthly, Risk::Low, false, &["epicgameslauncher.exe"], true, Action::DirContents),
    cat("battlenet_cache", Group::Monthly, Risk::Low, false, &["battle.net.exe"], true, Action::DirContents),
    cat("ea_cache", Group::Monthly, Risk::Low, false, &["eadesktop.exe"], true, Action::DirContents),
    cat("ubisoft_cache", Group::Monthly, Risk::Low, false, &["upc.exe"], true, Action::DirContents),
    cat("teams_cache", Group::Monthly, Risk::Low, false, &["teams.exe"], true, Action::DirContents),
    cat("slack_cache", Group::Monthly, Risk::Low, false, &["slack.exe"], true, Action::DirContents),
    cat("vscode_cache", Group::Monthly, Risk::Low, false, &["code.exe"], true, Action::DirContents),
    // Developer caches: re-downloaded on the next install/restore, so medium risk and opt-in.
    cat("dev_npm", Group::Advanced, Risk::Medium, false, &[], false, Action::DirContents),
    cat("dev_pip", Group::Advanced, Risk::Medium, false, &[], false, Action::DirContents),
    cat("dev_nuget", Group::Advanced, Risk::Medium, false, &[], false, Action::DirContents),
    cat("dev_yarn", Group::Advanced, Risk::Medium, false, &[], false, Action::DirContents),
    cat("dism", Group::Monthly, Risk::Medium, true, &[], false, Action::ComponentCleanup),
    cat("logs_windows", Group::Advanced, Risk::Medium, true, &[], false, Action::TopFiles(&["*.log", "*.etl"])),
    cat("dumps", Group::Advanced, Risk::Medium, true, &[], false, Action::DirContents),
];

pub fn find(id: &str) -> Option<&'static Category> {
    CATALOG.iter().find(|c| c.id == id)
}

pub(crate) fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute())
}

fn under(base: Option<PathBuf>, rel: &str) -> Option<PathBuf> {
    base.map(|b| b.join(rel))
}

/// Real (non-reparse) child directories of `base` whose name passes `keep`. Profile folders that
/// are junctions/symlinks are skipped so a cache path never routes through one.
fn child_dirs(base: Option<PathBuf>, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let Some(Ok(rd)) = base.map(fs::read_dir) else {
        return Vec::new();
    };
    rd.flatten()
        .filter(|e| e.metadata().is_ok_and(|md| md.is_dir() && !is_reparse(&md)))
        .filter(|e| keep(&e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .collect()
}

/// Chromium "User Data" profiles: `Default`, `Profile N`, `Guest Profile`. Only the cache
/// folders inside them; never `Cookies`, `Login Data`, `Preferences` or `Local State`.
fn chromium_caches(user_data: Option<PathBuf>) -> Vec<PathBuf> {
    let profiles = child_dirs(user_data, |n| n == "Default" || n.starts_with("Profile ") || n == "Guest Profile");
    profiles.iter().flat_map(|p| ["Cache\\Cache_Data", "Code Cache", "GPUCache"].map(|s| p.join(s))).collect()
}

/// Firefox keeps profile data in Roaming; the Local `Profiles\*` folders hold only `cache2`.
fn firefox_caches(profiles: Option<PathBuf>) -> Vec<PathBuf> {
    child_dirs(profiles, |_| true).into_iter().map(|p| p.join("cache2")).collect()
}

/// Opera / Opera GX: cache folders under the Local channel folder (flat layout and the newer
/// `Default\` layout). The profile itself lives in Roaming and is never listed.
fn opera_caches(opera_software: Option<PathBuf>) -> Vec<PathBuf> {
    let channels = child_dirs(opera_software, |n| n == "Opera Stable" || n == "Opera GX Stable");
    channels
        .iter()
        .flat_map(|c| [c.clone(), c.join("Default")])
        .flat_map(|r| ["Cache", "Code Cache", "GPUCache"].map(|s| r.join(s)))
        .collect()
}

/// Epic Games Launcher web caches: `Saved\webcache`, `webcache_4147`, ... (one per CEF version).
fn epic_caches(saved: Option<PathBuf>) -> Vec<PathBuf> {
    child_dirs(saved, |n| n.to_ascii_lowercase().starts_with("webcache"))
}

/// Resolved candidate directories for a category (may not exist yet; filtered later).
pub fn paths(id: &str) -> Vec<PathBuf> {
    let local = || env_dir("LOCALAPPDATA");
    let roaming = || env_dir("APPDATA");
    let program_data = || env_dir("PROGRAMDATA");
    let win = || env_dir("SystemRoot");
    let list: Vec<Option<PathBuf>> = match id {
        "temp_windows" => vec![under(win(), "Temp")],
        "temp_user" => vec![env_dir("TEMP"), env_dir("TMP")],
        "wer" => vec![
            under(local(), r"Microsoft\Windows\WER\ReportArchive"),
            under(local(), r"Microsoft\Windows\WER\ReportQueue"),
            under(local(), r"Microsoft\Windows\WER\Temp"),
            under(program_data(), r"Microsoft\Windows\WER\ReportArchive"),
            under(program_data(), r"Microsoft\Windows\WER\ReportQueue"),
            under(program_data(), r"Microsoft\Windows\WER\Temp"),
        ],
        "shaders" => vec![
            under(local(), r"NVIDIA\DXCache"),
            under(local(), r"NVIDIA\GLCache"),
            under(local(), r"NVIDIA Corporation\NV_Cache"),
            under(program_data(), r"NVIDIA Corporation\NV_Cache"),
            under(local(), r"AMD\DxCache"),
            under(local(), r"AMD\DXCache"),
            under(local(), r"AMD\GLCache"),
            under(local(), r"AMD\VkCache"),
            under(local(), r"D3DSCache"),
            under(local(), r"Intel\ShaderCache"),
        ],
        "thumbs" => vec![under(local(), r"Microsoft\Windows\Explorer"), local()],
        "wu_cache" => vec![
            under(win(), r"SoftwareDistribution\Download"),
            under(win(), r"ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache"),
        ],
        "nvidia_downloads" => vec![
            under(program_data(), r"NVIDIA Corporation\NVIDIA app\UpdateFramework\ota-artifacts"),
            under(program_data(), r"NVIDIA Corporation\Downloader"),
        ],
        "inetcache" => vec![under(local(), r"Microsoft\Windows\INetCache")],
        "chrome_cache" => return chromium_caches(under(local(), r"Google\Chrome\User Data")),
        "edge_cache" => return chromium_caches(under(local(), r"Microsoft\Edge\User Data")),
        "firefox_cache" => return firefox_caches(under(local(), r"Mozilla\Firefox\Profiles")),
        "brave_cache" => return chromium_caches(under(local(), r"BraveSoftware\Brave-Browser\User Data")),
        "opera_cache" => return opera_caches(under(local(), "Opera Software")),
        "vivaldi_cache" => return chromium_caches(under(local(), r"Vivaldi\User Data")),
        // Steam's `htmlcache` is a Chromium user-data root (cookies, logins): only its cache folders.
        "steam_cache" => return chromium_caches(under(local(), r"Steam\htmlcache")),
        "epic_cache" => return epic_caches(under(local(), r"EpicGamesLauncher\Saved")),
        "battlenet_cache" => vec![
            under(program_data(), r"Battle.net\Agent\data\cache"),
            under(local(), r"Battle.net\Cache"),
            under(local(), r"Battle.net\BrowserCache"),
        ],
        "ea_cache" => vec![
            under(local(), r"Electronic Arts\EA Desktop\cache"),
            under(local(), r"Electronic Arts\EA Desktop\CEF\BrowserCache"),
        ],
        "ubisoft_cache" => vec![under(local(), r"Ubisoft Game Launcher\cache")],
        // Classic Teams only; new Teams (MSTeams package, EBWebView) is left alone on purpose.
        "teams_cache" => vec![
            under(roaming(), r"Microsoft\Teams\Cache"),
            under(roaming(), r"Microsoft\Teams\Code Cache"),
            under(roaming(), r"Microsoft\Teams\GPUCache"),
        ],
        "slack_cache" => vec![
            under(roaming(), r"Slack\Cache"),
            under(roaming(), r"Slack\Code Cache"),
            under(roaming(), r"Slack\GPUCache"),
        ],
        "vscode_cache" => vec![
            under(roaming(), r"Code\Cache"),
            under(roaming(), r"Code\CachedData"),
            under(roaming(), r"Code\Code Cache"),
            under(roaming(), r"Code\GPUCache"),
        ],
        "dev_npm" => vec![under(local(), "npm-cache")],
        "dev_pip" => vec![under(local(), r"pip\cache")],
        "dev_nuget" => vec![under(local(), r"NuGet\v3-cache")],
        "dev_yarn" => vec![under(local(), r"Yarn\Cache")],
        "discord_cache" => vec![
            under(roaming(), r"discord\Cache"),
            under(roaming(), r"discord\Code Cache"),
            under(roaming(), r"discord\GPUCache"),
        ],
        "spotify_cache" => {
            let mut v = vec![under(local(), r"Spotify\Data")];
            if let Some(pk) = under(local(), "Packages")
                && let Ok(rd) = fs::read_dir(pk)
            {
                for e in rd.flatten() {
                    if e.file_name().to_string_lossy().starts_with("SpotifyAB.SpotifyMusic_") {
                        v.push(Some(e.path().join(r"LocalCache\Spotify\Data")));
                    }
                }
            }
            v
        }
        "logs_windows" => vec![
            under(win(), r"Logs\CBS"),
            under(win(), r"Logs\WindowsUpdate"),
            under(win(), r"Logs\DISM"),
        ],
        "dumps" => vec![under(local(), "CrashDumps"), under(win(), "Minidump")],
        _ => Vec::new(),
    };
    list.into_iter().flatten().collect()
}

/// Validate one catalog root: absolute, inside an allowed class, exists, not a reparse point.
fn checked_root(p: &Path) -> Result<Option<SafePath>, SafePathError> {
    let win = env_dir("SystemRoot").unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let profile = env_dir("USERPROFILE").unwrap_or_else(|| PathBuf::from(r"C:\Users\Default"));
    let safe = SafePath::classify(p, &win, &profile)?;
    if !matches!(safe.class(), PathClass::System | PathClass::UserProfile | PathClass::Other) {
        return Err(SafePathError::SystemCritical);
    }
    let Ok(md) = fs::symlink_metadata(p) else {
        return Ok(None); // does not exist on this machine
    };
    if !md.is_dir() || is_reparse(&md) {
        return Ok(None);
    }
    Ok(Some(safe))
}

/// `thumbcache_*.db` style match: one `*` at most, case-insensitive.
fn glob_match(pattern: &str, name: &str) -> bool {
    let (p, n) = (pattern.to_ascii_lowercase(), name.to_ascii_lowercase());
    match p.split_once('*') {
        None => p == n,
        Some((pre, suf)) => n.len() >= pre.len() + suf.len() && n.starts_with(pre) && n.ends_with(suf),
    }
}

/* ---------------- walking / deleting ---------------- */

#[derive(Debug, Default, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct Tally {
    pub bytes: u64,
    pub files: u64,
    /// In use / access denied / vanished while deleting.
    pub skipped: u64,
}

/// Sum files under `dir` (recursive), never crossing reparse points.
pub(crate) fn measure_dir(dir: &Path, t: &mut Tally) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let Ok(md) = e.metadata() else {
            continue;
        };
        if is_reparse(&md) {
            continue;
        }
        if md.is_dir() {
            measure_dir(&e.path(), t);
        } else {
            t.bytes += md.len();
            t.files += 1;
        }
    }
}

/// Delete files under `dir` and then the emptied subdirectories (bottom-up). `dir` itself stays.
pub(crate) fn delete_dir_contents(dir: &Path, t: &mut Tally) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let Ok(md) = e.metadata() else {
            t.skipped += 1;
            continue;
        };
        if is_reparse(&md) {
            continue; // never follow, never remove: not ours
        }
        let p = e.path();
        if md.is_dir() {
            delete_dir_contents(&p, t);
            let _ = fs::remove_dir(&p); // only succeeds when empty
        } else {
            let len = md.len();
            match fs::remove_file(&p) {
                Ok(()) => {
                    t.bytes += len;
                    t.files += 1;
                }
                Err(_) => t.skipped += 1,
            }
        }
    }
}

fn top_files(dir: &Path, globs: &[&str]) -> Vec<(PathBuf, u64)> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.flatten()
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            if md.is_dir() || is_reparse(&md) {
                return None;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            globs.iter().any(|g| glob_match(g, &name)).then(|| (e.path(), md.len()))
        })
        .collect()
}

fn recycle_bin_info() -> Option<(u64, u64)> {
    let mut info = SHQUERYRBINFO { cbSize: size_of::<SHQUERYRBINFO>() as u32, ..Default::default() };
    // SAFETY: sized struct; null root = all drives.
    unsafe { SHQueryRecycleBinW(PCWSTR::null(), &mut info) }.ok()?;
    Some((info.i64Size.max(0) as u64, info.i64NumItems.max(0) as u64))
}

fn empty_recycle_bin() -> Result<(), String> {
    // SAFETY: no window, all drives, silent flags.
    match unsafe { SHEmptyRecycleBinW(None, PCWSTR::null(), SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND) } {
        Ok(()) => Ok(()),
        // E_UNEXPECTED: already empty.
        Err(e) if e.code().0 as u32 == 0x8000_FFFF => Ok(()),
        Err(e) => Err(e.message()),
    }
}

/// Best-effort DNS cache flush (used after changing resolvers).
pub fn flush_dns_cache() {
    let _ = dns_flush();
}

/// `DnsFlushResolverCache` is exported by dnsapi.dll but not declared in the SDK headers.
pub(crate) fn dns_flush() -> Result<(), String> {
    type Flush = unsafe extern "system" fn() -> i32;
    let lib = wide("dnsapi.dll");
    // SAFETY: well-known system DLL; the export has the documented-by-practice signature.
    unsafe {
        let h = LoadLibraryW(PCWSTR(lib.as_ptr())).map_err(|e| e.message())?;
        let Some(f) = GetProcAddress(h, windows::core::s!("DnsFlushResolverCache")) else {
            return Err("DnsFlushResolverCache not found".into());
        };
        let f: Flush = std::mem::transmute::<unsafe extern "system" fn() -> isize, Flush>(f);
        if f() == 0 { Err("DnsFlushResolverCache failed".into()) } else { Ok(()) }
    }
}

/* ---------------- preview ---------------- */

#[derive(Debug, Clone, Serialize)]
pub struct PreviewItem {
    pub id: &'static str,
    pub bytes: u64,
    pub files: u64,
    /// DNS flush / component cleanup have no measurable size.
    pub measurable: bool,
    /// None of the category's paths exist on this machine.
    pub missing: bool,
    /// Elevation required and the process is not elevated.
    pub needs_admin: bool,
    /// Running processes from `requires_closed`.
    pub blocked_by: Vec<String>,
}

pub fn preview(ids: &[String]) -> Vec<PreviewItem> {
    let elevated = system::is_elevated();
    let running: HashSet<String> = system::running_processes();
    ids.iter()
        .filter_map(|id| find(id))
        .map(|c| {
            let mut item = PreviewItem {
                id: c.id,
                bytes: 0,
                files: 0,
                measurable: true,
                missing: false,
                needs_admin: c.admin && !elevated,
                blocked_by: c.requires_closed.iter().filter(|p| running.contains(**p)).map(|p| p.to_string()).collect(),
            };
            match c.action {
                Action::DirContents | Action::TopFiles(_) => {
                    let roots: Vec<PathBuf> = paths(c.id).into_iter().filter(|p| matches!(checked_root(p), Ok(Some(_)))).collect();
                    item.missing = roots.is_empty();
                    let mut t = Tally::default();
                    for r in &roots {
                        match c.action {
                            Action::DirContents => measure_dir(r, &mut t),
                            Action::TopFiles(globs) => {
                                for (_, len) in top_files(r, globs) {
                                    t.bytes += len;
                                    t.files += 1;
                                }
                            }
                            _ => unreachable!(),
                        }
                    }
                    item.bytes = t.bytes;
                    item.files = t.files;
                }
                Action::RecycleBin => {
                    if let Some((b, n)) = recycle_bin_info() {
                        item.bytes = b;
                        item.files = n;
                    }
                }
                Action::DnsFlush | Action::ComponentCleanup => item.measurable = false,
            }
            item
        })
        .collect()
}

/* ---------------- confirm token ---------------- */

/// Issued by the UI after the human confirms a preview; consumed exactly once by `execute`.
/// The MCP server never gets to mint one.
#[derive(Debug, Clone, Serialize)]
pub struct ConfirmToken {
    pub token: u64,
    pub ids: Vec<String>,
    #[serde(skip)]
    pub issued: Instant,
}

#[derive(Default)]
pub struct TokenStore {
    tokens: Mutex<Vec<ConfirmToken>>,
}

impl TokenStore {
    pub fn issue(&self, ids: Vec<String>) -> ConfirmToken {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(Instant::now().elapsed().as_nanos() ^ (ids.len() as u128));
        // 53 bits: the token round-trips through JavaScript, whose numbers are exact only up to
        // 2^53 (a 64-bit value came back rounded and never matched). Still unguessable for a
        // one-shot, 2-minute token bound to the exact ids.
        let token = ConfirmToken { token: (h.finish() & ((1u64 << 53) - 1)) | 1, ids, issued: Instant::now() };
        let mut guard = self.tokens.lock().unwrap();
        guard.retain(|t| t.issued.elapsed() < TOKEN_TTL);
        guard.push(token.clone());
        token
    }

    /// Take the token if it exists, is fresh and matches `ids` exactly.
    pub fn consume(&self, token: u64, ids: &[String]) -> bool {
        let mut guard = self.tokens.lock().unwrap();
        let pos = guard
            .iter()
            .position(|t| t.token == token && t.issued.elapsed() < TOKEN_TTL && t.ids == ids);
        match pos {
            Some(i) => {
                guard.remove(i);
                true
            }
            None => false,
        }
    }
}

/* ---------------- execute ---------------- */

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecItem {
    pub id: String,
    pub bytes_freed: u64,
    pub files_deleted: u64,
    pub skipped: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub at: u64,
    pub trigger: String,
    pub duration_ms: u64,
    pub items: Vec<ExecItem>,
}

#[derive(Debug, thiserror::Error)]
pub enum CleanupError {
    #[error("bad_token")]
    BadToken,
    #[error("unknown_category")]
    UnknownCategory,
}

/// Run the categories in `ids` (already confirmed). `on_progress(done, total)` after each one.
pub fn execute(ids: &[String], trigger: &str, mut on_progress: impl FnMut(usize, usize)) -> RunRecord {
    let t0 = Instant::now();
    let elevated = system::is_elevated();
    let mut items = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        let Some(c) = find(id) else {
            items.push(ExecItem { id: id.clone(), error: Some("unknown_category".into()), ..Default::default() });
            continue;
        };
        let mut item = ExecItem { id: c.id.to_string(), ..Default::default() };
        if c.admin && !elevated {
            item.error = Some("needs_admin".into());
        } else {
            run_one(c, &mut item);
        }
        items.push(item);
        on_progress(i + 1, ids.len());
    }
    let record = RunRecord { at: registry::now_unix(), trigger: trigger.to_string(), duration_ms: t0.elapsed().as_millis() as u64, items };
    record_run(&record);
    record
}

fn run_one(c: &Category, item: &mut ExecItem) {
    match c.action {
        Action::DirContents | Action::TopFiles(_) => {
            let mut t = Tally::default();
            for p in paths(c.id) {
                let Ok(Some(_)) = checked_root(&p) else {
                    continue;
                };
                match c.action {
                    Action::DirContents => delete_dir_contents(&p, &mut t),
                    Action::TopFiles(globs) => {
                        for (f, len) in top_files(&p, globs) {
                            match fs::remove_file(&f) {
                                Ok(()) => {
                                    t.bytes += len;
                                    t.files += 1;
                                }
                                Err(_) => t.skipped += 1,
                            }
                        }
                    }
                    _ => unreachable!(),
                }
            }
            item.bytes_freed = t.bytes;
            item.files_deleted = t.files;
            item.skipped = t.skipped;
        }
        Action::RecycleBin => {
            let before = recycle_bin_info();
            match empty_recycle_bin() {
                Ok(()) => {
                    if let Some((b, n)) = before {
                        item.bytes_freed = b;
                        item.files_deleted = n;
                    }
                }
                Err(e) => item.error = Some(e),
            }
        }
        Action::DnsFlush => {
            if let Err(e) = dns_flush() {
                item.error = Some(e);
            }
        }
        Action::ComponentCleanup => {
            if let Err(e) = tasksched::run_task("\\Microsoft\\Windows\\Servicing", "StartComponentCleanup") {
                item.error = Some(e.to_string());
            }
        }
    }
}

/* ---------------- history (local, no paths, no PII) ---------------- */

fn history_file() -> PathBuf {
    registry::data_dir().join("history.json")
}

pub fn history() -> Vec<RunRecord> {
    fs::read_to_string(history_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Append one run (manual, routine or game trigger) to the local history.
pub fn record_run(r: &RunRecord) {
    let mut all = history();
    all.push(r.clone());
    if all.len() > HISTORY_MAX {
        let drop = all.len() - HISTORY_MAX;
        all.drain(..drop);
    }
    if let Some(dir) = history_file().parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(history_file(), serde_json::to_string_pretty(&all).unwrap_or_default());
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::ffi::c_void;
    use std::io::Write;
    use windows::Win32::Foundation::{CloseHandle, GENERIC_WRITE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_EXISTING,
    };
    use windows::Win32::System::IO::DeviceIoControl;

    const FSCTL_SET_REPARSE_POINT: u32 = 0x000900A4;
    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;

    /// A real NTFS junction `link` → `target` (no privilege needed, unlike symlinks).
    pub(crate) fn make_junction(link: &Path, target: &Path) {
        fs::create_dir_all(link).unwrap();
        let subst: Vec<u16> = format!(r"\??\{}", target.display()).encode_utf16().collect();
        let print: Vec<u16> = target.to_string_lossy().encode_utf16().collect();
        let mut path_buf: Vec<u16> = Vec::new();
        path_buf.extend(&subst);
        path_buf.push(0);
        path_buf.extend(&print);
        path_buf.push(0);
        let path_bytes = path_buf.len() * 2;
        let data_len = 8 + path_bytes; // 4 u16 offsets/lengths + PathBuffer
        let mut buf: Vec<u8> = Vec::new();
        buf.extend(IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        buf.extend((data_len as u16).to_le_bytes());
        buf.extend(0u16.to_le_bytes()); // Reserved
        buf.extend(0u16.to_le_bytes()); // SubstituteNameOffset
        buf.extend(((subst.len() * 2) as u16).to_le_bytes());
        buf.extend(((subst.len() * 2 + 2) as u16).to_le_bytes()); // PrintNameOffset
        buf.extend(((print.len() * 2) as u16).to_le_bytes());
        for u in &path_buf {
            buf.extend(u.to_le_bytes());
        }
        let w = wide(&link.to_string_lossy());
        // SAFETY: open the empty directory as a reparse point and set the mount-point buffer.
        unsafe {
            let h = CreateFileW(
                PCWSTR(w.as_ptr()),
                GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
            .unwrap();
            let mut ret = 0u32;
            DeviceIoControl(h, FSCTL_SET_REPARSE_POINT, Some(buf.as_ptr().cast::<c_void>()), buf.len() as u32, None, 0, Some(&mut ret), None)
                .unwrap();
            let _ = CloseHandle(h);
        }
    }

    fn sandbox(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("g2boost-cleanup-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("cache/sub")).unwrap();
        fs::create_dir_all(dir.join("victim")).unwrap();
        let mk = |p: &str, n: usize| {
            let mut f = fs::File::create(dir.join(p)).unwrap();
            f.write_all(&vec![0u8; n]).unwrap();
        };
        mk("cache/a.tmp", 1000);
        mk("cache/sub/b.tmp", 2000);
        mk("cache/thumbcache_32.db", 300);
        mk("victim/keep.txt", 50);
        dir
    }

    #[test]
    fn deletes_contents_but_never_crosses_the_junction() {
        let dir = sandbox("junction");
        make_junction(&dir.join("cache/link"), &dir.join("victim"));
        // The junction resolves: reading through it sees the victim file.
        assert!(dir.join("cache/link/keep.txt").exists());

        let mut t = Tally::default();
        measure_dir(&dir.join("cache"), &mut t);
        assert_eq!(t, Tally { bytes: 3300, files: 3, skipped: 0 });

        let mut t = Tally::default();
        delete_dir_contents(&dir.join("cache"), &mut t);
        assert_eq!(t.files, 3);
        assert_eq!(t.bytes, 3300);
        assert!(!dir.join("cache/a.tmp").exists());
        assert!(!dir.join("cache/sub").exists(), "emptied subdir is removed");
        assert!(dir.join("victim/keep.txt").exists(), "junction target untouched");
        assert!(dir.join("cache").exists(), "root dir itself stays");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Fabricated profile tree: cache folders plus files that must survive.
    fn fake_profile(root: &Path, caches: &[&str], keep: &[&str]) {
        for c in caches {
            fs::create_dir_all(root.join(c)).unwrap();
            fs::write(root.join(c).join("data_0"), vec![0u8; 100]).unwrap();
        }
        for k in keep {
            fs::create_dir_all(root.join(k).parent().unwrap()).unwrap();
            fs::write(root.join(k), b"precious").unwrap();
        }
    }

    fn tmp_tree(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("g2boost-cleanup-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn multi_profile_patterns_find_only_cache_folders() {
        let dir = tmp_tree("profiles");
        let keep = ["Cookies", "Login Data", "Preferences", "History", "Network\\Cookies"];
        // Chromium (Chrome/Edge/Brave/Vivaldi/Steam htmlcache): Default, Profile N, Guest.
        let ud = dir.join("User Data");
        let chrome_caches = ["Cache\\Cache_Data", "Code Cache", "GPUCache"];
        for prof in ["Default", "Profile 2", "Guest Profile", "System Profile"] {
            fake_profile(&ud.join(prof), &chrome_caches, &keep);
        }
        fs::write(ud.join("Local State"), b"precious").unwrap();
        let found = chromium_caches(Some(ud.clone()));
        assert_eq!(found.len(), 9, "System Profile is not a listed profile");
        for f in &found {
            assert!(f.exists() && f.starts_with(&ud), "{}", f.display());
            assert!(!f.starts_with(ud.join("System Profile")));
        }
        // Firefox: only `cache2` per profile.
        let ff = dir.join("Profiles");
        fake_profile(&ff.join("abc.default-release"), &["cache2"], &["prefs.js", "cookies.sqlite"]);
        fake_profile(&ff.join("def.dev-edition"), &["cache2"], &["places.sqlite"]);
        let found = firefox_caches(Some(ff));
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|p| p.file_name().unwrap() == "cache2"));
        // Opera: both channels and layouts; channel roots and other channels never returned.
        let op = dir.join("Opera Software");
        fake_profile(&op.join("Opera Stable"), &["Cache", "Code Cache"], &["Preferences", "Cookies"]);
        fake_profile(&op.join("Opera GX Stable"), &["Default\\Cache", "GPUCache"], &["Local State"]);
        fake_profile(&op.join("Opera Crypto Stable"), &["Cache"], &[]);
        let found = opera_caches(Some(op.clone()));
        assert_eq!(found.len(), 12);
        assert!(!found.contains(&op.join("Opera Stable")));
        assert!(!found.iter().any(|p| p.starts_with(op.join("Opera Crypto Stable"))));
        // Epic: webcache, webcache_4147 only.
        let ep = dir.join("Saved");
        fake_profile(&ep, &["webcache", "webcache_4147", "Config", "Logs"], &[]);
        let found = epic_caches(Some(ep));
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|p| p.file_name().unwrap().to_string_lossy().starts_with("webcache")));

        // Deleting through the resolved paths: caches go, profile data and Local State survive.
        let mut t = Tally::default();
        for p in chromium_caches(Some(ud.clone())) {
            delete_dir_contents(&p, &mut t);
        }
        assert_eq!(t.files, 9);
        for prof in ["Default", "Profile 2", "Guest Profile"] {
            for k in keep {
                assert!(ud.join(prof).join(k).exists(), "{prof}/{k}");
            }
        }
        assert!(ud.join("Local State").exists());
        assert!(ud.join("System Profile/Cache/Cache_Data/data_0").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn junction_profile_and_junction_inside_cache_are_not_followed() {
        let dir = tmp_tree("pjunction");
        let victim = dir.join("victim");
        fs::create_dir_all(victim.join("cache2")).unwrap();
        fs::write(victim.join("cache2/keep.bin"), b"x").unwrap();
        let ff = dir.join("Profiles");
        fake_profile(&ff.join("real"), &["cache2"], &[]);
        make_junction(&ff.join("linked"), &victim);
        assert_eq!(firefox_caches(Some(ff.clone())), vec![ff.join("real").join("cache2")], "junction profile skipped");
        // A junction inside a cache folder: contents deleted, target untouched.
        make_junction(&ff.join("real/cache2/trap"), &victim);
        let mut t = Tally::default();
        delete_dir_contents(&ff.join("real/cache2"), &mut t);
        assert_eq!(t.files, 1);
        assert!(victim.join("cache2/keep.bin").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Read-only: bytes per new category on this PC.
    /// `cargo test -p g2boost-core -- --ignored measure_new --nocapture`
    #[test]
    #[ignore]
    fn measure_new_categories() {
        let ids = [
            "firefox_cache", "brave_cache", "opera_cache", "vivaldi_cache", "steam_cache", "epic_cache", "battlenet_cache",
            "ea_cache", "ubisoft_cache", "teams_cache", "slack_cache", "vscode_cache", "dev_npm", "dev_pip", "dev_nuget", "dev_yarn",
        ];
        for i in preview(&ids.map(String::from)) {
            println!("{:<16} bytes={:<12} files={:<7} missing={}", i.id, i.bytes, i.files, i.missing);
        }
    }

    #[test]
    fn top_files_glob_and_in_use_skip() {
        let dir = sandbox("glob");
        let hits = top_files(&dir.join("cache"), &["thumbcache_*.db"]);
        assert_eq!(hits.len(), 1);
        assert!(glob_match("*.log", "CBS.LOG"));
        assert!(!glob_match("*.log", "CBS.etl"));
        assert!(glob_match("IconCache.db", "iconcache.db"));
        // A file held open with no sharing is skipped, not an error.
        let held = dir.join("cache/held.tmp");
        let f = {
            use std::os::windows::fs::OpenOptionsExt;
            fs::OpenOptions::new().write(true).create(true).truncate(true).share_mode(0).open(&held).unwrap()
        };
        let mut t = Tally::default();
        delete_dir_contents(&dir.join("cache"), &mut t);
        assert_eq!(t.skipped, 1);
        assert!(held.exists());
        drop(f);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn roots_outside_the_allowlist_are_refused() {
        assert!(matches!(checked_root(Path::new(r"C:\Windows\System32\spool\PRINTERS")), Err(SafePathError::SystemCritical)));
        assert!(matches!(checked_root(Path::new(r"C:\Users\Other\AppData\Local\Temp")), Err(SafePathError::OtherUser)));
        assert!(matches!(checked_root(Path::new(r"\\server\share")), Err(SafePathError::Unc)));
        assert!(matches!(checked_root(Path::new(r"C:\Users\Windows\AppData\Local\..\..\..\Windows")), Err(SafePathError::Relative)));
        // Every catalog path must classify (exists or not) without hitting a critical area.
        for c in CATALOG {
            for p in paths(c.id) {
                assert!(checked_root(&p).is_ok(), "{}: {}", c.id, p.display());
            }
        }
    }

    #[test]
    fn preview_runs_for_every_category_without_deleting() {
        let ids: Vec<String> = CATALOG.iter().map(|c| c.id.to_string()).collect();
        let items = preview(&ids);
        assert_eq!(items.len(), CATALOG.len());
        let dns = items.iter().find(|i| i.id == "dns_flush").unwrap();
        assert!(!dns.measurable);
        let temp = items.iter().find(|i| i.id == "temp_user").unwrap();
        assert!(!temp.missing);
        assert!(preview(&["nope".into()]).is_empty());
    }

    #[test]
    fn tokens_are_single_use_and_bound_to_the_selection() {
        let store = TokenStore::default();
        let ids = vec!["temp_user".to_string(), "thumbs".to_string()];
        let t = store.issue(ids.clone());
        assert!(!store.consume(t.token, &["temp_user".to_string()]), "different selection");
        assert!(!store.consume(t.token ^ 1, &ids), "wrong token");
        assert!(store.consume(t.token, &ids));
        assert!(!store.consume(t.token, &ids), "already used");
        for _ in 0..64 {
            let t = store.issue(vec!["x".into()]);
            assert!(t.token < (1u64 << 53), "exact as a JS number");
            assert_eq!(t.token as f64 as u64, t.token, "survives the f64 round trip");
        }
    }

    #[test]
    fn execute_refuses_admin_categories_without_elevation_and_records_history() {
        if system::is_elevated() {
            return;
        }
        // SAFETY: shared data-dir override for tests (same value everywhere).
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
        let rec = execute(&["temp_windows".into(), "nope".into()], "test", |_, _| {});
        assert_eq!(rec.items[0].error.as_deref(), Some("needs_admin"));
        assert_eq!(rec.items[1].error.as_deref(), Some("unknown_category"));
        assert!(history().iter().any(|r| r.trigger == "test"));
    }
}
