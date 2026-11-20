//! Power saving of the network adapter in use: Energy Efficient Ethernet and the vendor "green"
//! modes, read from the adapter's driver key (the same values its Advanced tab edits). They save
//! a few watts on an idle link and can add latency spikes when traffic starts again.
//!
//! Turning them off (admin) backs up the values first, writes "0" and restarts the adapter
//! through `MSFT_NetAdapter.Restart()` (WMI, no `netsh`/PowerShell) so the driver reads them.

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::registry::{self, Backup, BackupEntry, Root, Value};
use crate::{system, wmi};

/// Backup id: the values before the first change, kept until restored.
const BACKUP: &str = "nicpower";
const NETADAPTER_NS: &str = r"ROOT\StandardCimv2";
/// How long `turn_off` waits for the link to come back after the restart.
const RELINK: Duration = Duration::from_secs(20);

#[derive(Debug, thiserror::Error)]
pub enum NicError {
    #[error("needs_admin")]
    NeedsAdmin,
    #[error("not_found")]
    NotFound,
    #[error("bad_backup")]
    BadBackup,
    #[error("{0}")]
    Other(String),
}

impl From<registry::RegError> for NicError {
    fn from(e: registry::RegError) -> Self {
        match e {
            registry::RegError::AccessDenied => NicError::NeedsAdmin,
            e => NicError::Other(e.to_string()),
        }
    }
}

/// Network adapters' device class.
const CLASS: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e972-e325-11ce-bfc1-08002be10318}";

