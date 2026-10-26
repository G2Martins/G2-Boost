//! Preinstalled Store apps ("apps de brinde"): a curated allowlist of packages that are safe to
//! remove for the current user and come back from the Microsoft Store. Listing reads the
//! installed packages; removal goes through `apps::uninstall` (PackageManager, no shell) after a
//! one-shot confirmation token bound to the exact ids, and every run is logged.
//!
//! Every caller is hostile: only ids from the last listing are accepted, and each package is
//! checked again (still installed, still allowlisted, never on [`NEVER`]) right before removal.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::apps::{self, App, Hive};
use crate::cleanup::{ConfirmToken, TokenStore};
use crate::registry;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DebloatError {
    #[error("bad_selection")]
    BadSelection,
    #[error("bad_token")]
    BadToken,
}

/// One allowlisted package: exact name (or a name prefix) and its Store publisher id.
pub struct Allowed {
    name: &'static str,
    publisher: &'static str,
    prefix: bool,
    /// What it is, for the UI (`debloat.app.kind.<kind>`).
    pub kind: &'static str,
}

const MS: &str = "8wekyb3d8bbwe";

const fn ms(name: &'static str, kind: &'static str) -> Allowed {
    Allowed { name, publisher: MS, prefix: false, kind }
}

const fn other(name: &'static str, publisher: &'static str, kind: &'static str) -> Allowed {
    Allowed { name, publisher, prefix: false, kind }
}

/// Removable and reinstallable from the Store. Sources: Docs/references/debloat-sources.md.
pub const ALLOWED: &[Allowed] = &[
    ms("Microsoft.BingNews", "news"),
    ms("Microsoft.BingWeather", "weather"),
    ms("Microsoft.GetStarted", "tips"),
    ms("Microsoft.MicrosoftSolitaireCollection", "game"),
    ms("Microsoft.WindowsFeedbackHub", "feedback"),
    ms("Microsoft.WindowsMaps", "maps"),
    ms("Microsoft.Microsoft3DViewer", "viewer3d"),
    ms("Microsoft.Print3D", "viewer3d"),
    ms("Microsoft.MixedReality.Portal", "mixed_reality"),
    ms("microsoft.windowscommunicationsapps", "mail"),
    ms("Microsoft.People", "mail"),
    ms("Microsoft.Messaging", "chat"),
    ms("Microsoft.Copilot", "ai"),
    ms("Microsoft.549981C3F5F10", "ai"), // Cortana
    ms("MicrosoftCorporationII.MicrosoftFamily", "family"),
    ms("Microsoft.PowerAutomateDesktop", "automation"),
    ms("Microsoft.MicrosoftOfficeHub", "promo"),
    ms("Microsoft.OneConnect", "promo"), // Mobile Plans
    other("Microsoft.SkypeApp", "kzf8qxf38zg5c", "chat"),
    other("Clipchamp.Clipchamp", "yxz26nhyzhsrt", "video"),
    Allowed { name: "king.com.", publisher: "kgqvnymyfvs32", prefix: true, kind: "game" }, // Candy Crush & co.
    other("SpotifyAB.SpotifyMusic", "zpdnekdrzrea0", "promo"),
    other("4DF9E0F8.Netflix", "mcm4njqhnhss8", "promo"),
    other("Disney.37853FC22B2CE", "6rarf9sa4v8jt", "promo"),
    other("AmazonVideo.PrimeVideo", "pwbj9vvecjh7j", "promo"),
    other("BytedancePte.Ltd.TikTok", "6yccndn6064se", "promo"),
];

/// Never offered, whatever the allowlist says: games and the Xbox stack, the Store and its
/// installer, everyday tools, codecs and hardware vendors' apps (lowercase name prefixes).
pub const NEVER: &[&str] = &[
    "microsoft.xbox", "microsoft.gamingapp", "microsoft.gamingservices", "microsoft.windowsstore",
    "microsoft.storepurchaseapp", "microsoft.desktopappinstaller", "microsoft.windows.photos", "microsoft.photos",
    "microsoft.windowscalculator", "microsoft.windowsterminal", "microsoft.windowsnotepad", "microsoft.paint",
    "microsoft.mspaint", "microsoft.windowscamera", "microsoft.screensketch", "microsoft.snippingtool",
    "microsoft.sechealthui", "microsoft.vclibs", "microsoft.ui.xaml", "microsoft.net.", "microsoft.services.store",
    "microsoft.winappruntime", "microsoft.windowsappruntime", "microsoft.hevc", "microsoft.heif", "microsoft.webp",
    "microsoft.vp9", "microsoft.av1", "microsoft.raw", "microsoft.webmedia", "microsoft.mpeg2", "nvidiacorp.",
    "realteksemiconductorcorp.", "appup.", "advancedmicrodevicesinc", "dolbylaboratories.", "wavesaudio.",
    "synapticsincorporated.", "elanmicroelectronicscorp.", "intelcorp.",
];

