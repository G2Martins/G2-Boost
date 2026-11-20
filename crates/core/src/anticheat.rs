//! "Pronto para jogar com anti-cheat" (round 11): which installed games ship a kernel anti-cheat
//! with firmware requirements, and whether this PC meets them. Read-only: no admin needed (an
//! elevated run also confirms the TPM is enabled and activated), nothing spawned.
//!
//! Requirements (2026-11, Docs/boost/anticheat.md): Riot Vanguard (Valorant, League of Legends)
//! needs TPM 2.0 + Secure Boot on Windows 11; its on-demand mode also needs 25H2+, VBS, memory
//! integrity and the IOMMU. FACEIT needs TPM 2.0 + Secure Boot + Windows 11 and lists VBS and the
//! IOMMU as protections. Battlefield 6 (Javelin) and Call of Duty BO7/Warzone (Ricochet) need
//! Secure Boot + TPM 2.0. Secure Boot itself needs UEFI without CSM, and UEFI boots Windows from a
//! GPT disk only. Windows cannot switch any of this on in consumer firmware: the app detects,
//! guides with the board maker's menu paths, restarts into the setup on request and re-checks on
//! the next start (`recheck`).
//!
//! Game names come from Steam manifests and Uninstall entries: hostile data, cleaned and shown as
//! text only.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::firmware::{self, Firmware, Guide};
use crate::hardware::Profile;
use crate::registry::{self, Root, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    /// TPM 2.0 present (TBS sees it) and, when readable, enabled and activated.
    Tpm2,
    SecureBoot,
    /// UEFI boot, CSM off.
    Uefi,
    /// System disk GPT.
    Gpt,
    /// Virtualization-based security running.
    Vbs,
    /// Memory integrity (HVCI) running.
    Hvci,
    /// DMA protection available: Intel VT-d / AMD-Vi on in the firmware.
    Iommu,
    Win11,
    /// Windows 11 25H2 (build 26200) or later.
    Win25h2,
    /// Microsoft's vulnerable driver blocklist on.
    Blocklist,
}

use Check::{Blocklist, Gpt, Hvci, Iommu, SecureBoot, Tpm2, Uefi, Vbs, Win11, Win25h2};

/// Display order: what the firmware needs first, then Windows.
pub const CHECKS: [Check; 10] = [Uefi, Gpt, SecureBoot, Tpm2, Vbs, Hvci, Iommu, Win11, Win25h2, Blocklist];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Ok,
    Missing,
    Unknown,
}

/// What Windows tells without a driver (`None` = could not tell).
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// "2.0" / "1.2"; `None` = TBS sees no TPM (absent or off in the firmware).
    pub tpm: Option<String>,
    /// `Win32_Tpm` enabled + activated (admin only).
    pub tpm_ready: Option<bool>,
    pub secure_boot: Option<bool>,
    pub boot_mode: Option<&'static str>,
    pub system_gpt: Option<bool>,
    pub vbs: Option<bool>,
    pub hvci: Option<bool>,
    pub iommu: Option<bool>,
    /// VT-x / AMD-V on (VBS cannot run without it).
    pub virtualization: bool,
    /// Windows build; 0 = unknown.
    pub build: u32,
    /// `CI\Config\VulnerableDriverBlocklistEnable` (`None` = not set).
    pub blocklist_reg: Option<u32>,
}

fn known(b: Option<bool>) -> State {
    match b {
        Some(true) => State::Ok,
        Some(false) => State::Missing,
        None => State::Unknown,
    }
}

