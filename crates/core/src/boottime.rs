//! Boot time and what slows it, from the `Microsoft-Windows-Diagnostics-Performance/Operational`
//! channel (native EvtQuery, no `wevtutil`). Event 100 is one boot (BootTime, MainPathBootTime,
//! BootPostBootTime, all ms); 101/102/103/106/109 are the applications, drivers, services,
//! background optimizations and devices that were slowed down, with `DegradationTime`.
//!
//! Each event is rendered as XML and parsed by a tiny tolerant scanner (`parse_event`), kept pure
//! so it is unit-tested with sample strings. Event text is hostile input: garbage yields `None`,
//! names are redacted of user/profile paths. The channel may need admin or the Event Log Readers
//! group: access denied maps to `BootError::NeedsAdmin`.

use std::collections::HashMap;

use serde::Serialize;
use thiserror::Error;
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, WIN32_ERROR};
use windows::Win32::System::EventLog::{
    EVT_HANDLE, EvtClose, EvtNext, EvtQuery, EvtQueryChannelPath, EvtQueryReverseDirection, EvtRender,
    EvtRenderEventXml,
};
use windows::core::PCWSTR;

use crate::registry::wide;

const CHANNEL: &str = "Microsoft-Windows-Diagnostics-Performance/Operational";
const MAX_BOOTS: usize = 30;
const MAX_EVENTS: u32 = 5000;
const MAX_ITEMS: usize = 50;
const MAX_XML_BYTES: u32 = 64 * 1024;

#[derive(Debug, Error)]
pub enum BootError {
    #[error("reading the boot performance log needs administrator rights (or the Event Log Readers group)")]
    NeedsAdmin,
    #[error("could not read the boot performance log: {0}")]
    Read(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Application,
    Driver,
    Service,
    BackgroundOptimization,
    Device,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Boot {
    /// Boot start, seconds since the Unix epoch.
    pub at: u64,
    pub total_ms: u64,
    pub main_ms: u64,
    pub post_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SlowItem {
    pub kind: ItemKind,
    pub name: String,
    pub friendly: String,
    pub seen: u32,
    pub avg_degradation_ms: u64,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BootReport {
    pub days: u32,
    /// Newest first, at most 30.
    pub boots: Vec<Boot>,
    /// Sorted by total degradation (avg x seen), descending.
    pub slow: Vec<SlowItem>,
    /// When nothing was measured in the window: the most recent boot ever logged (Windows stops
    /// measuring when the Diagnostic Policy Service is off).
    pub last_known: Option<Boot>,
}

/// One parsed event.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    Boot(Boot),
    Item { kind: ItemKind, name: String, friendly: String, degradation_ms: u64, at: u64 },
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// Text of the first `<tag ...>text</tag>`.
fn tag_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = xml.find(&format!("<{tag}"))?;
    let rest = &xml[open + tag.len() + 1..];
    // The char after the tag name must end the name, so `<EventID` does not match `<EventIDx`.
    if !rest.starts_with(['>', ' ']) {
        return None;
    }
    let gt = rest.find('>')?;
    let body = &rest[gt + 1..];
    Some(&body[..body.find('<')?])
}

/// Value of `attr='...'` / `attr="..."` inside the first `<tag ...>` start tag.
fn tag_attr<'a>(xml: &'a str, tag: &str, attr: &str) -> Option<&'a str> {
    let open = xml.find(&format!("<{tag} "))?;
    let head = &xml[open..open + xml[open..].find('>')?];
    let a = head.find(&format!("{attr}="))? + attr.len() + 1;
    let q = head[a..].chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let v = &head[a + 1..];
    Some(&v[..v.find(q)?])
}

/// `<Data Name="X">v</Data>` pairs.
fn data_map(xml: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    let mut rest = xml;
    while let Some(i) = rest.find("<Data ") {
        rest = &rest[i..];
        let Some(gt) = rest.find('>') else { break };
        let head = &rest[..gt];
        let name = head.find("Name=").and_then(|a| {
            let s = &head[a + 5..];
            let q = s.chars().next().filter(|c| *c == '\'' || *c == '"')?;
            let s = &s[1..];
            Some(s[..s.find(q)?].to_string())
        });
        let (value, adv) = if head.ends_with('/') {
            (String::new(), gt + 1)
        } else {
            let body = &rest[gt + 1..];
            let end = body.find("</Data>").unwrap_or(body.len());
            (unescape(&body[..end]), gt + 1 + end)
        };
        if let Some(n) = name {
            m.insert(n, value);
        }
        rest = &rest[adv.min(rest.len())..];
    }
    m
}

/// `2026-10-18T09:58:12.345Z` to Unix seconds (UTC). Days-from-civil, no date crate.
fn iso_to_unix(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d, hh, mm, ss) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    let days = crate::advisor::days_from_civil(y, m, d);
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
}

fn clean(s: &str) -> String {
    crate::winevents::redact_here(s).chars().take(200).collect()
}

