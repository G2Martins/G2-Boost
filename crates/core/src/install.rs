//! The **Instalar** tab's engine: which catalog apps are installed, and installing a selection.
//!
//! - **Installed:** `winget list` (exact ids, cached for ten minutes) plus the registry's Uninstall
//!   entries (a conservative name match: the fallback when winget is missing or does not know an
//!   app). A match also carries the registry app's id, which gives the tile its real icon and the
//!   native uninstaller (`apps.rs`): uninstalling never goes through winget.
//! - **Batch install:** the same shape as "update all" (`updates.rs`). The allowlist is the catalog,
//!   one confirmation token is bound to the exact, ordered id list, and `winget::install(id, false)`
//!   runs once per package with fixed arguments, never a free-form command line. Cancel stops
//!   between packages. The batch shares its busy flag with "update all" (winget installs one thing at
//!   a time) and records its runs in `install-history.json`.
//!
//! Everything the UI sends is hostile: ids are resolved against the catalog here.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::apps::App;
use crate::catalog::{self, Entry};
use crate::cleanup::{ConfirmToken, TokenStore};
use crate::updates::{self, AppProgress, Batch, Item};
use crate::winget::{self, Listed, Outcome, WingetError};

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    /// An id outside the catalog, a repeat, an empty list or one over `MAX_BATCH`.
    #[error("bad_selection")]
    BadSelection,
    #[error("bad_token")]
    BadToken,
    /// Another winget batch ("update all" or an install) is running in this process.
    #[error("busy")]
    Busy,
}

/* ---------------- what is installed ---------------- */

/// What the tab knows about one catalog app.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct State {
    /// The catalog id.
    pub id: &'static str,
    pub installed: bool,
    pub version: Option<String>,
    /// A newer version in the winget catalog (`winget list` shows it).
    pub update: Option<String>,
    /// `winget` (the id is in `winget list`) or `registry` (the name matched an Uninstall entry).
    pub via: Option<&'static str>,
    /// The id of the matching installed app (`apps::App.id`): its icon and its native uninstaller.
    pub app: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// false: winget.exe is not installed for this user, so nothing can be installed from here.
    pub winget: bool,
    pub states: Vec<State>,
}

