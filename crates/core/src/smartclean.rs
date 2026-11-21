//! Smart cleanup (round 11): one "Analyze" that asks every tool G2 Boost has where space can come
//! back from, and sorts the answer into three buckets.
//!
//! - **Safe**: catalog categories that are on by default and not high risk, no admin needed. One click.
//! - **System**: the same for admin categories plus Windows' own cleanup handlers (on by default).
//! - **Review**: everything that needs a human look: opt-in or risky catalog categories, big old
//!   files in the user's folders, folders nobody owns, leftovers of removed apps, duplicates. Each
//!   one leads to the flow that already exists for it (with its own confirmation); nothing here
//!   deletes outside the catalog.
//!
//! Read-only: measuring reuses `cleanup::preview_live`, `volcache::preview`, `apps::unowned`,
//! `apps::orphans`/`leftovers` and the disk filter syntax. Running reuses `cleanup::execute` and
//! `volcache::execute` behind the usual one-shot token (shell). Also here: which catalog category
//! owns a folder of the disk map ("cleanable by Cleanup" badge).

use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

use serde::Serialize;

use crate::cleanup::{self, CATALOG, Category, PreviewItem, Progress, Risk};
use crate::safety::{SafePath, is_reparse, starts_with_dir};
use crate::system::{self, DriveInfo};
use crate::{apps, disk, registry, restore, steam, volcache};

/// What counts as "big and old" in the user's folders (the disk page filter syntax).
pub const BIG_OLD_FILTER: &str = ">500mb >6m";
/// The quick walk of the user's folders stops here (it is a hint, not an inventory).
const BIG_OLD_BUDGET: Duration = Duration::from_secs(4);
const BIG_OLD_TOP: usize = 10;
/// Most selections the shell accepts (catalog + system handlers).
pub const MAX_IDS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Bucket {
    Safe,
    Review,
    System,
}

/// Where a catalog category lands. Pure: the whole sorting rule.
pub fn bucket(c: &Category) -> Bucket {
    match (c.default_on && c.risk != Risk::High, c.admin) {
        (true, false) => Bucket::Safe,
        (true, true) => Bucket::System,
        (false, _) => Bucket::Review,
    }
}