/// Parse one rendered event; `None` for anything that is not a usable 100/101/102/103/106/109.
pub fn parse_event(xml: &str) -> Option<Parsed> {
    let id: u32 = tag_text(xml, "EventID")?.trim().parse().ok()?;
    let sys_at = tag_attr(xml, "TimeCreated", "SystemTime").and_then(iso_to_unix).unwrap_or(0);
    let d = data_map(xml);
    let num = |k: &str| d.get(k).and_then(|v| v.trim().parse::<u64>().ok());
    if id == 100 {
        return Some(Parsed::Boot(Boot {
            at: d.get("BootStartTime").and_then(|v| iso_to_unix(v.trim())).unwrap_or(sys_at),
            total_ms: num("BootTime")?,
            main_ms: num("MainPathBootTime").unwrap_or(0),
            post_ms: num("BootPostBootTime").unwrap_or(0),
        }));
    }
    let kind = match id {
        101 => ItemKind::Application,
        102 => ItemKind::Driver,
        103 => ItemKind::Service,
        106 => ItemKind::BackgroundOptimization,
        109 => ItemKind::Device,
        _ => return None,
    };
    let name = clean(d.get("Name")?.trim());
    if name.is_empty() {
        return None;
    }
    Some(Parsed::Item {
        kind,
        friendly: clean(d.get("FriendlyName").map_or("", |s| s.trim())),
        name,
        degradation_ms: num("DegradationTime").or_else(|| num("TotalTime"))?,
        at: sys_at,
    })
}

/// Group items by (kind, name); sorted by total degradation, at most 50.
pub fn aggregate(items: Vec<Parsed>) -> Vec<SlowItem> {
    let mut m: HashMap<(ItemKind, String), (SlowItem, u64)> = HashMap::new();
    for p in items {
        let Parsed::Item { kind, name, friendly, degradation_ms, at } = p else { continue };
        let e = m.entry((kind, name.clone())).or_insert_with(|| {
            (SlowItem { kind, name, friendly: friendly.clone(), seen: 0, avg_degradation_ms: 0, last_seen: 0 }, 0)
        });
        e.0.seen += 1;
        e.1 += degradation_ms;
        if at >= e.0.last_seen {
            e.0.last_seen = at;
            if !friendly.is_empty() {
                e.0.friendly = friendly;
            }
        }
    }
    let mut v: Vec<(SlowItem, u64)> = m.into_values().collect();
    for (it, total) in &mut v {
        it.avg_degradation_ms = *total / u64::from(it.seen.max(1));
    }
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.name.cmp(&b.0.name)));
    v.into_iter().take(MAX_ITEMS).map(|(it, _)| it).collect()
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

fn map_err(e: &windows::core::Error) -> BootError {
    if e.code() == ERROR_ACCESS_DENIED.to_hresult() || WIN32_ERROR::from_error(e) == Some(ERROR_ACCESS_DENIED) {
        BootError::NeedsAdmin
    } else {
        BootError::Read(e.message())
    }
}

fn render_xml(event: EVT_HANDLE) -> Option<String> {
    let (mut used, mut count) = (0u32, 0u32);
    // SAFETY: size probe (fails with ERROR_INSUFFICIENT_BUFFER and fills `used`).
    let _ = unsafe { EvtRender(None, event, EvtRenderEventXml.0, 0, None, &mut used, &mut count) };
    if used == 0 || used > MAX_XML_BYTES {
        return None;
    }
    let mut buf = vec![0u16; (used as usize).div_ceil(2)];
    // SAFETY: buffer holds `used` bytes.
    unsafe { EvtRender(None, event, EvtRenderEventXml.0, used, Some(buf.as_mut_ptr().cast()), &mut used, &mut count) }
        .ok()?;
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..end]))
}

/// Boots and slow items of the last `days` (1..=90), newest first.
pub fn report(days: u32) -> Result<BootReport, BootError> {
    let days = days.clamp(1, 90);
    let ms = u64::from(days) * 86_400_000;
    let query = format!(
        "*[System[(EventID=100 or EventID=101 or EventID=102 or EventID=103 or EventID=106 or EventID=109) \
         and TimeCreated[timediff(@SystemTime) <= {ms}]]]"
    );
    let (wl, wq) = (wide(CHANNEL), wide(&query));
    // SAFETY: query handle closed by `Handle`.
    let rs = unsafe {
        EvtQuery(None, PCWSTR(wl.as_ptr()), PCWSTR(wq.as_ptr()), EvtQueryChannelPath.0 | EvtQueryReverseDirection.0)
    }
    .map_err(|e| map_err(&e))?;
    let rs = Handle(rs);

    let (mut boots, mut items, mut read) = (Vec::new(), Vec::new(), 0u32);
    'pages: loop {
        let mut batch = [0isize; 64];
        let mut got = 0u32;
        // SAFETY: up to 64 event handles written into `batch`; each closed below.
        if let Err(e) = unsafe { EvtNext(rs.0, &mut batch, 1000, 0, &mut got) } {
            if got == 0 && e.code() == ERROR_ACCESS_DENIED.to_hresult() {
                return Err(BootError::NeedsAdmin);
            }
            break;
        }
        if got == 0 {
            break;
        }
        let events: Vec<Handle> = batch[..got as usize].iter().map(|&h| Handle(EVT_HANDLE(h))).collect();
        for ev in &events {
            if read >= MAX_EVENTS {
                break 'pages;
            }
            read += 1;
            match render_xml(ev.0).as_deref().and_then(parse_event) {
                Some(Parsed::Boot(b)) if boots.len() < MAX_BOOTS => boots.push(b),
                Some(p @ Parsed::Item { .. }) => items.push(p),
                _ => {}
            }
        }
    }
    boots.sort_by_key(|b| std::cmp::Reverse(b.at));
    let last_known = if boots.is_empty() { latest_boot() } else { None };
    Ok(BootReport { days, boots, slow: aggregate(items), last_known })
}

