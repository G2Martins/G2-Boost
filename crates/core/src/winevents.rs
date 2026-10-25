//! Windows Event Log summary: errors and critical events of the System and Application logs over
//! the last days, read with the Event Log API (`wevtapi`), never `wevtutil`. Grouped by source +
//! event id so a flood of one error reads as one line, with a `hint` for the well-known ones
//! (explained in `knowledge/05-saude-do-windows.md`). Read-only; no admin needed for these logs.
//!
//! Messages are data written by drivers and apps: trimmed, single-line, and stripped of the user
//! profile path, the user name and the computer name before they leave this module.

use std::collections::HashMap;

use serde::Serialize;
use windows::Win32::System::EventLog::{
    EVT_HANDLE, EVT_VARIANT, EvtClose, EvtCreateRenderContext, EvtFormatMessage, EvtFormatMessageEvent, EvtNext,
    EvtOpenPublisherMetadata, EvtQuery, EvtQueryChannelPath, EvtQueryReverseDirection, EvtRender,
    EvtRenderContextSystem, EvtRenderEventValues, EvtSystemEventID, EvtSystemLevel, EvtSystemProviderName,
    EvtSystemTimeCreated,
};
use windows::core::PCWSTR;

use crate::registry::wide;

#[derive(Debug, Clone, Serialize)]
pub struct EventGroup {
    pub log: &'static str,
    pub source: String,
    pub event_id: u32,
    /// `critical`, `error` or `warning` (only Display 4101 is read at warning level).
    pub level: &'static str,
    pub count: u32,
    /// Newest occurrence.
    pub last_unix: u64,
    /// What it usually means: `hardware_whea`, `unexpected_shutdown`, `bugcheck`, `disk`,
    /// `gpu_driver_timeout`, `service_failed`, `driver_load_failed`, `app_crash`, `app_hang`,
    /// `restore_points_lost`, `dcom_noise` (harmless, ignore).
    pub hint: Option<&'static str>,
    /// The newest event's text, trimmed and redacted.
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub days: u32,
    /// Events read (newest first) before grouping.
    pub scanned: u32,
    /// Hit the per-log cap: older events were not read.
    pub truncated: bool,
    /// Most frequent first.
    pub groups: Vec<EventGroup>,
}

const LOGS: &[&str] = &["System", "Application"];
const MAX_PER_LOG: u32 = 3000;
const MAX_MESSAGE: usize = 300;

/// Known sources and ids → a stable hint the assistant (and the knowledge base) explains.
pub fn hint(source: &str, id: u32) -> Option<&'static str> {
    let s = source.to_ascii_lowercase();
    Some(match (s.as_str(), id) {
        ("microsoft-windows-whea-logger", _) => "hardware_whea",
        ("microsoft-windows-kernel-power", 41) | ("eventlog", 6008) => "unexpected_shutdown",
        ("volsnap", 36) => "restore_points_lost",
        ("microsoft-windows-wer-systemerrorreporting" | "bugcheck", 1001) => "bugcheck",
        ("disk", 7 | 51 | 153 | 157) | ("ntfs" | "microsoft-windows-ntfs", 55) | ("stornvme" | "storahci", _) => "disk",
        ("display", 4101) => "gpu_driver_timeout",
        ("service control manager", 7000 | 7001 | 7009 | 7011 | 7022 | 7023 | 7024 | 7031 | 7034) => "service_failed",
        ("microsoft-windows-kernel-pnp", 219) => "driver_load_failed",
        ("application error", 1000) => "app_crash",
        ("application hang", 1002) => "app_hang",
        ("microsoft-windows-distributedcom" | "distributedcom", _) => "dcom_noise",
        _ => return None,
    })
}

fn level_name(level: u8) -> &'static str {
    match level {
        1 => "critical",
        2 => "error",
        _ => "warning",
    }
}

/// ASCII case-insensitive replace of `needle` (Windows names), only where `ok(text, end)` accepts
/// the match end. The cursor moves past each replacement, so a name inside the placeholder cannot
/// loop.
fn replace_ci(s: &mut String, needle: &str, with: &str, ok: impl Fn(&str, usize) -> bool) {
    if needle.len() < 2 {
        return;
    }
    let needle_l = needle.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = s[from..].to_ascii_lowercase().find(&needle_l).map(|i| i + from) {
        if ok(s, i + needle.len()) {
            s.replace_range(i..i + needle.len(), with);
            from = i + with.len();
        } else {
            from = i + needle.len();
        }
    }
}

