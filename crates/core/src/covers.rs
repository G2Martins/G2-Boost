//! Cover art for the Jogos page. Steam games get their 2:3 library capsule: the Steam client's
//! own cache first, then G2 Boost's cache, then one download from Steam's CDN (fixed https hosts,
//! fetched by the shell). Every byte read from disk or the network is checked as an image (JPEG /
//! PNG magic, at most 2 MB) before it is served. Other launchers have no network source: their
//! tile shows the exe's own icon at the largest size Windows can give.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::games::InstalledGame;
use crate::registry;

/// Largest cover accepted from disk or the CDN.
pub const MAX_BYTES: usize = 2 << 20;
/// A CDN miss is not asked again for a day.
const MISS_SECS: u64 = 24 * 3600;

pub fn is_image(b: &[u8]) -> bool {
    b.len() <= MAX_BYTES && (b.starts_with(&[0xFF, 0xD8, 0xFF]) || b.starts_with(b"\x89PNG\r\n\x1a\n"))
}

pub fn content_type(b: &[u8]) -> &'static str {
    if b.starts_with(b"\x89PNG") { "image/png" } else { "image/jpeg" }
}

/// At most `MAX_BYTES + 1` bytes of `p`, kept only when they are an image.
fn read_image(p: &Path) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    std::fs::File::open(p).ok()?.take(MAX_BYTES as u64 + 1).read_to_end(&mut buf).ok()?;
    is_image(&buf).then_some(buf)
}

/// The Steam client's cached capsule for `appid` under its install folder `root`:
/// `appcache\librarycache\<appid>\library_600x900[_2x].jpg`, or the newer layout where each asset
/// sits in a folder named by its SHA-1 (`<sha1>\library_capsule.jpg`).
pub(crate) fn steam_cached(root: &Path, appid: u32) -> Option<PathBuf> {
    let dir = root.join(r"appcache\librarycache").join(appid.to_string());
    ["library_600x900_2x.jpg", "library_600x900.jpg"].iter().map(|n| dir.join(n)).find(|p| p.is_file()).or_else(|| {
        std::fs::read_dir(&dir)
            .ok()?
            .flatten()
            .filter(|e| {
                let n = e.file_name();
                let n = n.to_string_lossy();
                n.len() == 40 && n.bytes().all(|b| b.is_ascii_hexdigit())
            })
            .map(|e| e.path().join("library_capsule.jpg"))
            .find(|p| p.is_file())
    })
}

fn cache_dir() -> PathBuf {
    registry::data_dir().join("covers")
}

fn cache_file(appid: u32) -> PathBuf {
    cache_dir().join(format!("steam_{appid}.jpg"))
}

fn miss_file(appid: u32) -> PathBuf {
    cache_dir().join(format!("steam_{appid}.miss"))
}

/// A cover already on this PC: Steam's cache first, then ours.
pub fn local(appid: u32) -> Option<Vec<u8>> {
    crate::steam::steam_root()
        .filter(|r| r.is_absolute())
        .and_then(|r| steam_cached(&r, appid))
        .and_then(|p| read_image(&p))
        .or_else(|| read_image(&cache_file(appid)))
}

/// The CDN had no cover for `appid` less than a day ago (the marker file's age).
pub fn recent_miss(appid: u32) -> bool {
    std::fs::metadata(miss_file(appid))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age.as_secs() < MISS_SECS)
}

pub fn remember_miss(appid: u32) {
    let _ = std::fs::create_dir_all(cache_dir());
    let _ = std::fs::write(miss_file(appid), b"");
}

/// Keep a downloaded cover: checked again, written to a temp file, then renamed over the old one.
pub fn store(appid: u32, bytes: &[u8]) -> std::io::Result<()> {
    if !is_image(bytes) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "not an image"));
    }
    std::fs::create_dir_all(cache_dir())?;
    let tmp = cache_dir().join(format!("steam_{appid}.part"));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, cache_file(appid))?;
    let _ = std::fs::remove_file(miss_file(appid));
    Ok(())
}

/// Where a missing cover comes from, in order: Steam's CDN, fixed https hosts (600x900, then the
/// 300x450 copy on the second host).
pub fn urls(appid: u32) -> [String; 2] {
    [
        format!("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{appid}/library_600x900_2x.jpg"),
        format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/library_600x900.jpg"),
    ]
}

/// A path segment from the webview (`encodeURIComponent(id)`) back to the id: strict percent
/// decoding, valid UTF-8, bounded, no control characters. Anything else is `None`.
pub fn decode_id(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.len() > 600 {
        return None;
    }
    let b = raw.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = b.get(i + 1..i + 3).filter(|h| h.iter().all(u8::is_ascii_hexdigit))?;
            out.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    let s = String::from_utf8(out).ok()?;
    (s.len() <= 200 && !s.chars().any(char::is_control)).then_some(s)
}

/// The fallback tile's picture: the game's exe icon, up to 256 px (resources read as data).
pub fn icon_png(g: &InstalledGame) -> Option<Vec<u8>> {
    let exe = g.main_exe.as_deref().filter(|e| crate::games::plain_exe(e))?;
    crate::icons::png_sized(Path::new(exe), 256)
}

#[cfg(test)]
mod tests {
    use super::*;

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F'];