/* ---------------- analysis ---------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Catalog,
    System,
    BigOld,
    Unowned,
    Leftovers,
    Dupes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    Running,
    Done,
    /// Not available here (no admin, no disk scan, another job holds the servicing slot).
    Skipped,
}

/// One tick of the analysis, per source (the UI's pipeline).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SourceEvent {
    pub source: Source,
    pub state: SourceState,
    /// Found so far (running) or in total (done).
    pub bytes: u64,
    pub count: u64,
    /// Catalog category or system handler in flight.
    pub current: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReviewItem {
    /// Opt-in or risky catalog category: "clean only this" (preview + confirm).
    Category { id: &'static str, bytes: u64, needs_admin: bool },
    /// Big files untouched for months in the user's folders: the disk page, filtered.
    BigOld { count: u64, bytes: u64 },
    /// Folders no installed app owns: Apps › leftovers sweep.
    Unowned { count: u64, bytes: u64, high: u64 },
    /// What removed apps left behind (not already counted as unowned): Apps › leftovers.
    Leftovers { apps: u64, bytes: u64 },
    /// Copies found by the last duplicate search (disk page).
    Dupes { groups: u64, bytes: u64 },
}

impl ReviewItem {
    pub fn bytes(&self) -> u64 {
        match self {
            Self::Category { bytes, .. }
            | Self::BigOld { bytes, .. }
            | Self::Unowned { bytes, .. }
            | Self::Leftovers { bytes, .. }
            | Self::Dupes { bytes, .. } => *bytes,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OldFile {
    pub path: String,
    pub size: u64,
    pub mtime: u64,
}

/// Big items on the system drive that cleaning does not touch, with the safe way to shrink each
/// (explained by the UI): `hiberfil`, `pagefile`, `swapfile`, `games` (Steam library on the
/// system drive), `restore_points` (bytes unknown without admin: `count` points).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Extra {
    pub id: &'static str,
    pub bytes: u64,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub at: u64,
    pub elevated: bool,
    pub drive: Option<DriveInfo>,
    /// The system drive is short on space ([`tight`]).
    pub tight: bool,
    /// Catalog ids for the one-click clean.
    pub safe: Vec<&'static str>,
    pub safe_bytes: u64,
    /// Admin catalog ids and handler ids (cleaned with the safe ones when elevated).
    pub system: Vec<&'static str>,
    pub system_bytes: u64,
    pub review: Vec<ReviewItem>,
    pub review_bytes: u64,
    /// Every catalog category as measured (the UI shows sizes, apps in use, admin).
    pub catalog: Vec<PreviewItem>,
    /// Windows cleanup handlers that are on by default, as measured (`needs_admin` when not elevated).
    pub handlers: Vec<volcache::VcItem>,
    /// Largest big-old files (local UI only; cleared for the MCP).
    pub big_old: Vec<OldFile>,
    pub extras: Vec<Extra>,
    /// Unowned folders found, lowercased (the disk map's badge). Never leaves the app.
    #[serde(skip)]
    pub unowned_paths: Vec<String>,
}

impl Plan {
    /// The plan for the MCP: sizes and ids only, no file paths (profile names live there).
    pub fn for_mcp(mut self) -> Self {
        self.big_old.clear();
        self.unowned_paths.clear();
        self
    }
}

/// What the analysis can use that only the caller has.
#[derive(Default)]
pub struct Inputs {
    /// Installed apps: needed by the unowned sweep and the removed-apps leftovers (None = skip both).
    pub installed: Option<Vec<apps::App>>,
    /// (groups, wasted bytes) of the last duplicate search, if one ran.
    pub dupes: Option<(u64, u64)>,
}

fn ev(source: Source, state: SourceState, bytes: u64, count: u64, current: Option<&'static str>) -> SourceEvent {
    SourceEvent { source, state, bytes, count, current }
}

/// Measure every source, reporting as it goes (two threads: the catalog side and the apps side).
/// `cancel` stops the catalog and the walk; what was not reached is left out.
pub fn analyze(inp: &Inputs, cancel: &AtomicBool, report: impl FnMut(&SourceEvent) + Send) -> Plan {
    let report = Mutex::new(report);
    let send = |e: SourceEvent| {
        if let Ok(mut r) = report.lock() {
            r(&e);
        }
    };
    let elevated = system::is_elevated();
    let (catalog, handlers, (big_old, old_count, old_bytes), apps_side) = std::thread::scope(|sc| {
        let apps_side = sc.spawn(|| apps_side(inp, &send));
        let ids: Vec<String> = CATALOG.iter().map(|c| c.id.to_string()).collect();
        send(ev(Source::Catalog, SourceState::Running, 0, 0, None));
        let catalog = cleanup::preview_live(&ids, cancel, |p| send(ev(Source::Catalog, SourceState::Running, p.bytes, p.files, p.current)));
        let measured: u64 = catalog.iter().map(|i| i.bytes).sum();
        send(ev(Source::Catalog, SourceState::Done, measured, catalog.len() as u64, None));

        let hids: Vec<String> = volcache::HANDLERS.iter().filter(|h| h.default_on).map(|h| h.id.to_string()).collect();
        let handlers = if cancel.load(Ordering::Relaxed) {
            Vec::new()
        } else {
            send(ev(Source::System, SourceState::Running, 0, 0, None));
            // Busy (a repair holds the servicing slot): nothing measured, the source says skipped.
            volcache::preview(&hids, |p| send(ev(Source::System, SourceState::Running, p.bytes, p.done as u64, p.current))).unwrap_or_default()
        };
        let hb: u64 = handlers.iter().map(|h| h.bytes).sum();
        let st = if elevated && !handlers.is_empty() { SourceState::Done } else { SourceState::Skipped };
        send(ev(Source::System, st, hb, handlers.len() as u64, None));

        send(ev(Source::BigOld, SourceState::Running, 0, 0, None));
        let f = disk::parse_filter(BIG_OLD_FILTER);
        let old = big_old(&user_folders(), &f, registry::now_unix(), Instant::now() + BIG_OLD_BUDGET, cancel);
        send(ev(Source::BigOld, SourceState::Done, old.2, old.1, None));
        (catalog, handlers, old, apps_side.join().unwrap_or_default())
    });
    match inp.dupes {
        Some((groups, wasted)) => send(ev(Source::Dupes, SourceState::Done, wasted, groups, None)),
        None => send(ev(Source::Dupes, SourceState::Skipped, 0, 0, None)),
    }
    let mut plan = sort(elevated, catalog, handlers, inp.dupes, (old_count, old_bytes), &apps_side);
    plan.big_old = big_old;
    plan.unowned_paths = apps_side.unowned_paths;
    plan.extras = extras();
    plan.drive = system_drive();
    plan.tight = plan.drive.as_ref().is_some_and(tight);
    plan
}

#[derive(Default)]
struct AppsSide {
    unowned: Option<(u64, u64, u64)>,
    leftovers: Option<(u64, u64)>,
    unowned_paths: Vec<String>,
}

fn apps_side(inp: &Inputs, send: &(dyn Fn(SourceEvent) + Sync)) -> AppsSide {
    let Some(installed) = inp.installed.as_ref() else {
        send(ev(Source::Unowned, SourceState::Skipped, 0, 0, None));
        send(ev(Source::Leftovers, SourceState::Skipped, 0, 0, None));
        return AppsSide::default();
    };
    send(ev(Source::Unowned, SourceState::Running, 0, 0, None));
    let found = apps::unowned(installed);
    // The user's own data (saves, profiles) is never offered as space to free.
    let offer: Vec<&apps::Leftover> = found.iter().filter(|l| !l.precious).collect();
    let bytes = offer.iter().map(|l| l.bytes).sum();
    let high = offer.iter().filter(|l| l.confidence == apps::Confidence::High).count() as u64;
    send(ev(Source::Unowned, SourceState::Done, bytes, offer.len() as u64, None));
    let unowned_paths: Vec<String> = found.iter().map(|l| l.path.to_ascii_lowercase()).collect();

    send(ev(Source::Leftovers, SourceState::Running, 0, 0, None));
    let (mut n, mut lb) = (0u64, 0u64);
    for a in apps::orphans(installed) {
        // Already counted as unowned (same folder) = not counted twice.
        let fresh: u64 = apps::leftovers(&a)
            .iter()
            .filter(|l| !l.precious && !unowned_paths.contains(&l.path.to_ascii_lowercase()))
            .map(|l| l.bytes)
            .sum();
        if fresh > 0 {
            n += 1;
            lb += fresh;
        }
    }
    send(ev(Source::Leftovers, SourceState::Done, lb, n, None));
    AppsSide { unowned: Some((offer.len() as u64, bytes, high)), leftovers: Some((n, lb)), unowned_paths }
}

/// The buckets from what was measured. Pure (tested with fabricated measurements).
fn sort(
    elevated: bool,
    catalog: Vec<PreviewItem>,
    handlers: Vec<volcache::VcItem>,
    dupes: Option<(u64, u64)>,
    (old_count, old_bytes): (u64, u64),
    side: &AppsSide,
) -> Plan {
    let mut plan = Plan {
        at: registry::now_unix(),
        elevated,
        drive: None,
        tight: false,
        safe: Vec::new(),
        safe_bytes: 0,
        system: Vec::new(),
        system_bytes: 0,
        review: Vec::new(),
        review_bytes: 0,
        catalog: Vec::new(),
        handlers: Vec::new(),
        big_old: Vec::new(),
        extras: Vec::new(),
        unowned_paths: Vec::new(),
    };
    for item in &catalog {
        let Some(c) = cleanup::find(item.id) else { continue };
        // Nothing there (no folder, 0 bytes) is not offered; a flush with no size is.
        if item.missing || (item.measurable && item.bytes == 0) {
            continue;
        }
        match bucket(c) {
            Bucket::Safe => {
                plan.safe.push(c.id);
                plan.safe_bytes += item.bytes;
            }
            Bucket::System => {
                plan.system.push(c.id);
                plan.system_bytes += item.bytes;
            }
            Bucket::Review => plan.review.push(ReviewItem::Category { id: c.id, bytes: item.bytes, needs_admin: item.needs_admin }),
        }
    }
    for h in &handlers {
        // Unmeasured without admin: offered (the UI says "elevate to measure"); measured and empty: not.
        if h.needs_admin || (h.available && h.bytes > 0 && h.error.is_none()) {
            plan.system.push(h.id);
            plan.system_bytes += h.bytes;
        }
    }
    if old_count > 0 {
        plan.review.push(ReviewItem::BigOld { count: old_count, bytes: old_bytes });
    }
    if let Some((count, bytes, high)) = side.unowned.filter(|u| u.0 > 0) {
        plan.review.push(ReviewItem::Unowned { count, bytes, high });
    }
    if let Some((apps, bytes)) = side.leftovers.filter(|l| l.0 > 0) {
        plan.review.push(ReviewItem::Leftovers { apps, bytes });
    }
    if let Some((groups, bytes)) = dupes.filter(|d| d.0 > 0) {
        plan.review.push(ReviewItem::Dupes { groups, bytes });
    }
    plan.review.sort_by_key(|r| std::cmp::Reverse(r.bytes()));
    plan.review_bytes = plan.review.iter().map(ReviewItem::bytes).sum();
    plan.catalog = catalog;
    plan.handlers = handlers;
    plan
}

/* ---------------- running: two legs seen as one queue ---------------- */

