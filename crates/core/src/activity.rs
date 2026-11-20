//! Activity center (round 11): the notifications the app raises for the person, kept locally.
//!
//! A job that finished, a check whose result changed after a reboot, a routine that ran in the
//! background. Each module keeps its own detailed history (cleanup, updates, repair...); this file
//! only holds the short "something happened" line that the bell in the header shows.
//!
//! Text is never stored: an entry carries an i18n key plus small arguments, so the UI renders it in
//! the current language. No paths, user names or other PII go in `args` (callers pass sizes,
//! counts, game or app display names). The file is hostile input when read back.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::registry;

/// Newest entries kept; older ones fall off.
pub const MAX: usize = 200;
const MAX_ARGS: usize = 8;
const MAX_ARG_LEN: usize = 120;

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Info,
    Ok,
    Warn,
    Err,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Unique, increasing (unix ms, bumped on collision).
    pub id: u64,
    /// Unix seconds.
    pub at: u64,
    /// Module that raised it: "cleanup", "anticheat", "ram", "luli", "routine"...
    pub kind: String,
    /// i18n key of the line, e.g. "activity.cleanup.done".
    pub key: String,
    #[serde(default)]
    pub args: BTreeMap<String, String>,
    pub tone: Tone,
    /// Route the line opens when clicked (must be a known route; the UI re-checks).
    #[serde(default)]
    pub route: Option<String>,
    #[serde(default)]
    pub read: bool,
}

/// ASCII identifier: letters, digits, `.`, `_`, `-`, `:`; 1..=64 chars.
fn ident(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-:".contains(&b))
}

fn clean_arg(v: &str) -> String {
    v.chars().filter(|c| !c.is_control()).take(MAX_ARG_LEN).collect()
}

fn valid(e: &Entry) -> bool {
    ident(&e.kind)
        && ident(&e.key)
        && e.args.len() <= MAX_ARGS
        && e.args.iter().all(|(k, v)| ident(k) && v.chars().count() <= MAX_ARG_LEN && !v.chars().any(char::is_control))
        && e.route.as_deref().is_none_or(ident)
}

fn file() -> PathBuf {
    registry::data_dir().join("activity.json")
}

fn load() -> Vec<Entry> {
    let mut all: Vec<Entry> = fs::read_to_string(file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    all.retain(valid);
    if all.len() > MAX {
        let drop = all.len() - MAX;
        all.drain(..drop);
    }
    all
}

fn save(all: &[Entry]) {
    let _ = registry::write_atomic(&file(), serde_json::to_string(all).unwrap_or_default().as_bytes());
}

/// Oldest first.
pub fn list() -> Vec<Entry> {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    load()
}

/// Record one line. Bad identifiers are refused (None); argument values are trimmed and stripped
/// of control characters.
pub fn push(kind: &str, key: &str, args: &[(&str, &str)], tone: Tone, route: Option<&str>) -> Option<Entry> {
    if !ident(kind) || !ident(key) || route.is_some_and(|r| !ident(r)) || args.len() > MAX_ARGS {
        return None;
    }
    if args.iter().any(|(k, _)| !ident(k)) {
        return None;
    }
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut all = load();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let id = all.last().map_or(now_ms, |l| now_ms.max(l.id + 1));
    let e = Entry {
        id,
        at: registry::now_unix(),
        kind: kind.into(),
        key: key.into(),
        args: args.iter().map(|(k, v)| ((*k).to_string(), clean_arg(v))).collect(),
        tone,
        route: route.map(str::to_string),
        read: false,
    };
    all.push(e.clone());
    if all.len() > MAX {
        let drop = all.len() - MAX;
        all.drain(..drop);
    }
    save(&all);
    Some(e)
}

/// Mark the given ids as read; an empty slice marks everything.
pub fn mark_read(ids: &[u64]) {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut all = load();
    for e in &mut all {
        if ids.is_empty() || ids.contains(&e.id) {
            e.read = true;
        }
    }
    save(&all);
}

pub fn clear() {
    let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    save(&[]);
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test: every scenario shares activity.json in the test data dir, so they run in order.
    #[test]
    fn store_round_trip_hostile_input_and_cap() {
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", registry::tests::test_data_dir()) };

        // round trip
        clear();
        let a = push("cleanup", "activity.cleanup.done", &[("bytes", "1048576")], Tone::Ok, Some("cleanup")).unwrap();
        let b = push("anticheat", "activity.anticheat.changed", &[], Tone::Warn, None).unwrap();
        assert!(b.id > a.id, "ids increase");
        assert_eq!(list().len(), 2);
        mark_read(&[a.id]);
        let all = list();
        assert!(all[0].read && !all[1].read);
        mark_read(&[]);
        assert!(list().iter().all(|e| e.read));

        // hostile identifiers refused, argument values cleaned
        clear();
        assert!(push("../x", "k", &[], Tone::Info, None).is_none());
        assert!(push("k", "a b", &[], Tone::Info, None).is_none());
        assert!(push("k", "k", &[], Tone::Info, Some("javascript:alert(1)")).is_none());
        assert!(push("k", "k", &[("bad key", "v")], Tone::Info, None).is_none());
        let e = push("k", "k", &[("v", "line\nbreak\u{7}")], Tone::Info, None).unwrap();
        assert_eq!(e.args["v"], "linebreak");
        let long = "x".repeat(500);
        let e = push("k", "k", &[("v", &long)], Tone::Info, None).unwrap();
        assert_eq!(e.args["v"].len(), MAX_ARG_LEN);

        // a tampered file is filtered, garbage reads as empty
        let bad = r#"[{"id":1,"at":1,"kind":"../../x","key":"k","tone":"info"},
                      {"id":2,"at":1,"kind":"ok","key":"k","tone":"ok","route":"a b"},
                      {"id":3,"at":1,"kind":"ok","key":"k","tone":"err"}]"#;
        fs::write(file(), bad).unwrap();
        let all = list();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, 3);
        fs::write(file(), "not json").unwrap();
        assert!(list().is_empty());

        // capped, oldest dropped first
        clear();
        for i in 0..(MAX + 5) {
            push("k", "k", &[("i", &i.to_string())], Tone::Info, None);
        }
        let all = list();
        assert_eq!(all.len(), MAX);
        assert_eq!(all[0].args["i"], "5");
        clear();
    }
}

