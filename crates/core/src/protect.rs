//! Permissions an elevated G2 Boost depends on (`Docs/security/protected-storage.md`).
//!
//! **Protected store.** Tweak, DNS and network adapter backups and the undo of a leftover removal
//! become admin writes later (registry values, DNS servers, re-imported keys). The data dir
//! (`%LOCALAPPDATA%\G2 Boost`) is writable by every program of the user, so an elevated G2 Boost
//! keeps them in `%ProgramData%\G2 Boost\secure`: owner Administrators, a protected DACL (nothing
//! inherited) for SYSTEM and Administrators only. A folder there that is a junction, or that anyone
//! else owns or may write (another program can create it first), is never used: it is renamed
//! aside untouched and a new one is made. Not elevated, the data dir as before. Elevated, the data
//! dir is read once: the one-time move of what an older version left there (`migrate`); every
//! restore still validates its entries.
//!
//! **Install folder.** The installer may put G2 Boost anywhere (the owner's copy lives in
//! `G:\G2 Boost`, where Authenticated Users may modify). Elevated, an installed copy whose folder
//! others can write gets a protected DACL: SYSTEM and Administrators full, Users and Authenticated
//! Users read and run. Only on the installer's own folder (`G2 Boost.exe` + `uninstall.exe`,
//! nothing else), never a drive root, a profile or Windows; a portable copy is never touched.

use std::io::{self, Read};
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::OnceLock;

use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW,
};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, GetSecurityDescriptorOwner, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
};
use windows::Win32::Storage::FileSystem::{
    CreateDirectoryW, FILE_DISPOSITION_INFO, FILE_NAME_NORMALIZED, FileDispositionInfo, GetFinalPathNameByHandleW,
    SetFileInformationByHandle,
};
use windows::core::{BOOL, PCWSTR};

use crate::antivirus::{self, Ace};
use crate::registry::{self, known_folder, wide};
use crate::safety::{is_reparse, starts_with_dir};

/// Owner Administrators; SYSTEM and Administrators only, nothing inherited from ProgramData (which
/// lets Users add files to every folder below it).
const STORE_SDDL: &str = "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";
/// The install folder: the same, plus read and run (0x1200a9) for Users and Authenticated Users.
const INSTALL_SDDL: &str = "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)(A;OICI;0x1200a9;;;AU)";
/// On a folder above the exe's: rights that let someone else rename or replace what is on the
/// path (delete, delete child, change permissions or owner, everything).
const SWAP: u32 = 0x1_0000 | 0x40 | 0x4_0000 | 0x8_0000 | 0x1000_0000;
/// A drive root cannot be renamed: delete on it means nothing (Authenticated Users have it on `G:\`).
const SWAP_ROOT: u32 = SWAP & !0x1_0000;
/// What the migration copies: our JSON files, nothing big.
const MAX_MIGRATED: u64 = 1 << 20;

/* ---------------- security descriptors ---------------- */

/// A security descriptor parsed from SDDL, freed on drop.
pub(crate) struct Sd(pub(crate) PSECURITY_DESCRIPTOR);

impl Sd {
    pub(crate) fn new(sddl: &str) -> io::Result<Self> {
        let w = wide(sddl);
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: NUL-terminated string; the API allocates `sd`, freed in Drop.
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(w.as_ptr()), SDDL_REVISION_1, &mut sd, None) }
            .map_err(|e| io::Error::other(e.message()))?;
        Ok(Sd(sd))
    }
}

impl Drop for Sd {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW, freed once.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0.0)));
        }
    }
}

fn win32(e: windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(e.code().0 & 0xFFFF)
}

/// A new folder with `sddl` from the start (no moment with ProgramData's inherited grants).
fn create_dir_with(dir: &Path, sddl: &str) -> io::Result<()> {
    let sd = Sd::new(sddl)?;
    let sa = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sd.0.0, bInheritHandle: BOOL(0) };
    let w = wide(&dir.to_string_lossy());
    // SAFETY: NUL-terminated path; `sa` and the descriptor it points to outlive the call.
    unsafe { CreateDirectoryW(PCWSTR(w.as_ptr()), Some(&sa)) }.map_err(win32)
}

