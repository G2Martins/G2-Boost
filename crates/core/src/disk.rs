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
}

/// Largest `n` files anywhere under `root` (min-heap, one pass).
pub fn top_files(root: &Node, n: usize) -> Vec<BigFile> {
    let mut heap: BinaryHeap<std::cmp::Reverse<BigFile>> = BinaryHeap::with_capacity(n + 1);
    fn rec(node: &Node, prefix: &str, heap: &mut BinaryHeap<std::cmp::Reverse<BigFile>>, n: usize) {
        for c in &node.children {
            let path = if prefix.is_empty() { c.name.clone() } else { format!("{prefix}\\{}", c.name) };
            if c.is_dir {
                rec(c, &path, heap, n);
            } else {
                if heap.len() == n && heap.peek().is_some_and(|r| r.0.size >= c.size) {
                    continue;
                }
                heap.push(std::cmp::Reverse(BigFile { size: c.size, path, mtime: c.mtime }));
                if heap.len() > n {
                    heap.pop();
                }
            }
        }
    }
    rec(root, &root.name, &mut heap, n);
    let mut out: Vec<BigFile> = heap.into_iter().map(|r| r.0).collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.size));
    out
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
        let top = top_files(&tree, 2);
        assert_eq!(top.len(), 2);
        assert!(top[0].path.ends_with("a\\deep\\two.bin"));
        assert_eq!(top[1].size, 3000);
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
}
