//! Updates: installed apps through winget and Windows Update through the Windows Update Agent.
//!
//! - Apps: "update all" is a loop over `winget::upgrades()` calling the validated
//!   `winget::install(id, true)` once per package (never `winget upgrade --all`), after one
//!   confirmation token bound to the exact id list. G2 Boost itself, unknown versions and the
//!   user's exclusions are left out. Cancel stops between packages.
//! - Windows: the WUA COM API (`UpdateSession` → searcher/downloader/installer), never
//!   `usoclient`, `wuauclt` or PowerShell. Search works without admin; download/install need it
//!   (`needs_admin`). Jobs run asynchronously on a dedicated MTA thread and are polled, so cancel
//!   and timeouts are real (`RequestAbort`). Never restarts by itself.
//!
//! Everything the UI or the MCP sends is hostile: ids are validated here and must come from the
//! last list this process produced. History lives in `updates-history.json`, re-validated on read.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{VARIANT_FALSE, VARIANT_TRUE};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize};
use windows::Win32::System::UpdateAgent::{
    IDownloadCompletedCallback, IDownloadCompletedCallback_Impl, IDownloadCompletedCallbackArgs, IDownloadJob,
    IDownloadProgressChangedCallback, IDownloadProgressChangedCallback_Impl, IDownloadProgressChangedCallbackArgs,
    IInstallationCompletedCallback, IInstallationCompletedCallback_Impl, IInstallationCompletedCallbackArgs, IInstallationJob,
    IInstallationProgressChangedCallback, IInstallationProgressChangedCallback_Impl, IInstallationProgressChangedCallbackArgs,
    ISearchCompletedCallback, ISearchCompletedCallback_Impl, ISearchCompletedCallbackArgs, ISearchJob, IStringCollection,
    ISystemInformation, IUpdate, IUpdateCollection, IUpdateSession, OperationResultCode, SystemInformation, UpdateCollection,
    UpdateSession, irbAlwaysRequiresReboot, irbCanRequestReboot, orcAborted, orcSucceeded, orcSucceededWithErrors, uoUninstallation,
    utDriver,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, IUnknown, Ref, implement};

use crate::cleanup::{ConfirmToken, TokenStore};
use crate::winget;

#[derive(Debug, thiserror::Error)]
pub enum UpdatesError {
    #[error("needs_admin")]
    NeedsAdmin,
    #[error("bad_token")]
    BadToken,
    #[error("bad_selection")]
    BadSelection,
    #[error("invalid_id")]
    InvalidId,
    /// Another batch, search or install is running in this process.
    #[error("busy")]
    Busy,
    #[error("cancelled")]
    Cancelled,
    #[error("timeout")]
    Timeout,
    #[error("no_reboot_needed")]
    NoRebootNeeded,
    #[error(transparent)]
    Winget(#[from] winget::WingetError),
    /// A Windows Update Agent HRESULT.
    #[error("wu_0x{0:08X}")]
    Com(u32),
}

impl From<windows::core::Error> for UpdatesError {
    fn from(e: windows::core::Error) -> Self {
        match e.code().0 as u32 {
            0x8007_0005 => Self::NeedsAdmin, // E_ACCESSDENIED
            c => Self::Com(c),
        }
    }
}

/// One package or update in a run (also what the history keeps).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Item {
    pub id: String,
    pub name: String,
    /// `running`, `ok`, `failed`, `cancelled`.
    pub status: String,
    /// Short key for the UI (`ok`, `reboot`, `in_use`, `admin`, `download`, …).
    pub reason: String,
    /// winget exit code or Windows Update HRESULT.
    pub code: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Batch {
    pub at: u64,
    /// `apps` or `windows`.
    pub kind: String,
    pub items: Vec<Item>,
    /// A restart is needed to finish.
    pub reboot: bool,
    pub cancelled: bool,
}

/// Single-use, 2-minute tokens bound to the kind and the exact, ordered id list.
fn tokens() -> &'static TokenStore {
    static T: std::sync::OnceLock<TokenStore> = std::sync::OnceLock::new();
    T.get_or_init(TokenStore::default)
}

fn token_ids(kind: &str, ids: &[String]) -> Vec<String> {
    ids.iter().map(|i| format!("updates:{kind}:{i}")).collect()
}

