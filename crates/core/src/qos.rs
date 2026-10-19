//! Network priority for the running game: a Policy-based QoS policy that marks the game
//! exe's outbound packets with DSCP 46 (Expedited Forwarding) while Game Mode is on.
//!
//! Native WMI only (`ROOT\StandardCimv2`, `MSFT_NetQosPolicySettingData`, the class behind
//! `New-NetQosPolicy`). Every call carries `PolicyStore = "ActiveStore"` in the
//! `IWbemContext`, so the policy lives only in the active store: a reboot clears it even if
//! G2 Boost crashes. Never touches `HKLM\SOFTWARE\Policies` (the antivirus-flagged path)
//! and never spawns powershell/netsh. Only policies named `G2Boost-*` are ever touched.
//!
//! Mirrors the NetQos cmdlets (`MSFT_NetQosPolicy.cdxml`): New = `cim:CreateInstance`, Remove
//! = query + `cim:DeleteInstance`, `-PolicyStore` = operation option (our context value).
//! Reading the ActiveStore already needs admin (`WBEM_E_ACCESS_DENIED` → `NeedsAdmin`).

use std::mem::ManuallyDrop;

use serde::Serialize;
use windows::Win32::Foundation::E_ACCESSDENIED;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, CoSetProxyBlanket, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Variant::{VARIANT, VT_BSTR, VT_I2, VT_I4, VT_UI1, VariantClear};
use windows::Win32::System::Wmi::{
    CIM_SINT8, CIM_SINT16, CIM_UINT8, IWbemClassObject, IWbemContext, IWbemLocator, IWbemServices, WBEM_E_ACCESS_DENIED,
    WBEM_E_NOT_FOUND, WBEM_FLAG_CREATE_OR_UPDATE, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY,
    WBEM_GENERIC_FLAG_TYPE, WbemContext, WbemLocator,
};
use windows::core::{BSTR, PCWSTR};

use crate::registry::wide;
use crate::tasksched::Com;
use crate::wmi::WmiValue;

/// Every policy we create is named `G2Boost-<sanitized exe>`.
pub const PREFIX: &str = "G2Boost-";
/// Expedited Forwarding. `DSCPAction` is declared `sint8` (MinValue -1, MaxValue 63);
/// `put_int` reads the CIMTYPE from the class at runtime and sends it as `VT_I2`.
const DSCP_EF: i32 = 46;
/// `NetworkProfile` bitmap: Domain 1 | Public 2 | Private 4 (`NetQosPolicy.NetworkProfile` "All").
const NETWORK_PROFILE_ALL: i32 = 7;
const NAMESPACE: &str = r"ROOT\StandardCimv2";
const CLASS: &str = "MSFT_NetQosPolicySettingData";

/// Never prioritise (or let a caller target) core Windows processes.
const SYSTEM_EXES: &[&str] = &[
    "system.exe", "svchost.exe", "explorer.exe", "csrss.exe", "lsass.exe", "smss.exe", "wininit.exe", "winlogon.exe",
    "services.exe", "dwm.exe", "spoolsv.exe", "sihost.exe", "taskhostw.exe", "fontdrvhost.exe", "runtimebroker.exe",
    "searchhost.exe", "conhost.exe", "ctfmon.exe", "lsaiso.exe", "audiodg.exe", "msmpeng.exe",
];

#[derive(Debug, thiserror::Error)]
pub enum QosError {
    #[error("needs_admin")]
    NeedsAdmin,
    #[error("invalid")]
    Invalid,
    #[error("wmi: {0}")]
    Wmi(String),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QosPolicy {
    pub name: String,
    pub app: String,
    pub dscp: i64,
}

/// Bare `name.exe`, 5–64 chars, no path or wildcard chars, not a core Windows process.
pub(crate) fn valid_exe(exe: &str) -> bool {
    let n = exe.chars().count();
    (5..=64).contains(&n)
        && exe.to_ascii_lowercase().ends_with(".exe")
        && exe.trim() == exe
        && !exe.chars().any(|c| c.is_control() || r#"\/:*?"<>|"#.contains(c))
        && !SYSTEM_EXES.contains(&exe.to_ascii_lowercase().as_str())
}

/// `G2Boost-` + the exe name lowercased, anything outside `[a-z0-9._-]` turned into `_`.
pub(crate) fn policy_name(exe: &str) -> String {
    let tail: String = exe
        .chars()
        .map(|c| c.to_ascii_lowercase())
        .map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c) { c } else { '_' })
        .collect();
    format!("{PREFIX}{tail}")
}

