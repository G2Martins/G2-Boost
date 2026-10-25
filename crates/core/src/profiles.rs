//! Profiles and restoration.
//!
//! * A profile is a named set of tweak ids (settings.json `profiles`). Applying one is a plan
//!   (which tweaks to turn on, optionally which others to turn off) executed step by step through
//!   the normal per-tweak flow in `boost` (backup first, exact revert). Nothing here changes the
//!   system by itself.
//! * Restoration lists what G2 Boost changed that is still in place (tweaks, plus the backup stores
//!   other modules keep) and keeps an append-only history of the steps taken through these pages.
//! * Profiles travel as small JSON files in `Documents\G2 Boost\Perfis`, or pasted text. Everything
//!   that comes in (UI, files, pasted JSON, settings.json, the history file) is hostile input.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::boost::{self, BoostError, Status, TweakState};
use crate::registry::{self, safe_id};
use crate::visualfx::Level;

pub const MAX_PROFILES: usize = 50;
const MAX_NAME: usize = 40;
const MAX_IDS: usize = 200;
/// Largest profile file or pasted text accepted (a real profile is a few hundred bytes).
const MAX_IMPORT_BYTES: u64 = 64 * 1024;
const MAX_FILES: usize = 100;
const MAX_HISTORY: usize = 300;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProfileError {
    #[error("invalid_name")]
    InvalidName,
    #[error("name_taken")]
    NameTaken,
    #[error("too_many")]
    TooMany,
    #[error("not_found")]
    NotFound,
    #[error("file_too_big")]
    TooBig,
    #[error("bad_file")]
    BadFile,
    #[error("no_folder")]
    NoFolder,
    #[error("io: {0}")]
    Io(String),
}

/* ---------------- the profile ---------------- */

/// A named set of tweaks to turn on together.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    /// Tweak ids (`registry::safe_id`). Ids this build does not know are kept (a newer build may have
    /// them) and skipped, with a report, when the profile is applied.
    pub tweaks: Vec<String>,
    /// Visual-effects level when `visual_fx` is in `tweaks`: `minimal` | `balanced` | `performance`.
    pub fx_level: Option<String>,
}

/// `Level` <-> its serde (snake_case) name.
pub fn level_str(l: Level) -> &'static str {
    match l {
        Level::Appearance => "appearance",
        Level::Minimal => "minimal",
        Level::Balanced => "balanced",
        Level::Performance => "performance",
    }
}

/// A level a profile may ask for (`appearance` is "visual_fx off", not a level to apply).
fn level_of(s: &str) -> Option<Level> {
    match s {
        "minimal" => Some(Level::Minimal),
        "balanced" => Some(Level::Balanced),
        "performance" => Some(Level::Performance),
        _ => None,
    }
}

/// Free text from outside, safe to show and store: no control characters, bounded.
fn clean_text(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

/// 1..=40 chars: letters, digits, space and `_ - . ( ) +`. No control, separator or bidi characters
/// (the name ends up on screen and, slugged, in a file name). Runs of spaces collapse.
pub fn clean_name(raw: &str) -> Option<String> {
    let n = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let ok = !n.is_empty()
        && n.chars().count() <= MAX_NAME
        && n.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.' | '(' | ')' | '+'));
    ok.then_some(n)
}

fn clean_ids(ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.iter().filter(|id| safe_id(id) && seen.insert(id.as_str())).take(MAX_IDS).cloned().collect()
}

/// A valid profile, or `None` when the name is unusable. Ids are filtered, not rejected.
pub fn clean_profile(p: Profile) -> Option<Profile> {
    Some(Profile {
        name: clean_name(&p.name)?,
        tweaks: clean_ids(&p.tweaks),
        fx_level: p.fx_level.filter(|l| level_of(l).is_some()),
    })
}

/// settings.json is hostile too: bad profiles dropped, names unique (case-insensitive), at most 50.
pub fn sanitize_list(list: Vec<Profile>) -> Vec<Profile> {
    let mut seen = HashSet::new();
    list.into_iter()
        .filter_map(clean_profile)
        .filter(|p| seen.insert(p.name.to_lowercase()))
        .take(MAX_PROFILES)
        .collect()
}

/// Add or replace. `previous` is the name being edited (a rename replaces that entry).
pub fn upsert(list: &[Profile], profile: Profile, previous: Option<&str>) -> Result<Vec<Profile>, ProfileError> {
    let p = clean_profile(profile).ok_or(ProfileError::InvalidName)?;
    let mut out = list.to_vec();
    let at = previous.and_then(|prev| out.iter().position(|x| x.name.to_lowercase() == prev.to_lowercase()));
    if out.iter().enumerate().any(|(i, x)| Some(i) != at && x.name.to_lowercase() == p.name.to_lowercase()) {
        return Err(ProfileError::NameTaken);
    }
    match at {
        Some(i) => out[i] = p,
        None if out.len() >= MAX_PROFILES => return Err(ProfileError::TooMany),
        None => out.push(p),
    }
    Ok(out)
}

pub fn remove(list: &[Profile], name: &str) -> Result<Vec<Profile>, ProfileError> {
    let n = name.to_lowercase();
    let out: Vec<Profile> = list.iter().filter(|p| p.name.to_lowercase() != n).cloned().collect();
    if out.len() == list.len() { Err(ProfileError::NotFound) } else { Ok(out) }
}

/// "Name (2)", "Name (3)"… when an imported profile's name is already used.
pub fn unique_name(list: &[Profile], name: &str) -> String {
    let taken = |n: &str| list.iter().any(|p| p.name.to_lowercase() == n.to_lowercase());
    if !taken(name) {
        return name.to_string();
    }
    for i in 2..100 {
        let suffix = format!(" ({i})");
        let base: String = name.chars().take(MAX_NAME - suffix.chars().count()).collect();
        let cand = format!("{}{suffix}", base.trim_end());
        if !taken(&cand) {
            return cand;
        }
    }
    name.to_string()
}

/* ---------------- export / import ---------------- */

pub fn export_json(p: &Profile) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "g2boost_profile": 1,
        "name": p.name,
        "tweaks": p.tweaks,
        "fx_level": p.fx_level,
    }))
    .expect("serializable")
}

