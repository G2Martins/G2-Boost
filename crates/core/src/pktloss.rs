//! Packet-loss burst: a fixed schedule of ICMP echoes, one probe per short-lived thread so a
//! slow or lost reply never delays the next send, then classified as ok / late / lost and
//! "out of order". Native ICMP only (`network::ping`, no `ping.exe`). Every input is hostile:
//! the target is validated here and the profile is a closed enum (nobody picks a packet rate).
//! Docs/core/tools.md.

use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::network::{self, Probe};

/// A reply slower than this counts as lost: a game gave up long before.
pub const TIMEOUT_MS: u32 = 1000;
/// Absolute "late" line: a reply slower than this is late whatever the median says.
pub const LATE_MS: f64 = 150.0;
/// With fewer replies than this the relative (2x median) rule is not applied yet.
const MIN_FOR_MEDIAN: usize = 5;
/// Replies this close are the same instant: the API rounds to whole milliseconds, and a hiccup on
/// the path lets a whole queue go at once. Only a clearly earlier reply to a later packet counts.
const REORDER_TOL_MS: f64 = 2.0;

/* ---------------- profiles ---------------- */

/// How long and how fast to probe. A closed set: the caller never picks a packet rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    Quick,
    Standard,
    Long,
}

impl Profile {
    /// Packets per second.
    pub const fn rate(self) -> u32 {
        match self {
            Profile::Quick | Profile::Long => 10,
            Profile::Standard => 15,
        }
    }

    pub const fn seconds(self) -> u32 {
        match self {
            Profile::Quick => 10,
            Profile::Standard => 20,
            Profile::Long => 60,
        }
    }

    pub const fn count(self) -> u32 {
        self.rate() * self.seconds()
    }

    fn interval(self) -> Duration {
        Duration::from_micros(1_000_000 / u64::from(self.rate()))
    }
}

/* ---------------- target ---------------- */

/// Error codes (the `Display` text) go to the UI as-is.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TargetError {
    #[error("empty")]
    Empty,
    #[error("too_long")]
    TooLong,
    #[error("invalid")]
    Invalid,
    #[error("not_allowed")]
    NotAllowed,
    #[error("resolve")]
    Resolve,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub ip: Ipv4Addr,
}

enum Syntax {
    Ip(Ipv4Addr),
    Name(String),
}

/// Shape check only (no DNS): a strict dotted IPv4 or an ASCII host name. No ports, URLs, IPv6,
/// spaces, control or lookalike characters, and no old IPv4 spellings (`127.1`, `2130706433`):
/// the last label of a name always has a letter (RFC 1123 section 2.1).
fn syntax(input: &str) -> Result<Syntax, TargetError> {
    let s = input.trim();
    if s.is_empty() {
        return Err(TargetError::Empty);
    }
    if s.len() > 253 {
        return Err(TargetError::TooLong);
    }
    if !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-') {
        return Err(TargetError::Invalid);
    }
    if let Ok(ip) = s.parse::<Ipv4Addr>() {
        return Ok(Syntax::Ip(ip));
    }
    let name = s.strip_suffix('.').unwrap_or(s);
    let labels: Vec<&str> = name.split('.').collect();
    let label_ok = |l: &&str| (1..=63).contains(&l.len()) && !l.starts_with('-') && !l.ends_with('-');
    let last_has_letter = labels.last().is_some_and(|l| l.bytes().any(|b| b.is_ascii_alphabetic()));
    if !labels.iter().all(label_ok) || !last_has_letter {
        return Err(TargetError::Invalid);
    }
    Ok(Syntax::Name(name.to_string()))
}

/// Never probe "this network" (0/8), multicast (224/4) or the reserved block with the
/// broadcast address (240/4): one echo there would answer from many hosts.
fn ip_allowed(ip: Ipv4Addr) -> bool {
    ip.octets()[0] != 0 && ip.octets()[0] < 224
}

/// Validate what the caller typed and resolve it (IPv4). The resolved address is checked too:
/// a name can point anywhere.
pub fn parse_target(input: &str) -> Result<Target, TargetError> {
    let (host, ip) = match syntax(input)? {
        Syntax::Ip(ip) => (ip.to_string(), ip),
        Syntax::Name(name) => {
            let ip = network::resolve_v4(&name).map_err(|_| TargetError::Resolve)?;
            (name, ip)
        }
    };
    if !ip_allowed(ip) {
        return Err(TargetError::NotAllowed);
    }
    Ok(Target { host, ip })
}

/* ---------------- classification ---------------- */

/// One finished probe. `at_ms` is when its outcome came in, in milliseconds since the run began:
/// the moment it was sent plus the reply time (or the timeout, for a lost one). It is computed from
/// the API's own round trip, so thread wake-up delays on this PC cannot reorder anything.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Arrived {
    pub seq: u32,
    pub rtt_ms: Option<u32>,
    pub at_ms: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Sample {
    pub seq: u32,
    /// `None` = no reply within [`TIMEOUT_MS`].
    pub rtt_ms: Option<u32>,
    pub late: bool,
    /// A reply to a packet sent later came in clearly (more than 2 ms) before it.
    pub out_of_order: bool,
    /// Position in the order outcomes came in (0 = first).
    pub arrival: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Summary {
    /// Probes with an outcome (a reply or a timeout).
    pub sent: u32,
    pub received: u32,
    pub lost: u32,
    pub loss_pct: f64,
    pub late: u32,
    pub out_of_order: u32,
    pub avg_ms: Option<f64>,
    pub min_ms: Option<f64>,
    pub max_ms: Option<f64>,
    pub median_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    /// Mean difference between consecutive replies in send order (same as the network monitor).
    pub jitter_ms: Option<f64>,
    /// Most packets lost back to back (consecutive sequence numbers).
    pub longest_loss_run: u32,
}

fn median(sorted: &[f64]) -> Option<f64> {
    let n = sorted.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(sorted[n / 2]),
        _ => Some((sorted[n / 2 - 1] + sorted[n / 2]) / 2.0),
    }
}