    #[test]
    fn images_only_and_bounded() {
        assert!(is_image(JPEG));
        assert!(is_image(b"\x89PNG\r\n\x1a\nrest"));
        assert!(!is_image(b"<html>404</html>"), "an error page is not a cover");
        assert!(!is_image(b""));
        assert!(!is_image(b"\x89PNG"), "truncated magic");
        let mut big = JPEG.to_vec();
        big.resize(MAX_BYTES + 1, 0);
        assert!(!is_image(&big), "over 2 MB");
        assert_eq!(content_type(JPEG), "image/jpeg");
        assert_eq!(content_type(b"\x89PNG\r\n\x1a\n"), "image/png");
    }

    #[test]
    fn decode_id_is_strict() {
        assert_eq!(decode_id("steam%3A730").as_deref(), Some("steam:730"));
        assert_eq!(decode_id("app%3Am64%3A%7BABC%7D").as_deref(), Some("app:m64:{ABC}"));
        assert_eq!(decode_id("exe%3Aj%C3%B3go.exe").as_deref(), Some("exe:jógo.exe"), "UTF-8");
        assert_eq!(decode_id(""), None);
        assert_eq!(decode_id("%"), None, "truncated escape");
        assert_eq!(decode_id("%4"), None);
        assert_eq!(decode_id("%zz"), None, "not hex");
        assert_eq!(decode_id("%+1"), None, "a sign is not hex");
        assert_eq!(decode_id("a%00b"), None, "control character");
        assert_eq!(decode_id("a%0Ab"), None);
        assert_eq!(decode_id("%FF%FE"), None, "not UTF-8");
        assert_eq!(decode_id(&"a".repeat(601)), None, "oversize");
        assert_eq!(decode_id(&"a".repeat(201)), None, "longer than any game id");
        assert_eq!(decode_id("..%5C..%5Cwindows").as_deref(), Some(r"..\..\windows"), "decoded as text: only a listed id is ever used");
    }

    #[test]
    fn steam_cache_layouts_in_a_sandbox() {
        let root = std::env::temp_dir().join(format!("g2boost-covers-{}", std::process::id()));
        let cache = root.join(r"appcache\librarycache");
        let _ = std::fs::remove_dir_all(&root);
        // Old layout, 2x preferred.
        std::fs::create_dir_all(cache.join("730")).unwrap();
        std::fs::write(cache.join(r"730\library_600x900.jpg"), JPEG).unwrap();
        assert!(steam_cached(&root, 730).unwrap().ends_with("library_600x900.jpg"));
        std::fs::write(cache.join(r"730\library_600x900_2x.jpg"), JPEG).unwrap();
        assert!(steam_cached(&root, 730).unwrap().ends_with("library_600x900_2x.jpg"));
        // New layout: `<sha1>\library_capsule.jpg`; other folder names are ignored.
        let sha = "81210d536b70c14e4cf9725f3f03d82bf64e5e25";
        std::fs::create_dir_all(cache.join(format!(r"4080220\{sha}"))).unwrap();
        std::fs::create_dir_all(cache.join(r"4080220\not-a-hash")).unwrap();
        std::fs::write(cache.join(r"4080220\not-a-hash\library_capsule.jpg"), JPEG).unwrap();
        assert_eq!(steam_cached(&root, 4080220), None, "only SHA-1 folders");
        std::fs::write(cache.join(format!(r"4080220\{sha}\library_capsule.jpg")), JPEG).unwrap();
        assert!(steam_cached(&root, 4080220).unwrap().to_string_lossy().contains(sha));
        assert_eq!(steam_cached(&root, 1), None);
        // Whatever is there must still be an image.
        std::fs::write(cache.join(r"730\library_600x900_2x.jpg"), b"<html>").unwrap();
        assert_eq!(read_image(&steam_cached(&root, 730).unwrap()), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn store_miss_and_reread() {
        // SAFETY: same override every data-dir test in this crate uses.
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
        let id = 4_000_000_001;
        let _ = std::fs::remove_file(cache_file(id));
        assert!(store(id, b"<html>not found</html>").is_err(), "only images are kept");
        remember_miss(id);
        assert!(recent_miss(id));
        store(id, JPEG).unwrap();
        assert!(!recent_miss(id), "a stored cover clears the miss");
        assert_eq!(read_image(&cache_file(id)).as_deref(), Some(JPEG));
        assert!(!cache_dir().join(format!("steam_{id}.part")).exists(), "temp file renamed");
        let _ = std::fs::remove_file(cache_file(id));
    }

    #[test]
    fn cdn_urls_are_fixed_https_hosts() {
        for u in urls(730) {
            assert!(u.starts_with("https://shared.akamai.steamstatic.com/") || u.starts_with("https://cdn.cloudflare.steamstatic.com/"));
            assert!(u.contains("/730/"));
        }
    }

    /// Real download path on this machine is the shell's (reqwest); this checks the local Steam
    /// cache instead (`cargo test -p g2boost-core -- covers --ignored --nocapture`).
    #[test]
    #[ignore]
    fn local_covers_live() {
        for g in crate::games::installed(&[]) {
            if let Some(a) = crate::games::steam_appid(&g.id) {
                println!("{a:>8} {:<40} local={:?}", g.name, local(a).map(|b| b.len()));
            }
        }
    }
}