/// Without the profile path, the computer name and `\<user>` (account names like `PC\maria`).
/// The bare user name is left alone on purpose: an account called "Windows" or "Steam" would
/// otherwise mangle ordinary text.
pub fn redact(raw: &str, user: &str, computer: &str, profile: &str) -> String {
    let mut s = raw.to_string();
    let word_end = |t: &str, end: usize| !t[end..].chars().next().is_some_and(char::is_alphanumeric);
    replace_ci(&mut s, profile, "%USERPROFILE%", |_, _| true);
    replace_ci(&mut s, computer, "%COMPUTERNAME%", word_end);
    if user.len() >= 2 {
        replace_ci(&mut s, &format!("\\{user}"), "\\%USERNAME%", word_end);
    }
    s
}

/// [`redact`] with this session's names.
pub fn redact_here(raw: &str) -> String {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    redact(raw, &var("USERNAME"), &var("COMPUTERNAME"), &var("USERPROFILE"))
}

/// One line, bounded, redacted.
pub fn clean_message(raw: &str, user: &str, computer: &str, profile: &str) -> String {
    let one_line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut s = redact(&one_line, user, computer, profile);
    if s.chars().count() > MAX_MESSAGE {
        s = s.chars().take(MAX_MESSAGE).collect::<String>() + "…";
    }
    s
}

struct Handle(EVT_HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        if self.0.0 != 0 {
            // SAFETY: handle returned by the Evt* API, closed once.
            let _ = unsafe { EvtClose(self.0) };
        }
    }
}

fn pcwstr_to_string(p: PCWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    // SAFETY: EvtRender returns NUL-terminated strings inside the buffer we own.
    unsafe { p.to_string().unwrap_or_default() }
}

/// System properties of one event: (provider, id, level, filetime).
fn render(ctx: EVT_HANDLE, event: EVT_HANDLE) -> Option<(String, u32, u8, u64)> {
    let (mut used, mut count) = (0u32, 0u32);
    // SAFETY: size probe (fails with ERROR_INSUFFICIENT_BUFFER, fills `used`).
    let _ = unsafe { EvtRender(Some(ctx), event, EvtRenderEventValues.0, 0, None, &mut used, &mut count) };
    if used == 0 {
        return None;
    }
    let mut buf = vec![0u64; (used as usize).div_ceil(8)];
    // SAFETY: buffer of `used` bytes, 8-aligned for EVT_VARIANT.
    unsafe {
        EvtRender(Some(ctx), event, EvtRenderEventValues.0, used, Some(buf.as_mut_ptr().cast()), &mut used, &mut count)
            .ok()?;
    }
    let vals = buf.as_ptr().cast::<EVT_VARIANT>();
    let n = count as usize;
    let at = |i: i32| -> Option<&EVT_VARIANT> {
        let i = usize::try_from(i).ok()?;
        // SAFETY: `count` variants were written at the start of `buf`.
        (i < n).then(|| unsafe { &*vals.add(i) })
    };
    // SAFETY: union fields read per the documented type of each system property.
    unsafe {
        let provider = pcwstr_to_string(at(EvtSystemProviderName.0)?.Anonymous.StringVal);
        let id = u32::from(at(EvtSystemEventID.0)?.Anonymous.UInt16Val);
        let level = at(EvtSystemLevel.0)?.Anonymous.ByteVal;
        let ft = at(EvtSystemTimeCreated.0)?.Anonymous.FileTimeVal;
        Some((provider, id, level, ft))
    }
}

