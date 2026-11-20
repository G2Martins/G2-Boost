//! Program icons as PNG data URLs for the UI: `ExtractIconExW` on an exe/dll/ico resource,
//! encoded by WIC (Windows Imaging Component). Nothing is executed — resources are read as data.

use std::path::{Path, PathBuf};

use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA, IWICImagingFactory,
    WICBitmapEncoderNoCache,
};
use windows::Win32::Foundation::S_OK;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, STATFLAG_NONAME, STREAM_SEEK_SET};
use windows::Win32::UI::Shell::{ExtractIconExW, SHCreateMemStream, SHDefExtractIconW};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, HICON};
use windows::core::PCWSTR;

use crate::registry::wide;

const EXTS: &[&str] = &["exe", "dll", "ico"];

/// `"C:\x\app.exe",0` / `C:\x\app.exe,-101` / `C:\x\app.ico` → (path, index).
pub(crate) fn parse_source(raw: &str) -> Option<(PathBuf, i32)> {
    let s = raw.trim().trim_matches('"');
    let (path, index) = match s.rsplit_once(',') {
        Some((p, i)) if i.trim().parse::<i32>().is_ok() => (p.trim().trim_matches('"'), i.trim().parse().unwrap_or(0)),
        _ => (s, 0),
    };
    let p = PathBuf::from(path);
    let ok = p.is_absolute() && p.extension().is_some_and(|e| EXTS.iter().any(|x| e.eq_ignore_ascii_case(x)));
    ok.then_some((p, index))
}

/// The large icon at `index` of `path`, as PNG bytes.
pub fn png_from_resource(path: &Path, index: i32) -> Option<Vec<u8>> {
    if !path.is_file() {
        return None;
    }
    let w = wide(&path.to_string_lossy());
    let mut large = HICON::default();
    // SAFETY: one icon requested into `large`; destroyed below.
    let n = unsafe { ExtractIconExW(PCWSTR(w.as_ptr()), index, Some(&mut large), None, 1) };
    if n == 0 || large.is_invalid() {
        return None;
    }
    let png = encode(large);
    // SAFETY: icon returned by ExtractIconExW, destroyed once.
    let _ = unsafe { DestroyIcon(large) };
    png
}

/// The first icon of `path` at `size` px (Windows picks the closest image in the file and
/// scales it), as PNG bytes. For big tiles; `png_from_resource` gives the 32 px one.
pub fn png_sized(path: &Path, size: u32) -> Option<Vec<u8>> {
    if !path.is_file() {
        return None;
    }
    let w = wide(&path.to_string_lossy());
    let mut icon = HICON::default();
    // SAFETY: one large icon requested into `icon`; destroyed below.
    let hr = unsafe { SHDefExtractIconW(PCWSTR(w.as_ptr()), 0, 0, Some(&mut icon), None, size.min(256)) };
    if hr != S_OK || icon.is_invalid() {
        return None;
    }
    let png = encode(icon);
    // SAFETY: icon returned by SHDefExtractIconW, destroyed once.
    let _ = unsafe { DestroyIcon(icon) };
    png
}

fn encode(icon: HICON) -> Option<Vec<u8>> {
    let _com = crate::tasksched::Com::init().ok()?;
    // SAFETY: standard WIC sequence (bitmap from HICON → PNG encoder → memory stream); COM objects drop.
    unsafe {
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let bitmap = factory.CreateBitmapFromHICON(icon).ok()?;
        let (mut width, mut height) = (0u32, 0u32);
        bitmap.GetSize(&mut width, &mut height).ok()?;
        let stream = SHCreateMemStream(None)?;
        let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null()).ok()?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache).ok()?;
        let (mut frame, mut props) = (None, None);
        encoder.CreateNewFrame(&mut frame, &mut props).ok()?;
        let frame = frame?;
        frame.Initialize(props.as_ref()).ok()?;
        frame.SetSize(width, height).ok()?;
        let mut format = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut format).ok()?;
        frame.WriteSource(&bitmap, std::ptr::null()).ok()?;
        frame.Commit().ok()?;
        encoder.Commit().ok()?;
        let mut stat = Default::default();
        stream.Stat(&mut stat, STATFLAG_NONAME).ok()?;
        let len = usize::try_from(stat.cbSize).ok()?;
        if len == 0 || len > 4 << 20 {
            return None;
        }
        stream.Seek(0, STREAM_SEEK_SET, None).ok()?;
        let mut buf = vec![0u8; len];
        let mut read = 0u32;
        stream.Read(buf.as_mut_ptr().cast(), len as u32, Some(&mut read)).ok().ok()?;
        buf.truncate(read as usize);
        Some(buf)
    }
}

