//! "Recommended for this PC": hardware-aware rules over the tweak catalog. Pure logic on
//! [`Facts`] (tested with fake profiles); `facts()` gathers them from the live machine.
//! Evidence behind the rules: Docs/references/windows-services-research.md.

use std::path::PathBuf;

use serde::Serialize;

use crate::boost::{self, Status, TweakState};
use crate::hardware;
use crate::registry::{self, Root, Value};

#[derive(Debug, Clone, Default, Serialize)]
pub struct Facts {
    /// `None` = unknown (no seek-penalty answer): treated as "not an SSD" for safety.
    pub system_ssd: Option<bool>,
    pub laptop: bool,
    pub gpu_vendor: Option<String>,
    /// `HwSchMode` exists under GraphicsDrivers → the driver knows about HAGS.
    pub hags_supported: bool,
    pub game_pass: bool,
    pub outlook: bool,
    /// Build 22000+ (the windowed-games optimization only exists there).
    pub windows11: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Recommendation {
    pub id: &'static str,
    pub recommended: bool,
    /// i18n reason code when not recommended: `hdd`, `unknown_disk`, `laptop`, `hags_unsupported`,
    /// `game_pass`, `outlook`, `beta`, `preference`, `security`.
    pub reason: Option<&'static str>,
}

pub fn facts() -> Facts {
    facts_for(&hardware::profile())
}

/// Same, from a profile the caller already has (the profile is the slow part: SMBIOS, DXGI, IOCTLs).
pub fn facts_for(p: &hardware::Profile) -> Facts {
    let packages = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).map(|l| l.join("Packages"));
    let game_pass = packages
        .as_ref()
        .map(|pk| ["Microsoft.GamingApp_8wekyb3d8bbwe", "Microsoft.XboxApp_8wekyb3d8bbwe"].iter().any(|n| pk.join(n).is_dir()))
        .unwrap_or(false);
    let outlook = matches!(registry::read(Root::Hklm, r"SOFTWARE\Clients\Mail\Microsoft Outlook", ""), Ok(Value::Str(_)))
        || matches!(registry::read(Root::Hklm, r"SOFTWARE\Microsoft\Office\ClickToRun\Configuration", "Platform"), Ok(Value::Str(_)));
    Facts {
        system_ssd: p.system_disk_ssd,
        laptop: p.laptop,
        gpu_vendor: p.gpus.first().map(|g| g.vendor.to_string()),
        hags_supported: !matches!(
            registry::read(Root::Hklm, r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers", "HwSchMode"),
            Ok(Value::Absent) | Err(_)
        ),
        game_pass,
        outlook,
        windows11: matches!(
            registry::read(Root::Hklm, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "CurrentBuildNumber"),
            Ok(Value::Str(b)) if b.parse::<u32>().is_ok_and(|n| n >= 22_000)
        ),
    }
}

/// One entry per catalog tweak, in catalog order.
pub fn recommend(f: &Facts) -> Vec<Recommendation> {
    boost::TWEAKS
        .iter()
        .map(|t| {
            let (recommended, reason) = match t.id {
                "svc_sysmain" => match f.system_ssd {
                    Some(true) => (true, None),
                    Some(false) => (false, Some("hdd")),
                    None => (false, Some("unknown_disk")),
                },
                "power_plan" if f.laptop => (false, Some("laptop")),
                "hags" if !f.hags_supported => (false, Some("hags_unsupported")),
                "svc_xbox" if f.game_pass => (false, Some("game_pass")),
                "svc_xbox" => (false, Some("preference")),
                "svc_search" if f.outlook => (false, Some("outlook")),
                "mouse" => (false, Some("preference")),
                "windowed_optimizations" if !f.windows11 => (false, Some("windows10")),
                "hvci_off" => (false, Some("security")),
                "svc_dps" | "svc_geolocation" => (false, Some("preference")),
                _ if t.beta => (false, Some("beta")),
                _ => (true, None),
            };
            Recommendation { id: t.id, recommended, reason }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct Score {
    pub recommended: u32,
    pub active: u32,
    /// 0–100; 100 when nothing is recommended.
    pub percent: u32,
    /// Recommended tweaks that are not fully on yet.
    pub pending: Vec<&'static str>,
}

pub fn score(recs: &[Recommendation], states: &[TweakState]) -> Score {
    let mut recommended = 0u32;
    let mut active = 0u32;
    let mut pending = Vec::new();
    for r in recs.iter().filter(|r| r.recommended) {
        let Some(st) = states.iter().find(|s| s.id == r.id) else {
            continue;
        };
        if st.status == Status::Unavailable {
            continue;
        }
        recommended += 1;
        if st.status == Status::On {
            active += 1;
        } else {
            pending.push(r.id);
        }
    }
    let percent = (active * 100).checked_div(recommended).unwrap_or(100);
    Score { recommended, active, percent, pending }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn by<'a>(recs: &'a [Recommendation], id: &str) -> &'a Recommendation {
        recs.iter().find(|r| r.id == id).unwrap()
    }

    #[test]
    fn rules_follow_the_hardware() {
        let ssd_desktop = Facts { system_ssd: Some(true), hags_supported: true, windows11: true, ..Default::default() };
        let r = recommend(&ssd_desktop);
        assert_eq!(r.len(), boost::TWEAKS.len());
        assert!(by(&r, "svc_sysmain").recommended);
        assert!(by(&r, "power_plan").recommended);
        assert!(by(&r, "hags").recommended);
        assert!(by(&r, "svc_search").recommended);
        assert!(!by(&r, "svc_xbox").recommended, "zero gain, real side effects: custom only");
        assert_eq!(by(&r, "hvci_off").reason, Some("security"));
        assert_eq!(by(&r, "perf_counters").reason, Some("beta"));
        assert!(by(&r, "windowed_optimizations").recommended);

        let hdd_laptop = Facts { system_ssd: Some(false), laptop: true, game_pass: true, outlook: true, ..Default::default() };
        let r = recommend(&hdd_laptop);
        assert_eq!(by(&r, "svc_sysmain").reason, Some("hdd"));
        assert_eq!(by(&r, "power_plan").reason, Some("laptop"));
        assert_eq!(by(&r, "hags").reason, Some("hags_unsupported"));
        assert_eq!(by(&r, "svc_xbox").reason, Some("game_pass"));
        assert_eq!(by(&r, "svc_search").reason, Some("outlook"));
        assert_eq!(by(&r, "windowed_optimizations").reason, Some("windows10"));
        assert_eq!(recommend(&Facts::default()).iter().find(|r| r.id == "svc_sysmain").unwrap().reason, Some("unknown_disk"));
    }

    #[test]
    fn score_counts_only_recommended_and_available() {
        let recs = vec![
            Recommendation { id: "game_mode", recommended: true, reason: None },
            Recommendation { id: "tips", recommended: true, reason: None },
            Recommendation { id: "mouse", recommended: false, reason: Some("preference") },
            Recommendation { id: "svc_xbox", recommended: true, reason: None },
        ];
        let st = |id: &'static str, status: Status| TweakState {
            id,
            admin: false,
            beta: false,
            category: boost::Category::Performance,
            status,
            has_backup: false,
            level: None,
            detail: None,
            services: Vec::new(),
        };
        let states = vec![st("game_mode", Status::On), st("tips", Status::Partial), st("mouse", Status::Off), st("svc_xbox", Status::Unavailable)];
        let s = score(&recs, &states);
        assert_eq!((s.recommended, s.active, s.percent), (2, 1, 50));
        assert_eq!(s.pending, vec!["tips"]);
        assert_eq!(score(&[], &states).percent, 100);
    }
}