/// Lowercase alphanumeric words, brackets and what is inside them dropped:
/// `Notepad++ (64-bit)` gives `[notepad]`, `Mozilla Firefox (x64 pt-BR)` gives `[mozilla, firefox]`.
fn words(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth) = (Vec::new(), String::new(), 0u32);
    for ch in s.chars() {
        match ch {
            '(' | '[' => {
                depth += 1;
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            ')' | ']' => depth = depth.saturating_sub(1),
            c if depth == 0 && c.is_alphanumeric() => cur.extend(c.to_lowercase()),
            _ => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// What may follow a product name in an Uninstall entry without making it another product.
fn is_noise(w: &str) -> bool {
    const NOISE: &[&str] = &["x64", "x86", "amd64", "arm64", "win64", "win32", "bit", "64bit", "32bit", "stable", "setup"];
    w.chars().all(|c| c.is_ascii_digit()) || NOISE.contains(&w)
}

/// Does this Uninstall entry look like the catalog app? Conservative on purpose: the entry's name
/// must be the catalog name (the publisher may lead it) followed only by versions and bitness, so
/// `Python 3.13.5` is never `Python 3.14`, `Opera GX Stable` is not `Opera` and `Plex Media Server`
/// is not `Plex`. A miss only costs an "install" button that winget answers with "already installed".
fn same_app(e: &Entry, app: &App) -> bool {
    let want = words(e.name);
    if want.concat().chars().count() < 3 {
        return false;
    }
    let have = words(&app.name);
    let publisher = e.id.split('.').next().unwrap_or_default().to_ascii_lowercase();
    let lead = have.first().is_some_and(|w| *w == publisher) && want.first() != Some(&publisher);
    let have = &have[usize::from(lead)..];
    have.starts_with(&want) && have[want.len()..].iter().all(|w| is_noise(w))
}

/// `winget list` names an app by the Uninstall entry's own name (cut with `…` when too wide).
fn same_name(listed: &str, registry: &str) -> bool {
    let (l, r) = (listed.trim().to_lowercase(), registry.trim().to_lowercase());
    match l.strip_suffix('…') {
        Some(stem) => !stem.is_empty() && r.starts_with(stem),
        None => l == r,
    }
}

/// The catalog entry a `winget list` id belongs to: the id or an alias, ignoring case. A cell cut
/// short by the console width (`Publisher.Long…`) counts only when exactly one entry starts with it.
fn owner(listed_id: &str) -> Option<&'static Entry> {
    /// The entry's own id, then its aliases.
    fn ids(e: &'static Entry) -> impl Iterator<Item = &'static str> {
        std::iter::once(e.id).chain(e.same_as.iter().copied())
    }
    match listed_id.strip_suffix('…') {
        None => catalog::all().iter().find(|e| ids(e).any(|i| i.eq_ignore_ascii_case(listed_id))),
        Some(stem) if stem.len() >= 8 => {
            let mut hits = catalog::all()
                .iter()
                .filter(|e| ids(e).any(|i| i.get(..stem.len()).is_some_and(|p| p.eq_ignore_ascii_case(stem))));
            let first = hits.next();
            if hits.next().is_some() { None } else { first }
        }
        Some(_) => None,
    }
}

/// Which catalog apps are installed, given `winget list` and the registry's installed apps.
pub fn detect(listed: &[Listed], registry: &[App]) -> Vec<State> {
    let mut by_entry: HashMap<&'static str, &Listed> = HashMap::new();
    for l in listed {
        if let Some(e) = owner(&l.id) {
            by_entry.entry(e.id).or_insert(l);
        }
    }
    catalog::all()
        .iter()
        .map(|e| {
            let hit = by_entry.get(e.id).copied();
            let app = hit
                .and_then(|l| registry.iter().find(|a| same_name(&l.name, &a.name)))
                .or_else(|| registry.iter().find(|a| same_app(e, a)));
            let non_empty = |v: &str| Some(v.to_string()).filter(|v| !v.is_empty());
            State {
                id: e.id,
                installed: hit.is_some() || app.is_some(),
                version: hit.and_then(|l| non_empty(&l.version)).or_else(|| app.and_then(|a| non_empty(&a.version))),
                update: hit.and_then(|l| l.available.clone()),
                via: if hit.is_some() {
                    Some("winget")
                } else if app.is_some() {
                    Some("registry")
                } else {
                    None
                },
                app: app.map(|a| a.id.clone()),
            }
        })
        .collect()
}

const FRESH: Duration = Duration::from_secs(10 * 60);
static LISTED: Mutex<Option<(Instant, Vec<Listed>)>> = Mutex::new(None);

/// `winget list`, kept for ten minutes (it takes several seconds); `refresh` asks again.
fn listed(refresh: bool) -> Result<Vec<Listed>, WingetError> {
    if !refresh
        && let Some((at, rows)) = LISTED.lock().unwrap_or_else(|p| p.into_inner()).as_ref()
        && at.elapsed() < FRESH
    {
        return Ok(rows.clone());
    }
    let rows = winget::list()?;
    *LISTED.lock().unwrap_or_else(|p| p.into_inner()) = Some((Instant::now(), rows.clone()));
    Ok(rows)
}

/// The installed state of every catalog app, plus the registry's installed apps the shell keeps for
/// icons and the native uninstaller. `winget list` and the registry are read at the same time.
pub fn scan(refresh: bool, protected: &[String]) -> (Status, Vec<App>) {
    let (rows, apps) = std::thread::scope(|s| {
        let rows = s.spawn(|| listed(refresh));
        let apps = crate::apps::list(protected);
        (rows.join().unwrap_or(Err(WingetError::Timeout)), apps)
    });
    let (has_winget, rows) = match rows {
        Ok(r) => (true, r),
        Err(WingetError::Unavailable) => (false, Vec::new()),
        // winget is there but did not answer: the registry still tells most of it.
        Err(_) => (true, Vec::new()),
    };
    (Status { winget: has_winget, states: detect(&rows, &apps) }, apps)
}

/* ---------------- batch install ---------------- */

/// More than this in one batch is a mistake or an attack, not a selection.
pub const MAX_BATCH: usize = 60;

fn tokens() -> &'static TokenStore {
    static T: OnceLock<TokenStore> = OnceLock::new();
    T.get_or_init(TokenStore::default)
}

fn token_ids(entries: &[&Entry]) -> Vec<String> {
    entries.iter().map(|e| format!("install:{}", e.id)).collect()
}

/// The catalog entries for a selection, in the order given. An id outside the catalog, a repeat
/// (any case), an empty list or an oversized one is refused: nothing else may reach winget.
pub fn resolve(ids: &[String]) -> Result<Vec<&'static Entry>, InstallError> {
    if ids.is_empty() || ids.len() > MAX_BATCH {
        return Err(InstallError::BadSelection);
    }
    let mut seen = HashSet::new();
    ids.iter()
        .map(|id| catalog::find(id).filter(|e| seen.insert(e.id)).ok_or(InstallError::BadSelection))
        .collect()
}

/// Issued after the user confirmed the list: single use, two minutes, bound to the exact order.
pub fn confirm(ids: &[String]) -> Result<ConfirmToken, InstallError> {
    Ok(tokens().issue(token_ids(&resolve(ids)?)))
}

static CANCEL: AtomicBool = AtomicBool::new(false);

/// Stop after the package being installed now (killing an installer midway can leave a broken app).
pub fn cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// winget's exit code to a short reason key for the UI: the "update all" mapping, plus what only
/// an install can say.
pub fn reason(code: u32) -> &'static str {
    match code {
        0x8A15_010D | 0x8A15_0061 => "installed",
        c => match updates::reason(c) {
            "not_applicable" => "no_installer",
            r => r,
        },
    }
}