/// Pasted text or a file's content. Size first, then a strict parse, then the same cleaning as
/// everything else; a bad name is an error (the user should see why), bad ids are dropped.
pub fn parse_import(text: &str) -> Result<Profile, ProfileError> {
    if text.len() as u64 > MAX_IMPORT_BYTES {
        return Err(ProfileError::TooBig);
    }
    let v: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|_| ProfileError::BadFile)?;
    if !v.is_object() {
        return Err(ProfileError::BadFile); // serde would also read a bare array as a struct
    }
    let p: Profile = serde_json::from_value(v).map_err(|_| ProfileError::BadFile)?;
    clean_profile(p).ok_or(ProfileError::InvalidName)
}

/// `Documents\G2 Boost\Perfis` (Documents may live in OneDrive).
pub fn profiles_dir() -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Documents, KNOWN_FOLDER_FLAG, SHGetKnownFolderPath};
    // SAFETY: the API allocates the string; it is copied then freed with CoTaskMemFree.
    let known = unsafe {
        SHGetKnownFolderPath(&FOLDERID_Documents, KNOWN_FOLDER_FLAG(0), None).ok().and_then(|p| {
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const std::ffi::c_void));
            s.map(PathBuf::from)
        })
    };
    let docs = known.or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Documents")))?;
    Some(docs.join("G2 Boost").join("Perfis"))
}

/// Accented Latin letters to plain ones, so "Ação" becomes "acao" and not "a-o".
fn fold(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
        'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
        'ç' | 'Ç' => 'c',
        'ñ' | 'Ñ' => 'n',
        other => other,
    }
}

/// ASCII file name from a profile name. The `perfil-` prefix keeps it clear of device names (CON, NUL…).
pub fn file_name(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars().map(fold) {
        let c = if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' };
        if c != '-' || !slug.ends_with('-') {
            slug.push(c);
        }
    }
    let slug = slug.trim_matches('-');
    format!("perfil-{}.json", if slug.is_empty() { "sem-nome" } else { slug })
}

/// Not there, or a plain folder/file: never a junction, symlink or other reparse point.
fn not_a_link(p: &Path) -> bool {
    match std::fs::symlink_metadata(p) {
        Ok(m) => !crate::safety::is_reparse(&m),
        Err(_) => true, // not there yet
    }
}

/// Create the profiles folder, unless it (or its parent) is a link: a junction planted over it
/// would send what is written there somewhere else, with the app's privileges.
pub fn prepare_dir(dir: &Path) -> Result<(), ProfileError> {
    if !not_a_link(dir) || !dir.parent().is_none_or(not_a_link) {
        return Err(ProfileError::BadFile);
    }
    std::fs::create_dir_all(dir).map_err(|e| ProfileError::Io(e.to_string()))
}

/// Write `<dir>\perfil-<slug>.json` (same name overwrites: exporting a profile twice is one file).
/// Nothing is written through a link, the file's own name included.
pub fn export_file(dir: &Path, p: &Profile) -> Result<PathBuf, ProfileError> {
    let p = clean_profile(p.clone()).ok_or(ProfileError::InvalidName)?;
    prepare_dir(dir)?;
    let path = dir.join(file_name(&p.name));
    if let Ok(m) = std::fs::symlink_metadata(&path)
        && (!m.file_type().is_file() || crate::safety::is_reparse(&m))
    {
        return Err(ProfileError::BadFile);
    }
    std::fs::write(&path, export_json(&p)).map_err(|e| ProfileError::Io(e.to_string()))?;
    Ok(path)
}

/// A bare `.json` file name from the listing: no separators, no `..`, no hidden or odd names.
pub fn valid_file(f: &str) -> bool {
    f.len() <= 100
        && f.ends_with(".json")
        && !f.starts_with('.')
        && !f.contains("..")
        && f.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '_' | '-' | '.' | '(' | ')' | '+'))
}

pub fn read_file(dir: &Path, file: &str) -> Result<Profile, ProfileError> {
    if !valid_file(file) {
        return Err(ProfileError::BadFile);
    }
    let path = dir.join(file);
    // A real, small file: not a link, not a folder, not a device.
    let meta = std::fs::symlink_metadata(&path).map_err(|_| ProfileError::NotFound)?;
    if !meta.file_type().is_file() {
        return Err(ProfileError::BadFile);
    }
    if meta.len() > MAX_IMPORT_BYTES {
        return Err(ProfileError::TooBig);
    }
    let mut text = String::new();
    std::fs::File::open(&path)
        .and_then(|f| f.take(MAX_IMPORT_BYTES + 1).read_to_string(&mut text))
        .map_err(|_| ProfileError::BadFile)?;
    parse_import(&text)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileEntry {
    pub file: String,
    pub name: String,
    pub tweaks: usize,
}

/// Valid profile files in the folder (others are ignored, not reported).
pub fn list_files(dir: &Path) -> Vec<FileEntry> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<FileEntry> = rd
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|f| valid_file(f))
        .take(MAX_FILES)
        .filter_map(|f| read_file(dir, &f).ok().map(|p| FileEntry { file: f, name: p.name, tweaks: p.tweaks.len() }))
        .collect();
    out.sort_by_key(|f| f.name.to_lowercase());
    out
}