/// Pure.
pub(crate) fn state(c: Check, f: &Facts) -> State {
    let uefi = f.boot_mode == Some("uefi");
    let legacy = f.boot_mode == Some("legacy");
    match c {
        Tpm2 => match f.tpm.as_deref() {
            Some("2.0") if f.tpm_ready != Some(false) => State::Ok,
            _ => State::Missing,
        },
        // In legacy mode the Secure Boot state key may be missing altogether: it is off.
        SecureBoot => match f.secure_boot {
            None if legacy => State::Missing,
            b => known(b),
        },
        Uefi => known(f.boot_mode.map(|_| uefi)),
        Gpt => known(f.system_gpt.or(uefi.then_some(true))),
        Vbs => known(f.vbs),
        Hvci => known(f.hvci),
        Iommu => known(f.iommu),
        Win11 | Win25h2 if f.build == 0 => State::Unknown,
        Win11 => known(Some(f.build >= 22_000)),
        Win25h2 => known(Some(f.build >= 26_200)),
        // On by default since Windows 11 22H2 (build 22621); memory integrity enforces it anyway.
        Blocklist => match f.blocklist_reg {
            _ if f.hvci == Some(true) => State::Ok,
            Some(0) => State::Missing,
            Some(_) => State::Ok,
            None if f.build >= 22_621 => State::Ok,
            None if f.build == 0 => State::Unknown,
            None => State::Missing,
        },
    }
}

/// Where the fix lives: a BIOS guide item (`bios.item.*`, menu path per maker) or a Windows step
/// (`anticheat.fix.*`).
pub(crate) fn fix(c: Check, f: &Facts) -> (Option<&'static str>, Option<&'static str>) {
    match c {
        Tpm2 => (Some("tpm"), None),
        SecureBoot => (Some("secure_boot"), None),
        Uefi => (Some("csm"), None),
        Gpt => (None, Some("mbr2gpt")),
        Vbs if !f.virtualization => (Some("virtualization"), Some("core_isolation")),
        Vbs | Hvci => (None, Some("core_isolation")),
        Iommu => (Some("iommu"), None),
        Win11 | Win25h2 => (None, Some("update")),
        Blocklist => (None, Some("blocklist")),
    }
}

/* ---------------- titles ---------------- */

pub struct Title {
    /// `valorant`, `lol`, `faceit`, `bf6`, `cod` (i18n `anticheat.title.*`).
    pub id: &'static str,
    /// `vanguard`, `faceit`, `javelin`, `ricochet` (i18n `anticheat.ac.*`).
    pub anticheat: &'static str,
    /// Without these the anti-cheat refuses to start.
    pub required: &'static [Check],
    /// Listed by the anti-cheat as protections it uses.
    pub recommended: &'static [Check],
    /// Vanguard's on-demand mode (the driver loads with the game, not at boot).
    pub on_demand: &'static [Check],
}

const FIRMWARE: &[Check] = &[Uefi, Gpt, SecureBoot, Tpm2];
const VANGUARD_ON_DEMAND: &[Check] = &[Win25h2, SecureBoot, Tpm2, Vbs, Hvci, Iommu];

pub const TITLES: &[Title] = &[
    Title { id: "valorant", anticheat: "vanguard", required: FIRMWARE, recommended: &[], on_demand: VANGUARD_ON_DEMAND },
    Title { id: "lol", anticheat: "vanguard", required: FIRMWARE, recommended: &[], on_demand: VANGUARD_ON_DEMAND },
    Title { id: "faceit", anticheat: "faceit", required: &[Uefi, Gpt, SecureBoot, Tpm2, Win11], recommended: &[Vbs, Iommu], on_demand: &[] },
    Title { id: "bf6", anticheat: "javelin", required: FIRMWARE, recommended: &[], on_demand: &[] },
    Title { id: "cod", anticheat: "ricochet", required: FIRMWARE, recommended: &[], on_demand: &[] },
];

/// Steam appids: Battlefield 6, Call of Duty HQ (Black Ops 7 and Warzone install through it).
const STEAM_BF6: u32 = 2_807_960;
const STEAM_COD_HQ: u32 = 1_938_090;