/// Nearest-rank 95th percentile of an ascending slice.
fn p95(sorted: &[f64]) -> Option<f64> {
    let n = sorted.len();
    (n > 0).then(|| sorted[((n * 95).div_ceil(100)).clamp(1, n) - 1])
}

/// Late = over [`LATE_MS`], or over twice the median and at least 10 ms above it (the same
/// spike rule as the network monitor; without the 10 ms floor a 1 ms router would call 3 ms late).
pub fn is_late(rtt_ms: f64, median_ms: Option<f64>) -> bool {
    rtt_ms > LATE_MS || median_ms.is_some_and(|m| rtt_ms > 2.0 * m && rtt_ms > m + 10.0)
}

/// Flags per sample, with the final median, in the order outcomes came in (`at_ms`). A reply is out
/// of order when a reply to a packet sent *later* came in clearly before it (RFC 4737 reordering).
/// A lost packet is never out of order.
pub(crate) fn classify(arrived: &[Arrived]) -> Vec<Sample> {
    let mut rtts: Vec<f64> = arrived.iter().filter_map(|a| a.rtt_ms.map(f64::from)).collect();
    rtts.sort_by(f64::total_cmp);
    let med = if rtts.len() >= MIN_FOR_MEDIAN { median(&rtts) } else { None };

    let mut order: Vec<&Arrived> = arrived.iter().collect();
    order.sort_by(|a, b| a.at_ms.total_cmp(&b.at_ms).then(a.seq.cmp(&b.seq)));
    // `behind` walks after the current sample, a tolerance back; `highest` is the largest sequence
    // number among the replies it has passed.
    let mut behind = 0;
    let mut highest: Option<u32> = None;
    order
        .iter()
        .enumerate()
        .map(|(i, a)| {
            while behind < i && order[behind].at_ms < a.at_ms - REORDER_TOL_MS {
                if order[behind].rtt_ms.is_some() {
                    highest = Some(highest.map_or(order[behind].seq, |h| h.max(order[behind].seq)));
                }
                behind += 1;
            }
            Sample {
                seq: a.seq,
                rtt_ms: a.rtt_ms,
                late: a.rtt_ms.is_some_and(|r| is_late(f64::from(r), med)),
                out_of_order: a.rtt_ms.is_some() && highest.is_some_and(|h| a.seq < h),
                arrival: i as u32,
            }
        })
        .collect()
}

pub fn summarize(samples: &[Sample]) -> Summary {
    let mut by_seq: Vec<&Sample> = samples.iter().collect();
    by_seq.sort_by_key(|s| s.seq);
    let series: Vec<Option<f64>> = by_seq.iter().map(|s| s.rtt_ms.map(f64::from)).collect();
    let stats = network::latency_stats(&series);
    let mut replies: Vec<f64> = series.iter().flatten().copied().collect();
    replies.sort_by(f64::total_cmp);

    let (mut run, mut longest) = (0u32, 0u32);
    let mut last_lost: Option<u32> = None;
    for s in by_seq.iter().filter(|s| s.rtt_ms.is_none()) {
        run = if last_lost.and_then(|l| l.checked_add(1)) == Some(s.seq) { run + 1 } else { 1 };
        last_lost = Some(s.seq);
        longest = longest.max(run);
    }

    let finite = |v: f64| v.is_finite().then_some(v);
    let received = replies.len() as u32;
    Summary {
        sent: samples.len() as u32,
        received,
        lost: samples.len() as u32 - received,
        loss_pct: stats.loss_pct,
        late: samples.iter().filter(|s| s.late).count() as u32,
        out_of_order: samples.iter().filter(|s| s.out_of_order).count() as u32,
        avg_ms: finite(stats.avg_ms),
        min_ms: finite(stats.min_ms),
        max_ms: finite(stats.max_ms),
        median_ms: median(&replies),
        p95_ms: p95(&replies),
        jitter_ms: finite(stats.jitter_ms),
        longest_loss_run: longest,
    }
}

/* ---------------- the run ---------------- */

