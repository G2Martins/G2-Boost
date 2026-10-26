//! Disk check and drive optimization through the Storage Management API (WMI
//! `ROOT\Microsoft\Windows\Storage`, `MSFT_Volume.Repair` / `MSFT_Volume.Optimize`): the provider
//! behind `Repair-Volume` and `Optimize-Volume`. Never `chkdsk.exe`, `defrag.exe` or PowerShell.
//!
//! - **Scan** = online scan (`chkdsk /scan`): runs with the volume in use, no restart.
//! - **SpotFix** = fixes only what a scan logged; offered only after a scan (or Windows) flagged it.
//! - **Analyze / ReTrim / Defrag**: ReTrim only on SSDs, Defrag only on disks that positively
//!   report a seek penalty (HDD). An unknown drive type never gets a defrag.
//!
//! Every volume id comes back from the hostile caller and is matched against the live list; the
//! WMI path used for the call is the one WMI itself returned (`__RELPATH`), never caller text.
//! Shares the app-wide servicing slot and cancel flag with `repair` (one job at a time).
//! Calls are asynchronous (`ExecMethodAsync` + our sink) on a dedicated MTA thread, so the run can
//! be cancelled (`CancelAsyncCall`) and progress is picked up if the provider reports it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CoUninitialize,
    EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Variant::{VARIANT, VT_NULL};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemObjectSink, IWbemObjectSink_Impl, IWbemObjectSinkEx, IWbemObjectSinkEx_Impl,
    IWbemServices, WBEM_FLAG_SEND_STATUS, WBEM_GENERIC_FLAG_TYPE, WBEM_STATUS_COMPLETE, WBEM_STATUS_PROGRESS, WbemLocator,
};
use windows::core::{BSTR, HRESULT, Interface, PCWSTR, Ref, implement};

use crate::registry::{self, Root, Value, wide};
use crate::{hardware, repair, system, wmi};

