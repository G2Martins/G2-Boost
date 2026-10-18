//! Program icons as PNG data URLs for the UI: `ExtractIconExW` on an exe/dll/ico resource,
//! encoded by WIC (Windows Imaging Component). Nothing is executed — resources are read as data.

use std::path::{Path, PathBuf};

use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA, IWICImagingFactory,
    WICBitmapEncoderNoCache,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, STATFLAG_NONAME, STREAM_SEEK_SET};
use windows::Win32::UI::Shell::{ExtractIconExW, SHCreateMemStream};
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
        let png = png_from_resource(&PathBuf::from(win).join("explorer.exe"), 0).expect("explorer icon");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(png_from_resource(Path::new(r"C:\nope\x.exe"), 0).is_none());
    }
}