/// Emitted once per finished probe (`packetloss:probe`): the live view of the run.
#[derive(Debug, Clone, Serialize)]
pub struct ProbeEvent {
    pub seq: u32,
    pub rtt_ms: Option<u32>,
    pub late: bool,
    pub out_of_order: bool,
    /// Totals so far.
    pub summary: Summary,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub host: String,
    pub ip: Ipv4Addr,
    pub profile: Profile,
    /// Stopped by the user: only the probes that had finished are counted.
    pub cancelled: bool,
    /// Local wall clock at the start, `YYYY-MM-DD HH:MM:SS`.
    pub started: String,
    pub duration_ms: u64,
    /// By sequence number.
    pub samples: Vec<Sample>,
    pub summary: Summary,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunError {
    #[error("busy")]
    Busy,
    #[error("{0}")]
    Target(#[from] TargetError),
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);
static LAST: Mutex<Option<Report>> = Mutex::new(None);

/// One run at a time.
struct Slot;

impl Slot {
    fn take() -> Option<Slot> {
        RUNNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).ok().map(|_| Slot)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

/// Stop the run in progress (a no-op when none).
pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// The last finished run, for the CSV.
pub fn last() -> Option<Report> {
    LAST.lock().ok().and_then(|g| g.clone())
}

type ProbeFn = dyn Fn(u32) -> Option<u32> + Send + Sync;

/// The schedule. One thread sends `count` probes `interval` apart on absolute deadlines (so the
/// average rate holds even when a sleep overshoots); each probe runs on its own thread and its
/// outcome comes back over a channel. This thread collects the outcomes and reports each one.
/// On cancel it stops at once: probes still in flight are not counted (they end on their own).
pub(crate) fn run_with(
    count: u32,
    interval: Duration,
    probe: Arc<ProbeFn>,
    cancel: &'static AtomicBool,
    on_probe: &mut dyn FnMut(&ProbeEvent),
) -> (Vec<Sample>, bool) {
    let (tx, rx) = mpsc::channel::<(u32, Option<u32>, f64)>();
    let t0 = Instant::now();
    std::thread::spawn(move || {
        for seq in 0..count {
            let due = t0 + interval * seq;
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let now = Instant::now();
                if now >= due {
                    break;
                }
                std::thread::sleep((due - now).min(Duration::from_millis(50)));
            }
            let (worker_tx, worker_probe) = (tx.clone(), probe.clone());
            let started = std::thread::Builder::new()
                .name("pktloss-probe".into())
                .spawn(move || {
                    let sent_ms = t0.elapsed().as_secs_f64() * 1000.0;
                    let _ = worker_tx.send((seq, worker_probe(seq), sent_ms));
                });
            if started.is_err() {
                // No thread to probe with: count it as lost.
                let _ = tx.send((seq, None, t0.elapsed().as_secs_f64() * 1000.0));
            }
        }
    });

    // The last send, the longest wait for its reply, and some slack for a stuck worker.
    let give_up = t0 + interval * count + Duration::from_millis(u64::from(TIMEOUT_MS)) + Duration::from_secs(2);
    let mut arrived: Vec<Arrived> = Vec::with_capacity(count as usize);
    let mut seen = vec![false; count as usize];
    while arrived.len() < count as usize && !cancel.load(Ordering::Relaxed) && Instant::now() < give_up {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok((seq, rtt_ms, sent_ms)) => {
                // A hostile or buggy probe cannot report the same sequence twice or outside the run.
                if let Some(slot) = seen.get_mut(seq as usize).filter(|s| !**s) {
                    *slot = true;
                    let took = rtt_ms.map_or(f64::from(TIMEOUT_MS), f64::from);
                    arrived.push(Arrived { seq, rtt_ms, at_ms: sent_ms + took });
                    let flags = classify(&arrived);
                    if let Some(mine) = flags.iter().find(|f| f.seq == seq) {
                        on_probe(&ProbeEvent {
                            seq,
                            rtt_ms,
                            late: mine.late,
                            out_of_order: mine.out_of_order,
                            summary: summarize(&flags),
                        });
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let cancelled = cancel.load(Ordering::Relaxed) && arrived.len() < count as usize;
    let mut samples = classify(&arrived);
    samples.sort_by_key(|s| s.seq);
    (samples, cancelled)
}

/// Probe `host` with `profile`, calling `on_probe` for every finished probe. Blocks until the
/// run ends (10 to 60 s) or [`cancel`] is called.
pub fn run(host: &str, profile: Profile, on_probe: &mut dyn FnMut(&ProbeEvent)) -> Result<Report, RunError> {
    let _slot = Slot::take().ok_or(RunError::Busy)?;
    CANCEL.store(false, Ordering::SeqCst);
    let target = parse_target(host)?;
    let started = local_stamp();
    let t0 = Instant::now();
    let ip = target.ip;
    let probe: Arc<ProbeFn> = Arc::new(move |_seq: u32| match network::ping(ip, TIMEOUT_MS) {
        Ok(Probe::Reply { rtt_ms, .. }) => Some(rtt_ms),
        _ => None,
    });
    let (samples, cancelled) = run_with(profile.count(), profile.interval(), probe, &CANCEL, on_probe);
    let report = Report {
        host: target.host,
        ip,
        profile,
        cancelled,
        started,
        duration_ms: t0.elapsed().as_millis() as u64,
        summary: summarize(&samples),
        samples,
    };
    if let Ok(mut last) = LAST.lock() {
        *last = Some(report.clone());
    }
    Ok(report)
}

/* ---------------- CSV ---------------- */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsvFormat {
    sep: char,
    decimal: char,
}

impl CsvFormat {
    /// Brazilian spreadsheets want `;` and a decimal comma; everything else gets `,` and a dot.
    pub fn for_locale(locale: &str) -> Self {
        if locale == "pt-BR" {
            CsvFormat { sep: ';', decimal: ',' }
        } else {
            CsvFormat { sep: ',', decimal: '.' }
        }
    }
}

/// A text cell: control characters dropped, a leading `= + - @` defused (spreadsheets run those
/// as formulas), quoted when it holds the separator or a quote.
fn text_cell(s: &str, sep: char) -> String {
    let clean: String = s.chars().filter(|c| !c.is_control()).collect();
    let clean = if clean.starts_with(['=', '+', '-', '@']) { format!("'{clean}") } else { clean };
    if clean.contains([sep, '"']) || clean != clean.trim() {
        format!("\"{}\"", clean.replace('"', "\"\""))
    } else {
        clean
    }
}

fn number(v: Option<f64>, f: CsvFormat) -> String {
    v.map(|v| format!("{v:.1}").replace('.', &f.decimal.to_string())).unwrap_or_default()
}

pub fn csv(r: &Report, f: CsvFormat) -> String {
    let mut out = String::new();
    let mut row = |cells: &[String]| {
        out.push_str(&cells.join(&f.sep.to_string()));
        out.push_str("\r\n");
    };
    row(&["seq".into(), "result".into(), "rtt_ms".into(), "out_of_order".into(), "arrival".into()]);
    for s in &r.samples {
        let result = if s.rtt_ms.is_none() {
            "lost"
        } else if s.late {
            "late"
        } else {
            "ok"
        };
        row(&[
            s.seq.to_string(),
            result.into(),
            s.rtt_ms.map(|v| v.to_string()).unwrap_or_default(),
            u8::from(s.out_of_order).to_string(),
            s.arrival.to_string(),
        ]);
    }
    row(&[]);
    row(&["metric".into(), "value".into()]);
    let m = &r.summary;
    let profile = match r.profile {
        Profile::Quick => "quick",
        Profile::Standard => "standard",
        Profile::Long => "long",
    };
    let pairs: [(&str, String); 17] = [
        ("target", text_cell(&r.host, f.sep)),
        ("address", r.ip.to_string()),
        ("profile", profile.into()),
        ("started", text_cell(&r.started, f.sep)),
        ("duration_s", number(Some(r.duration_ms as f64 / 1000.0), f)),
        ("cancelled", u8::from(r.cancelled).to_string()),
        ("sent", m.sent.to_string()),
        ("received", m.received.to_string()),
        ("lost", m.lost.to_string()),
        ("loss_pct", number(Some(m.loss_pct), f)),
        ("late", m.late.to_string()),
        ("out_of_order", m.out_of_order.to_string()),
        ("avg_ms", number(m.avg_ms, f)),
        ("min_ms", number(m.min_ms, f)),
        ("p95_ms", number(m.p95_ms, f)),
        ("jitter_ms", number(m.jitter_ms, f)),
        ("longest_loss_run", m.longest_loss_run.to_string()),
    ];
    for (k, v) in pairs {
        row(&[k.into(), v]);
    }
    out
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("no_run")]
    NoRun,
    #[error("no_folder")]
    NoFolder,
    #[error("io")]
    Io,
}

/// A file name that can only be `G2Boost-perda-de-pacotes-<host>-<date>_<time>.csv`: the host
/// was validated, and anything else is turned into `_` anyway.
fn file_name(r: &Report) -> String {
    let host: String = r
        .host
        .chars()
        .take(40)
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect();
    let stamp = r.started.replace(' ', "_").replace(':', "-");
    format!("G2Boost-perda-de-pacotes-{host}-{stamp}.csv")
}

/// Write the CSV into `dir` without overwriting anything. Returns the path.
fn write_report(dir: &Path, r: &Report, f: CsvFormat) -> Result<PathBuf, ExportError> {
    use std::io::Write;
    let name = file_name(r);
    let stem = name.strip_suffix(".csv").unwrap_or(&name);
    let text = csv(r, f);
    for n in 0..10 {
        let candidate = if n == 0 { name.clone() } else { format!("{stem}-{n}.csv") };
        let path = dir.join(candidate);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(text.as_bytes()).map_err(|_| ExportError::Io)?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(ExportError::Io),
        }
    }
    Err(ExportError::Io)
}

/// The Documents folder (it may live in OneDrive).
fn documents_dir() -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Documents, KNOWN_FOLDER_FLAG, SHGetKnownFolderPath};
    // SAFETY: the API allocates the string; freed with CoTaskMemFree.
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Documents, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const std::ffi::c_void));
        s.map(PathBuf::from)
    }
}