/// Split a selection into catalog ids and system handler ids (order kept). `None` = refused:
/// empty, too long, unknown id or the same id twice. The caller is hostile.
pub fn split_ids(ids: &[String]) -> Option<(Vec<String>, Vec<String>)> {
    if ids.is_empty() || ids.len() > MAX_IDS || ids.iter().enumerate().any(|(k, i)| ids[..k].contains(i)) {
        return None;
    }
    let mut out = (Vec::new(), Vec::new());
    for id in ids {
        if cleanup::find(id).is_some() {
            out.0.push(id.clone());
        } else if volcache::find(id).is_some() {
            out.1.push(id.clone());
        } else {
            return None;
        }
    }
    Some(out)
}

/// A leg's progress inside the whole run: `base` is what the earlier leg ended with.
pub fn joined(base: &Progress, p: &Progress, total: usize) -> Progress {
    Progress { done: base.done + p.done, total, current: p.current, bytes: base.bytes + p.bytes, files: base.files + p.files }
}

/* ---------------- big old files in the user's folders ---------------- */

const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x4_0000;
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x40_0000;

/// Downloads, Desktop, Documents, Videos, Pictures, Music, when they live on the system drive
/// (a folder moved to another drive frees nothing there). Nested ones are walked once.
fn user_folders() -> Vec<PathBuf> {
    use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Videos};
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).to_ascii_lowercase();
    let all: Vec<(PathBuf, String)> = [FOLDERID_Downloads, FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Videos, FOLDERID_Pictures, FOLDERID_Music]
        .iter()
        .filter_map(registry::known_folder)
        .map(|p| {
            let l = lower(&p);
            (p, l)
        })
        .filter(|(_, l)| l.starts_with(&drive))
        .collect();
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for (p, l) in &all {
        let nested = all.iter().any(|(_, o)| o != l && starts_with_dir(l, o));
        if !nested && !seen.contains(&l.as_str()) {
            seen.push(l);
            out.push(p.clone());
        }
    }
    out
}

