//! Segurança › Antivírus: which antivirus products Windows knows about (`root\SecurityCenter2
//! AntiVirusProduct`, read without admin) and the one change the app offers: excluding its own
//! exe from Microsoft Defender through WMI (`root\Microsoft\Windows\Defender`, static methods
//! `MSFT_MpPreference.Add` / `Remove` with `ExclusionPath`, the provider behind
//! `Add-MpPreference`). Never disables or weakens anything else; third-party products get a
//! written guide in the UI and nothing is changed in them.
//!
//! The exclusion is offered only when (a) Defender is the active antivirus and (b) the exe lives
//! where standard users cannot write: inside Program Files, with the exe and every folder up to
//! Program Files owned by, and writable only by, SYSTEM, Administrators or TrustedInstaller.
//! Anywhere else (a portable copy, Downloads, the per-user install in `%LOCALAPPDATA%`) another
//! program could replace the file and inherit the exclusion. Only that one file is excluded: no
//! folder, no process exclusion (which would also skip every file the app opens).

use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{ERROR_SUCCESS, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT};
use windows::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce, INHERIT_ONLY_ACE, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    PSID,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, CoSetProxyBlanket, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Ole::{SafeArrayCreateVector, SafeArrayDestroy, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound, SafeArrayPutElement};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Variant::{VARENUM, VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_ARRAY, VT_BSTR};
use windows::Win32::System::Wmi::{
    IWbemLocator, IWbemServices, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_GENERIC_FLAG_TYPE, WbemLocator,
};
use windows::core::{BSTR, PCWSTR, PWSTR, w};

use crate::exposure::{clean, product_state};
use crate::registry::{self, known_folder, wide};
use crate::tasksched::Com;
use crate::{system, volumes, wmi};