/// Held while a batch/search/install runs; released on drop (also on panic).
struct Busy(&'static AtomicBool);

impl Busy {
    fn take(flag: &'static AtomicBool) -> Result<Self, UpdatesError> {
        if flag.swap(true, Ordering::SeqCst) { Err(UpdatesError::Busy) } else { Ok(Self(flag)) }
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

/* ---------------- apps (winget) ---------------- */

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AppUpdate {
    pub id: String,
    pub name: String,
    pub version: String,
    pub available: String,
    /// Why "update all" leaves it out: `self`, `unknown_version`, `excluded`.
    pub skip: Option<&'static str>,
    /// Installed for all users: its installer will likely ask for administrator rights.
    pub machine: bool,
}

static APPS_LAST: Mutex<Vec<AppUpdate>> = Mutex::new(Vec::new());
static APPS_BUSY: AtomicBool = AtomicBool::new(false);
static APPS_CANCEL: AtomicBool = AtomicBool::new(false);

const MAX_EXCLUDED: usize = 300;

/// The exclusion list from settings (hostile file): valid winget ids, deduplicated, bounded.
pub fn excluded_ids(raw: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    raw.iter()
        .map(|s| s.trim().to_string())
        .filter(|s| winget::valid_id(s) && seen.insert(s.to_ascii_lowercase()))
        .take(MAX_EXCLUDED)
        .collect()
}

/// Add (`on`) or remove `id` from the exclusion list; returns the list to save.
pub fn exclude(current: &[String], id: &str, on: bool) -> Result<Vec<String>, UpdatesError> {
    if !winget::valid_id(id) {
        return Err(UpdatesError::InvalidId);
    }
    let mut list: Vec<String> = excluded_ids(current).into_iter().filter(|x| !x.eq_ignore_ascii_case(id)).collect();
    if on {
        list.push(id.to_string());
        list.truncate(MAX_EXCLUDED);
    }
    for a in APPS_LAST.lock().unwrap_or_else(|p| p.into_inner()).iter_mut().filter(|a| a.id.eq_ignore_ascii_case(id)) {
        if a.skip.is_none() || a.skip == Some("excluded") {
            a.skip = on.then_some("excluded");
        }
    }
    Ok(list)
}

fn is_self(p: &winget::Package) -> bool {
    p.name.to_lowercase().contains("g2 boost") || p.id.to_ascii_lowercase().contains("g2boost")
}

/// `machine`: installed apps as (name, all users). winget cuts long names with "…".
pub fn classify(list: Vec<winget::Package>, excluded: &[String], machine: &[(String, bool)]) -> Vec<AppUpdate> {
    list.into_iter()
        .map(|p| {
            let available = p.available.clone().unwrap_or_default();
            let skip = if is_self(&p) {
                Some("self")
            } else if !p.version.chars().any(|c| c.is_ascii_digit()) || !available.chars().any(|c| c.is_ascii_digit()) {
                Some("unknown_version")
            } else if excluded.iter().any(|e| e.eq_ignore_ascii_case(&p.id)) {
                Some("excluded")
            } else {
                None
            };
            let cut = p.name.strip_suffix('…');
            let is_machine = machine
                .iter()
                .any(|(n, all)| *all && (n.eq_ignore_ascii_case(&p.name) || cut.is_some_and(|c| !c.is_empty() && n.starts_with(c))));
            AppUpdate { id: p.id, name: p.name, version: p.version, available, skip, machine: is_machine }
        })
        .collect()
}

/// Installed apps with a newer version in winget, classified; remembered for the confirmation.
pub fn app_updates(excluded: &[String]) -> Result<Vec<AppUpdate>, UpdatesError> {
    let list = winget::upgrades()?;
    let machine: Vec<(String, bool)> = crate::apps::list(&[]).into_iter().map(|a| (a.name, a.needs_admin)).collect();
    let out = classify(list, &excluded_ids(excluded), &machine);
    *APPS_LAST.lock().unwrap_or_else(|p| p.into_inner()) = out.clone();
    Ok(out)
}

/// Issued after the user confirmed the selection: every id from the last list, not skipped.
pub fn apps_confirm(ids: &[String]) -> Result<ConfirmToken, UpdatesError> {
    let last = APPS_LAST.lock().unwrap_or_else(|p| p.into_inner());
    let mut seen = std::collections::HashSet::new();
    let ok = !ids.is_empty()
        && ids.len() <= 200
        && ids.iter().all(|id| {
            seen.insert(id.to_ascii_lowercase()) && last.iter().any(|a| a.id == *id && a.skip.is_none())
        });
    if !ok {
        return Err(UpdatesError::BadSelection);
    }
    Ok(tokens().issue(token_ids("apps", ids)))
}

pub fn apps_cancel() {
    APPS_CANCEL.store(true, Ordering::SeqCst);
}

/// winget's exit code → a short reason. `0x8A15…` are winget's own codes; the rest are the
/// common installer codes winget passes through.
pub fn reason(code: u32) -> &'static str {
    match code {
        0 => "ok",
        0x8A15_0109 | 0x8A15_010B | 3010 | 1641 => "reboot",
        0x8A15_010A => "reboot_first",
        0x8A15_0101 | 0x8A15_0103 | 0x8A15_0111 => "in_use",
        0x8A15_0102 | 1618 => "busy",
        0x8A15_010C | 1602 | 1223 => "cancelled",
        0x8A15_0019 => "admin",
        0x8A15_002B | 0x8A15_0010 | 0x8A15_0014 | 0x8A15_010D => "not_applicable",
        0x8A15_0008 | 0x8A15_0107 => "download",
        0x8A15_0011 => "hash",
        0x8A15_0105 => "disk_full",
        0x8A15_010F | 0x8A15_001B | 0x8A15_001C => "blocked",
        0x8A15_0006 => "start_failed",
        _ => "failed",
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AppProgress {
    pub index: usize,
    pub total: usize,
    #[serde(flatten)]
    pub item: Item,
}

/// Upgrade the confirmed packages one by one, in order. `on` sees each start and result.
pub fn apps_run(token: u64, ids: &[String], mut on: impl FnMut(&AppProgress)) -> Result<Batch, UpdatesError> {
    if !tokens().consume(token, &token_ids("apps", ids)) {
        return Err(UpdatesError::BadToken);
    }
    let _busy = Busy::take(&APPS_BUSY)?;
    APPS_CANCEL.store(false, Ordering::SeqCst);
    let names: Vec<String> = {
        let last = APPS_LAST.lock().unwrap_or_else(|p| p.into_inner());
        ids.iter().map(|id| last.iter().find(|a| a.id == *id).map_or_else(|| id.clone(), |a| a.name.clone())).collect()
    };
    let total = ids.len();
    let mut items = Vec::with_capacity(total);
    for (index, (id, name)) in ids.iter().zip(names).enumerate() {
        let mut item = Item { id: id.clone(), name, status: "running".into(), reason: String::new(), code: 0 };
        if APPS_CANCEL.load(Ordering::SeqCst) {
            item.status = "cancelled".into();
            item.reason = "cancelled".into();
        } else {
            on(&AppProgress { index, total, item: item.clone() });
            match winget::install(id, true) {
                Ok(o) => {
                    let r = reason(o.code);
                    item.status = if o.ok || r == "reboot" { "ok" } else { "failed" }.into();
                    item.reason = r.into();
                    item.code = o.code;
                }
                Err(e) => {
                    item.status = "failed".into();
                    item.reason = match e {
                        winget::WingetError::Unavailable => "winget_missing",
                        winget::WingetError::Timeout => "timeout",
                        winget::WingetError::NotFound => "not_applicable",
                        _ => "failed",
                    }
                    .into();
                }
            }
        }
        on(&AppProgress { index, total, item: item.clone() });
        items.push(item);
    }
    let batch = Batch {
        at: crate::registry::now_unix(),
        kind: "apps".into(),
        reboot: items.iter().any(|i| i.reason == "reboot"),
        cancelled: items.iter().any(|i| i.status == "cancelled"),
        items,
    };
    record(&batch);
    Ok(batch)
}

/* ---------------- Windows Update (WUA COM) ---------------- */

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WuUpdate {
    /// `UpdateID` (GUID): the only handle the UI sends back.
    pub id: String,
    pub title: String,
    /// `KB5066835`…
    pub kb: Vec<String>,
    /// Largest download size in bytes (0 = unknown).
    pub size: u64,
    /// MSRC severity (`Critical`, `Important`, `Moderate`, `Low`) when Microsoft rates it.
    pub severity: Option<String>,
    pub categories: Vec<String>,
    pub driver: bool,
    /// `never`, `always`, `can` (may ask for a restart).
    pub reboot: &'static str,
    pub downloaded: bool,
    pub mandatory: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WuScan {
    pub updates: Vec<WuUpdate>,
    pub reboot_required: bool,
    /// false = Windows' last check (local cache), true = asked Microsoft now.
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WuHistoryEntry {
    pub title: String,
    pub kb: Option<String>,
    /// Unix seconds (UTC).
    pub at: u64,
    /// `install` or `uninstall`.
    pub operation: &'static str,
    /// `ok`, `partial`, `failed`, `cancelled`, `in_progress`.
    pub result: &'static str,
    pub hresult: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct WuStatus {
    pub history: Vec<WuHistoryEntry>,
    pub reboot_required: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WuProgress {
    /// `download` or `install`.
    pub phase: &'static str,
    pub percent: i32,
    /// Index of the update being worked on (in the selection).
    pub current: i32,
    pub total: usize,
}

static WU_LAST: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static WU_BUSY: AtomicBool = AtomicBool::new(false);
static WU_CANCEL: AtomicBool = AtomicBool::new(false);
/// Set when an install here reported that a restart is needed.
static WU_REBOOT: AtomicBool = AtomicBool::new(false);

const CRITERIA: &str = "IsInstalled=0 and IsHidden=0";
const SEARCH_ONLINE: Duration = Duration::from_secs(5 * 60);
const SEARCH_CACHED: Duration = Duration::from_secs(90);
const JOB_LIMIT: Duration = Duration::from_secs(3 * 60 * 60);

/// `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`, hex only: it goes into a search criterion.
pub fn valid_guid(s: &str) -> bool {
    s.len() == 36
        && s.char_indices().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == '-' } else { c.is_ascii_hexdigit() })
}

/// First `KB` followed by six or more digits.
pub fn kb_in(title: &str) -> Option<String> {
    title.match_indices("KB").find_map(|(i, _)| {
        let d: String = title[i + 2..].chars().take_while(char::is_ascii_digit).collect();
        (d.len() >= 6).then(|| format!("KB{d}"))
    })
}

/// OLE automation date (days since 1899-12-30) → Unix seconds.
pub fn ole_to_unix(d: f64) -> u64 {
    ((d - 25569.0) * 86400.0).max(0.0) as u64
}

pub fn wu_cancel() {
    WU_CANCEL.store(true, Ordering::SeqCst);
}

/// Run `f` on a fresh thread in the multithreaded apartment: WUA calls back on its own threads,
/// and polling a job from an STA without a message loop never sees it complete.
fn mta<T: Send + 'static>(f: impl FnOnce() -> Result<T, UpdatesError> + Send + 'static) -> Result<T, UpdatesError> {
    std::thread::spawn(move || {
        // SAFETY: this new thread's own apartment, balanced below after every COM object is dropped.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr.is_err() {
            return Err(UpdatesError::Com(hr.0 as u32));
        }
        let r = f();
        // SAFETY: balanced with the CoInitializeEx above.
        unsafe { CoUninitialize() };
        r
    })
    .join()
    .unwrap_or(Err(UpdatesError::Com(0x8000_FFFF)))
}

/// Completion/progress sink handed to `Begin*`: we poll the job instead.
#[implement(
    ISearchCompletedCallback,
    IDownloadCompletedCallback,
    IDownloadProgressChangedCallback,
    IInstallationCompletedCallback,
    IInstallationProgressChangedCallback
)]
struct Noop;

impl ISearchCompletedCallback_Impl for Noop_Impl {
    fn Invoke(&self, _: Ref<ISearchJob>, _: Ref<ISearchCompletedCallbackArgs>) -> windows::core::Result<()> {
        Ok(())
    }
}
impl IDownloadCompletedCallback_Impl for Noop_Impl {
    fn Invoke(&self, _: Ref<IDownloadJob>, _: Ref<IDownloadCompletedCallbackArgs>) -> windows::core::Result<()> {
        Ok(())
    }
}
impl IDownloadProgressChangedCallback_Impl for Noop_Impl {
    fn Invoke(&self, _: Ref<IDownloadJob>, _: Ref<IDownloadProgressChangedCallbackArgs>) -> windows::core::Result<()> {
        Ok(())
    }
}
impl IInstallationCompletedCallback_Impl for Noop_Impl {
    fn Invoke(&self, _: Ref<IInstallationJob>, _: Ref<IInstallationCompletedCallbackArgs>) -> windows::core::Result<()> {
        Ok(())
    }
}
impl IInstallationProgressChangedCallback_Impl for Noop_Impl {
    fn Invoke(&self, _: Ref<IInstallationJob>, _: Ref<IInstallationProgressChangedCallbackArgs>) -> windows::core::Result<()> {
        Ok(())
    }
}

#[derive(PartialEq)]
enum Stop {
    Done,
    Cancelled,
    Timeout,
}

/// Poll a job until it completes. A cancel or the deadline asks it to abort and keeps waiting
/// (up to 2 more minutes) so its per-update results can still be read.
fn wait(done: &dyn Fn() -> bool, abort: &dyn Fn(), limit: Duration, tick: &mut dyn FnMut()) -> Stop {
    let deadline = Instant::now() + limit;
    let mut stop = Stop::Done;
    let mut grace = None;
    while !done() {
        if stop == Stop::Done {
            if WU_CANCEL.load(Ordering::SeqCst) {
                stop = Stop::Cancelled;
            } else if Instant::now() > deadline {
                stop = Stop::Timeout;
            }
            if stop != Stop::Done {
                abort();
                grace = Some(Instant::now() + Duration::from_secs(120));
            }
        }
        if grace.is_some_and(|g| Instant::now() > g) {
            break;
        }
        tick();
        std::thread::sleep(Duration::from_millis(300));
    }
    stop
}

fn session() -> Result<IUpdateSession, UpdatesError> {
    // SAFETY: in-process WUA object on this MTA thread.
    unsafe {
        let s: IUpdateSession = CoCreateInstance(&UpdateSession, None, CLSCTX_INPROC_SERVER)?;
        s.SetClientApplicationID(&BSTR::from("G2 Boost"))?;
        Ok(s)
    }
}

fn succeeded(c: OperationResultCode) -> bool {
    c == orcSucceeded || c == orcSucceededWithErrors
}

fn search(s: &IUpdateSession, criteria: &str, online: bool) -> Result<IUpdateCollection, UpdatesError> {
    // SAFETY: WUA calls on objects created on this thread; the job is polled, then ended.
    unsafe {
        let searcher = s.CreateUpdateSearcher()?;
        searcher.SetOnline(if online { VARIANT_TRUE } else { VARIANT_FALSE })?;
        let cb: IUnknown = Noop.into();
        let job = searcher.BeginSearch(&BSTR::from(criteria), &cb, &VARIANT::default())?;
        let limit = if online { SEARCH_ONLINE } else { SEARCH_CACHED };
        let stop = wait(&|| job.IsCompleted().map_or(true, |b| b.as_bool()), &|| drop(job.RequestAbort()), limit, &mut || {});
        let r = searcher.EndSearch(&job);
        let _ = job.CleanUp();
        match stop {
            Stop::Cancelled => return Err(UpdatesError::Cancelled),
            Stop::Timeout => return Err(UpdatesError::Timeout),
            Stop::Done => {}
        }
        let r = r?;
        let code = r.ResultCode()?;
        if code == orcAborted {
            return Err(UpdatesError::Cancelled);
        }
        if !succeeded(code) {
            return Err(UpdatesError::Com(0x8024_0FFF)); // WU_E_UNEXPECTED: no HRESULT on a search result
        }
        Ok(r.Updates()?)
    }
}

fn strings(c: &IStringCollection) -> Vec<String> {
    // SAFETY: bounded index over the collection's own count.
    unsafe { (0..c.Count().unwrap_or(0).min(50)).filter_map(|i| c.get_Item(i).ok()).map(|b| clean(&b.to_string(), 64)).collect() }
}

fn read_update(u: &IUpdate) -> windows::core::Result<WuUpdate> {
    // SAFETY: property reads on a WUA object.
    unsafe {
        let cats = u.Categories()?;
        let categories =
            (0..cats.Count()?.min(20)).filter_map(|i| cats.get_Item(i).ok()).filter_map(|c| c.Name().ok()).map(|n| clean(&n.to_string(), 80)).collect();
        let size = u.MaxDownloadSize().map(|d| d.Anonymous2.Lo64).unwrap_or(0);
        let reboot = match u.InstallationBehavior().and_then(|b| b.RebootBehavior()) {
            Ok(b) if b == irbAlwaysRequiresReboot => "always",
            Ok(b) if b == irbCanRequestReboot => "can",
            _ => "never",
        };
        let severity = clean(&u.MsrcSeverity().unwrap_or_default().to_string(), 32);
        Ok(WuUpdate {
            id: u.Identity()?.UpdateID()?.to_string(),
            title: clean(&u.Title()?.to_string(), 300),
            kb: strings(&u.KBArticleIDs()?).into_iter().filter(|k| k.chars().all(|c| c.is_ascii_digit())).map(|k| format!("KB{k}")).collect(),
            size,
            severity: (!severity.is_empty()).then_some(severity),
            categories,
            driver: u.Type()? == utDriver,
            reboot,
            downloaded: u.IsDownloaded()?.as_bool(),
            mandatory: u.IsMandatory()?.as_bool(),
        })
    }
}

fn collection(c: &IUpdateCollection) -> Vec<(IUpdate, WuUpdate)> {
    // SAFETY: bounded index over the collection's own count.
    unsafe {
        (0..c.Count().unwrap_or(0).min(300))
            .filter_map(|i| c.get_Item(i).ok())
            .filter_map(|u| read_update(&u).ok().filter(|w| valid_guid(&w.id)).map(|w| (u, w)))
            .collect()
    }
}

fn system_reboot_required() -> bool {
    // SAFETY: in-process WUA object, one property read.
    unsafe {
        CoCreateInstance::<_, ISystemInformation>(&SystemInformation, None, CLSCTX_INPROC_SERVER)
            .and_then(|s| s.RebootRequired())
            .is_ok_and(|b| b.as_bool())
    }
}

/// Pending updates. `online` asks Microsoft now (can take minutes); otherwise Windows' last
/// check from the local cache (seconds). No admin needed.
pub fn wu_search(online: bool) -> Result<WuScan, UpdatesError> {
    let _busy = Busy::take(&WU_BUSY)?;
    WU_CANCEL.store(false, Ordering::SeqCst);
    let scan = mta(move || {
        let s = session()?;
        let found = search(&s, CRITERIA, online)?;
        let updates: Vec<WuUpdate> = collection(&found).into_iter().map(|(_, w)| w).collect();
        Ok(WuScan { updates, reboot_required: system_reboot_required(), online })
    })?;
    *WU_LAST.lock().unwrap_or_else(|p| p.into_inner()) = scan.updates.iter().map(|u| (u.id.clone(), u.title.clone())).collect();
    Ok(scan)
}

/// Issued after the user confirmed: every id from the last search, needs the app elevated.
pub fn wu_confirm(ids: &[String]) -> Result<ConfirmToken, UpdatesError> {
    if !crate::system::is_elevated() {
        return Err(UpdatesError::NeedsAdmin);
    }
    let last = WU_LAST.lock().unwrap_or_else(|p| p.into_inner());
    let mut seen = std::collections::HashSet::new();
    let ok = !ids.is_empty()
        && ids.len() <= 100
        && ids.iter().all(|id| valid_guid(id) && seen.insert(id.to_ascii_lowercase()) && last.iter().any(|(l, _)| l == id));
    if !ok {
        return Err(UpdatesError::BadSelection);
    }
    Ok(tokens().issue(token_ids("wu", ids)))
}

fn wu_item(id: &str, title: &str, status: &str, reason: &str, code: u32) -> Item {
    Item { id: id.into(), name: title.into(), status: status.into(), reason: reason.into(), code }
}

/// Download and install the confirmed updates (admin). Never restarts: `reboot` tells the UI.
pub fn wu_install(token: u64, ids: &[String], on: impl FnMut(&WuProgress) + Send + 'static) -> Result<Batch, UpdatesError> {
    if !crate::system::is_elevated() {
        return Err(UpdatesError::NeedsAdmin);
    }
    if !tokens().consume(token, &token_ids("wu", ids)) {
        return Err(UpdatesError::BadToken);
    }
    let _busy = Busy::take(&WU_BUSY)?;
    WU_CANCEL.store(false, Ordering::SeqCst);
    let ids = ids.to_vec();
    let batch = mta(move || install_on_mta(&ids, on))?;
    if batch.reboot {
        WU_REBOOT.store(true, Ordering::SeqCst);
    }
    record(&batch);
    Ok(batch)
}

fn install_on_mta(ids: &[String], mut on: impl FnMut(&WuProgress)) -> Result<Batch, UpdatesError> {
    let s = session()?;
    // The ids are GUIDs (checked), so the criterion cannot be bent. The local cache answers in
    // seconds after the search the user just ran; ask Microsoft only if it misses some.
    let criteria = ids.iter().map(|id| format!("UpdateID='{id}'")).collect::<Vec<_>>().join(" or ");
    let mut found = collection(&search(&s, &criteria, false)?);
    if found.len() < ids.len() {
        found = collection(&search(&s, &criteria, true)?);
    }
    let picked: Vec<(IUpdate, WuUpdate)> =
        ids.iter().filter_map(|id| found.iter().position(|(_, w)| w.id == *id).map(|i| found.swap_remove(i))).collect();
    let mut items: Vec<Item> = ids
        .iter()
        .filter(|id| !picked.iter().any(|(_, w)| w.id == **id))
        .map(|id| wu_item(id, id, "failed", "not_found", 0))
        .collect();
    let total = picked.len();
    if total == 0 {
        return Ok(Batch { at: crate::registry::now_unix(), kind: "windows".into(), items, reboot: false, cancelled: false });
    }
    // SAFETY: WUA calls on objects created on this thread; jobs are polled, then ended.
    unsafe {
        let all: IUpdateCollection = CoCreateInstance(&UpdateCollection, None, CLSCTX_INPROC_SERVER)?;
        for (u, _) in &picked {
            // The user confirmed this exact update: its license terms are accepted with it, as
            // Settings > Windows Update does.
            if !u.EulaAccepted()?.as_bool() {
                u.AcceptEula()?;
            }
            all.Add(u)?;
        }
        let cb: IUnknown = Noop.into();

        let downloader = s.CreateUpdateDownloader()?;
        downloader.SetUpdates(&all)?;
        let job = downloader.BeginDownload(&cb, &cb, &VARIANT::default())?;
        let stop = wait(
            &|| job.IsCompleted().map_or(true, |b| b.as_bool()),
            &|| drop(job.RequestAbort()),
            JOB_LIMIT,
            &mut || {
                if let Ok(p) = job.GetProgress() {
                    on(&WuProgress { phase: "download", percent: p.PercentComplete().unwrap_or(0), current: p.CurrentUpdateIndex().unwrap_or(0), total });
                }
            },
        );
        let _ = downloader.EndDownload(&job);
        let _ = job.CleanUp();

        let ready: IUpdateCollection = CoCreateInstance(&UpdateCollection, None, CLSCTX_INPROC_SERVER)?;
        let mut order = Vec::new();
        for (u, w) in &picked {
            if u.IsDownloaded().is_ok_and(|b| b.as_bool()) && stop == Stop::Done {
                ready.Add(u)?;
                order.push(w);
            } else {
                let (status, reason) = if stop == Stop::Done { ("failed", "download") } else { ("cancelled", "cancelled") };
                items.push(wu_item(&w.id, &w.title, status, reason, 0));
            }
        }
        let mut reboot = false;
        if !order.is_empty() {
            let installer = s.CreateUpdateInstaller()?;
            installer.SetUpdates(&ready)?;
            let job = installer.BeginInstall(&cb, &cb, &VARIANT::default())?;
            let n = order.len();
            wait(
                &|| job.IsCompleted().map_or(true, |b| b.as_bool()),
                &|| drop(job.RequestAbort()),
                JOB_LIMIT,
                &mut || {
                    if let Ok(p) = job.GetProgress() {
                        on(&WuProgress { phase: "install", percent: p.PercentComplete().unwrap_or(0), current: p.CurrentUpdateIndex().unwrap_or(0), total: n });
                    }
                },
            );
            let result = installer.EndInstall(&job);
            let _ = job.CleanUp();
            let result = result?;
            reboot = result.RebootRequired().is_ok_and(|b| b.as_bool());
            for (i, w) in order.iter().enumerate() {
                let r = result.GetUpdateResult(i as i32)?;
                let code = r.ResultCode()?;
                let hr = r.HResult().unwrap_or(0) as u32;
                let needs = r.RebootRequired().is_ok_and(|b| b.as_bool());
                let (status, reason) = if code == orcSucceeded {
                    ("ok", if needs { "reboot" } else { "ok" })
                } else if code == orcSucceededWithErrors {
                    ("ok", "partial")
                } else if code == orcAborted {
                    ("cancelled", "cancelled")
                } else {
                    ("failed", "failed")
                };
                reboot |= needs;
                items.push(wu_item(&w.id, &w.title, status, reason, hr));
            }
        }
        let reboot = reboot || system_reboot_required();
        Ok(Batch {
            at: crate::registry::now_unix(),
            kind: "windows".into(),
            cancelled: items.iter().any(|i| i.status == "cancelled"),
            items,
            reboot,
        })
    }
}

/// Windows' own update history (newest first, up to `n`), the "Get-HotFix" view, and whether a
/// restart is pending.
pub fn wu_status(n: usize) -> Result<WuStatus, UpdatesError> {
    mta(move || {
        let s = session()?;
        // SAFETY: WUA reads on objects created on this thread; index bounded by the total.
        let mut history: Vec<WuHistoryEntry> = unsafe {
            let searcher = s.CreateUpdateSearcher()?;
            let total = searcher.GetTotalHistoryCount()?.max(0);
            // Store apps log one entry per package architecture: read more, then fold repeats.
            let list = searcher.QueryHistory(0, total.min((n * 4).min(400) as i32))?;
            (0..list.Count()?)
                .filter_map(|i| list.get_Item(i).ok())
                .filter_map(|e| {
                    let title = clean(&e.Title().ok()?.to_string(), 300);
                    if title.is_empty() {
                        return None;
                    }
                    let code = e.ResultCode().ok()?;
                    Some(WuHistoryEntry {
                        kb: kb_in(&title),
                        title,
                        at: ole_to_unix(e.Date().unwrap_or(0.0)),
                        operation: if e.Operation().ok() == Some(uoUninstallation) { "uninstall" } else { "install" },
                        result: match code.0 {
                            1 => "in_progress",
                            2 => "ok",
                            3 => "partial",
                            5 => "cancelled",
                            _ => "failed",
                        },
                        hresult: e.HResult().unwrap_or(0) as u32,
                    })
                })
                .collect()
        };
        history.dedup_by(|a, b| a.title == b.title && a.at == b.at && a.result == b.result);
        history.truncate(n);
        Ok(WuStatus { history, reboot_required: system_reboot_required() || WU_REBOOT.load(Ordering::SeqCst) })
    })
}

/// Restart now, only when Windows (or an install here) says one is pending. Apps still get to
/// save; nothing is forced.
pub fn restart() -> Result<(), UpdatesError> {
    use windows::Win32::System::Shutdown::{InitiateShutdownW, SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_OPERATINGSYSTEM, SHTDN_REASON_MINOR_SERVICEPACK, SHUTDOWN_RESTART};
    let pending = WU_REBOOT.load(Ordering::SeqCst) || mta(|| Ok(system_reboot_required()))?;
    if !pending {
        return Err(UpdatesError::NoRebootNeeded);
    }
    crate::boost::enable_privilege("SeShutdownPrivilege").map_err(|_| UpdatesError::NeedsAdmin)?;
    // SAFETY: local restart, no message, no grace period, applications are asked (not forced).
    let rc = unsafe {
        InitiateShutdownW(
            windows::core::PCWSTR::null(),
            windows::core::PCWSTR::null(),
            0,
            SHUTDOWN_RESTART,
            SHTDN_REASON_MAJOR_OPERATINGSYSTEM | SHTDN_REASON_MINOR_SERVICEPACK | SHTDN_REASON_FLAG_PLANNED,
        )
    };
    if rc == 0 { Ok(()) } else { Err(UpdatesError::Com(rc)) }
}

/* ---------------- history ---------------- */

const HISTORY_MAX: usize = 50;

fn history_file() -> std::path::PathBuf {
    crate::registry::data_dir().join("updates-history.json")
}

/// Runs of both kinds, oldest first. The file is hostile: bounded, re-validated field by field.
pub fn history() -> Vec<Batch> {
    let f = history_file();
    if std::fs::metadata(&f).map_or(true, |m| m.len() > 2 << 20) {
        return Vec::new();
    }
    let all: Vec<Batch> = std::fs::read_to_string(&f).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let skip = all.len().saturating_sub(HISTORY_MAX);
    all.into_iter()
        .skip(skip)
        .filter(|b| matches!(b.kind.as_str(), "apps" | "windows"))
        .map(|b| Batch {
            items: b
                .items
                .into_iter()
                .take(300)
                .map(|i| Item {
                    id: clean(&i.id, 128),
                    name: clean(&i.name, 300),
                    status: if matches!(i.status.as_str(), "ok" | "failed" | "cancelled") { i.status } else { "failed".into() },
                    reason: if i.reason.len() <= 32 && i.reason.chars().all(|c| c.is_ascii_lowercase() || c == '_') { i.reason } else { "failed".into() },
                    code: i.code,
                })
                .collect(),
            ..b
        })
        .collect()
}

fn record(b: &Batch) {
    let mut all = history();
    all.push(b.clone());
    let n = all.len();
    if n > HISTORY_MAX {
        all.drain(..n - HISTORY_MAX);
    }
    let f = history_file();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, serde_json::to_string_pretty(&all).unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(id: &str, name: &str, v: &str, a: &str) -> winget::Package {
        winget::Package { id: id.into(), name: name.into(), version: v.into(), available: Some(a.into()) }
    }

    #[test]
    fn classify_skips_self_unknown_and_excluded_and_spots_machine_installs() {
        let list = vec![
            pkg("Git.Git", "Git", "2.40.0", "2.41.0"),
            pkg("Grovic.G2Boost", "G2 Boost", "0.1.0", "0.2.0"),
            pkg("Some.App", "Some App", "Unknown", "1.0"),
            pkg("Valve.Steam", "Steam", "2.10", "2.11"),
            pkg("Long.Name", "Microsoft Visual C++ 2015-2022 Redistrib…", "14.1", "14.4"),
        ];
        let machine = vec![("Git".to_string(), true), ("Steam".to_string(), false), ("Microsoft Visual C++ 2015-2022 Redistributable (x64)".to_string(), true)];
        let out = classify(list, &["valve.steam".into()], &machine);
        assert_eq!(out[0].skip, None);
        assert!(out[0].machine, "HKLM install");
        assert_eq!(out[1].skip, Some("self"));
        assert_eq!(out[2].skip, Some("unknown_version"));
        assert_eq!(out[3].skip, Some("excluded"), "case-insensitive");
        assert!(!out[3].machine);
        assert!(out[4].machine, "name cut by winget still matches");
    }

    #[test]
    fn exclusion_list_is_validated() {
        let raw = vec![" Git.Git ".into(), "git.git".into(), "a;b".into(), "--force".into(), "Valve.Steam".into()];
        assert_eq!(excluded_ids(&raw), vec!["Git.Git".to_string(), "Valve.Steam".to_string()]);
        assert_eq!(exclude(&raw, "Valve.Steam", false).unwrap(), vec!["Git.Git".to_string()]);
        assert_eq!(exclude(&[], "Valve.Steam", true).unwrap(), vec!["Valve.Steam".to_string()]);
        assert!(matches!(exclude(&[], "x y", true), Err(UpdatesError::InvalidId)));
    }

    #[test]
    fn exit_codes_map_to_reasons() {
        assert_eq!(reason(0), "ok");
        assert_eq!(reason(0x8A15_0109), "reboot");
        assert_eq!(reason(0x8A15_0101), "in_use");
        assert_eq!(reason(0x8A15_0019), "admin");
        assert_eq!(reason(1602), "cancelled");
        assert_eq!(reason(0x8A15_002B), "not_applicable");
        assert_eq!(reason(0xDEAD_BEEF), "failed");
    }

    #[test]
    fn guids_kbs_and_dates() {
        assert!(valid_guid("2bd4bf48-5e2b-4b7a-9e1c-0b8e0c1f1a2b"));
        for bad in ["", "2bd4bf48-5e2b-4b7a-9e1c-0b8e0c1f1a2", "2bd4bf48-5e2b-4b7a-9e1c-0b8e0c1f1a2g", "x' or IsInstalled=1 or UpdateID='aaaaa", "2bd4bf485e2b-4b7a-9e1c-0b8e0c1f1a2b-"] {
            assert!(!valid_guid(bad), "{bad}");
        }
        assert_eq!(kb_in("Atualização Cumulativa (KB5066835) (26200.6899)").as_deref(), Some("KB5066835"));
        assert_eq!(kb_in("KBx then KB890830"), Some("KB890830".into()));
        assert_eq!(kb_in("Driver Intel 1.2"), None);
        assert_eq!(ole_to_unix(25569.0), 0);
        assert_eq!(ole_to_unix(25570.5), 86_400 + 43_200);
    }

    #[test]
    fn confirmation_is_bound_to_the_last_list_and_single_use() {
        *APPS_LAST.lock().unwrap() = classify(
            vec![pkg("Git.Git", "Git", "2.40.0", "2.41.0"), pkg("Grovic.G2Boost", "G2 Boost", "0.1", "0.2"), pkg("GitHub.cli", "GitHub CLI", "2.1", "2.2")],
            &[],
            &[],
        );
        let ids = vec!["Git.Git".to_string(), "GitHub.cli".to_string()];
        for bad in [vec![], vec!["Grovic.G2Boost".to_string()], vec!["Not.Listed".to_string()], vec!["Git.Git".to_string(), "Git.Git".to_string()]] {
            assert!(matches!(apps_confirm(&bad), Err(UpdatesError::BadSelection)), "{bad:?}");
        }
        // Excluded after the list was made: refused too; included again: accepted.
        exclude(&[], "GitHub.cli", true).unwrap();
        assert!(apps_confirm(&ids).is_err());
        exclude(&[], "GitHub.cli", false).unwrap();
        let t = apps_confirm(&ids).unwrap().token;
        let reversed: Vec<String> = ids.iter().rev().cloned().collect();
        assert!(!tokens().consume(t, &token_ids("apps", &reversed)), "order is part of the binding");
        assert!(!tokens().consume(t, &token_ids("wu", &ids)), "kind is part of the binding");
        assert!(matches!(apps_run(t + 2, &ids, |_| {}), Err(UpdatesError::BadToken)));
        assert!(tokens().consume(t, &token_ids("apps", &ids)));
        assert!(matches!(apps_run(t, &ids, |_| {}), Err(UpdatesError::BadToken)), "used once");

        *WU_LAST.lock().unwrap() = vec![("2bd4bf48-5e2b-4b7a-9e1c-0b8e0c1f1a2b".into(), "x".into())];
        if !crate::system::is_elevated() {
            assert!(matches!(wu_confirm(&["2bd4bf48-5e2b-4b7a-9e1c-0b8e0c1f1a2b".into()]), Err(UpdatesError::NeedsAdmin)));
            assert!(matches!(wu_install(1, &[], |_| {}), Err(UpdatesError::NeedsAdmin)));
        } else {
            assert!(matches!(wu_confirm(&["aaaaaaaa-5e2b-4b7a-9e1c-0b8e0c1f1a2b".into()]), Err(UpdatesError::BadSelection)));
        }
    }

    #[test]
    fn history_file_is_hostile() {
        // SAFETY: same value every data-dir test sets; see `registry::tests::test_data_dir`.
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
        let f = history_file();
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        let evil = r#"[{"at":1,"kind":"apps","items":[{"id":"a\u0000b","name":"N","status":"pwned","reason":"<script>","code":1}],"reboot":false,"cancelled":false},
            {"at":2,"kind":"other","items":[],"reboot":false,"cancelled":false}]"#;
        std::fs::write(&f, evil).unwrap();
        let h = history();
        assert_eq!(h.len(), 1, "unknown kinds dropped");
        assert_eq!(h[0].items[0].id, "ab");
        assert_eq!(h[0].items[0].status, "failed");
        assert_eq!(h[0].items[0].reason, "failed");
        std::fs::write(&f, "not json").unwrap();
        assert!(history().is_empty());
        record(&Batch { at: 3, kind: "windows".into(), items: vec![], reboot: true, cancelled: false });
        assert_eq!(history().len(), 1);
    }

    /// Read-only, talks to the real winget and Windows Update (`cargo test -p g2boost-core -- --ignored live_updates --nocapture`).
    /// Lists upgrades and searches; never installs anything.
    #[test]
    #[ignore]
    fn live_updates_list_and_search() {
        let apps = app_updates(&[]).expect("winget upgrades");
        println!("winget: {} upgrades, {} eligible", apps.len(), apps.iter().filter(|a| a.skip.is_none()).count());
        for a in &apps {
            println!("  {} {} -> {} skip={:?} machine={}", a.id, a.version, a.available, a.skip, a.machine);
        }
        let cached = wu_search(false).expect("cached search");
        println!("windows update (cache): {} pending, reboot={}", cached.updates.len(), cached.reboot_required);
        let online = wu_search(true).expect("online search");
        println!("windows update (online): {} pending", online.updates.len());
        for u in &online.updates {
            println!("  {} {:?} {} B sev={:?} driver={} reboot={} cats={:?}", u.title, u.kb, u.size, u.severity, u.driver, u.reboot, u.categories);
        }
        let s = wu_status(10).expect("history");
        println!("history: {} entries, reboot_required={}", s.history.len(), s.reboot_required);
        for e in &s.history {
            println!("  {} {:?} {} {} 0x{:08X} {}", e.at, e.kb, e.operation, e.result, e.hresult, e.title);
        }
        assert!(online.updates.iter().all(|u| valid_guid(&u.id)));
    }
}
