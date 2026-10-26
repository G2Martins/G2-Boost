//! G2 Boost's own updates from its GitHub releases. The shell does the HTTP (hosts fixed here);
//! this module only decides: which release is newer, which asset to use, and whether the bytes
//! match the `SHA256SUMS.txt` published in the same release. The release JSON is hostile input:
//! every URL must point at this repository on GitHub, sizes are capped.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const LATEST_URL: &str = "https://api.github.com/repos/G2Martins/G2-Boost/releases/latest";
const RELEASE_PAGE: &str = "https://github.com/G2Martins/G2-Boost/releases/";
const DOWNLOAD: &str = "https://github.com/G2Martins/G2-Boost/releases/download/";
/// Where GitHub redirects release downloads (and the API itself).
pub const HOSTS: &[&str] =
    &["api.github.com", "github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"];
pub const MAX_JSON: usize = 1024 * 1024;
pub const MAX_SUMS: usize = 64 * 1024;
pub const MAX_INSTALLER: usize = 128 * 1024 * 1024;
const MAX_NOTES: usize = 6000;

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    body: String,
    html_url: String,
    #[serde(default)]
    published_at: String,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    size: u64,
    browser_download_url: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Asset {
    pub name: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Release {
    pub version: String,
    pub name: String,
    pub notes: String,
    pub url: String,
    pub published_at: String,
    pub assets: Vec<Asset>,
}

/// `v1.2.3` / `1.2.3` → (1, 2, 3). Pre-release suffixes are not published, so they don't parse.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let mut it = s.strip_prefix('v').unwrap_or(s).split('.');
    let n = |p: Option<&str>| p.filter(|p| !p.is_empty() && p.len() <= 6 && p.bytes().all(|b| b.is_ascii_digit()))?.parse().ok();
    let v = (n(it.next())?, n(it.next())?, n(it.next())?);
    it.next().is_none().then_some(v)
}

pub fn newer(current: &str, latest: &str) -> bool {
    matches!((parse_version(current), parse_version(latest)), (Some(c), Some(l)) if l > c)
}

/// The GitHub "latest release" JSON, validated: a plain version tag, links only into this
/// repository, notes bounded and without control characters.
pub fn parse_release(json: &[u8]) -> Option<Release> {
    let r: RawRelease = serde_json::from_slice(json).ok()?;
    let (a, b, c) = parse_version(&r.tag_name)?;
    if !r.html_url.starts_with(RELEASE_PAGE) {
        return None;
    }
    let assets = r
        .assets
        .into_iter()
        .filter(|x| x.browser_download_url.starts_with(DOWNLOAD) && safe_name(&x.name))
        .map(|x| Asset { name: x.name, size: x.size, url: x.browser_download_url })
        .collect();
    let notes: String = r.body.chars().filter(|c| !c.is_control() || *c == '\n').take(MAX_NOTES).collect();
    Some(Release {
        version: format!("{a}.{b}.{c}"),
        name: r.name.chars().filter(|c| !c.is_control()).take(120).collect(),
        notes,
        url: r.html_url,
        published_at: r.published_at.chars().take(40).collect(),
        assets,
    })
}

/// Asset names we ever download: letters, digits, `._-`, no path pieces.
fn safe_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 120 && n.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) && !n.contains("..")
}

/// The NSIS installer of `version` (the update path for installed copies).
pub fn installer(r: &Release) -> Option<&Asset> {
    let want = format!("G2-Boost_{}_x64-setup.exe", r.version);
    r.assets.iter().find(|a| a.name == want && a.size as usize <= MAX_INSTALLER)
}

pub fn sums(r: &Release) -> Option<&Asset> {
    r.assets.iter().find(|a| a.name == "SHA256SUMS.txt" && a.size as usize <= MAX_SUMS)
}

/// The hash listed for `name` in a `sha256sum`-style file (`<64 hex>  <name>`).
pub fn expected_hash(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hash, file) = l.trim().split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// How this copy runs: `installed` (Program Files, the installer can replace it), `portable`
/// (portable.txt next to the exe: the user replaces the folder) or `dev` (a build folder).
pub fn install_kind(exe: &Path) -> &'static str {
    let dir = exe.parent();
    if dir.is_some_and(|d| d.join("portable.txt").is_file()) {
        return "portable";
    }
    let pf = std::env::var_os("ProgramFiles").map(std::path::PathBuf::from);
    match (pf, dir) {
        (Some(pf), Some(d)) if d.starts_with(&pf) => "installed",
        _ => "dev",
    }
}