/// Only names this module could have produced: `G2Boost-` + 1..=64 chars of `[a-z0-9._-]`.
pub(crate) fn is_ours(name: &str) -> bool {
    name.strip_prefix(PREFIX).is_some_and(|t| {
        (1..=64).contains(&t.len()) && t.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c))
    })
}

/// `MSFT_NetQosPolicySettingData.InstanceID="..."` with `\` and `"` escaped (WMI object path
/// syntax). The key is `InstanceID` (inherited from `CIM_SettingData`, checked via the `key`
/// qualifier), not `Name`, so deletes go through the id the query returned.
pub(crate) fn object_path(instance_id: &str) -> String {
    let esc = instance_id.replace('\\', r"\\").replace('"', "\\\"");
    format!("{CLASS}.InstanceID=\"{esc}\"")
}

/// VARIANT type WMI expects for an integer property of `cimtype` (`CIM_SINT8`/`CIM_SINT16` →
/// `VT_I2`, `CIM_UINT8` → `VT_UI1`, the 16/32-bit rest → `VT_I4`; WMI's documented mapping).
pub(crate) fn int_vt(cimtype: i32) -> u16 {
    match cimtype {
        t if t == CIM_SINT8.0 || t == CIM_SINT16.0 => VT_I2.0,
        t if t == CIM_UINT8.0 => VT_UI1.0,
        _ => VT_I4.0,
    }
}

/// VARIANT that frees itself (BSTR) on drop.
struct Var(VARIANT);

impl Var {
    fn bstr(s: &str) -> Self {
        let mut v = VARIANT::default();
        // SAFETY: tag and payload are written together; the BSTR is freed by VariantClear on drop.
        unsafe {
            let inner = &mut *v.Anonymous.Anonymous;
            inner.vt = VT_BSTR;
            inner.Anonymous.bstrVal = ManuallyDrop::new(BSTR::from(s));
        }
        Self(v)
    }
    fn int(vt: u16, x: i32) -> Self {
        let mut v = VARIANT::default();
        // SAFETY: tag and payload are written together; integers own nothing.
        unsafe {
            let inner = &mut *v.Anonymous.Anonymous;
            inner.vt.0 = vt;
            match vt {
                x2 if x2 == VT_I2.0 => inner.Anonymous.iVal = x as i16,
                x2 if x2 == VT_UI1.0 => inner.Anonymous.bVal = x as u8,
                _ => inner.Anonymous.lVal = x,
            }
        }
        Self(v)
    }
}

impl Drop for Var {
    fn drop(&mut self) {
        // SAFETY: the VARIANT was initialised by us or by WMI's Get.
        unsafe {
            let _ = VariantClear(&mut self.0);
        }
    }
}

fn wmi_err(e: windows::core::Error) -> QosError {
    let c = e.code().0;
    if c == WBEM_E_ACCESS_DENIED.0 || c == E_ACCESSDENIED.0 {
        QosError::NeedsAdmin
    } else {
        QosError::Wmi(format!("{:#010x} {}", c as u32, e.message()))
    }
}

/// Connected `ROOT\StandardCimv2` + the ActiveStore context. Field order matters: COM is
/// uninitialised last.
struct Session {
    svc: IWbemServices,
    ctx: IWbemContext,
    _com: Com,
}

