//! Path classification guard. Every path that comes from outside the core
//! (UI selection, MCP tool argument, preset file) goes through [`SafePath`]
//! before any destructive operation. [`pin`] and [`Pinned::open_below`] then re-check on an open
//! handle, right before acting, that the path still names what was checked (TOCTOU).

use std::fs;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Component, Path, PathBuf};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    DELETE, FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX, FILE_DISPOSITION_INFO_EX_FLAGS,
    FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FileDispositionInfo, FileDispositionInfoEx,
    GETFINALPATHNAMEBYHANDLE_FLAGS, GetFinalPathNameByHandleW, GetFullPathNameW, GetLongPathNameW, SetFileInformationByHandle,
    VOLUME_NAME_NT,
};
use windows::core::PCWSTR;

/// Where a path lives, from the point of view of "may we delete here?".
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathClass {
    /// `C:\Windows\System32`, `WinSxS`, boot files: never touched by cleanup.
    SystemCritical,
    /// Rest of `C:\Windows` and `ProgramData`: only catalog entries.
    System,
    /// Current user's profile.
    UserProfile,
    /// Another user's profile: never touched.
    OtherUserProfile,
    /// Anything else on a fixed/removable drive.
    Other,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SafePathError {
    #[error("path must be absolute")]
    NotAbsolute,
    #[error("path contains '..' or '.' components")]
    Relative,
    #[error("path is inside a system-critical folder")]
    SystemCritical,
    #[error("path belongs to another user's profile")]
    OtherUser,
    #[error("UNC / network paths are not allowed")]
    Unc,
    /// Windows would rewrite it before opening (`/`, doubled `\`, `.` segments, trailing dots or
    /// spaces, `\\?\` and device forms, `:stream`, DOS device names): what is classified must be
    /// exactly what gets opened, so these are refused instead of guessed.
    #[error("path is not in plain form")]
    NotPlain,
}

/// A validated absolute path plus its class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafePath {
    path: PathBuf,
    class: PathClass,
}

impl SafePath {
    /// Validate `raw` against the rules. `current_profile` is the running user's profile dir
    /// (`%USERPROFILE%`), `windows_dir` the Windows folder (best from `GetWindowsDirectoryW`: the
    /// environment is the user's). Lexical, on the plain spelling only ([`SafePathError::NotPlain`]);
    /// links and 8.3 short names are the caller's to resolve first (`canonicalize`, [`pin`]).
    pub fn classify(
        raw: &Path,
        windows_dir: &Path,
        current_profile: &Path,
    ) -> Result<Self, SafePathError> {
        if !raw.is_absolute() {
            return Err(SafePathError::NotAbsolute);
        }
        if raw
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(SafePathError::Relative);
        }
        if let Some(Component::Prefix(p)) = raw.components().next()
            && matches!(
                p.kind(),
                std::path::Prefix::UNC(..) | std::path::Prefix::VerbatimUNC(..)
            )
        {
            return Err(SafePathError::Unc);
        }
        if !plain(raw) {
            return Err(SafePathError::NotPlain);
        }

        let lower = |p: &Path| p.to_string_lossy().trim_end_matches('\\').to_ascii_lowercase();
        let raw_l = lower(raw);
        let win_l = lower(windows_dir);
        let prof_l = lower(current_profile);

        let critical = ["system32", "syswow64", "winsxs", "boot", "servicing"]
            .iter()
            .any(|sub| starts_with_dir(&raw_l, &format!("{win_l}\\{sub}")));
        if critical || raw_l == win_l {
            return Err(SafePathError::SystemCritical);
        }

        let class = if starts_with_dir(&raw_l, &win_l) {
            PathClass::System
        } else if starts_with_dir(&raw_l, &prof_l) {
            PathClass::UserProfile
        } else if let Some(users_root) = current_profile.parent()
            && starts_with_dir(&raw_l, &lower(users_root))
        {
            // Under C:\Users but not under the current profile.
            // `C:\Users\Public` is shared, treat as Other.
            let rest = raw_l[lower(users_root).len()..].trim_start_matches('\\');
            if rest == "public" || rest.starts_with("public\\") {
                PathClass::Other
            } else {
                return Err(SafePathError::OtherUser);
            }
        } else if raw_l.contains("\\programdata") {
            PathClass::System
        } else {
            PathClass::Other
        };

