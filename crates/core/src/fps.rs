//! Frames per second of ONE process, read from Windows' own present events: nothing is injected
//! into the game and no driver is installed (anti-cheat safe: a separate process reading event
//! tracing, like the tools reviewers use).
//!
//! Source: a real-time ETW session (`G2Boost-Present`, native API only: StartTraceW, EnableTraceEx2,
//! OpenTraceW, ProcessTrace) on three manifest providers, the same events PresentMon (MIT) builds
//! on. Ids, keywords and fields read from the manifests registered on Windows 11 26200
//! (`Get-WinEvent -ListProvider <name>`, at dev time):
//!
//! | provider | GUID | events | keyword |
//! |---|---|---|---|
//! | Microsoft-Windows-DXGI | {CA11C036-0102-4A2D-A6AD-F03CFED5D3C9} | 42 Present_Start, 55 PresentMultiplaneOverlay_Start | Events 0x2 |
//! | Microsoft-Windows-D3D9 | {783ACA0A-790E-4D7F-8451-AA850511C6B9} | 1 Present_Start | Events 0x2 |
//! | Microsoft-Windows-DxgKrnl | {802EC45A-1E99-4B83-9920-87C98277BA9D} | 166 Blit, 168 Flip, 171 PresentHistory_Start, 215 PresentHistoryDetailed_Start; 184 Present (end of the kernel present call) | Present 0x8000000 |
//!
//! Counting (`Tracker`, pure and tested with synthetic sequences):
//! * Direct3D 9/10/11/12 games: one frame per runtime Present_Start, grouped by swap chain (the
//!   first payload field, 4 bytes in a 32-bit process); the busiest swap chain is the game's.
//! * Vulkan / OpenGL (no DXGI/D3D9 present seen): one frame per kernel present call on a thread.
//!   The first work event (blit, flip, present history) opens it, Present_Info closes it, and the
//!   same work event showing up again without a close starts the next one. DXGI presents reach the
//!   kernel too, so kernel frames only count when the process showed no runtime present.
//!
//! Only the listed ids reach the session (EVENT_FILTER_TYPE_EVENT_ID), buffers are bounded
//! (64 KB each, at most 2 per CPU + 16), timestamps are QPC (ClientContext 1), and the session is
//! flushed before each read so the overlay is one read behind at most. Needs administrator:
//! StartTraceW answers ERROR_ACCESS_DENIED otherwise, surfaced as [`EtwError::NeedsAdmin`].

use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

use serde::Serialize;
use windows::Win32::Foundation::GetLastError;
use windows::Win32::System::Diagnostics::Etw::{
    CONTROLTRACE_HANDLE, CloseTrace, ENABLE_TRACE_PARAMETERS, ENABLE_TRACE_PARAMETERS_VERSION_2,
    EVENT_CONTROL_CODE_ENABLE_PROVIDER, EVENT_FILTER_DESCRIPTOR, EVENT_FILTER_TYPE_EVENT_ID,
    EVENT_HEADER_FLAG_32_BIT_HEADER, EVENT_RECORD, EVENT_TRACE_CONTROL_FLUSH, EVENT_TRACE_CONTROL_STOP,
    EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE, EnableTraceEx2, OpenTraceW,
    PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME, PROCESSTRACE_HANDLE, ProcessTrace, StartTraceW,
    TRACE_LEVEL_INFORMATION,
};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::core::{GUID, PCWSTR, PWSTR};

pub use crate::netetw::EtwError;
use crate::netetw::{api, control_session, props};

/// Session name. Constant so an orphan left by a crash is found and stopped on the next start.
pub const SESSION_NAME: &str = "G2Boost-Present";

