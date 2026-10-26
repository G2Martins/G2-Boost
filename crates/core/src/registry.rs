//! Registry access (HKCU/HKLM) and the per-tweak backup store.
//! No `reg.exe`: `RegOpenKeyExW` / `RegQueryValueExW` / `RegSetValueExW` only.
//! A backup records the exact original values a tweak touched, so `revert`
//! restores what was there — never a guessed "Windows default".

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, KEY_WRITE, REG_BINARY, REG_DWORD,
    REG_EXPAND_SZ, REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW,
    RegDeleteTreeW, RegDeleteValueW, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::{PCWSTR, PWSTR};

const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_NO_MORE_ITEMS: u32 = 259;

/// Null-terminated UTF-16 for Win32 calls.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Decode a little-endian UTF-16 byte buffer up to its first NUL.
pub(crate) fn utf16z(bytes: &[u8]) -> String {
    let (chunks, _) = bytes.as_chunks::<2>();
    let units: Vec<u16> = chunks.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Root {
    Hkcu,
    Hklm,
}

impl Root {
    fn hkey(self) -> HKEY {
        match self {
            Root::Hkcu => HKEY_CURRENT_USER,
            Root::Hklm => HKEY_LOCAL_MACHINE,
        }
    }
}

/// The value kinds tweaks use. `Absent` = value does not exist (deleting restores that).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum Value {
    Dword(u32),
    Str(String),
    Binary(Vec<u8>),
    Absent,
}

#[derive(Debug, thiserror::Error)]
pub enum RegError {
    #[error("access_denied")]
    AccessDenied,
    #[error("registry error {0}")]
    Win32(u32),
    #[error("unsupported registry value type {0}")]
    Unsupported(u32),
}

