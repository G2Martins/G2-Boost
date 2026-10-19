//! "Who used your camera / microphone / location": the Windows privacy consent store.
//!
//! Source: `...\CurrentVersion\CapabilityAccessManager\ConsentStore\{webcam,microphone,location}`
//! under HKCU and HKLM. Each subkey is a Store app (package family name) except `NonPackaged`,
//! whose subkeys are exe paths with `#` instead of `\`. `LastUsedTimeStart`/`LastUsedTimeStop` are
//! FILETIME QWORDs (the registry helper has no QWORD support, so they are read here with
//! `RegGetValueW`). Read-only, no admin; every name and value is hostile input.

use serde::Serialize;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_QWORD, RegGetValueW};
use windows::core::PCWSTR;

use crate::registry::{Root, subkeys, wide};

const BASE: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore";
const MAX_STR: usize = 260;
/// FILETIME ticks (100 ns) between 1601-01-01 and 1970-01-01, in seconds.
const EPOCH_DIFF_SECS: u64 = 11_644_473_600;
/// Year 3000: anything later is garbage.
const MAX_UNIX: u64 = 32_503_680_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Webcam,
    Microphone,
    Location,
}

impl Capability {
    fn key(self) -> &'static str {
        match self {
            Capability::Webcam => "webcam",
            Capability::Microphone => "microphone",
            Capability::Location => "location",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AccessRecord {
    pub capability: Capability,
    /// Exe file stem, or the package family name before `_`.
    pub app: String,
    /// Decoded exe path (desktop apps) or the package family name (Store apps).
    pub path: String,
    pub packaged: bool,
    /// Unix seconds; 0 = never / absent.
    pub last_start: u64,
    pub last_stop: u64,
    /// Start newer than stop: the device is in use right now.
    pub in_use: bool,
}

/// FILETIME (100 ns since 1601) → unix seconds. 0, pre-1970 and absurd values → 0.
pub fn filetime_to_unix(ft: u64) -> u64 {
    let secs = ft / 10_000_000;
    match secs.checked_sub(EPOCH_DIFF_SECS) {
        Some(u) if u <= MAX_UNIX => u,
        _ => 0,
    }
}

/// `C:#Users#me#app.exe` → `C:\Users\me\app.exe`.
pub fn decode_path(sub: &str) -> String {
    sub.replace('#', "\\")
}

pub fn in_use(start_ft: u64, stop_ft: u64) -> bool {
    start_ft != 0 && start_ft > stop_ft
}

fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(MAX_STR).collect()
}

fn exe_stem(path: &str) -> String {
    let file = path.rsplit('\\').next().unwrap_or(path);
    match file.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => file.to_string(),
    }
}

fn package_name(pfn: &str) -> String {
    pfn.split('_').next().unwrap_or(pfn).to_string()
}

/// QWORD value; missing / wrong type / error → 0.
fn qword(root: Root, path: &str, name: &str) -> u64 {
    let hk = match root {
        Root::Hkcu => HKEY_CURRENT_USER,
        Root::Hklm => HKEY_LOCAL_MACHINE,
    };
    let (p, n) = (wide(path), wide(name));
    let mut v = 0u64;
    let mut sz = 8u32;
    // SAFETY: valid null-terminated strings; output is an 8-byte u64 and `sz` says so.
    let e = unsafe {
        RegGetValueW(hk, PCWSTR(p.as_ptr()), PCWSTR(n.as_ptr()), RRF_RT_REG_QWORD, None, Some((&mut v as *mut u64).cast()), Some(&mut sz))
    };
    if e == ERROR_SUCCESS { v } else { 0 }
}

fn record(root: Root, cap: Capability, sub_path: &str, app: String, path: String, packaged: bool) -> AccessRecord {
    let start = qword(root, sub_path, "LastUsedTimeStart");
    let stop = qword(root, sub_path, "LastUsedTimeStop");
    AccessRecord {
        capability: cap,
        app: clean(&app),
        path: clean(&path),
        packaged,
        last_start: filetime_to_unix(start),
        last_stop: filetime_to_unix(stop),
        in_use: in_use(start, stop),
    }
}

/// Every app that ever asked for camera / microphone / location, most recently used first.
pub fn history() -> Vec<AccessRecord> {
    let mut out = Vec::new();
    for root in [Root::Hkcu, Root::Hklm] {
        for cap in [Capability::Webcam, Capability::Microphone, Capability::Location] {
            let base = format!(r"{BASE}\{}", cap.key());
            for name in subkeys(root, &base).unwrap_or_default() {
                if name.eq_ignore_ascii_case("NonPackaged") {
                    let np = format!(r"{base}\NonPackaged");
                    for exe in subkeys(root, &np).unwrap_or_default() {
                        let path = decode_path(&exe);
                        out.push(record(root, cap, &format!(r"{np}\{exe}"), exe_stem(&path), path, false));
                    }
                } else {
                    out.push(record(root, cap, &format!(r"{base}\{name}"), package_name(&name), name.clone(), true));
                }
            }
        }
    }
    out.sort_by(|a, b| b.last_start.max(b.last_stop).cmp(&a.last_start.max(a.last_stop)).then_with(|| a.app.cmp(&b.app)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_paths() {
        assert_eq!(decode_path("C:#Users#me#My App#zoom.exe"), r"C:\Users\me\My App\zoom.exe");
        assert_eq!(exe_stem(r"C:\x\zoom.exe"), "zoom");
        assert_eq!(exe_stem(r"C:\x\.hidden"), ".hidden");
        assert_eq!(package_name("Microsoft.WindowsCamera_8wekyb3d8bbwe"), "Microsoft.WindowsCamera");
    }

    #[test]
    fn filetime_conversion() {
        assert_eq!(filetime_to_unix(0), 0);
        assert_eq!(filetime_to_unix(u64::MAX), 0);
        assert_eq!(filetime_to_unix(5), 0);
        assert_eq!(filetime_to_unix(116_444_736_000_000_000), 0); // exactly 1970
        assert_eq!(filetime_to_unix(116_444_736_000_000_000 + 10_000_000 * 86_400), 86_400);
    }

    #[test]
    fn in_use_logic() {
        assert!(in_use(10, 0));
        assert!(in_use(10, 5));
        assert!(!in_use(5, 10));
        assert!(!in_use(10, 10));
        assert!(!in_use(0, 0));
    }

    #[test]
    #[ignore]
    fn live_history() {
        let h = history();
        println!("records={} in_use={}", h.len(), h.iter().filter(|r| r.in_use).count());
        for cap in [Capability::Webcam, Capability::Microphone, Capability::Location] {
            println!("{cap:?}: {}", h.iter().filter(|r| r.capability == cap).count());
        }
        for r in h.iter().take(3) {
            println!("{} start={} stop={}", r.app, r.last_start, r.last_stop);
        }
    }
}