const DXGI: GUID = GUID::from_u128(0xca11c036_0102_4a2d_a6ad_f03cfed5d3c9);
const D3D9: GUID = GUID::from_u128(0x783aca0a_790e_4d7f_8451_aa850511c6b9);
const DXGKRNL: GUID = GUID::from_u128(0x802ec45a_1e99_4b83_9920_87c98277ba9d);
const DXGI_PRESENTS: &[u16] = &[42, 55];
const D3D9_PRESENTS: &[u16] = &[1];
const KERNEL_WORK: &[u16] = &[166, 168, 171, 215];
const KERNEL_END: u16 = 184;
const KERNEL_EVENTS: &[u16] = &[166, 168, 171, 215, KERNEL_END];
const KW_EVENTS: u64 = 0x2; // DXGI / D3D9 "Events"
const KW_PRESENT: u64 = 0x800_0000; // DxgKrnl "Present"
/// INVALID_PROCESSTRACE_HANDLE on 64-bit.
const INVALID_TRACE: u64 = u64::MAX;

/// Statistics window: the 1 % low and p99 come from the frame times of the last 10 s.
pub const WINDOW_MS: f64 = 10_000.0;
/// No present for this long: the process is not drawing (paused, minimized, a menu that waits).
const STALE_MS: f64 = 2_000.0;
/// A gap this long is a pause (alt-tab, loading screen), not a frame.
const PAUSE_MS: f64 = 1_000.0;
// ponytail: fixed caps (16 swap chains × 4096 presents, 256 threads); at 1000+ FPS the window shrinks to ~4 s.
const MAX_SWAPCHAINS: usize = 16;
const MAX_FRAMES: usize = 4096;
const MAX_THREADS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Stats {
    /// Frames in the last second.
    pub fps: f64,
    /// Average frame time over the last second (ms).
    pub frame_ms: f64,
    /// 1 % low: average FPS of the slowest 1 % of the frames in the window.
    pub low1_fps: f64,
    /// 99th percentile frame time in the window (nearest rank, ms).
    pub p99_ms: f64,
    /// Frame times the window holds.
    pub frames: u32,
}

/// Statistics from present timestamps in ms (any order, any clock) at `now_ms` on the same clock.
/// The window ends at the newest present (delivery latency does not matter). `None` while the
/// process is not drawing: fewer than two presents, or none in the last 2 s. Pure.
pub fn stats(ts_ms: &[f64], now_ms: f64) -> Option<Stats> {
    if !now_ms.is_finite() {
        return None;
    }
    let mut ts: Vec<f64> = ts_ms.iter().copied().filter(|t| t.is_finite() && *t <= now_ms + 1.0).collect();
    ts.sort_by(f64::total_cmp);
    ts.dedup();
    let end = *ts.last()?;
    if now_ms - end > STALE_MS {
        return None;
    }
    // (present time, frame time = gap to the previous present); pauses are not frames.
    let frames: Vec<(f64, f64)> = ts
        .windows(2)
        .filter(|w| w[0] >= end - WINDOW_MS)
        .map(|w| (w[1], w[1] - w[0]))
        .filter(|&(_, d)| d > 0.0 && d <= PAUSE_MS)
        .collect();
    if frames.is_empty() {
        return None;
    }
    let mean = |v: &[f64]| if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 };
    let last_second: Vec<f64> = frames.iter().filter(|(t, _)| *t > end - 1000.0).map(|&(_, d)| d).collect();
    let frame_ms = mean(&last_second);
    let mut all: Vec<f64> = frames.iter().map(|&(_, d)| d).collect();
    all.sort_by(f64::total_cmp);
    let n = all.len();
    let slowest = mean(&all[n - n.div_ceil(100)..]);
    Some(Stats {
        fps: if frame_ms > 0.0 { 1000.0 / frame_ms } else { 0.0 },
        frame_ms,
        low1_fps: 1000.0 / slowest,
        p99_ms: all[(n * 99).div_ceil(100) - 1],
        frames: n as u32,
    })
}