/// The newest event 100 of any age (reads at most a handful of events).
fn latest_boot() -> Option<Boot> {
    let (wl, wq) = (wide(CHANNEL), wide("*[System[EventID=100]]"));
    // SAFETY: query handle closed by `Handle`.
    let rs = Handle(unsafe { EvtQuery(None, PCWSTR(wl.as_ptr()), PCWSTR(wq.as_ptr()), EvtQueryChannelPath.0 | EvtQueryReverseDirection.0) }.ok()?);
    let mut batch = [0isize; 4];
    let mut got = 0u32;
    // SAFETY: up to 4 handles written into `batch`; each closed by `Handle`.
    unsafe { EvtNext(rs.0, &mut batch, 1000, 0, &mut got) }.ok()?;
    let events: Vec<Handle> = batch[..got as usize].iter().map(|&h| Handle(EVT_HANDLE(h))).collect();
    events.iter().find_map(|ev| match render_xml(ev.0).as_deref().and_then(parse_event) {
        Some(Parsed::Boot(b)) => Some(b),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOOT: &str = r#"<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='Microsoft-Windows-Diagnostics-Performance' Guid='{CFC18EC0-96B1-4EBA-961B-622CAEE05B0A}'/><EventID>100</EventID><Level>2</Level><TimeCreated SystemTime='2026-10-18T10:01:30.1234567Z'/></System><EventData><Data Name='BootTsVersion'>2</Data><Data Name='BootStartTime'>2026-10-18T09:59:00.000000000Z</Data><Data Name='BootEndTime'>2026-10-18T10:00:30.000000000Z</Data><Data Name='BootTime'>41234</Data><Data Name='MainPathBootTime'>28000</Data><Data Name='BootPostBootTime'>13234</Data></EventData></Event>"#;

    const APP: &str = r#"<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><EventID Qualifiers='0'>101</EventID><TimeCreated SystemTime='2026-10-18T10:01:31.0Z'/></System><EventData><Data Name='Name'>Discord.exe</Data><Data Name='FriendlyName'>Discord &amp; Co</Data><Data Name='TotalTime'>9000</Data><Data Name='DegradationTime'>4500</Data></EventData></Event>"#;

    #[test]
    fn parses_boot_event() {
        let Some(Parsed::Boot(b)) = parse_event(BOOT) else { panic!("not a boot") };
        assert_eq!((b.total_ms, b.main_ms, b.post_ms), (41234, 28000, 13234));
        assert_eq!(b.at, iso_to_unix("2026-10-18T09:59:00Z").unwrap());
        assert_eq!(iso_to_unix("1970-01-02T00:00:00Z"), Some(86_400));
    }

    #[test]
    fn parses_app_event() {
        let Some(Parsed::Item { kind, name, friendly, degradation_ms, at }) = parse_event(APP) else {
            panic!("not an item")
        };
        assert_eq!(kind, ItemKind::Application);
        assert_eq!((name.as_str(), friendly.as_str()), ("Discord.exe", "Discord & Co"));
        assert_eq!(degradation_ms, 4500);
        assert!(at > 0);
    }

    #[test]
    fn garbage_is_none() {
        for s in ["", "not xml", "<Event><EventID>abc</EventID></Event>", "<Event><EventID>100</EventID></Event>", "<<<>>><Data Name=", "<EventID>7</EventID>"] {
            assert!(parse_event(s).is_none(), "{s}");
        }
        assert_eq!(iso_to_unix("2026-13-40Tzz"), None);
    }

    #[test]
    fn aggregates_by_name_sorted_by_total() {
        let it = |name: &str, d: u64, at: u64| Parsed::Item {
            kind: ItemKind::Service,
            name: name.into(),
            friendly: format!("{name} svc"),
            degradation_ms: d,
            at,
        };
        let v = aggregate(vec![it("a", 100, 1), it("b", 500, 2), it("a", 300, 5), it("a", 200, 3)]);
        assert_eq!(v[0].name, "a");
        assert_eq!((v[0].seen, v[0].avg_degradation_ms, v[0].last_seen), (3, 200, 5));
        assert_eq!(v[1].name, "b");
    }

    #[test]
    #[ignore = "reads the live event log; prints the report or the admin requirement"]
    fn live_report() {
        match report(30) {
            Ok(r) => println!("{} boots, {} slow items: {:?} / {:?}", r.boots.len(), r.slow.len(), r.boots.first(), r.slow.first()),
            Err(e) => println!("live report unavailable: {e}"),
        }
    }
}