/* ---------------- plan: what applying a target would change ---------------- */

/// What to reach: these tweaks on, and (`off_others`) everything else that is applied turned off.
/// "Windows defaults" is `ids = []` with `off_others`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Target {
    pub ids: Vec<String>,
    pub fx_level: Option<String>,
    pub off_others: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    On,
    Off,
}

/// One tweak to flip. `level` (visual effects only): the level to apply for `On`, the level it had
/// for `Off` (so the step can be undone).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub op: Op,
    #[serde(default)]
    pub level: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Plan {
    /// Ready to run, in order: the ons first, then the offs.
    pub steps: Vec<Step>,
    /// Would need administrator and the app is not elevated.
    pub blocked: Vec<Step>,
    /// Ids this build does not have.
    pub unknown: Vec<String>,
    /// Exist, but this PC cannot use them.
    pub unavailable: Vec<String>,
    /// Wanted tweaks that were already in the wanted state.
    pub already: usize,
}

/// Pure: live states in, steps out.
pub fn plan(target: &Target, states: &[TweakState], elevated: bool) -> Plan {
    let wanted = clean_ids(&target.ids);
    let fx = target.fx_level.as_deref().and_then(level_of).unwrap_or(Level::Balanced);
    let mut plan = Plan::default();
    let put = |plan: &mut Plan, st: &TweakState, step: Step| {
        if st.admin && !elevated { plan.blocked.push(step) } else { plan.steps.push(step) }
    };
    for id in &wanted {
        let Some(st) = states.iter().find(|s| s.id == id) else {
            plan.unknown.push(id.clone());
            continue;
        };
        if st.status == Status::Unavailable {
            plan.unavailable.push(id.clone());
            continue;
        }
        let is_fx = st.id == "visual_fx";
        let satisfied = if is_fx { st.level == Some(fx) } else { st.status == Status::On };
        if satisfied {
            plan.already += 1;
            continue;
        }
        let step = Step { id: id.clone(), op: Op::On, level: is_fx.then(|| level_str(fx).to_string()) };
        put(&mut plan, st, step);
    }
    if target.off_others {
        for st in states.iter().filter(|s| !wanted.iter().any(|w| w == s.id)) {
            if matches!(st.status, Status::On | Status::Partial) {
                let step = Step { id: st.id.to_string(), op: Op::Off, level: st.level.map(|l| level_str(l).to_string()) };
                put(&mut plan, st, step);
            }
        }
    }
    plan
}

/// The plan against the live PC.
pub fn plan_now(target: &Target) -> Plan {
    plan(target, &boost::detect_all(), crate::system::is_elevated())
}

/* ---------------- execute one step ---------------- */

#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub id: String,
    pub op: Op,
    pub ok: bool,
    /// Stable code (`needs_admin`, `unknown_tweak`, `cancelled`) or a short message.
    pub error: Option<String>,
    pub state: Option<TweakState>,
}

fn exec(step: &Step) -> Result<TweakState, BoostError> {
    if !safe_id(&step.id) {
        return Err(BoostError::UnknownTweak);
    }
    let t = boost::find(&step.id)?;
    match (t.id, step.op) {
        ("visual_fx", Op::On) => boost::apply_visual_fx(step.level.as_deref().and_then(level_of).unwrap_or(Level::Balanced)),
        ("visual_fx", Op::Off) => boost::revert_visual_fx(),
        (id, op) => boost::apply(id, op == Op::On),
    }
}

/// Run one step through the normal tweak flow (backup first) and log it. `source` is a profile name,
/// or `@undo` / `@undo_all` / `@periph` for the app's own flows.
pub fn run_step(step: &Step, source: &str) -> StepResult {
    let res = exec(step);
    let error = res.as_ref().err().map(|e| clean_text(&e.to_string(), 120));
    log(vec![HistoryEntry {
        at: registry::now_unix(),
        source: source.to_string(),
        id: step.id.clone(),
        on: step.op == Op::On,
        ok: error.is_none(),
        error: error.clone(),
    }]);
    StepResult { id: step.id.chars().take(64).collect(), op: step.op, ok: error.is_none(), error, state: res.ok() }
}

/* ---------------- history (append-only, capped) ---------------- */

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub at: u64,
    /// Profile name, or `@undo`, `@undo_all`, `@periph`.
    pub source: String,
    /// Tweak id, or `periph_<setting>`.
    pub id: String,
    /// Turned on / set (`true`) or turned off / put back (`false`).
    pub on: bool,
    pub ok: bool,
    pub error: Option<String>,
}

static HISTORY_LOCK: Mutex<()> = Mutex::new(());

fn history_file() -> PathBuf {
    registry::data_dir().join("profiles-history.json")
}

fn clean_entry(e: HistoryEntry) -> Option<HistoryEntry> {
    if !safe_id(&e.id) {
        return None;
    }
    let source = clean_text(&e.source, 60);
    Some(HistoryEntry {
        source: if source.is_empty() { "@manual".into() } else { source },
        error: e.error.map(|x| clean_text(&x, 120)).filter(|x| !x.is_empty()),
        ..e
    })
}