/// Write the verified installer into `dir` and start it (it asks for administrator itself).
/// The file stays open, shared for reading only, until the installer process exists: nothing can
/// swap it between the hash check and the start. Documented exception to "no spawning"
/// (global-rules §1.3): our own installer, hash-checked, started only by a click.
pub fn run_installer(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::UI::Shell::{SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::PCWSTR;
    if !safe_name(name) || !name.ends_with(".exe") {
        return Err(std::io::Error::other("bad_name"));
    }
    std::fs::create_dir_all(dir)?;
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path); // an older download of the same version
    const FILE_SHARE_READ: u32 = 1;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).share_mode(FILE_SHARE_READ).open(&path)?;
    f.write_all(bytes)?;
    f.flush()?;
    let file = crate::registry::wide(&path.to_string_lossy());
    let verb = crate::registry::wide("open");
    // SAFETY: zeroed struct with cbSize; the strings outlive the call.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOASYNC;
        info.lpVerb = PCWSTR(verb.as_ptr());
        info.lpFile = PCWSTR(file.as_ptr());
        info.nShow = SW_SHOWNORMAL.0;
        ShellExecuteExW(&mut info).map_err(|e| std::io::Error::other(e.to_string()))?;
    }
    drop(f);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{
        "tag_name": "v0.9.3", "name": "G2 Boost 0.9.3", "body": "Correções\r\n- um\u0007",
        "html_url": "https://github.com/G2Martins/G2-Boost/releases/tag/v0.9.3",
        "published_at": "2026-10-26T03:00:00Z",
        "assets": [
            {"name": "G2-Boost_0.9.3_x64-setup.exe", "size": 7000000,
             "browser_download_url": "https://github.com/G2Martins/G2-Boost/releases/download/v0.9.3/G2-Boost_0.9.3_x64-setup.exe"},
            {"name": "SHA256SUMS.txt", "size": 383,
             "browser_download_url": "https://github.com/G2Martins/G2-Boost/releases/download/v0.9.3/SHA256SUMS.txt"},
            {"name": "evil.exe", "size": 1, "browser_download_url": "https://evil.example/evil.exe"},
            {"name": "..\\x.exe", "size": 1, "browser_download_url": "https://github.com/G2Martins/G2-Boost/releases/download/v0.9.3/x.exe"}
        ]}"#;

    #[test]
    fn versions_compare_numerically() {
        assert!(newer("0.9.2", "0.9.3"));
        assert!(newer("0.9.9", "0.10.0"));
        assert!(newer("0.9.2", "v1.0.0"));
        assert!(!newer("0.9.2", "0.9.2"));
        assert!(!newer("0.10.0", "0.9.9"));
        for bad in ["", "v", "1.2", "1.2.3.4", "1.2.x", "1.2.3-beta", "01234567.0.0", " 1.2.3"] {
            assert!(parse_version(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn release_json_is_validated() {
        let r = parse_release(JSON.as_bytes()).unwrap();
        assert_eq!(r.version, "0.9.3");
        assert_eq!(r.assets.len(), 2, "foreign host and path pieces dropped: {:?}", r.assets);
        assert!(!r.notes.contains('\u{7}'));
        assert_eq!(installer(&r).unwrap().name, "G2-Boost_0.9.3_x64-setup.exe");
        assert!(sums(&r).is_some());
        // A release page outside this repository is refused outright.
        let other = JSON.replace("https://github.com/G2Martins/G2-Boost/releases/tag", "https://github.com/evil/x/releases/tag");
        assert!(parse_release(other.as_bytes()).is_none());
        assert!(parse_release(b"not json").is_none());
        assert!(parse_release(JSON.replace("v0.9.3\"", "latest\"").as_bytes()).is_none());
    }

    #[test]
    fn hashes_come_from_the_sums_file() {
        let sums = "2f2344242df379436d014423d0b043f476b222b8aec0c233d7672cedb1802a3f  G2-Boost_0.9.1_x64-setup.exe\n\
                    zz  bad.exe\n";
        assert_eq!(
            expected_hash(sums, "G2-Boost_0.9.1_x64-setup.exe").as_deref(),
            Some("2f2344242df379436d014423d0b043f476b222b8aec0c233d7672cedb1802a3f")
        );
        assert!(expected_hash(sums, "bad.exe").is_none());
        assert!(expected_hash(sums, "missing.exe").is_none());
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn run_installer_refuses_odd_names() {
        let dir = std::env::temp_dir();
        for bad in [r"..\x.exe", "x.msi", "", "a b.exe"] {
            assert!(run_installer(&dir, bad, b"x").is_err(), "{bad}");
        }
    }

    #[test]
    fn install_kind_reads_the_folder() {
        let dir = std::env::temp_dir().join(format!("g2b-kind-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("G2 Boost.exe");
        assert_eq!(install_kind(&exe), "dev");
        std::fs::write(dir.join("portable.txt"), "x").unwrap();
        assert_eq!(install_kind(&exe), "portable");
        std::fs::remove_dir_all(&dir).unwrap();
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            assert_eq!(install_kind(&std::path::PathBuf::from(pf).join(r"G2 Boost\G2 Boost.exe")), "installed");
        }
    }
}
