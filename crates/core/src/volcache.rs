//! "System files": Windows' own Disk Cleanup handlers (`IEmptyVolumeCache`/`IEmptyVolumeCache2`, registered under
//! `HKLM\...\Explorer\VolumeCaches`), driven in-process without `cleanmgr.exe`. Same flow as the
//! catalog: preview (`GetSpaceUsed`) → confirmation token → execute (`Purge`) → history.
//!
//! Security (the app may be elevated; a planted handler would run inside it):
//! - only the key names in [`HANDLERS`] are ever opened (callers pass ids, never key names);
//! - the CLSID is parsed as a GUID and looked up in `HKLM\SOFTWARE\Classes` (never the per-user
//!   `HKCU` view); its `InprocServer32` must resolve to the expected DLL file name directly
//!   inside `System32` (only `%SystemRoot%`/`%windir%` are expanded; canonicalised, so links out
//!   of `System32` are refused);
//! - that exact file is loaded with `LoadLibraryExW` and the object comes from its own
//!   `DllGetClassObject`, so COM's registry lookup is never trusted.
//!
//! Handlers run on a dedicated STA thread; some (Update Cleanup) take minutes. The callback
//! honours `repair::cancel` (returns `E_ABORT`). One servicing job at a time (`repair::exclusive`).

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use serde::Serialize;
use windows::Win32::Foundation::{E_ABORT, S_FALSE};
use windows::Win32::System::Com::{CoTaskMemFree, IClassFactory};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::Win32::System::Registry::{HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegOpenKeyExW};
use windows::Win32::UI::LegacyWindowsEnvironmentFeatures::{
    EMPTY_VOLUME_CACHE_FLAGS, IEmptyVolumeCache, IEmptyVolumeCache2, IEmptyVolumeCacheCallBack,
    IEmptyVolumeCacheCallBack_Impl,
};
use windows::core::{GUID, HRESULT, IUnknown, Interface, PCWSTR, PWSTR, implement, s};

use crate::cleanup::{ExecItem, Risk, RunRecord};
use crate::registry::{self, Root, Value, wide};
use crate::{repair, system, tasksched};

const VOLUME_CACHES: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VolumeCaches";

#[derive(Debug, Clone, Copy)]
pub struct Handler {
    /// Our id (`sys_*`, disjoint from the catalog ids).
    pub id: &'static str,
    /// Registry key name under `VolumeCaches` (fixed; never from a caller).
    key: &'static str,
    /// Expected `InprocServer32` file name inside `System32`.
    dll: &'static str,
    pub risk: Risk,
    pub default_on: bool,
    /// Removes a way back (rollback / uninstalling updates / older drivers): opt-in with a warning.
    pub warning: bool,
}

const fn h(id: &'static str, key: &'static str, dll: &'static str, risk: Risk, default_on: bool, warning: bool) -> Handler {
    Handler { id, key, dll, risk, default_on, warning }
}

/// Allowlist (verified on Windows 11 26200, 2026-10-22). Left out on purpose:
/// `DownloadsFolder` (the user's Downloads), `Recycle Bin`, `Temporary Files`, `Thumbnail Cache`,
/// `D3D Shader Cache`, `Delivery Optimization Files`, `Windows Error Reporting Files`,
/// `System error minidump files`, `Internet Cache Files` (the catalog already covers those paths),
/// `User file versions` (File History: user data), `Windows ESD installation files` (needed by
/// "Reset this PC"), `Language Pack`, and legacy/IE-era handlers.
pub const HANDLERS: &[Handler] = &[
    h("sys_update_cleanup", "Update Cleanup", "scavengeui.dll", Risk::Medium, false, true),
    h("sys_previous_installations", "Previous Installations", "setupcln.dll", Risk::High, false, true),
    h("sys_driver_packages", "Device Driver Packages", "pnpclean.dll", Risk::Medium, false, true),
    h("sys_upgrade_logs", "Windows Upgrade Log Files", "setupcln.dll", Risk::Low, true, false),
    h("sys_temp_setup", "Temporary Setup Files", "setupcln.dll", Risk::Low, true, false),
    h("sys_upgrade_discarded", "Upgrade Discarded Files", "setupcln.dll", Risk::Low, true, false),
    h("sys_reset_logs", "Windows Reset Log Files", "setupcln.dll", Risk::Low, true, false),
    h("sys_setup_logs", "Setup Log Files", "dataclen.dll", Risk::Low, true, false),
    h("sys_chkdsk", "Old ChkDsk Files", "dataclen.dll", Risk::Low, true, false),
    h("sys_defender", "Windows Defender", "dataclen.dll", Risk::Low, true, false),
    h("sys_memory_dump", "System error memory dump files", "dataclen.dll", Risk::Medium, false, false),
];

