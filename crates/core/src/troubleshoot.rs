//! Quick fixes on the Maintenance page. One click each, admin, every step reported:
//!
//! - **Windows Update reset**: stop `wuauserv`, `bits` and `cryptsvc` (running dependents first,
//!   SCM with a deadline), rename `%windir%\SoftwareDistribution` and `%windir%\System32\catroot2`
//!   aside to `<name>.bak-<yyyymmdd-hhmm>` with `MoveFileExW` (never deleted; refused when the
//!   target exists or the folder is a reparse point), start the services again. Holds the
//!   servicing slot (`repair::exclusive`): DISM's RestoreHealth downloads through Windows Update.
//! - **Print queue**: purge every *local* queue through the spooler API (`EnumPrintersW` +
//!   `OpenPrinterW` + `SetPrinterW(PRINTER_CONTROL_PURGE)`); jobs still queued a few seconds later
//!   (or a spooler that stopped answering) = restart the Spooler service and purge again. Never
//!   touches the files in `System32\spool`.
//! - **Audio**: restart `Audiosrv` and `AudioEndpointBuilder` (and anything running on top of
//!   them) in dependency order.
//!
//! Nothing but the enum comes from the caller: service names and folders are fixed, printers are
//! enumerated live. Each run goes to `fixes-history.json` (service, folder and printer names only).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, TryLockError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::Graphics::Printing::{
    ClosePrinter, EnumPrintersW, OpenPrinterW, PRINTER_ACCESS_ADMINISTER, PRINTER_CONTROL_PURGE, PRINTER_DEFAULTSW,
    PRINTER_ENUM_LOCAL, PRINTER_HANDLE, PRINTER_INFO_2W, SetPrinterW,
};
use windows::Win32::Storage::FileSystem::{MOVE_FILE_FLAGS, MoveFileExW};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::core::{PCWSTR, PWSTR};

use crate::exposure::clean;
use crate::registry::{self, wide};
use crate::{repair, safety, svc, system};

const HISTORY_MAX: usize = 50;
const STOP_LIMIT: Duration = Duration::from_secs(30);
const START_LIMIT: Duration = Duration::from_secs(30);
/// How long purged queues get to empty before the Spooler is restarted.
const SETTLE: Duration = Duration::from_secs(5);
const WU_SERVICES: [&str; 3] = ["wuauserv", "bits", "cryptsvc"];
/// Dependent first: Windows Audio runs on top of the endpoint builder.
const AUDIO_SERVICES: [&str; 2] = ["Audiosrv", "AudioEndpointBuilder"];
const SPOOLER: &str = "Spooler";
const INSUFFICIENT_BUFFER: u32 = 122;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fix {
    WuReset,
    PrintQueue,
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Stop,
    Start,
    Rename,
    Purge,
    /// Print queue: jobs left at the end.
    Check,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub action: Action,
    /// Service, folder (`SoftwareDistribution`) or printer name (data from this PC).
    pub target: String,
    pub ok: bool,
    /// Win32 error when the call failed (0 otherwise or when refused before any call).
    pub code: u32,
    /// Rename: the new folder name, or why it was refused (`reparse_point`, `target_exists`,
    /// `not_a_folder`) / skipped (`missing`). Start: `disabled` (left off). Purge: `empty`.
    /// Check: `jobs_left` / `no_answer`.
    pub note: Option<String>,
    /// Purge: jobs in the queue before; check: jobs left.
    pub jobs: Option<u32>,
}