/// One package after the other, in order. `on` sees each start and result; a stop request turns the
/// packages that have not started into `cancelled`.
fn run_loop(
    entries: &[&Entry],
    on: &mut dyn FnMut(&AppProgress),
    install: &dyn Fn(&str) -> Result<Outcome, WingetError>,
    stop: &dyn Fn() -> bool,
) -> Vec<Item> {
    let total = entries.len();
    let mut items = Vec::with_capacity(total);
    for (index, e) in entries.iter().enumerate() {
        let mut item = Item { id: e.id.to_string(), name: e.name.to_string(), status: "running".into(), reason: String::new(), code: 0 };
        if stop() {
            item.status = "cancelled".into();
            item.reason = "cancelled".into();
        } else {
            on(&AppProgress { index, total, item: item.clone() });
            match install(e.id) {
                Ok(o) => {
                    let r = reason(o.code);
                    item.status = if o.ok || matches!(r, "reboot" | "installed") { "ok" } else { "failed" }.into();
                    item.reason = r.into();
                    item.code = o.code;
                }
                Err(err) => {
                    item.status = "failed".into();
                    item.reason = match err {
                        WingetError::Unavailable => "winget_missing",
                        WingetError::Timeout => "timeout",
                        _ => "failed",
                    }
                    .into();
                }
            }
        }
        on(&AppProgress { index, total, item: item.clone() });
        items.push(item);
    }
    items
}