/// Save the last run as CSV in Documents and show it in Explorer. The content is what this
/// module measured, never what the caller sends. Returns the file name.
pub fn export_csv(locale: &str) -> Result<String, ExportError> {
    let report = last().filter(|r| !r.samples.is_empty()).ok_or(ExportError::NoRun)?;
    let dir = documents_dir().ok_or(ExportError::NoFolder)?;
    let path = write_report(&dir, &report, CsvFormat::for_locale(locale))?;
    let _ = crate::disk::reveal(&path); // the file is saved either way
    Ok(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
}

/// Local wall clock, `YYYY-MM-DD HH:MM:SS`.
fn local_stamp() -> String {
    // SAFETY: GetLocalTime only fills the returned struct.
    let t = unsafe { GetLocalTime() };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `run()` shares global state (the slot and the cancel flag): tests that touch it take turns.
    static GLOBAL: Mutex<()> = Mutex::new(());

    /// `list` in the order the outcomes came in, ten milliseconds apart.
    fn arrived(list: &[(u32, Option<u32>)]) -> Vec<Arrived> {
        list.iter().enumerate().map(|(i, &(seq, rtt_ms))| Arrived { seq, rtt_ms, at_ms: i as f64 * 10.0 }).collect()
    }

    fn at(seq: u32, at_ms: f64) -> Arrived {
        Arrived { seq, rtt_ms: Some(5), at_ms }
    }

    fn reordered(list: &[Arrived]) -> Vec<u32> {
        let mut seqs: Vec<u32> = classify(list).iter().filter(|s| s.out_of_order).map(|s| s.seq).collect();
        seqs.sort_unstable();
        seqs
    }

    fn sum(list: &[(u32, Option<u32>)]) -> Summary {
        summarize(&classify(&arrived(list)))
    }

    #[test]
    fn profiles_match_the_documented_bursts() {
        assert_eq!((Profile::Quick.rate(), Profile::Quick.seconds(), Profile::Quick.count()), (10, 10, 100));
        assert_eq!((Profile::Standard.rate(), Profile::Standard.seconds(), Profile::Standard.count()), (15, 20, 300));
        assert_eq!((Profile::Long.rate(), Profile::Long.seconds(), Profile::Long.count()), (10, 60, 600));
        assert_eq!(Profile::Standard.interval(), Duration::from_micros(66_666));
        // Over IPC the profile is a word, never a number.
        assert_eq!(serde_json::from_str::<Profile>("\"standard\"").unwrap(), Profile::Standard);
        assert!(serde_json::from_str::<Profile>("\"turbo\"").is_err());
        assert!(serde_json::from_str::<Profile>("15").is_err());
    }

    #[test]
    fn target_syntax_refuses_hostile_strings() {
        let bad = [
            "1.1.1.1; calc",
            "1.1.1.1 -t",
            "-n 5 1.1.1.1",
            "http://1.1.1.1",
            "1.1.1.1:80",
            "1.1.1.1/24",
            "[::1]",
            "::1",
            "user@example.com",
            "example.com/path",
            "ex ample.com",
            "exa\0mple.com",
            "exa\nmple.com",
            "\u{202e}moc.elpmaxe",
            "ｅxample.com",
            "exämple.com",
            "-bad.com",
            "bad-.com",
            "a..b",
            ".a",
            "..",
            "1.2.3",
            "127.1",
            "2130706433",
            "256.1.1.1",
            "01.1.1.1",
            "1.1.1.1.",
            "1.2.3.4.5",
            "a_b.com",
            "*.com",
            "%41.com",
        ];
        for s in bad {
            assert!(
                matches!(syntax(s), Err(TargetError::Invalid)),
                "{s:?} should be invalid, got {:?}",
                syntax(s).err()
            );
        }
        assert_eq!(syntax("").err(), Some(TargetError::Empty));
        assert_eq!(syntax("   \t ").err(), Some(TargetError::Empty));
        assert_eq!(syntax(&"a".repeat(254)).err(), Some(TargetError::TooLong));
        assert_eq!(syntax(&format!("{}.com", "a".repeat(64))).err(), Some(TargetError::Invalid), "label over 63");
        assert!(syntax(&format!("{}.com", "a".repeat(63))).is_ok());
    }

    #[test]
    fn target_syntax_accepts_ips_and_names() {
        assert!(matches!(syntax("1.1.1.1"), Ok(Syntax::Ip(ip)) if ip == Ipv4Addr::new(1, 1, 1, 1)));
        assert!(matches!(syntax("  8.8.8.8\t"), Ok(Syntax::Ip(_))), "surrounding space is trimmed");
        assert!(matches!(syntax("192.168.0.1"), Ok(Syntax::Ip(_))));
        for name in ["localhost", "Router", "cloudflare.com", "a-b.example.com.br", "xn--bcher-kva.example", "x1.y2.io"] {
            assert!(matches!(syntax(name), Ok(Syntax::Name(_))), "{name}");
        }
        assert!(matches!(syntax("example.com."), Ok(Syntax::Name(n)) if n == "example.com"), "trailing root dot");
    }

    #[test]
    fn target_ips_that_would_answer_from_many_hosts_are_refused() {
        for ip in ["0.0.0.0", "0.1.2.3", "224.0.0.1", "239.255.255.250", "240.0.0.1", "255.255.255.255"] {
            assert_eq!(parse_target(ip).err(), Some(TargetError::NotAllowed), "{ip}");
        }
        for ip in ["127.0.0.1", "10.0.0.1", "192.168.1.1", "169.254.1.1", "100.64.0.1", "1.1.1.1", "223.255.255.254"] {
            assert!(ip_allowed(ip.parse().unwrap()), "{ip}");
        }
        let t = parse_target(" 8.8.8.8 ").unwrap();
        assert_eq!((t.host.as_str(), t.ip), ("8.8.8.8", Ipv4Addr::new(8, 8, 8, 8)));
        // Syntax errors win before any DNS lookup.
        assert_eq!(parse_target("http://x").err(), Some(TargetError::Invalid));
    }

    #[test]
    fn target_names_resolve_and_the_answer_is_checked() {
        let t = parse_target("localhost").unwrap();
        assert_eq!(t.ip, Ipv4Addr::LOCALHOST);
        assert_eq!(t.host, "localhost");
    }

    #[test]
    fn late_needs_both_a_ratio_and_a_floor() {
        // Healthy LAN: 3 ms against a 1 ms median is noise, not a late packet.
        assert!(!is_late(3.0, Some(1.0)));
        assert!(!is_late(11.0, Some(1.0)), "ten over the median is not enough");
        assert!(is_late(12.0, Some(1.0)));
        assert!(!is_late(25.0, Some(20.0)), "under twice the median");
        assert!(is_late(41.0, Some(20.0)));
        assert!(is_late(151.0, Some(100.0)), "over 150 ms is late whatever the median");
        assert!(!is_late(150.0, None));
        assert!(is_late(150.5, None), "the absolute line applies before there is a median");
        assert!(!is_late(80.0, None));
    }

    #[test]
    fn classify_marks_late_with_the_final_median_once_there_are_enough_replies() {
        let reps = [(0, 20), (1, 21), (2, 19), (3, 22), (4, 20), (5, 300), (6, 45), (7, 25)];
        let list: Vec<_> = reps.iter().map(|&(s, r)| (s, Some(r))).collect();
        let late: Vec<u32> = classify(&arrived(&list)).iter().filter(|s| s.late).map(|s| s.seq).collect();
        assert_eq!(late, vec![5, 6], "300 and 45 (median 21.5), not 25");
        // Four replies: no median yet, only the absolute line.
        let few = classify(&arrived(&[(0, Some(20)), (1, Some(60)), (2, Some(20)), (3, Some(200))]));
        assert_eq!(few.iter().map(|s| s.late).collect::<Vec<_>>(), vec![false, false, false, true]);
    }

    #[test]
    fn out_of_order_means_after_a_higher_sequence() {
        let flags = |seqs: &[u32]| -> Vec<bool> {
            let list: Vec<_> = seqs.iter().map(|&s| (s, Some(5))).collect();
            classify(&arrived(&list)).iter().map(|s| s.out_of_order).collect()
        };
        assert_eq!(flags(&[0, 1, 2, 3]), vec![false; 4]);
        assert_eq!(flags(&[0, 1, 3, 2, 4]), vec![false, false, false, true, false]);
        assert_eq!(flags(&[0, 2, 1, 4, 3]), vec![false, false, true, false, true]);
        assert_eq!(flags(&[3, 2, 1, 0]), vec![false, true, true, true]);
        // A skipped (lost) number is not reordering, and a lost packet is never "out of order".
        let with_loss = classify(&arrived(&[(0, Some(5)), (1, None), (2, Some(5)), (3, Some(5))]));
        assert!(with_loss.iter().all(|s| !s.out_of_order));
        let late_loss = classify(&arrived(&[(0, Some(5)), (2, Some(5)), (1, None)]));
        assert!(late_loss.iter().all(|s| !s.out_of_order), "the timeout of #1 arrives after #2 but it never replied");
        let s = sum(&[(0, Some(5)), (2, Some(5)), (1, Some(5))]);
        assert_eq!(s.out_of_order, 1);
        assert_eq!(classify(&arrived(&[(0, Some(5)), (2, Some(5)), (1, Some(5))]))[2].arrival, 2);
    }

    #[test]
    fn the_time_an_outcome_came_in_decides_the_order_not_the_order_it_was_reported() {
        // Reported 1 then 0, but 0 came in first: nothing is out of order.
        assert_eq!(reordered(&[at(1, 20.0), at(0, 5.0)]), Vec::<u32>::new());
        // Reported 0 then 1, but 1 came in first: 0 is late to its own party.
        assert_eq!(reordered(&[at(0, 50.0), at(1, 20.0)]), vec![0]);
        // The arrival rank follows the time, not the list.
        let ranks: Vec<(u32, u32)> = classify(&[at(0, 50.0), at(1, 20.0), at(2, 30.0)]).iter().map(|s| (s.seq, s.arrival)).collect();
        assert_eq!(ranks, vec![(1, 0), (2, 1), (0, 2)]);
    }

    #[test]
    fn a_queue_let_go_at_once_is_not_reordering() {
        // Three packets held up on the path and released together (a Wi-Fi retry, a full buffer):
        // the API rounds to the millisecond, so their order inside the burst means nothing.
        assert_eq!(reordered(&[at(2, 100.0), at(1, 100.6), at(0, 101.4)]), Vec::<u32>::new());
        assert_eq!(reordered(&[at(2, 100.0), at(1, 102.0), at(0, 101.0)]), Vec::<u32>::new(), "2 ms apart is still the same instant");
        // A real swap, clearly apart.
        assert_eq!(reordered(&[at(2, 100.0), at(1, 102.5)]), vec![1]);
        assert_eq!(reordered(&[at(2, 100.0), at(1, 103.0), at(0, 160.0)]), vec![0, 1]);
        // A lost packet in the middle changes nothing.
        let mut list = vec![at(3, 100.0), at(1, 150.0)];
        list.push(Arrived { seq: 2, rtt_ms: None, at_ms: 120.0 });
        assert_eq!(reordered(&list), vec![1]);
    }

    #[test]
    fn percentile_is_nearest_rank() {
        let hundred: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(p95(&hundred), Some(95.0));
        let twenty: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(p95(&twenty), Some(19.0));
        assert_eq!(p95(&[7.0]), Some(7.0));
        assert_eq!(p95(&[1.0, 2.0]), Some(2.0));
        assert_eq!(p95(&[]), None);
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[3.0]), Some(3.0));
        assert_eq!(median(&[1.0, 2.0, 3.0, 10.0]), Some(2.5));
    }

    #[test]
    fn summary_counts_loss_runs_and_percentiles() {
        // Sequence numbers 1 to 3 lost back to back, 6 lost alone.
        let mut list: Vec<(u32, Option<u32>)> = (0..9).map(|s| (s, Some(10 + s))).collect();
        for s in [1, 2, 3, 6] {
            list[s as usize].1 = None;
        }
        let m = sum(&list);
        assert_eq!((m.sent, m.received, m.lost), (9, 5, 4));
        assert!((m.loss_pct - 100.0 * 4.0 / 9.0).abs() < 1e-9);
        assert_eq!(m.longest_loss_run, 3);
        assert_eq!((m.min_ms, m.max_ms), (Some(10.0), Some(18.0)));
        assert_eq!(m.median_ms, Some(15.0));
        assert_eq!(m.p95_ms, Some(18.0));
    }

    #[test]
    fn loss_runs_follow_sequence_numbers_not_arrival() {
        // Timeouts arrive late and interleaved with replies; the run is still 3 (4, 5, 6).
        let m = sum(&[(0, Some(5)), (7, Some(5)), (4, None), (5, None), (6, None), (1, Some(5))]);
        assert_eq!(m.longest_loss_run, 3);
        // Two losses with a gap between are two runs of one.
        assert_eq!(sum(&[(0, None), (1, Some(5)), (2, None)]).longest_loss_run, 1);
        assert_eq!(sum(&[(u32::MAX, None), (0, None)]).longest_loss_run, 1, "huge numbers do not wrap");
    }

    #[test]
    fn jitter_follows_send_order() {
        // Sent 10, 20, 10, 20 but they arrived scrambled: jitter is still 10.
        let m = sum(&[(1, Some(20)), (0, Some(10)), (3, Some(20)), (2, Some(10))]);
        assert_eq!(m.jitter_ms, Some(10.0));
        assert_eq!(m.avg_ms, Some(15.0));
    }

    #[test]
    fn empty_and_total_loss_do_not_invent_numbers() {
        let none = sum(&[]);
        assert_eq!((none.sent, none.loss_pct, none.avg_ms, none.p95_ms, none.median_ms), (0, 0.0, None, None, None));
        let all = sum(&[(0, None), (1, None), (2, None)]);
        assert_eq!((all.received, all.lost, all.loss_pct), (0, 3, 100.0));
        assert_eq!((all.avg_ms, all.jitter_ms, all.longest_loss_run), (None, None, 3));
        assert!(serde_json::to_string(&all).unwrap().contains("\"avg_ms\":null"), "never NaN in the JSON");
    }

    fn fake_report() -> Report {
        let list = [(0, Some(12)), (1, None), (2, Some(14)), (3, Some(400)), (4, Some(13)), (5, Some(12)), (6, Some(15))];
        let samples = {
            let mut s = classify(&arrived(&list));
            s.sort_by_key(|x| x.seq);
            s
        };
        Report {
            host: "example.com".into(),
            ip: Ipv4Addr::new(93, 184, 216, 34),
            profile: Profile::Quick,
            cancelled: false,
            started: "2026-10-25 14:30:12".into(),
            duration_ms: 10_042,
            summary: summarize(&samples),
            samples,
        }
    }

    #[test]
    fn csv_has_one_row_per_packet_then_the_summary() {
        let r = fake_report();
        let text = csv(&r, CsvFormat::for_locale("en"));
        let lines: Vec<&str> = text.split("\r\n").collect();
        assert_eq!(lines[0], "seq,result,rtt_ms,out_of_order,arrival");
        assert_eq!(lines[1], "0,ok,12,0,0");
        assert_eq!(lines[2], "1,lost,,0,1");
        assert_eq!(lines[4], "3,late,400,0,3");
        assert_eq!(lines[8], "", "a blank line before the summary");
        assert_eq!(lines[9], "metric,value");
        assert!(lines.contains(&"target,example.com"));
        assert!(lines.contains(&"lost,1"));
        assert!(lines.contains(&"late,1"));
        assert!(lines.contains(&"loss_pct,14.3"));
        assert!(lines.contains(&"started,2026-10-25 14:30:12"));
        assert!(lines.contains(&"duration_s,10.0"));
        assert!(text.ends_with("\r\n"));
    }

    #[test]
    fn csv_for_brazil_uses_semicolons_and_decimal_commas() {
        let text = csv(&fake_report(), CsvFormat::for_locale("pt-BR"));
        assert!(text.starts_with("seq;result;rtt_ms;out_of_order;arrival\r\n"));
        assert!(text.contains("\r\nloss_pct;14,3\r\n"));
        assert!(text.contains("\r\nstarted;2026-10-25 14:30:12\r\n"));
        // Anything but pt-BR (a made-up locale included) falls back to the plain format.
        assert_eq!(CsvFormat::for_locale("xx; rm"), CsvFormat::for_locale("en"));
    }

    #[test]
    fn csv_cells_defuse_formulas_and_quoting() {
        assert_eq!(text_cell("=cmd|' /C calc'!A0", ';'), "'=cmd|' /C calc'!A0");
        assert_eq!(text_cell("+1+1", ','), "'+1+1");
        assert_eq!(text_cell("-2+3", ','), "'-2+3");
        assert_eq!(text_cell("@SUM(A1)", ','), "'@SUM(A1)");
        assert_eq!(text_cell("a,b", ','), "\"a,b\"");
        assert_eq!(text_cell("a;b", ','), "a;b", "only the active separator forces quotes");
        assert_eq!(text_cell("say \"hi\"", ','), "\"say \"\"hi\"\"\"");
        assert_eq!(text_cell("two\r\nlines\0", ','), "twolines", "control characters never reach the file");
        assert_eq!(text_cell(" x", ','), "\" x\"");
        assert_eq!(text_cell("plain.host-1", ','), "plain.host-1");
    }

    #[test]
    fn file_names_cannot_escape_the_folder() {
        let mut r = fake_report();
        assert_eq!(file_name(&r), "G2Boost-perda-de-pacotes-example.com-2026-10-25_14-30-12.csv");
        r.host = "..\\..\\Windows\\evil".into();
        r.started = "2026-10-25 14:30:12".into();
        let n = file_name(&r);
        assert!(!n.contains(['\\', '/', ':']), "{n}");
        assert!(n.starts_with("G2Boost-perda-de-pacotes-") && n.ends_with(".csv"));
        r.host = "h".repeat(500);
        assert!(file_name(&r).len() < 120);
    }

    #[test]
    fn write_report_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("g2boost-pktloss-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let r = fake_report();
        let f = CsvFormat::for_locale("en");
        let a = write_report(&dir, &r, f).unwrap();
        let b = write_report(&dir, &r, f).unwrap();
        assert_ne!(a, b);
        assert!(b.file_name().unwrap().to_string_lossy().ends_with("-1.csv"));
        assert_eq!(std::fs::read_to_string(&a).unwrap(), csv(&r, f));
        assert!(write_report(&dir.join("missing").join("deeper"), &r, f).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn documents_folder_exists() {
        let dir = documents_dir().expect("a Documents folder");
        assert!(dir.is_dir(), "{dir:?}");
    }

    fn probe_fn(f: impl Fn(u32) -> Option<u32> + Send + Sync + 'static) -> Arc<ProbeFn> {
        Arc::new(f)
    }

    #[test]
    fn schedule_reports_every_probe_once() {
        static NO_CANCEL: AtomicBool = AtomicBool::new(false);
        let mut events: Vec<ProbeEvent> = Vec::new();
        let (samples, cancelled) = run_with(
            30,
            Duration::from_millis(1),
            probe_fn(|seq| (seq % 3 != 0).then_some(5 + seq % 4)),
            &NO_CANCEL,
            &mut |e| events.push(e.clone()),
        );
        assert!(!cancelled);
        assert_eq!(samples.len(), 30);
        assert_eq!(events.len(), 30);
        assert!(samples.windows(2).all(|w| w[0].seq + 1 == w[1].seq), "by sequence number");
        let mut seqs: Vec<u32> = events.iter().map(|e| e.seq).collect();
        seqs.sort_unstable();
        assert_eq!(seqs, (0..30).collect::<Vec<_>>());
        let last = events.last().unwrap();
        assert_eq!((last.summary.sent, last.summary.lost), (30, 10));
        assert_eq!(summarize(&samples).lost, 10);
        assert!(events.windows(2).all(|w| w[1].summary.sent == w[0].summary.sent + 1), "running totals");
    }

    #[test]
    fn schedule_sees_a_slow_reply_come_in_after_newer_ones() {
        static NO_CANCEL: AtomicBool = AtomicBool::new(false);
        // Sent 40 ms apart, the first one answers after 600 ms: it comes in last, whatever the
        // threads did on this busy machine.
        let (samples, _) = run_with(
            6,
            Duration::from_millis(40),
            probe_fn(|seq| {
                if seq == 0 {
                    std::thread::sleep(Duration::from_millis(600));
                    return Some(600);
                }
                Some(1)
            }),
            &NO_CANCEL,
            &mut |_| {},
        );
        let m = summarize(&samples);
        assert!(samples[0].out_of_order && samples[0].late);
        assert_eq!(samples[0].arrival, 5, "it came back last");
        assert!(m.out_of_order >= 1);
        assert_eq!(m.late, 1);
    }

    #[test]
    fn schedule_stops_on_cancel_and_drops_what_is_in_flight() {
        static CANCEL_NOW: AtomicBool = AtomicBool::new(false);
        let mut seen = 0;
        let (samples, cancelled) = run_with(
            200,
            Duration::from_millis(10),
            probe_fn(|_| Some(1)),
            &CANCEL_NOW,
            &mut |_| {
                seen += 1;
                if seen == 5 {
                    CANCEL_NOW.store(true, Ordering::SeqCst);
                }
            },
        );
        assert!(cancelled);
        assert!(samples.len() >= 5 && samples.len() < 60, "{}", samples.len());
        assert_eq!(summarize(&samples).sent as usize, samples.len());
    }

    #[test]
    fn schedule_survives_a_probe_that_reports_nonsense() {
        // Probes only return rtt values; the schedule itself owns the sequence numbers.
        static NO_CANCEL: AtomicBool = AtomicBool::new(false);
        let (samples, _) = run_with(
            10,
            Duration::from_millis(1),
            probe_fn(|_| Some(u32::MAX)),
            &NO_CANCEL,
            &mut |_| {},
        );
        assert_eq!(samples.len(), 10);
        assert!(samples.iter().all(|s| s.late));
        assert!(summarize(&samples).avg_ms.is_some_and(f64::is_finite));
    }

    #[test]
    fn only_one_run_at_a_time() {
        let _turn = GLOBAL.lock().unwrap_or_else(|e| e.into_inner());
        let slot = Slot::take().expect("free");
        assert!(Slot::take().is_none());
        assert_eq!(run("127.0.0.1", Profile::Quick, &mut |_| {}).err(), Some(RunError::Busy));
        drop(slot);
        assert!(Slot::take().is_some());
    }

    #[test]
    fn a_bad_target_never_starts_a_run() {
        let _turn = GLOBAL.lock().unwrap_or_else(|e| e.into_inner());
        let err = run("1.1.1.1; calc", Profile::Quick, &mut |_| panic!("no probe may be sent")).err();
        assert_eq!(err, Some(RunError::Target(TargetError::Invalid)));
        assert_eq!(err.unwrap().to_string(), "invalid");
        assert_eq!(RunError::Busy.to_string(), "busy");
        assert!(!RUNNING.load(Ordering::SeqCst), "the slot is released");
    }

    #[test]
    fn export_without_a_run_says_so() {
        let _turn = GLOBAL.lock().unwrap_or_else(|e| e.into_inner());
        if last().is_none() {
            assert!(matches!(export_csv("en"), Err(ExportError::NoRun)));
        }
        assert_eq!(ExportError::NoRun.to_string(), "no_run");
    }

    #[test]
    #[ignore = "real ICMP on loopback, 10 s"]
    fn live_loopback_burst() {
        let _turn = GLOBAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut n = 0;
        let r = run("127.0.0.1", Profile::Quick, &mut |_| n += 1).expect("run");
        assert_eq!(n, 100);
        assert_eq!(r.summary.sent, 100);
        assert_eq!(r.summary.lost, 0, "loopback never drops: {:?}", r.summary);
        assert!(!r.cancelled);
        assert!(last().is_some_and(|l| l.summary == r.summary));
    }

    #[test]
    #[ignore = "real internet, 10 s"]
    fn live_internet_burst() {
        let _turn = GLOBAL.lock().unwrap_or_else(|e| e.into_inner());
        let r = run("1.1.1.1", Profile::Quick, &mut |_| {}).expect("run");
        eprintln!("{:#?}", r.summary);
        assert_eq!(r.summary.sent, 100);
    }

    #[test]
    #[ignore = "real ICMP on loopback; cancels after about a second"]
    fn live_cancel_returns_a_partial_report() {
        let _turn = GLOBAL.lock().unwrap_or_else(|e| e.into_inner());
        let stopper = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(1200));
            cancel();
        });
        let r = run("127.0.0.1", Profile::Long, &mut |_| {}).expect("run");
        stopper.join().unwrap();
        assert!(r.cancelled);
        assert!(r.summary.sent > 3 && r.summary.sent < 600, "{}", r.summary.sent);
    }
}