/// Owner and a protected DACL from `sddl` on an existing file or folder. Inheritable grants reach
/// what is inside (SetNamedSecurityInfo propagates them).
fn set_security(path: &Path, sddl: &str) -> io::Result<()> {
    let sd = Sd::new(sddl)?;
    let (mut owner, mut dacl) = (PSID::default(), std::ptr::null_mut::<ACL>());
    let (mut present, mut defaulted) = (BOOL(0), BOOL(0));
    let w = wide(&path.to_string_lossy());
    // SAFETY: owner and DACL point into `sd`, alive for the whole block; NUL-terminated path.
    unsafe {
        GetSecurityDescriptorOwner(sd.0, &mut owner, &mut defaulted).map_err(win32)?;
        GetSecurityDescriptorDacl(sd.0, &mut present, &mut dacl, &mut defaulted).map_err(win32)?;
        SetNamedSecurityInfoW(
            PCWSTR(w.as_ptr()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            Some(owner),
            None,
            Some(dacl),
            None,
        )
        .ok()
        .map_err(win32)
    }
}

/* ---------------- protected store ---------------- */

/// Ours: a real folder (no junction) that only SYSTEM, Administrators or TrustedInstaller own and
/// may write.
fn trusted_dir(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir() && !is_reparse(&m)) && !antivirus::writable(dir)
}

/// `dir` as a folder only SYSTEM and Administrators can change. One that exists and is not ours is
/// never used: renamed aside (a junction is renamed, not followed) and replaced by a new one.
fn ensure(dir: &Path) -> io::Result<()> {
    if trusted_dir(dir) {
        return Ok(());
    }
    if std::fs::symlink_metadata(dir).is_ok() {
        let mut aside = dir.as_os_str().to_owned();
        aside.push(format!(".untrusted-{}", registry::now_unix()));
        std::fs::rename(dir, &aside)?;
    }
    create_dir_with(dir, STORE_SDDL)?;
    if trusted_dir(dir) { Ok(()) } else { Err(io::Error::other("store_not_protected")) }
}

/// `<root>\secure`, with `root` and itself made safe. Fails for a standard user (the owner cannot
/// be set to Administrators): an elevated caller then has no store, never the data dir.
pub(crate) fn prepare(root: &Path) -> io::Result<PathBuf> {
    if let Some(parent) = root.parent() {
        std::fs::create_dir_all(parent)?;
    }
    ensure(root)?;
    let dir = root.join("secure");
    ensure(&dir)?;
    Ok(dir)
}

/// `%ProgramData%\G2 Boost` from the shell (never an environment variable). Debug builds (tests,
/// elevated test runs) follow `G2BOOST_DATA_DIR`; a release build ignores it.
fn store_root() -> Option<PathBuf> {
    if cfg!(debug_assertions)
        && let Some(d) = std::env::var_os("G2BOOST_DATA_DIR")
    {
        return Some(PathBuf::from(d).join("ProgramData").join("G2 Boost"));
    }
    known_folder(&windows::Win32::UI::Shell::FOLDERID_ProgramData).map(|p| p.join("G2 Boost"))
}

/// Where backups and the undo of a removal live: the data dir, or, elevated, the protected store
/// (made safe, and filled once from the data dir). Elevated it is never the data dir: without a
/// store this is an error, so nothing is backed up or restored from a folder others can write.
pub fn state_dir() -> io::Result<PathBuf> {
    if !crate::system::is_elevated() {
        return Ok(registry::data_dir());
    }
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = prepare(&store_root()?).ok()?;
        migrate(&registry::data_dir(), &dir);
        Some(dir)
    })
    .clone()
    .ok_or_else(|| io::Error::other("protected_store_unavailable"))
}

/// Names the migration takes: `<id>.json` (tweaks), `<unix>-<n>.json` (registry keys), the DNS
/// backup and the undo record. Nothing else in those folders is ours.
fn ours(name: &str) -> bool {
    name.strip_suffix(".json").is_some_and(|n| !n.is_empty() && n.len() <= 64 && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b)))
}