/// Lowercase ASCII letters and digits only: "Battlefield™ 6" → "battlefield6".
fn norm(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

/// A name fit to show and to put in a notification: no control or bidi formatting characters, ≤ 80.
pub(crate) fn display(s: &str) -> String {
    let bidi = |c: &char| matches!(*c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
    s.chars().filter(|c| !c.is_control() && !bidi(c)).take(80).collect::<String>().trim().to_string()
}

fn title_of_app(name: &str, publisher: &str) -> Option<&'static str> {
    let (n, p) = (norm(name), norm(publisher));
    let riot = p.contains("riotgames");
    if riot && n == "valorant" {
        Some("valorant")
    } else if riot && n.starts_with("leagueoflegends") {
        Some("lol")
    } else if n.starts_with("faceit") && p.contains("faceit") {
        Some("faceit")
    } else if n.contains("battlefield6") && (p.contains("electronicarts") || p == "ea") {
        Some("bf6")
    } else if cod_name(&n) && (p.contains("activision") || p.contains("blizzard")) {
        Some("cod")
    } else {
        None
    }
}

/// The current Call of Duty (HQ launcher, Black Ops 7, Warzone); older standalone titles use no
/// firmware checks and keep their own names ("Call of Duty: Black Ops III").
fn cod_name(n: &str) -> bool {
    n == "callofduty" || n == "callofdutyhq" || n.contains("blackops7") || n.contains("warzone")
}

fn title_of_steam(appid: u32, name: &str) -> Option<&'static str> {
    let n = norm(name);
    if appid == STEAM_BF6 || n.contains("battlefield6") {
        Some("bf6")
    } else if appid == STEAM_COD_HQ || cod_name(&n) {
        Some("cod")
    } else {
        None
    }
}