/// Driver keywords that mean "save power on the link", with "1" = on and "0" = off.
pub const KEYWORDS: &[&str] = &["*EEE", "AdvancedEEE", "EnableGreenEthernet", "PowerSavingMode", "GigaLite"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NicPower {
    /// `{GUID}` of the adapter (its NetCfgInstanceId).
    pub guid: String,
    /// Driver description, e.g. "Realtek Gaming 2.5GbE Family Controller".
    pub name: String,
    /// The [`KEYWORDS`] this driver has that are on now.
    pub on: Vec<&'static str>,
}

/// Driver keywords are REG_SZ "1"/"0" (a few drivers write a DWORD).
fn is_on(v: &Value) -> bool {
    match v {
        Value::Str(s) => s.trim() == "1",
        Value::Dword(d) => *d == 1,
        _ => false,
    }
}

/// `0001`-style driver key of the adapter `guid` under the class.
fn driver_key(guid: &str) -> Option<String> {
    registry::subkeys(Root::Hklm, CLASS).ok()?.into_iter().find_map(|sub| {
        if sub.len() != 4 || !sub.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let path = format!(r"{CLASS}\{sub}");
        matches!(registry::read(Root::Hklm, &path, "NetCfgInstanceId"), Ok(Value::Str(id)) if id.eq_ignore_ascii_case(guid)).then_some(path)
    })
}

/// Power saving of the adapter `guid` (`None` = no driver key for it, e.g. a virtual adapter).
pub fn for_adapter(guid: &str) -> Option<NicPower> {
    let path = driver_key(guid)?;
    let name = match registry::read(Root::Hklm, &path, "DriverDesc") {
        Ok(Value::Str(s)) => s,
        _ => String::new(),
    };
    let on = KEYWORDS.iter().copied().filter(|k| registry::read(Root::Hklm, &path, k).is_ok_and(|v| is_on(&v))).collect();
    Some(NicPower { guid: guid.to_string(), name, on })
}

/// The adapter Windows sends internet traffic through, when it has a driver key.
pub fn primary() -> Option<NicPower> {
    crate::dns::adapters().into_iter().find(|a| a.primary).and_then(|a| for_adapter(&a.guid))
}

pub fn has_backup() -> bool {
    registry::load_backup(BACKUP).is_some()
}

/// `{8-4-4-4-12}` hex GUID: the only text that ever reaches the WQL below.
fn valid_guid(g: &str) -> bool {
    g.strip_prefix('{').and_then(|g| g.strip_suffix('}')).is_some_and(crate::updates::valid_guid)
}

/// Turn the link power saving of adapter `guid` off (admin): back up the current values (each
/// value only the first time it is seen, so the backup always holds the originals, for every
/// adapter turned off), write "0", restart the adapter and wait for the link to come back.
pub fn turn_off(guid: &str) -> Result<NicPower, NicError> {
    if !valid_guid(guid) {
        return Err(NicError::NotFound);
    }
    if !system::is_elevated() {
        return Err(NicError::NeedsAdmin);
    }
    let path = driver_key(guid).ok_or(NicError::NotFound)?;
    let mut present = Vec::new();
    for k in KEYWORDS {
        let v = registry::read(Root::Hklm, &path, k)?;
        if v != Value::Absent {
            present.push(BackupEntry { root: Root::Hklm, path: path.clone(), name: (*k).to_string(), value: v });
        }
    }
    let mut backup = registry::load_backup(BACKUP).unwrap_or(Backup { tweak: BACKUP.into(), taken_at: registry::now_unix(), entries: Vec::new() });
    let before = backup.entries.len();
    for e in &present {
        if !backup.entries.iter().any(|b| b.path.eq_ignore_ascii_case(&e.path) && b.name == e.name) {
            backup.entries.push(e.clone());
        }
    }
    if backup.entries.len() != before {
        registry::save_backup(&backup).map_err(|e| NicError::Other(e.to_string()))?;
    }
    for e in &present {
        registry::write(Root::Hklm, &path, &e.name, &Value::Str("0".into()))?;
    }
    restart(guid)?;
    for_adapter(guid).ok_or(NicError::NotFound)
}

/// Put the saved values back (admin), restart the adapter, drop the backup.
pub fn restore() -> Result<(), NicError> {
    if !system::is_elevated() {
        return Err(NicError::NeedsAdmin);
    }
    let b = registry::load_backup(BACKUP).ok_or(NicError::NotFound)?;
    // The file is user-writable: only our keywords under the network class may come back.
    let ours = |e: &BackupEntry| {
        e.root == Root::Hklm
            && KEYWORDS.contains(&e.name.as_str())
            && e.path.strip_prefix(CLASS).and_then(|s| s.strip_prefix('\\')).is_some_and(|k| k.len() == 4 && k.bytes().all(|b| b.is_ascii_digit()))
    };
    if !b.entries.iter().all(ours) {
        return Err(NicError::BadBackup);
    }
    for e in &b.entries {
        registry::write(e.root, &e.path, &e.name, &e.value)?;
    }
    let mut paths: Vec<&str> = b.entries.iter().map(|e| e.path.as_str()).collect();
    paths.sort_unstable();
    paths.dedup();
    for p in paths {
        if let Ok(Value::Str(id)) = registry::read(Root::Hklm, p, "NetCfgInstanceId")
            && valid_guid(&id)
        {
            restart(&id)?;
        }
    }
    registry::delete_backup(BACKUP);
    Ok(())
}

/// `MSFT_NetAdapter.Restart()` (disable + enable: how the driver picks up new Advanced values),
/// then wait until the adapter has its gateway again.
fn restart(guid: &str) -> Result<(), NicError> {
    let rows = wmi::query(NETADAPTER_NS, &format!("SELECT * FROM MSFT_NetAdapter WHERE DeviceID = '{guid}'"), &["__RELPATH"]).map_err(NicError::Other)?;
    let rel = rows.first().and_then(|r| r.get("__RELPATH")?.as_str().map(str::to_string)).ok_or(NicError::NotFound)?;
    match wmi::call(NETADAPTER_NS, &rel, "Restart").map_err(NicError::Other)? {
        0 => {}
        rv => return Err(NicError::Other(format!("restart {rv}"))),
    }
    let t0 = Instant::now();
    std::thread::sleep(Duration::from_secs(2)); // the old link is still listed for a moment
    while t0.elapsed() < RELINK {
        if crate::dns::adapters().iter().any(|a| a.guid.eq_ignore_ascii_case(guid) && a.gateway.is_some()) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Ok(()) // slow DHCP: the change is made, the link comes back on its own
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_values() {
        assert!(is_on(&Value::Str("1".into())));
        assert!(is_on(&Value::Str(" 1 ".into())));
        assert!(is_on(&Value::Dword(1)));
        assert!(!is_on(&Value::Str("0".into())));
        assert!(!is_on(&Value::Str("2".into())), "only the documented on value counts");
        assert!(!is_on(&Value::Absent));
    }

    #[test]
    fn reads_this_pc_without_admin() {
        // Live and read-only: the primary adapter, when it is a real one, has a driver key.
        if let Some(n) = primary() {
            assert!(n.guid.starts_with('{'));
            assert!(n.on.iter().all(|k| KEYWORDS.contains(k)));
            eprintln!("{} on: {:?}", n.name, n.on);
        }
        assert!(for_adapter("{00000000-0000-0000-0000-000000000000}").is_none());
    }

    #[test]
    fn only_a_plain_guid_reaches_wql() {
        assert!(valid_guid("{AD698D0B-2095-47FA-B355-ECDAEC8D9156}"));
        for bad in ["", "{AD698D0B-2095-47FA-B355-ECDAEC8D915}", "{AD698D0B-2095-47FA-B355-ECDAEC8D9156}' OR 1=1", "AD698D0B-2095-47FA-B355-ECDAEC8D9156ab", "{AD698D0B_2095-47FA-B355-ECDAEC8D9156}", "{ZZ698D0B-2095-47FA-B355-ECDAEC8D9156}"] {
            assert!(!valid_guid(bad), "{bad}");
        }
        assert!(matches!(turn_off("x' OR '1'='1"), Err(NicError::NotFound)));
    }
}