const DEFENDER_GUID: &str = "{D68DDC3A-831F-4fae-9E44-DA132C1ACF46}";
const DEFENDER_NS: &str = r"ROOT\Microsoft\Windows\Defender";
/// Vendors with a written guide in the UI (keys of `av.guide.<vendor>.*`).
const VENDORS: [&str; 10] = ["kaspersky", "avast", "avg", "eset", "norton", "bitdefender", "mcafee", "malwarebytes", "trend", "panda"];
/// SYSTEM, Administrators, TrustedInstaller: the only principals allowed to change the exe.
const TRUSTED: [&str; 3] = ["S-1-5-18", "S-1-5-32-544", "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"];
/// Rights that let someone replace, rename or delete the file, drop files next to it, or change its
/// permissions or owner: write/add file, append/add folder, delete child, delete, WRITE_DAC,
/// WRITE_OWNER, GENERIC_ALL, GENERIC_WRITE.
const WRITE_RIGHTS: u32 = 0x2 | 0x4 | 0x40 | 0x1_0000 | 0x4_0000 | 0x8_0000 | 0x1000_0000 | 0x4000_0000;
const ACCESS_ALLOWED: u8 = 0;
const ACCESS_ALLOWED_CALLBACK: u8 = 9;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Product {
    pub name: String,
    /// `on` | `off` | `snoozed` | `expired` (`productState` bits 12 to 15).
    pub scanner: &'static str,
    pub up_to_date: bool,
    pub defender: bool,
    /// Key of the written guide; `None` for Defender and for products without one.
    pub vendor: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// One entry per product (old versions leave extra registrations: folded), active first.
    pub products: Vec<Product>,
    /// Defender is on and no other antivirus is.
    pub defender_active: bool,
    pub exe: String,
    /// Why the Defender exclusion is not offered: `defender_inactive`, `unknown_exe`,
    /// `not_protected_folder`, `user_writable`. `None` = offered.
    pub blocked: Option<&'static str>,
    /// The exe is in Defender's exclusion list (read back elevated; `None` = cannot be read).
    pub excluded: Option<bool>,
    pub elevated: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AvError {
    #[error("needs_admin")]
    NeedsAdmin,
    /// Refused by the rule (same codes as [`Status::blocked`]).
    #[error("{0}")]
    Blocked(&'static str),
    /// Defender took the call but the read-back disagrees (a policy may manage its exclusions).
    #[error("not_applied")]
    NotApplied,
    #[error("failed: {0}")]
    Failed(String),
}

fn failed(e: impl std::fmt::Display) -> AvError {
    AvError::Failed(e.to_string())
}

/* ---------------- products (pure) ---------------- */

pub(crate) struct Row {
    pub name: String,
    pub guid: String,
    pub exe: String,
    pub state: i64,
}

pub(crate) fn is_defender(guid: &str, product_exe: &str) -> bool {
    guid.eq_ignore_ascii_case(DEFENDER_GUID) || product_exe.to_ascii_lowercase().starts_with("windowsdefender:")
}

/// The guide for a product name: whole words, or a prefix for the longer names ("NortonLifeLock").
pub(crate) fn vendor_of(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    lower
        .split(|c: char| !c.is_alphanumeric())
        .find_map(|w| VENDORS.iter().find(|v| w == **v || (v.len() >= 5 && w.starts_with(**v))).copied())
}

fn rank(p: &Product) -> u8 {
    u8::from(p.scanner == "on") * 2 + u8::from(p.up_to_date)
}

/// One product per name (the best registration wins), active ones first.
pub(crate) fn fold(rows: &[Row]) -> Vec<Product> {
    let mut out: Vec<Product> = Vec::new();
    for r in rows {
        let (scanner, up_to_date) = product_state(r.state);
        let defender = is_defender(&r.guid, &r.exe);
        let p = Product { name: clean(&r.name), scanner, up_to_date, defender, vendor: if defender { None } else { vendor_of(&r.name) } };
        match out.iter_mut().find(|o| o.name.eq_ignore_ascii_case(&p.name)) {
            Some(o) if rank(&p) > rank(o) => *o = p,
            Some(_) => {}
            None => out.push(p),
        }
    }
    out.sort_by_key(|p| std::cmp::Reverse(rank(p)));
    out
}

pub(crate) fn defender_active(products: &[Product]) -> bool {
    products.iter().any(|p| p.defender && p.scanner == "on") && !products.iter().any(|p| !p.defender && p.scanner == "on")
}

/* ---------------- where the exe lives (pure) ---------------- */

/// `\\?\C:\x` (what `current_exe` can return) → `C:\x`, rebuilt from its components (a `.`
/// segment is dropped, as Windows does; `..` is kept and refused later).
fn plain(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    Path::new(s.strip_prefix(r"\\?\").filter(|r| r.as_bytes().get(1) == Some(&b':')).unwrap_or(&s)).components().collect()
}

fn same(a: &Component, b: &Component) -> bool {
    a.as_os_str().to_string_lossy().eq_ignore_ascii_case(&b.as_os_str().to_string_lossy())
}

/// The paths whose permissions decide who can swap the exe: the exe, then each folder up to and
/// including the protected root it is strictly inside. `None` = not inside one (component by
/// component, any case; a `.` or `..` anywhere is refused).
pub(crate) fn chain_under(exe: &Path, roots: &[PathBuf]) -> Option<Vec<PathBuf>> {
    if !exe.is_absolute() || exe.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return None;
    }
    let parts: Vec<Component> = exe.components().collect();
    let root = roots.iter().find(|r| {
        let rp: Vec<Component> = r.components().collect();
        parts.len() > rp.len() && parts.iter().zip(&rp).all(|(a, b)| same(a, b))
    })?;
    let depth = root.components().count();
    let mut chain = vec![exe.to_path_buf()];
    let mut dir = exe.parent();
    while let Some(d) = dir {
        chain.push(d.to_path_buf());
        if d.components().count() <= depth {
            break;
        }
        dir = d.parent();
    }
    Some(chain)
}

/// Why the exclusion is refused (`None` = offered). `writable(path)`: someone other than SYSTEM,
/// Administrators or TrustedInstaller could change that file or folder.
pub(crate) fn block_reason(defender_active: bool, exe: Option<&Path>, roots: &[PathBuf], writable: impl Fn(&Path) -> bool) -> Option<&'static str> {
    if !defender_active {
        return Some("defender_inactive");
    }
    let Some(exe) = exe else {
        return Some("unknown_exe");
    };
    let Some(chain) = chain_under(exe, roots) else {
        return Some("not_protected_folder");
    };
    chain.iter().any(|p| writable(p)).then_some("user_writable")
}

/// One grant of a DACL, as `GetAce` returns it.
#[derive(Debug, Clone)]
pub(crate) struct Ace {
    pub sid: String,
    pub mask: u32,
    pub inherit_only: bool,
}

/// Could anyone but SYSTEM / Administrators / TrustedInstaller change the object? An owner
/// outside that set can always rewrite the permissions; a NULL DACL grants everyone everything;
/// inherit-only grants do not apply to the object itself. Deny entries are not listed (they could
/// only make it stricter).
pub(crate) fn writable_by_others(owner: &str, dacl: Option<&[Ace]>) -> bool {
    let trusted = |s: &str| TRUSTED.iter().any(|t| t.eq_ignore_ascii_case(s));
    let Some(aces) = dacl else {
        return true;
    };
    !trusted(owner) || aces.iter().any(|a| !a.inherit_only && a.mask & WRITE_RIGHTS != 0 && !trusted(&a.sid))
}

/* ---------------- readers ---------------- */

fn sid_string(sid: PSID) -> Option<String> {
    let mut s = PWSTR::null();
    // SAFETY: valid SID from the security descriptor; the API allocates the string.
    unsafe { ConvertSidToStringSidW(sid, &mut s) }.ok()?;
    // SAFETY: NUL-terminated string from the call above, freed once with LocalFree.
    unsafe {
        let out = s.to_string().ok();
        let _ = LocalFree(Some(HLOCAL(s.0.cast())));
        out
    }
}

/// (owner SID, allow grants) of a file or folder; `None` when it cannot be read.
fn read_acl(p: &Path) -> Option<(String, Option<Vec<Ace>>)> {
    let w = wide(&p.to_string_lossy());
    let mut owner = PSID::default();
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: NUL-terminated path; the out-pointers point into `sd`, freed below.
    let rc = unsafe {
        GetNamedSecurityInfoW(PCWSTR(w.as_ptr()), SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION, Some(&mut owner), None, Some(&mut dacl), None, &mut sd)
    };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let read = || -> Option<(String, Option<Vec<Ace>>)> {
        let owner = sid_string(owner)?;
        if dacl.is_null() {
            return Some((owner, None));
        }
        let mut aces = Vec::new();
        // SAFETY: a valid ACL inside `sd`; each ACE pointer is valid while `sd` lives.
        unsafe {
            for i in 0..u32::from((*dacl).AceCount) {
                let mut ace: *mut c_void = std::ptr::null_mut();
                GetAce(dacl, i, &mut ace).ok()?;
                let h = &*(ace as *const ACE_HEADER);
                if !matches!(h.AceType, ACCESS_ALLOWED | ACCESS_ALLOWED_CALLBACK) {
                    continue;
                }
                // Both types start with header, mask, SID.
                let a = &*(ace as *const ACCESS_ALLOWED_ACE);
                let sid = PSID((&raw const a.SidStart) as *mut c_void);
                aces.push(Ace { sid: sid_string(sid)?, mask: a.Mask, inherit_only: u32::from(h.AceFlags) & INHERIT_ONLY_ACE.0 != 0 });
            }
        }
        Some((owner, Some(aces)))
    };
    let out = read();
    // SAFETY: the descriptor GetNamedSecurityInfoW allocated, freed once.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(sd.0)));
    }
    out
}

/// Unreadable permissions count as writable: refusing is the safe answer.
fn writable(p: &Path) -> bool {
    read_acl(p).is_none_or(|(owner, aces)| writable_by_others(&owner, aces.as_deref()))
}

/// Program Files and Program Files (x86), from the shell (not from environment variables).
fn program_files() -> Vec<PathBuf> {
    use windows::Win32::UI::Shell::{FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86};
    let mut out: Vec<PathBuf> = [FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86].iter().filter_map(known_folder).collect();
    out.dedup();
    out
}

fn av_rows() -> Vec<Row> {
    let fields = ["displayName", "instanceGuid", "pathToSignedProductExe", "productState"];
    let rows = wmi::query_timeout(r"ROOT\SecurityCenter2", &format!("SELECT {} FROM AntiVirusProduct", fields.join(", ")), &fields, Duration::from_secs(5)).unwrap_or_default();
    let text = |r: &wmi::Row, k: &str| r.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    rows.iter()
        .take(16)
        .filter_map(|r| {
            Some(Row { name: r.get("displayName")?.as_str()?.to_string(), guid: text(r, "instanceGuid"), exe: text(r, "pathToSignedProductExe"), state: r.get("productState")?.as_f64()? as i64 })
        })
        .collect()
}

/* ---------------- Defender (WMI) ---------------- */

fn defender() -> Result<IWbemServices, AvError> {
    // SAFETY: standard WMI client sequence (as in wmi.rs); COM initialised by the caller.
    unsafe {
        let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).map_err(failed)?;
        let empty = BSTR::new();
        let svc = locator.ConnectServer(&BSTR::from(DEFENDER_NS), &empty, &empty, &empty, 0, &empty, None).map_err(failed)?;
        CoSetProxyBlanket(&svc, RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE, PCWSTR::null(), RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE)
            .map_err(failed)?;
        Ok(svc)
    }
}