impl Session {
    fn open() -> Result<Self, QosError> {
        let com = Com::init().map_err(|e| QosError::Wmi(e.to_string()))?;
        // SAFETY: standard WMI client sequence (same as wmi.rs); COM objects release on drop.
        unsafe {
            let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).map_err(wmi_err)?;
            let empty = BSTR::new();
            let svc = locator.ConnectServer(&BSTR::from(NAMESPACE), &empty, &empty, &empty, 0, &empty, None).map_err(wmi_err)?;
            CoSetProxyBlanket(&svc, RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE, PCWSTR::null(), RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE)
                .map_err(wmi_err)?;
            let ctx: IWbemContext = CoCreateInstance(&WbemContext, None, CLSCTX_INPROC_SERVER).map_err(wmi_err)?;
            // The NetQos provider picks the store from this context value (what -PolicyStore sets).
            let store = Var::bstr("ActiveStore");
            let key = wide("PolicyStore");
            ctx.SetValue(PCWSTR(key.as_ptr()), 0, &store.0).map_err(wmi_err)?;
            Ok(Self { svc, ctx, _com: com })
        }
    }

    fn class(&self) -> Result<IWbemClassObject, QosError> {
        let mut obj = None;
        // SAFETY: out-pointer to a local Option; WMI fills it on success.
        unsafe { self.svc.GetObject(&BSTR::from(CLASS), WBEM_GENERIC_FLAG_TYPE(0), &self.ctx, Some(&mut obj), None) }.map_err(wmi_err)?;
        obj.ok_or_else(|| QosError::Wmi("class not found".into()))
    }

    /// Our policies (`is_ours` names only) with their `InstanceID` key.
    fn list(&self) -> Result<Vec<(QosPolicy, String)>, QosError> {
        let wql = format!("SELECT InstanceID, Name, AppPathNameMatchCondition, DSCPAction FROM {CLASS}");
        let flags = WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_FORWARD_ONLY.0 | WBEM_FLAG_RETURN_IMMEDIATELY.0);
        let mut out = Vec::new();
        // SAFETY: standard enumeration; each object is released on drop.
        unsafe {
            let rows = self.svc.ExecQuery(&BSTR::from("WQL"), &BSTR::from(wql), flags, &self.ctx).map_err(wmi_err)?;
            loop {
                let mut objs: [Option<IWbemClassObject>; 1] = [None];
                let mut returned = 0u32;
                let hr = rows.Next(10_000, &mut objs, &mut returned);
                if hr.is_err() {
                    return Err(wmi_err(hr.into()));
                }
                let Some(obj) = objs[0].take().filter(|_| returned == 1) else {
                    break;
                };
                let name = get(&obj, "Name").as_str().unwrap_or_default().to_string();
                let id = get(&obj, "InstanceID").as_str().unwrap_or_default().to_string();
                if !is_ours(&name) || id.is_empty() {
                    continue;
                }
                let app = get(&obj, "AppPathNameMatchCondition").as_str().unwrap_or_default().to_string();
                let dscp = match get(&obj, "DSCPAction") {
                    WmiValue::Int(i) => i,
                    _ => -1,
                };
                out.push((QosPolicy { name, app, dscp }, id));
            }
        }
        Ok(out)
    }

    /// Deletes every policy of ours named `name` (none = Ok, so it is idempotent).
    fn delete_named(&self, name: &str) -> Result<usize, QosError> {
        let ids: Vec<String> = self.list()?.into_iter().filter(|(p, _)| p.name == name).map(|(_, id)| id).collect();
        for id in &ids {
            self.delete(id)?;
        }
        Ok(ids.len())
    }

    fn delete(&self, instance_id: &str) -> Result<(), QosError> {
        // SAFETY: plain call with BSTR/context we own.
        let r = unsafe { self.svc.DeleteInstance(&BSTR::from(object_path(instance_id)), WBEM_GENERIC_FLAG_TYPE(0), &self.ctx, None) };
        match r {
            Err(e) if e.code().0 == WBEM_E_NOT_FOUND.0 => Ok(()), // already gone (reboot, manual removal)
            r => r.map_err(wmi_err),
        }
    }
}

/// Property as a `WmiValue` (strings and integers; anything else is `Null`).
fn get(obj: &IWbemClassObject, prop: &str) -> WmiValue {
    let key = wide(prop);
    let mut v = Var(VARIANT::default());
    // SAFETY: WMI writes an initialised VARIANT; we only read the member `vt` names.
    unsafe {
        if obj.Get(PCWSTR(key.as_ptr()), 0, &mut v.0, None, None).is_err() {
            return WmiValue::Null;
        }
        let inner = &v.0.Anonymous.Anonymous;
        let u = &inner.Anonymous;
        match inner.vt {
            VT_BSTR => WmiValue::Str((*u.bstrVal).to_string()),
            VT_I2 => WmiValue::Int(i64::from(u.iVal)),
            VT_I4 => WmiValue::Int(i64::from(u.lVal)),
            VT_UI1 => WmiValue::Int(i64::from(u.bVal)),
            _ => WmiValue::Null,
        }
    }
}

/// Puts `value` into `prop`, as the integer VARIANT the class declares for it.
fn put_int(class: &IWbemClassObject, inst: &IWbemClassObject, prop: &str, value: i32) -> Result<(), QosError> {
    let key = wide(prop);
    let mut cimtype = 0i32;
    let mut probe = Var(VARIANT::default());
    // SAFETY: reading the class definition fills a VARIANT we clear and an i32 we own.
    unsafe { class.Get(PCWSTR(key.as_ptr()), 0, &mut probe.0, Some(&mut cimtype), None) }.map_err(wmi_err)?;
    let v = Var::int(int_vt(cimtype), value);
    // SAFETY: type 0 = keep the declared CIM type; the VARIANT outlives the call.
    unsafe { inst.Put(PCWSTR(key.as_ptr()), 0, &v.0, 0) }.map_err(wmi_err)
}