impl Step {
    fn new(action: Action, target: &str) -> Self {
        Step { action, target: clean(target), ok: true, code: 0, note: None, jobs: None }
    }
    fn note(mut self, n: &str) -> Self {
        self.note = Some(n.to_string());
        self
    }
    fn fail(mut self, code: u32) -> Self {
        self.ok = false;
        self.code = code;
        self
    }
    fn refuse(self, why: &str) -> Self {
        self.note(why).fail(0)
    }
    fn result(self, r: Result<(), u32>) -> Self {
        match r {
            Ok(()) => self,
            Err(c) => self.fail(c),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    pub fix: Fix,
    pub at: u64,
    pub duration_ms: u64,
    pub ok: bool,
    pub steps: Vec<Step>,
}

#[derive(Debug, thiserror::Error)]
pub enum FixError {
    #[error("needs_admin")]
    NeedsAdmin,
    /// Another quick fix runs, or (Windows Update reset) a repair holds the servicing slot.
    #[error("busy")]
    Busy,
    /// `spooler_missing` / `spooler_disabled` (turned off by the user or a tweak: left alone).
    #[error("{0}")]
    Unavailable(&'static str),
}

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn code(e: windows::core::Error) -> u32 {
    (e.code().0 as u32) & 0xFFFF
}

/// Run one fix to the end (seconds; a stuck service can take its full deadline). Admin only.
pub fn run(fix: Fix) -> Result<Outcome, FixError> {
    if !system::is_elevated() {
        return Err(FixError::NeedsAdmin);
    }
    let _one = match ONE_AT_A_TIME.try_lock() {
        Ok(g) => g,
        Err(TryLockError::Poisoned(p)) => p.into_inner(),
        Err(TryLockError::WouldBlock) => return Err(FixError::Busy),
    };
    let t0 = Instant::now();
    let mut steps = Vec::new();
    match fix {
        Fix::WuReset => {
            let _slot = repair::exclusive().ok_or(FixError::Busy)?;
            wu_reset(&mut steps);
        }
        Fix::PrintQueue => print_queue(&mut steps)?,
        Fix::Audio => restart(&AUDIO_SERVICES, &mut steps),
    }
    let out = Outcome { fix, at: registry::now_unix(), duration_ms: t0.elapsed().as_millis() as u64, ok: steps.iter().all(|s| s.ok), steps };
    record(&out);
    Ok(out)
}

/* ---------------- services ---------------- */

/// Stop `order` one by one; the first failure ends the sequence. Returns what was stopped.
fn stop_all(order: &[String], steps: &mut Vec<Step>) -> (Vec<String>, bool) {
    let mut stopped = Vec::new();
    for n in order {
        let r = svc::stop(n, STOP_LIMIT);
        let failed = r.is_err();
        steps.push(Step::new(Action::Stop, n).result(r));
        if failed {
            return (stopped, false);
        }
        stopped.push(n.clone());
    }
    (stopped, true)
}

/// Start again, reverse stop order: every target (even one that was not running: that is often
/// the problem) and every dependent we stopped. Disabled services stay off.
fn start_again(order: &[String], targets: &[&str], stopped: &[String], steps: &mut Vec<Step>) {
    for n in order.iter().rev() {
        if !targets.iter().any(|t| t.eq_ignore_ascii_case(n)) && !stopped.contains(n) {
            continue;
        }
        let step = Step::new(Action::Start, n);
        steps.push(match svc::query(n) {
            Ok(Some(i)) if i.disabled => step.note("disabled"),
            Ok(None) => step.note("missing"),
            _ => step.result(svc::start(n, START_LIMIT)),
        });
    }
}

fn plan(targets: &[&str]) -> Vec<String> {
    svc::stop_order(targets, |n| svc::active_dependents(n).unwrap_or_default())
}

fn restart(targets: &[&str], steps: &mut Vec<Step>) {
    let order = plan(targets);
    let (stopped, _) = stop_all(&order, steps);
    start_again(&order, targets, &stopped, steps);
}

/* ---------------- Windows Update reset ---------------- */

/// `yyyymmdd-hhmm`, the suffix of the renamed folders.
pub(crate) fn stamp(y: u16, mo: u16, d: u16, h: u16, mi: u16) -> String {
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}")
}

/// `<dir>.bak-<stamp>` next to `dir`.
pub(crate) fn aside_path(dir: &Path, stamp: &str) -> Option<PathBuf> {
    let name = dir.file_name()?.to_str()?;
    Some(dir.with_file_name(format!("{name}.bak-{stamp}")))
}

/// Rename `dir` aside, never delete: missing = nothing to do; a reparse point, a file or an
/// existing target = refused; otherwise `MoveFileExW` with no replace/copy flag (fails rather than
/// overwrite).
pub(crate) fn rename_aside(dir: &Path, stamp: &str) -> Step {
    let step = Step::new(Action::Rename, &dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    let Some(to) = aside_path(dir, stamp) else {
        return step.refuse("bad_path");
    };
    match std::fs::symlink_metadata(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return step.note("missing"),
        Err(e) => return step.fail(e.raw_os_error().unwrap_or(0) as u32),
        Ok(md) if safety::is_reparse(&md) => return step.refuse("reparse_point"),
        Ok(md) if !md.is_dir() => return step.refuse("not_a_folder"),
        Ok(_) => {}
    }
    if std::fs::symlink_metadata(&to).is_ok() {
        return step.refuse("target_exists");
    }
    let (from_w, to_w) = (wide(&dir.to_string_lossy()), wide(&to.to_string_lossy()));
    // SAFETY: NUL-terminated paths; flags 0 = same-volume rename that fails if `to` exists.
    match unsafe { MoveFileExW(PCWSTR(from_w.as_ptr()), PCWSTR(to_w.as_ptr()), MOVE_FILE_FLAGS(0)) } {
        Ok(()) => step.note(&to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
        Err(e) => step.fail(code(e)),
    }
}

fn wu_reset(steps: &mut Vec<Step>) {
    let order = plan(&WU_SERVICES);
    let (stopped, all) = stop_all(&order, steps);
    if all {
        // SAFETY: GetLocalTime only fills the returned struct.
        let t = unsafe { GetLocalTime() };
        let stamp = stamp(t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute);
        steps.push(rename_aside(&repair::windows_dir().join("SoftwareDistribution"), &stamp));
        steps.push(rename_aside(&repair::system32().join("catroot2"), &stamp));
    }
    start_again(&order, &WU_SERVICES, &stopped, steps);
}

/* ---------------- print queue ---------------- */

#[derive(Debug, Clone, Serialize)]
pub struct Printer {
    pub name: String,
    pub jobs: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrintStatus {
    /// `running` | `stopped` | `disabled` | `missing` | `unknown`.
    pub spooler: &'static str,
    /// Local queues with their job counts; `None` when the spooler is not running or did not answer.
    pub printers: Option<Vec<Printer>>,
}

/// One local queue: the exact name (to open it again) and its jobs.
struct Queue {
    raw: String,
    jobs: u32,
}

fn local_queues() -> Result<Vec<Queue>, u32> {
    let (mut needed, mut count) = (0u32, 0u32);
    // SAFETY: size probe; succeeds only when there is nothing to list.
    match unsafe { EnumPrintersW(PRINTER_ENUM_LOCAL, PCWSTR::null(), 2, None, &mut needed, &mut count) }.map_err(code) {
        Ok(()) => return Ok(Vec::new()),
        Err(INSUFFICIENT_BUFFER) if needed > 0 => {}
        Err(c) => return Err(c),
    }
    // u64 storage keeps the PRINTER_INFO_2W array pointer-aligned.
    let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: the byte view covers `needed` bytes of `buf`.
    let bytes = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), needed as usize) };
    // SAFETY: a buffer of the size Windows asked for.
    unsafe { EnumPrintersW(PRINTER_ENUM_LOCAL, PCWSTR::null(), 2, Some(bytes), &mut needed, &mut count) }.map_err(code)?;
    let room = buf.len() * 8 / size_of::<PRINTER_INFO_2W>();
    // SAFETY: Windows wrote `count` structs at the start of the buffer; their strings live in it.
    let list = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<PRINTER_INFO_2W>(), (count as usize).min(room)) };
    // SAFETY: NUL-terminated names inside the same buffer.
    Ok(list.iter().filter_map(|p| Some(Queue { raw: unsafe { p.pPrinterName.to_string() }.ok()?, jobs: p.cJobs })).collect())
}