/// Install the confirmed packages one by one. `ids` must be the list the token was issued for.
pub fn run(token: u64, ids: &[String], mut on: impl FnMut(&AppProgress)) -> Result<Batch, InstallError> {
    let entries = resolve(ids)?;
    if !tokens().consume(token, &token_ids(&entries)) {
        return Err(InstallError::BadToken);
    }
    let _busy = updates::Busy::take(&updates::APPS_BUSY).map_err(|_| InstallError::Busy)?;
    CANCEL.store(false, Ordering::SeqCst);
    let items = run_loop(&entries, &mut on, &|id| winget::install(id, false), &|| CANCEL.load(Ordering::SeqCst));
    let batch = Batch {
        at: crate::registry::now_unix(),
        kind: "install".into(),
        reboot: items.iter().any(|i| i.reason == "reboot"),
        cancelled: items.iter().any(|i| i.status == "cancelled"),
        items,
    };
    updates::append_history(&history_file(), KINDS, &batch);
    // What is installed just changed: the next status asks winget again.
    *LISTED.lock().unwrap_or_else(|p| p.into_inner()) = None;
    Ok(batch)
}

/* ---------------- history ---------------- */

const KINDS: &[&str] = &["install"];

fn history_file() -> std::path::PathBuf {
    crate::registry::data_dir().join("install-history.json")
}