/* ---------------- event classification + present bookkeeping (pure) ---------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// DXGI / D3D9 Present_Start on this swap chain.
    Runtime(u64),
    /// A step of a kernel present call (event id).
    Work(u16),
    /// DxgKrnl Present_Info: the kernel present call on this thread returned.
    End,
}

/// What an event means for the counter. `payload` is the event's user data; `ptr32` = the event
/// came from a 32-bit process (pointers are 4 bytes). Unknown events → `None`.
pub(crate) fn kind(provider: &GUID, id: u16, payload: &[u8], ptr32: bool) -> Option<Kind> {
    if (*provider == DXGI && DXGI_PRESENTS.contains(&id)) || (*provider == D3D9 && D3D9_PRESENTS.contains(&id)) {
        let chain = if ptr32 {
            payload.first_chunk::<4>().map(|b| u64::from(u32::from_le_bytes(*b)))
        } else {
            payload.first_chunk::<8>().map(|b| u64::from_le_bytes(*b))
        };
        return Some(Kind::Runtime(chain.unwrap_or(0)));
    }
    if *provider != DXGKRNL {
        return None;
    }
    if id == KERNEL_END { Some(Kind::End) } else { KERNEL_WORK.contains(&id).then_some(Kind::Work(id)) }
}

/// Presents of the target process (QPC ticks). Fed by the ETW callback; bounded.
#[derive(Default)]
pub(crate) struct Tracker {
    runtime: HashMap<u64, VecDeque<i64>>,
    kernel: VecDeque<i64>,
    /// Thread → id of the work event that opened its in-flight kernel present.
    open: HashMap<u32, u16>,
}

/// Sorted insert (events from different CPUs can arrive a little out of order), capped.
fn insert(q: &mut VecDeque<i64>, ts: i64) {
    let at = q.partition_point(|&t| t <= ts);
    q.insert(at, ts);
    while q.len() > MAX_FRAMES {
        q.pop_front();
    }
}

impl Tracker {
    pub(crate) fn push(&mut self, thread: u32, ts: i64, kind: Kind) {
        match kind {
            Kind::Runtime(chain) => {
                if !self.runtime.contains_key(&chain) && self.runtime.len() >= MAX_SWAPCHAINS {
                    // Forget the swap chain that presented longest ago.
                    let oldest = self.runtime.iter().min_by_key(|(_, q)| q.back().copied().unwrap_or(i64::MIN)).map(|(k, _)| *k);
                    if let Some(k) = oldest {
                        self.runtime.remove(&k);
                    }
                }
                insert(self.runtime.entry(chain).or_default(), ts);
            }
            Kind::Work(id) => match self.open.get(&thread) {
                Some(&first) if first != id => {} // a later step of the present already counted
                _ => {
                    if self.open.len() >= MAX_THREADS {
                        self.open.clear(); // threads that never closed: start over
                    }
                    self.open.insert(thread, id);
                    insert(&mut self.kernel, ts);
                }
            },
            Kind::End => {
                self.open.remove(&thread);
            }
        }
    }

    /// The game's presents since `since`: its busiest runtime swap chain, else the kernel presents.
    pub(crate) fn frames(&self, since: i64) -> Vec<i64> {
        let recent = |q: &VecDeque<i64>| q.len() - q.partition_point(|&t| t < since);
        let chosen = self
            .runtime
            .values()
            .filter(|q| recent(q) >= 2)
            .max_by_key(|q| recent(q))
            .or_else(|| (recent(&self.kernel) >= 2).then_some(&self.kernel));
        chosen.map(|q| q.iter().copied().filter(|&t| t >= since).collect()).unwrap_or_default()
    }
}

/* ---------------- live session ---------------- */

struct Shared {
    /// Process whose presents count (0 = none).
    target: AtomicU32,
    tracker: Mutex<Tracker>,
}

/// Live session. Dropping it stops the ETW session and joins the consumer thread.
pub struct Monitor {
    shared: Arc<Shared>,
    trace: u64,
    freq: i64,
    thread: Option<JoinHandle<()>>,
}

fn qpc() -> i64 {
    let mut v = 0i64;
    // SAFETY: plain out-param.
    let _ = unsafe { QueryPerformanceCounter(&mut v) };
    v
}

/// Stop a session left by a crash (it keeps the providers on with nobody reading).
pub fn stop_stale() {
    control_session(SESSION_NAME, EVENT_TRACE_CONTROL_STOP);
}