/// Move what an older version (or a standard-user run) left in the data dir into the store, once
/// (`migrated` marker). Each file is copied as a new file (it gets the store's permissions; a moved
/// file would keep the user's) and then deleted through the same handle, only when no junction or
/// link led to it: an admin delete never lands outside the data dir. Restores still validate.
pub(crate) fn migrate(from: &Path, to: &Path) {
    let mark = to.join("migrated");
    if mark.exists() {
        return;
    }
    let mut files: Vec<PathBuf> = ["dns_backup.json", "uninstall_undo.json"].iter().map(PathBuf::from).collect();
    for sub in ["backups", "uninstall_backups"] {
        if let Ok(rd) = std::fs::read_dir(from.join(sub)) {
            files.extend(rd.flatten().take(500).filter_map(|e| e.file_name().to_str().filter(|n| ours(n)).map(|n| Path::new(sub).join(n))));
        }
    }
    for rel in files {
        let _ = move_in(&from.join(&rel), &to.join(&rel));
    }
    let _ = std::fs::write(mark, b"");
}

fn move_in(src: &Path, dst: &Path) -> Option<()> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::AsRawHandle;
    const GENERIC_READ: u32 = 0x8000_0000;
    const DELETE: u32 = 0x1_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let mut f = std::fs::OpenOptions::new().access_mode(GENERIC_READ | DELETE).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(src).ok()?;
    let md = f.metadata().ok()?;
    if !md.is_file() || md.file_attributes() & 0x400 != 0 || md.len() > MAX_MIGRATED {
        return None;
    }
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).ok()?;
    if !dst.exists() {
        std::fs::create_dir_all(dst.parent()?).ok()?;
        std::fs::write(dst, &bytes).ok()?;
    }
    let h = HANDLE(f.as_raw_handle());
    let mut buf = vec![0u16; 1024];
    // SAFETY: valid handle owned by `f`; the buffer length is passed.
    let n = unsafe { GetFinalPathNameByHandleW(h, &mut buf, FILE_NAME_NORMALIZED) } as usize;
    let real = String::from_utf16_lossy(buf.get(..n)?);
    if !real.trim_start_matches(r"\\?\").eq_ignore_ascii_case(&src.to_string_lossy()) {
        return None; // reached through a junction or link: copied, never deleted
    }
    let del = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: the handle was opened with DELETE; the struct outlives the call.
    unsafe { SetFileInformationByHandle(h, FileDispositionInfo, (&raw const del).cast(), size_of::<FILE_DISPOSITION_INFO>() as u32) }.ok()
}

/* ---------------- the exe's folder chain ---------------- */

/// Could a program without admin swap what an elevated task starts? The exe and its folder: no
/// write of any kind by others (no file dropped next to it). Each folder above, up to the drive
/// root: nobody else can rename or replace what is on the path. Drive paths only.
pub(crate) fn chain_safe(exe: &Path, acl: impl Fn(&Path) -> Option<(String, Option<Vec<Ace>>)>) -> bool {
    let Some(Component::Prefix(p)) = exe.components().next() else {
        return false;
    };
    if !matches!(p.kind(), Prefix::Disk(_)) {
        return false;
    }
    let root: PathBuf = exe.components().take(2).collect();
    let Some(chain) = antivirus::chain_under(exe, &[root]) else {
        return false;
    };
    let last = chain.len() - 1;
    chain.iter().enumerate().all(|(i, p)| {
        let rights = match i {
            0 | 1 => antivirus::WRITE_RIGHTS,
            _ if i == last => SWAP_ROOT,
            _ => SWAP,
        };
        acl(p).is_some_and(|(owner, aces)| !antivirus::grants_others(&owner, aces.as_deref(), rights))
    })
}

/// [`chain_safe`] with the real permissions of `exe` and its folders.
pub fn exe_chain_safe(exe: &Path) -> bool {
    chain_safe(&antivirus::plain(exe), antivirus::read_acl)
}

/* ---------------- install folder ---------------- */

/// The installer's folder holds exactly these (lowercase): anything else means it is not only ours.
const INSTALL_FILES: [&str; 2] = ["g2 boost.exe", "uninstall.exe"];