fn check(e: WIN32_ERROR) -> Result<(), RegError> {
    match e {
        ERROR_SUCCESS => Ok(()),
        WIN32_ERROR(ERROR_ACCESS_DENIED) => Err(RegError::AccessDenied),
        WIN32_ERROR(code) => Err(RegError::Win32(code)),
    }
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: handle came from RegOpenKeyExW/RegCreateKeyExW and is closed once.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// `Ok(None)` when the key does not exist.
fn open(root: Root, path: &str, sam: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Result<Option<Key>, RegError> {
    let p = wide(path);
    let mut h = HKEY::default();
    // SAFETY: valid null-terminated path; out param is a plain handle.
    let e = unsafe { RegOpenKeyExW(root.hkey(), PCWSTR(p.as_ptr()), None, sam, &mut h) };
    if e == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    check(e)?;
    Ok(Some(Key(h)))
}

fn create(root: Root, path: &str) -> Result<Key, RegError> {
    let p = wide(path);
    let mut h = HKEY::default();
    // SAFETY: as above; class is null, no security attributes (inherits).
    let e = unsafe {
        RegCreateKeyExW(
            root.hkey(),
            PCWSTR(p.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_READ,
            None,
            &mut h,
            None,
        )
    };
    check(e)?;
    Ok(Key(h))
}

/// Read one value. Missing key or value → `Value::Absent`.
pub fn read(root: Root, path: &str, name: &str) -> Result<Value, RegError> {
    let Some(key) = open(root, path, KEY_READ)? else {
        return Ok(Value::Absent);
    };
    let n = wide(name);
    let mut ty = REG_VALUE_TYPE::default();
    let mut len = 0u32;
    // SAFETY: querying the size only; all pointers valid for the call.
    let e = unsafe { RegQueryValueExW(key.0, PCWSTR(n.as_ptr()), None, Some(&mut ty), None, Some(&mut len)) };
    if e == ERROR_FILE_NOT_FOUND {
        return Ok(Value::Absent);
    }
    check(e)?;
    let mut buf = vec![0u8; len as usize + 2];
    // SAFETY: buffer is at least `len` bytes; `len` is updated by the call.
    let e = unsafe {
        RegQueryValueExW(key.0, PCWSTR(n.as_ptr()), None, Some(&mut ty), Some(buf.as_mut_ptr()), Some(&mut len))
    };
    check(e)?;
    let len = len as usize;
    match ty {
        REG_DWORD if len >= 4 => Ok(Value::Dword(u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]))),
        REG_SZ | REG_EXPAND_SZ => Ok(Value::Str(utf16z(&buf[..len]))),
        REG_BINARY => Ok(Value::Binary(buf[..len].to_vec())),
        other => Err(RegError::Unsupported(other.0)),
    }
}

/// Write one value; `Value::Absent` deletes it. Creates the key when needed.
pub fn write(root: Root, path: &str, name: &str, value: &Value) -> Result<(), RegError> {
    let n = wide(name);
    match value {
        Value::Absent => {
            let Some(key) = open(root, path, KEY_SET_VALUE)? else {
                return Ok(());
            };
            // SAFETY: valid handle and name.
            let e = unsafe { RegDeleteValueW(key.0, PCWSTR(n.as_ptr())) };
            if e == ERROR_FILE_NOT_FOUND {
                return Ok(());
            }
            check(e)
        }
        Value::Dword(d) => {
            let key = create(root, path)?;
            // SAFETY: 4-byte little-endian payload for REG_DWORD.
            let e = unsafe { RegSetValueExW(key.0, PCWSTR(n.as_ptr()), None, REG_DWORD, Some(&d.to_le_bytes())) };
            check(e)
        }
        Value::Str(s) => {
            let key = create(root, path)?;
            let w = wide(s);
            let bytes: Vec<u8> = w.iter().flat_map(|u| u.to_le_bytes()).collect();
            // SAFETY: null-terminated UTF-16 payload for REG_SZ.
            let e = unsafe { RegSetValueExW(key.0, PCWSTR(n.as_ptr()), None, REG_SZ, Some(&bytes)) };
            check(e)
        }
        Value::Binary(b) => {
            let key = create(root, path)?;
            // SAFETY: raw bytes for REG_BINARY.
            let e = unsafe { RegSetValueExW(key.0, PCWSTR(n.as_ptr()), None, REG_BINARY, Some(b)) };
            check(e)
        }
    }
}

/// Direct subkey names (e.g. TCP/IP interface GUIDs). Missing key → empty.
pub fn subkeys(root: Root, path: &str) -> Result<Vec<String>, RegError> {
    let Some(key) = open(root, path, KEY_READ)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    let mut buf = [0u16; 256];
    for i in 0.. {
        let mut len = buf.len() as u32;
        // SAFETY: buffer of `len` UTF-16 units; the call writes at most that.
        let e = unsafe { RegEnumKeyExW(key.0, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) };
        if e == WIN32_ERROR(ERROR_NO_MORE_ITEMS) {
            break;
        }
        check(e)?;
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    Ok(out)
}

/// Every value name under a key (Run keys, StartupApproved…). Missing key → empty.
/// Values are read through [`read`] so the type handling stays in one place.
pub fn value_names(root: Root, path: &str) -> Result<Vec<String>, RegError> {
    let Some(key) = open(root, path, KEY_READ)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    let mut buf = vec![0u16; 16_384]; // max value name length is 16 383 chars
    for i in 0.. {
        let mut len = buf.len() as u32;
        // SAFETY: name buffer of `len` UTF-16 units; data not requested.
        let e = unsafe { RegEnumValueW(key.0, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) };
        if e == WIN32_ERROR(ERROR_NO_MORE_ITEMS) {
            break;
        }
        check(e)?;
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    Ok(out)
}

/* ---------------- whole trees (uninstall leftovers) ---------------- */

/// One value with its raw type and bytes (any type round-trips, not only the tweak types).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeValue {
    pub name: String,
    pub kind: u32,
    pub data: Vec<u8>,
}

/// A key's values and subkeys, recursively — the backup written before a tree is deleted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tree {
    pub values: Vec<TreeValue>,
    pub keys: Vec<(String, Tree)>,
}

pub fn export_tree(root: Root, path: &str) -> Result<Tree, RegError> {
    export_depth(root, path, 0)
}

fn export_depth(root: Root, path: &str, depth: u32) -> Result<Tree, RegError> {
    if depth > 32 {
        return Err(RegError::Win32(1010)); // ERROR_BADKEY: absurdly deep, refuse to back up
    }
    let Some(key) = open(root, path, KEY_READ)? else {
        return Ok(Tree::default());
    };
    let mut values = Vec::new();
    let mut name = vec![0u16; 16_384];
    let mut data = vec![0u8; 4096];
    let mut i = 0u32;
    loop {
        let mut nlen = name.len() as u32;
        let mut dlen = data.len() as u32;
        let mut ty = 0u32;
        // SAFETY: name/data buffers of the sizes passed; the call updates both lengths.
        let e = unsafe {
            RegEnumValueW(key.0, i, Some(PWSTR(name.as_mut_ptr())), &mut nlen, None, Some(&mut ty), Some(data.as_mut_ptr()), Some(&mut dlen))
        };
        if e == WIN32_ERROR(ERROR_NO_MORE_ITEMS) {
            break;
        }
        if e == WIN32_ERROR(234) {
            data.resize(dlen as usize + 2, 0); // ERROR_MORE_DATA: retry the same index
            continue;
        }
        check(e)?;
        values.push(TreeValue { name: String::from_utf16_lossy(&name[..nlen as usize]), kind: ty, data: data[..dlen as usize].to_vec() });
        i += 1;
    }
    let mut keys = Vec::new();
    for k in subkeys(root, path)? {
        let t = export_depth(root, &format!(r"{path}\{k}"), depth + 1)?;
        keys.push((k, t));
    }
    Ok(Tree { values, keys })
}

/// Delete `path` with everything below it. Already gone = Ok.
pub fn delete_tree(root: Root, path: &str) -> Result<(), RegError> {
    let (parent, leaf) = path.rsplit_once('\\').ok_or(RegError::Win32(87))?;
    let Some(key) = open(root, parent, REG_SAM_FLAGS(KEY_READ.0 | KEY_WRITE.0 | 0x0001_0000))? else {
        return Ok(());
    };
    let w = wide(leaf);
    // SAFETY: parent opened with DELETE + read/write; leaf is a NUL-terminated name.
    let e = unsafe { RegDeleteTreeW(key.0, PCWSTR(w.as_ptr())) };
    if e == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    check(e)
}

/// Write a backed-up tree back (undo of a leftover removal): keys are created, values written
/// with their original type and bytes. The caller validates `path` first.
pub fn import_tree(root: Root, path: &str, tree: &Tree) -> Result<(), RegError> {
    let key = create(root, path)?;
    for v in &tree.values {
        let n = wide(&v.name);
        // SAFETY: raw payload exactly as exported, with its original value type.
        check(unsafe { RegSetValueExW(key.0, PCWSTR(n.as_ptr()), None, REG_VALUE_TYPE(v.kind), Some(&v.data)) })?;
    }
    for (name, sub) in &tree.keys {
        if name.contains('\\') || name.is_empty() {
            return Err(RegError::Win32(87)); // a subkey name is one path segment
        }
        import_tree(root, &format!(r"{path}\{name}"), sub)?;
    }
    Ok(())
}

/// Block until `stop`, calling `on_change` whenever one of `keys` gains or loses a subkey
/// (an app installed or removed under an Uninstall key). `RegNotifyChangeKeyValue`, re-armed
/// after every signal; bursts (installers touch the key several times) are debounced.
pub fn watch_subkeys(keys: &[(Root, &str)], stop: &std::sync::atomic::AtomicBool, mut on_change: impl FnMut()) {
    use std::sync::atomic::Ordering;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Registry::{KEY_NOTIFY, REG_NOTIFY_CHANGE_NAME, RegNotifyChangeKeyValue};
    use windows::Win32::System::Threading::{CreateEventW, WaitForMultipleObjects};

    let mut watched: Vec<(Key, HANDLE)> = Vec::new();
    for (root, path) in keys {
        let Ok(Some(key)) = open(*root, path, KEY_NOTIFY) else {
            continue;
        };
        // SAFETY: auto-reset event; armed on the open key handle (both live in `watched`).
        let Ok(ev) = (unsafe { CreateEventW(None, false, false, PCWSTR::null()) }) else {
            continue;
        };
        if unsafe { RegNotifyChangeKeyValue(key.0, false, REG_NOTIFY_CHANGE_NAME, Some(ev), true) } == ERROR_SUCCESS {
            watched.push((key, ev));
        } else {
            let _ = unsafe { CloseHandle(ev) };
        }
    }
    let events: Vec<HANDLE> = watched.iter().map(|(_, e)| *e).collect();
    while !events.is_empty() && !stop.load(Ordering::Relaxed) {
        // SAFETY: valid event handles; 1 s timeout so `stop` is honoured.
        let r = unsafe { WaitForMultipleObjects(&events, false, 1000) };
        let i = r.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
        if i >= events.len() {
            continue; // timeout
        }
        let (key, ev) = &watched[i];
        // SAFETY: re-arm the same key/event pair.
        let _ = unsafe { RegNotifyChangeKeyValue(key.0, false, REG_NOTIFY_CHANGE_NAME, Some(*ev), true) };
        std::thread::sleep(std::time::Duration::from_millis(1500));
        on_change();
    }
    for (_, ev) in watched {
        // SAFETY: events created above, closed once; keys close on drop.
        let _ = unsafe { CloseHandle(ev) };
    }
}

/* ---------------- backups ---------------- */

/// One original value captured before a tweak changed it. `path` is a registry
/// path, or a pseudo-path handled by the boost module (`spi:mouse`, `svc:SysMain`, `power:active`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupEntry {
    pub root: Root,
    pub path: String,
    pub name: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Backup {
    pub tweak: String,
    pub taken_at: u64,
    pub entries: Vec<BackupEntry>,
}

/// Tweak ids come from the UI (hostile): only `[a-z0-9_]` reaches the filesystem.
pub fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// A shell known folder (Desktop/Documents may be redirected to OneDrive).
pub(crate) fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{KNOWN_FOLDER_FLAG, SHGetKnownFolderPath};
    // SAFETY: the API allocates the string; it is copied, then freed with CoTaskMemFree.
    unsafe {
        let p = SHGetKnownFolderPath(id, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const std::ffi::c_void));
        s.map(PathBuf::from)
    }
}