/// `Name_PublisherId` as Windows reports a package family (hostile: checked here).
fn split_family(family: &str) -> Option<(&str, &str)> {
    let (name, publisher) = family.rsplit_once('_')?;
    let name_ok = (3..=64).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        && !name.contains("..")
        && !name.starts_with('.');
    let publisher_ok = publisher.len() == 13 && publisher.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    (name_ok && publisher_ok).then_some((name, publisher))
}

/// The allowlist entry for a package family, if it is one we may remove.
pub fn allowed(family: &str) -> Option<&'static Allowed> {
    let (name, publisher) = split_family(family)?;
    let n = name.to_ascii_lowercase();
    if NEVER.iter().any(|x| n.starts_with(x)) {
        return None;
    }
    ALLOWED.iter().find(|a| {
        let an = a.name.to_ascii_lowercase();
        a.publisher == publisher && if a.prefix { n.starts_with(&an) && n.len() > an.len() } else { n == an }
    })
}

/// One installed, removable preinstalled app.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Preinstalled {
    /// The id from `apps::list` (`s:<package full name>`): the only handle the UI sends back.
    pub id: String,
    pub name: String,
    pub family: String,
    pub kind: &'static str,
}

fn preinstalled(a: &App) -> Option<Preinstalled> {
    if a.hive != Hive::Store || a.protected.is_some() {
        return None;
    }
    let entry = allowed(&a.uninstall)?; // Store apps carry their family name here
    Some(Preinstalled { id: a.id.clone(), name: a.name.clone(), family: a.uninstall.clone(), kind: entry.kind })
}

static LAST: Mutex<Vec<Preinstalled>> = Mutex::new(Vec::new());

fn tokens() -> &'static TokenStore {
    static T: OnceLock<TokenStore> = OnceLock::new();
    T.get_or_init(TokenStore::default)
}

fn token_ids(ids: &[String]) -> Vec<String> {
    ids.iter().map(|i| format!("debloat:app:{i}")).collect()
}

/// Allowlisted Store apps installed for this user (apps the user protected are left out).
pub fn installed(user_protected: &[String]) -> Vec<Preinstalled> {
    let mut list: Vec<Preinstalled> = apps::list(user_protected).iter().filter_map(preinstalled).collect();
    list.sort_by_key(|p| p.name.to_lowercase());
    *LAST.lock().unwrap_or_else(|p| p.into_inner()) = list.clone();
    list
}