/// Why `dir` must not get the install DACL (`None` = it may). `entries`: (lowercase name, is a
/// plain file); `forbidden`: Windows and the profiles.
pub(crate) fn refuse_install_dir(dir: &Path, entries: &[(String, bool)], forbidden: &[PathBuf]) -> Option<&'static str> {
    if !matches!(dir.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_))) {
        return Some("not_local");
    }
    if dir.components().count() < 3 {
        return Some("drive_root");
    }
    let l = |p: &Path| p.to_string_lossy().to_ascii_lowercase();
    if forbidden.iter().any(|f| starts_with_dir(&l(dir), &l(f)) || starts_with_dir(&l(f), &l(dir))) {
        return Some("system_or_profile");
    }
    let mut names: Vec<&str> = entries.iter().filter(|(_, file)| *file).map(|(n, _)| n.as_str()).collect();
    names.sort_unstable();
    (entries.len() != INSTALL_FILES.len() || names != INSTALL_FILES).then_some("other_files")
}

/// Windows, the profiles root and this user's profile, from the shell.
fn forbidden() -> Vec<PathBuf> {
    use windows::Win32::UI::Shell::{FOLDERID_Profile, FOLDERID_UserProfiles, FOLDERID_Windows};
    [FOLDERID_Windows, FOLDERID_UserProfiles, FOLDERID_Profile].iter().filter_map(known_folder).collect()
}