/// The user's Documents folder (OneDrive-redirected or not), else `%USERPROFILE%\Documents`.
pub(crate) fn documents_dir() -> Option<PathBuf> {
    known_folder(&windows::Win32::UI::Shell::FOLDERID_Documents)
        .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Documents")))
}

/// `Documents\G2 Boost`, created when missing: where every user-visible export goes.
pub fn documents_g2() -> std::io::Result<PathBuf> {
    let dir = documents_dir()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no Documents folder"))?
        .join("G2 Boost");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// `%LOCALAPPDATA%\G2 Boost`, or `<exe folder>\data` for the portable zip (a `portable.txt`
/// next to the exe). `G2BOOST_DATA_DIR` overrides both (tests).
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("G2BOOST_DATA_DIR") {
        return PathBuf::from(d);
    }
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from));
        resolve_data_dir(exe_dir.as_deref(), std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
    })
    .clone()
}

fn resolve_data_dir(exe_dir: Option<&std::path::Path>, local: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = exe_dir.filter(|d| d.join("portable.txt").is_file()) {
        return dir.join("data");
    }
    local.unwrap_or_else(std::env::temp_dir).join("G2 Boost")
}

fn backup_file(tweak: &str) -> PathBuf {
    data_dir().join("backups").join(format!("{tweak}.json"))
}

