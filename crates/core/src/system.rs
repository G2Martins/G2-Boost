//! Read-only system facts: elevation, OS version, memory, drives.
//! All via Win32; nothing here spawns a process.

use std::collections::HashSet;
use std::mem::size_of;

use serde::Serialize;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
};
use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW};
use windows::Win32::System::ProcessStatus::EnumProcesses;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX, OSVERSIONINFOW};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::core::{PCWSTR, PWSTR};

/// Lowercase exe names of running processes (best effort: protected processes are skipped).
/// Used to warn "close X first" and to detect games.
pub fn running_processes() -> HashSet<String> {
    running_process_paths().into_iter().filter_map(|p| p.rsplit('\\').next().map(|n| n.to_ascii_lowercase())).collect()
}

/// Every process id (the buffer grows until the list fits; a full buffer may be truncated).
pub fn pids() -> Vec<u32> {
    let mut pids = vec![0u32; 1024];
    let mut needed = 0u32;
    loop {
        let cb = (pids.len() * 4) as u32;
        // SAFETY: buffer of `cb` bytes; `needed` receives the bytes written.
        if unsafe { EnumProcesses(pids.as_mut_ptr(), cb, &mut needed) }.is_err() {
            return Vec::new();
        }
        if needed < cb {
            break;
        }
        pids.resize(pids.len() * 2, 0);
    }
    pids.truncate((needed / 4) as usize);
    pids
}

/// Full image paths of running processes we are allowed to query.
pub fn running_process_paths() -> Vec<String> {
    let mut out = Vec::new();
    for pid in pids() {
        if pid == 0 {
            continue;
        }
        // SAFETY: limited-information handle; closed below; buffer sized for the call.
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                continue;
            };
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
            let _ = CloseHandle(h);
            if ok {
                out.push(String::from_utf16_lossy(&buf[..len as usize]));
            }
        }
    }
    out
}

/// Is the current process running with an elevated (admin) token?
pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut info = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut info as *mut TOKEN_ELEVATION).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && info.TokenIsElevated != 0
    }
}

/// "Windows 11 (build 26200)" — from RtlGetVersion, which is not subject to manifest lies.
pub fn os_version() -> String {
    let mut v = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: struct is sized and zeroed; RtlGetVersion only writes into it.
    let status = unsafe { windows::Wdk::System::SystemServices::RtlGetVersion(&mut v) };
    if status.is_err() {
        return "Windows".into();
    }
    let name = if v.dwBuildNumber >= 22000 { "Windows 11" } else { "Windows 10" };
    format!("{name} (build {})", v.dwBuildNumber)
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryStatus {
    pub total_bytes: u64,
    pub avail_bytes: u64,
    pub load_percent: u32,
}

pub fn memory_status() -> Option<MemoryStatus> {
    let mut ms = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe { GlobalMemoryStatusEx(&mut ms).ok()? };
    Some(MemoryStatus {
        total_bytes: ms.ullTotalPhys,
        avail_bytes: ms.ullAvailPhys,
        load_percent: ms.dwMemoryLoad,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveKind {
    Fixed,
    Removable,
    Network,
    Cdrom,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct DriveInfo {
    pub letter: String,
    pub kind: DriveKind,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

/// Fixed + removable drives with their sizes (network/cdrom listed without sizes).
pub fn list_drives() -> Vec<DriveInfo> {
    let mut out = Vec::new();
    for c in b'A'..=b'Z' {
        let letter = format!("{}:", c as char);
        let root: Vec<u16> = format!("{letter}\\").encode_utf16().chain(std::iter::once(0)).collect();
        let root_ptr = PCWSTR::from_raw(root.as_ptr());
        // DRIVE_UNKNOWN = 0, DRIVE_NO_ROOT_DIR = 1, REMOVABLE = 2, FIXED = 3, REMOTE = 4, CDROM = 5, RAMDISK = 6
        let kind = match unsafe { GetDriveTypeW(root_ptr) } {
            0 | 1 => continue,
            2 => DriveKind::Removable,
            3 | 6 => DriveKind::Fixed,
            4 => DriveKind::Network,
            5 => DriveKind::Cdrom,
            _ => DriveKind::Other,
        };
        let (mut free, mut total) = (0u64, 0u64);
        if matches!(kind, DriveKind::Fixed | DriveKind::Removable) {
            // SAFETY: valid null-terminated wide string; out params are plain u64s.
            let _ = unsafe { GetDiskFreeSpaceExW(root_ptr, Some(&mut free), Some(&mut total), None) };
        }
        out.push(DriveInfo { letter, kind, total_bytes: total, free_bytes: free });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drives_include_system_drive() {
        let drives = list_drives();
        assert!(drives.iter().any(|d| d.letter.eq_ignore_ascii_case("C:")));
        let c = drives.iter().find(|d| d.letter.eq_ignore_ascii_case("C:")).unwrap();
        assert!(c.total_bytes > 0 && c.free_bytes <= c.total_bytes);
    }

    #[test]
    fn running_processes_include_this_test_binary() {
        let me = std::env::current_exe().unwrap();
        let name = me.file_name().unwrap().to_string_lossy().to_ascii_lowercase();
        assert!(running_processes().contains(&name));
    }

    #[test]
    fn memory_and_os_are_sane() {
        let m = memory_status().expect("GlobalMemoryStatusEx");
        assert!(m.total_bytes > 1 << 30 && m.avail_bytes <= m.total_bytes);
        assert!(os_version().starts_with("Windows"));
        let _ = is_elevated();
    }
}