pub fn find(id: &str) -> Option<&'static Handler> {
    HANDLERS.iter().find(|h| h.id == id)
}

/// Known ids only, at most once each, at least one.
pub fn valid_selection(ids: &[String]) -> bool {
    !ids.is_empty()
        && ids.len() <= HANDLERS.len()
        && ids.iter().all(|i| find(i).is_some())
        && ids.iter().enumerate().all(|(k, i)| !ids[..k].contains(i))
}

/* ---------------- handler trust (pure parts tested) ---------------- */

/// `InprocServer32` value → absolute path. Expands only `%SystemRoot%` / `%windir%`; refuses any
/// other variable, relative paths, `.`/`..`, UNC and device paths.
pub(crate) fn resolve_inproc(raw: &str, windir: &Path) -> Option<PathBuf> {
    let s = raw.trim().trim_matches('"');
    let lower = s.to_ascii_lowercase();
    let expanded = if lower.starts_with("%systemroot%") {
        format!("{}{}", windir.display(), &s["%systemroot%".len()..])
    } else if lower.starts_with("%windir%") {
        format!("{}{}", windir.display(), &s["%windir%".len()..])
    } else {
        s.to_string()
    };
    // Segments, not `components()`: that one silently drops a `.` in the middle.
    let dots = expanded.split(['\\', '/']).any(|seg| seg == "." || seg == "..");
    if dots || expanded.contains('%') || expanded.starts_with(r"\\") || expanded.starts_with("//") {
        return None;
    }
    let p = PathBuf::from(expanded);
    p.is_absolute().then_some(p)
}

/// The canonical DLL sits directly in (canonical) `System32` and has the expected name.
pub(crate) fn dll_ok(dll: &Path, system32: &Path, expected: &str) -> bool {
    let same_dir = dll.parent().is_some_and(|d| d.to_string_lossy().eq_ignore_ascii_case(&system32.to_string_lossy()));
    same_dir && dll.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(expected))
}

/// `{XXXXXXXX-...}` → GUID, re-formatted by us before it goes into a registry path.
fn parse_clsid(s: &str) -> Option<GUID> {
    let inner = s.trim().strip_prefix('{')?.strip_suffix('}')?;
    GUID::try_from(inner).ok()
}

fn clsid_key(g: &GUID) -> String {
    let d = g.data4;
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        g.data1, g.data2, g.data3, d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
    )
}

/// Resolve and vet a handler: CLSID + the System32 DLL that may serve it.
fn trusted(h: &Handler) -> Result<(GUID, PathBuf), &'static str> {
    let Ok(Value::Str(raw)) = registry::read(Root::Hklm, &format!(r"{VOLUME_CACHES}\{}", h.key), "") else {
        return Err("missing");
    };
    let clsid = parse_clsid(&raw).ok_or("untrusted_handler")?;
    let server = format!(r"SOFTWARE\Classes\CLSID\{}\InprocServer32", clsid_key(&clsid));
    let Ok(Value::Str(dll)) = registry::read(Root::Hklm, &server, "") else {
        return Err("untrusted_handler");
    };
    let path = resolve_inproc(&dll, &repair::windows_dir()).ok_or("untrusted_handler")?;
    let (Ok(canon), Ok(sys)) = (std::fs::canonicalize(&path), std::fs::canonicalize(repair::system32())) else {
        return Err("untrusted_handler");
    };
    if !dll_ok(&canon, &sys, h.dll) {
        return Err("untrusted_handler");
    }
    Ok((clsid, crate::disk::strip_verbatim(&canon)))
}