const NS: &str = r"ROOT\Microsoft\Windows\Storage";
const HISTORY_MAX: usize = 50;
/// `ERROR_TIMEOUT`, as in `repair`.
const TIMEOUT_CODE: u32 = 1460;
const DFRG_STATS: &str = r"SOFTWARE\Microsoft\Dfrg\Statistics";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// `Repair(Scan)`: online scan, the volume stays in use.
    Scan,
    /// `Repair(SpotFix)`: fixes what the scan logged (brief dismount; system drive at restart).
    SpotFix,
    /// `Optimize(Analyze)`: fragmentation, read by Windows' own defrag statistics afterwards.
    Analyze,
    /// `Optimize(ReTrim)`: SSD only.
    Retrim,
    /// `Optimize(Defrag)`: HDD only.
    Defrag,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Media {
    Ssd,
    Hdd,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Healthy,
    /// Problems found and fixed (online or by the spot fix).
    Fixed,
    /// The scan logged problems a spot fix can repair.
    NeedsSpotFix,
    /// Windows wants a full offline check at restart (not run from here).
    NeedsOffline,
    /// Transient (snapshot) error: run the scan again.
    Retry,
    /// Optimize / analyze finished.
    Optimized,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct Volume {
    /// `\\?\Volume{GUID}\`: the id every operation must name.
    pub id: String,
    pub letter: String,
    pub label: String,
    pub fs: String,
    pub size: u64,
    pub free: u64,
    /// `healthy` | `warning` | `unhealthy` | `unknown` (`MSFT_Volume.HealthStatus`).
    pub health: &'static str,
    /// What Windows itself flagged: `scan` | `spot_fix` | `full_repair` (`OperationalStatus`).
    pub needs: Option<&'static str>,
    pub media: Media,
    pub system: bool,
    /// Last fragmentation Windows measured (HDD / unknown drives only), percent.
    pub fragmentation: Option<u32>,
    /// Date of Windows' last analysis or optimization (`YYYY-MM-DD`).
    pub last_optimized: Option<String>,
    /// The operations this volume allows (the core enforces the same list).
    pub ops: Vec<Op>,
    #[serde(skip)]
    relpath: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    pub op: Op,
    pub volume: String,
    pub letter: String,
    pub at: u64,
    pub duration_ms: u64,
    pub verdict: Verdict,
    /// HRESULT, method return value (e.g. 40001 access denied, 43001 file system not supported)
    /// or 1460 = stopped after the time limit.
    pub code: u32,
    /// `Repair` output (`Volume.RepairStatus`: 0 none, 1/2/5 fixed, 6/7 spot fix, 13 offline...).
    pub output: Option<u32>,
    /// After an analyze: percent fragmented.
    pub fragmentation: Option<u32>,
    /// The provider asked a yes/no question and was answered "no" (never auto-confirmed).
    #[serde(default)]
    pub prompted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Running {
    pub op: Op,
    pub volume: String,
    pub letter: String,
    pub percent: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum VolError {
    #[error("needs_admin")]
    NeedsAdmin,
    #[error("busy")]
    Busy,
    #[error("unknown_volume")]
    UnknownVolume,
    /// The volume does not allow this operation (file system, drive type, no scan finding).
    #[error("not_allowed")]
    NotAllowed,
    #[error("wmi: {0}")]
    Wmi(String),
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

pub fn status() -> Option<Running> {
    RUNNING.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

fn set_running(v: Option<Running>) {
    *RUNNING.lock().unwrap_or_else(|p| p.into_inner()) = v;
}

/* ---------------- pure rules ---------------- */

/// What a volume allows. Defrag needs positive HDD evidence; ReTrim needs an SSD.
pub(crate) fn allowed(fs: &str, media: Media, drive_type: i64, spot_fix_due: bool) -> Vec<Op> {
    if drive_type != 3 {
        return Vec::new(); // fixed drives only
    }
    let ntfs = fs.eq_ignore_ascii_case("NTFS");
    let refs = fs.eq_ignore_ascii_case("ReFS");
    let mut ops = Vec::new();
    if ntfs {
        ops.push(Op::Scan);
        if spot_fix_due {
            ops.push(Op::SpotFix);
        }
    }
    match media {
        Media::Ssd if ntfs || refs => ops.push(Op::Retrim),
        Media::Hdd if ntfs => ops.extend([Op::Analyze, Op::Defrag]),
        Media::Unknown if ntfs => ops.push(Op::Analyze),
        _ => {}
    }
    ops
}

/// SSD only when every disk under the volume says so; HDD likewise; anything mixed or silent
/// is unknown (and so never defragmented).
pub(crate) fn media_of(disks: &[u32], ssd: impl Fn(u32) -> Option<bool>) -> Media {
    let kinds: Vec<Option<bool>> = disks.iter().map(|d| ssd(*d)).collect();
    if kinds.is_empty() || kinds.iter().any(Option::is_none) {
        Media::Unknown
    } else if kinds.iter().all(|k| *k == Some(true)) {
        Media::Ssd
    } else if kinds.iter().all(|k| *k == Some(false)) {
        Media::Hdd
    } else {
        Media::Unknown
    }
}

/// `MSFT_Volume.OperationalStatus` codes Windows sets after its own checks.
pub(crate) fn needs_of(status: &[i64]) -> Option<&'static str> {
    if status.contains(&53263) {
        Some("full_repair")
    } else if status.contains(&53262) {
        Some("spot_fix")
    } else if status.contains(&53261) {
        Some("scan")
    } else {
        None
    }
}

/// `Volume.RepairStatus` (Storage module) → verdict.
pub(crate) fn repair_verdict(output: u32) -> Verdict {
    match output {
        0 | 4 => Verdict::Healthy,
        1 | 2 | 5 | 16 => Verdict::Fixed,
        6 | 7 | 15 => Verdict::NeedsSpotFix,
        13 => Verdict::NeedsOffline,
        8 | 10 | 12 => Verdict::Retry,
        _ => Verdict::Failed,
    }
}

/// `\\?\Volume{GUID}\` → `Volume{GUID}` (the Dfrg statistics key), only for a well-formed GUID.
pub(crate) fn guid_key(path: &str) -> Option<String> {
    let g = path.strip_prefix(r"\\?\Volume{")?.strip_suffix(r"}\")?;
    (g.len() == 36 && g.chars().all(|c| c.is_ascii_hexdigit() || c == '-')).then(|| format!("Volume{{{g}}}"))
}

/// SYSTEMTIME blob (`wYear, wMonth, wDayOfWeek, wDay, ...`) → `YYYY-MM-DD`; zero = never.
pub(crate) fn systemtime_date(b: &[u8]) -> Option<String> {
    let w = |i: usize| b.get(i * 2..i * 2 + 2).map(|x| u16::from_le_bytes([x[0], x[1]]));
    let (y, m, d) = (w(0)?, w(1)?, w(3)?);
    ((1990..=2200).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d)).then(|| format!("{y:04}-{m:02}-{d:02}"))
}

/* ---------------- listing ---------------- */

fn int(row: &wmi::Row, k: &str) -> i64 {
    match row.get(k) {
        Some(wmi::WmiValue::Int(i)) => *i,
        _ => -1,
    }
}

/// `uint64` properties arrive as strings (WMI's VARIANT mapping for CIM_UINT64).
fn bytes(row: &wmi::Row, k: &str) -> u64 {
    match row.get(k) {
        Some(wmi::WmiValue::Str(s)) => s.parse().unwrap_or(0),
        Some(wmi::WmiValue::Int(i)) => (*i).max(0) as u64,
        _ => 0,
    }
}

fn text(row: &wmi::Row, k: &str) -> String {
    row.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string()
}

/// Windows' last fragmentation figure and date for the volume (readable without admin).
fn dfrg_stats(path: &str) -> (Option<u32>, Option<String>) {
    let Some(key) = guid_key(path) else {
        return (None, None);
    };
    let k = format!(r"{DFRG_STATS}\{key}");
    let date = match registry::read(Root::Hklm, &k, "LastRunTime") {
        Ok(Value::Binary(b)) => systemtime_date(&b),
        _ => None,
    };
    let frag = match registry::read(Root::Hklm, &k, "LastRunPercentFragmentation") {
        Ok(Value::Dword(d)) if date.is_some() => Some(d.min(100)),
        _ => None,
    };
    (frag, date)
}

/// Fixed volumes with a drive letter, from the live `MSFT_Volume` list (no admin).
pub fn list() -> Result<Vec<Volume>, VolError> {
    // The key (`ObjectId`) must be selected, or WMI returns no `__RELPATH`.
    let fields =
        ["__RELPATH", "ObjectId", "Path", "DriveLetter", "FileSystem", "FileSystemLabel", "Size", "SizeRemaining", "HealthStatus", "OperationalStatus", "DriveType"];
    let rows = wmi::query(NS, &format!("SELECT {} FROM MSFT_Volume", fields[1..].join(", ")), &fields).map_err(VolError::Wmi)?;
    let disks = hardware::disks();
    let system_letter = std::env::var("SystemDrive").ok().and_then(|s| s.chars().next()).map(|c| c.to_ascii_uppercase());
    let history = history();
    let mut out = Vec::new();
    for r in rows {
        let Some(letter) = u32::try_from(int(&r, "DriveLetter")).ok().and_then(char::from_u32).filter(char::is_ascii_alphabetic) else {
            continue;
        };
        let letter = letter.to_ascii_uppercase();
        let (id, relpath, fs) = (text(&r, "Path"), text(&r, "__RELPATH"), text(&r, "FileSystem"));
        if id.is_empty() || relpath.is_empty() {
            continue;
        }
        let status = match r.get("OperationalStatus") {
            Some(wmi::WmiValue::IntList(v)) => v.clone(),
            Some(wmi::WmiValue::Int(i)) => vec![*i],
            _ => Vec::new(),
        };
        let needs = needs_of(&status);
        let media = media_of(&hardware::volume_disks(letter), |n| disks.iter().find(|d| d.index == n).and_then(|d| d.ssd));
        let last_scan = history.iter().rev().find(|o| o.volume == id && matches!(o.op, Op::Scan | Op::SpotFix));
        let spot_fix_due =
            matches!(needs, Some("spot_fix" | "full_repair")) || last_scan.is_some_and(|o| o.op == Op::Scan && o.verdict == Verdict::NeedsSpotFix);
        let (frag, last_optimized) = dfrg_stats(&id);
        let drive_type = int(&r, "DriveType");
        out.push(Volume {
            ops: allowed(&fs, media, drive_type, spot_fix_due),
            letter: letter.to_string(),
            label: text(&r, "FileSystemLabel"),
            size: bytes(&r, "Size"),
            free: bytes(&r, "SizeRemaining"),
            health: match int(&r, "HealthStatus") {
                0 => "healthy",
                1 => "warning",
                2 => "unhealthy",
                _ => "unknown",
            },
            needs,
            media,
            system: system_letter == Some(letter),
            fragmentation: if media == Media::Ssd { None } else { frag },
            last_optimized,
            fs,
            id,
            relpath,
        });
    }
    out.sort_by(|a, b| a.letter.cmp(&b.letter));
    Ok(out)
}

/* ---------------- async WMI method call ---------------- */

#[derive(Default)]
struct Shared {
    /// Final HRESULT from `SetStatus(WBEM_STATUS_COMPLETE)`.
    done: Mutex<Option<i32>>,
    /// (`ReturnValue`, `Output`) from the out-parameters object.
    out: Mutex<(Option<u32>, Option<u32>)>,
    /// Percent + 1 (0 = none reported yet).
    percent: AtomicU32,
    prompted: AtomicBool,
}

pub(crate) fn get_u32(o: &IWbemClassObject, name: &str) -> Option<u32> {
    let n = wide(name);
    let mut v = VARIANT::default();
    // SAFETY: NUL-terminated name; WMI fills a VARIANT that drops (VariantClear) itself.
    unsafe { o.Get(PCWSTR(n.as_ptr()), 0, &mut v, None, None) }.ok()?;
    if v.is_empty() || v.vt() == VT_NULL {
        return None;
    }
    u32::try_from(&v).ok()
}

/// Receives the out-parameters, the final status and any progress the provider reports.
#[implement(IWbemObjectSinkEx)]
struct Sink(Arc<Shared>);

impl IWbemObjectSink_Impl for Sink_Impl {
    fn Indicate(&self, count: i32, objs: *const Option<IWbemClassObject>) -> windows::core::Result<()> {
        if objs.is_null() || count <= 0 {
            return Ok(());
        }
        // SAFETY: WMI passes `count` object pointers, valid for this call.
        let objs = unsafe { std::slice::from_raw_parts(objs, count as usize) };
        let mut out = self.0.out.lock().unwrap_or_else(|p| p.into_inner());
        for o in objs.iter().flatten() {
            out.0 = get_u32(o, "ReturnValue").or(out.0);
            out.1 = get_u32(o, "Output").or(out.1);
        }
        Ok(())
    }

    fn SetStatus(&self, flags: i32, hr: HRESULT, _param: &BSTR, _obj: Ref<IWbemClassObject>) -> windows::core::Result<()> {
        if flags == WBEM_STATUS_COMPLETE.0 {
            *self.0.done.lock().unwrap_or_else(|p| p.into_inner()) = Some(hr.0);
        } else if flags == WBEM_STATUS_PROGRESS.0 {
            // LOWORD = progress so far, HIWORD = total.
            let (cur, total) = (hr.0 as u32 & 0xFFFF, (hr.0 as u32) >> 16);
            if let Some(p) = (cur * 100).checked_div(total) {
                self.0.percent.store(p.min(100) + 1, Ordering::Relaxed);
            }
        }
        Ok(())
    }
}

impl IWbemObjectSinkEx_Impl for Sink_Impl {
    fn WriteMessage(&self, _channel: u32, _message: &BSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn WriteError(&self, _err: Ref<IWbemClassObject>) -> windows::core::Result<u8> {
        Ok(0)
    }
    /// Never auto-confirm a question from the provider (e.g. a forced dismount): answer "no".
    fn PromptUser(&self, _message: &BSTR, _kind: u8) -> windows::core::Result<u8> {
        self.0.prompted.store(true, Ordering::Relaxed);
        Ok(0)
    }
    fn WriteProgress(&self, _a: &BSTR, _b: &BSTR, _c: &BSTR, percent: u32, _secs: u32) -> windows::core::Result<()> {
        if percent <= 100 {
            self.0.percent.store(percent + 1, Ordering::Relaxed);
        }
        Ok(())
    }
    fn WriteStreamParameter(&self, _n: &BSTR, _v: *const VARIANT, _t: u32, _f: u32) -> windows::core::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Default)]
struct Call {
    hr: i32,
    ret: Option<u32>,
    output: Option<u32>,
    cancelled: bool,
    timed_out: bool,
    prompted: bool,
}

fn hr_err(e: windows::core::Error) -> String {
    format!("{:#010x} {}", e.code().0 as u32, e.message())
}

/// `relpath.method(params)` asynchronously, polled every 200 ms for the end, the shared cancel
/// flag and `limit` (both cancel the call). Runs on its own MTA thread: the sink is called on RPC
/// threads, no message pump needed.
fn invoke(relpath: &str, method: &str, params: &[(&str, bool)], limit: Duration, on_progress: &(dyn Fn(Option<u32>) + Sync)) -> Result<Call, String> {
    std::thread::scope(|s| {
        s.spawn(|| {
            // SAFETY: fresh thread, balanced with CoUninitialize below after every COM object is gone.
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if hr.is_err() {
                return Err(format!("{:#010x}", hr.0 as u32));
            }
            let r = invoke_mta(relpath, method, params, limit, on_progress);
            // SAFETY: balanced with the successful CoInitializeEx above.
            unsafe { CoUninitialize() };
            r
        })
        .join()
        .unwrap_or_else(|_| Err("panic".into()))
    })
}

fn invoke_mta(relpath: &str, method: &str, params: &[(&str, bool)], limit: Duration, on_progress: &(dyn Fn(Option<u32>) + Sync)) -> Result<Call, String> {
    let shared = Arc::new(Shared::default());
    // SAFETY: standard WMI client sequence (as in wmi.rs); every COM object releases on drop.
    unsafe {
        let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).map_err(hr_err)?;
        let empty = BSTR::new();
        let svc: IWbemServices = locator.ConnectServer(&BSTR::from(NS), &empty, &empty, &empty, 0, &empty, None).map_err(hr_err)?;
        CoSetProxyBlanket(&svc, RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE, PCWSTR::null(), RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE)
            .map_err(hr_err)?;
        let mut class = None;
        svc.GetObject(&BSTR::from("MSFT_Volume"), WBEM_GENERIC_FLAG_TYPE(0), None, Some(&mut class), None).map_err(hr_err)?;
        let class: IWbemClassObject = class.ok_or("no class")?;
        let m = wide(method);
        let mut sig = None;
        class.GetMethod(PCWSTR(m.as_ptr()), 0, &mut sig, std::ptr::null_mut()).map_err(hr_err)?;
        let input = match sig {
            Some(sig) => Some(sig.SpawnInstance(0).map_err(hr_err)?),
            None => None, // method without input parameters
        };
        if let Some(input) = &input {
            for (name, on) in params {
                let n = wide(name);
                input.Put(PCWSTR(n.as_ptr()), 0, &VARIANT::from(*on), 0).map_err(hr_err)?;
            }
        }
        let sink_ex: IWbemObjectSinkEx = Sink(shared.clone()).into();
        let sink: IWbemObjectSink = sink_ex.cast().map_err(hr_err)?;
        svc.ExecMethodAsync(&BSTR::from(relpath), &BSTR::from(method), WBEM_FLAG_SEND_STATUS, None, input.as_ref(), &sink).map_err(hr_err)?;

        let deadline = Instant::now() + limit;
        let mut call = Call::default();
        let mut stop_at: Option<Instant> = None;
        let mut last = 0u32;
        loop {
            if let Some(hr) = *shared.done.lock().unwrap_or_else(|p| p.into_inner()) {
                call.hr = hr;
                break;
            }
            let p = shared.percent.load(Ordering::Relaxed);
            if p != last {
                last = p;
                on_progress(p.checked_sub(1));
            }
            if stop_at.is_none() && (repair::cancelled() || Instant::now() > deadline) {
                call.cancelled = repair::cancelled();
                call.timed_out = !call.cancelled;
                let _ = svc.CancelAsyncCall(&sink);
                stop_at = Some(Instant::now());
            }
            // ponytail: after a cancel, give the provider 30 s to say so, then stop waiting.
            if stop_at.is_some_and(|t| t.elapsed() > Duration::from_secs(30)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        let out = *shared.out.lock().unwrap_or_else(|p| p.into_inner());
        (call.ret, call.output) = out;
        call.prompted = shared.prompted.load(Ordering::Relaxed);
        Ok(call)
    }
}

/* ---------------- run ---------------- */

fn method_of(op: Op) -> (&'static str, &'static str, Duration) {
    let h = |n: u64| Duration::from_secs(n * 3600);
    match op {
        Op::Scan => ("Repair", "Scan", h(2)),
        Op::SpotFix => ("Repair", "SpotFix", h(1)),
        Op::Analyze => ("Optimize", "Analyze", h(1)),
        Op::Retrim => ("Optimize", "ReTrim", h(2)),
        Op::Defrag => ("Optimize", "Defrag", h(12)),
    }
}

/// Run `op` on the live volume `volume` (an id from [`list`]) to the end. Admin only; one
/// servicing job at a time (shared with DISM/SFC/MRT and the system-files cleanup).
pub fn run(op: Op, volume: &str, on_progress: &(dyn Fn(Option<u32>) + Sync)) -> Result<Outcome, VolError> {
    if !system::is_elevated() {
        return Err(VolError::NeedsAdmin);
    }
    let v = live(op, volume)?;
    let _slot = repair::exclusive().ok_or(VolError::Busy)?;
    run_on(&v, op, on_progress)
}

/// [`run`] for a caller that already holds the servicing slot (the weekly review).
pub(crate) fn run_held(op: Op, volume: &str, on_progress: &(dyn Fn(Option<u32>) + Sync)) -> Result<Outcome, VolError> {
    if !system::is_elevated() {
        return Err(VolError::NeedsAdmin);
    }
    run_on(&live(op, volume)?, op, on_progress)
}

/// The live volume `volume` names, if it allows `op`.
fn live(op: Op, volume: &str) -> Result<Volume, VolError> {
    let v = list()?.into_iter().find(|v| v.id == volume).ok_or(VolError::UnknownVolume)?;
    if !v.ops.contains(&op) {
        return Err(VolError::NotAllowed);
    }
    Ok(v)
}

fn run_on(v: &Volume, op: Op, on_progress: &(dyn Fn(Option<u32>) + Sync)) -> Result<Outcome, VolError> {
    let running = |percent| Running { op, volume: v.id.clone(), letter: v.letter.clone(), percent };
    set_running(Some(running(None)));
    let progress = |p: Option<u32>| {
        set_running(Some(running(p)));
        on_progress(p);
    };
    let t0 = Instant::now();
    let (method, flag, limit) = method_of(op);
    let call = invoke(&v.relpath, method, &[(flag, true)], limit, &progress);
    set_running(None);
    let call = call.map_err(VolError::Wmi)?;
    let mut out = Outcome {
        op,
        volume: v.id.clone(),
        letter: v.letter.clone(),
        at: registry::now_unix(),
        duration_ms: t0.elapsed().as_millis() as u64,
        verdict: Verdict::Failed,
        code: 0,
        output: call.output,
        fragmentation: None,
        prompted: call.prompted,
    };
    if call.cancelled {
        out.verdict = Verdict::Cancelled;
    } else if call.timed_out {
        out.code = TIMEOUT_CODE;
    } else if call.hr < 0 {
        out.code = call.hr as u32;
    } else if let Some(ret) = call.ret.filter(|r| *r != 0) {
        out.code = ret;
    } else if matches!(op, Op::Scan | Op::SpotFix) {
        out.verdict = call.output.map_or(Verdict::Failed, repair_verdict);
    } else {
        out.verdict = Verdict::Optimized;
        if op == Op::Analyze {
            out.fragmentation = dfrg_stats(&v.id).0;
        }
    }
    record(&out);
    Ok(out)
}

/* ---------------- history (volume ids and letters only) ---------------- */

fn history_file() -> PathBuf {
    registry::data_dir().join("volume-history.json")
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
    /// Elevated, real actions, gated by `G2BOOST_LIVE_ACTIONS=1`: online scan + ReTrim of the
    /// system drive and an Analyze of every drive Windows can't classify (no defrag, no SpotFix).
    #[test]
    #[ignore]
    fn live_actions_elevated() {
        if std::env::var("G2BOOST_LIVE_ACTIONS").as_deref() != Ok("1") || !crate::system::is_elevated() {
            println!("skip: needs G2BOOST_LIVE_ACTIONS=1 and admin");
            return;
        }
        let vols = super::list().unwrap();
        let progress = |p: Option<u32>| eprintln!("  {p:?}%");
        for v in &vols {
            let ops: &[super::Op] = if v.letter.trim_end_matches(':').eq_ignore_ascii_case("C") {
                &[super::Op::Scan, super::Op::Retrim]
            } else if v.ops.contains(&super::Op::Analyze) {
                &[super::Op::Analyze]
            } else {
                &[]
            };
            for &op in ops {
                let t0 = std::time::Instant::now();
                let r = super::run(op, &v.id, &progress);
                println!("{} {op:?}: {r:?} in {} s", v.letter, t0.elapsed().as_secs());
            }
        }
    }
    use super::*;

    #[test]
    fn defrag_only_with_hdd_evidence_and_retrim_only_on_ssd() {
        assert_eq!(allowed("NTFS", Media::Ssd, 3, false), vec![Op::Scan, Op::Retrim]);
        assert_eq!(allowed("NTFS", Media::Hdd, 3, false), vec![Op::Scan, Op::Analyze, Op::Defrag]);
        assert_eq!(allowed("NTFS", Media::Unknown, 3, false), vec![Op::Scan, Op::Analyze], "unknown drive: never defrag");
        assert_eq!(allowed("ntfs", Media::Hdd, 3, true), vec![Op::Scan, Op::SpotFix, Op::Analyze, Op::Defrag]);
        assert_eq!(allowed("ReFS", Media::Ssd, 3, true), vec![Op::Retrim], "no chkdsk on ReFS");
        assert_eq!(allowed("ReFS", Media::Hdd, 3, false), Vec::<Op>::new());
        assert_eq!(allowed("FAT32", Media::Hdd, 3, false), Vec::<Op>::new());
        assert_eq!(allowed("NTFS", Media::Hdd, 2, false), Vec::<Op>::new(), "removable");
        for media in [Media::Ssd, Media::Unknown] {
            for fs in ["NTFS", "ReFS", "FAT32", "exFAT"] {
                assert!(!allowed(fs, media, 3, true).contains(&Op::Defrag), "{fs} {media:?}");
            }
        }
    }

    #[test]
    fn media_needs_every_disk_to_agree() {
        let ssd = |n: u32| match n {
            0 | 1 => Some(true),
            2 => Some(false),
            _ => None,
        };
        assert_eq!(media_of(&[0], ssd), Media::Ssd);
        assert_eq!(media_of(&[0, 1], ssd), Media::Ssd);
        assert_eq!(media_of(&[2], ssd), Media::Hdd);
        assert_eq!(media_of(&[0, 2], ssd), Media::Unknown, "spanned over SSD + HDD");
        assert_eq!(media_of(&[2, 9], ssd), Media::Unknown, "a silent disk");
        assert_eq!(media_of(&[], ssd), Media::Unknown);
    }

    #[test]
    fn statuses_and_repair_outputs() {
        assert_eq!(needs_of(&[2]), None);
        assert_eq!(needs_of(&[2, 53261]), Some("scan"));
        assert_eq!(needs_of(&[53262, 53261]), Some("spot_fix"));
        assert_eq!(needs_of(&[53263]), Some("full_repair"));
        assert_eq!(repair_verdict(0), Verdict::Healthy);
        assert_eq!(repair_verdict(4), Verdict::Healthy);
        assert_eq!(repair_verdict(5), Verdict::Fixed);
        assert_eq!(repair_verdict(7), Verdict::NeedsSpotFix);
        assert_eq!(repair_verdict(13), Verdict::NeedsOffline);
        assert_eq!(repair_verdict(10), Verdict::Retry);
        assert_eq!(repair_verdict(3), Verdict::Failed);
        assert_eq!(repair_verdict(999), Verdict::Failed);
    }

    #[test]
    fn dfrg_key_and_dates() {
        assert_eq!(guid_key(r"\\?\Volume{814d8108-e1ce-4b41-b0b2-cc9ab53904a9}\").as_deref(), Some("Volume{814d8108-e1ce-4b41-b0b2-cc9ab53904a9}"));
        assert_eq!(guid_key(r"\\?\Volume{..\..\x}\"), None, "no traversal into another key");
        assert_eq!(guid_key(r"C:\"), None);
        // Real blob from this PC's C: (2025-07-31 08:05:53).
        let b = [233, 7, 7, 0, 4, 0, 31, 0, 8, 0, 5, 0, 53, 0, 174, 1];
        assert_eq!(systemtime_date(&b).as_deref(), Some("2025-07-31"));
        assert_eq!(systemtime_date(&[0; 16]), None, "never ran");
        assert_eq!(systemtime_date(&[1, 2]), None);
    }

    #[test]
    fn ops_round_trip_and_unknown_are_refused() {
        assert_eq!(serde_json::from_str::<Op>("\"spot_fix\"").unwrap(), Op::SpotFix);
        assert!(serde_json::from_str::<Op>("\"format\"").is_err());
        assert!(serde_json::from_str::<Op>("\"offline_scan_and_fix\"").is_err());
        assert_eq!(serde_json::to_string(&Verdict::NeedsSpotFix).unwrap(), "\"needs_spot_fix\"");
    }

    #[test]
    fn refuses_without_elevation_or_for_an_unknown_volume() {
        if system::is_elevated() {
            // Elevated: an id that is not in the live list never reaches WMI.
            assert!(matches!(run(Op::Scan, r"\\?\Volume{00000000-0000-0000-0000-000000000000}\", &|_| {}), Err(VolError::UnknownVolume)));
        } else {
            assert!(matches!(run(Op::Scan, "C:", &|_| {}), Err(VolError::NeedsAdmin)));
        }
    }

    /// Read-only, no admin: the live volumes with media, health and what each allows.
    /// `--ignored live_volumes --nocapture`
    #[test]
    #[ignore]
    fn live_volumes() {
        let v = list().expect("MSFT_Volume");
        assert!(v.iter().any(|v| v.system && v.size > v.free && v.free > 0), "system drive listed with sizes");
        for v in &v {
            println!(
                "{}: {} {} {:?} health={} needs={:?} frag={:?} last={:?} ops={:?} size={} free={}",
                v.letter, v.fs, v.label, v.media, v.health, v.needs, v.fragmentation, v.last_optimized, v.ops, v.size, v.free
            );
        }
    }

    /// Read-only: the async plumbing (sink, out-parameters, final status) with a method that
    /// changes nothing (`GetSupportedFileSystems`). Non-elevated it answers 40001 (access denied),
    /// which still proves the callbacks arrive. `--ignored live_async_method --nocapture`
    #[test]
    #[ignore]
    fn live_async_method() {
        let v = list().unwrap().into_iter().find(|v| v.system).unwrap();
        let c = invoke(&v.relpath, "GetSupportedFileSystems", &[], Duration::from_secs(60), &|_| {}).expect("call");
        println!("{c:?}");
        assert!(!c.cancelled && !c.timed_out, "the final status arrived");
        assert!(c.hr >= 0 && c.ret.is_some(), "{c:?}");
    }
}
