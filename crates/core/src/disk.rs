//! Parallel directory scan (rayon) producing an in-memory tree for the treemap.
//! Read-only. Reparse points (junctions, symlinks) are never followed — the classic way to
//! count the same bytes twice or wander into `C:\Users\All Users` loops.
//!
//! ponytail: one `Node` per file keeps ~150 MB for 1.5 M entries; swap for a compact arena
//! (names interned, children as ranges) if the C: scan ever hurts.

use std::collections::BinaryHeap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Instant, UNIX_EPOCH};

use rayon::prelude::*;
use serde::Serialize;

const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    /// Files below this node (recursive); 1 for a file.
    pub files: u64,
    /// Modified time, seconds since epoch (0 if unknown).
    pub mtime: u64,
    /// Sorted by size, descending, after the scan.
    pub children: Vec<Node>,
}

/// Live counters read by the progress reporter while the scan runs.
#[derive(Default)]
pub struct ScanProgress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub errors: AtomicU64,
    pub current: Mutex<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanSummary {
    pub root: String,
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub errors: u64,
    pub elapsed_ms: u64,
    pub cancelled: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("root must be an absolute, existing directory")]
    BadRoot,
    #[error("UNC / network paths are not scanned")]
    Unc,
}

/// Scan `root`. `cancel` makes the walk stop early (the partial tree is still returned).
pub fn scan(root: &Path, progress: &ScanProgress, cancel: &AtomicBool) -> Result<(Node, ScanSummary), ScanError> {
    if !root.is_absolute() || !root.is_dir() {
        return Err(ScanError::BadRoot);
    }
    if root.to_string_lossy().starts_with("\\\\") {
        return Err(ScanError::Unc);
    }
    let t0 = Instant::now();
    let name = root.to_string_lossy().trim_end_matches('\\').to_string();
    let mut node = walk(root, name, progress, cancel);
    sort_tree(&mut node);
    let summary = ScanSummary {
        root: root.to_string_lossy().into_owned(),
        files: progress.files.load(Ordering::Relaxed),
        dirs: progress.dirs.load(Ordering::Relaxed),
        bytes: node.size,
        errors: progress.errors.load(Ordering::Relaxed),
        elapsed_ms: t0.elapsed().as_millis() as u64,
        cancelled: cancel.load(Ordering::Relaxed),
    };
    Ok((node, summary))
}

fn walk(dir: &Path, name: String, progress: &ScanProgress, cancel: &AtomicBool) -> Node {
    let mut node = Node { name, size: 0, is_dir: true, files: 0, mtime: 0, children: Vec::new() };
    if cancel.load(Ordering::Relaxed) {
        return node;
    }
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => {
            progress.errors.fetch_add(1, Ordering::Relaxed);
            return node;
        }
    };
    progress.dirs.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut cur) = progress.current.lock() {
        *cur = dir.to_string_lossy().into_owned();
    }
    let mut subdirs: Vec<(PathBuf, String)> = Vec::new();
    for entry in rd.flatten() {
        // On Windows `DirEntry::metadata` is filled from the directory listing: no extra syscall.
        let Ok(md) = entry.metadata() else {
            progress.errors.fetch_add(1, Ordering::Relaxed);
            continue;
        };
        #[cfg(windows)]
        let reparse = {
            use std::os::windows::fs::MetadataExt;
            md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        };
        #[cfg(not(windows))]
        let reparse = md.file_type().is_symlink();
        if reparse {
            continue;
        }
        let fname = entry.file_name().to_string_lossy().into_owned();
        if md.is_dir() {
            subdirs.push((entry.path(), fname));
        } else {
            let size = md.len();
            let mtime = md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            progress.files.fetch_add(1, Ordering::Relaxed);
            progress.bytes.fetch_add(size, Ordering::Relaxed);
            node.children.push(Node { name: fname, size, is_dir: false, files: 1, mtime, children: Vec::new() });
        }
    }
    let dir_nodes: Vec<Node> = subdirs
        .into_par_iter()
        .map(|(p, n)| walk(&p, n, progress, cancel))
        .collect();
    node.children.extend(dir_nodes);
    for c in &node.children {
        node.size += c.size;
        node.files += c.files;
        node.mtime = node.mtime.max(c.mtime);
    }
    node
}

