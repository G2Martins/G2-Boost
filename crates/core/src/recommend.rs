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
    /// A real printer (local or connection, not PDF/XPS/OneNote/Fax) is installed.
    /// `None` = the list could not be read (the Print Spooler is off, for one).
    pub printers: Option<bool>,
    /// Keyboard layouts loaded at sign-in (`Keyboard Layout\Preload`): Alt+Shift only switches
    /// something when there are two or more.
    pub layouts: u32,
    /// Ryzen X3D with two CCDs (7900X3D, 7950X3D, 9900X3D, 9950X3D): the chipset driver parks the
    /// cache-less CCD in games, so core parking must stay on.
    pub x3d_dual_ccd: bool,
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
        windows11: boost::windows11(),
        printers: real_printers(),
        layouts: registry::value_names(Root::Hkcu, r"Keyboard Layout\Preload").map_or(0, |v| v.len() as u32),
        x3d_dual_ccd: is_x3d_dual_ccd(&p.cpu, p.cores_physical),
    }
}

pub(crate) fn is_x3d_dual_ccd(cpu: &str, cores: u32) -> bool {
    cpu.to_ascii_uppercase().contains("X3D") && cores > 8
}

/// Tweaks that change how Windows looks or behaves, with no gain to measure: offered, never
/// counted as pending in the score.
const PREFERENCE: &[&str] = &[
    "mouse", "svc_dps", "svc_geolocation", "teams_chat_off", "copilot_off", "recall_off", "click_to_do_off",
    "recall_feature_off", "paint_ai_off", "notepad_ai_off", "edge_ai_off", "delivery_p2p_off", "spotlight_off",
    "ntfs_last_access_off", "storage_sense_off", "network_throttling_off", "transparency_off", "classic_context_menu",
    "taskbar_left", "notifications_off", "error_reporting_off", "narrator_shortcut_off",
];

/// Printers that print to a file or app, not paper: they need the Spooler but no device.
pub(crate) fn is_virtual_printer(name: &str) -> bool {
    let n = name.to_lowercase();
    ["pdf", "xps", "onenote", "fax"].iter().any(|v| n.contains(v))
}