/// Elevated, installed copy: give its folder the install DACL when others can write it. Returns
/// what happened (`safe`, `protected`, or why not), for the log line.
pub fn protect_install_dir() -> &'static str {
    if !crate::system::is_elevated() {
        return "not_elevated";
    }
    let Some(exe) = std::env::current_exe().ok().map(|p| antivirus::plain(&p)) else {
        return "unknown_exe";
    };
    if crate::selfupdate::install_kind(&exe) != "installed" {
        return "not_installed"; // portable and dev copies are never touched
    }
    let Some(dir) = exe.parent() else {
        return "unknown_exe";
    };
    let Ok(rd) = std::fs::read_dir(dir) else {
        return "unreadable";
    };
    let entries: Vec<(String, bool, PathBuf)> = rd
        .flatten()
        .take(64)
        .map(|e| {
            let plain_file = e.file_type().is_ok_and(|t| t.is_file()) && std::fs::symlink_metadata(e.path()).is_ok_and(|m| !is_reparse(&m));
            (e.file_name().to_string_lossy().to_ascii_lowercase(), plain_file, e.path())
        })
        .collect();
    let locked = || !antivirus::writable(dir) && entries.iter().all(|(_, _, p)| !antivirus::writable(p));
    if locked() {
        return "safe";
    }
    let names: Vec<(String, bool)> = entries.iter().map(|(n, f, _)| (n.clone(), *f)).collect();
    let dir_is_link = std::fs::symlink_metadata(dir).map_or(true, |m| is_reparse(&m));
    if let Some(why) = refuse_install_dir(dir, &names, &forbidden()).or(dir_is_link.then_some("junction")) {
        return why;
    }
    if set_security(dir, INSTALL_SDDL).is_err() {
        return "failed";
    }
    if locked() { "protected" } else { "failed" }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TI: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";

    fn ace(sid: &str, mask: u32, inherit_only: bool) -> Ace {
        Ace { sid: sid.into(), mask, inherit_only }
    }

    fn sandbox(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("g2boost-protect-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// An elevated run gets the protected store or nothing, never the user's data dir; a store
    /// folder another program made first is set aside unread.
    #[test]
    fn elevated_store_is_protected_or_refused_never_the_data_dir() {
        let root = sandbox("store");
        let store = root.join(r"ProgramData\G2 Boost");
        // Planted first by a program of the user, with a forged backup inside.
        std::fs::create_dir_all(store.join(r"secure\backups")).unwrap();
        std::fs::write(store.join(r"secure\backups\svc_x.json"), b"{}").unwrap();
        match prepare(&store) {
            Ok(dir) => {
                assert!(crate::system::is_elevated());
                assert_eq!(dir, store.join("secure"));
                assert!(trusted_dir(&store) && trusted_dir(&dir));
                assert!(!dir.join(r"backups\svc_x.json").exists(), "the planted folder was set aside");
                let (owner, aces) = antivirus::read_acl(&dir).unwrap();
                assert_eq!(owner, "S-1-5-32-544");
                assert!(aces.unwrap().iter().all(|a| ["S-1-5-18", "S-1-5-32-544"].contains(&a.sid.as_str())), "no Users, no inheritance");
            }
            Err(_) => assert!(!crate::system::is_elevated(), "only a standard user cannot make it"),
        }
        let aside: Vec<_> = std::fs::read_dir(store.parent().unwrap()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".untrusted-")).collect();
        assert_eq!(aside.len(), 1, "the user's folder is renamed aside, never read or deleted");
        assert!(aside[0].path().join(r"secure\backups\svc_x.json").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn migration_moves_once_and_never_deletes_through_a_junction() {
        let root = sandbox("migrate");
        let (data, store, elsewhere) = (root.join("data"), root.join("store"), root.join("elsewhere"));
        std::fs::create_dir_all(data.join("backups")).unwrap();
        std::fs::create_dir_all(&store).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(data.join(r"backups\svc_x.json"), b"tweak").unwrap();
        std::fs::write(data.join(r"backups\notes.txt"), b"not ours").unwrap();
        std::fs::write(data.join("dns_backup.json"), b"dns").unwrap();
        std::fs::write(elsewhere.join("1700000000-0.json"), b"key").unwrap();
        crate::cleanup::tests::make_junction(&data.join("uninstall_backups"), &elsewhere);
        migrate(&data, &store);
        assert_eq!(std::fs::read(store.join(r"backups\svc_x.json")).unwrap(), b"tweak");
        assert_eq!(std::fs::read(store.join("dns_backup.json")).unwrap(), b"dns");
        assert!(!data.join(r"backups\svc_x.json").exists() && !data.join("dns_backup.json").exists(), "moved");
        assert!(data.join(r"backups\notes.txt").exists() && !store.join(r"backups\notes.txt").exists(), "only our names");
        assert!(store.join(r"uninstall_backups\1700000000-0.json").exists(), "copied through the junction");
        assert!(elsewhere.join("1700000000-0.json").exists(), "but never deleted behind it");
        // Once: a file that shows up later in the data dir stays there.
        std::fs::write(data.join("uninstall_undo.json"), b"late").unwrap();
        migrate(&data, &store);
        assert!(!store.join("uninstall_undo.json").exists() && data.join("uninstall_undo.json").exists());
        std::fs::remove_dir(data.join("uninstall_backups")).unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn chain_rule_from_real_acl_shapes() {
        let pf_dir = vec![ace("S-1-5-18", 0x1f01ff, false), ace("S-1-5-32-544", 0x1f01ff, false), ace("S-1-5-32-545", 0x1200a9, false)];
        // C:\ and G:\ on this PC (2026-11-21): Authenticated Users may add folders to C:\ and
        // modify G:\ itself (delete included, meaningless on a root), neither may delete children.
        let c_root = vec![ace("S-1-5-32-544", 0x1f01ff, false), ace("S-1-5-11", 0x1301bf, true), ace("S-1-5-11", 0x4, false), ace("S-1-5-32-545", 0x1200a9, false)];
        let g_root = vec![ace("S-1-5-32-544", 0x1f01ff, false), ace("S-1-5-11", 0x1301bf, false), ace("S-1-5-11", 0x1301bf, true), ace("S-1-5-32-545", 0x1200a9, false)];
        // G:\G2 Boost as the installer left it: Authenticated Users' modify inherited from G:\.
        let inherited = vec![ace("S-1-5-32-544", 0x1f01ff, false), ace("S-1-5-11", 0x1301bf, false)];
        let acl = |map: Vec<(&'static str, &'static str, Vec<Ace>)>| {
            move |p: &Path| map.iter().find(|(k, ..)| Path::new(k) == p).map(|(_, o, a)| (o.to_string(), Some(a.clone())))
        };
        let pf = acl(vec![
            (r"C:\Program Files\G2 Boost\G2 Boost.exe", "S-1-5-32-544", pf_dir.clone()),
            (r"C:\Program Files\G2 Boost", "S-1-5-32-544", pf_dir.clone()),
            (r"C:\Program Files", TI, pf_dir.clone()),
            (r"C:\", TI, c_root),
        ]);
        assert!(chain_safe(Path::new(r"C:\Program Files\G2 Boost\G2 Boost.exe"), &pf));
        assert!(!chain_safe(Path::new(r"C:\Program Files\Other\G2 Boost.exe"), &pf), "unreadable = unsafe");
        let g = |dir: Vec<Ace>| {
            acl(vec![(r"G:\G2 Boost\G2 Boost.exe", "S-1-5-32-544", dir.clone()), (r"G:\G2 Boost", "S-1-5-32-544", dir), (r"G:\", "S-1-5-18", g_root.clone())])
        };
        assert!(!chain_safe(Path::new(r"G:\G2 Boost\G2 Boost.exe"), g(inherited.clone())), "as installed");
        assert!(chain_safe(Path::new(r"G:\G2 Boost\G2 Boost.exe"), g(pf_dir.clone())), "after the install DACL");
        // A folder in between that others may rename (delete inherited from G:\).
        let deep = acl(vec![
            (r"G:\Apps\G2 Boost\G2 Boost.exe", "S-1-5-32-544", pf_dir.clone()),
            (r"G:\Apps\G2 Boost", "S-1-5-32-544", pf_dir.clone()),
            (r"G:\Apps", "S-1-5-32-544", inherited),
            (r"G:\", "S-1-5-18", g_root.clone()),
        ]);
        assert!(!chain_safe(Path::new(r"G:\Apps\G2 Boost\G2 Boost.exe"), deep));
        let owned = acl(vec![(r"G:\G2 Boost\G2 Boost.exe", "S-1-5-21-1-2-3-1001", pf_dir.clone()), (r"G:\G2 Boost", "S-1-5-32-544", pf_dir.clone()), (r"G:\", "S-1-5-18", g_root)]);
        assert!(!chain_safe(Path::new(r"G:\G2 Boost\G2 Boost.exe"), owned), "an exe owned by a user account");
        let any = |_: &Path| Some(("S-1-5-18".to_string(), Some(pf_dir.clone())));
        assert!(!chain_safe(Path::new(r"\\srv\share\G2 Boost.exe"), any), "a share");
        assert!(!chain_safe(Path::new(r"G2 Boost.exe"), any));
        assert!(!chain_safe(Path::new(r"C:\Program Files\..\Users\me\G2 Boost.exe"), any));
    }

    /// Read-only, no admin: real permissions.
    #[test]
    fn real_chains() {
        assert!(exe_chain_safe(&crate::repair::system32().join("notepad.exe")), "System32 is closed to users");
        let tmp = sandbox("chain");
        std::fs::write(tmp.join("G2 Boost.exe"), b"x").unwrap();
        assert!(!exe_chain_safe(&tmp.join("G2 Boost.exe")), "the user's temp folder");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_dacl_only_on_the_installers_own_folder() {
        let ok = vec![("g2 boost.exe".to_string(), true), ("uninstall.exe".to_string(), true)];
        let forbidden = [PathBuf::from(r"C:\Windows"), PathBuf::from(r"C:\Users"), PathBuf::from(r"D:\Profiles\me")];
        assert_eq!(refuse_install_dir(Path::new(r"G:\G2 Boost"), &ok, &forbidden), None);
        assert_eq!(refuse_install_dir(Path::new(r"G:\"), &ok, &forbidden), Some("drive_root"));
        assert_eq!(refuse_install_dir(Path::new(r"\\srv\share\G2 Boost"), &ok, &forbidden), Some("not_local"));
        for p in [r"C:\Windows\G2 Boost", r"c:\users\me\G2 Boost", r"D:\Profiles\me\Apps\G2 Boost", r"D:\Profiles"] {
            assert_eq!(refuse_install_dir(Path::new(p), &ok, &forbidden), Some("system_or_profile"), "{p}");
        }
        let mut more = ok.clone();
        more.push(("save.dat".into(), true));
        assert_eq!(refuse_install_dir(Path::new(r"G:\Games"), &more, &forbidden), Some("other_files"), "a folder with other things");
        let folder = vec![("g2 boost.exe".to_string(), true), ("uninstall.exe".to_string(), false)];
        assert_eq!(refuse_install_dir(Path::new(r"G:\G2 Boost"), &folder, &forbidden), Some("other_files"), "a folder named like the uninstaller");
        assert_eq!(refuse_install_dir(Path::new(r"G:\G2 Boost"), &ok[..1], &forbidden), Some("other_files"), "no uninstaller");
        // The test binary is a dev build: never touched, elevated or not.
        assert!(matches!(protect_install_dir(), "not_elevated" | "not_installed"));
    }
}
