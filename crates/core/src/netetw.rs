//! Bytes per process (download/upload) for the "Usage by app" network screen.
//!
//! Source: a real-time ETW session on the manifest provider **Microsoft-Windows-Kernel-Network**
//! ({7DD42A49-5329-4832-8DFD-43D979153A88}), keywords `KERNEL_NETWORK_KEYWORD_IPV4` (0x10) and
//! `KERNEL_NETWORK_KEYWORD_IPV6` (0x20), level Informational. Native API only (StartTraceW,
//! EnableTraceEx2, OpenTraceW, ProcessTrace); no logman/xperf/powershell.
//!
//! Event ids and payload layout come from the provider manifest registered on Windows 11 26200
//! (`Get-WinEvent -ListProvider Microsoft-Windows-Kernel-Network`, read at dev time):
//!
//! | id | task / opcode     | keyword |
//! |----|-------------------|---------|
//! | 10 | TCPIP / SEND      | IPV4    |
//! | 11 | TCPIP / RECV      | IPV4    |
//! | 26 | TCPIP / SEND      | IPV6    |
//! | 27 | TCPIP / RECV      | IPV6    |
//! | 42 | UDPIP / SENDUDP   | IPV4    |
//! | 43 | UDPIP / RECVUDP   | IPV4    |
//! | 58 | UDPIP / SENDUDP   | IPV6    |
//! | 59 | UDPIP / RECVUDP   | IPV6    |
//!
//! Every template (version 0) starts with `PID: win:UInt32` then `size: win:UInt32`, so the
//! parser reads two little-endian u32 at offsets 0 and 4. The payload PID is the owning process
//! (the header ProcessId is whatever thread ran the stack, often System on receive).
//!
//! Needs admin (or membership in Performance Log Users): `StartTraceW` returns
//! ERROR_ACCESS_DENIED otherwise, surfaced as [`EtwError::NeedsAdmin`].
//!
//! Counters are reached through `EVENT_TRACE_LOGFILEW.Context` (delivered as
//! `EVENT_RECORD.UserContext`), not a global: the pointer targets an `Arc` clone owned by the
//! ProcessTrace thread, so it outlives every callback, and two monitors never share counts.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

use serde::Serialize;
use windows::Win32::Foundation::GetLastError;
use windows::Win32::System::Diagnostics::Etw::{
    CONTROLTRACE_HANDLE, CloseTrace, ControlTraceW, EVENT_CONTROL_CODE_ENABLE_PROVIDER, EVENT_RECORD,
    EVENT_TRACE_CONTROL, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES,
    EVENT_TRACE_REAL_TIME_MODE, EnableTraceEx2, OpenTraceW, PROCESS_TRACE_MODE_EVENT_RECORD,
    PROCESS_TRACE_MODE_REAL_TIME, PROCESSTRACE_HANDLE, ProcessTrace, StartTraceW, TRACE_LEVEL_INFORMATION,
    WNODE_FLAG_TRACED_GUID,
};
use windows::core::{GUID, PCWSTR, PWSTR};

/// Session name. Constant so an orphan left by a crash is found and stopped on the next start.
pub const SESSION_NAME: &str = "G2Boost-NetUsage";
const PROVIDER: GUID = GUID::from_u128(0x7dd42a49_5329_4832_8dfd_43d979153a88);
const KEYWORDS: u64 = 0x10 | 0x20; // KERNEL_NETWORK_KEYWORD_IPV4 | KERNEL_NETWORK_KEYWORD_IPV6
/// Wide chars reserved after EVENT_TRACE_PROPERTIES for each name ETW writes back.
const NAME_ROOM: usize = 1024;
/// INVALID_PROCESSTRACE_HANDLE on 64-bit.
const INVALID_TRACE: u64 = u64::MAX;
const ERROR_ACCESS_DENIED: u32 = 5;

/// pid -> (bytes sent, bytes received), cumulative since start.
// ponytail: one mutex for all events; shard per CPU if the event rate ever shows up in a profile.
type Counters = Mutex<HashMap<u32, (u64, u64)>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum EtwError {
    /// Stable code: the UI maps it to "needs administrator".
    #[error("needs_admin")]
    NeedsAdmin,
    /// Raw Win32 error from the ETW API.
    #[error("etw error {0}")]
    Api(u32),
}