/// Pure: (title id, name as installed), first find per title, in `TITLES` order.
pub(crate) fn detect(steam: &[(u32, String)], apps: &[(String, String)]) -> Vec<(&'static str, String)> {
    let found: Vec<(&'static str, &str)> = steam
        .iter()
        .filter_map(|(id, n)| Some((title_of_steam(*id, n)?, n.as_str())))
        .chain(apps.iter().filter_map(|(n, p)| Some((title_of_app(n, p)?, n.as_str()))))
        .collect();
    TITLES
        .iter()
        .filter_map(|t| found.iter().find(|f| f.0 == t.id).map(|f| (t.id, display(f.1))))
        .filter(|(_, n)| !n.is_empty())
        .collect()
}

/* ---------------- readiness ---------------- */

#[derive(Debug, Clone, Serialize)]
pub struct CheckState {
    pub id: Check,
    pub state: State,
    /// BIOS guide item that fixes it (`bios.item.*`).
    pub bios: Option<&'static str>,
    /// Windows step that fixes it (`anticheat.fix.*`).
    pub windows: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TitleState {
    pub id: &'static str,
    pub anticheat: &'static str,
    /// The name it is installed under; `None` = not installed here.
    pub installed: Option<String>,
    pub required: &'static [Check],
    pub recommended: &'static [Check],
    pub on_demand: &'static [Check],
    /// Required checks that are missing.
    pub missing: Vec<Check>,
    /// All required checks ok (`None` = some could not be read).
    pub ready: Option<bool>,
    /// Vanguard's on-demand mode possible (`None` for other anti-cheats, or unknown).
    pub on_demand_ok: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Readiness {
    pub checks: Vec<CheckState>,
    /// Every known title; `installed` tells which are on this PC.
    pub titles: Vec<TitleState>,
    /// Names of installed games whose anti-cheat uses VBS, memory integrity or the IOMMU (the
    /// `hvci_off` tweak warns about these).
    pub vbs_games: Vec<String>,
    /// BIOS steps for what is missing (for the installed games, or for every title when none is
    /// installed), with this board maker's menu paths.
    pub guide: Guide,
}

fn all_of(states: impl Iterator<Item = State>) -> Option<bool> {
    let v: Vec<State> = states.collect();
    if v.contains(&State::Missing) {
        Some(false)
    } else if v.contains(&State::Unknown) {
        None
    } else {
        Some(true)
    }
}

/// Pure: the readiness from the facts and what is installed. `vendor`/`amd` pick menu paths.
pub(crate) fn evaluate(f: &Facts, found: &[(&'static str, String)], vendor: firmware::Vendor, amd: bool) -> Readiness {
    let st = |c: Check| state(c, f);
    let checks = CHECKS.iter().map(|&c| {
        let (bios, windows) = fix(c, f);
        CheckState { id: c, state: st(c), bios, windows }
    });
    let titles: Vec<TitleState> = TITLES
        .iter()
        .map(|t| TitleState {
            id: t.id,
            anticheat: t.anticheat,
            installed: found.iter().find(|x| x.0 == t.id).map(|x| x.1.clone()),
            required: t.required,
            recommended: t.recommended,
            on_demand: t.on_demand,
            missing: t.required.iter().copied().filter(|&c| st(c) == State::Missing).collect(),
            ready: all_of(t.required.iter().map(|&c| st(c))),
            on_demand_ok: if t.on_demand.is_empty() { None } else { all_of(t.on_demand.iter().map(|&c| st(c))) },
        })
        .collect();
    let any = titles.iter().any(|t| t.installed.is_some());
    let relevant: Vec<&Title> = TITLES.iter().filter(|t| !any || found.iter().any(|x| x.0 == t.id)).collect();
    let vbs_games = titles
        .iter()
        .filter(|t| t.installed.is_some())
        .filter(|t| t.required.iter().chain(t.recommended).chain(t.on_demand).any(|c| matches!(c, Vbs | Hvci | Iommu)))
        .filter_map(|t| t.installed.clone())
        .collect();
    // BIOS items in the order to do them; required by a relevant title = not optional.
    let mut items: Vec<(&'static str, bool)> = Vec::new();
    for c in [Uefi, SecureBoot, Tpm2, Vbs, Iommu] {
        let Some(item) = fix(c, f).0 else { continue };
        let wanted = relevant.iter().any(|t| t.required.iter().chain(t.recommended).chain(t.on_demand).any(|&x| x == c));
        if st(c) == State::Missing && wanted && !items.iter().any(|i| i.0 == item) {
            items.push((item, !relevant.iter().any(|t| t.required.contains(&c))));
        }
    }
    Readiness {
        checks: checks.collect(),
        titles,
        vbs_games,
        guide: firmware::guide_with(vendor, amd, f.boot_mode == Some("uefi"), &items),
    }
}

/// `Win32_Tpm` enabled + activated. Only answers elevated; anything else is `None`.
fn tpm_ready() -> Option<bool> {
    let rows = crate::wmi::query(
        r"ROOT\CIMV2\Security\MicrosoftTpm",
        "SELECT IsEnabled_InitialValue, IsActivated_InitialValue FROM Win32_Tpm",
        &["IsEnabled_InitialValue", "IsActivated_InitialValue"],
    )
    .ok()?;
    let r = rows.first()?;
    let flag = |k: &str| match r.get(k) {
        Some(crate::wmi::WmiValue::Bool(b)) => Some(*b),
        _ => None,
    };
    Some(flag("IsEnabled_InitialValue")? && flag("IsActivated_InitialValue")?)
}

pub(crate) fn facts(p: &Profile, fw: &Firmware) -> Facts {
    Facts {
        tpm: p.tpm.clone(),
        tpm_ready: if p.tpm.is_some() { tpm_ready() } else { None },
        secure_boot: p.secure_boot,
        boot_mode: fw.boot_mode,
        system_gpt: fw.system_gpt,
        vbs: fw.vbs_running,
        hvci: fw.hvci_running,
        iommu: fw.iommu,
        virtualization: fw.virtualization_enabled,
        build: crate::boost::build(),
        blocklist_reg: match registry::read(Root::Hklm, r"SYSTEM\CurrentControlSet\Control\CI\Config", "VulnerableDriverBlocklistEnable") {
            Ok(Value::Dword(d)) => Some(d),
            _ => None,
        },
    }
}

/// Steam manifests + Uninstall entries (names and publishers only, no folder scan).
fn installed() -> Vec<(&'static str, String)> {
    let steam: Vec<(u32, String)> = crate::steam::installs().into_iter().map(|(g, _)| (g.appid, g.name)).collect();
    let apps: Vec<(String, String)> = crate::apps::desktop_list(&[]).into_iter().map(|a| (a.name, a.publisher)).collect();
    detect(&steam, &apps)
}

pub fn readiness(p: &Profile, fw: &Firmware) -> Readiness {
    evaluate(&facts(p, fw), &installed(), firmware::vendor_of(p.board.as_deref(), p.laptop), firmware::is_amd(p))
}

/// What an AI sees: states and game names, nothing else.
pub fn for_ai(r: &Readiness) -> serde_json::Value {
    let checks: BTreeMap<Check, State> = r.checks.iter().map(|c| (c.id, c.state)).collect();
    let games: Vec<_> = r
        .titles
        .iter()
        .filter_map(|t| {
            Some(serde_json::json!({ "game": t.installed.as_ref()?, "anticheat": t.anticheat, "ready": t.ready, "missing": t.missing, "on_demand_ok": t.on_demand_ok }))
        })
        .collect();
    serde_json::json!({ "checks": checks, "games": games })
}

/* ---------------- re-check after a boot ---------------- */

/// Last known states, saved in the data dir (`anticheat.json`). Hostile when read back.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub checks: BTreeMap<Check, State>,
    /// Installed title id → ready.
    #[serde(default)]
    pub ready: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    /// A requirement turned on (`on`) or off since the last start.
    Check { id: Check, on: bool },
    /// An installed game became ready, or stopped being ready.
    Game { name: String, ready: bool },
}

/// Pure: changes since `old`, and the snapshot to keep (an unknown reading keeps the last known
/// one, so a non-elevated start never looks like a change).
pub(crate) fn diff(old: &Snapshot, r: &Readiness) -> (Vec<Change>, Snapshot) {
    let mut out = Vec::new();
    let mut next = Snapshot::default();
    for c in &r.checks {
        let was = old.checks.get(&c.id).copied().filter(|s| *s != State::Unknown);
        let now = Some(c.state).filter(|s| *s != State::Unknown);
        if let (Some(w), Some(n)) = (was, now)
            && w != n
        {
            out.push(Change::Check { id: c.id, on: n == State::Ok });
        }
        if let Some(s) = now.or(was) {
            next.checks.insert(c.id, s);
        }
    }
    for t in r.titles.iter().filter(|t| t.installed.is_some()) {
        let now = t.ready.or_else(|| old.ready.get(t.id).copied());
        if let (Some(w), Some(n)) = (old.ready.get(t.id), t.ready)
            && *w != n
        {
            out.push(Change::Game { name: t.installed.clone().unwrap_or_default(), ready: n });
        }
        if let Some(n) = now {
            next.ready.insert(t.id.to_string(), n);
        }
    }
    (out, next)
}

fn snapshot_file() -> PathBuf {
    registry::data_dir().join("anticheat.json")
}

fn load_snapshot() -> Option<Snapshot> {
    let path = snapshot_file();
    if std::fs::metadata(&path).ok()?.len() > 64 * 1024 {
        return None;
    }
    let mut s: Snapshot = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    s.ready.retain(|id, _| TITLES.iter().any(|t| t.id == id));
    Some(s)
}

fn save_snapshot(s: &Snapshot) {
    let path = snapshot_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_string(s).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// A 2.0 TPM whose enabled/activated state was not readable (`Win32_Tpm` answers elevated only)
/// is a blind spot for the re-check: shown as ok on screen, but compared as unknown so that
/// switching between an elevated and a normal start never looks like a change.
pub(crate) fn blind_tpm(f: &Facts, r: &mut Readiness) {
    if f.tpm.as_deref() != Some("2.0") || f.tpm_ready.is_some() {
        return;
    }
    for c in r.checks.iter_mut().filter(|c| c.id == Tpm2) {
        c.state = State::Unknown;
    }
    for t in r.titles.iter_mut().filter(|t| t.required.contains(&Tpm2)) {
        t.ready = None;
    }
}

/// Compare with the last start and remember this one. The first run only records a baseline.
pub fn recheck(p: &Profile) -> Vec<Change> {
    let f = facts(p, &firmware::read(p));
    let mut r = evaluate(&f, &installed(), firmware::vendor_of(p.board.as_deref(), p.laptop), firmware::is_amd(p));
    blind_tpm(&f, &mut r);
    let old = load_snapshot();
    let (changes, next) = diff(old.as_ref().unwrap_or(&Snapshot::default()), &r);
    save_snapshot(&next);
    if old.is_some() { changes } else { Vec::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::firmware::Vendor;

    /// A gaming PC that meets everything (Windows 11 25H2, UEFI, VBS + HVCI, IOMMU on).
    fn ready_pc() -> Facts {
        Facts {
            tpm: Some("2.0".into()),
            tpm_ready: None,
            secure_boot: Some(true),
            boot_mode: Some("uefi"),
            system_gpt: Some(true),
            vbs: Some(true),
            hvci: Some(true),
            iommu: Some(true),
            virtualization: true,
            build: 26_200,
            blocklist_reg: Some(1),
        }
    }

    fn found(ids: &[&'static str]) -> Vec<(&'static str, String)> {
        ids.iter().map(|i| (*i, format!("{i} game"))).collect()
    }

    fn title<'a>(r: &'a Readiness, id: &str) -> &'a TitleState {
        r.titles.iter().find(|t| t.id == id).unwrap()
    }

    fn check(r: &Readiness, c: Check) -> State {
        r.checks.iter().find(|x| x.id == c).unwrap().state
    }

    #[test]
    fn matrix_game_by_requirement() {
        let all = found(&["valorant", "lol", "faceit", "bf6", "cod"]);
        let r = evaluate(&ready_pc(), &all, Vendor::Msi, false);
        assert!(r.titles.iter().all(|t| t.ready == Some(true) && t.missing.is_empty()), "{:?}", r.titles);
        assert_eq!(title(&r, "valorant").on_demand_ok, Some(true));
        assert_eq!(title(&r, "faceit").on_demand_ok, None, "only Vanguard has an on-demand mode");
        assert!(r.guide.items.is_empty());
        assert_eq!(r.vbs_games, vec!["valorant game", "lol game", "faceit game"], "BF6 and CoD do not use VBS");

        // Secure Boot off: every title blocked, the BIOS step listed as required.
        let sb_off = Facts { secure_boot: Some(false), ..ready_pc() };
        let r = evaluate(&sb_off, &all, Vendor::Asus, false);
        for t in &r.titles {
            assert_eq!((t.ready, t.missing.as_slice()), (Some(false), &[SecureBoot][..]), "{}", t.id);
        }
        assert_eq!(title(&r, "lol").on_demand_ok, Some(false));
        assert_eq!(r.guide.items.iter().map(|i| (i.id, i.optional)).collect::<Vec<_>>(), vec![("secure_boot", false)]);
        assert!(r.guide.items[0].path.starts_with("Boot › Secure Boot"), "ASUS path");

        // Windows 11 24H2 with VBS off and VT-d off: Valorant still ready, on-demand not.
        let h24 = Facts { build: 26_100, vbs: Some(false), hvci: Some(false), iommu: Some(false), ..ready_pc() };
        let r = evaluate(&h24, &found(&["valorant"]), Vendor::Gigabyte, true);
        let v = title(&r, "valorant");
        assert_eq!((v.ready, v.on_demand_ok), (Some(true), Some(false)));
        assert_eq!(r.guide.items.iter().map(|i| (i.id, i.optional)).collect::<Vec<_>>(), vec![("iommu", true)], "on-demand only: optional");
        assert!(r.guide.items[0].path.contains("IOMMU"), "AMD Gigabyte path");
        let vbs = r.checks.iter().find(|c| c.id == Vbs).unwrap();
        assert_eq!((vbs.bios, vbs.windows), (None, Some("core_isolation")), "VT-x on: Windows step only");

        // FACEIT on Windows 10: blocked by the Windows version, not the firmware.
        let w10 = Facts { build: 19_045, ..ready_pc() };
        let r = evaluate(&w10, &found(&["faceit", "cod"]), Vendor::Generic, false);
        assert_eq!(title(&r, "faceit").missing, vec![Win11]);
        assert_eq!(title(&r, "cod").ready, Some(true));
        assert!(r.guide.items.is_empty(), "nothing to do in the BIOS");
    }

    #[test]
    fn legacy_pc_lists_the_whole_chain() {
        // CSM on, MBR disk, TPM off in the firmware, no virtualization: nothing installed.
        let old = Facts {
            tpm: None,
            secure_boot: None,
            boot_mode: Some("legacy"),
            system_gpt: Some(false),
            vbs: Some(false),
            hvci: Some(false),
            iommu: None,
            virtualization: false,
            build: 22_631,
            blocklist_reg: None,
            ..Default::default()
        };
        let r = evaluate(&old, &[], Vendor::Asrock, true);
        for c in [Uefi, Gpt, SecureBoot, Tpm2, Vbs, Hvci] {
            assert_eq!(check(&r, c), State::Missing, "{c:?}");
        }
        assert_eq!(check(&r, Iommu), State::Unknown);
        assert_eq!(check(&r, Blocklist), State::Ok, "default on since 22H2");
        assert!(r.titles.iter().all(|t| t.installed.is_none() && t.ready == Some(false)));
        // No game installed: the steps cover every title, in the order to do them.
        assert_eq!(
            r.guide.items.iter().map(|i| (i.id, i.optional)).collect::<Vec<_>>(),
            vec![("csm", false), ("secure_boot", false), ("tpm", false), ("virtualization", true)]
        );
        assert!(!r.guide.uefi, "no restart into the setup on legacy firmware");
        let gpt = r.checks.iter().find(|c| c.id == Gpt).unwrap();
        assert_eq!((gpt.bios, gpt.windows), (None, Some("mbr2gpt")));
        assert!(r.vbs_games.is_empty());
    }

    #[test]
    fn single_states() {
        let f = ready_pc();
        assert_eq!(state(Tpm2, &Facts { tpm: Some("1.2".into()), ..f.clone() }), State::Missing);
        assert_eq!(state(Tpm2, &Facts { tpm_ready: Some(false), ..f.clone() }), State::Missing, "present but not activated");
        assert_eq!(state(SecureBoot, &Facts { secure_boot: None, ..f.clone() }), State::Unknown, "UEFI without the key: unknown");
        assert_eq!(state(Gpt, &Facts { system_gpt: None, ..f.clone() }), State::Ok, "UEFI boots Windows from GPT");
        assert_eq!(state(Gpt, &Facts { system_gpt: None, boot_mode: None, ..f.clone() }), State::Unknown);
        assert_eq!(state(Win11, &Facts { build: 0, ..f.clone() }), State::Unknown);
        assert_eq!(state(Win25h2, &Facts { build: 26_100, ..f.clone() }), State::Missing);
        let no_hvci = Facts { hvci: Some(false), ..f.clone() };
        assert_eq!(state(Blocklist, &Facts { blocklist_reg: Some(0), ..no_hvci.clone() }), State::Missing);
        assert_eq!(state(Blocklist, &Facts { blocklist_reg: Some(0), ..f.clone() }), State::Ok, "HVCI enforces it");
        assert_eq!(state(Blocklist, &Facts { blocklist_reg: None, build: 19_045, ..no_hvci }), State::Missing);
    }

    #[test]
    fn detection_from_steam_and_uninstall_entries() {
        let s = |a: u32, n: &str| (a, n.to_string());
        let a = |n: &str, p: &str| (n.to_string(), p.to_string());
        let steam = vec![s(730, "Counter-Strike 2"), s(STEAM_BF6, "Battlefield™ 6"), s(42_700, "Call of Duty: Black Ops")];
        let apps = vec![
            a("VALORANT", "Riot Games, Inc"),
            a("Riot Vanguard", "Riot Games, Inc"),
            a("League of Legends", "Riot Games, Inc"),
            a("FACEIT", "FACEIT Ltd"),
            a("Call of Duty", "Blizzard Entertainment"),
        ];
        let got = detect(&steam, &apps);
        assert_eq!(
            got,
            vec![("valorant", "VALORANT".into()), ("lol", "League of Legends".into()), ("faceit", "FACEIT".into()), ("bf6", "Battlefield™ 6".into()), ("cod", "Call of Duty".into())]
        );
        // Older Call of Duty and a fake "VALORANT" from someone else are not the real thing.
        let got = detect(&[s(42_700, "Call of Duty: Black Ops"), s(10, "Call of Duty: Modern Warfare 2 (2009)")], &[a("VALORANT", "Some Modder"), a("Faceit Overlay", "Other")]);
        assert!(got.is_empty(), "{got:?}");
        assert_eq!(detect(&[s(STEAM_COD_HQ, "Call of Duty®")], &[]), vec![("cod", "Call of Duty®".into())]);
    }

    #[test]
    fn hostile_names_are_cleaned() {
        let evil = "\u{202E}VALORANT\u{7}\r\n";
        let got = detect(&[], &[(evil.to_string(), "Riot Games".into())]);
        assert_eq!(got, vec![("valorant", "VALORANT".into())], "no bidi override or control chars");
        let long = format!("Battlefield 6 {}", "x".repeat(500));
        let got = detect(&[(1, long)], &[]);
        assert_eq!(got[0].1.chars().count(), 80);
        // Junk that normalizes to nothing matches nothing.
        assert!(detect(&[(0, "\u{0}\u{1}".into())], &[("".into(), "".into()), ("\u{202E}".into(), "Riot Games".into())]).is_empty());
    }

    #[test]
    fn diff_after_a_boot() {
        let all = found(&["valorant", "cod"]);
        let before = evaluate(&Facts { secure_boot: Some(false), ..ready_pc() }, &all, Vendor::Msi, false);
        let (none, base) = diff(&Snapshot::default(), &before);
        assert!(none.is_empty(), "no snapshot yet: nothing to compare");
        assert_eq!(base.checks.get(&SecureBoot), Some(&State::Missing));
        assert_eq!(base.ready.get("valorant"), Some(&false));

        // The user turned Secure Boot on in the BIOS.
        let after = evaluate(&ready_pc(), &all, Vendor::Msi, false);
        let (changes, next) = diff(&base, &after);
        assert_eq!(
            changes,
            vec![
                Change::Check { id: SecureBoot, on: true },
                Change::Game { name: "valorant game".into(), ready: true },
                Change::Game { name: "cod game".into(), ready: true },
            ]
        );
        assert!(diff(&next, &after).0.is_empty(), "same state: quiet");

        // A BIOS update reset the TPM.
        let (changes, _) = diff(&next, &evaluate(&Facts { tpm: None, ..ready_pc() }, &all, Vendor::Msi, false));
        assert!(changes.contains(&Change::Check { id: Tpm2, on: false }));

        // TPM readiness seen elevated (not activated), then a normal start (unreadable): no flip.
        let elevated = Facts { tpm_ready: Some(false), ..ready_pc() };
        let (_, snap) = diff(&next, &evaluate(&elevated, &all, Vendor::Msi, false));
        let normal = Facts { tpm_ready: None, ..ready_pc() };
        let mut r = evaluate(&normal, &all, Vendor::Msi, false);
        blind_tpm(&normal, &mut r);
        let (changes, kept) = diff(&snap, &r);
        assert!(changes.is_empty(), "elevation alone is not a change: {changes:?}");
        assert_eq!(kept.checks.get(&Tpm2), Some(&State::Missing));

        // An unreadable value keeps the last known one (no fake "changed").
        let unknown = evaluate(&Facts { vbs: None, hvci: None, ..ready_pc() }, &all, Vendor::Msi, false);
        let (changes, kept) = diff(&next, &unknown);
        assert!(changes.is_empty());
        assert_eq!(kept.checks.get(&Vbs), Some(&State::Ok));
    }

    #[test]
    fn snapshot_file_is_hostile_input() {
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", registry::tests::test_data_dir()) };
        let _ = std::fs::create_dir_all(registry::data_dir());
        std::fs::write(snapshot_file(), r#"{"checks":{"secure_boot":"ok"},"ready":{"../../x":true,"valorant":false}}"#).unwrap();
        let s = load_snapshot().unwrap();
        assert_eq!(s.ready.len(), 1, "unknown title ids dropped");
        std::fs::write(snapshot_file(), r#"{"checks":{"nope":"ok"}}"#).unwrap();
        assert!(load_snapshot().is_none(), "unknown check: no snapshot, no notification");
        std::fs::write(snapshot_file(), vec![b' '; 70 * 1024]).unwrap();
        assert!(load_snapshot().is_none(), "oversized file ignored");
        save_snapshot(&Snapshot::default());
        assert_eq!(load_snapshot(), Some(Snapshot::default()));
        let _ = std::fs::remove_file(snapshot_file());
    }

    #[test]
    fn live_readiness_answers() {
        let p = crate::hardware::profile();
        let r = readiness(&p, &firmware::read(&p));
        println!("{}", serde_json::to_string_pretty(&r).unwrap());
        assert_eq!(r.checks.len(), CHECKS.len());
        assert_eq!(r.titles.len(), TITLES.len());
        assert!(for_ai(&r).to_string().len() < 2_000, "compact for the context pack");
    }
}