fn purge(name: &str) -> Result<(), u32> {
    let w = wide(name);
    let defaults = PRINTER_DEFAULTSW { pDatatype: PWSTR::null(), pDevMode: std::ptr::null_mut(), DesiredAccess: PRINTER_ACCESS_ADMINISTER };
    let mut h = PRINTER_HANDLE::default();
    // SAFETY: NUL-terminated name from EnumPrintersW; administer access is what a purge needs.
    unsafe { OpenPrinterW(PCWSTR(w.as_ptr()), &mut h, Some(&defaults)) }.map_err(code)?;
    // SAFETY: valid printer handle; level 0 + no buffer = a control command.
    let r = unsafe { SetPrinterW(h, 0, None, PRINTER_CONTROL_PURGE) }.map_err(code);
    // SAFETY: the handle opened above, closed once.
    unsafe {
        let _ = ClosePrinter(h);
    }
    r
}

/// Local queues and their jobs (no admin). Never lists network connections: purging those would
/// delete other people's jobs on the server.
pub fn print_status() -> PrintStatus {
    let spooler = match svc::query(SPOOLER) {
        Ok(None) => "missing",
        Ok(Some(i)) if i.disabled => "disabled",
        Ok(Some(i)) if !i.running => "stopped",
        Ok(Some(_)) => "running",
        Err(_) => "unknown",
    };
    let printers = (spooler == "running")
        .then(|| local_queues().ok())
        .flatten()
        .map(|q| q.into_iter().map(|q| Printer { name: clean(&q.raw), jobs: q.jobs }).collect());
    PrintStatus { spooler, printers }
}