/// The strings of a `VT_ARRAY | VT_BSTR` VARIANT (empty for anything else).
fn strings(v: &VARIANT) -> Vec<String> {
    // SAFETY: the union member read is the one `vt` names; elements are copied out (owned BSTRs).
    unsafe {
        let inner = &v.Anonymous.Anonymous;
        let psa = inner.Anonymous.parray;
        if inner.vt.0 != (VT_ARRAY.0 | VT_BSTR.0) || psa.is_null() {
            return Vec::new();
        }
        let (Ok(lo), Ok(hi)) = (SafeArrayGetLBound(psa, 1), SafeArrayGetUBound(psa, 1)) else {
            return Vec::new();
        };
        (lo..=hi)
            .take(10_000)
            .filter_map(|i| {
                let mut b = BSTR::new();
                SafeArrayGetElement(psa, &i, (&mut b as *mut BSTR).cast()).ok()?;
                Some(b.to_string())
            })
            .collect()
    }
}

/// A one-element `VT_ARRAY | VT_BSTR` VARIANT (the type of `ExclusionPath`); freed on drop.
fn one_string(s: &str) -> Result<VARIANT, AvError> {
    // SAFETY: the array is created here and owned by the returned VARIANT (VariantClear frees it);
    // PutElement copies the BSTR.
    unsafe {
        let psa = SafeArrayCreateVector(VT_BSTR, 0, 1);
        if psa.is_null() {
            return Err(failed("SafeArrayCreateVector"));
        }
        let b = BSTR::from(s);
        if let Err(e) = SafeArrayPutElement(psa, &0, b.as_ptr().cast()) {
            let _ = SafeArrayDestroy(psa);
            return Err(failed(e));
        }
        Ok(VARIANT {
            Anonymous: VARIANT_0 {
                Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                    vt: VARENUM(VT_ARRAY.0 | VT_BSTR.0),
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: VARIANT_0_0_0 { parray: psa },
                }),
            },
        })
    }
}