/// Oldest first, as stored; the file is hostile: bad entries dropped, newest 300 kept.
fn load_history() -> Vec<HistoryEntry> {
    let raw: Vec<serde_json::Value> =
        std::fs::read_to_string(history_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let mut list: Vec<HistoryEntry> =
        raw.into_iter().filter_map(|v| serde_json::from_value(v).ok()).filter_map(clean_entry).collect();
    cap(&mut list);
    list
}

fn cap(list: &mut Vec<HistoryEntry>) {
    if list.len() > MAX_HISTORY {
        list.drain(..list.len() - MAX_HISTORY);
    }
}

/// Newest first.
pub fn history() -> Vec<HistoryEntry> {
    let mut h = load_history();
    h.reverse();
    h
}

/// Append (best effort: a full disk must not break the change that was just made).
pub fn log(new: Vec<HistoryEntry>) {
    let _g = HISTORY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut all = load_history();
    all.extend(new.into_iter().filter_map(clean_entry));
    cap(&mut all);
    let _ = std::fs::create_dir_all(registry::data_dir());
    let _ = std::fs::write(history_file(), serde_json::to_string(&all).unwrap_or_default());
}

/* ---------------- restoration: what is still in place ---------------- */

#[derive(Debug, Clone, Serialize)]
pub struct AppliedTweak {
    pub id: &'static str,
    pub status: Status,
    pub has_backup: bool,
    pub admin: bool,
    pub detail: Option<String>,
}

/// A change another module keeps its own backup for. The screen that undoes it is the UI's to name.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Other {
    /// `dns` | `gpu_driver` | `gpu_pref` | `exposure` | `qos` | `apps_undo` | `game_mode` | `peripherals`
    pub kind: &'static str,
    pub count: usize,
    /// A few labels from the data (adapter, game, exe). Never paths.
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Restoration {
    pub tweaks: Vec<AppliedTweak>,
    pub others: Vec<Other>,
}

fn other(kind: &'static str, count: usize, names: Vec<String>) -> Option<Other> {
    let names: Vec<String> = names.into_iter().map(|n| clean_text(&n, 64)).filter(|n| !n.is_empty()).take(8).collect();
    (count > 0).then_some(Other { kind, count, names })
}

fn exe_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// `gpu_settings_backup.json`: how many driver settings are saved, and for which targets
/// (`global` or a game's exe). Read as plain JSON: this module does not need the driver.
fn gpu_driver_saved(text: &str) -> (usize, Vec<String>) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return (0, Vec::new()) };
    let Some(entries) = v.get("entries").and_then(|e| e.as_array()) else { return (0, Vec::new()) };
    let mut targets: Vec<String> = Vec::new();
    let mut count = 0;
    for e in entries.iter().take(4096) {
        let Some(t) = e.get("target").and_then(|t| t.as_str()) else { continue };
        count += 1;
        if !targets.iter().any(|x| x == t) {
            targets.push(t.to_string());
        }
    }
    (count, targets)
}

fn gpu_driver_other() -> Option<Other> {
    let path = registry::data_dir().join("gpu_settings_backup.json");
    if std::fs::metadata(&path).ok()?.len() > 1 << 20 {
        return None;
    }
    let (count, targets) = gpu_driver_saved(&std::fs::read_to_string(path).ok()?);
    other("gpu_driver", count, targets)
}

fn others() -> Vec<Other> {
    let mut out = Vec::new();
    let dns: Vec<String> = crate::dns::adapters().into_iter().filter(|a| crate::dns::has_backup(&a.guid)).map(|a| a.name).collect();
    out.extend(other("dns", dns.len(), dns));
    out.extend(gpu_driver_other());
    if let Some(b) = registry::load_backup("gpu_pref") {
        out.extend(other("gpu_pref", b.entries.len(), b.entries.iter().map(|e| exe_name(&e.name).to_string()).collect()));
    }
    out.extend(other("exposure", crate::exposure::reverts().len(), Vec::new()));
    if let Ok(list) = crate::qos::list_ours() {
        out.extend(other("qos", list.len(), list.into_iter().map(|p| p.app).collect()));
    }
    if let Some(u) = crate::apps::undo_status() {
        out.extend(other("apps_undo", u.files.len() + u.keys.len(), vec![u.app]));
    }
    if let Some(s) = crate::gamemode::session() {
        out.extend(other("game_mode", 1, vec![s.game_name]));
    }
    out.extend(other("peripherals", crate::peripherals::originals_count(), Vec::new()));
    out
}