/* ---------------- COM plumbing ---------------- */

/// Progress sink: remembers the bytes freed and turns a cancel into `E_ABORT`.
#[implement(IEmptyVolumeCacheCallBack)]
struct Callback {
    freed: Arc<AtomicU64>,
}

impl IEmptyVolumeCacheCallBack_Impl for Callback_Impl {
    fn ScanProgress(&self, _used: u64, _flags: u32, _status: &PCWSTR) -> windows::core::Result<()> {
        if repair::cancelled() { Err(E_ABORT.into()) } else { Ok(()) }
    }

    fn PurgeProgress(&self, freed: u64, _to_free: u64, _flags: u32, _status: &PCWSTR) -> windows::core::Result<()> {
        self.freed.store(freed, Ordering::Relaxed);
        if repair::cancelled() { Err(E_ABORT.into()) } else { Ok(()) }
    }
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: opened by RegOpenKeyExW below, closed once.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn free(p: PWSTR) {
    if !p.is_null() {
        // SAFETY: the handler allocated it with CoTaskMemAlloc (IEmptyVolumeCache contract).
        unsafe { CoTaskMemFree(Some(p.0 as *const c_void)) };
    }
}

/// The vetted DLL's own class factory: no registry lookup by COM.
fn instantiate(dll: &Path, clsid: &GUID) -> Result<IEmptyVolumeCache, String> {
    type GetClassObject = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
    let w = wide(&dll.to_string_lossy());
    // SAFETY: full path to a System32 DLL vetted by `trusted`; DllGetClassObject has the
    // documented signature; the module stays loaded for the life of the process.
    unsafe {
        let m = LoadLibraryExW(PCWSTR(w.as_ptr()), None, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32)
            .map_err(|e| format!("load 0x{:08X}", e.code().0))?;
        let f = GetProcAddress(m, s!("DllGetClassObject")).ok_or("no_class_factory")?;
        let f: GetClassObject = std::mem::transmute::<unsafe extern "system" fn() -> isize, GetClassObject>(f);
        let mut p = std::ptr::null_mut();
        f(clsid, &IClassFactory::IID, &mut p).ok().map_err(|e| format!("factory 0x{:08X}", e.code().0))?;
        let factory = IClassFactory::from_raw(p);
        let obj = factory.CreateInstance::<_, IUnknown>(None::<&IUnknown>).map_err(|e| format!("create 0x{:08X}", e.code().0))?;
        obj.cast::<IEmptyVolumeCache>().map_err(|e| format!("iface 0x{:08X}", e.code().0))
    }
}

/// What one handler reported.
struct Measured {
    /// `Initialize`/`InitializeEx` returned `S_FALSE`: nothing to clean.
    empty: bool,
    used: u64,
    freed: u64,
}

fn run_one(h: &Handler, volume: &[u16], purge: bool) -> Result<Measured, String> {
    let (clsid, dll) = trusted(h)?;
    let cache = instantiate(&dll, &clsid)?;
    let path = wide(&format!(r"{VOLUME_CACHES}\{}", h.key));
    let name = wide(h.key);
    let mut hk = HKEY::default();
    // SAFETY: fixed allowlisted key path; read-only.
    let e = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, PCWSTR(path.as_ptr()), None, KEY_READ, &mut hk) };
    if e.is_err() {
        return Err("missing".into());
    }
    let key = Key(hk);
    let (mut disp, mut desc, mut btn) = (PWSTR::null(), PWSTR::null(), PWSTR::null());
    let mut flags = EMPTY_VOLUME_CACHE_FLAGS(0);
    // Most built-in handlers only implement the original interface (verified elevated on 24H2:
    // 10 of 11 refuse `IEmptyVolumeCache2`), so `InitializeEx` when offered, else `Initialize`,
    // as Disk Cleanup itself does.
    // SAFETY: valid key handle, NUL-terminated volume/key name, out params owned here. Called
    // through the vtable to see S_FALSE ("nothing to clean"), which `.ok()` would hide.
    let hr = match cache.cast::<IEmptyVolumeCache2>() {
        Ok(v2) => unsafe {
            (Interface::vtable(&v2).InitializeEx)(
                Interface::as_raw(&v2),
                key.0,
                PCWSTR(volume.as_ptr()),
                PCWSTR(name.as_ptr()),
                &mut disp,
                &mut desc,
                &mut btn,
                &mut flags,
            )
        },
        Err(_) => unsafe {
            (Interface::vtable(&cache).Initialize)(
                Interface::as_raw(&cache),
                key.0,
                PCWSTR(volume.as_ptr()),
                &mut disp,
                &mut desc,
                &mut flags,
            )
        },
    };
    for p in [disp, desc, btn] {
        free(p);
    }
    if hr == S_FALSE {
        return Ok(Measured { empty: true, used: 0, freed: 0 });
    }
    hr.ok().map_err(|e| format!("init 0x{:08X}", e.code().0))?;
    let freed = Arc::new(AtomicU64::new(0));
    let cb: IEmptyVolumeCacheCallBack = Callback { freed: freed.clone() }.into();
    let mut used = 0u64;
    // SAFETY: initialised handler; the callback object outlives the calls.
    let res = unsafe {
        cache.GetSpaceUsed(&mut used, &cb).and_then(|()| if purge && used > 0 { cache.Purge(used, &cb) } else { Ok(()) })
    };
    // SAFETY: as above; Deactivate ends the handler's session whatever happened.
    unsafe {
        let _ = cache.Deactivate();
    }
    drop(key);
    match res {
        Err(e) if e.code() == E_ABORT => Err("cancelled".into()),
        Err(e) => Err(format!("0x{:08X}", e.code().0)),
        Ok(()) => {
            let f = freed.load(Ordering::Relaxed);
            Ok(Measured { empty: false, used, freed: if purge { if f > 0 { f } else { used } } else { 0 } })
        }
    }
}

