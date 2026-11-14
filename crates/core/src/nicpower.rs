//! Power saving of the network adapter in use: Energy Efficient Ethernet and the vendor "green"
//! modes, read from the adapter's driver key (the same values its Advanced tab edits). They save
//! a few watts on an idle link and can add latency spikes when traffic starts again.

use serde::Serialize;

use crate::registry::{self, Root, Value};

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
}