/// Purge every queue that has jobs (one step each); false when the spooler did not answer or a
/// purge failed.
fn purge_all(steps: &mut Vec<Step>) -> bool {
    let Ok(queues) = local_queues() else {
        return false;
    };
    let mut ok = true;
    for q in queues {
        let mut s = Step::new(Action::Purge, &q.raw);
        s.jobs = Some(q.jobs);
        let s = if q.jobs == 0 { s.note("empty") } else { s.result(purge(&q.raw)) };
        ok &= s.ok;
        steps.push(s);
    }
    ok
}

/// Jobs left after giving the queues a few seconds to empty; `None` = no answer from the spooler.
fn settle() -> Option<u32> {
    let deadline = Instant::now() + SETTLE;
    loop {
        let left = local_queues().ok()?.iter().map(|q| q.jobs).sum::<u32>();
        if left == 0 || Instant::now() > deadline {
            return Some(left);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn print_queue(steps: &mut Vec<Step>) -> Result<(), FixError> {
    match svc::query(SPOOLER) {
        Ok(None) => return Err(FixError::Unavailable("spooler_missing")),
        Ok(Some(i)) if i.disabled => return Err(FixError::Unavailable("spooler_disabled")),
        Ok(Some(i)) if !i.running => steps.push(Step::new(Action::Start, SPOOLER).result(svc::start(SPOOLER, START_LIMIT))),
        _ => {}
    }
    let answered = purge_all(steps);
    let mut left = settle();
    if !answered || left != Some(0) {
        // Stuck jobs (often one "deleting" forever) or a spooler that stopped answering.
        restart(&[SPOOLER], steps);
        purge_all(steps);
        left = settle();
    }
    let mut check = Step::new(Action::Check, "jobs");
    check.jobs = left;
    steps.push(match left {
        Some(0) => check,
        Some(_) => check.refuse("jobs_left"),
        None => check.refuse("no_answer"),
    });
    Ok(())
}

/* ---------------- history ---------------- */

fn history_file() -> PathBuf {
    registry::data_dir().join("fixes-history.json")
}

pub fn history() -> Vec<Outcome> {
    std::fs::read_to_string(history_file()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn record(o: &Outcome) {
    let mut all = history();
    all.push(o.clone());
    let skip = all.len().saturating_sub(HISTORY_MAX);
    all.drain(..skip);
    let f = history_file();
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(f, serde_json::to_string_pretty(&all).unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("g2boost-fixes-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn stamp_and_aside_names() {
        assert_eq!(stamp(2026, 10, 25, 9, 5), "20261025-0905");
        let p = aside_path(Path::new(r"C:\Windows\SoftwareDistribution"), "20261025-0905").unwrap();
        assert_eq!(p, Path::new(r"C:\Windows\SoftwareDistribution.bak-20261025-0905"));
        assert_eq!(aside_path(Path::new(r"C:\"), "x"), None, "a drive root has no name to rename");
    }

    #[test]
    fn rename_aside_moves_the_folder_and_never_overwrites() {
        let dir = sandbox("rename");
        let src = dir.join("SoftwareDistribution");
        std::fs::create_dir_all(src.join("DataStore")).unwrap();
        std::fs::write(src.join("DataStore").join("DataStore.edb"), b"db").unwrap();

        let s = rename_aside(&src, "20261025-0905");
        assert!(s.ok, "{s:?}");
        assert_eq!(s.note.as_deref(), Some("SoftwareDistribution.bak-20261025-0905"));
        let moved = dir.join("SoftwareDistribution.bak-20261025-0905");
        assert!(!src.exists() && moved.join("DataStore").join("DataStore.edb").is_file(), "moved, not deleted");

        // Same minute again: the old copy is never replaced.
        std::fs::create_dir_all(&src).unwrap();
        let s = rename_aside(&src, "20261025-0905");
        assert_eq!((s.ok, s.note.as_deref()), (false, Some("target_exists")));
        assert!(src.is_dir() && moved.join("DataStore").join("DataStore.edb").is_file());

        // Nothing there: nothing to do (Windows recreates the folder anyway).
        std::fs::remove_dir(&src).unwrap();
        let s = rename_aside(&src, "20261025-0906");
        assert_eq!((s.ok, s.note.as_deref()), (true, Some("missing")));

        // A file with the folder's name.
        std::fs::write(&src, b"x").unwrap();
        assert_eq!(rename_aside(&src, "20261025-0907").note.as_deref(), Some("not_a_folder"));
        std::fs::remove_file(&src).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_aside_refuses_a_junction() {
        let dir = sandbox("junction");
        let victim = dir.join("victim");
        std::fs::create_dir_all(&victim).unwrap();
        std::fs::write(victim.join("keep.txt"), b"keep").unwrap();
        let link = dir.join("catroot2");
        crate::cleanup::tests::make_junction(&link, &victim);
        let s = rename_aside(&link, "20261025-0905");
        assert_eq!((s.ok, s.note.as_deref()), (false, Some("reparse_point")));
        assert!(link.exists() && victim.join("keep.txt").is_file() && !dir.join("catroot2.bak-20261025-0905").exists());
        let _ = std::fs::remove_dir(&link);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fixes_round_trip_and_unknown_are_refused() {
        assert_eq!(serde_json::from_str::<Fix>("\"wu_reset\"").unwrap(), Fix::WuReset);
        assert!(serde_json::from_str::<Fix>("\"delete_spool\"").is_err());
        assert!(serde_json::from_str::<Fix>("\"stop_defender\"").is_err());
        assert_eq!(serde_json::to_string(&Action::Purge).unwrap(), "\"purge\"");
    }

    #[test]
    fn refuses_without_elevation() {
        if !system::is_elevated() {
            for f in [Fix::WuReset, Fix::PrintQueue, Fix::Audio] {
                assert!(matches!(run(f), Err(FixError::NeedsAdmin)), "{f:?}");
            }
        }
        // Read-only and harmless as any user: a known spooler state, never a panic.
        let s = print_status();
        assert!(["running", "stopped", "disabled", "missing", "unknown"].contains(&s.spooler));
        assert!(s.spooler == "running" || s.printers.is_none());
    }

    /// Elevated, real actions, gated by `G2BOOST_LIVE_ACTIONS=1`: restarts Windows audio (sound
    /// drops for a few seconds) and purges the local print queues (queued jobs are lost). The
    /// Windows Update reset also needs `G2BOOST_LIVE_WU=1` (it renames the real folders aside).
    /// Elevated: `g2boost_core-*.exe --ignored troubleshoot::tests::live_fixes_elevated --nocapture`
    /// with `G2BOOST_DATA_DIR` set.
    #[test]
    #[ignore]
    fn live_fixes_elevated() {
        if std::env::var("G2BOOST_LIVE_ACTIONS").as_deref() != Ok("1") || !system::is_elevated() {
            println!("skip: needs G2BOOST_LIVE_ACTIONS=1 and admin");
            return;
        }
        println!("print status: {:?}", print_status());
        let mut fixes = vec![Fix::Audio, Fix::PrintQueue];
        if std::env::var("G2BOOST_LIVE_WU").as_deref() == Ok("1") {
            fixes.push(Fix::WuReset);
        }
        for f in fixes {
            let r = run(f);
            println!("{f:?}: {r:#?}");
        }
    }
}