/// Defender's `ExclusionPath` list. Admin only: a standard user gets a placeholder string.
fn exclusion_paths(svc: &IWbemServices) -> Result<Vec<String>, AvError> {
    let flags = WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_FORWARD_ONLY.0 | WBEM_FLAG_RETURN_IMMEDIATELY.0);
    // SAFETY: plain WMI query and property read on objects owned here.
    unsafe {
        let rows = svc.ExecQuery(&BSTR::from("WQL"), &BSTR::from("SELECT ExclusionPath FROM MSFT_MpPreference"), flags, None).map_err(failed)?;
        let mut objs = [None];
        let mut n = 0u32;
        let _ = rows.Next(15_000, &mut objs, &mut n);
        let obj = objs[0].take().ok_or_else(|| failed("no MSFT_MpPreference"))?;
        let mut v = VARIANT::default();
        obj.Get(w!("ExclusionPath"), 0, &mut v, None, None).map_err(failed)?;
        Ok(strings(&v))
    }
}

/// `MSFT_MpPreference.<method>(ExclusionPath = [path])`, a static method; `ReturnValue` 0 = done.
fn mp_call(svc: &IWbemServices, method: &str, path: &str) -> Result<(), AvError> {
    let class_name = BSTR::from("MSFT_MpPreference");
    let m = wide(method);
    // SAFETY: WMI method call with an input instance spawned from the method's own signature.
    unsafe {
        let mut class = None;
        svc.GetObject(&class_name, WBEM_GENERIC_FLAG_TYPE(0), None, Some(&mut class), None).map_err(failed)?;
        let class = class.ok_or_else(|| failed("no class"))?;
        let mut sig = None;
        class.GetMethod(PCWSTR(m.as_ptr()), 0, &mut sig, std::ptr::null_mut()).map_err(failed)?;
        let input = sig.ok_or_else(|| failed("no signature"))?.SpawnInstance(0).map_err(failed)?;
        input.Put(w!("ExclusionPath"), 0, &one_string(path)?, 0).map_err(failed)?;
        let mut out = None;
        svc.ExecMethod(&class_name, &BSTR::from(method), WBEM_GENERIC_FLAG_TYPE(0), None, &input, Some(&mut out), None).map_err(failed)?;
        match out.as_ref().and_then(|o| volumes::get_u32(o, "ReturnValue")) {
            Some(rv) if rv != 0 => Err(AvError::Failed(format!("ReturnValue {rv}"))),
            _ => Ok(()),
        }
    }
}