fn sort_tree(node: &mut Node) {
    node.children.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
    node.children.par_iter_mut().for_each(sort_tree);
}

/// Depth-limited, child-limited view for the UI. `others` aggregates the children cut by `max_children`.
#[derive(Debug, Clone, Serialize)]
pub struct NodeView {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    pub files: u64,
    pub mtime: u64,
    /// Index path from the scan root (empty = root). Stable for the life of one scan.
    pub ids: Vec<u32>,
    pub children: Vec<NodeView>,
    pub others_size: u64,
    pub others_count: u32,
}

pub fn find<'a>(root: &'a Node, ids: &[u32]) -> Option<&'a Node> {
    let mut n = root;
    for &i in ids {
        n = n.children.get(i as usize)?;
    }
    Some(n)
}

pub fn view(root: &Node, ids: &[u32], depth: u8, max_children: usize) -> Option<NodeView> {
    let node = find(root, ids)?;
    Some(view_of(node, ids.to_vec(), depth, max_children.max(1)))
}

fn view_of(node: &Node, ids: Vec<u32>, depth: u8, max_children: usize) -> NodeView {
    let mut v = NodeView {
        name: node.name.clone(),
        size: node.size,
        is_dir: node.is_dir,
        files: node.files,
        mtime: node.mtime,
        ids: ids.clone(),
        children: Vec::new(),
        others_size: 0,
        others_count: 0,
    };
    if depth == 0 {
        return v;
    }
    for (i, c) in node.children.iter().enumerate() {
        if i < max_children && c.size > 0 {
            let mut cid = ids.clone();
            cid.push(i as u32);
            v.children.push(view_of(c, cid, depth - 1, max_children));
        } else {
            v.others_size += c.size;
            v.others_count += 1;
        }
    }
    v
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct BigFile {
    pub size: u64,
    pub path: String,
    pub mtime: u64,
    /// Index path in the scan tree (for reveal / recycle).
    pub ids: Vec<u32>,
}

/// Filter typed in the UI. Tokens are AND-ed, extensions OR-ed:
/// `*.mp4 .mkv` extension · `>1gb` / `<200mb` size · `>6m` / `<7d` age (d, w, m, y) · `word` name contains.
#[derive(Debug, Default, PartialEq)]
pub struct Filter {
    pub exts: Vec<String>,
    pub min_bytes: u64,
    pub max_bytes: Option<u64>,
    pub older_than_s: Option<u64>,
    pub newer_than_s: Option<u64>,
    pub contains: Vec<String>,
}

fn size_unit(u: &str) -> Option<u64> {
    Some(match u {
        "b" => 1,
        "k" | "kb" => 1 << 10,
        "m" | "mb" => 1 << 20,
        "g" | "gb" => 1 << 30,
        "t" | "tb" => 1 << 40,
        _ => return None,
    })
}

fn age_unit(u: &str) -> Option<u64> {
    Some(match u {
        "d" => 86_400,
        "w" => 7 * 86_400,
        "mo" | "m" => 30 * 86_400,
        "y" => 365 * 86_400,
        _ => return None,
    })
}

pub fn parse_filter(q: &str) -> Filter {
    let mut f = Filter::default();
    for tok in q.split_whitespace().map(str::to_ascii_lowercase) {
        if let Some(ext) = tok.strip_prefix("*.").or_else(|| tok.strip_prefix('.')) {
            if !ext.is_empty() {
                f.exts.push(ext.to_string());
            }
            continue;
        }
        let (op, rest) = match tok.as_bytes().first() {
            Some(b'>') => ('>', &tok[1..]),
            Some(b'<') => ('<', &tok[1..]),
            _ => {
                f.contains.push(tok.clone());
                continue;
            }
        };
        let split = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
        let (num, unit) = rest.split_at(split);
        let Ok(n) = num.parse::<f64>() else { continue };
        // "m" alone means months (sizes use "mb"), so `>6m` is age and `>6mb` is size.
        if unit != "m"
            && let Some(mul) = size_unit(unit)
        {
            let v = (n * mul as f64) as u64;
            if op == '>' { f.min_bytes = v } else { f.max_bytes = Some(v) }
        } else if let Some(mul) = age_unit(unit) {
            let v = (n * mul as f64) as u64;
            if op == '>' { f.older_than_s = Some(v) } else { f.newer_than_s = Some(v) }
        }
    }
    f
}

impl Filter {
    pub fn matches(&self, name: &str, size: u64, mtime: u64, now: u64) -> bool {
        let age = now.saturating_sub(mtime);
        let numeric = size >= self.min_bytes
            && self.max_bytes.is_none_or(|m| size <= m)
            && self.older_than_s.is_none_or(|s| mtime > 0 && age >= s)
            && self.newer_than_s.is_none_or(|s| mtime > 0 && age <= s);
        if !numeric {
            return false;
        }
        if self.exts.is_empty() && self.contains.is_empty() {
            return true; // no name terms: skip the lowercase copy (the walk visits every file)
        }
        let lower = name.to_ascii_lowercase();
        (self.exts.is_empty() || lower.rsplit_once('.').is_some_and(|(_, e)| self.exts.iter().any(|x| x == e)))
            && self.contains.iter().all(|c| lower.contains(c.as_str()))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    /// Largest matches first (at most `n`).
    pub files: Vec<BigFile>,
    pub matched: u64,
    pub matched_bytes: u64,
}

/// Files under `root` matching `f`: totals for all of them, the largest `n` listed (min-heap, one pass).
pub fn search(root: &Node, f: &Filter, now: u64, n: usize) -> SearchResult {
    struct Acc<'a> {
        root: &'a Node,
        f: &'a Filter,
        now: u64,
        n: usize,
        heap: BinaryHeap<std::cmp::Reverse<BigFile>>,
        matched: u64,
        bytes: u64,
    }
    fn rec(node: &Node, ids: &mut Vec<u32>, acc: &mut Acc) {
        for (i, c) in node.children.iter().enumerate() {
            ids.push(i as u32);
            if c.is_dir {
                rec(c, ids, acc);
            } else if acc.f.matches(&c.name, c.size, c.mtime, acc.now) {
                acc.matched += 1;
                acc.bytes += c.size;
                if acc.heap.len() < acc.n || acc.heap.peek().is_some_and(|r| r.0.size < c.size) {
                    // Path built from the ids only for files that enter the top-n, not for every dir walked.
                    let path = path_of(acc.root, ids).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                    acc.heap.push(std::cmp::Reverse(BigFile { size: c.size, path, mtime: c.mtime, ids: ids.clone() }));
                    if acc.heap.len() > acc.n {
                        acc.heap.pop();
                    }
                }
            }
            ids.pop();
        }
    }
    let mut acc = Acc { root, f, now, n, heap: BinaryHeap::with_capacity(n + 1), matched: 0, bytes: 0 };
    rec(root, &mut Vec::new(), &mut acc);
    let mut files: Vec<BigFile> = acc.heap.into_iter().map(|r| r.0).collect();
    files.sort_by_key(|a| std::cmp::Reverse(a.size));
    SearchResult { files, matched: acc.matched, matched_bytes: acc.bytes }
}

/// Absolute path of the node at `ids` (the root node's name is the scanned path).
pub fn path_of(root: &Node, ids: &[u32]) -> Option<PathBuf> {
    let mut s = root.name.clone();
    let mut n = root;
    for &i in ids {
        n = n.children.get(i as usize)?;
        s.push('\\');
        s.push_str(&n.name);
    }
    Some(PathBuf::from(if ids.is_empty() && s.ends_with(':') { format!("{s}\\") } else { s }))
}

/// Big system items people find in SpaceSniffer and wonder about, as paths relative to the
/// scanned drive root. The UI explains each one and points to the safe way to shrink it.
pub const ESSENTIALS: &[(&str, &str)] = &[
    ("pagefile", "pagefile.sys"),
    ("hiberfil", "hiberfil.sys"),
    ("swapfile", "swapfile.sys"),
    ("winsxs", r"Windows\WinSxS"),
    ("windows_old", "Windows.old"),
    ("recycle_bin", "$Recycle.Bin"),
    ("restore_points", "System Volume Information"),
    ("wu_download", r"Windows\SoftwareDistribution\Download"),
    ("installer", r"Windows\Installer"),
    ("package_cache", r"ProgramData\Package Cache"),
];

#[derive(Debug, Clone, Serialize)]
pub struct Essential {
    pub id: &'static str,
    pub path: String,
    pub size: u64,
}

/// Which [`ESSENTIALS`] exist under `root` and how big they are (0 = present but unreadable).
pub fn essentials(root: &Node) -> Vec<Essential> {
    ESSENTIALS
        .iter()
        .filter_map(|(id, rel)| {
            let mut n = root;
            for part in rel.split('\\') {
                n = n.children.iter().find(|c| c.name.eq_ignore_ascii_case(part))?;
            }
            Some(Essential { id, path: format!("{}\\{rel}", root.name), size: n.size })
        })
        .collect()
}

/* ---------------- reveal + recycle ---------------- */

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RecycleError {
    #[error("not_found")]
    NotFound,
    /// System folders, other users' profiles, drive/profile roots, junctions.
    #[error("protected_path")]
    Protected,
    #[error("shell error {0}")]
    Shell(i32),
}

/// May the user send `p` to the Recycle Bin from the treemap? Pure, so it is tested.
pub(crate) fn recyclable(p: &Path, windows_dir: &Path, profile: &Path) -> bool {
    let Ok(safe) = crate::safety::SafePath::classify(p, windows_dir, profile) else {
        return false;
    };
    if !matches!(safe.class(), crate::safety::PathClass::UserProfile | crate::safety::PathClass::Other) {
        return false;
    }
    let l = p.to_string_lossy().trim_end_matches('\\').to_ascii_lowercase();
    let prof = profile.to_string_lossy().trim_end_matches('\\').to_ascii_lowercase();
    // Roots whose loss breaks the machine or the account; what lives below them is fine.
    let roots = [
        prof.clone(),
        format!(r"{prof}\appdata"),
        format!(r"{prof}\appdata\local"),
        format!(r"{prof}\appdata\roaming"),
        format!(r"{prof}\appdata\locallow"),
    ];
    let drive_root = l.len() <= 2;
    let program_files = l.len() > 3 && matches!(&l[3..], "program files" | "program files (x86)" | "users" | "programdata");
    !(drive_root || program_files || roots.contains(&l))
}

/// `canonicalize` returns an extended-length prefix; the guard and the shell want the plain path.
pub(crate) fn strip_verbatim(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        p.to_path_buf()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RecycleOutcome {
    pub recycled: u32,
    pub bytes: u64,
    pub failed: u32,
}

fn is_reparse(md: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

/// Detach the node at `ids` and subtract its size/count from every ancestor.
fn remove(n: &mut Node, ids: &[u32]) -> Option<Node> {
    let (&first, rest) = ids.split_first()?;
    let gone = if rest.is_empty() {
        if first as usize >= n.children.len() {
            return None;
        }
        n.children.remove(first as usize)
    } else {
        remove(n.children.get_mut(first as usize)?, rest)?
    };
    n.size = n.size.saturating_sub(gone.size);
    n.files = n.files.saturating_sub(gone.files);
    Some(gone)
}

/// Send the selected nodes to the Recycle Bin. The user picked them in the treemap and confirmed
/// in the app; Windows itself asks before anything too big for the Bin is deleted for good.
pub fn recycle(root: &mut Node, selection: &[Vec<u32>]) -> Result<RecycleOutcome, RecycleError> {
    // Drop entries nested inside another selected entry; deepest/last first keeps ids valid while removing.
    let mut sel: Vec<Vec<u32>> = selection.to_vec();
    sel.sort();
    sel.dedup();
    let sel: Vec<Vec<u32>> = sel.iter().filter(|s| !sel.iter().any(|o| o.len() < s.len() && s.starts_with(o))).cloned().collect();
    if sel.is_empty() {
        return Err(RecycleError::NotFound);
    }
    let win = crate::cleanup::env_dir("SystemRoot").unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let profile = crate::cleanup::env_dir("USERPROFILE").unwrap_or_else(|| PathBuf::from(r"C:\Users\Default"));
    let mut paths = Vec::with_capacity(sel.len());
    for ids in &sel {
        if ids.is_empty() {
            return Err(RecycleError::Protected);
        }
        let p = path_of(root, ids).ok_or(RecycleError::NotFound)?;
        let md = fs::symlink_metadata(&p).map_err(|_| RecycleError::NotFound)?;
        if is_reparse(&md) {
            return Err(RecycleError::Protected);
        }
        // Resolve the real target (any reparse point on the path, `/` separators, trailing dots)
        // before the lexical guard, and recycle that resolved path.
        let real = strip_verbatim(&fs::canonicalize(&p).map_err(|_| RecycleError::NotFound)?);
        if !recyclable(&real, &win, &profile) {
            return Err(RecycleError::Protected);
        }
        paths.push(real);
    }
    let t0 = Instant::now();
    let (rc, aborted) = shell_recycle(&paths);
    // Whatever is gone now was recycled (some items may be in use or the user cancelled).
    let mut out = RecycleOutcome { recycled: 0, bytes: 0, failed: 0 };
    let mut order: Vec<(usize, &Vec<u32>)> = sel.iter().enumerate().collect();
    order.sort_by(|a, b| b.1.cmp(a.1));
    for (i, ids) in order {
        if fs::symlink_metadata(&paths[i]).is_err() {
            if let Some(n) = remove(root, ids) {
                out.recycled += 1;
                out.bytes += n.size;
            }
        } else {
            out.failed += 1;
        }
    }
    if out.recycled > 0 {
        crate::cleanup::record_run(&crate::cleanup::RunRecord {
            at: crate::registry::now_unix(),
            trigger: "disk_recycle".into(),
            duration_ms: t0.elapsed().as_millis() as u64,
            items: vec![crate::cleanup::ExecItem {
                id: "disk_recycle".into(),
                bytes_freed: out.bytes,
                files_deleted: u64::from(out.recycled),
                skipped: u64::from(out.failed),
                error: None,
            }],
        });
    }
    if rc != 0 && out.recycled == 0 && !aborted {
        return Err(RecycleError::Shell(rc));
    }
    Ok(out)
}

/// One shell operation sending `paths` (validated by the caller) to the Recycle Bin. Windows asks
/// itself before deleting for good anything too big for the Bin. Returns (code, user aborted).
pub(crate) fn shell_recycle(paths: &[PathBuf]) -> (i32, bool) {
    use windows::Win32::UI::Shell::{FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_WANTNUKEWARNING, SHFILEOPSTRUCTW, SHFileOperationW};
    use windows::core::PCWSTR;
    if paths.is_empty() {
        return (0, false);
    }
    // Double-NUL-terminated list.
    let mut from: Vec<u16> = Vec::new();
    for p in paths {
        use std::os::windows::ffi::OsStrExt;
        from.extend(p.as_os_str().encode_wide());
        from.push(0);
    }
    from.push(0);
    let mut op = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        fFlags: (FOF_ALLOWUNDO.0 | FOF_NOCONFIRMATION.0 | FOF_WANTNUKEWARNING.0) as u16,
        ..Default::default()
    };
    // SAFETY: `from` outlives the call; the struct is fully initialised.
    let rc = unsafe { SHFileOperationW(&mut op) };
    (rc, op.fAnyOperationsAborted.as_bool())
}

/// Open Explorer with the item selected (shell API, no `explorer.exe` spawn).
pub fn reveal(p: &Path) -> Result<(), RecycleError> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{SHOpenFolderAndSelectItems, SHParseDisplayName};
    use windows::core::PCWSTR;

    fs::symlink_metadata(p).map_err(|_| RecycleError::NotFound)?;
    let _com = crate::tasksched::Com::init().map_err(|_| RecycleError::Shell(-1))?;
    let w = crate::registry::wide(&p.to_string_lossy());
    let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
    // SAFETY: NUL-terminated path; the PIDL is freed with CoTaskMemFree as documented.
    unsafe {
        SHParseDisplayName(PCWSTR(w.as_ptr()), None, &mut pidl, 0, None).map_err(|e| RecycleError::Shell(e.code().0))?;
        let r = SHOpenFolderAndSelectItems(pidl, None, 0);
        CoTaskMemFree(Some(pidl as *const std::ffi::c_void));
        r.map_err(|e| RecycleError::Shell(e.code().0))
    }
}

/* ---------------- usage report (MCP, console) ---------------- */

/// A folder measured with a deadline, ready to leave the app: the profile folder under `Users`
/// is `%USERNAME%` and paths have the profile, computer and account names redacted.
#[derive(Debug, Clone, Serialize)]
pub struct Usage {
    pub summary: ScanSummary,
    /// Two levels, 25 per level.
    pub tree: Option<NodeView>,
    /// Files over 100 MB, largest first (25).
    pub largest_files: Vec<BigFile>,
    pub essentials: Vec<Essential>,
}

/// Scan `root` (default: the system drive) for at most `max_secs` (clamped 5..=110).
pub fn usage(root: Option<&str>, max_secs: u64) -> Result<Usage, ScanError> {
    let default_root = std::env::var("SystemDrive").map(|d| format!("{d}\\")).unwrap_or_else(|_| "C:\\".into());
    let root = PathBuf::from(root.map(str::to_string).unwrap_or(default_root));
    let secs = max_secs.clamp(5, 110);
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let timer = {
        let cancel = cancel.clone();
        std::thread::spawn(move || {
            if done_rx.recv_timeout(std::time::Duration::from_secs(secs)).is_err() {
                cancel.store(true, Ordering::Relaxed);
            }
        })
    };
    let progress = ScanProgress::default();
    let result = scan(&root, &progress, &cancel);
    let _ = done_tx.send(());
    let _ = timer.join();
    let (node, summary) = result?;
    let mut tree = view(&node, &[], 2, 25);
    if let Some(t) = tree.as_mut() {
        redact_view(t, false, &std::env::var("USERNAME").unwrap_or_default());
    }
    let mut largest_files = search(&node, &parse_filter(">100mb"), crate::registry::now_unix(), 25).files;
    for f in &mut largest_files {
        f.path = crate::winevents::redact_here(&f.path);
    }
    let mut essentials = essentials(&node);
    for e in &mut essentials {
        e.path = crate::winevents::redact_here(&e.path);
    }
    Ok(Usage { summary, tree, largest_files, essentials })
}

/// The profile folder under `Users` carries the account name: hide it.
fn redact_view(v: &mut NodeView, parent_is_users: bool, user: &str) {
    if parent_is_users && !user.is_empty() && v.name.eq_ignore_ascii_case(user) {
        v.name = "%USERNAME%".into();
    }
    let is_users = v.name.eq_ignore_ascii_case("users");
    for c in &mut v.children {
        redact_view(c, is_users, user);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sandbox(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("g2boost-scan-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("a/deep")).unwrap();
        fs::create_dir_all(dir.join("b")).unwrap();
        let mk = |p: &str, n: usize| {
            let mut f = fs::File::create(dir.join(p)).unwrap();
            f.write_all(&vec![0u8; n]).unwrap();
        };
        mk("a/one.bin", 3000);
        mk("a/deep/two.bin", 5000);
        mk("b/three.bin", 1000);
        mk("root.bin", 100);
        dir
    }

    #[test]
    fn scan_counts_sizes_and_sorts() {
        let dir = sandbox("sorts");
        let p = ScanProgress::default();
        let (tree, sum) = scan(&dir, &p, &AtomicBool::new(false)).unwrap();
        assert_eq!(sum.files, 4);
        assert_eq!(sum.bytes, 9100);
        assert_eq!(tree.size, 9100);
        assert_eq!(tree.children[0].name, "a"); // 8000 first
        assert_eq!(tree.children[1].name, "b");
        assert_eq!(tree.children[2].name, "root.bin");
        let v = view(&tree, &[], 2, 2).unwrap();
        assert_eq!(v.children.len(), 2);
        assert_eq!(v.others_count, 1);
        assert_eq!(v.others_size, 100);
        assert_eq!(v.children[0].children[0].name, "deep");
        assert_eq!(v.children[0].children[0].ids, vec![0, 0]);
        let top = search(&tree, &Filter::default(), 0, 2).files;
        assert_eq!(top.len(), 2);
        assert!(top[0].path.ends_with("a\\deep\\two.bin"));
        assert_eq!(top[1].size, 3000);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn filter_syntax() {
        let f = parse_filter("*.MP4 .mkv >1gb <6m clip");
        assert_eq!(f.exts, vec!["mp4", "mkv"]);
        assert_eq!(f.min_bytes, 1 << 30);
        assert_eq!(f.newer_than_s, Some(6 * 30 * 86_400));
        assert_eq!(f.contains, vec!["clip"]);
        let now = 1_000_000_000;
        assert!(f.matches("My Clip.mp4", 2 << 30, now - 86_400, now));
        assert!(!f.matches("My Clip.mp4", 100, now - 86_400, now), "too small");
        assert!(!f.matches("My Clip.avi", 2 << 30, now - 86_400, now), "wrong extension");
        assert!(!f.matches("other.mp4", 2 << 30, now - 86_400, now), "name");
        let old = parse_filter(">1y <500mb");
        assert_eq!((old.older_than_s, old.max_bytes), (Some(365 * 86_400), Some(500 << 20)));
        assert!(old.matches("a.log", 10, now - 400 * 86_400, now));
        assert!(!old.matches("a.log", 10, 0, now), "unknown mtime never counts as old");
        assert_eq!(parse_filter(">6mb").min_bytes, 6 << 20);
        assert_eq!(parse_filter(">abc"), Filter::default());
    }

    #[test]
    fn search_paths_and_removal() {
        let dir = sandbox("search");
        let p = ScanProgress::default();
        let (mut tree, _) = scan(&dir, &p, &AtomicBool::new(false)).unwrap();
        let r = search(&tree, &parse_filter(".bin >2000b"), 0, 10);
        assert_eq!((r.matched, r.matched_bytes), (2, 8000));
        assert_eq!(r.files[0].ids, vec![0, 0, 0]); // a/deep/two.bin
        assert_eq!(path_of(&tree, &r.files[0].ids).unwrap(), dir.join(r"a\deep\two.bin"));
        let gone = remove(&mut tree, &[0, 0]).unwrap(); // a/deep
        assert_eq!(gone.size, 5000);
        assert_eq!((tree.size, tree.files), (4100, 3));
        assert_eq!(tree.children[0].size, 3000);
        assert!(remove(&mut tree, &[9]).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn essentials_found_by_relative_path() {
        let leaf = |name: &str, size: u64| Node { name: name.into(), size, is_dir: false, files: 1, mtime: 0, children: vec![] };
        let dir = |name: &str, children: Vec<Node>| Node {
            name: name.into(),
            size: children.iter().map(|c| c.size).sum(),
            is_dir: true,
            files: children.len() as u64,
            mtime: 0,
            children,
        };
        let root = dir("C:", vec![leaf("pagefile.sys", 8 << 30), dir("Windows", vec![dir("WinSxS", vec![leaf("x", 7 << 30)])])]);
        let e = essentials(&root);
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].id, e[0].size), ("pagefile", 8 << 30));
        assert_eq!((e[1].id, e[1].path.as_str()), ("winsxs", r"C:\Windows\WinSxS"));
    }

    #[test]
    fn recycle_guard() {
        let win = Path::new(r"C:\Windows");
        let prof = Path::new(r"C:\Users\me");
        for ok in [r"C:\Users\me\Downloads\big.iso", r"C:\Users\me\AppData\Local\Temp\x", r"D:\Games\Old", r"C:\Program Files (x86)\Steam\steamapps\common\Old"] {
            assert!(recyclable(Path::new(ok), win, prof), "{ok}");
        }
        for no in [
            r"C:\", r"C:", r"D:\", r"C:\Windows\Temp", r"C:\Windows\System32\x.dll", r"C:\Users\other\Desktop\a.txt",
            r"C:\Users\me", r"C:\Users\me\AppData", r"C:\Users\me\AppData\Local", r"C:\Program Files", r"C:\ProgramData\x",
            r"C:\Users", r"\\server\share\x", r"C:\Users\me\..\other",
        ] {
            assert!(!recyclable(Path::new(no), win, prof), "{no}");
        }
    }

    #[test]
    fn verbatim_prefix_is_stripped() {
        assert_eq!(strip_verbatim(Path::new(r"\\?\C:\Users\me\a.txt")), PathBuf::from(r"C:\Users\me\a.txt"));
        assert_eq!(strip_verbatim(Path::new(r"\\?\UNC\srv\share\x")), PathBuf::from(r"\\srv\share\x"));
        assert_eq!(strip_verbatim(Path::new(r"D:\x")), PathBuf::from(r"D:\x"));
        // A forward-slash spelling of System32 resolves to the real path, which the guard blocks.
        let win = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let real = strip_verbatim(&fs::canonicalize(format!("{}/System32", win.replace('\\', "/"))).unwrap());
        assert!(!recyclable(&real, Path::new(&win), Path::new(r"C:\Users\me")), "{real:?}");
    }

    #[test]
    fn recycle_refuses_root_and_unknown_ids() {
        let dir = sandbox("refuse");
        let p = ScanProgress::default();
        let (mut tree, _) = scan(&dir, &p, &AtomicBool::new(false)).unwrap();
        assert_eq!(recycle(&mut tree, &[vec![]]).unwrap_err(), RecycleError::Protected);
        assert_eq!(recycle(&mut tree, &[vec![42]]).unwrap_err(), RecycleError::NotFound);
        assert_eq!(recycle(&mut tree, &[]).unwrap_err(), RecycleError::NotFound);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Security: the folder is swapped for a junction to `%SystemRoot%` after the scan (TOCTOU).
    /// The stale node `a\System32` must resolve through the junction and be refused — never recycled.
    #[test]
    fn recycle_refuses_paths_through_a_junction() {
        let dir = std::env::temp_dir().join(format!("g2boost-scan-{}-junction", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(r"a\System32")).unwrap();
        fs::write(dir.join(r"a\System32\x.bin"), [0u8; 10]).unwrap();
        let p = ScanProgress::default();
        let (mut tree, _) = scan(&dir, &p, &AtomicBool::new(false)).unwrap();
        assert_eq!(tree.children[0].name, "a");
        fs::remove_dir_all(dir.join("a")).unwrap();
        let win = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        crate::cleanup::tests::make_junction(&dir.join("a"), Path::new(&win));
        // Intermediate junction: `a\System32` itself is a real directory, only its parent is a link.
        assert_eq!(recycle(&mut tree, &[vec![0, 0]]).unwrap_err(), RecycleError::Protected);
        // The junction itself is a reparse point: refused before anything is resolved.
        assert_eq!(recycle(&mut tree, &[vec![0]]).unwrap_err(), RecycleError::Protected);
        assert!(Path::new(&win).join("System32").exists(), "nothing behind the junction was touched");
        fs::remove_dir(dir.join("a")).unwrap(); // removes the junction only
        let _ = fs::remove_dir_all(&dir);
    }

    /// Real Recycle Bin round trip on a throwaway file (manual: `cargo test -- --ignored recycle_live`).
    #[test]
    #[ignore]
    fn recycle_live() {
        // SAFETY: same override every data-dir test in this crate uses (keeps the real history clean).
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
        let dir = sandbox("live");
        let p = ScanProgress::default();
        let (mut tree, _) = scan(&dir, &p, &AtomicBool::new(false)).unwrap();
        let out = recycle(&mut tree, &[vec![2], vec![0, 1]]).unwrap(); // root.bin + a/one.bin
        assert_eq!((out.recycled, out.bytes, out.failed), (2, 3100, 0));
        assert!(!dir.join("root.bin").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_bad_roots_and_honours_cancel() {
        let p = ScanProgress::default();
        assert!(matches!(scan(Path::new("relative"), &p, &AtomicBool::new(false)), Err(ScanError::BadRoot)));
        let dir = sandbox("cancel");
        let (tree, sum) = scan(&dir, &p, &AtomicBool::new(true)).unwrap();
        assert!(sum.cancelled);
        assert_eq!(tree.children.len(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn profile_folder_is_hidden_in_the_usage_tree() {
        let leaf = |name: &str| NodeView {
            name: name.into(),
            size: 1,
            is_dir: true,
            files: 0,
            mtime: 0,
            ids: Vec::new(),
            children: Vec::new(),
            others_size: 0,
            others_count: 0,
        };
        let mut users = leaf("Users");
        users.children = vec![leaf("Maria"), leaf("Public")];
        let mut root = leaf("C:");
        root.children = vec![users, leaf("Maria")];
        redact_view(&mut root, false, "maria");
        assert_eq!(root.children[0].children[0].name, "%USERNAME%");
        assert_eq!(root.children[0].children[1].name, "Public");
        assert_eq!(root.children[1].name, "Maria", "only under Users");
    }
}