/// Files under `roots` matching `f` (disk filter), largest `BIG_OLD_TOP` listed, until `deadline`.
/// Never crosses reparse points; cloud placeholders (online-only files) take no disk and are skipped.
pub(crate) fn big_old(roots: &[PathBuf], f: &disk::Filter, now: u64, deadline: Instant, cancel: &AtomicBool) -> (Vec<OldFile>, u64, u64) {
    let (mut top, mut count, mut bytes) = (Vec::new(), 0u64, 0u64);
    let mut stack: Vec<PathBuf> = roots.to_vec();
    while let Some(dir) = stack.pop() {
        if Instant::now() >= deadline || cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(md) = e.metadata() else { continue };
            if is_reparse(&md) {
                continue;
            }
            if md.is_dir() {
                stack.push(e.path());
                continue;
            }
            let attrs = md.file_attributes();
            if attrs & (FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) != 0 {
                continue;
            }
            let mtime = md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            let name = e.file_name().to_string_lossy().into_owned();
            if f.matches(&name, md.len(), mtime, now) {
                count += 1;
                bytes += md.len();
                top.push(OldFile { path: e.path().to_string_lossy().into_owned(), size: md.len(), mtime });
            }
        }
    }
    top.sort_by_key(|o| std::cmp::Reverse(o.size));
    top.truncate(BIG_OLD_TOP);
    (top, count, bytes)
}

/* ---------------- the system drive and what cleaning leaves ---------------- */

pub fn system_drive() -> Option<DriveInfo> {
    let d = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    system::list_drives().into_iter().find(|x| x.letter.eq_ignore_ascii_case(&d))
}