pub fn restoration() -> Restoration {
    let tweaks = boost::detect_all()
        .into_iter()
        .filter(|s| matches!(s.status, Status::On | Status::Partial))
        .map(|s| AppliedTweak { id: s.id, status: s.status, has_backup: s.has_backup, admin: s.admin, detail: s.detail })
        .collect();
    Restoration { tweaks, others: others() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, ids: &[&str]) -> Profile {
        Profile { name: name.into(), tweaks: ids.iter().map(|s| s.to_string()).collect(), fx_level: None }
    }

    /// A tweak state with only the fields the planner reads set. Built from a real one, so a field
    /// added to `TweakState` later does not break these fixtures.
    fn st(id: &'static str, status: Status, admin: bool) -> TweakState {
        static BASE: std::sync::OnceLock<TweakState> = std::sync::OnceLock::new();
        let mut s = BASE.get_or_init(|| boost::detect_all().remove(0)).clone();
        s.id = id;
        s.admin = admin;
        s.beta = false;
        s.status = status;
        s.has_backup = false;
        s.level = None;
        s.detail = None;
        s.services.clear();
        s
    }

    fn target(ids: &[&str], off_others: bool) -> Target {
        Target { ids: ids.iter().map(|s| s.to_string()).collect(), fx_level: None, off_others }
    }

    fn ids(steps: &[Step]) -> Vec<(&str, Op)> {
        steps.iter().map(|s| (s.id.as_str(), s.op)).collect()
    }

    #[test]
    fn names_are_short_plain_and_unambiguous() {
        assert_eq!(clean_name("  Jogos   rápidos  ").as_deref(), Some("Jogos rápidos"));
        assert_eq!(clean_name("CS2 (2026)+").as_deref(), Some("CS2 (2026)+"));
        for bad in [
            "",
            "   ",
            "a\u{0}b",
            "a\nb\u{202e}",
            "..\\x",
            "a/b",
            "a:b",
            "a*b",
            "a|b",
            "<b>",
            "emoji 🙂",
            &"x".repeat(41),
        ] {
            assert_eq!(clean_name(bad), None, "{bad:?}");
        }
        assert_eq!(clean_name(&"x".repeat(40)).map(|n| n.len()), Some(40));
    }

    #[test]
    fn sanitize_drops_bad_ids_dupes_and_caps() {
        let many: Vec<String> = (0..300).map(|i| format!("t{i}")).collect();
        let mut raw = vec![
            Profile {
                name: "A".into(),
                tweaks: vec![
                    "game_mode".into(),
                    "game_mode".into(),
                    "../evil".into(),
                    "UPPER".into(),
                    "with space".into(),
                    "".into(),
                    "x".repeat(65),
                    "mouse".into(),
                ],
                fx_level: Some("performance".into()),
            },
            p("a", &["mouse"]),                                // same name, other case
            Profile { name: "B".into(), tweaks: many, fx_level: Some("appearance".into()) },
            p("bad\u{1}name", &["mouse"]),
        ];
        for i in 0..80 {
            raw.push(p(&format!("P{i}"), &[]));
        }
        let got = sanitize_list(raw);
        assert_eq!(got.len(), MAX_PROFILES);
        assert_eq!(got[0].tweaks, vec!["game_mode", "mouse"]);
        assert_eq!(got[0].fx_level.as_deref(), Some("performance"));
        assert_eq!(got[1].name, "B");
        assert_eq!(got[1].tweaks.len(), 200, "ids capped");
        assert_eq!(got[1].fx_level, None, "appearance is not a level to apply");
        assert!(got.iter().all(|x| x.name != "a" && x.name != "bad\u{1}name"));
    }

    #[test]
    fn upsert_rename_conflict_and_limit() {
        let list = vec![p("Jogos", &["mouse"]), p("Trabalho", &[])];
        let new = upsert(&list, p("Streaming", &["tips"]), None).unwrap();
        assert_eq!(new.len(), 3);
        // Edit in place, rename keeps the slot.
        let renamed = upsert(&list, p("JOGOS 2", &["mouse"]), Some("jogos")).unwrap();
        assert_eq!(renamed.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), vec!["JOGOS 2", "Trabalho"]);
        // Same name (any case) as another profile.
        assert_eq!(upsert(&list, p("trabalho", &[]), None), Err(ProfileError::NameTaken));
        assert_eq!(upsert(&list, p("TRABALHO", &[]), Some("Jogos")), Err(ProfileError::NameTaken));
        // Keeping its own name while editing is fine.
        assert!(upsert(&list, p("jogos", &["tips"]), Some("Jogos")).is_ok());
        assert_eq!(upsert(&list, p("x\u{7}y", &[]), None), Err(ProfileError::InvalidName));
        // Tabs and line breaks are just whitespace: they collapse to one space.
        assert_eq!(upsert(&list, p("x\ty\n z", &[]), None).unwrap()[2].name, "x y z");
        let full: Vec<Profile> = (0..MAX_PROFILES).map(|i| p(&format!("P{i}"), &[])).collect();
        assert_eq!(upsert(&full, p("Novo", &[]), None), Err(ProfileError::TooMany));
        assert!(upsert(&full, p("P3", &["mouse"]), Some("P3")).is_ok(), "editing a full list still works");
        assert_eq!(remove(&list, "JOGOS").unwrap().len(), 1);
        assert_eq!(remove(&list, "nope"), Err(ProfileError::NotFound));
    }

    #[test]
    fn unique_name_adds_a_counter_inside_the_limit() {
        let list = vec![p("Jogos", &[]), p("Jogos (2)", &[])];
        assert_eq!(unique_name(&list, "Outro"), "Outro");
        assert_eq!(unique_name(&list, "jogos"), "jogos (3)");
        let long = "x".repeat(40);
        let got = unique_name(&[p(&long, &[])], &long);
        assert!(got.chars().count() <= 40 && got.ends_with(" (2)"), "{got}");
    }

    #[test]
    fn export_import_roundtrip() {
        let orig = Profile { name: "Alto desempenho".into(), tweaks: vec!["visual_fx".into(), "mouse".into()], fx_level: Some("performance".into()) };
        let text = export_json(&orig);
        assert!(text.contains("\"g2boost_profile\": 1"));
        assert_eq!(parse_import(&text).unwrap(), orig);
        // A hand-written minimal file and one with a BOM work too.
        assert_eq!(parse_import("{\"name\":\"X\",\"tweaks\":[\"mouse\"]}").unwrap().tweaks, vec!["mouse"]);
        assert_eq!(parse_import(&format!("\u{feff}{text}")).unwrap(), orig);
    }

    #[test]
    fn import_rejects_hostile_input() {
        assert_eq!(parse_import(&" ".repeat(70_000)), Err(ProfileError::TooBig));
        for bad in [
            "",
            "not json",
            "[]",
            "null",
            "\"name\"",
            "{\"name\":\"X\",\"tweaks\":\"mouse\"}",
            "{\"name\":\"X\",\"tweaks\":[1,2]}",
            "{\"name\":\"X\",\"tweaks\":{\"a\":1}}",
            "{\"name\":5,\"tweaks\":[]}",
            &"[".repeat(5000),
        ] {
            assert_eq!(parse_import(bad), Err(ProfileError::BadFile), "{bad:.40}");
        }
        for bad in ["{}", "{\"tweaks\":[\"mouse\"]}", "{\"name\":\"a\\u0000b\"}", "{\"name\":\"..\\\\..\\\\x\"}", "{\"name\":\"   \"}"] {
            assert_eq!(parse_import(bad), Err(ProfileError::InvalidName), "{bad}");
        }
        // Hostile ids and levels are cleaned, not trusted.
        let got = parse_import(r#"{"name":"X","tweaks":["mouse","../../x","C:\\evil","a b","game_mode"],"fx_level":"warp","extra":{"a":1}}"#).unwrap();
        assert_eq!(got.tweaks, vec!["mouse", "game_mode"]);
        assert_eq!(got.fx_level, None);
        // Many ids inside the size limit are capped.
        let ids: Vec<String> = (0..5000).map(|i| format!("\"t{i}\"")).collect();
        let big = parse_import(&format!("{{\"name\":\"X\",\"tweaks\":[{}]}}", ids.join(","))).unwrap_or_default();
        assert!(big.tweaks.len() <= MAX_IDS);
    }

    #[test]
    fn file_names_are_slugs_clear_of_device_names() {
        assert_eq!(file_name("Alto desempenho"), "perfil-alto-desempenho.json");
        assert_eq!(file_name("CON"), "perfil-con.json");
        assert_eq!(file_name("Ação (2)"), "perfil-acao-2.json");
        assert_eq!(file_name("Padrão do Windows"), "perfil-padrao-do-windows.json");
        assert_eq!(file_name("日本語"), "perfil-sem-nome.json", "other scripts cannot be slugged");
        assert_eq!(file_name("..."), "perfil-sem-nome.json");
        assert!(valid_file(&file_name("Jogos")));
        for bad in ["", ".json", "a.txt", "..\\x.json", "a/b.json", "a\\b.json", ".hidden.json", "a..json", "a:b.json", "CON:.json", "a\u{0}.json"] {
            assert!(!valid_file(bad), "{bad:?}");
        }
        assert!(!valid_file(&format!("{}.json", "x".repeat(100))));
    }

    #[test]
    fn files_in_the_folder_export_list_and_read() {
        let root = std::env::temp_dir().join(format!("g2boost-profiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("G2 Boost").join("Perfis");
        let path = export_file(&dir, &p("Jogos", &["mouse", "game_mode"])).unwrap();
        assert_eq!(path.file_name().unwrap(), "perfil-jogos.json");
        export_file(&dir, &p("Alpha", &[])).unwrap();
        // Junk the folder may hold.
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        std::fs::write(dir.join("broken.json"), "{ nope").unwrap();
        std::fs::write(dir.join("big.json"), " ".repeat(70_000)).unwrap();
        std::fs::create_dir_all(dir.join("folder.json")).unwrap();
        let list = list_files(&dir);
        assert_eq!(list.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), vec!["Alpha", "Jogos"]);
        assert_eq!(list[1], FileEntry { file: "perfil-jogos.json".into(), name: "Jogos".into(), tweaks: 2 });
        assert_eq!(read_file(&dir, "perfil-jogos.json").unwrap().tweaks, vec!["mouse", "game_mode"]);
        // Hostile names and special files.
        assert_eq!(read_file(&dir, "..\\perfil-jogos.json"), Err(ProfileError::BadFile));
        assert_eq!(read_file(&dir, "../perfil-jogos.json"), Err(ProfileError::BadFile));
        assert_eq!(read_file(&dir, "folder.json"), Err(ProfileError::BadFile));
        assert_eq!(read_file(&dir, "big.json"), Err(ProfileError::TooBig));
        assert_eq!(read_file(&dir, "missing.json"), Err(ProfileError::NotFound));
        assert_eq!(read_file(&dir, "broken.json"), Err(ProfileError::BadFile));
        assert_eq!(export_file(&dir, &p("a\u{1}b", &[])), Err(ProfileError::InvalidName));
        assert!(list_files(&dir.join("nope")).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn export_never_writes_through_a_junction() {
        let root = std::env::temp_dir().join(format!("g2boost-profiles-junction-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let victim = root.join("victim");
        std::fs::create_dir_all(&victim).unwrap();
        // The Perfis folder replaced by a junction to somewhere else.
        let linked = root.join("a").join("Perfis");
        std::fs::create_dir_all(linked.parent().unwrap()).unwrap();
        crate::cleanup::tests::make_junction(&linked, &victim);
        assert_eq!(export_file(&linked, &p("Jogos", &["mouse"])), Err(ProfileError::BadFile));
        // The parent (G2 Boost) replaced by a junction.
        let parent = root.join("b");
        crate::cleanup::tests::make_junction(&parent, &victim);
        assert_eq!(export_file(&parent.join("Perfis"), &p("Jogos", &[])), Err(ProfileError::BadFile));
        // The file's own name taken by a junction (a folder link) instead of a file.
        let dir = root.join("c").join("Perfis");
        std::fs::create_dir_all(&dir).unwrap();
        crate::cleanup::tests::make_junction(&dir.join(file_name("Jogos")), &victim);
        assert_eq!(export_file(&dir, &p("Jogos", &[])), Err(ProfileError::BadFile));
        assert_eq!(read_file(&dir, &file_name("Jogos")), Err(ProfileError::BadFile));
        assert_eq!(std::fs::read_dir(&victim).unwrap().count(), 0, "nothing was written behind a link");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn profiles_folder_is_under_documents() {
        let d = profiles_dir().expect("a documents folder");
        assert!(d.ends_with(r"G2 Boost\Perfis"), "{d:?}");
    }

    #[test]
    fn plan_turns_on_what_is_missing_and_reports_the_rest() {
        let states = [
            st("mouse", Status::Off, false),
            st("keyboard", Status::On, false),
            st("tips", Status::Partial, false),
            st("hags", Status::Off, true),
            st("svc_spooler", Status::Unavailable, true),
        ];
        let t = target(&["mouse", "keyboard", "tips", "hags", "svc_spooler", "copilot_off", "mouse", "../x", "UP"], false);
        let plan = plan(&t, &states, false);
        assert_eq!(ids(&plan.steps), vec![("mouse", Op::On), ("tips", Op::On)], "partial completes, on is left alone");
        assert_eq!(ids(&plan.blocked), vec![("hags", Op::On)], "admin without elevation");
        assert_eq!(plan.unknown, vec!["copilot_off"], "unknown id reported once; unsafe ids vanish");
        assert_eq!(plan.unavailable, vec!["svc_spooler"]);
        assert_eq!(plan.already, 1);
        // Elevated: nothing is blocked.
        let plan = super::plan(&t, &states, true);
        assert_eq!(ids(&plan.steps), vec![("mouse", Op::On), ("tips", Op::On), ("hags", Op::On)]);
        assert!(plan.blocked.is_empty());
    }

    #[test]
    fn plan_can_turn_off_the_others() {
        let states = [
            st("mouse", Status::Off, false),
            st("keyboard", Status::On, false),
            st("tips", Status::Partial, false),
            st("hags", Status::On, true),
            st("mmcss", Status::Off, true),
            st("svc_spooler", Status::Unavailable, true),
        ];
        // Profile = mouse only; the rest that is applied goes off. Offs follow the ons.
        let plan = plan(&target(&["mouse"], true), &states, false);
        assert_eq!(ids(&plan.steps), vec![("mouse", Op::On), ("keyboard", Op::Off), ("tips", Op::Off)]);
        assert_eq!(ids(&plan.blocked), vec![("hags", Op::Off)]);
        // Windows defaults: every applied tweak off, nothing on.
        let plan = super::plan(&target(&[], true), &states, true);
        assert_eq!(ids(&plan.steps), vec![("keyboard", Op::Off), ("tips", Op::Off), ("hags", Op::Off)]);
        // Without `off_others` the rest is left alone.
        assert!(super::plan(&target(&[], false), &states, true).steps.is_empty());
    }

    #[test]
    fn plan_handles_the_visual_effects_level() {
        let mut fx = st("visual_fx", Status::On, false);
        fx.level = Some(Level::Performance);
        let states = [fx.clone()];
        let mut t = target(&["visual_fx"], false);
        // Default wanted level is balanced: performance must be re-applied as balanced.
        let p1 = plan(&t, &states, false);
        assert_eq!(p1.steps, vec![Step { id: "visual_fx".into(), op: Op::On, level: Some("balanced".into()) }]);
        t.fx_level = Some("performance".into());
        let p2 = plan(&t, &states, false);
        assert!(p2.steps.is_empty());
        assert_eq!(p2.already, 1);
        // An invalid level falls back to balanced.
        t.fx_level = Some("warp".into());
        assert_eq!(plan(&t, &states, false).steps[0].level.as_deref(), Some("balanced"));
        // A custom mix (no level, partial) is brought to the wanted level.
        let mut mix = st("visual_fx", Status::Partial, false);
        mix.level = None;
        assert_eq!(plan(&target(&["visual_fx"], false), &[mix], false).steps.len(), 1);
        // Turning it off keeps the level it had, so the step can be undone.
        let off = plan(&target(&[], true), &states, false);
        assert_eq!(off.steps, vec![Step { id: "visual_fx".into(), op: Op::Off, level: Some("performance".into()) }]);
    }

    #[test]
    fn a_step_for_an_unknown_or_hostile_id_changes_nothing() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use_test_dir();
        for id in ["no_such_tweak", "../../etc", "UPPER", ""] {
            let r = run_step(&Step { id: id.into(), op: Op::On, level: None }, "test");
            assert!(!r.ok, "{id:?}");
            assert_eq!(r.error.as_deref(), Some("unknown_tweak"));
            assert!(r.state.is_none());
        }
        let h = history();
        assert!(h.iter().all(|e| safe_id(&e.id)), "only valid ids reach the log");
    }

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn use_test_dir() {
        // SAFETY: same override every data-dir test in this crate uses (process-wide env var).
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
    }

    fn entry(i: u64) -> HistoryEntry {
        HistoryEntry { at: i, source: "t".into(), id: "mouse".into(), on: true, ok: true, error: None }
    }

    #[test]
    fn history_is_capped_ordered_and_cleaned() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use_test_dir();
        let _ = std::fs::remove_file(history_file());
        assert!(history().is_empty());
        log((1..=250).map(entry).collect());
        log((251..=400).map(entry).collect());
        let h = history();
        assert_eq!(h.len(), MAX_HISTORY);
        assert_eq!(h[0].at, 400, "newest first");
        assert_eq!(h.last().unwrap().at, 101, "the oldest 100 fell off");
        // Hostile entries are dropped or cleaned on the way in.
        log(vec![
            HistoryEntry { id: "../x".into(), ..entry(1) },
            HistoryEntry { source: "a\u{0}\nb".repeat(40), error: Some("bad\u{7}\n".repeat(60)), ..entry(2) },
            HistoryEntry { source: "  ".into(), ..entry(3) },
        ]);
        let h = history();
        assert_eq!(h.len(), MAX_HISTORY);
        assert_eq!(h[0].source, "@manual");
        assert!(h[1].source.chars().count() <= 60 && !h[1].source.chars().any(char::is_control));
        assert!(h[1].error.as_deref().is_some_and(|e| e.chars().count() <= 120 && !e.chars().any(char::is_control)));
        assert!(h.iter().all(|e| e.id != "../x"));
        // A tampered file never panics and keeps only what is valid.
        std::fs::write(history_file(), r#"[{"at":1,"source":"s","id":"mouse","on":true,"ok":true,"error":null},{"nope":1},"str"]"#).unwrap();
        assert_eq!(history().len(), 1, "bad elements are skipped, the valid one stays");
        std::fs::write(history_file(), "{{{").unwrap();
        assert!(history().is_empty());
        let _ = std::fs::remove_file(history_file());
    }

    /// Flips the real "show file extensions" setting of this user (HKCU, no admin) through a profile
    /// step and puts it back, checking the plan, the backup and the history on the way. Backups go
    /// to the test data dir. Run by hand: `cargo test -p g2boost-core -- --ignored profiles_live`.
    #[test]
    #[ignore]
    fn profiles_live_step_roundtrip() {
        /// Writes the value back even when an assertion fails halfway.
        struct Back(registry::Value);
        const PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
        impl Drop for Back {
            fn drop(&mut self) {
                let _ = registry::write(registry::Root::Hkcu, PATH, "HideFileExt", &self.0);
            }
        }
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use_test_dir();
        let _ = std::fs::remove_file(history_file());
        registry::delete_backup("show_extensions");
        let _back = Back(registry::read(registry::Root::Hkcu, PATH, "HideFileExt").unwrap());
        let status = || boost::detect(boost::find("show_extensions").unwrap()).status;
        let start = status();
        assert!(matches!(start, Status::On | Status::Off), "{start:?}");
        let was_on = start == Status::On;
        let (flip, back) = if was_on { (Op::Off, Op::On) } else { (Op::On, Op::Off) };
        let step = |op| Step { id: "show_extensions".into(), op, level: None };

        // The planner sees the flip before it happens.
        let want_on = target(&["show_extensions"], false);
        assert_eq!(plan_now(&want_on).steps.len(), usize::from(!was_on));

        let r = run_step(&step(flip), "live test");
        assert!(r.ok, "{:?}", r.error);
        assert_eq!(status(), if was_on { Status::Off } else { Status::On });
        assert_eq!(plan_now(&want_on).steps.len(), usize::from(was_on));

        let r = run_step(&step(back), "live test");
        assert!(r.ok, "{:?}", r.error);
        assert_eq!(status(), start, "the setting is back where it was");

        let h = history();
        assert!(h.len() >= 2 && h[0].ok && h[1].ok && h[0].source == "live test", "{h:?}");
        assert_eq!((h[0].on, h[1].on), (back == Op::On, flip == Op::On));
        let _ = std::fs::remove_file(history_file());
        registry::delete_backup("show_extensions");
    }

    #[test]
    fn gpu_backup_summary_reads_only_what_it_needs() {
        let (n, t) = gpu_driver_saved(r#"{"entries":[{"target":"global","drs":1,"prev":null},{"target":"cs2.exe","drs":2,"prev":3},{"target":"global","drs":4,"prev":1}],"created":[]}"#);
        assert_eq!((n, t), (3, vec!["global".to_string(), "cs2.exe".to_string()]));
        for bad in ["", "[]", "{}", r#"{"entries":5}"#, r#"{"entries":[1,"a",{"x":1}]}"#, "{{"] {
            assert_eq!(gpu_driver_saved(bad), (0, Vec::new()), "{bad}");
        }
    }

    #[test]
    fn other_entries_are_bounded_and_clean() {
        assert_eq!(other("dns", 0, vec!["x".into()]), None);
        let o = other("dns", 12, (0..20).map(|i| format!("a\u{0}{i}{}", "x".repeat(100))).collect()).unwrap();
        assert_eq!(o.count, 12);
        assert_eq!(o.names.len(), 8);
        assert!(o.names.iter().all(|n| n.chars().count() <= 64 && !n.chars().any(char::is_control)));
        assert_eq!(exe_name(r"C:\Games\cs2\bin\cs2.exe"), "cs2.exe");
        assert_eq!(exe_name("cs2.exe"), "cs2.exe");
    }

    #[test]
    fn settings_json_keeps_only_clean_profiles() {
        use crate::settings::{Settings, sanitize};
        let text = r#"{"profiles":[
            {"name":"Jogos","tweaks":["mouse","../x","game_mode"],"fx_level":"minimal"},
            {"name":"jogos","tweaks":[]},
            {"name":"bad\u0000","tweaks":["mouse"]},
            {"tweaks":["mouse"]}
        ]}"#;
        let s = sanitize(serde_json::from_str::<Settings>(text).unwrap());
        assert_eq!(s.profiles, vec![Profile { name: "Jogos".into(), tweaks: vec!["mouse".into(), "game_mode".into()], fx_level: Some("minimal".into()) }]);
        // Old settings files have no `profiles`.
        let old = sanitize(serde_json::from_str::<Settings>(r#"{"game_ram_clean":true}"#).unwrap());
        assert!(old.profiles.is_empty());
        // And the field survives a save/load round trip.
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.profiles, s.profiles);
    }

    #[test]
    fn restoration_reads_without_admin() {
        let r = restoration();
        assert!(r.tweaks.iter().all(|t| registry::safe_id(t.id) && matches!(t.status, Status::On | Status::Partial)));
        assert!(r.others.iter().all(|o| o.count > 0));
    }
}