fn message(provider: &str, event: EVT_HANDLE) -> Option<String> {
    let w = wide(provider);
    // SAFETY: publisher metadata handle closed by `Handle`; message buffer sized by the probe.
    unsafe {
        let meta = Handle(EvtOpenPublisherMetadata(None, PCWSTR(w.as_ptr()), PCWSTR::null(), 0, 0).ok()?);
        let mut used = 0u32;
        let _ = EvtFormatMessage(Some(meta.0), Some(event), 0, None, EvtFormatMessageEvent.0, None, &mut used);
        if used == 0 || used > 64 * 1024 {
            return None;
        }
        let mut buf = vec![0u16; used as usize];
        EvtFormatMessage(Some(meta.0), Some(event), 0, None, EvtFormatMessageEvent.0, Some(&mut buf), &mut used).ok()?;
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

/// Errors and critical events of the last `days` (1..=30), newest first, grouped.
pub fn summary(days: u32) -> Summary {
    let days = days.clamp(1, 30);
    let ms = u64::from(days) * 86_400_000;
    let query = format!(
        "*[System[(Level=1 or Level=2 or (Level=3 and EventID=4101)) and TimeCreated[timediff(@SystemTime) <= {ms}]]]"
    );
    let user = std::env::var("USERNAME").unwrap_or_default();
    let computer = std::env::var("COMPUTERNAME").unwrap_or_default();
    let profile = std::env::var("USERPROFILE").unwrap_or_default();
    let mut groups: HashMap<(&'static str, String, u32), EventGroup> = HashMap::new();
    let (mut scanned, mut truncated) = (0u32, false);

    // SAFETY: render context closed by `Handle`.
    let Ok(ctx) = (unsafe { EvtCreateRenderContext(None, EvtRenderContextSystem.0) }) else {
        return Summary { days, scanned, truncated, groups: Vec::new() };
    };
    let ctx = Handle(ctx);
    for &log in LOGS {
        let (wl, wq) = (wide(log), wide(&query));
        // SAFETY: query handle closed by `Handle`.
        let Ok(rs) = (unsafe {
            EvtQuery(None, PCWSTR(wl.as_ptr()), PCWSTR(wq.as_ptr()), EvtQueryChannelPath.0 | EvtQueryReverseDirection.0)
        }) else {
            continue;
        };
        let rs = Handle(rs);
        let mut read = 0u32;
        'pages: loop {
            let mut batch = [0isize; 64];
            let mut got = 0u32;
            // SAFETY: up to 64 event handles written into `batch`; each closed below.
            if unsafe { EvtNext(rs.0, &mut batch, 1000, 0, &mut got) }.is_err() || got == 0 {
                break;
            }
            let events: Vec<Handle> = batch[..got as usize].iter().map(|&h| Handle(EVT_HANDLE(h))).collect();
            for ev in &events {
                if read >= MAX_PER_LOG {
                    truncated = true;
                    break 'pages;
                }
                read += 1;
                let Some((source, id, level, ft)) = render(ctx.0, ev.0) else {
                    continue;
                };
                let key = (log, source.clone(), id);
                if let Some(g) = groups.get_mut(&key) {
                    g.count += 1;
                    continue;
                }
                // Newest first: the first one seen carries the time and the message.
                let msg = message(&source, ev.0).map(|m| clean_message(&m, &user, &computer, &profile));
                groups.insert(
                    key,
                    EventGroup {
                        log,
                        hint: hint(&source, id),
                        source,
                        event_id: id,
                        level: level_name(level),
                        count: 1,
                        last_unix: crate::registry::filetime_to_unix(ft),
                        message: msg.filter(|m| !m.is_empty()),
                    },
                );
            }
        }
        scanned += read;
    }
    let mut groups: Vec<EventGroup> = groups.into_values().collect();
    groups.sort_by(|a, b| b.count.cmp(&a.count).then(b.last_unix.cmp(&a.last_unix)));
    Summary { days, scanned, truncated, groups }
}

/* ---------------- counts for the check-up ---------------- */

/// Counting stops here; the check-up only needs "none / a few / many".
const COUNT_CAP: u32 = 500;

/// XPath for one provider + event id over the last `ms` milliseconds. The arguments are
/// constants of ours (never user input).
fn count_query(provider: &str, id: u32, ms: u64) -> String {
    format!("*[System[Provider[@Name='{provider}'] and EventID={id} and TimeCreated[timediff(@SystemTime) <= {ms}]]]")
}

/// How many `provider`/`id` events `log` holds for the last `days` (1..=90), capped. `None` when
/// the log cannot be queried. Nothing is rendered: handles are only counted and closed.
fn count_matching(log: &str, provider: &str, id: u32, days: u32) -> Option<u32> {
    let query = count_query(provider, id, u64::from(days.clamp(1, 90)) * 86_400_000);
    let (wl, wq) = (wide(log), wide(&query));
    // SAFETY: query handle closed by `Handle`.
    let rs = Handle(unsafe { EvtQuery(None, PCWSTR(wl.as_ptr()), PCWSTR(wq.as_ptr()), EvtQueryChannelPath.0 | EvtQueryReverseDirection.0) }.ok()?);
    let mut n = 0u32;
    loop {
        let mut batch = [0isize; 64];
        let mut got = 0u32;
        // SAFETY: up to 64 event handles written into `batch`; each is closed through `Handle`.
        if unsafe { EvtNext(rs.0, &mut batch, 1000, 0, &mut got) }.is_err() || got == 0 {
            return Some(n);
        }
        let events: Vec<Handle> = batch[..got as usize].iter().map(|&h| Handle(EVT_HANDLE(h))).collect();
        n += events.len() as u32;
        if n >= COUNT_CAP {
            return Some(COUNT_CAP);
        }
    }
}

/// Kernel-Power 41 and EventLog 6008 both log the same unexpected shutdown (one per boot after
/// it): the larger count is the number of shutdowns. `None` only when neither log answered.
pub(crate) fn shutdowns_from(kernel_power: Option<u32>, event_log: Option<u32>) -> Option<u32> {
    match (kernel_power, event_log) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0).max(b.unwrap_or(0))),
    }
}