fn extras() -> Vec<Extra> {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let mut out = Vec::new();
    // Sizes from the directory listing: these files are locked, but the listing has them.
    if let Ok(rd) = std::fs::read_dir(format!("{drive}\\")) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_ascii_lowercase();
            let id = match n.as_str() {
                "hiberfil.sys" => "hiberfil",
                "pagefile.sys" => "pagefile",
                "swapfile.sys" => "swapfile",
                _ => continue,
            };
            if let Ok(md) = e.metadata() {
                out.push(Extra { id, bytes: md.len(), count: 1 });
            }
        }
    }
    let games: Vec<steam::Game> = steam::installed().into_iter().filter(|g| g.library.to_ascii_lowercase().starts_with(&drive.to_ascii_lowercase())).collect();
    if !games.is_empty() {
        out.push(Extra { id: "games", bytes: games.iter().map(|g| g.size_bytes).sum(), count: games.len() as u64 });
    }
    let points = restore::status().count;
    if points > 0 {
        out.push(Extra { id: "restore_points", bytes: 0, count: u64::from(points) });
    }
    out.sort_by_key(|x| std::cmp::Reverse(x.bytes));
    out
}

/// Under 15 % or under 20 GB free: the plan's next step points at what else takes the space.
pub fn tight(d: &DriveInfo) -> bool {
    d.total_bytes > 0 && (d.free_bytes < 20 << 30 || d.free_bytes * 100 < d.total_bytes * 15)
}

/* ---------------- the app behind a category (its real icon) ---------------- */

/// Where each app's exe usually lives: `(category, base, relative)`. Base: `L` LocalAppData,
/// `R` AppData, `P` Program Files, `X` Program Files (x86).
const APP_EXES: &[(&str, char, &str)] = &[
    ("chrome_cache", 'P', r"Google\Chrome\Application\chrome.exe"),
    ("chrome_cache", 'X', r"Google\Chrome\Application\chrome.exe"),
    ("chrome_cache", 'L', r"Google\Chrome\Application\chrome.exe"),
    ("edge_cache", 'X', r"Microsoft\Edge\Application\msedge.exe"),
    ("edge_cache", 'P', r"Microsoft\Edge\Application\msedge.exe"),
    ("inetcache", 'X', r"Microsoft\Edge\Application\msedge.exe"),
    ("firefox_cache", 'P', r"Mozilla Firefox\firefox.exe"),
    ("firefox_cache", 'X', r"Mozilla Firefox\firefox.exe"),
    ("brave_cache", 'P', r"BraveSoftware\Brave-Browser\Application\brave.exe"),
    ("brave_cache", 'L', r"BraveSoftware\Brave-Browser\Application\brave.exe"),
    ("opera_cache", 'L', r"Programs\Opera GX\opera.exe"),
    ("opera_cache", 'L', r"Programs\Opera\opera.exe"),
    ("vivaldi_cache", 'L', r"Vivaldi\Application\vivaldi.exe"),
    ("discord_cache", 'L', r"Discord\Discord.exe"),
    ("spotify_cache", 'R', r"Spotify\Spotify.exe"),
    ("steam_cache", 'X', r"Steam\steam.exe"),
    ("epic_cache", 'X', r"Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe"),
    ("battlenet_cache", 'X', r"Battle.net\Battle.net.exe"),
    ("ea_cache", 'P', r"Electronic Arts\EA Desktop\EA Desktop\EADesktop.exe"),
    ("ubisoft_cache", 'X', r"Ubisoft\Ubisoft Game Launcher\UbisoftConnect.exe"),
    ("teams_cache", 'L', r"Microsoft\Teams\current\Teams.exe"),
    ("slack_cache", 'L', r"slack\slack.exe"),
    ("vscode_cache", 'L', r"Programs\Microsoft VS Code\Code.exe"),
    ("vscode_cache", 'P', r"Microsoft VS Code\Code.exe"),
];