/// Enable `provider` on the session for `ids` only (EVENT_FILTER_EVENT_ID). Win32 code, 0 = ok.
fn enable(session: CONTROLTRACE_HANDLE, provider: &GUID, keywords: u64, ids: &[u16]) -> u32 {
    // EVENT_FILTER_EVENT_ID = FilterIn (BOOLEAN) + Reserved (u8) + Count (u16) + Events[Count] (u16):
    // as u16s, [1 (FilterIn = TRUE, Reserved = 0), count, ids...].
    let mut filter: Vec<u16> = vec![1, ids.len() as u16];
    filter.extend_from_slice(ids);
    let mut desc = EVENT_FILTER_DESCRIPTOR {
        Ptr: filter.as_ptr() as u64,
        Size: (filter.len() * 2) as u32,
        Type: EVENT_FILTER_TYPE_EVENT_ID,
    };
    let params = ENABLE_TRACE_PARAMETERS {
        Version: ENABLE_TRACE_PARAMETERS_VERSION_2,
        EnableFilterDesc: &mut desc,
        FilterDescCount: 1,
        ..Default::default()
    };
    // SAFETY: valid session; `filter`, `desc` and `params` outlive the call.
    unsafe {
        EnableTraceEx2(
            session,
            provider,
            EVENT_CONTROL_CODE_ENABLE_PROVIDER.0,
            TRACE_LEVEL_INFORMATION as u8,
            keywords,
            0,
            0,
            Some(&params),
        )
    }
    .0
}

