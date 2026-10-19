//! Duplicate file finder. Read-only: it never deletes (removal goes through the Recycle Bin guard
//! in `disk`).
//!
//! Pipeline, cheapest filter first: group by size, then hash the first and last 64 KiB, then
//! confirm survivors by streaming a full byte comparison (1 MiB buffers). A hash is never trusted
//! alone. Hard links to the same file (same volume serial + file index) are collapsed so they are
//! not reported as duplicates (deleting one frees nothing). Reparse points, zero-byte files and
//! unreadable files are skipped; unreadable ones are counted in `DupProgress::errors`.
//! The scan tree from `disk` already skips reparse points; we re-check at open time because the
//! filesystem may have changed since the scan (hostile input).

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::fs::File;
use std::hash::Hasher;
use std::io::{self, Read, Seek, SeekFrom};
use std::os::windows::fs::MetadataExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use rayon::prelude::*;
use serde::Serialize;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};

use crate::disk::Node;

const EDGE: u64 = 64 * 1024;
const BUF: usize = 1024 * 1024;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// Live counters for the UI.
#[derive(Default)]
pub struct DupProgress {
    /// Files that share a size with another file (work to do).
    pub candidates: AtomicU64,
    /// Bytes read so far (edges + full comparisons).
    pub hashed_bytes: AtomicU64,
    /// Files that could not be opened or read.
    pub errors: AtomicU64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DupGroup {
    pub size: u64,
    pub paths: Vec<String>,
    /// `size * (n - 1)`: what is freed by keeping one copy.
    pub wasted: u64,
}

/// Every file of at least `min_size` bytes (and never empty) with its full path.
pub fn files_from_tree(root: &Node, root_path: &Path, min_size: u64) -> Vec<(PathBuf, u64)> {
    fn walk(n: &Node, p: &Path, min: u64, out: &mut Vec<(PathBuf, u64)>) {
        for c in &n.children {
            let cp = p.join(&c.name);
            if c.is_dir {
                walk(c, &cp, min, out);
            } else if c.size >= min {
                out.push((cp, c.size));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root_path, min_size.max(1), &mut out);
    out
}

/// Volume serial + file index: equal means the same physical file.
fn identity(f: &File) -> Option<(u32, u64)> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is owned by `f` and stays open for the call; `info` is a valid out buffer.
    unsafe { GetFileInformationByHandle(HANDLE(f.as_raw_handle()), &mut info) }.ok()?;
    Some((info.dwVolumeSerialNumber, (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow)))
}

/// Open a regular file (no reparse point) and check its size still matches.
fn open_checked(path: &Path, size: u64) -> io::Result<File> {
    let f = File::open(path)?;
    let md = f.metadata()?;
    if md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !md.is_file() || md.len() != size {
        return Err(io::Error::other("not a stable regular file"));
    }
    Ok(f)
}

fn edge_hash(f: &mut File, size: u64, progress: &DupProgress) -> io::Result<u64> {
    let mut h = DefaultHasher::new();
    let n = EDGE.min(size) as usize;
    let mut buf = vec![0u8; n];
    f.read_exact(&mut buf)?;
    h.write(&buf);
    if size > EDGE {
        f.seek(SeekFrom::Start(size - EDGE))?;
        f.read_exact(&mut buf)?;
        h.write(&buf);
        progress.hashed_bytes.fetch_add(EDGE, Ordering::Relaxed);
    }
    progress.hashed_bytes.fetch_add(n as u64, Ordering::Relaxed);
    Ok(h.finish())
}

type FileId = (u32, u64);

enum Cmp {
    Same,
    Diff,
    Failed,
    Cancelled,
}

fn read_full(f: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

fn same_content(a: &Path, b: &Path, size: u64, cancel: &AtomicBool, progress: &DupProgress) -> Cmp {
    let (Ok(mut fa), Ok(mut fb)) = (open_checked(a, size), open_checked(b, size)) else {
        return Cmp::Failed;
    };
    let (mut ba, mut bb) = (vec![0u8; BUF], vec![0u8; BUF]);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Cmp::Cancelled;
        }
        let (Ok(na), Ok(nb)) = (read_full(&mut fa, &mut ba), read_full(&mut fb, &mut bb)) else {
            return Cmp::Failed;
        };
        progress.hashed_bytes.fetch_add((na + nb) as u64, Ordering::Relaxed);
        if na != nb || ba[..na] != bb[..nb] {
            return Cmp::Diff;
        }
        if na == 0 {
            return Cmp::Same;
        }
    }
}

/// Confirmed duplicate groups, largest waste first. `cancel` stops early and returns what is
/// already confirmed.
pub fn find(files: Vec<(PathBuf, u64)>, cancel: &AtomicBool, progress: &DupProgress) -> Vec<DupGroup> {
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    for (p, s) in files {
        if s > 0 {
            by_size.entry(s).or_default().push(p);
        }
    }
    by_size.retain(|_, v| v.len() >= 2);
    let n: usize = by_size.values().map(Vec::len).sum();
    progress.candidates.store(n as u64, Ordering::Relaxed);

    // Stage 1 (parallel over files): identity + edge hash.
    let items: Vec<(u64, PathBuf)> = by_size.into_iter().flat_map(|(s, v)| v.into_iter().map(move |p| (s, p))).collect();
    let probed: Vec<(u64, PathBuf, (u32, u64), u64)> = items
        .into_par_iter()
        .filter_map(|(size, path)| {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let r = open_checked(&path, size).and_then(|mut f| {
                let id = identity(&f).ok_or_else(|| io::Error::other("no identity"))?;
                Ok((id, edge_hash(&mut f, size, progress)?))
            });
            match r {
                Ok((id, h)) => Some((size, path, id, h)),
                Err(_) => {
                    progress.errors.fetch_add(1, Ordering::Relaxed);
                    None
                }
            }
        })
        .collect();

    // Group by (size, edge hash), collapsing hard links (same identity) to one entry.
    let mut buckets: HashMap<(u64, u64), Vec<(PathBuf, FileId)>> = HashMap::new();
    for (size, path, id, h) in probed {
        let b = buckets.entry((size, h)).or_default();
        if !b.iter().any(|(_, i)| *i == id) {
            b.push((path, id));
        }
    }
    let buckets: Vec<(u64, Vec<PathBuf>)> = buckets
        .into_iter()
        .filter(|(_, v)| v.len() >= 2)
        .map(|((s, _), v)| (s, v.into_iter().map(|(p, _)| p).collect()))
        .collect();

    // Stage 2 (parallel over buckets): cluster by full comparison against each cluster head.
    let mut groups: Vec<DupGroup> = buckets
        .into_par_iter()
        .flat_map_iter(|(size, paths)| {
            let mut clusters: Vec<Vec<PathBuf>> = Vec::new();
            'files: for p in paths {
                for c in &mut clusters {
                    match same_content(&c[0], &p, size, cancel, progress) {
                        Cmp::Same => {
                            c.push(p);
                            continue 'files;
                        }
                        Cmp::Diff => {}
                        Cmp::Failed => {
                            progress.errors.fetch_add(1, Ordering::Relaxed);
                            continue 'files;
                        }
                        Cmp::Cancelled => return Vec::new(),
                    }
                }
                clusters.push(vec![p]);
            }
            clusters
                .into_iter()
                .filter(|c| c.len() >= 2)
                .map(|c| DupGroup {
                    size,
                    wasted: size * (c.len() as u64 - 1),
                    paths: c.iter().map(|p| p.to_string_lossy().into_owned()).collect(),
                })
                .collect::<Vec<_>>()
        })
        .collect();
    groups.sort_by(|a, b| b.wasted.cmp(&a.wasted).then_with(|| a.paths.cmp(&b.paths)));
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sandbox(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("g2boost-dupes-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn run(files: &[&Path]) -> (Vec<DupGroup>, DupProgress) {
        let v = files.iter().map(|p| (p.to_path_buf(), fs::metadata(p).unwrap().len())).collect();
        let pr = DupProgress::default();
        let g = find(v, &AtomicBool::new(false), &pr);
        (g, pr)
    }

    #[test]
    fn groups_identical_and_rejects_lookalikes() {
        let d = sandbox("basic");
        let body = vec![7u8; 1000];
        let (a, b, c) = (d.join("a"), d.join("b"), d.join("c"));
        for p in [&a, &b, &c] {
            fs::write(p, &body).unwrap();
        }
        let same_size = d.join("d");
        fs::write(&same_size, vec![8u8; 1000]).unwrap();
        // Same first/last 64 KiB, different middle.
        let mut big = vec![1u8; 300 * 1024];
        let (e, f) = (d.join("e"), d.join("f"));
        fs::write(&e, &big).unwrap();
        big[150 * 1024] = 99;
        fs::write(&f, &big).unwrap();
        let (g, pr) = run(&[&a, &b, &c, &same_size, &e, &f]);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].paths.len(), 3);
        assert_eq!(g[0].wasted, 2000);
        assert_eq!(pr.errors.load(Ordering::Relaxed), 0);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn hard_links_are_not_duplicates_but_copies_are() {
        let d = sandbox("links");
        let (a, l, c) = (d.join("a"), d.join("l"), d.join("c"));
        fs::write(&a, vec![5u8; 4096]).unwrap();
        fs::hard_link(&a, &l).unwrap();
        let (g, _) = run(&[&a, &l]);
        assert!(g.is_empty());
        fs::write(&c, vec![5u8; 4096]).unwrap();
        let (g, _) = run(&[&a, &l, &c]);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].paths.len(), 2);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_files_count_as_errors_and_cancel_is_honored() {
        let d = sandbox("cancel");
        let (a, b) = (d.join("a"), d.join("b"));
        fs::write(&a, vec![1u8; 100]).unwrap();
        fs::write(&b, vec![1u8; 100]).unwrap();
        let ghost = d.join("ghost");
        let pr = DupProgress::default();
        let files = vec![(a.clone(), 100), (b.clone(), 100), (ghost, 100)];
        let g = find(files.clone(), &AtomicBool::new(false), &pr);
        assert_eq!(g.len(), 1);
        assert_eq!(pr.errors.load(Ordering::Relaxed), 1);
        let g = find(files, &AtomicBool::new(true), &DupProgress::default());
        assert!(g.is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn tree_paths_and_min_size() {
        let leaf = |n: &str, s: u64| Node { name: n.into(), size: s, is_dir: false, files: 1, mtime: 0, children: vec![] };
        let sub = Node { name: "sub".into(), size: 15, is_dir: true, files: 2, mtime: 0, children: vec![leaf("x", 10), leaf("e", 0)] };
        let root = Node { name: "root".into(), size: 20, is_dir: true, files: 3, mtime: 0, children: vec![sub, leaf("y", 5)] };
        let v = files_from_tree(&root, Path::new("C:\\r"), 6);
        assert_eq!(v, vec![(PathBuf::from("C:\\r\\sub\\x"), 10)]);
        assert_eq!(files_from_tree(&root, Path::new("C:\\r"), 0).len(), 2);
    }
}