fn put_str(inst: &IWbemClassObject, prop: &str, value: &str) -> Result<(), QosError> {
    let key = wide(prop);
    let v = Var::bstr(value);
    // SAFETY: type 0 = keep the declared CIM type; the VARIANT outlives the call.
    unsafe { inst.Put(PCWSTR(key.as_ptr()), 0, &v.0, 0) }.map_err(wmi_err)
}

/// Marks `exe_name`'s outbound traffic DSCP 46 in the ActiveStore. Returns the policy name.
pub fn set_game(exe_name: &str) -> Result<String, QosError> {
    if !valid_exe(exe_name) {
        return Err(QosError::Invalid);
    }
    if !crate::system::is_elevated() {
        return Err(QosError::NeedsAdmin);
    }
    let name = policy_name(exe_name);
    let s = Session::open()?;
    // The key is a provider-made InstanceID, so a leftover with our name would become a
    // duplicate (or an "already exists" error): drop it first.
    s.delete_named(&name)?;
    let class = s.class()?;
    // SAFETY: SpawnInstance on a class object we hold.
    let inst = unsafe { class.SpawnInstance(0) }.map_err(wmi_err)?;
    put_str(&inst, "Name", &name)?;
    put_str(&inst, "AppPathNameMatchCondition", exe_name)?;
    put_int(&class, &inst, "DSCPAction", DSCP_EF)?;
    put_int(&class, &inst, "NetworkProfile", NETWORK_PROFILE_ALL)?;
    // SAFETY: plain call with an instance and context we own.
    unsafe { s.svc.PutInstance(&inst, WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_CREATE_OR_UPDATE.0), &s.ctx, None) }.map_err(wmi_err)?;
    Ok(name)
}

/// Removes one of our policies (gone already = Ok). Foreign names are `Invalid`.
pub fn clear(policy: &str) -> Result<(), QosError> {
    if !is_ours(policy) {
        return Err(QosError::Invalid);
    }
    Session::open()?.delete_named(policy).map(|_| ())
}

/// Crash recovery: removes every ActiveStore policy named `G2Boost-*`. Returns how many.
pub fn clear_all_ours() -> Result<usize, QosError> {
    let s = Session::open()?;
    let ours = s.list()?;
    for (_, id) in &ours {
        s.delete(id)?;
    }
    Ok(ours.len())
}