pub fn load_backup(tweak: &str) -> Option<Backup> {
    if !safe_id(tweak) {
        return None;
    }
    let text = std::fs::read_to_string(backup_file(tweak)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn save_backup(b: &Backup) -> std::io::Result<()> {
    if !safe_id(&b.tweak) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad tweak id"));
    }
    let file = backup_file(&b.tweak);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(file, serde_json::to_string_pretty(b).expect("serializable"))
}

pub fn delete_backup(tweak: &str) {
    if safe_id(tweak) {
        let _ = std::fs::remove_file(backup_file(tweak));
    }
}

/// FILETIME (100 ns ticks since 1601) to Unix seconds; before 1970 or after year 3000 = 0 (garbage).
pub fn filetime_to_unix(ft: u64) -> u64 {
    const EPOCH_DIFF_SECS: u64 = 11_644_473_600;
    const MAX_UNIX: u64 = 32_503_680_000;
    (ft / 10_000_000).checked_sub(EPOCH_DIFF_SECS).filter(|&u| u <= MAX_UNIX).unwrap_or(0)
}

/// Unix seconds to FILETIME (100 ns ticks since 1601).
pub fn unix_to_filetime(secs: u64) -> u64 {
    (secs + 11_644_473_600) * 10_000_000
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const TEST_KEY: &str = r"Software\G2 Boost\Test\registry";

    #[test]
    fn roundtrip_dword_and_string_then_delete() {
        write(Root::Hkcu, TEST_KEY, "d", &Value::Dword(42)).unwrap();
        write(Root::Hkcu, TEST_KEY, "s", &Value::Str("olá".into())).unwrap();
        assert_eq!(read(Root::Hkcu, TEST_KEY, "d").unwrap(), Value::Dword(42));
        assert_eq!(read(Root::Hkcu, TEST_KEY, "s").unwrap(), Value::Str("olá".into()));
        write(Root::Hkcu, TEST_KEY, "d", &Value::Absent).unwrap();
        assert_eq!(read(Root::Hkcu, TEST_KEY, "d").unwrap(), Value::Absent);
        // deleting again (or a missing key) is not an error
        write(Root::Hkcu, TEST_KEY, "d", &Value::Absent).unwrap();
        write(Root::Hkcu, r"Software\G2 Boost\Test\missing", "x", &Value::Absent).unwrap();
        assert_eq!(read(Root::Hkcu, r"Software\G2 Boost\Test\missing", "x").unwrap(), Value::Absent);
        assert!(subkeys(Root::Hkcu, r"Software\G2 Boost\Test").unwrap().iter().any(|k| k == "registry"));
    }

    /// Whole-tree backup then delete (uninstall leftovers), on a throwaway HKCU key.
    #[test]
    fn tree_export_then_delete() {
        let base = r"Software\G2 Boost\Test\tree";
        write(Root::Hkcu, &format!(r"{base}\sub"), "s", &Value::Str("x".into())).unwrap();
        write(Root::Hkcu, base, "d", &Value::Dword(7)).unwrap();
        let t = export_tree(Root::Hkcu, base).unwrap();
        assert_eq!(t.values.len(), 1);
        assert_eq!((t.values[0].name.as_str(), t.values[0].kind, t.values[0].data.as_slice()), ("d", 4, &7u32.to_le_bytes()[..]));
        assert_eq!(t.keys.len(), 1);
        assert_eq!(t.keys[0].0, "sub");
        delete_tree(Root::Hkcu, base).unwrap();
        assert_eq!(read(Root::Hkcu, base, "d").unwrap(), Value::Absent);
        assert!(subkeys(Root::Hkcu, base).unwrap().is_empty());
        delete_tree(Root::Hkcu, base).unwrap(); // already gone = Ok
        assert!(delete_tree(Root::Hkcu, "NoBackslash").is_err());
        // Undo: the backup goes back exactly (values with their types, subkeys).
        import_tree(Root::Hkcu, base, &t).unwrap();
        assert_eq!(export_tree(Root::Hkcu, base).unwrap(), t);
        delete_tree(Root::Hkcu, base).unwrap();
        let evil = Tree { values: vec![], keys: vec![(r"..\Run".into(), Tree::default())] };
        assert!(import_tree(Root::Hkcu, base, &evil).is_err(), "subkey names are single segments");
        delete_tree(Root::Hkcu, base).unwrap();
    }

    #[test]
    fn hklm_is_readable_without_admin() {
        let v = read(Root::Hklm, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "CurrentBuild").unwrap();
        assert!(matches!(v, Value::Str(_)));
    }

    /// One stable scratch dir for every test that touches the data dir (the env override is
    /// process-wide, so all tests must agree on the same value).
    pub(crate) fn test_data_dir() -> PathBuf {
        std::env::temp_dir().join("g2boost-core-tests")
    }

    #[test]
    fn portable_marker_keeps_data_next_to_the_exe() {
        let exe_dir = std::env::temp_dir().join(format!("g2boost-portable-{}", std::process::id()));
        std::fs::create_dir_all(&exe_dir).unwrap();
        let local = PathBuf::from(r"C:\Users\x\AppData\Local");
        assert_eq!(resolve_data_dir(Some(&exe_dir), Some(local.clone())), local.join("G2 Boost"));
        std::fs::write(exe_dir.join("portable.txt"), "x").unwrap();
        assert_eq!(resolve_data_dir(Some(&exe_dir), Some(local)), exe_dir.join("data"));
        std::fs::remove_dir_all(&exe_dir).unwrap();
    }

    #[test]
    fn backup_store_roundtrip_and_hostile_ids() {
        // SAFETY: same value every data-dir test sets; see `test_data_dir`.
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", test_data_dir()) };
        let b = Backup {
            tweak: "unit_test".into(),
            taken_at: now_unix(),
            entries: vec![BackupEntry {
                root: Root::Hkcu,
                path: TEST_KEY.into(),
                name: "d".into(),
                value: Value::Absent,
            }],
        };
        save_backup(&b).unwrap();
        assert_eq!(load_backup("unit_test").unwrap().entries, b.entries);
        delete_backup("unit_test");
        assert!(load_backup("unit_test").is_none());
        assert!(!safe_id("../evil"));
        assert!(!safe_id("UPPER"));
        assert!(load_backup("../../etc").is_none());
    }
}