/// Local printers and connections through the Spooler (`EnumPrintersW`, level 4).
fn real_printers() -> Option<bool> {
    use windows::Win32::Graphics::Printing::{EnumPrintersW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_INFO_4W};
    use windows::core::PCWSTR;
    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let (mut needed, mut count) = (0u32, 0u32);
    // SAFETY: size probe; succeeds only when there is nothing to list.
    match unsafe { EnumPrintersW(flags, PCWSTR::null(), 4, None, &mut needed, &mut count) } {
        Ok(()) => return Some(false),
        Err(_) if needed == 0 => return None, // Spooler stopped or RPC failure
        Err(_) => {}
    }
    // u64 storage keeps the PRINTER_INFO_4W array pointer-aligned.
    let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: the byte view covers `needed` bytes of `buf`.
    let bytes = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), needed as usize) };
    // SAFETY: buffer of the size Windows asked for.
    unsafe { EnumPrintersW(flags, PCWSTR::null(), 4, Some(bytes), &mut needed, &mut count) }.ok()?;
    // SAFETY: Windows wrote `count` structs at the start of the buffer; their strings live in it.
    let list = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<PRINTER_INFO_4W>(), count as usize) };
    Some(list.iter().any(|p| !is_virtual_printer(&unsafe { p.pPrinterName.to_string() }.unwrap_or_default())))
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
                "windowed_optimizations" if !f.windows11 => (false, Some("windows10")),
                "hvci_off" => (false, Some("security")),
                "svc_spooler" => match f.printers {
                    Some(false) => (true, None),
                    Some(true) => (false, Some("printer")),
                    None => (false, Some("unknown")),
                },
                // Fast Startup is a half hibernation: it saves real seconds on a hard disk, next to
                // nothing on an SSD, and keeps stale driver state between "shutdowns".
                "fast_startup_off" => match f.system_ssd {
                    Some(true) => (true, None),
                    Some(false) => (false, Some("hdd")),
                    None => (false, Some("unknown_disk")),
                },
                "alt_shift_off" if f.layouts < 2 => (false, Some("single_layout")),
                "core_parking_off" if f.laptop => (false, Some("laptop")),
                "core_parking_off" if f.x3d_dual_ccd => (false, Some("x3d")),
                "core_parking_off" => (false, Some("preference")),
                "wifi_power_saving_off" if f.laptop => (false, Some("laptop")),
                "wifi_power_saving_off" => (false, Some("preference")),
                id if PREFERENCE.contains(&id) => (false, Some("preference")),
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
    fn spooler_only_without_real_printers() {
        let no_printer = Facts { printers: Some(false), ..Default::default() };
        assert!(by(&recommend(&no_printer), "svc_spooler").recommended);
        assert_eq!(by(&recommend(&Facts { printers: Some(true), ..Default::default() }), "svc_spooler").reason, Some("printer"));
        assert_eq!(by(&recommend(&Facts::default()), "svc_spooler").reason, Some("unknown"), "list unreadable (Spooler off)");
        assert!(by(&recommend(&no_printer), "winsat_task_off").recommended);
        assert!(by(&recommend(&no_printer), "feedback_tasks_off").recommended);
        for v in ["Microsoft Print to PDF", "Microsoft XPS Document Writer", "OneNote (Desktop)", "Fax", "Enviar para o OneNote 2016"] {
            assert!(is_virtual_printer(v), "{v}");
        }
        for real in ["HP LaserJet Pro M404", "EPSON L3250 Series", r"\\office\Brother HL-L2350DW"] {
            assert!(!is_virtual_printer(real), "{real}");
        }
    }

    #[test]
    fn round9_rules_keep_the_score_meaningful() {
        let desktop = Facts { system_ssd: Some(true), windows11: true, layouts: 2, ..Default::default() };
        let r = recommend(&desktop);
        for id in ["edge_background_off", "consumer_features_off", "telemetry_min", "accessibility_shortcuts_off", "fast_startup_off", "alt_shift_off"] {
            assert!(by(&r, id).recommended, "{id}");
        }
        for id in ["copilot_off", "recall_feature_off", "classic_context_menu", "taskbar_left", "notifications_off", "error_reporting_off"] {
            assert_eq!(by(&r, id).reason, Some("preference"), "{id}: a taste, not a gain");
        }
        // Adding the debloat catalog must not bury the score: only a handful become "pending".
        let start = boost::TWEAKS.iter().position(|t| t.id == "edge_background_off").unwrap();
        let new_recommended = r[start..].iter().filter(|x| x.recommended).count();
        assert!(new_recommended <= 12, "{new_recommended} new recommendations");

        let hdd = recommend(&Facts { system_ssd: Some(false), layouts: 1, ..Default::default() });
        assert_eq!(by(&hdd, "fast_startup_off").reason, Some("hdd"));
        assert_eq!(by(&hdd, "alt_shift_off").reason, Some("single_layout"));
        let x3d = recommend(&Facts { x3d_dual_ccd: true, ..Default::default() });
        assert_eq!(by(&x3d, "core_parking_off").reason, Some("x3d"));
        let laptop = recommend(&Facts { laptop: true, ..Default::default() });
        assert_eq!(by(&laptop, "core_parking_off").reason, Some("laptop"));
        assert_eq!(by(&laptop, "wifi_power_saving_off").reason, Some("laptop"));
        assert_eq!(by(&r, "core_parking_off").reason, Some("preference"));

        assert!(is_x3d_dual_ccd("AMD Ryzen 9 7950X3D 16-Core Processor", 16));
        assert!(is_x3d_dual_ccd("AMD Ryzen 9 9900X3D 12-Core Processor", 12));
        assert!(!is_x3d_dual_ccd("AMD Ryzen 7 7800X3D 8-Core Processor", 8), "one CCD");
        assert!(!is_x3d_dual_ccd("AMD Ryzen 9 7950X 16-Core Processor", 16));
        assert!(!is_x3d_dual_ccd("13th Gen Intel(R) Core(TM) i9-13900K", 24));
    }

    #[test]
    fn live_printer_probe_answers() {
        // Any answer is fine (no printer / printer / Spooler off); it must not crash or hang.
        let _ = real_printers();
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