/// Unexpected shutdowns (power loss, freeze + reset, bug check) in the last `days`.
pub fn unexpected_shutdowns(days: u32) -> Option<u32> {
    shutdowns_from(
        count_matching("System", "Microsoft-Windows-Kernel-Power", 41, days),
        count_matching("System", "EventLog", 6008, days),
    )
}

/// Application crashes (Application Error 1000) in the last `days`.
pub fn app_crashes(days: u32) -> Option<u32> {
    count_matching("Application", "Application Error", 1000, days)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_query_and_shutdown_rule() {
        assert_eq!(
            count_query("Application Error", 1000, 604_800_000),
            "*[System[Provider[@Name='Application Error'] and EventID=1000 and TimeCreated[timediff(@SystemTime) <= 604800000]]]"
        );
        assert_eq!(shutdowns_from(None, None), None);
        assert_eq!(shutdowns_from(Some(2), None), Some(2));
        assert_eq!(shutdowns_from(None, Some(3)), Some(3));
        assert_eq!(shutdowns_from(Some(2), Some(3)), Some(3), "the pair describes the same shutdown, not two");
        assert_eq!(shutdowns_from(Some(0), Some(0)), Some(0));
    }

    /// Live: counts on this machine's logs (read-only, no admin).
    #[test]
    #[ignore = "live: reads this machine's event logs"]
    fn live_counts_answer() {
        let (s, c) = (unexpected_shutdowns(30), app_crashes(7));
        println!("unexpected shutdowns (30 d): {s:?}, app crashes (7 d): {c:?}");
        assert!(s.is_some() && c.is_some());
        assert_eq!(count_matching("System", "No-Such-Provider", 1, 7), Some(0));
    }

    #[test]
    fn hints_and_cleaning() {
        assert_eq!(hint("Microsoft-Windows-Kernel-Power", 41), Some("unexpected_shutdown"));
        assert_eq!(hint("Microsoft-Windows-WHEA-Logger", 18), Some("hardware_whea"));
        assert_eq!(hint("Application Error", 1000), Some("app_crash"));
        assert_eq!(hint("Display", 4101), Some("gpu_driver_timeout"));
        assert_eq!(hint("Disk", 153), Some("disk"));
        assert_eq!(hint("Something", 1), None);
        let m = clean_message(
            "Falha em C:\\Users\\Maria\\AppData\\x.exe\r\n  conta PC-SALA\\maria",
            "Maria",
            "PC-SALA",
            "C:\\Users\\Maria",
        );
        assert_eq!(m, "Falha em %USERPROFILE%\\AppData\\x.exe conta %COMPUTERNAME%\\%USERNAME%");
        // A user named like a common word does not mangle ordinary text.
        let m = clean_message("Windows Update falhou em C:\\Users\\Windows\\x", "Windows", "PC", "C:\\Users\\Windows");
        assert_eq!(m, "Windows Update falhou em %USERPROFILE%\\x");
        assert_eq!(redact("PCX e PC", "u", "PC", ""), "PCX e %COMPUTERNAME%");
        assert!(clean_message(&"x".repeat(500), "", "", "").chars().count() <= MAX_MESSAGE + 1);
        assert_eq!(crate::registry::filetime_to_unix(116_444_736_000_000_000), 0);
    }

    #[test]
    fn reads_the_real_logs_without_admin() {
        let s = summary(7);
        assert_eq!(s.days, 7);
        for g in &s.groups {
            assert!(g.count >= 1 && !g.source.is_empty());
            if let Some(m) = &g.message {
                assert!(!m.contains('\n') && m.chars().count() <= MAX_MESSAGE + 1);
            }
        }
        println!("scanned {} events, {} groups", s.scanned, s.groups.len());
    }
}