fn api(code: u32) -> EtwError {
    if code == ERROR_ACCESS_DENIED { EtwError::NeedsAdmin } else { EtwError::Api(code) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Sent,
    Received,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ProcBytes {
    pub pid: u32,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ProcRate {
    pub pid: u32,
    pub down_bytes_per_sec: f64,
    pub up_bytes_per_sec: f64,
}

/// Payload parser (pure). `None` for events we do not count or payloads shorter than 8 bytes.
pub fn parse(event_id: u16, user_data: &[u8]) -> Option<(u32, u32, Direction)> {
    let dir = match event_id {
        10 | 26 | 42 | 58 => Direction::Sent,
        11 | 27 | 43 | 59 => Direction::Received,
        _ => return None,
    };
    let (pid, rest) = user_data.split_first_chunk::<4>()?;
    let (size, _) = rest.split_first_chunk::<4>()?;
    Some((u32::from_le_bytes(*pid), u32::from_le_bytes(*size), dir))
}

/// Per-pid bytes/s between two snapshots. Pids missing from `now` drop out, pids missing from
/// `prev` count from zero, a counter that went backwards (restart) clamps to 0.
pub fn rates(prev: &[ProcBytes], now: &[ProcBytes], secs: f64) -> Vec<ProcRate> {
    let before: HashMap<u32, (u64, u64)> = prev.iter().map(|p| (p.pid, (p.bytes_sent, p.bytes_received))).collect();
    let per_sec = |now: u64, then: u64| if secs > 0.0 { now.saturating_sub(then) as f64 / secs } else { 0.0 };
    now.iter()
        .map(|n| {
            let (sent, recv) = before.get(&n.pid).copied().unwrap_or_default();
            ProcRate {
                pid: n.pid,
                down_bytes_per_sec: per_sec(n.bytes_received, recv),
                up_bytes_per_sec: per_sec(n.bytes_sent, sent),
            }
        })
        .collect()
}

/// Live session. Dropping it stops the ETW session and joins the consumer thread.
pub struct Monitor {
    counters: Arc<Counters>,
    trace: u64,
    thread: Option<JoinHandle<()>>,
}

impl Monitor {
    pub fn start() -> Result<Monitor, EtwError> {
        // An orphan from a crash keeps running; not-found / access-denied here is fine,
        // StartTraceW below reports the real outcome.
        control(EVENT_TRACE_CONTROL_STOP);

        let name = crate::registry::wide(SESSION_NAME);
        let mut buf = props();
        {
            // SAFETY: `props` returns a zeroed, 8-byte aligned buffer at least as large as
            // EVENT_TRACE_PROPERTIES; nothing else borrows it in this scope.
            let p = unsafe { &mut *buf.as_mut_ptr().cast::<EVENT_TRACE_PROPERTIES>() };
            p.LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
            p.FlushTimer = 1; // seconds: events reach the UI within ~1 s
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
            control(EVENT_TRACE_CONTROL_STOP);
            e
        };

        // SAFETY: `session` came from a successful StartTraceW; PROVIDER is a valid GUID.
        let err = unsafe {
            EnableTraceEx2(
                session,
                &PROVIDER,
                EVENT_CONTROL_CODE_ENABLE_PROVIDER.0,
                TRACE_LEVEL_INFORMATION as u8,
                KEYWORDS,
                0,
                0,
                None,
            )
        }
        .0;
        if err != 0 {
            return Err(fail(api(err)));
        }

        let counters: Arc<Counters> = Arc::default();
        let ctx = Arc::clone(&counters);
        let (tx, rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("g2boost-netetw".into())
            .spawn(move || consume(&ctx, &tx))
            .map_err(|e| fail(EtwError::Api(e.raw_os_error().unwrap_or(0) as u32)))?;
        match rx.recv() {
            Ok(Ok(trace)) => Ok(Monitor { counters, trace, thread: Some(thread) }),
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

    /// Cumulative bytes per pid since `start`.
    // ponytail: dead pids stay in the map (20 bytes each); prune if sessions ever run for weeks.
    pub fn snapshot(&self) -> Vec<ProcBytes> {
        let map = self.counters.lock().unwrap_or_else(PoisonError::into_inner);
        map.iter()
            .map(|(&pid, &(bytes_sent, bytes_received))| ProcBytes { pid, bytes_sent, bytes_received })
            .collect()
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        control(EVENT_TRACE_CONTROL_STOP);
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
fn consume(ctx: &Counters, tx: &SyncSender<Result<u64, u32>>) {
    let mut name = crate::registry::wide(SESSION_NAME);
    let mut log = EVENT_TRACE_LOGFILEW { LoggerName: PWSTR(name.as_mut_ptr()), ..Default::default() };
    log.Anonymous1.ProcessTraceMode = PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
    log.Anonymous2.EventRecordCallback = Some(on_event);
    log.Context = (ctx as *const Counters).cast_mut().cast::<c_void>();
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

/// EventRecordCallback. Kept tiny and panic-free: no unwrap, no indexing, saturating math.
/// (A panic here would abort, not unwind: `extern "system"` aborts on unwind and release
/// builds use panic = "abort".)
extern "system" fn on_event(rec: *mut EVENT_RECORD) {
    // SAFETY: ETW passes a valid EVENT_RECORD (or null, handled) for the duration of the call.
    let Some(rec) = (unsafe { rec.as_ref() }) else { return };
    if rec.EventHeader.ProviderId != PROVIDER || rec.UserContext.is_null() || rec.UserData.is_null() {
        return;
    }
    // SAFETY: ETW guarantees UserData points to UserDataLength readable bytes for this call.
    let data = unsafe { std::slice::from_raw_parts(rec.UserData.cast::<u8>(), usize::from(rec.UserDataLength)) };
    let Some((pid, size, dir)) = parse(rec.EventHeader.EventDescriptor.Id, data) else { return };
    // SAFETY: UserContext is the `Context` set in `consume`, which points at the Counters kept
    // alive by the ProcessTrace thread's Arc; callbacks only run inside that ProcessTrace call.
    let counters = unsafe { &*rec.UserContext.cast::<Counters>() };
    let mut map = counters.lock().unwrap_or_else(PoisonError::into_inner);
    let entry = map.entry(pid).or_default();
    match dir {
        Direction::Sent => entry.0 = entry.0.saturating_add(u64::from(size)),
        Direction::Received => entry.1 = entry.1.saturating_add(u64::from(size)),
    }
}

/// Zeroed EVENT_TRACE_PROPERTIES followed by room for the logger and log file names.
/// `Vec<u64>` keeps the 8-byte alignment the struct needs.
fn props() -> Vec<u64> {
    let head = size_of::<EVENT_TRACE_PROPERTIES>();
    let mut buf = vec![0u64; (head + 2 * NAME_ROOM * 2).div_ceil(8)];
    let bytes = buf.len() * 8;
    // SAFETY: buf is zeroed, 8-byte aligned and larger than EVENT_TRACE_PROPERTIES.
    let p = unsafe { &mut *buf.as_mut_ptr().cast::<EVENT_TRACE_PROPERTIES>() };
    p.Wnode.BufferSize = bytes as u32;
    p.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
    p.Wnode.ClientContext = 1; // QPC timestamps
    p.LoggerNameOffset = head as u32;
    p.LogFileNameOffset = (head + NAME_ROOM * 2) as u32;
    buf
}

/// ControlTraceW on our session by name. Returns the Win32 code (0 = ok,
/// 4201 = ERROR_WMI_INSTANCE_NOT_FOUND when no such session).
fn control(code: EVENT_TRACE_CONTROL) -> u32 {
    let name = crate::registry::wide(SESSION_NAME);
    let mut buf = props();
    // SAFETY: `name` is NUL-terminated; `buf` is a sized EVENT_TRACE_PROPERTIES with room for
    // both names ETW writes back on stop/query.
    unsafe {
        ControlTraceW(CONTROLTRACE_HANDLE::default(), PCWSTR(name.as_ptr()), buf.as_mut_ptr().cast(), code)
    }
    .0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pb(pid: u32, bytes_sent: u64, bytes_received: u64) -> ProcBytes {
        ProcBytes { pid, bytes_sent, bytes_received }
    }

    fn payload(pid: u32, size: u32) -> Vec<u8> {
        let mut v = pid.to_le_bytes().to_vec();
        v.extend_from_slice(&size.to_le_bytes());
        v.extend_from_slice(&[0xAA; 16]); // addresses/ports we ignore
        v
    }

    #[test]
    fn netetw_parse_known_ids() {
        let d = payload(1234, 1500);
        for id in [10, 26, 42, 58] {
            assert_eq!(parse(id, &d), Some((1234, 1500, Direction::Sent)), "id {id}");
        }
        for id in [11, 27, 43, 59] {
            assert_eq!(parse(id, &d), Some((1234, 1500, Direction::Received)), "id {id}");
        }
        // exactly 8 bytes is enough
        assert_eq!(parse(10, &d[..8]), Some((1234, 1500, Direction::Sent)));
    }

    #[test]
    fn netetw_parse_rejects_short_and_unknown() {
        let d = payload(1, 2);
        for len in 0..8 {
            assert_eq!(parse(10, &d[..len]), None, "len {len}");
        }
        for id in [0, 12, 13, 15, 18, 28, 44, 49, 60, u16::MAX] {
            assert_eq!(parse(id, &d), None, "id {id}");
        }
    }

    #[test]
    fn netetw_rates() {
        let prev = [pb(1, 1000, 2000), pb(2, 500, 500), pb(3, 900, 900)];
        let now = [pb(1, 3000, 6000), pb(3, 100, 50), pb(4, 400, 800)];
        let r = rates(&prev, &now, 2.0);
        assert_eq!(r.len(), 3, "pid 2 disappeared and drops out");
        let get = |pid| r.iter().find(|x| x.pid == pid).copied().unwrap();
        assert_eq!((get(1).up_bytes_per_sec, get(1).down_bytes_per_sec), (1000.0, 2000.0));
        assert_eq!((get(3).up_bytes_per_sec, get(3).down_bytes_per_sec), (0.0, 0.0), "reset clamps");
        assert_eq!((get(4).up_bytes_per_sec, get(4).down_bytes_per_sec), (200.0, 400.0), "new pid from 0");
        // degenerate interval never divides by zero / NaN
        for secs in [0.0, -1.0, f64::NAN] {
            assert!(rates(&prev, &now, secs).iter().all(|x| x.up_bytes_per_sec == 0.0 && x.down_bytes_per_sec == 0.0));
        }
    }

    #[test]
    fn netetw_props_layout() {
        let buf = props();
        // SAFETY: same buffer contract as in production code.
        let p = unsafe { &*buf.as_ptr().cast::<EVENT_TRACE_PROPERTIES>() };
        assert_eq!(p.Wnode.BufferSize as usize, buf.len() * 8);
        assert!(p.LoggerNameOffset as usize + NAME_ROOM * 2 <= p.Wnode.BufferSize as usize);
        assert!(p.LogFileNameOffset as usize + NAME_ROOM * 2 <= p.Wnode.BufferSize as usize);
    }

    /// Generates a little TCP (HTTP to 1.1.1.1:80) and UDP (DNS query to 1.1.1.1:53) traffic.
    fn traffic(log: &mut Vec<String>) {
        use std::io::{Read, Write};
        use std::net::{TcpStream, UdpSocket};
        use std::time::Duration;
        let t = Duration::from_secs(3);
        for _ in 0..3 {
            match TcpStream::connect_timeout(&"1.1.1.1:80".parse().unwrap(), t) {
                Ok(mut s) => {
                    let _ = s.set_read_timeout(Some(t));
                    let _ = s.write_all(b"GET / HTTP/1.1\r\nHost: one.one.one.one\r\nConnection: close\r\n\r\n");
                    let mut b = [0u8; 4096];
                    let n = s.read(&mut b).unwrap_or(0);
                    log.push(format!("tcp: read {n} bytes"));
                }
                Err(e) => log.push(format!("tcp: connect failed: {e}")),
            }
        }
        let query: Vec<u8> = [0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]
            .into_iter()
            .chain(*b"\x07example\x03com\x00")
            .chain([0, 1, 0, 1])
            .collect();
        if let Ok(u) = UdpSocket::bind("0.0.0.0:0") {
            let _ = u.set_read_timeout(Some(t));
            let sent = u.send_to(&query, "1.1.1.1:53");
            let mut b = [0u8; 512];
            let got = u.recv_from(&mut b).map(|(n, _)| n);
            log.push(format!("udp: sent {sent:?}, received {got:?}"));
        }
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
        traffic(log);
        std::thread::sleep(std::time::Duration::from_millis(2500));
        let snap = mon.snapshot();
        let me = std::process::id();
        let mine = snap.iter().find(|p| p.pid == me).copied();
        let mut top = snap.clone();
        top.sort_by_key(|p| std::cmp::Reverse(p.bytes_sent + p.bytes_received));
        log.push(format!("pids seen: {}; own pid {me}: {mine:?}", snap.len()));
        log.push(format!("top 5: {:?}", &top[..top.len().min(5)]));
        drop(mon);
        let query = control(windows::Win32::System::Diagnostics::Etw::EVENT_TRACE_CONTROL_QUERY);
        log.push(format!("after drop, query session -> {query} (4201 = gone)"));
        let again = Monitor::start();
        log.push(format!("second start: {:?}", again.as_ref().map(|_| "ok")));
        let again_ok = again.is_ok();
        drop(again);
        if !again_ok {
            return Err("second start failed".into());
        }
        match mine {
            Some(p) if p.bytes_sent > 0 => {}
            _ => return Err("own pid has no sent bytes".into()),
        }
        if query != 4201 {
            return Err(format!("session still present after drop ({query})"));
        }
        Ok(())
    }

    #[test]
    #[ignore = "live ETW + network; run elevated: cargo test -p g2boost-core -- --ignored netetw_live"]
    fn netetw_live() {
        let mut log = Vec::new();
        let result = run_live(&mut log);
        log.push(format!("result: {result:?}"));
        let dir = std::env::temp_dir().join("g2boost-live");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("netetw.txt"), log.join("\n")).unwrap();
        println!("{}", log.join("\n"));
        result.unwrap();
    }
}
