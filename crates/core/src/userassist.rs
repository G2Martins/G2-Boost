//! "Apps you don't use": last run of programs started from Explorer / Start.
//!
//! Source: `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist\{GUID}\Count`
//! for executables (`{CEBFF5CD-...}`) and shortcuts (`{F4E57C4B-...}`). Value names are ROT13;
//! the Windows 7+ data is 72 bytes: run count u32 @4, focus count u32 @8, focus time ms u32 @12,
//! last-run FILETIME u64 @60. Names may start with a known-folder GUID, mapped here to the real
//! folder via `SHGetKnownFolderPath`. Read-only, no admin; all data is hostile input.

use std::collections::HashMap;

use serde::Serialize;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{KNOWN_FOLDER_FLAG, SHGetKnownFolderPath};
use windows::core::GUID;

use crate::registry::{Root, Value, read, value_names};

const BASE: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\UserAssist";
const GUIDS: [&str; 2] = ["{CEBFF5CD-ACE2-4F4F-9178-9926F41749EA}", "{F4E57C4B-2036-45F0-A9AB-443BCFE33D9F}"];
const MAX_STR: usize = 520;
const EPOCH_DIFF_SECS: u64 = 11_644_473_600;
const MAX_UNIX: u64 = 32_503_680_000;

#[derive(Debug, Clone, Serialize)]
pub struct ExeUsage {
    pub path: String,
    pub runs: u32,
    pub focus_ms: u32,
    /// Unix seconds; 0 = unknown.
    pub last_run: u64,
}

/// ROT13 on ASCII letters only.
pub fn rot13(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' => (((c as u8 - b'a') + 13) % 26 + b'a') as char,
            'A'..='Z' => (((c as u8 - b'A') + 13) % 26 + b'A') as char,
            _ => c,
        })
        .collect()
}

/// (runs, focus_ms, last_run unix) from a 72-byte record; anything else → None.
pub fn parse_record(d: &[u8]) -> Option<(u32, u32, u64)> {
    if d.len() != 72 {
        return None;
    }
    let u32_at = |o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
    let ft = u64::from_le_bytes(d[60..68].try_into().ok()?);
    let last = (ft / 10_000_000).checked_sub(EPOCH_DIFF_SECS).filter(|&u| u <= MAX_UNIX).unwrap_or(0);
    Some((u32_at(4), u32_at(12), last))
}

/// Replace a leading `{known-folder GUID}` with the real folder. `resolve` maps an upper-case GUID
/// (with braces) to a path; unknown GUIDs are left as-is.
pub fn map_known_folder(name: &str, mut resolve: impl FnMut(&str) -> Option<String>) -> String {
    if name.starts_with('{')
        && let Some(end) = name.find('}')
        && let Some(base) = resolve(&name[..=end].to_ascii_uppercase())
    {
        let rest = name[end + 1..].trim_start_matches('\\');
        return format!(r"{}\{}", base.trim_end_matches('\\'), rest);
    }
    name.to_string()
}

fn folder_guid(g: &str) -> Option<GUID> {
    match g {
        "{6D809377-6AF0-444B-8957-A3773F02200E}" => Some(GUID::from_u128(0x6D809377_6AF0_444B_8957_A3773F02200E)),
        "{7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E}" => Some(GUID::from_u128(0x7C5A40EF_A0FB_4BFC_874A_C0F2E0B9FA8E)),
        "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}" => Some(GUID::from_u128(0x1AC14E77_02E7_4E5D_B744_2EB1AE5198B7)),
        "{F38BF404-1D43-42F2-9305-67DE0B28FC23}" => Some(GUID::from_u128(0xF38BF404_1D43_42F2_9305_67DE0B28FC23)),
        _ => None,
    }
}

fn known_folder(g: &str) -> Option<String> {
    let id = folder_guid(g)?;
    // SAFETY: the API allocates the string; it is copied then freed with CoTaskMemFree.
    unsafe {
        let p = SHGetKnownFolderPath(&id, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const std::ffi::c_void));
        s
    }
}

fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(MAX_STR).collect()
}