/// A Store package's logo: the manifest path, or its `.scale-100` / `.targetsize-*` variant.
pub fn store_logo(declared: &Path) -> Option<Vec<u8>> {
    let pick = if declared.is_file() {
        Some(declared.to_path_buf())
    } else {
        let stem = declared.file_stem()?.to_string_lossy().to_ascii_lowercase();
        let mut found: Vec<PathBuf> = std::fs::read_dir(declared.parent()?)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let n = p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                n.starts_with(&format!("{stem}.")) && n.ends_with(".png") && !n.contains("contrast")
            })
            .collect();
        // Prefer a 100% scale, then the smallest file (a small tile reads best at 24 px).
        found.sort_by_key(|p| (!p.to_string_lossy().to_ascii_lowercase().contains("scale-100"), std::fs::metadata(p).map(|m| m.len()).unwrap_or(u64::MAX)));
        found.into_iter().next()
    }?;
    let bytes = std::fs::read(pick).ok()?;
    (bytes.starts_with(b"\x89PNG") && bytes.len() < 4 << 20).then_some(bytes)
}

/* ---------------- icons by path (the path may come from the webview: hostile) ---------------- */

/// `C:\…` on a local drive (never UNC, `\\?\`/`\\.\` device paths or a mapped network drive):
/// reading an icon must never reach a share, which would send the user's credentials.
fn local_drive_path(s: &str) -> bool {
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    let b = s.as_bytes();
    if s.len() > 520 || b.len() < 4 || !b[0].is_ascii_alphabetic() || b[1] != b':' || b[2] != b'\\' || s.contains('/') {
        return false;
    }
    let root = wide(&s[..3]);
    // SAFETY: NUL-terminated root path; 4 = DRIVE_REMOTE, 1 = DRIVE_NO_ROOT_DIR.
    let kind = unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) };
    kind != 4 && kind != 1
}

fn reparse(md: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    md.file_attributes() & 0x400 != 0
}

/// A local `.exe` that is a plain file (a symlink or other reparse point is refused: it could
/// lead anywhere, a share included).
pub fn local_exe(path: &str) -> Option<PathBuf> {
    if !local_drive_path(path) || !path.to_ascii_lowercase().ends_with(".exe") {
        return None;
    }
    let md = std::fs::symlink_metadata(path).ok()?;
    (md.is_file() && !reparse(&md)).then(|| PathBuf::from(path))
}

/// Helpers whose icon says nothing about the app (uninstallers, updaters, crash reporters).
fn main_exe(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.ends_with(".exe")
        && !["unins", "uninst", "crash", "setup", "updater", "update.exe", "helper", "elevat"].iter().any(|k| n.contains(k))
}

/// The biggest program inside `dir`, three levels down at most and a few thousand entries, never
/// through a junction or symlink: the icon of a leftover or orphan folder.
pub fn largest_exe(dir: &Path) -> Option<PathBuf> {
    let md = std::fs::symlink_metadata(dir).ok()?;
    if !md.is_dir() || reparse(&md) || !local_drive_path(&dir.to_string_lossy()) {
        return None;
    }
    let mut best: Option<(u64, PathBuf)> = None;
    let (mut stack, mut seen) = (vec![(dir.to_path_buf(), 0u8)], 0usize);
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            seen += 1;
            if seen > 4000 {
                return best.map(|b| b.1); // ponytail: a huge folder gets the best of what was seen
            }
            // DirEntry metadata on Windows is the link's own (never followed).
            let Ok(md) = e.metadata() else {
                continue;
            };
            if reparse(&md) {
                continue;
            }
            if md.is_dir() {
                if depth < 3 {
                    stack.push((e.path(), depth + 1));
                }
            } else if main_exe(&e.file_name().to_string_lossy()) && best.as_ref().is_none_or(|b| md.len() > b.0) {
                best = Some((md.len(), e.path()));
            }
        }
    }
    best.map(|b| b.1)
}