/// `C:\` of the Windows volume, NUL-terminated.
fn system_volume() -> Vec<u16> {
    let w = repair::windows_dir();
    let root = w.to_string_lossy().chars().take(2).collect::<String>();
    wide(&format!("{root}\\"))
}

/// Run `ids` (validated) on a fresh STA thread; `on_progress(done, total)` after each one.
fn drive(ids: &[String], purge: bool, on_progress: &mut (dyn FnMut(usize, usize) + Send)) -> Vec<(&'static Handler, Result<Measured, String>)> {
    let list: Vec<&'static Handler> = ids.iter().filter_map(|i| find(i)).collect();
    std::thread::scope(|sc| {
        sc.spawn(|| {
            let com = tasksched::Com::init();
            let volume = system_volume();
            let mut out = Vec::new();
            for (i, h) in list.iter().enumerate() {
                let r = if com.is_err() {
                    Err("com".into())
                } else if repair::cancelled() {
                    Err("cancelled".into())
                } else {
                    run_one(h, &volume, purge)
                };
                out.push((*h, r));
                on_progress(i + 1, list.len());
            }
            out
        })
        .join()
        .unwrap_or_default()
    })
}

/* ---------------- preview / execute ---------------- */

#[derive(Debug, Clone, Serialize)]
pub struct VcItem {
    pub id: &'static str,
    pub bytes: u64,
    /// Handler present and reporting something (false = nothing to clean or not on this PC).
    pub available: bool,
    pub needs_admin: bool,
    /// `missing`, `untrusted_handler`, `cancelled`, `busy` or an HRESULT.
    pub error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum VcError {
    #[error("bad_selection")]
    BadSelection,
    #[error("busy")]
    Busy,
}

/// Measure (`GetSpaceUsed`) the selected handlers. Admin only: without elevation every item
/// comes back `needs_admin` and no handler is loaded.
pub fn preview(ids: &[String], mut on_progress: impl FnMut(usize, usize) + Send) -> Result<Vec<VcItem>, VcError> {
    if !valid_selection(ids) {
        return Err(VcError::BadSelection);
    }
    if !system::is_elevated() {
        let items = ids.iter().filter_map(|i| find(i)).map(|h| VcItem { id: h.id, bytes: 0, available: false, needs_admin: true, error: None });
        return Ok(items.collect());
    }
    let _slot = repair::exclusive().ok_or(VcError::Busy)?;
    Ok(drive(ids, false, &mut on_progress)
        .into_iter()
        .map(|(h, r)| match r {
            Ok(m) => VcItem { id: h.id, bytes: m.used, available: !m.empty, needs_admin: false, error: None },
            Err(e) => VcItem { id: h.id, bytes: 0, available: false, needs_admin: false, error: Some(e) },
        })
        .collect())
}

/// Purge the selected handlers (already confirmed with a token by the caller). Recorded in the
/// cleanup history like any other run.
pub fn execute(ids: &[String], trigger: &str, mut on_progress: impl FnMut(usize, usize) + Send) -> Result<RunRecord, VcError> {
    if !valid_selection(ids) {
        return Err(VcError::BadSelection);
    }
    let t0 = Instant::now();
    let items: Vec<ExecItem> = if !system::is_elevated() {
        ids.iter().map(|id| ExecItem { id: id.clone(), error: Some("needs_admin".into()), ..Default::default() }).collect()
    } else {
        let _slot = repair::exclusive().ok_or(VcError::Busy)?;
        drive(ids, true, &mut on_progress)
            .into_iter()
            .map(|(h, r)| match r {
                Ok(m) => ExecItem { id: h.id.into(), bytes_freed: m.freed, ..Default::default() },
                Err(e) => ExecItem { id: h.id.into(), error: Some(e), ..Default::default() },
            })
            .collect()
    };
    let record = RunRecord { at: registry::now_unix(), trigger: trigger.into(), duration_ms: t0.elapsed().as_millis() as u64, items };
    crate::cleanup::record_run(&record);
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_never_includes_user_data_or_catalog_overlaps() {
        for banned in ["DownloadsFolder", "Recycle Bin", "User file versions", "Windows ESD installation files", "Temporary Files"] {
            assert!(HANDLERS.iter().all(|h| h.key != banned), "{banned}");
        }
        for h in HANDLERS {
            assert!(h.id.starts_with("sys_") && crate::cleanup::find(h.id).is_none(), "{}: disjoint from the catalog", h.id);
            assert!(!(h.warning && h.default_on), "{}: warned handlers are opt-in", h.id);
            assert!(h.dll.ends_with(".dll") && !h.dll.contains(['\\', '/']));
        }
        for warned in ["sys_update_cleanup", "sys_previous_installations"] {
            assert!(find(warned).is_some_and(|h| h.warning && !h.default_on));
        }
    }

    #[test]
    fn selection_is_validated() {
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(valid_selection(&ids(&["sys_setup_logs", "sys_update_cleanup"])));
        assert!(!valid_selection(&[]));
        assert!(!valid_selection(&ids(&["temp_user"])), "catalog ids are not handlers");
        assert!(!valid_selection(&ids(&["DownloadsFolder"])), "key names are not ids");
        assert!(!valid_selection(&ids(&["sys_chkdsk", "sys_chkdsk"])));
        assert!(matches!(preview(&ids(&["../evil"]), |_, _| {}), Err(VcError::BadSelection)));
    }

    #[test]
    fn inproc_paths_outside_system32_are_refused() {
        let win = Path::new(r"C:\Windows");
        let sys = Path::new(r"C:\Windows\System32");
        let ok = |raw: &str, name: &str| resolve_inproc(raw, win).is_some_and(|p| dll_ok(&p, sys, name));
        assert!(ok(r"%SystemRoot%\System32\dataclen.dll", "dataclen.dll"));
        assert!(ok(r"%windir%\system32\DATACLEN.DLL", "dataclen.dll"));
        assert!(ok(r"C:\WINDOWS\System32\scavengeui.dll", "scavengeui.dll"));
        assert!(ok(r#""C:\Windows\System32\setupcln.dll""#, "setupcln.dll"));
        for bad in [
            r"C:\Users\x\AppData\evil.dll",
            r"C:\Windows\System32\sub\dataclen.dll",
            r"C:\Windows\System32\..\Temp\dataclen.dll",
            r"C:\Windows\System32\.\dataclen.dll",
            r"dataclen.dll",
            r"System32\dataclen.dll",
            r"%TEMP%\dataclen.dll",
            r"%SystemRoot%\%x%\dataclen.dll",
            r"\\server\share\dataclen.dll",
            r"\\?\C:\Windows\System32\dataclen.dll",
            r"C:\Windows\System32\other.dll",
        ] {
            assert!(!ok(bad, "dataclen.dll"), "{bad}");
        }
    }

    #[test]
    fn clsids_are_reformatted_not_trusted_verbatim() {
        let g = parse_clsid("{606B3777-3051-401F-974A-E66ACA82A3A3}").unwrap();
        assert_eq!(clsid_key(&g), "{606B3777-3051-401F-974A-E66ACA82A3A3}");
        assert_eq!(clsid_key(&parse_clsid("{84e04a55-2d42-4909-86e3-62fd11483e8b}").unwrap()), "{84E04A55-2D42-4909-86E3-62FD11483E8B}");
        for bad in ["606B3777-3051-401F-974A-E66ACA82A3A3", r"{606B3777-3051-401F-974A-E66ACA82A3A3}\..\x", "{x}", ""] {
            assert!(parse_clsid(bad).is_none(), "{bad}");
        }
    }

    /// Every allowlisted handler on this PC passes the trust check (registry + canonical path;
    /// nothing is loaded). Handlers missing on this Windows build are reported, not failed.
    #[test]
    fn handlers_on_this_pc_resolve_to_system32() {
        for h in HANDLERS {
            match trusted(h) {
                Ok((_, dll)) => {
                    let dir = dll.parent().map(|d| d.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                    assert!(dir.ends_with(r"\system32") && !dir.starts_with(r"\\?\"), "{}", dll.display());
                }
                Err(e) => assert_eq!(e, "missing", "{}: {e}", h.key),
            }
        }
    }

    #[test]
    fn execute_without_elevation_records_needs_admin() {
        if system::is_elevated() {
            return;
        }
        // SAFETY: shared data-dir override for tests (same value everywhere).
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", crate::registry::tests::test_data_dir()) };
        let rec = execute(&["sys_setup_logs".into()], "test", |_, _| {}).unwrap();
        assert_eq!(rec.items[0].error.as_deref(), Some("needs_admin"));
        let p = preview(&["sys_update_cleanup".into()], |_, _| {}).unwrap();
        assert!(p[0].needs_admin && !p[0].available);
    }

    /// Elevated, read-only: loads each handler and calls `GetSpaceUsed` only (never `Purge`).
    /// `g2boost_core-*.exe --ignored volcache::tests::measure_live --nocapture`
    #[test]
    #[ignore]
    fn measure_live() {
        assert!(system::is_elevated(), "run elevated");
        let ids: Vec<String> = HANDLERS.iter().map(|h| h.id.to_string()).collect();
        for i in preview(&ids, |d, t| eprintln!("{d}/{t}")).unwrap() {
            println!("{:<28} bytes={:<14} available={} error={:?}", i.id, i.bytes, i.available, i.error);
            assert!(i.error.as_deref() != Some("untrusted_handler"), "{}", i.id);
        }
    }
}