impl Monitor {
    pub fn start() -> Result<Monitor, EtwError> {
        // An orphan from a crash keeps running; StartTraceW below reports the real outcome.
        stop_stale();
        let mut freq = 0i64;
        // SAFETY: plain out-param.
        let _ = unsafe { QueryPerformanceFrequency(&mut freq) };
        if freq <= 0 {
            return Err(EtwError::Api(0));
        }
        let cpus = std::thread::available_parallelism().map_or(8, |n| n.get()) as u32;
        let name = crate::registry::wide(SESSION_NAME);
        let mut buf = props();
        {
            // SAFETY: `props` returns a zeroed, 8-byte aligned buffer at least as large as
            // EVENT_TRACE_PROPERTIES; nothing else borrows it in this scope.
            let p = unsafe { &mut *buf.as_mut_ptr().cast::<EVENT_TRACE_PROPERTIES>() };
            p.LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
            p.FlushTimer = 1; // seconds; reads also flush
            p.BufferSize = 64; // KB
            p.MaximumBuffers = (2 * cpus + 16).min(512);
            p.LogFileNameOffset = 0; // real-time only, no file
        }
        let mut session = CONTROLTRACE_HANDLE::default();
        // SAFETY: `name` is NUL-terminated and outlives the call; `buf` is a sized
        // EVENT_TRACE_PROPERTIES with room for the logger name at LoggerNameOffset.
        let err = unsafe { StartTraceW(&mut session, PCWSTR(name.as_ptr()), buf.as_mut_ptr().cast()) }.0;
        if err != 0 {
            return Err(api(err));
        }
        let fail = |e: EtwError| {
            stop_stale();
            e
        };
        // DXGI is the one that matters (Direct3D 10/11/12); D3D9 and the kernel path are extras.
        let err = enable(session, &DXGI, KW_EVENTS, DXGI_PRESENTS);
        if err != 0 {
            return Err(fail(api(err)));
        }
        let _ = enable(session, &D3D9, KW_EVENTS, D3D9_PRESENTS);
        let _ = enable(session, &DXGKRNL, KW_PRESENT, KERNEL_EVENTS);

        let shared = Arc::new(Shared { target: AtomicU32::new(0), tracker: Mutex::default() });
        let ctx = Arc::clone(&shared);
        let (tx, rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("g2boost-fps".into())
            .spawn(move || consume(&ctx, &tx))
            .map_err(|e| fail(EtwError::Api(e.raw_os_error().unwrap_or(0) as u32)))?;
        match rx.recv() {
            Ok(Ok(trace)) => Ok(Monitor { shared, trace, freq, thread: Some(thread) }),
            Ok(Err(code)) => {
                let _ = thread.join();
                Err(fail(api(code)))
            }
            Err(_) => {
                let _ = thread.join();
                Err(fail(EtwError::Api(0)))
            }
        }
    }

    /// Count the presents of `pid` from now on (a new target starts from zero).
    pub fn set_target(&self, pid: u32) {
        if self.shared.target.swap(pid, Ordering::SeqCst) != pid {
            *self.shared.tracker.lock().unwrap_or_else(PoisonError::into_inner) = Tracker::default();
        }
    }

    /// Statistics of the target right now (`None` while it is not drawing).
    pub fn stats(&self) -> Option<Stats> {
        // Hand partly filled buffers to the consumer: the overlay never waits for the flush timer.
        control_session(SESSION_NAME, EVENT_TRACE_CONTROL_FLUSH);
        let now = qpc();
        let since = now - self.freq * 11; // the 10 s window ends at the newest present
        let ts = self.shared.tracker.lock().unwrap_or_else(PoisonError::into_inner).frames(since);
        let ms: Vec<f64> = ts.iter().map(|&t| (t - now) as f64 * 1000.0 / self.freq as f64).collect();
        stats(&ms, 0.0)
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        stop_stale();
        // SAFETY: `trace` came from a successful OpenTraceW and is closed exactly once, here.
        // CloseTrace from another thread is the documented way to make ProcessTrace return.
        unsafe {
            let _ = CloseTrace(PROCESSTRACE_HANDLE { Value: self.trace });
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Consumer thread: open the real-time session, report the handle, pump events until closed.
fn consume(ctx: &Shared, tx: &SyncSender<Result<u64, u32>>) {
    let mut name = crate::registry::wide(SESSION_NAME);
    let mut log = EVENT_TRACE_LOGFILEW { LoggerName: PWSTR(name.as_mut_ptr()), ..Default::default() };
    log.Anonymous1.ProcessTraceMode = PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
    log.Anonymous2.EventRecordCallback = Some(on_event);
    log.Context = (ctx as *const Shared).cast_mut().cast::<c_void>();
    // SAFETY: `log` and `name` live on this stack frame until ProcessTrace returns.
    let trace = unsafe { OpenTraceW(&mut log) };
    if trace.Value == INVALID_TRACE {
        // SAFETY: plain thread-local error read right after the failing call.
        let _ = tx.send(Err(unsafe { GetLastError() }.0));
        return;
    }
    let _ = tx.send(Ok(trace.Value));
    // SAFETY: valid handle; blocks until Monitor::drop stops the session / closes the handle.
    let _ = unsafe { ProcessTrace(&[trace], None, None) };
}

/// EventRecordCallback. Tiny and panic-free: no unwrap, no indexing (a panic here would abort:
/// `extern "system"` does not unwind and release builds use panic = "abort").
extern "system" fn on_event(rec: *mut EVENT_RECORD) {
    // SAFETY: ETW passes a valid EVENT_RECORD (or null, handled) for the duration of the call.
    let Some(rec) = (unsafe { rec.as_ref() }) else { return };
    if rec.UserContext.is_null() {
        return;
    }
    // SAFETY: UserContext is the `Context` set in `consume`, which points at the Shared kept alive
    // by the ProcessTrace thread's Arc; callbacks only run inside that ProcessTrace call.
    let shared = unsafe { &*rec.UserContext.cast::<Shared>() };
    let h = &rec.EventHeader;
    let target = shared.target.load(Ordering::Relaxed);
    if target == 0 || h.ProcessId != target {
        return;
    }
    let data: &[u8] = if rec.UserData.is_null() {
        &[]
    } else {
        // SAFETY: ETW guarantees UserData points to UserDataLength readable bytes for this call.
        unsafe { std::slice::from_raw_parts(rec.UserData.cast::<u8>(), usize::from(rec.UserDataLength)) }
    };
    let ptr32 = u32::from(h.Flags) & EVENT_HEADER_FLAG_32_BIT_HEADER != 0;
    let Some(k) = kind(&h.ProviderId, h.EventDescriptor.Id, data, ptr32) else { return };
    shared.tracker.lock().unwrap_or_else(PoisonError::into_inner).push(h.ThreadId, h.TimeStamp, k);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` presents every `step` ms starting at `from`.
    fn steady(from: f64, step: f64, n: usize) -> Vec<f64> {
        (0..n).map(|i| from + i as f64 * step).collect()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 0.05
    }

    #[test]
    fn fps_stats_steady_60() {
        let ts = steady(0.0, 1000.0 / 60.0, 301); // 5 s
        let now = *ts.last().unwrap() + 5.0;
        let s = stats(&ts, now).unwrap();
        assert!(close(s.fps, 60.0), "{s:?}");
        assert!(close(s.frame_ms, 16.667), "{s:?}");
        assert!(close(s.low1_fps, 60.0) && close(s.p99_ms, 16.667), "{s:?}");
        assert_eq!(s.frames, 300);
    }

    #[test]
    fn fps_stats_hitches_drive_low_and_p99() {
        // 10 hitches of 50 ms, then 890 frames of 10 ms: 9.4 s in all.
        let mut ts = vec![0.0];
        for i in 0..900 {
            let gap = if i < 10 { 50.0 } else { 10.0 };
            ts.push(ts.last().unwrap() + gap);
        }
        let s = stats(&ts, *ts.last().unwrap()).unwrap();
        assert_eq!(s.frames, 900);
        // slowest 1 % = 9 frames, all hitches → 20 FPS; nearest rank 891 of 900 is the first 50 ms.
        assert!(close(s.low1_fps, 20.0), "{s:?}");
        assert!(close(s.p99_ms, 50.0), "{s:?}");
        // the last second is 100 frames of 10 ms: the average hides the hitches, the low shows them
        assert!(close(s.fps, 100.0) && close(s.frame_ms, 10.0), "{s:?}");
    }

    #[test]
    fn fps_stats_order_duplicates_and_garbage_do_not_matter() {
        let clean = steady(1000.0, 8.0, 200);
        let mut noisy = clean.clone();
        noisy.reverse();
        noisy.extend([f64::NAN, f64::INFINITY, f64::NEG_INFINITY, clean[10], clean[50], 1e12]);
        let now = *clean.last().unwrap();
        assert_eq!(stats(&noisy, now), stats(&clean, now), "unsorted, duplicated, NaN/inf, future");
    }

    #[test]
    fn fps_stats_none_when_not_drawing() {
        assert_eq!(stats(&[], 0.0), None);
        assert_eq!(stats(&[5.0], 10.0), None, "one present is no frame time");
        let ts = steady(0.0, 10.0, 100);
        assert!(stats(&ts, 990.0 + 1500.0).is_some());
        assert_eq!(stats(&ts, 990.0 + 2500.0), None, "nothing for 2.5 s: stalled");
        assert_eq!(stats(&ts, f64::NAN), None);
        assert_eq!(stats(&[0.0, 0.0, 0.0], 0.0), None, "identical stamps have no frame time");
    }

    #[test]
    fn fps_stats_pauses_are_not_frames_and_window_is_10s() {
        // 30 s at 100 FPS, a 5 s alt-tab, then 1 s at 50 FPS.
        let mut ts = steady(0.0, 10.0, 3000);
        let resume = ts.last().unwrap() + 5000.0;
        ts.extend(steady(resume, 20.0, 51));
        let s = stats(&ts, *ts.last().unwrap()).unwrap();
        assert!(close(s.fps, 50.0), "{s:?}");
        assert!(s.p99_ms <= 20.0 + 1e-9, "the 5 s gap is a pause, not a 5000 ms frame: {s:?}");
        // window = last 10 s of presents: 50 frames after the pause + the 10 ms frames inside it
        assert!(s.frames < 600, "old frames outside the window: {s:?}");
    }

    fn chain(c: u64) -> Vec<u8> {
        let mut p = c.to_le_bytes().to_vec();
        p.extend([0u8; 12]); // Flags, SyncInterval, ...
        p
    }

    #[test]
    fn fps_kind_maps_providers_and_ids() {
        assert_eq!(kind(&DXGI, 42, &chain(0xAB), false), Some(Kind::Runtime(0xAB)));
        assert_eq!(kind(&DXGI, 55, &chain(7), false), Some(Kind::Runtime(7)));
        assert_eq!(kind(&D3D9, 1, &chain(9), false), Some(Kind::Runtime(9)));
        // 32-bit game: 4-byte pointer
        assert_eq!(kind(&DXGI, 42, &[0x78, 0x56, 0x34, 0x12, 0xFF, 0xFF], true), Some(Kind::Runtime(0x1234_5678)));
        // short / empty payload still counts the frame (unknown chain)
        assert_eq!(kind(&DXGI, 42, &[1, 2], false), Some(Kind::Runtime(0)));
        assert_eq!(kind(&D3D9, 1, &[], true), Some(Kind::Runtime(0)));
        for id in [166, 168, 171, 215] {
            assert_eq!(kind(&DXGKRNL, id, &[], false), Some(Kind::Work(id)));
        }
        assert_eq!(kind(&DXGKRNL, 184, &[], false), Some(Kind::End));
        // stops, other events, ids from the wrong provider
        for (p, id) in [(DXGI, 43), (DXGI, 1), (D3D9, 2), (D3D9, 42), (DXGKRNL, 42), (DXGKRNL, 17), (DXGKRNL, 252)] {
            assert_eq!(kind(&p, id, &chain(1), false), None, "{id}");
        }
        assert_eq!(kind(&GUID::zeroed(), 42, &chain(1), false), None);
    }

    #[test]
    fn fps_tracker_picks_the_busiest_swap_chain() {
        let mut t = Tracker::default();
        for i in 0..120 {
            t.push(1, i * 10, Kind::Runtime(0xA)); // the game
        }
        for i in 0..3 {
            t.push(2, i * 300, Kind::Runtime(0xB)); // a launcher widget
        }
        for i in 0..500 {
            t.push(1, i * 2, Kind::Work(171)); // the same frames seen by the kernel
        }
        let f = t.frames(0);
        assert_eq!(f.len(), 120, "runtime presents win and the kernel ones are not added");
        assert!(f.windows(2).all(|w| w[0] <= w[1]));
        assert!(t.frames(10_000).is_empty(), "nothing since then");
    }

    #[test]
    fn fps_tracker_kernel_presents_open_close_and_repeat() {
        let mut t = Tracker::default();
        // with Present_Info closing each call: work steps of one call count once
        for i in 0..10 {
            let ts = i * 1000;
            t.push(5, ts, Kind::Work(171));
            t.push(5, ts + 1, Kind::Work(215));
            t.push(5, ts + 2, Kind::Work(166));
            t.push(5, ts + 3, Kind::End);
        }
        assert_eq!(t.frames(0).len(), 10);
        // without a close: the opening event showing up again is the next frame
        let mut t = Tracker::default();
        for i in 0..10 {
            t.push(6, i * 1000, Kind::Work(168));
            t.push(6, i * 1000 + 1, Kind::Work(171));
        }
        assert_eq!(t.frames(0).len(), 10);
        // two threads presenting at once: each call counts
        let mut t = Tracker::default();
        for i in 0..10 {
            t.push(7, i * 1000, Kind::Work(171));
            t.push(8, i * 1000 + 3, Kind::Work(171));
            t.push(7, i * 1000 + 5, Kind::End);
            t.push(8, i * 1000 + 6, Kind::End);
        }
        assert_eq!(t.frames(0).len(), 20);
    }

    #[test]
    fn fps_tracker_is_bounded_and_sorted() {
        let mut t = Tracker::default();
        for i in 0..(MAX_FRAMES as i64 + 500) {
            t.push(1, i, Kind::Runtime(1));
        }
        assert_eq!(t.frames(0).len(), MAX_FRAMES, "oldest presents dropped");
        for c in 0..100u64 {
            t.push(1, 1_000_000 + c as i64, Kind::Runtime(1000 + c));
        }
        assert!(t.runtime.len() <= MAX_SWAPCHAINS);
        for th in 0..1000u32 {
            t.push(th, 2_000_000, Kind::Work(171));
        }
        assert!(t.open.len() <= MAX_THREADS);
        // out-of-order arrival lands sorted
        let mut t = Tracker::default();
        for ts in [50, 10, 40, 20, 30, 30] {
            t.push(1, ts, Kind::Runtime(1));
        }
        assert_eq!(t.frames(0), vec![10, 20, 30, 30, 40, 50]);
    }

    #[test]
    fn fps_filter_layout() {
        // The EVENT_FILTER_EVENT_ID we build: FilterIn = 1, Reserved = 0, Count, ids (little-endian).
        let ids = [42u16, 55];
        let mut filter: Vec<u16> = vec![1, ids.len() as u16];
        filter.extend_from_slice(&ids);
        let bytes: Vec<u8> = filter.iter().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(bytes, vec![1, 0, 2, 0, 42, 0, 55, 0]);
        assert_eq!(std::mem::size_of::<windows::Win32::System::Diagnostics::Etw::EVENT_FILTER_EVENT_ID>(), 6);
    }

    fn run_live(log: &mut Vec<String>) -> Result<(), String> {
        let mon = match Monitor::start() {
            Err(EtwError::NeedsAdmin) => {
                log.push("start: needs_admin (not elevated); elevated path not exercised".into());
                return Ok(());
            }
            Err(e) => return Err(format!("start failed: {e}")),
            Ok(m) => m,
        };
        log.push("start: ok".into());
        let pid = std::env::var("G2BOOST_FPS_PID").ok().and_then(|v| v.trim().parse::<u32>().ok());
        if let Some(pid) = pid {
            mon.set_target(pid);
            for second in 1..=5 {
                std::thread::sleep(std::time::Duration::from_secs(1));
                log.push(format!("t+{second}s pid {pid}: {:?}", mon.stats()));
            }
        } else {
            log.push("G2BOOST_FPS_PID not set: session start/stop only".into());
        }
        let last = pid.and_then(|_| mon.stats());
        drop(mon);
        let query = control_session(SESSION_NAME, windows::Win32::System::Diagnostics::Etw::EVENT_TRACE_CONTROL_QUERY);
        log.push(format!("after drop, query session -> {query} (4201 = gone)"));
        let again = Monitor::start();
        log.push(format!("second start: {:?}", again.as_ref().map(|_| "ok")));
        let again_ok = again.is_ok();
        drop(again);
        if !again_ok {
            return Err("second start failed".into());
        }
        if query != 4201 {
            return Err(format!("session still present after drop ({query})"));
        }
        if pid.is_some() && !last.is_some_and(|s| s.fps > 0.0) {
            return Err("no frames counted for the target".into());
        }
        Ok(())
    }

    /// Live check of the ETW path. Needs administrator and something drawing:
    /// 1. open a game (windowed or borderless) or any 3D app and note its PID (Task Manager ›
    ///    Details);
    /// 2. in an elevated terminal at the repo root:
    ///    `$env:G2BOOST_FPS_PID = <pid>; cargo test -p g2boost-core -- --ignored fps_live --nocapture`
    ///
    /// Prints five seconds of stats, then checks the session is gone after drop and starts again.
    /// Without the variable it only exercises start → stop → restart. Log: `%TEMP%\g2boost-live\fps.txt`.
    #[test]
    #[ignore = "live ETW; run elevated with a game open (see the doc comment)"]
    fn fps_live() {
        let mut log = Vec::new();
        let result = run_live(&mut log);
        log.push(format!("result: {result:?}"));
        let dir = std::env::temp_dir().join("g2boost-live");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("fps.txt"), log.join("\n")).unwrap();
        println!("{}", log.join("\n"));
        result.unwrap();
    }
}