/// `…\WindowsApps\<package full name>\…` → the package full name.
fn package_of(path: &str) -> Option<&str> {
    let i = path.to_ascii_lowercase().find(r"\windowsapps\")? + r"\windowsapps\".len();
    path[i..].split('\\').next().filter(|s| !s.is_empty())
}

/// A program's icon as a data URL: its own icon resource, or its Store package logo when the
/// program lives in `WindowsApps` and has none. `None` for anything that is not a local exe.
pub fn exe_data_url(path: &str) -> Option<String> {
    let p = local_exe(path)?;
    let png = png_from_resource(&p, 0).or_else(|| package_of(path).and_then(crate::apps::package_logo))?;
    Some(data_url(&png))
}

/// `data:image/png;base64,…`
pub fn data_url(png: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(22 + png.len().div_ceil(3) * 4);
    s.push_str("data:image/png;base64,");
    for c in png.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_and_base64() {
        assert_eq!(parse_source(r#""C:\x\app.exe",0"#), Some((PathBuf::from(r"C:\x\app.exe"), 0)));
        assert_eq!(parse_source(r"C:\x\app.dll,-101"), Some((PathBuf::from(r"C:\x\app.dll"), -101)));
        assert_eq!(parse_source(r"C:\x\a.ico"), Some((PathBuf::from(r"C:\x\a.ico"), 0)));
        assert_eq!(parse_source("app.exe"), None, "relative");
        assert_eq!(parse_source(r"C:\x\script.ps1"), None, "not an icon container");
        assert_eq!(data_url(b"Man"), "data:image/png;base64,TWFu");
        assert_eq!(data_url(b"Ma"), "data:image/png;base64,TWE=");
        assert_eq!(data_url(b"M"), "data:image/png;base64,TQ==");
    }

    #[test]
    fn extracts_a_real_icon_as_png() {
        let win = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let png = png_from_resource(&PathBuf::from(&win).join("explorer.exe"), 0).expect("explorer icon");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(png_from_resource(Path::new(r"C:\nope\x.exe"), 0).is_none());
        let big = png_sized(&PathBuf::from(win).join("explorer.exe"), 256).expect("big explorer icon");
        assert!(big.starts_with(b"\x89PNG\r\n\x1a\n") && big.len() > png.len(), "256 px is bigger than 32 px");
        assert!(png_sized(Path::new(r"C:\nope\x.exe"), 256).is_none());
    }

    #[test]
    fn exe_paths_from_outside_are_local_files_only() {
        let win = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let explorer = format!(r"{win}\explorer.exe");
        assert!(local_exe(&explorer).is_some());
        assert!(exe_data_url(&explorer).is_some_and(|u| u.starts_with("data:image/png;base64,")));
        for bad in [
            r"\\server\share\app.exe",
            r"\\?\C:\Windows\explorer.exe",
            r"\\.\C:\Windows\explorer.exe",
            "//server/share/app.exe",
            "explorer.exe",
            r"C:\Windows\win.ini",
            r"C:/Windows/explorer.exe",
            r"C:\nope\nope.exe",
            &win,
        ] {
            assert!(local_exe(bad).is_none(), "{bad}");
            assert!(exe_data_url(bad).is_none(), "{bad}");
        }
        assert_eq!(package_of(r"C:\Program Files\WindowsApps\Foo.Bar_1.0.0.0_x64__abc\app\x.exe"), Some("Foo.Bar_1.0.0.0_x64__abc"));
        assert_eq!(package_of(r"C:\Program Files\App\x.exe"), None);
    }

    /// Sandbox folder: the biggest real program wins; uninstallers, junctions and symlinks never do.
    #[test]
    fn largest_exe_never_crosses_a_link() {
        let root = std::env::temp_dir().join(format!("g2boost-icons-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (app, outside) = (root.join("App"), root.join("Outside"));
        for d in [app.join(r"bin\x64"), app.join(r"a\b\c\d"), outside.clone()] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(app.join("small.exe"), [0u8; 10]).unwrap();
        std::fs::write(app.join(r"bin\x64\game.exe"), [0u8; 200]).unwrap();
        std::fs::write(app.join("unins000.exe"), [0u8; 5000]).unwrap();
        std::fs::write(app.join(r"a\b\c\d\deep.exe"), [0u8; 9000]).unwrap(); // past the depth limit
        std::fs::write(outside.join("huge.exe"), [0u8; 20000]).unwrap();
        crate::cleanup::tests::make_junction(&app.join("linked"), &outside);
        // A file symlink needs a privilege (or developer mode); checked when it can be made.
        let sym = std::os::windows::fs::symlink_file(outside.join("huge.exe"), app.join("sym.exe")).is_ok();
        assert_eq!(largest_exe(&app), Some(app.join(r"bin\x64\game.exe")));
        assert_eq!(largest_exe(&app.join("linked")), None, "the junction itself is never walked");
        if sym {
            assert!(local_exe(&app.join("sym.exe").to_string_lossy()).is_none(), "a symlinked exe is refused");
        }
        assert!(largest_exe(Path::new(r"\\server\share")).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