        Ok(Self {
            path: raw.to_path_buf(),
            class,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn class(&self) -> PathClass {
        self.class
    }
}

/// `child` is `dir` itself or lives below it (case-insensitive, already lowered).
pub(crate) fn starts_with_dir(child: &str, dir: &str) -> bool {
    let dir = dir.trim_end_matches('\\');
    child == dir || child.starts_with(&format!("{dir}\\"))
}

/// Reparse point (junction, symlink, mount point): never walked into by scans or cleanup.
pub(crate) fn is_reparse(md: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
}

fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

/// A Win32 call that fills a buffer and returns the length it needs (grown once if too small).
fn fill(mut call: impl FnMut(&mut [u16]) -> u32) -> Option<String> {
    let mut buf = vec![0u16; 300];
    for _ in 0..2 {
        let n = call(&mut buf) as usize;
        if n == 0 {
            return None;
        }
        if n < buf.len() {
            return Some(String::from_utf16_lossy(&buf[..n]));
        }
        buf.resize(n + 1, 0);
    }
    None
}

/// The path as every Win32 file call rewrites it before opening (lexical: `/`, `.`, `..`, doubled
/// separators, trailing dots and spaces, DOS device names).
fn full_path(p: &Path) -> Option<String> {
    let w = wide(p);
    // SAFETY: NUL-terminated input; the wrapper passes the buffer length.
    fill(|buf| unsafe { GetFullPathNameW(PCWSTR(w.as_ptr()), Some(buf), None) })
}

/// `X:\…` exactly as Windows itself resolves it, with no stream name and no forward slash.
fn plain(raw: &Path) -> bool {
    let s = raw.to_string_lossy();
    let b = s.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\';
    drive && !s[2..].contains([':', '/']) && full_path(raw).is_some_and(|f| f.eq_ignore_ascii_case(&s))
}

/* ---------------- acting on a handle, not on a path (TOCTOU) ---------------- */

/// `path` opened as itself: a reparse point is opened as the link, never followed; folders too.
fn open_itself(path: &Path, access: u32) -> io::Result<(fs::File, fs::Metadata)> {
    let f = fs::OpenOptions::new()
        .access_mode(access)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | FILE_FLAG_BACKUP_SEMANTICS.0)
        .open(path)?;
    let md = f.metadata()?;
    if is_reparse(&md) {
        return Err(io::Error::other("reparse_point"));
    }
    Ok((f, md))
}

/// Where Windows really has the object open (long names, real case: `FILE_NAME_NORMALIZED` is 0).
fn final_path(f: &fs::File, flags: GETFINALPATHNAMEBYHANDLE_FLAGS) -> io::Result<String> {
    let h = HANDLE(f.as_raw_handle());
    // SAFETY: `h` is owned by `f`, alive for the call; the wrapper passes the buffer length.
    fill(|buf| unsafe { GetFinalPathNameByHandleW(h, buf, flags) }).ok_or_else(io::Error::last_os_error)
}

fn same_path(a: &str, b: &str) -> bool {
    a.trim_end_matches('\\').to_lowercase() == b.trim_end_matches('\\').to_lowercase()
}

/// A file or folder pinned where its path says: opened as itself (not a reparse point), and its
/// final path is that path (once Windows' own rewrite and short names are applied), so no junction
/// or link sits anywhere on the way. The handle stays open while the value lives; it shares
/// everything (others may still move or delete it, but then what moved is what was checked).
pub(crate) struct Pinned {
    pub(crate) md: fs::Metadata,
    /// Final path, DOS form (`C:\…`).
    pub(crate) real: PathBuf,
    /// Final path, NT form (`\device\harddiskvolumen\…`, lowercase): what entries below must start with.
    nt: String,
    _file: fs::File,
}

pub(crate) fn pin(path: &Path) -> io::Result<Pinned> {
    let (file, md) = open_itself(path, FILE_READ_ATTRIBUTES.0)?;
    let dos = final_path(&file, GETFINALPATHNAMEBYHANDLE_FLAGS(0))?; // VOLUME_NAME_DOS
    let real = dos.strip_prefix(r"\\?\").unwrap_or(&dos).to_string();
    let mut want = full_path(path).ok_or_else(io::Error::last_os_error)?;
    if want.contains('~') {
        let w = wide(Path::new(&want));
        // SAFETY: NUL-terminated input; the wrapper passes the buffer length.
        want = fill(|buf| unsafe { GetLongPathNameW(PCWSTR(w.as_ptr()), Some(buf)) }).ok_or_else(io::Error::last_os_error)?;
    }
    if !same_path(&want, &real) {
        return Err(io::Error::other("moved")); // reached through a junction, or swapped meanwhile
    }
    let nt = final_path(&file, VOLUME_NAME_NT)?.to_lowercase();
    Ok(Pinned { md, real: PathBuf::from(real), nt, _file: file })
}

impl Pinned {
    /// `p`, a path below this folder, opened as itself with delete access. Refused when it is a
    /// reparse point or when Windows resolved it outside this folder (a folder on the way was swapped
    /// for a junction after it was listed). Delete it with [`delete_open`]: that acts on this very
    /// object, whatever its path names by then. Held open by another program without delete
    /// sharing, it opens for attributes only: a folder can still be walked, the delete then fails.
    pub(crate) fn open_below(&self, p: &Path) -> io::Result<(fs::File, fs::Metadata)> {
        let (f, md) = open_itself(p, DELETE.0 | FILE_READ_ATTRIBUTES.0).or_else(|_| open_itself(p, FILE_READ_ATTRIBUTES.0))?;
        let nt = final_path(&f, VOLUME_NAME_NT)?.to_lowercase();
        if nt == self.nt || !starts_with_dir(&nt, &self.nt) {
            return Err(io::Error::other("outside"));
        }
        Ok((f, md))
    }
}

/// Delete what `f` (opened by [`Pinned::open_below`]) is open on, when it closes; a folder only when
/// empty. POSIX semantics take the name away even if another program keeps the file open with
/// delete sharing; read-only files in a cache go too. FAT/exFAT know neither: classic delete there.
pub(crate) fn delete_open(f: &fs::File) -> io::Result<()> {
    let h = HANDLE(f.as_raw_handle());
    let ex = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_INFO_EX_FLAGS(
            FILE_DISPOSITION_FLAG_DELETE.0 | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS.0 | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE.0,
        ),
    };
    let classic = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: `h` is owned by `f`; each struct is passed with its own size.
    unsafe {
        SetFileInformationByHandle(h, FileDispositionInfoEx, (&raw const ex).cast(), size_of::<FILE_DISPOSITION_INFO_EX>() as u32)
            .or_else(|_| SetFileInformationByHandle(h, FileDispositionInfo, (&raw const classic).cast(), size_of::<FILE_DISPOSITION_INFO>() as u32))
    }
    .map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win() -> PathBuf {
        PathBuf::from(r"C:\Windows")
    }
    fn me() -> PathBuf {
        PathBuf::from(r"C:\Users\Windows")
    }
    fn classify(p: &str) -> Result<SafePath, SafePathError> {
        SafePath::classify(Path::new(p), &win(), &me())
    }

    #[test]
    fn rejects_relative_and_dotdot() {
        assert_eq!(classify(r"Temp\x").unwrap_err(), SafePathError::NotAbsolute);
        assert_eq!(
            classify(r"C:\Users\Windows\..\Other").unwrap_err(),
            SafePathError::Relative
        );
    }

    #[test]
    fn rejects_system_critical_and_unc() {
        assert_eq!(
            classify(r"C:\Windows\System32\drivers").unwrap_err(),
            SafePathError::SystemCritical
        );
        assert_eq!(
            classify(r"C:\WINDOWS\WinSxS").unwrap_err(),
            SafePathError::SystemCritical
        );
        assert_eq!(classify(r"C:\Windows").unwrap_err(), SafePathError::SystemCritical);
        assert_eq!(
            classify(r"\\server\share\x").unwrap_err(),
            SafePathError::Unc
        );
    }

    #[test]
    fn classifies_profiles() {
        assert_eq!(
            classify(r"C:\Users\Windows\AppData\Local\Temp").unwrap().class(),
            PathClass::UserProfile
        );
        assert_eq!(
            classify(r"C:\Users\Other\AppData").unwrap_err(),
            SafePathError::OtherUser
        );
        assert_eq!(classify(r"C:\Users\Public\x").unwrap().class(), PathClass::Other);
        assert_eq!(classify(r"C:\Users\Publicity\x").unwrap_err(), SafePathError::OtherUser, "a user named Public…");
        assert_eq!(classify(r"C:\Windows\Temp").unwrap().class(), PathClass::System);
        assert_eq!(classify(r"C:\ProgramData\NVIDIA").unwrap().class(), PathClass::System);
        assert_eq!(classify(r"D:\Games").unwrap().class(), PathClass::Other);
    }

    /// 2026-11-21 review: the check was lexical, so other spellings of System32 or of another user's
    /// profile passed as "other". Every spelling Windows would rewrite is refused now.
    #[test]
    fn other_spellings_of_a_path_are_refused() {
        for p in [
            r"C:\Windows\.\System32",
            r"C:\Windows/System32",
            r"\\?\C:\Windows\System32",
            r"\\.\C:\Windows\System32",
            r"C:\Windows\\System32",
            r"C:\Users/Other/AppData",
            r"C:\Windows\System32.",
            r"C:\Windows.\System32",
            r"C:\Windows\System32 ",
            r"C:\Windows\System32:stream",
            r"C:\Windows\System32\drivers\.",
            r"C:/Windows/Temp",
        ] {
            assert_eq!(classify(p).unwrap_err(), SafePathError::NotPlain, "{p}");
        }
        assert_eq!(classify(r"\\?\UNC\server\share").unwrap_err(), SafePathError::Unc);
        // A trailing separator is plain, and the Windows folder itself stays critical with one.
        assert_eq!(classify(r"C:\Windows\").unwrap_err(), SafePathError::SystemCritical);
        assert_eq!(classify(r"C:\Windows\System32\").unwrap_err(), SafePathError::SystemCritical);
        assert_eq!(classify(r"c:\windows\temp\").unwrap().class(), PathClass::System);
        assert_eq!(classify(r"C:\").unwrap().class(), PathClass::Other);
    }

    #[test]
    fn pin_refuses_links_on_the_way_and_does_not_block_a_move() {
        let dir = std::env::temp_dir().join(format!("g2boost-safety-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(r"real\sub")).unwrap();
        fs::write(dir.join(r"real\sub\f.bin"), b"x").unwrap();
        crate::cleanup::tests::make_junction(&dir.join("link"), &dir.join("real"));
        assert!(pin(&dir.join(r"real\sub")).is_ok());
        assert!(pin(&dir.join("link")).is_err(), "the link itself");
        assert!(pin(&dir.join(r"link\sub")).is_err(), "reached through the link");
        // Below a pinned folder: a path through the link resolves outside it and is refused.
        let real = pin(&dir.join("real")).unwrap();
        assert!(real.open_below(&dir.join(r"real\sub\f.bin")).is_ok());
        let other = pin(&dir.join(r"real\sub")).unwrap();
        assert!(other.open_below(&dir.join(r"real\sub")).is_err(), "the pinned folder is not below itself");
        drop(other);
        // Held open, a pinned folder can still be moved (the Recycle Bin is a move).
        let held = pin(&dir.join(r"real\sub")).unwrap();
        fs::rename(dir.join(r"real\sub"), dir.join(r"real\moved")).unwrap();
        drop(held);
        // Deleting through the handle takes a read-only file too; a folder only once empty.
        let f = dir.join(r"real\moved\f.bin");
        let mut perms = fs::metadata(&f).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&f, perms).unwrap();
        let (h, _) = real.open_below(&dir.join(r"real\moved")).unwrap();
        assert!(delete_open(&h).is_err(), "not empty");
        drop(h);
        let (h, _) = real.open_below(&f).unwrap();
        delete_open(&h).unwrap();
        drop(h);
        assert!(!f.exists());
        fs::remove_dir(dir.join("link")).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }
}