fn read_excluded(exe: &str) -> Option<bool> {
    let _com = Com::init().ok()?;
    let paths = exclusion_paths(&defender().ok()?).ok()?;
    Some(paths.iter().any(|p| p.trim_end_matches('\\').eq_ignore_ascii_case(exe)))
}

/* ---------------- public ---------------- */

fn exe_path() -> Option<PathBuf> {
    std::env::current_exe().ok().map(|p| plain(&p))
}

/// Products, whether the Defender exclusion is offered (and why not) and, elevated, whether it is
/// in place. No admin needed (the read-back is skipped without it).
pub fn status() -> Status {
    let products = fold(&av_rows());
    let defender_active = defender_active(&products);
    let exe = exe_path();
    let blocked = block_reason(defender_active, exe.as_deref(), &program_files(), writable);
    let elevated = system::is_elevated();
    let exe = exe.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let excluded = if elevated && defender_active && !exe.is_empty() { read_excluded(&exe) } else { None };
    Status { products, defender_active, exe, blocked, excluded, elevated }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogLine {
    at: u64,
    action: String,
}

/// When the exclusion was added or removed (no paths).
fn log(action: &str) {
    let f = registry::data_dir().join("antivirus-log.json");
    let mut all: Vec<LogLine> = std::fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    all.push(LogLine { at: registry::now_unix(), action: action.to_string() });
    let skip = all.len().saturating_sub(20);
    all.drain(..skip);
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(f, serde_json::to_string_pretty(&all).unwrap_or_default());
}

/// Add this exe to Defender's exclusions (admin, after the UI's confirmation). The rule is
/// evaluated again here, never taken from the caller; the result is read back.
pub fn exclude() -> Result<Status, AvError> {
    if !system::is_elevated() {
        return Err(AvError::NeedsAdmin);
    }
    let s = status();
    if let Some(why) = s.blocked {
        return Err(AvError::Blocked(why));
    }
    if s.excluded != Some(true) {
        let _com = Com::init().map_err(failed)?;
        mp_call(&defender()?, "Add", &s.exe)?;
    }
    let after = status();
    if after.excluded != Some(true) {
        return Err(AvError::NotApplied);
    }
    log("add");
    Ok(after)
}

/// Undo: remove this exe (and only it) from Defender's exclusions. Admin.
pub fn unexclude() -> Result<Status, AvError> {
    if !system::is_elevated() {
        return Err(AvError::NeedsAdmin);
    }
    let exe = exe_path().ok_or(AvError::Blocked("unknown_exe"))?;
    {
        let _com = Com::init().map_err(failed)?;
        mp_call(&defender()?, "Remove", &exe.to_string_lossy())?;
    }
    let after = status();
    if after.excluded == Some(true) {
        return Err(AvError::NotApplied);
    }
    log("remove");
    Ok(after)
}

/// "Mostrar na pasta" for the app's own exe.
pub fn reveal_exe() -> Result<(), AvError> {
    crate::disk::reveal(&exe_path().ok_or(AvError::Blocked("unknown_exe"))?).map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, guid: &str, exe: &str, state: i64) -> Row {
        Row { name: name.into(), guid: guid.into(), exe: exe.into(), state }
    }

    #[test]
    fn product_states_from_known_values() {
        // Real values from this PC (2026-10-25): third-party AV on and current, Defender off.
        assert_eq!(product_state(266_240), ("on", true));
        assert_eq!(product_state(393_472), ("off", true));
        // Defender on and current / on with old definitions / snoozed.
        assert_eq!(product_state(397_568), ("on", true));
        assert_eq!(product_state(397_584), ("on", false));
        assert_eq!(product_state(0x6_2100).0, "snoozed");
        assert_eq!(product_state(0x6_3100).0, "expired");
    }

    #[test]
    fn products_fold_and_who_is_active() {
        let k = "C:\\Program Files (x86)\\Kaspersky Lab\\Kaspersky 21.26\\wmiav.exe";
        let rows = [
            row("Kaspersky", "{A03EE001-4276-7D6F-4B4A-400EB933F7F3}", k, 266_240),
            row("Kaspersky", "{DABD1ABC-6D70-BB0E-89E6-BFA3FC920FD1}", k, 266_240),
            row("Windows Defender", "{D68DDC3A-831F-4fae-9E44-DA132C1ACF46}", "windowsdefender://", 393_472),
            row("Kaspersky", "{70E35457-C7D9-669C-FEA5-55382EABDC78}", k, 266_240),
        ];
        let p = fold(&rows);
        assert_eq!(p.len(), 2, "old registrations of one product are folded");
        assert_eq!((p[0].name.as_str(), p[0].vendor, p[0].defender), ("Kaspersky", Some("kaspersky"), false));
        assert_eq!((p[1].defender, p[1].scanner, p[1].vendor), (true, "off", None));
        assert!(!defender_active(&p), "a third-party antivirus is on: Defender is passive");

        let alone = fold(&[row("Windows Defender", "{d68ddc3a-831f-4fae-9e44-da132c1acf46}", "", 397_568)]);
        assert!(alone[0].defender && defender_active(&alone), "GUID compared in any case");
        let both = fold(&[row("Microsoft Defender Antivirus", "", "windowsdefender://", 397_568), row("ESET Security", "", "", 266_240)]);
        assert!(!defender_active(&both));
        assert!(!defender_active(&fold(&[row("Windows Defender", DEFENDER_GUID, "", 393_472)])), "off");
        assert!(!defender_active(&[]));
        // The best registration of a name wins.
        let best = fold(&[row("AV", "", "", 393_472), row("AV", "", "", 266_240)]);
        assert_eq!((best.len(), best[0].scanner), (1, "on"));
    }

    #[test]
    fn vendor_names() {
        for (name, v) in [
            ("Kaspersky", "kaspersky"),
            ("Avast Antivirus", "avast"),
            ("AVG AntiVirus FREE", "avg"),
            ("ESET Security", "eset"),
            ("Norton 360", "norton"),
            ("NortonLifeLock Security", "norton"),
            ("Bitdefender Antivirus", "bitdefender"),
            ("McAfee VirusScan", "mcafee"),
            ("Malwarebytes", "malwarebytes"),
            ("Trend Micro Antivirus+", "trend"),
            ("Panda Dome", "panda"),
        ] {
            assert_eq!(vendor_of(name), Some(v), "{name}");
        }
        assert_eq!(vendor_of("Avira Security"), None);
        assert_eq!(vendor_of("AVGuard Pro"), None, "short keys need the whole word");
        assert_eq!(vendor_of("Reset Tool"), None);
        assert_eq!(vendor_of(""), None);
    }

    #[test]
    fn exclusion_only_inside_program_files_and_never_where_users_can_write() {
        let roots = [PathBuf::from(r"C:\Program Files"), PathBuf::from(r"C:\Program Files (x86)")];
        let none = |_: &Path| false;
        let exe = Path::new(r"C:\Program Files\G2 Boost\G2 Boost.exe");
        assert_eq!(block_reason(true, Some(exe), &roots, none), None);
        assert_eq!(
            chain_under(exe, &roots).unwrap(),
            [PathBuf::from(r"C:\Program Files\G2 Boost\G2 Boost.exe"), PathBuf::from(r"C:\Program Files\G2 Boost"), PathBuf::from(r"C:\Program Files")]
        );
        // A writable install folder (an installer that granted Users write) is refused.
        let folder_open = |p: &Path| p.ends_with("G2 Boost");
        assert_eq!(block_reason(true, Some(exe), &roots, folder_open), Some("user_writable"));
        assert_eq!(block_reason(true, Some(exe), &roots, |p: &Path| p.extension().is_some()), Some("user_writable"), "the exe itself");
        assert_eq!(block_reason(false, Some(exe), &roots, none), Some("defender_inactive"));
        assert_eq!(block_reason(true, None, &roots, none), Some("unknown_exe"));
        assert_eq!(block_reason(true, Some(Path::new(r"c:\PROGRAM FILES (X86)\g2 boost\g2 boost.exe")), &roots, none), None, "any case");
        for hostile in [
            r"C:\Users\me\Downloads\G2 Boost.exe",
            r"C:\Users\me\AppData\Local\G2 Boost\G2 Boost.exe",
            r"C:\Program Files Evil\G2 Boost.exe",
            r"C:\Program Files\..\Users\me\G2 Boost.exe",
            r"C:\Program Files\G2 Boost\..\..\Users\me\G2 Boost.exe",
            r"\\server\share\Program Files\G2 Boost.exe",
            r"D:\Program Files\G2 Boost.exe",
            r"C:\Program Files",
            r"Program Files\G2 Boost.exe",
        ] {
            assert_eq!(block_reason(true, Some(Path::new(hostile)), &roots, none), Some("not_protected_folder"), "{hostile}");
        }
        assert_eq!(plain(Path::new(r"\\?\C:\Program Files\G2 Boost\G2 Boost.exe")), Path::new(r"C:\Program Files\G2 Boost\G2 Boost.exe"));
        assert_eq!(plain(Path::new(r"C:\Program Files\.\G2 Boost\G2 Boost.exe")), Path::new(r"C:\Program Files\G2 Boost\G2 Boost.exe"), "`.` is the same folder");
        assert_eq!(plain(Path::new(r"\\?\UNC\srv\x")), Path::new(r"\\?\UNC\srv\x"), "only drive paths are unwrapped");
    }

    fn ace(sid: &str, mask: u32, inherit_only: bool) -> Ace {
        Ace { sid: sid.into(), mask, inherit_only }
    }

    #[test]
    fn who_can_write_from_real_acl_shapes() {
        const TI: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";
        // C:\Program Files on this PC (SDDL, 2026-10-25).
        let program_files = [
            ace("S-1-3-0", 0x1000_0000, true),
            ace("S-1-5-18", 0x1000_0000, true),
            ace("S-1-5-18", 0x1301bf, false),
            ace("S-1-5-32-544", 0x1000_0000, true),
            ace("S-1-5-32-544", 0x1301bf, false),
            ace("S-1-5-32-545", 0xA000_0000, true),
            ace("S-1-5-32-545", 0x1200a9, false),
            ace(TI, 0x1f01ff, false),
            ace("S-1-15-2-1", 0x1200a9, false),
            ace("S-1-15-2-2", 0x1200a9, false),
        ];
        assert!(!writable_by_others(TI, Some(&program_files)));
        // A folder an elevated installer created inside it: owner Administrators, inherited ACL.
        let app = [
            ace(TI, 0x1f01ff, false),
            ace("S-1-5-18", 0x1f01ff, false),
            ace("S-1-5-32-544", 0x1f01ff, false),
            ace("S-1-5-32-545", 0x1200a9, false),
            ace("S-1-3-0", 0x1000_0000, true),
            ace("S-1-15-2-1", 0x1200a9, false),
        ];
        assert!(!writable_by_others("S-1-5-32-544", Some(&app)));
        assert!(!writable_by_others("S-1-5-18", Some(&app)));
        let mut users_modify = app.to_vec();
        users_modify.push(ace("S-1-5-32-545", 0x1301bf, false));
        assert!(writable_by_others("S-1-5-32-544", Some(&users_modify)), "Users can modify");
        assert!(writable_by_others("S-1-5-21-1-2-3-1001", Some(&app)), "owned by a user account");
        assert!(writable_by_others("S-1-5-32-544", Some(&[ace("S-1-5-11", 0x1301bf, false)])), "Authenticated Users");
        assert!(writable_by_others("S-1-5-32-544", Some(&[ace("S-1-1-0", 0x1000_0000, false)])), "Everyone GENERIC_ALL");
        assert!(writable_by_others("S-1-5-32-544", Some(&[ace("S-1-5-32-545", 0x4, false)])), "Users can add folders");
        assert!(!writable_by_others("S-1-5-32-544", Some(&[ace("S-1-5-32-545", 0x110, false)])), "attributes only: cannot swap the file");
        assert!(!writable_by_others("S-1-5-32-544", Some(&[ace("S-1-5-32-545", 0x1301bf, true)])), "inherit-only does not apply here");
        assert!(writable_by_others("S-1-5-32-544", None), "NULL DACL");
    }

    /// Read-only, no admin: the real permissions of system folders, a user folder and this exe.
    #[test]
    fn real_permissions() {
        let pf = program_files();
        assert!(pf.iter().any(|p| p.join("Common Files").is_dir()), "{pf:?}");
        assert!(!writable(&pf[0]), "Program Files is closed to users");
        assert!(!writable(&crate::repair::system32().join("kernel32.dll")));
        assert!(writable(&std::env::temp_dir()), "the user's temp folder is theirs");
        assert!(writable(Path::new(r"C:\no\such\file.exe")), "unreadable = refused");
        // The test binary lives in a build folder: never offered.
        let s = status();
        assert!(matches!(s.blocked, Some("defender_inactive" | "not_protected_folder" | "user_writable")), "{:?}", s.blocked);
        if !s.elevated {
            assert_eq!(s.excluded, None);
            assert!(matches!(exclude(), Err(AvError::NeedsAdmin)));
            assert!(matches!(unexclude(), Err(AvError::NeedsAdmin)));
        }
    }

    /// Read-only: what this PC reports. `--ignored live_antivirus_status --nocapture`
    #[test]
    #[ignore]
    fn live_antivirus_status() {
        println!("{:#?}", status());
    }
}
