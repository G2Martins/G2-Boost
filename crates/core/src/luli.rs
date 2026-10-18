//! Luli's local history: conversations (grouped like Claude's sidebar), what she should remember
//! about this user, nothing else. Files in `%LOCALAPPDATA%\G2 Boost\luli\`, never sent anywhere
//! by this module. Everything coming from the webview is hostile input: ids, names and sizes are
//! validated, messages are kept as opaque JSON but bounded.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_CONVERSATIONS: usize = 300;
const MAX_MESSAGES: usize = 200;
const MAX_CONVERSATION_BYTES: usize = 2 << 20;
const MAX_GROUPS: usize = 40;
const MAX_MEMORY: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Group {
    pub id: String,
    pub name: String,
}

/// A conversation as the sidebar lists it (no messages).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Summary {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub group: Option<String>,
    pub created: u64,
    pub updated: u64,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub updated: u64,
    /// Transcript as the UI keeps it (question, answer with actions and sources): opaque, bounded.
    pub messages: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Index {
    groups: Vec<Group>,
    conversations: Vec<Summary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryItem {
    pub id: String,
    pub text: String,
    pub created: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct History {
    pub groups: Vec<Group>,
    /// Most recently updated first.
    pub conversations: Vec<Summary>,
}

#[derive(Debug, thiserror::Error)]
pub enum LuliError {
    #[error("invalid_id")]
    InvalidId,
    #[error("invalid_text")]
    InvalidText,
    #[error("too_large")]
    TooLarge,
    #[error("not_found")]
    NotFound,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

fn dir() -> PathBuf {
    crate::registry::data_dir().join("luli")
}

/// Ids the UI makes (`c-<time>-<rand>`): short, lowercase, no path characters.
pub fn valid_id(id: &str) -> bool {
    (3..=48).contains(&id.len()) && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A single printable line, trimmed and bounded (titles, group names, memory items).
fn clean(s: &str, max: usize) -> Result<String, LuliError> {
    let t: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        return Err(LuliError::InvalidText);
    }
    Ok(t.chars().take(max).collect())
}

fn read<T: for<'de> Deserialize<'de> + Default>(name: &str) -> T {
    std::fs::read_to_string(dir().join(name)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write<T: Serialize>(name: &str, v: &T) -> Result<(), LuliError> {
    std::fs::create_dir_all(dir())?;
    let tmp = dir().join(format!("{name}.tmp"));
    std::fs::write(&tmp, serde_json::to_vec(v).unwrap_or_default())?;
    std::fs::rename(tmp, dir().join(name))?;
    Ok(())
}

fn load_index() -> Index {
    let mut ix: Index = read("index.json");
    // Hostile file: keep only well-formed entries.
    ix.groups.retain(|g| valid_id(&g.id) && !g.name.is_empty());
    ix.conversations.retain(|c| valid_id(&c.id));
    ix
}

pub fn history() -> History {
    let ix = load_index();
    let mut conversations = ix.conversations;
    conversations.sort_by_key(|c| std::cmp::Reverse(c.updated));
    History { groups: ix.groups, conversations }
}

pub fn get(id: &str) -> Result<Conversation, LuliError> {
    if !valid_id(id) {
        return Err(LuliError::InvalidId);
    }
    let t = std::fs::read_to_string(dir().join(format!("{id}.json"))).map_err(|_| LuliError::NotFound)?;
    serde_json::from_str(&t).map_err(|_| LuliError::NotFound)
}

/// Insert or replace a conversation (the UI saves after each answer, rename or move).
pub fn save(mut c: Conversation) -> Result<Summary, LuliError> {
    if !valid_id(&c.id) {
        return Err(LuliError::InvalidId);
    }
    c.title = clean(&c.title, 120)?;
    let mut ix = load_index();
    if c.group.as_ref().is_some_and(|g| !ix.groups.iter().any(|x| &x.id == g)) {
        c.group = None;
    }
    if c.messages.len() > MAX_MESSAGES {
        let n = c.messages.len();
        c.messages.drain(..n - MAX_MESSAGES);
    }
    if serde_json::to_vec(&c.messages).map_or(usize::MAX, |b| b.len()) > MAX_CONVERSATION_BYTES {
        return Err(LuliError::TooLarge);
    }
    let now = crate::registry::now_unix();
    let created = ix.conversations.iter().find(|s| s.id == c.id).map_or(now, |s| s.created);
    c.created = created;
    c.updated = now;
    write(&format!("{}.json", c.id), &c)?;
    let summary = Summary { id: c.id.clone(), title: c.title.clone(), group: c.group.clone(), created, updated: now, count: c.messages.len() };
    ix.conversations.retain(|s| s.id != c.id);
    ix.conversations.push(summary.clone());
    // Oldest go first when the history is full.
    if ix.conversations.len() > MAX_CONVERSATIONS {
        ix.conversations.sort_by_key(|s| s.updated);
        let drop: Vec<String> = ix.conversations.drain(..ix.conversations.len() - MAX_CONVERSATIONS).map(|s| s.id).collect();
        for id in drop {
            let _ = std::fs::remove_file(dir().join(format!("{id}.json")));
        }
    }
    write("index.json", &ix)?;
    Ok(summary)
}

pub fn rename(id: &str, title: &str) -> Result<(), LuliError> {
    let mut c = get(id)?;
    c.title = title.to_string();
    save(c).map(|_| ())
}

pub fn set_group(id: &str, group: Option<&str>) -> Result<(), LuliError> {
    let mut c = get(id)?;
    c.group = group.map(str::to_string);
    save(c).map(|_| ())
}

pub fn delete(id: &str) -> Result<(), LuliError> {
    if !valid_id(id) {
        return Err(LuliError::InvalidId);
    }
    let mut ix = load_index();
    ix.conversations.retain(|s| s.id != id);
    let _ = std::fs::remove_file(dir().join(format!("{id}.json")));
    write("index.json", &ix)
}

/// Delete every conversation and group (the memory stays: it has its own list).
pub fn clear() -> Result<(), LuliError> {
    for s in load_index().conversations {
        let _ = std::fs::remove_file(dir().join(format!("{}.json", s.id)));
    }
    write("index.json", &Index::default())
}

/// Create (new id) or rename (existing id) a group.
pub fn put_group(id: &str, name: &str) -> Result<Vec<Group>, LuliError> {
    if !valid_id(id) {
        return Err(LuliError::InvalidId);
    }
    let name = clean(name, 60)?;
    let mut ix = load_index();
    if let Some(g) = ix.groups.iter_mut().find(|g| g.id == id) {
        g.name = name;
    } else if ix.groups.len() < MAX_GROUPS {
        ix.groups.push(Group { id: id.into(), name });
    } else {
        return Err(LuliError::TooLarge);
    }
    write("index.json", &ix)?;
    Ok(ix.groups)
}

/// Remove a group; its conversations go back to "no group".
pub fn delete_group(id: &str) -> Result<Vec<Group>, LuliError> {
    let mut ix = load_index();
    ix.groups.retain(|g| g.id != id);
    for c in ix.conversations.iter_mut().filter(|c| c.group.as_deref() == Some(id)) {
        c.group = None;
        if let Ok(mut full) = get(&c.id) {
            full.group = None;
            let _ = write(&format!("{}.json", c.id), &full);
        }
    }
    write("index.json", &ix)?;
    Ok(ix.groups)
}

/* ---------------- memory ---------------- */

pub fn memory() -> Vec<MemoryItem> {
    let mut m: Vec<MemoryItem> = read("memory.json");
    m.retain(|i| valid_id(&i.id) && !i.text.is_empty());
    m.truncate(MAX_MEMORY);
    m
}

/// Remember one fact the user approved ("Prefere jogar em 1440p", "Não mexer na pasta da Steam").
pub fn remember(text: &str) -> Result<Vec<MemoryItem>, LuliError> {
    let text = clean(text, 300)?;
    let mut m = memory();
    if m.iter().any(|i| i.text.eq_ignore_ascii_case(&text)) {
        return Ok(m);
    }
    if m.len() >= MAX_MEMORY {
        return Err(LuliError::TooLarge);
    }
    let now = crate::registry::now_unix();
    m.push(MemoryItem { id: format!("m-{now}-{}", m.len()), text, created: now });
    write("memory.json", &m)?;
    Ok(m)
}

pub fn forget(id: &str) -> Result<Vec<MemoryItem>, LuliError> {
    let mut m = memory();
    m.retain(|i| i.id != id);
    write("memory.json", &m)?;
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_groups_and_memory_round_trip() {
        // SAFETY: same override every data-dir test in this crate uses (process-wide env var).
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
        let _ = clear();
        let conv = |id: &str, title: &str| Conversation {
            id: id.into(),
            title: title.into(),
            group: None,
            created: 0,
            updated: 0,
            messages: vec![serde_json::json!({ "role": "user", "text": "oi" })],
        };
        save(conv("c-1-a", "Minhas temperaturas\n e consumo")).unwrap();
        assert_eq!(get("c-1-a").unwrap().title, "Minhas temperaturas e consumo", "one line");
        put_group("g-1-x", "Jogos").unwrap();
        set_group("c-1-a", Some("g-1-x")).unwrap();
        assert_eq!(history().conversations[0].group.as_deref(), Some("g-1-x"));
        set_group("c-1-a", Some("g-9-missing")).unwrap();
        assert_eq!(get("c-1-a").unwrap().group, None, "unknown group dropped");
        set_group("c-1-a", Some("g-1-x")).unwrap();
        delete_group("g-1-x").unwrap();
        assert_eq!(get("c-1-a").unwrap().group, None, "back to no group");
        rename("c-1-a", "Temperaturas").unwrap();
        assert_eq!(history().conversations[0].title, "Temperaturas");
        // Hostile ids and texts.
        assert!(matches!(save(conv("../evil", "x")), Err(LuliError::InvalidId)));
        assert!(matches!(save(conv("c-2-b", "   ")), Err(LuliError::InvalidText)));
        assert!(matches!(get("C:\\x"), Err(LuliError::InvalidId)));
        // Bounded transcript.
        let mut big = conv("c-3-c", "longa");
        big.messages = (0..250).map(|i| serde_json::json!({ "i": i })).collect();
        save(big).unwrap();
        assert_eq!(get("c-3-c").unwrap().messages.len(), MAX_MESSAGES);
        delete("c-3-c").unwrap();
        assert!(get("c-3-c").is_err());
        // Memory: deduplicated, deletable.
        let m = remember("Joga em 1440p").unwrap();
        assert_eq!(remember("joga em 1440p").unwrap().len(), m.len());
        let id = m.last().unwrap().id.clone();
        assert!(forget(&id).unwrap().iter().all(|i| i.id != id));
        clear().unwrap();
        assert!(history().conversations.is_empty());
    }
}
