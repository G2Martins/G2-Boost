//! System Restore: status + count through WMI (`root\default:SystemRestore`, COM, no
//! PowerShell) and point creation through `SRSetRestorePointW` (admin). Windows throttles
//! app-created points to one per `SystemRestorePointCreationFrequency` minutes (default 1440);
//! we report "skipped" instead of pretending.

use serde::Serialize;
use windows::Win32::System::Restore::{
    BEGIN_SYSTEM_CHANGE, END_SYSTEM_CHANGE, MODIFY_SETTINGS, RESTOREPOINTINFOW, SRSetRestorePointW, STATEMGRSTATUS,
};

use crate::registry::{self, Root, Value};
use crate::system;
use crate::tasksched::{Com, TaskError};
use crate::wmi;

const SR_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\SystemRestore";

#[derive(Debug, Clone, Serialize)]
pub struct RestoreStatus {
    /// System Protection on the system drive (None = could not tell).
    pub enabled: Option<bool>,
    pub count: u32,
    /// Newest point, "YYYY-MM-DD HH:MM" local, from WMI `CreationTime`.
    pub last: Option<String>,
    /// Minimum minutes between app-created points (Windows default 1440).
    pub frequency_minutes: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CreateOutcome {
    Created,
    /// Windows accepted the call but created nothing (24 h throttle).
    Skipped,
}

#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    #[error("needs_admin")]
    NeedsAdmin,
    #[error("protection_off")]
    ProtectionOff,
    #[error("{0}")]
    Com(#[from] TaskError),
    #[error("wmi: {0}")]
    Wmi(String),
    #[error("win32: {0}")]
    Win32(u32),
}

/// "20261014213000.000000-180" → "2026-10-14 21:30"
pub(crate) fn wmi_datetime(s: &str) -> Option<String> {
    if s.len() < 12 || !s.is_char_boundary(12) {
        return None;
    }
    let (y, m, d, hh, mm) = (&s[0..4], &s[4..6], &s[6..8], &s[8..10], &s[10..12]);
    if ![y, m, d, hh, mm].iter().all(|p| p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    Some(format!("{y}-{m}-{d} {hh}:{mm}"))
}

/// (count, newest CreationTime) from `SELECT CreationTime FROM SystemRestore`.
fn wmi_points() -> Result<(u32, Option<String>), RestoreError> {
    let rows = wmi::query(r"ROOT\DEFAULT", "SELECT CreationTime FROM SystemRestore", &["CreationTime"]).map_err(RestoreError::Wmi)?;
    let newest = rows
        .iter()
        .filter_map(|r| r.get("CreationTime").and_then(|v| v.as_str()).map(str::to_string))
        .max();
    Ok((rows.len() as u32, newest.and_then(|s| wmi_datetime(&s))))
}

pub fn status() -> RestoreStatus {
    let enabled = match (registry::read(Root::Hklm, SR_KEY, "RPSessionInterval"), registry::read(Root::Hklm, SR_KEY, "DisableSR")) {
        (_, Ok(Value::Dword(1))) => Some(false),
        (Ok(Value::Dword(v)), _) => Some(v != 0),
        _ => None,
    };
    let frequency_minutes = match registry::read(Root::Hklm, SR_KEY, "SystemRestorePointCreationFrequency") {
        Ok(Value::Dword(m)) => m,
        _ => 1440,
    };
    let (count, last) = wmi_points().unwrap_or((0, None));
    RestoreStatus { enabled, count, last, frequency_minutes }
}

/// Create a "modify settings" restore point. Admin only; the description is clipped to 255 chars.
pub fn create(description: &str) -> Result<CreateOutcome, RestoreError> {
    if !system::is_elevated() {
        return Err(RestoreError::NeedsAdmin);
    }
    let before = wmi_points().map(|(c, _)| c).unwrap_or(0);
    let _com = Com::init()?;
    let mut info = RESTOREPOINTINFOW { dwEventType: BEGIN_SYSTEM_CHANGE, dwRestorePtType: MODIFY_SETTINGS, llSequenceNumber: 0, szDescription: [0; 256] };
    for (i, u) in description.encode_utf16().take(255).enumerate() {
        info.szDescription[i] = u;
    }
    let mut st = STATEMGRSTATUS::default();
    // SAFETY: fully initialised structs; the API only writes `st`.
    if !unsafe { SRSetRestorePointW(&info, &mut st) }.as_bool() {
        return Err(match st.nStatus.0 {
            1058 => RestoreError::ProtectionOff, // ERROR_SERVICE_DISABLED
            code => RestoreError::Win32(code),
        });
    }
    info.dwEventType = END_SYSTEM_CHANGE;
    info.llSequenceNumber = st.llSequenceNumber;
    let mut end = STATEMGRSTATUS::default();
    // SAFETY: as above; the END marker closes the change sequence.
    let _ = unsafe { SRSetRestorePointW(&info, &mut end) };
    let after = wmi_points().map(|(c, _)| c).unwrap_or(before);
    Ok(if after > before { CreateOutcome::Created } else { CreateOutcome::Skipped })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wmi_datetime_parses_and_rejects() {
        assert_eq!(wmi_datetime("20261014213000.000000-180").as_deref(), Some("2026-10-14 21:30"));
        assert!(wmi_datetime("garbage").is_none());
        assert!(wmi_datetime("2026101421XX00.000000-180").is_none());
    }

    #[test]
    fn status_reads_without_admin() {
        let s = status();
        assert!(s.frequency_minutes >= 1);
        let _ = (s.enabled, s.count, s.last);
        if !system::is_elevated() {
            assert!(matches!(create("G2 Boost test"), Err(RestoreError::NeedsAdmin)));
        }
    }
}