/// Our policies in the ActiveStore.
pub fn list_ours() -> Result<Vec<QosPolicy>, QosError> {
    Ok(Session::open()?.list()?.into_iter().map(|(p, _)| p).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qos_exe_validation() {
        for ok in ["cs2.exe", "a.exe", "VALORANT-Win64-Shipping.exe", "Counter Strike 2.exe", "jogo.EXE"] {
            assert!(valid_exe(ok), "{ok}");
        }
        for bad in [
            "", ".exe", "cs2", "cs2.bat", r"C:\Games\cs2.exe", "../cs2.exe", "c:cs2.exe", "*.exe", "a?.exe", "\"a\".exe",
            "a<b.exe", "a|b.exe", " cs2.exe", "cs2.exe ", "a\nb.exe", "svchost.exe", "EXPLORER.EXE", "System", "system.exe",
            &format!("{}.exe", "a".repeat(61)),
        ] {
            assert!(!valid_exe(bad), "{bad:?}");
        }
        assert!(valid_exe(&format!("{}.exe", "a".repeat(60))));
    }

    #[test]
    fn qos_policy_name_sanitized() {
        assert_eq!(policy_name("cs2.exe"), "G2Boost-cs2.exe");
        assert_eq!(policy_name("Counter Strike 2.EXE"), "G2Boost-counter_strike_2.exe");
        assert_eq!(policy_name("Jogo-Ção_1.exe"), "G2Boost-jogo-__o_1.exe");
        for exe in ["cs2.exe", "Counter Strike 2.EXE", "Jogo-Ção_1.exe"] {
            assert!(is_ours(&policy_name(exe)));
        }
        for foreign in ["", "G2Boost-", "g2boost-cs2.exe", "Other-cs2.exe", "G2Boost-CS2.exe", "G2Boost-a\"b", r"G2Boost-a\b"] {
            assert!(!is_ours(foreign), "{foreign:?}");
        }
    }

    #[test]
    fn qos_object_path_escaped() {
        assert_eq!(object_path("{1D2C}"), r#"MSFT_NetQosPolicySettingData.InstanceID="{1D2C}""#);
        assert_eq!(object_path(r#"a"b\c"#), r#"MSFT_NetQosPolicySettingData.InstanceID="a\"b\\c""#);
    }

    #[test]
    fn qos_variant_type_follows_cimtype() {
        assert_eq!(int_vt(CIM_SINT8.0), VT_I2.0);
        assert_eq!(int_vt(CIM_UINT8.0), VT_UI1.0);
        assert_eq!(int_vt(windows::Win32::System::Wmi::CIM_UINT32.0), VT_I4.0);
    }

    #[test]
    fn qos_rejects_before_wmi() {
        assert!(matches!(set_game(r"C:\x\cs2.exe"), Err(QosError::Invalid)));
        assert!(matches!(set_game("svchost.exe"), Err(QosError::Invalid)));
        assert!(matches!(clear("SomeoneElse"), Err(QosError::Invalid)));
    }

    /// The class definition is readable without admin: pin the schema facts this module relies on.
    #[test]
    fn qos_class_schema() {
        let s = Session::open().unwrap(); // must outlive `c` (it owns the COM apartment)
        let c = s.class().unwrap();
        let cimtype = |prop: &str| {
            let k = wide(prop);
            let (mut t, mut v) = (0i32, Var(VARIANT::default()));
            // SAFETY: class Get into locals we own.
            unsafe { c.Get(PCWSTR(k.as_ptr()), 0, &mut v.0, Some(&mut t), None) }.unwrap();
            t
        };
        assert_eq!(cimtype("DSCPAction"), CIM_SINT8.0);
        assert_eq!(cimtype("NetworkProfile"), windows::Win32::System::Wmi::CIM_UINT32.0);
        assert_eq!(cimtype("Name"), windows::Win32::System::Wmi::CIM_STRING.0);
        let is_key = |prop: &str| {
            let (k, kk) = (wide(prop), wide("key"));
            let mut v = Var(VARIANT::default());
            // SAFETY: qualifier lookups into locals we own.
            unsafe { c.GetPropertyQualifierSet(PCWSTR(k.as_ptr())).unwrap().Get(PCWSTR(kk.as_ptr()), 0, &mut v.0, std::ptr::null_mut()) }.is_ok()
        };
        assert!(is_key("InstanceID"));
        assert!(!is_key("Name"));
    }

    /// Live round-trip. Elevated: create, list, delete, verify gone. Not elevated: only
    /// `set_game` → NeedsAdmin. Output also goes to `%TEMP%\g2boost-live\qos.txt`.
    #[test]
    #[ignore = "live: touches the QoS ActiveStore when elevated"]
    fn qos_live_roundtrip() {
        use std::fmt::Write as _;
        let mut log = String::new();
        let exe = "g2boost-qos-test.exe";
        let elevated = crate::system::is_elevated();
        let _ = writeln!(log, "elevated: {elevated}");
        let result = std::panic::catch_unwind(|| {
            let mut log = String::new();
            if !elevated {
                let r = set_game(exe);
                let _ = writeln!(log, "set_game: {r:?}");
                assert!(matches!(r, Err(QosError::NeedsAdmin)));
                return log;
            }
            let name = set_game(exe).expect("set_game");
            let _ = writeln!(log, "created: {name}");
            let listed = list_ours().expect("list_ours");
            let _ = writeln!(log, "listed: {listed:?}");
            let mine = listed.iter().find(|p| p.name == name).expect("policy listed");
            assert_eq!(mine.app, exe);
            assert_eq!(mine.dscp, 46);
            clear(&name).expect("clear");
            let after = list_ours().expect("list_ours after");
            let _ = writeln!(log, "after clear: {after:?}");
            assert!(after.iter().all(|p| p.name != name));
            clear(&name).expect("clear is idempotent");
            let _ = writeln!(log, "clear_all_ours: {:?}", clear_all_ours());
            log
        });
        if elevated {
            let _ = writeln!(log, "cleanup: {:?}", clear(&policy_name(exe))); // even after a failed assert
        }
        match &result {
            Ok(l) => log.push_str(l),
            Err(p) => {
                let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()));
                let _ = writeln!(log, "panic: {}", msg.unwrap_or_default());
            }
        }
        let _ = writeln!(log, "result: {}", if result.is_ok() { "PASS" } else { "FAIL" });
        let dir = std::env::temp_dir().join("g2boost-live");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("qos.txt"), &log);
        println!("{log}");
        if let Err(p) = result {
            std::panic::resume_unwind(p);
        }
    }
}