/// Everything Explorer recorded as launched (executables + shortcuts), merged per path
/// (runs / focus summed, newest last run), most recent first. Entries that are not file paths
/// (UWP `Microsoft.*!App` ids) are kept as-is.
pub fn usage() -> Vec<ExeUsage> {
    let mut cache: HashMap<String, Option<String>> = HashMap::new();
    let mut merged: HashMap<String, ExeUsage> = HashMap::new();
    for g in GUIDS {
        let key = format!(r"{BASE}\{g}\Count");
        for name in value_names(Root::Hkcu, &key).unwrap_or_default() {
            let Ok(Value::Binary(data)) = read(Root::Hkcu, &key, &name) else { continue };
            let Some((runs, focus_ms, last_run)) = parse_record(&data) else { continue };
            let plain = rot13(&name);
            let path = clean(&map_known_folder(&plain, |guid| cache.entry(guid.to_string()).or_insert_with(|| known_folder(guid)).clone()));
            if path.is_empty() || path.starts_with("UEME_") {
                continue; // bookkeeping counters (UEME_CTLSESSION etc.)
            }
            let e = merged.entry(path.to_ascii_lowercase()).or_insert(ExeUsage { path, runs: 0, focus_ms: 0, last_run: 0 });
            e.runs = e.runs.saturating_add(runs);
            e.focus_ms = e.focus_ms.saturating_add(focus_ms);
            e.last_run = e.last_run.max(last_run);
        }
    }
    let mut v: Vec<ExeUsage> = merged.into_values().collect();
    v.sort_by(|a, b| b.last_run.cmp(&a.last_run).then_with(|| a.path.cmp(&b.path)));
    v
}

/// Newest `last_run` among entries whose path is under `dir` (case-insensitive, whole path segment).
pub fn last_used_under(entries: &[ExeUsage], dir: &str) -> Option<u64> {
    let d = dir.trim_end_matches('\\').to_ascii_lowercase();
    if d.is_empty() {
        return None;
    }
    entries
        .iter()
        .filter(|e| {
            let p = e.path.to_ascii_lowercase();
            p.len() > d.len() && p.starts_with(&d) && p.as_bytes()[d.len()] == b'\\'
        })
        .map(|e| e.last_run)
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rot13_letters_only() {
        assert_eq!(rot13("HRZR_PGYFRFFVBA"), "UEME_CTLSESSION");
        assert_eq!(rot13(&rot13("C:\\Prog 1\\a.exe {X}")), "C:\\Prog 1\\a.exe {X}");
        assert_eq!(rot13("P:\\Hfref"), "C:\\Users");
    }

    fn rec(runs: u32, focus: u32, ft: u64) -> Vec<u8> {
        let mut d = vec![0u8; 72];
        d[4..8].copy_from_slice(&runs.to_le_bytes());
        d[12..16].copy_from_slice(&focus.to_le_bytes());
        d[60..68].copy_from_slice(&ft.to_le_bytes());
        d
    }

    #[test]
    fn parses_records() {
        let ft = (EPOCH_DIFF_SECS + 1_700_000_000) * 10_000_000;
        assert_eq!(parse_record(&rec(7, 1234, ft)), Some((7, 1234, 1_700_000_000)));
        assert_eq!(parse_record(&rec(1, 0, 0)), Some((1, 0, 0)));
        assert_eq!(parse_record(&rec(1, 0, u64::MAX)), Some((1, 0, 0)));
        assert_eq!(parse_record(&[0u8; 16]), None);
        assert_eq!(parse_record(&[]), None);
        assert_eq!(parse_record(&[0u8; 73]), None);
    }

    #[test]
    fn maps_guid_prefix() {
        let fake = |g: &str| match g {
            "{6D809377-6AF0-444B-8957-A3773F02200E}" => Some(r"C:\Program Files".to_string()),
            _ => None,
        };
        assert_eq!(map_known_folder(r"{6d809377-6af0-444b-8957-a3773f02200e}\App\a.exe", fake), r"C:\Program Files\App\a.exe");
        assert_eq!(map_known_folder(r"{AAAA}\x.exe", fake), r"{AAAA}\x.exe");
        assert_eq!(map_known_folder(r"D:\x.exe", fake), r"D:\x.exe");
        assert_eq!(map_known_folder("{broken", fake), "{broken");
    }

    #[test]
    fn last_used_prefix() {
        let e = |p: &str, t| ExeUsage { path: p.into(), runs: 1, focus_ms: 0, last_run: t };
        let v = vec![e(r"C:\Program Files\App\a.exe", 10), e(r"c:\program files\app\sub\b.exe", 30), e(r"C:\Program Files\Application\c.exe", 99)];
        assert_eq!(last_used_under(&v, r"C:\PROGRAM FILES\app"), Some(30));
        assert_eq!(last_used_under(&v, r"C:\Program Files\App\"), Some(30));
        assert_eq!(last_used_under(&v, r"C:\Other"), None);
        assert_eq!(last_used_under(&v, ""), None);
    }

    #[test]
    #[ignore]
    fn live_usage() {
        let u = usage();
        println!("entries={} with_last_run={}", u.len(), u.iter().filter(|e| e.last_run > 0).count());
        for e in u.iter().take(3) {
            println!("runs={} last_run={}", e.runs, e.last_run);
        }
    }
}