/// Issued after the user confirmed the list: every id from the last listing, no repeats.
pub fn confirm(ids: &[String]) -> Result<ConfirmToken, DebloatError> {
    let last = LAST.lock().unwrap_or_else(|p| p.into_inner());
    let mut seen = HashSet::new();
    let ok = !ids.is_empty() && ids.len() <= 64 && ids.iter().all(|id| seen.insert(id.as_str()) && last.iter().any(|p| p.id == *id));
    if !ok {
        return Err(DebloatError::BadSelection);
    }
    Ok(tokens().issue(token_ids(ids)))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Removed {
    pub id: String,
    pub name: String,
    pub ok: bool,
    /// `not_installed`, `not_allowed`, or the uninstall error.
    pub error: Option<String>,
}

/// Remove the confirmed packages, one by one, for the current user.
pub fn remove(token: u64, ids: &[String]) -> Result<Vec<Removed>, DebloatError> {
    if !tokens().consume(token, &token_ids(ids)) {
        return Err(DebloatError::BadToken);
    }
    let live = apps::list(&[]);
    let out: Vec<Removed> = ids
        .iter()
        .map(|id| {
            let fail = |name: &str, e: &str| Removed { id: id.clone(), name: name.to_string(), ok: false, error: Some(e.into()) };
            let Some(app) = live.iter().find(|a| a.id == *id) else {
                return fail("", "not_installed");
            };
            if preinstalled(app).is_none() {
                return fail(&app.name, "not_allowed");
            }
            match apps::uninstall(app) {
                Ok(()) => Removed { id: id.clone(), name: app.name.clone(), ok: true, error: None },
                Err(e) => fail(&app.name, &e.to_string()),
            }
        })
        .collect();
    LAST.lock().unwrap_or_else(|p| p.into_inner()).retain(|p| !out.iter().any(|r| r.ok && r.id == p.id));
    log(&out);
    Ok(out)
}

#[derive(Serialize, Deserialize)]
struct LogEntry {
    at: u64,
    name: String,
    ok: bool,
    error: Option<String>,
}

fn log_file() -> PathBuf {
    registry::data_dir().join("debloat-apps-log.json")
}

/// Last 200 removals (`debloat-apps-log.json`): when, which app, result. No paths, no PII.
fn log(run: &[Removed]) {
    let mut all: Vec<LogEntry> = std::fs::read_to_string(log_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let at = registry::now_unix();
    all.extend(run.iter().map(|r| LogEntry { at, name: r.name.clone(), ok: r.ok, error: r.error.clone() }));
    let n = all.len();
    if n > 200 {
        all.drain(..n - 200);
    }
    if let Some(d) = log_file().parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(log_file(), serde_json::to_string(&all).unwrap_or_default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_matches_exact_names_and_publishers_only() {
        assert_eq!(allowed("Microsoft.BingNews_8wekyb3d8bbwe").map(|a| a.kind), Some("news"));
        assert_eq!(allowed("microsoft.bingnews_8wekyb3d8bbwe").map(|a| a.kind), Some("news"), "names are case-insensitive");
        assert_eq!(allowed("king.com.CandyCrushSaga_kgqvnymyfvs32").map(|a| a.kind), Some("game"));
        assert_eq!(allowed("Clipchamp.Clipchamp_yxz26nhyzhsrt").map(|a| a.kind), Some("video"));
        for hostile in [
            "Microsoft.BingNews_xxxxxxxxxxxxx",          // same name, another publisher
            "Microsoft.BingNews.Evil_8wekyb3d8bbwe",     // longer name
            "Microsoft.BingNew_8wekyb3d8bbwe",           // shorter name
            "king.com._kgqvnymyfvs32",                   // bare prefix
            "king.com.CandyCrushSaga_8wekyb3d8bbwe",     // prefix with the wrong publisher
            "Microsoft.BingNews",                        // no publisher
            "Microsoft.BingNews_8WEKYB3D8BBWE",          // publisher ids are lowercase
            "..\\Microsoft.BingNews_8wekyb3d8bbwe",      // path
            "Microsoft..BingNews_8wekyb3d8bbwe",
            "Microsoft.BingNews_8wekyb3d8bbwe\0",
            "Microsoft.BingNews _8wekyb3d8bbwe",
            "",
            "_",
            "Microsoft.XboxApp_8wekyb3d8bbwe",
            "Microsoft.GamingApp_8wekyb3d8bbwe",
            "Microsoft.WindowsStore_8wekyb3d8bbwe",
            "Microsoft.Paint_8wekyb3d8bbwe",
            "NVIDIACorp.NVIDIAControlPanel_56jybvy8sckqj",
        ] {
            assert!(allowed(hostile).is_none(), "{hostile:?}");
        }
    }

    #[test]
    fn allowlist_and_never_list_do_not_overlap() {
        let mut seen = HashSet::new();
        for a in ALLOWED {
            let n = a.name.to_ascii_lowercase();
            assert!(seen.insert(n.clone()), "duplicate {n}");
            assert!(!NEVER.iter().any(|x| n.starts_with(x) || x.starts_with(&n)), "{n} collides with the never list");
            assert!(split_family(&format!("{}{}_{}", a.name, if a.prefix { "X" } else { "" }, a.publisher)).is_some(), "{n}");
            assert!(!a.kind.is_empty() && a.kind.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'));
            for dict in [include_str!("../../../src/locales/pt-BR.ts"), include_str!("../../../src/locales/en.ts")] {
                assert!(dict.contains(&format!("\"debloat.app.kind.{}\":", a.kind)), "kind {} has no text", a.kind);
            }
        }
        // What the product promises never to remove.
        for keep in ["microsoft.xboxapp", "microsoft.xboxgamingoverlay", "microsoft.windowsstore", "microsoft.desktopappinstaller",
            "microsoft.windows.photos", "microsoft.windowscalculator", "microsoft.windowsterminal", "microsoft.windowsnotepad",
            "microsoft.paint", "microsoft.windowscamera", "microsoft.screensketch"]
        {
            assert!(NEVER.iter().any(|x| keep.starts_with(x)), "{keep}");
        }
    }

    #[test]
    fn selection_and_token_are_checked() {
        *LAST.lock().unwrap() =
            vec![Preinstalled { id: "s:Microsoft.BingNews_1.0.0.0_x64__8wekyb3d8bbwe".into(), name: "News".into(), family: "Microsoft.BingNews_8wekyb3d8bbwe".into(), kind: "news" }];
        let good = vec!["s:Microsoft.BingNews_1.0.0.0_x64__8wekyb3d8bbwe".to_string()];
        assert_eq!(confirm(&[]).unwrap_err(), DebloatError::BadSelection);
        assert_eq!(confirm(&["s:Microsoft.XboxApp_1_x64__8wekyb3d8bbwe".into()]).unwrap_err(), DebloatError::BadSelection);
        assert_eq!(confirm(&[good[0].clone(), good[0].clone()]).unwrap_err(), DebloatError::BadSelection, "no repeats");
        let t = confirm(&good).unwrap();
        assert_eq!(remove(t.token ^ 2, &good).unwrap_err(), DebloatError::BadToken);
        assert_eq!(remove(t.token, &["s:other".into()]).unwrap_err(), DebloatError::BadToken, "bound to the ids");
    }
}