/// The installed exe of each app category (first that exists), for its icon. Categories whose app
/// is not here are left out: the UI shows a plain tile.
pub fn app_exes() -> Vec<(&'static str, String)> {
    let base = |b: char| {
        cleanup::env_dir(match b {
            'L' => "LOCALAPPDATA",
            'R' => "APPDATA",
            'P' => "ProgramFiles",
            _ => "ProgramFiles(x86)",
        })
    };
    let mut out: Vec<(&'static str, String)> = Vec::new();
    for (id, b, rel) in APP_EXES {
        if out.iter().any(|(k, _)| k == id) {
            continue;
        }
        if let Some(p) = base(*b).map(|d| d.join(rel)).filter(|p| p.is_file()) {
            out.push((id, p.to_string_lossy().into_owned()));
        }
    }
    out
}

/* ---------------- which category owns a folder (disk map badge) ---------------- */

/// Roots of the folder-clearing catalog categories, as real long paths, lowercased. Roots that
/// do not exist or are reparse points are left out (cleanup skips them too).
pub fn catalog_roots() -> Vec<(&'static str, String)> {
    CATALOG
        .iter()
        .filter(|c| c.clears_folders())
        .flat_map(|c| cleanup::paths(c.id).into_iter().map(move |p| (c.id, p)))
        .filter_map(|(id, p)| {
            let md = std::fs::symlink_metadata(&p).ok()?;
            if !md.is_dir() || is_reparse(&md) {
                return None;
            }
            // %TEMP% may be an 8.3 short path; the disk map shows long names.
            let real = std::fs::canonicalize(&p).map(|r| disk::strip_verbatim(&r)).unwrap_or(p);
            Some((id, lower(&real)))
        })
        .collect()
}

fn lower(p: &Path) -> String {
    p.to_string_lossy().trim_end_matches('\\').to_ascii_lowercase()
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Owners {
    /// The folder is a category's root or lives inside one: cleaning that category empties it.
    pub inside: Vec<&'static str>,
    /// Category roots that live inside the folder.
    pub contains: Vec<&'static str>,
}

/// Which categories own `path` (lexical, case-insensitive). The path comes from the webview: it
/// must pass `SafePath` (absolute, local, no `..`, not System32/WinSxS, not another user's).
pub fn owners(path: &Path, roots: &[(&'static str, String)]) -> Owners {
    let win = cleanup::env_dir("SystemRoot").unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let profile = cleanup::env_dir("USERPROFILE").unwrap_or_else(|| PathBuf::from(r"C:\Users\Default"));
    owners_with(path, roots, &win, &profile)
}

fn owners_with(path: &Path, roots: &[(&'static str, String)], win: &Path, profile: &Path) -> Owners {
    let mut o = Owners::default();
    if SafePath::classify(path, win, profile).is_err() {
        return o;
    }
    let p = lower(path);
    for (id, r) in roots {
        let list = if starts_with_dir(&p, r) {
            &mut o.inside
        } else if starts_with_dir(r, &p) {
            &mut o.contains
        } else {
            continue;
        };
        if !list.contains(id) {
            list.push(id);
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::SystemTime;

    fn item(id: &'static str, bytes: u64) -> PreviewItem {
        PreviewItem { id, bytes, files: 1, measurable: true, missing: false, needs_admin: false, blocked_by: Vec::new() }
    }

    #[test]
    fn buckets_follow_risk_default_and_admin() {
        let b = |id: &str| bucket(cleanup::find(id).unwrap());
        assert_eq!(b("temp_user"), Bucket::Safe);
        assert_eq!(b("chrome_cache"), Bucket::Safe);
        assert_eq!(b("temp_windows"), Bucket::System);
        assert_eq!(b("wu_cache"), Bucket::System, "medium but on by default: admin bucket");
        assert_eq!(b("recycle"), Bucket::Review, "high risk never goes in one click");
        assert_eq!(b("shaders"), Bucket::Review, "opt-in");
        assert_eq!(b("dev_npm"), Bucket::Review);
        assert_eq!(b("dism"), Bucket::Review, "admin but opt-in");
        // Every high-risk or opt-in category stays out of the one-click buckets.
        for c in CATALOG {
            if c.risk == Risk::High || !c.default_on {
                assert_eq!(bucket(c), Bucket::Review, "{}", c.id);
            }
        }
    }

    #[test]
    fn sort_skips_empty_and_counts_each_byte_once() {
        let mut dns = item("dns_flush", 0);
        dns.measurable = false;
        let mut gone = item("edge_cache", 0);
        gone.missing = true;
        let catalog = vec![item("temp_user", 100), item("chrome_cache", 0), dns, gone, item("temp_windows", 50), item("recycle", 900), item("shaders", 30)];
        let handlers = vec![
            volcache::VcItem { id: "sys_setup_logs", bytes: 20, available: true, needs_admin: false, error: None },
            volcache::VcItem { id: "sys_chkdsk", bytes: 0, available: false, needs_admin: false, error: None },
            volcache::VcItem { id: "sys_defender", bytes: 5, available: true, needs_admin: false, error: Some("busy".into()) },
        ];
        let side = AppsSide { unowned: Some((2, 400, 1)), leftovers: Some((0, 0)), unowned_paths: Vec::new() };
        let p = sort(true, catalog, handlers, Some((3, 70)), (1, 600), &side);
        assert_eq!(p.safe, vec!["temp_user", "dns_flush"], "0-byte and missing left out, the flush stays");
        assert_eq!(p.safe_bytes, 100);
        assert_eq!(p.system, vec!["temp_windows", "sys_setup_logs"]);
        assert_eq!(p.system_bytes, 70);
        let kinds: Vec<u64> = p.review.iter().map(ReviewItem::bytes).collect();
        assert_eq!(kinds, vec![900, 600, 400, 70, 30], "largest first; leftovers with nothing are not offered");
        assert_eq!(p.review_bytes, 2000);
        assert!(matches!(p.review[0], ReviewItem::Category { id: "recycle", .. }));
        // Not elevated: handlers come back unmeasured and are offered behind Elevate.
        let unmeasured = vec![volcache::VcItem { id: "sys_setup_logs", bytes: 0, available: false, needs_admin: true, error: None }];
        let p = sort(false, Vec::new(), unmeasured, None, (0, 0), &AppsSide::default());
        assert_eq!(p.system, vec!["sys_setup_logs"]);
        assert!(p.review.is_empty());
    }

    #[test]
    fn selections_are_split_and_validated() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let (c, h) = split_ids(&s(&["temp_user", "sys_setup_logs", "thumbs"])).unwrap();
        assert_eq!((c, h), (s(&["temp_user", "thumbs"]), s(&["sys_setup_logs"])));
        assert!(split_ids(&[]).is_none());
        assert!(split_ids(&s(&["temp_user", "temp_user"])).is_none(), "twice");
        assert!(split_ids(&s(&["../../Windows"])).is_none());
        assert!(split_ids(&s(&["Update Cleanup"])).is_none(), "registry key names are not ids");
        assert!(split_ids(&vec!["temp_user".to_string(); MAX_IDS + 1]).is_none());
    }

    #[test]
    fn two_legs_read_as_one_queue() {
        let base = Progress { done: 3, total: 3, current: None, bytes: 1000, files: 10 };
        let p = Progress { done: 1, total: 2, current: Some("sys_chkdsk"), bytes: 5, files: 0 };
        assert_eq!(joined(&base, &p, 5), Progress { done: 4, total: 5, current: Some("sys_chkdsk"), bytes: 1005, files: 10 });
        assert_eq!(joined(&Progress::default(), &base, 5).total, 5);
    }

    #[test]
    fn owners_are_lexical_both_ways_and_refuse_hostile_paths() {
        let win = Path::new(r"C:\Windows");
        let prof = Path::new(r"C:\Users\me");
        let roots = vec![
            ("chrome_cache", r"c:\users\me\appdata\local\google\chrome\user data\default\cache\cache_data".to_string()),
            ("chrome_cache", r"c:\users\me\appdata\local\google\chrome\user data\default\gpucache".to_string()),
            ("temp_user", r"c:\users\me\appdata\local\temp".to_string()),
            ("temp_windows", r"c:\windows\temp".to_string()),
        ];
        let o = |p: &str| owners_with(Path::new(p), &roots, win, prof);
        assert_eq!(o(r"C:\Users\me\AppData\Local\Temp\").inside, vec!["temp_user"]);
        assert_eq!(o(r"C:\Users\me\AppData\Local\Temp\x\y.tmp").inside, vec!["temp_user"]);
        assert_eq!(o(r"C:\Users\me\AppData\Local\Google").contains, vec!["chrome_cache"], "deduplicated");
        assert_eq!(o(r"C:\Users\me\AppData\Local").contains, vec!["chrome_cache", "temp_user"]);
        assert_eq!(o(r"C:\Windows\Temp").inside, vec!["temp_windows"]);
        assert_eq!(o(r"C:\Users\me\AppData\Local\Templates"), Owners::default(), "a name prefix is not a folder");
        for hostile in [r"C:\Windows", r"C:\Windows\System32", r"\\server\share\temp", r"relative\temp", r"C:\Users\me\..\other", r"C:\Users\other\AppData"] {
            assert_eq!(o(hostile), Owners::default(), "{hostile}");
        }
    }

    #[test]
    fn real_catalog_roots_are_long_lowercase_and_owned() {
        let roots = catalog_roots();
        assert!(roots.iter().all(|(_, r)| r == &r.to_ascii_lowercase() && !r.starts_with(r"\\?\") && !r.ends_with('\\')));
        assert!(roots.iter().all(|(id, _)| cleanup::find(id).is_some_and(|c| c.clears_folders())));
        assert!(!roots.iter().any(|(id, _)| *id == "thumbs"), "top-file categories never badge a folder");
        if let Some(t) = std::env::var_os("TEMP") {
            assert!(owners(Path::new(&t), &roots).inside.contains(&"temp_user"));
        }
    }

    #[test]
    fn big_old_walk_skips_fresh_small_and_junctions() {
        let dir = std::env::temp_dir().join(format!("g2boost-smart-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("root/sub")).unwrap();
        fs::create_dir_all(dir.join("victim")).unwrap();
        let old = SystemTime::now() - Duration::from_secs(400 * 86_400);
        let mk = |rel: &str, n: usize, aged: bool| {
            let p = dir.join(rel);
            fs::write(&p, vec![0u8; n]).unwrap();
            if aged {
                fs::File::options().write(true).open(&p).unwrap().set_times(fs::FileTimes::new().set_modified(old)).unwrap();
            }
        };
        mk("root/big.iso", 4000, true);
        mk("root/sub/bigger.zip", 6000, true);
        mk("root/new.iso", 9000, false);
        mk("root/small.txt", 10, true);
        mk("victim/hidden.iso", 8000, true);
        crate::cleanup::tests::make_junction(&dir.join("root/link"), &dir.join("victim"));
        let f = disk::parse_filter(">1kb >6m");
        let now = registry::now_unix();
        let (top, count, bytes) = big_old(&[dir.join("root")], &f, now, Instant::now() + Duration::from_secs(5), &AtomicBool::new(false));
        assert_eq!((count, bytes), (2, 10_000), "fresh, small and behind-a-junction files are not counted");
        assert!(top[0].path.ends_with("bigger.zip") && top[1].path.ends_with("big.iso"), "largest first");
        // A deadline already passed or a cancel: nothing walked.
        let (_, c, _) = big_old(&[dir.join("root")], &f, now, Instant::now(), &AtomicBool::new(false));
        assert_eq!(c, 0);
        let (_, c, _) = big_old(&[dir.join("root")], &f, now, Instant::now() + Duration::from_secs(5), &AtomicBool::new(true));
        assert_eq!(c, 0);
        fs::remove_dir(dir.join("root/link")).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn app_exes_name_catalog_apps_that_exist() {
        for (id, b, rel) in APP_EXES {
            assert!(cleanup::find(id).is_some(), "{id}");
            assert!("LRPX".contains(*b) && rel.ends_with(".exe") && !rel.contains(".."), "{id}");
        }
        for (id, p) in app_exes() {
            assert!(Path::new(&p).is_file(), "{id}: {p}");
        }
    }

    #[test]
    fn tight_means_under_15_percent_or_20_gb() {
        let d = |total: u64, free: u64| DriveInfo { letter: "C:".into(), kind: system::DriveKind::Fixed, total_bytes: total << 30, free_bytes: free << 30 };
        assert!(tight(&d(240, 33)), "the owner's SATA SSD: 14 % free");
        assert!(tight(&d(2000, 19)), "big drive, under 20 GB");
        assert!(!tight(&d(240, 60)));
        assert!(!tight(&d(0, 0)), "unknown size is not tight");
    }

    /// Read-only analysis on this PC (no deletion): `cargo test -p g2boost-core -- --ignored analyze_live --nocapture`
    #[test]
    #[ignore]
    fn analyze_live() {
        let installed = apps::list(&[]);
        let t0 = Instant::now();
        let p = analyze(&Inputs { installed: Some(installed), dupes: None }, &AtomicBool::new(false), |e| {
            if e.state != SourceState::Running {
                eprintln!("{:?} {:?} bytes={} count={}", e.source, e.state, e.bytes, e.count);
            }
        });
        eprintln!("{} ms; safe {} system {} review {}", t0.elapsed().as_millis(), p.safe_bytes, p.system_bytes, p.review_bytes);
        eprintln!("{:?}", p.review);
        eprintln!("{:?}", p.extras);
    }
}