/// The last runs, oldest first. The file is hostile: bounded and re-validated on read.
pub fn history() -> Vec<Batch> {
    updates::read_history(&history_file(), KINDS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::Hive;

    fn reg(id: &str, name: &str, version: &str) -> App {
        App {
            id: id.into(),
            hive: Hive::Machine,
            key: id.into(),
            name: name.into(),
            publisher: String::new(),
            version: version.into(),
            install_location: None,
            exe: None,
            icon_source: None,
            size_bytes: None,
            install_date: None,
            uninstall: String::new(),
            msi: false,
            needs_admin: true,
            protected: None,
        }
    }

    fn row(id: &str, name: &str, version: &str, available: Option<&str>) -> Listed {
        Listed { id: id.into(), name: name.into(), version: version.into(), available: available.map(str::to_string) }
    }

    fn state<'a>(all: &'a [State], id: &str) -> &'a State {
        all.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("{id} not in the catalog"))
    }

    #[test]
    fn winget_list_marks_installed_with_version_update_and_the_registry_app() {
        let listed = [
            row("git.git", "Git", "2.56.0.2", Some("2.56.0.3")),
            row("Valve.Steam", "Steam", "2.10.91.91", None),
            row("Some.Other.App", "Other", "1.0", None),
        ];
        let registry = [reg("m64:Git_is1", "Git", "2.56.0.2"), reg("m32:Steam", "Steam", "2.10"), reg("m64:zzz", "Other", "1")];
        let all = detect(&listed, &registry);
        let git = state(&all, "Git.Git");
        assert!(git.installed);
        assert_eq!(git.via, Some("winget"));
        assert_eq!(git.version.as_deref(), Some("2.56.0.2"));
        assert_eq!(git.update.as_deref(), Some("2.56.0.3"));
        assert_eq!(git.app.as_deref(), Some("m64:Git_is1"));
        let steam = state(&all, "Valve.Steam");
        assert_eq!((steam.installed, steam.update.as_deref()), (true, None));
        assert!(!state(&all, "Google.Chrome").installed);
        assert!(state(&all, "Google.Chrome").app.is_none());
        assert_eq!(all.len(), catalog::all().len(), "one state per catalog entry, in catalog order");
    }

    #[test]
    fn an_alias_id_counts_as_the_same_app() {
        // Firefox installed from the en-US id, the catalog offers the pt-BR build.
        let all = detect(&[row("Mozilla.Firefox", "Mozilla Firefox (x64 en-US)", "157.0", None)], &[]);
        assert!(state(&all, "Mozilla.Firefox.pt-BR").installed);
        let all = detect(&[row("GIMP.GIMP.2", "GIMP 2.10.38", "2.10.38", None)], &[]);
        assert!(state(&all, "GIMP.GIMP.3").installed);
    }

    #[test]
    fn the_registry_fallback_matches_names_conservatively() {
        let registry = [
            reg("m64:7z", "7-Zip 23.01 (x64)", "23.01"),
            reg("m64:vsc", "Microsoft Visual Studio Code", "1.99"),
            reg("u:ff", "Mozilla Firefox (x64 pt-BR)", "157.0"),
            reg("m64:py", "Python 3.13.5 (64-bit)", "3.13.5"),
            reg("m64:opgx", "Opera GX Stable 119.0.5497.88", "119"),
            reg("m64:link", "Steam Link", "1.0"),
            reg("m64:pms", "Plex Media Server", "1.0"),
            reg("m64:dn", "Microsoft .NET SDK 10.0.103 (x64)", "10.0.103"),
            reg("m64:op", "Opera Stable 120.0.5543.93", "120"),
        ];
        let all = detect(&[], &registry);
        for (id, app) in [
            ("7zip.7zip", "m64:7z"),
            ("Microsoft.VisualStudioCode", "m64:vsc"),
            ("Mozilla.Firefox.pt-BR", "u:ff"),
            ("Microsoft.DotNet.SDK.10", "m64:dn"),
            ("Opera.Opera", "m64:op"),
            ("Opera.OperaGX", "m64:opgx"),
        ] {
            let s = state(&all, id);
            assert_eq!((s.installed, s.via, s.app.as_deref()), (true, Some("registry"), Some(app)), "{id}");
        }
        // Another version, another edition or another product is not the app.
        for id in ["Python.Python.3.14", "Valve.Steam", "Plex.Plex", "Plex.PlexMediaServer"] {
            let s = state(&all, id);
            assert_eq!(s.installed, id == "Plex.PlexMediaServer", "{id}");
        }
        assert!(!state(&all, "Microsoft.DotNet.DesktopRuntime.8").installed);
    }

    #[test]
    fn a_cut_id_counts_only_when_one_entry_starts_with_it() {
        let ok = detect(&[row("EclipseAdoptium.Temurin.21.JD…", "Eclipse Temurin JDK with Hotspot 21.0.…", "21.0.5", None)], &[]);
        assert!(state(&ok, "EclipseAdoptium.Temurin.21.JDK").installed);
        // Two runtimes begin with this stem: neither is claimed.
        let both = detect(&[row("Microsoft.DotNet.DesktopRu…", ".NET Desktop Runtime", "8.0.1", None)], &[]);
        assert!(!state(&both, "Microsoft.DotNet.DesktopRuntime.8").installed);
        assert!(!state(&both, "Microsoft.DotNet.DesktopRuntime.10").installed);
        let short = detect(&[row("Goo…", "x", "1", None)], &[]);
        assert!(short.iter().all(|s| !s.installed), "a stem that short proves nothing");
    }

    #[test]
    fn a_listed_name_cut_by_the_console_still_finds_the_registry_app() {
        let registry = [reg("m64:vs", "Microsoft Visual Studio Community 2026", "18.1")];
        let all = detect(&[row("Microsoft.VisualStudio.Community", "Microsoft Visual Studio Commun…", "18.1", None)], &registry);
        assert_eq!(state(&all, "Microsoft.VisualStudio.Community").app.as_deref(), Some("m64:vs"));
        assert!(same_name("steam", "Steam") && !same_name("steam", "Steam Link") && !same_name("…", "Steam"));
    }

    #[test]
    fn hostile_ids_and_names_match_nothing() {
        let evil = [
            row("Google.Chrome\0", "Google Chrome", "1", None),
            row("../../Google.Chrome", "x", "1", None),
            row("Google.Chrome;calc", "x", "1", None),
            row("", "", "", None),
            row("…", "x", "1", None),
        ];
        let all = detect(&evil, &[]);
        assert!(all.iter().all(|s| !s.installed), "no id outside the catalog's spelling reaches an entry");
        // A registry name is data: control and direction characters only split words.
        let all = detect(&[], &[reg("m64:x", "\u{202e}Google Chrome\0", "1")]);
        assert_eq!(state(&all, "Google.Chrome").app.as_deref(), Some("m64:x"));
        assert_eq!(words("a (b [c] d) e"), ["a", "e"]);
        assert_eq!(words("))((x"), Vec::<String>::new());
    }

    #[test]
    fn selections_resolve_against_the_catalog_only() {
        let ok = resolve(&["google.chrome".into(), "Valve.Steam".into()]).unwrap();
        assert_eq!(ok.iter().map(|e| e.id).collect::<Vec<_>>(), ["Google.Chrome", "Valve.Steam"], "canonical ids, same order");
        let too_many: Vec<String> = catalog::all().iter().take(MAX_BATCH + 1).map(|e| e.id.to_string()).collect();
        for bad in [
            vec![],
            vec!["Not.InTheCatalog".to_string()],
            vec!["Google.Chrome".to_string(), "GOOGLE.CHROME".to_string()],
            vec!["Google.Chrome".to_string(), "--silent".to_string()],
            vec!["Google.Chrome --force".to_string()],
            vec!["Mozilla.Firefox".to_string()], // an alias is not an installable entry
            too_many,
        ] {
            assert!(matches!(resolve(&bad), Err(InstallError::BadSelection)), "{bad:?}");
        }
        assert!(resolve(&catalog::all().iter().take(MAX_BATCH).map(|e| e.id.to_string()).collect::<Vec<_>>()).is_ok());
    }

    #[test]
    fn the_token_is_bound_to_the_exact_ordered_list_and_used_once() {
        let ids = vec!["Google.Chrome".to_string(), "Valve.Steam".to_string()];
        let entries = resolve(&ids).unwrap();
        let t = confirm(&ids).unwrap().token;
        let reversed: Vec<&Entry> = entries.iter().rev().copied().collect();
        assert!(!tokens().consume(t, &token_ids(&reversed)), "order is part of the binding");
        assert!(!tokens().consume(t, &token_ids(&entries[..1])), "so is the whole list");
        assert!(!tokens().consume(t, &entries.iter().map(|e| format!("updates:apps:{}", e.id)).collect::<Vec<_>>()), "and the kind");
        assert!(tokens().consume(t, &token_ids(&entries)));
        assert!(!tokens().consume(t, &token_ids(&entries)), "single use");
        // run() refuses a token nobody issued, an unknown id and a stale token, before any winget call.
        assert!(matches!(run(t, &ids, |_| {}), Err(InstallError::BadToken)));
        assert!(matches!(run(1, &["Not.InTheCatalog".to_string()], |_| {}), Err(InstallError::BadSelection)));
        let other = confirm(&["7zip.7zip".to_string()]).unwrap().token;
        assert!(matches!(run(other, &ids, |_| {}), Err(InstallError::BadToken)), "a token for another list");
    }

    fn outcome(code: u32) -> Result<Outcome, WingetError> {
        Ok(Outcome { ok: code == 0, code, message: String::new() })
    }

    #[test]
    fn the_loop_reports_each_package_and_maps_exit_codes() {
        let entries = resolve(&["Google.Chrome".into(), "Valve.Steam".into(), "VideoLAN.VLC".into(), "7zip.7zip".into(), "Discord.Discord".into()]).unwrap();
        let mut events = Vec::new();
        let items = run_loop(
            &entries,
            &mut |p| events.push((p.index, p.item.id.clone(), p.item.status.clone())),
            &|id| match id {
                "Google.Chrome" => outcome(0),
                "Valve.Steam" => outcome(0x8A15_0101),    // app open
                "VideoLAN.VLC" => outcome(0x8A15_0109),   // restart to finish: still a success
                "7zip.7zip" => outcome(0x8A15_010D),      // already installed
                _ => Err(WingetError::Timeout),
            },
            &|| false,
        );
        let got: Vec<(&str, &str, u32)> = items.iter().map(|i| (i.status.as_str(), i.reason.as_str(), i.code)).collect();
        assert_eq!(
            got,
            [("ok", "ok", 0), ("failed", "in_use", 0x8A15_0101), ("ok", "reboot", 0x8A15_0109), ("ok", "installed", 0x8A15_010D), ("failed", "timeout", 0)]
        );
        // A start and a result per package, in order.
        assert_eq!(events.len(), 10);
        assert_eq!((events[0].0, events[0].2.as_str()), (0, "running"));
        assert_eq!((events[1].0, events[1].2.as_str()), (0, "ok"));
        assert_eq!(items.len(), 5);
    }

    #[test]
    fn a_stop_request_cancels_what_has_not_started() {
        let entries = resolve(&["Google.Chrome".into(), "Valve.Steam".into(), "VideoLAN.VLC".into()]).unwrap();
        let stop = AtomicBool::new(false);
        let mut ran = Vec::new();
        let items = {
            let ran = std::cell::RefCell::new(&mut ran);
            run_loop(
                &entries,
                &mut |_| {},
                &|id| {
                    ran.borrow_mut().push(id.to_string());
                    stop.store(true, Ordering::SeqCst); // the user pressed stop while the first one ran
                    outcome(0)
                },
                &|| stop.load(Ordering::SeqCst),
            )
        };
        assert_eq!(ran, ["Google.Chrome"], "the running package finishes, nothing else starts");
        assert_eq!(items.iter().map(|i| i.status.as_str()).collect::<Vec<_>>(), ["ok", "cancelled", "cancelled"]);
    }

    #[test]
    fn exit_codes_have_install_specific_reasons() {
        assert_eq!(reason(0), "ok");
        assert_eq!(reason(0x8A15_010D), "installed");
        assert_eq!(reason(0x8A15_0061), "installed");
        assert_eq!(reason(0x8A15_0010), "no_installer");
        assert_eq!(reason(0x8A15_0109), "reboot");
        assert_eq!(reason(0x8A15_0019), "admin");
        assert_eq!(reason(1602), "cancelled");
        assert_eq!(reason(0xDEAD_BEEF), "failed");
    }

    #[test]
    fn the_history_file_is_hostile() {
        // SAFETY: same value every data-dir test sets; see `registry::tests::test_data_dir`.
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
        let f = history_file();
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        let evil = r#"[{"at":1,"kind":"install","items":[{"id":"a\u0000b","name":"N","status":"pwned","reason":"<script>","code":1}],"reboot":false,"cancelled":false},
            {"at":2,"kind":"apps","items":[],"reboot":false,"cancelled":false}]"#;
        std::fs::write(&f, evil).unwrap();
        let h = history();
        assert_eq!(h.len(), 1, "another feature's kind is not ours");
        assert_eq!((h[0].items[0].id.as_str(), h[0].items[0].status.as_str(), h[0].items[0].reason.as_str()), ("ab", "failed", "failed"));
        std::fs::write(&f, "not json").unwrap();
        assert!(history().is_empty());
        let b = Batch { at: 3, kind: "install".into(), items: vec![], reboot: false, cancelled: false };
        updates::append_history(&f, KINDS, &b);
        assert_eq!(history().len(), 1);
    }

    /// Read-only, talks to the real winget and the registry (`cargo test -p g2boost-core -- --ignored live_install --nocapture`).
    #[test]
    #[ignore]
    fn live_install_status() {
        let (status, apps) = scan(true, &[]);
        println!("winget: {} · {} registry apps", status.winget, apps.len());
        for s in status.states.iter().filter(|s| s.installed) {
            println!("  {:<45} via {:?} v{:?} update {:?} app {:?}", s.id, s.via, s.version, s.update, s.app);
        }
        println!("{} of {} catalog apps installed", status.states.iter().filter(|s| s.installed).count(), status.states.len());
        assert_eq!(status.states.len(), catalog::all().len());
    }
}
