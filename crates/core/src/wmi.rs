//! Minimal WMI client over COM (`IWbemLocator`), no PowerShell. Used for System Restore
//! points and the last WinSAT assessment. Values come back as a flat, typed map per row.

use std::collections::HashMap;

use serde::Serialize;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, CoSetProxyBlanket, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Ole::{SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound};
use windows::Win32::System::Variant::{
    VARENUM, VARIANT, VT_ARRAY, VT_BOOL, VT_BSTR, VT_I2, VT_I4, VT_I8, VT_NULL, VT_R4, VT_R8, VT_TYPEMASK, VT_UI1, VT_UI2,
    VT_UI4, VT_UI8, VariantClear,
};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_GENERIC_FLAG_TYPE, WBEM_INFINITE,
    WbemLocator,
};
use windows::core::{BSTR, PCWSTR};

use crate::registry::wide;
use crate::tasksched::Com;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum WmiValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// Integer arrays (e.g. `Win32_DeviceGuard.SecurityServicesRunning`).
    IntList(Vec<i64>),
}

impl WmiValue {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            WmiValue::Int(i) => Some(*i as f64),
            WmiValue::Float(f) => Some(*f),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            WmiValue::Str(s) => Some(s),
            _ => None,
        }
    }
}

pub type Row = HashMap<String, WmiValue>;

fn variant_value(v: &VARIANT) -> WmiValue {
    // SAFETY: we read the union member that `vt` says is active.
    unsafe {
        let inner = &v.Anonymous.Anonymous;
        let u = &inner.Anonymous;
        if inner.vt.0 & VT_ARRAY.0 != 0 {
            let elem = VARENUM(inner.vt.0 & VT_TYPEMASK.0);
            let psa = u.parray;
            if psa.is_null() || !matches!(elem, VT_I4 | VT_UI4) {
                return WmiValue::Null;
            }
            let (Ok(lo), Ok(hi)) = (SafeArrayGetLBound(psa, 1), SafeArrayGetUBound(psa, 1)) else {
                return WmiValue::Null;
            };
            let mut out = Vec::new();
            for i in lo..=hi {
                let mut x = 0i32;
                if SafeArrayGetElement(psa, &i, (&mut x as *mut i32).cast()).is_ok() {
                    out.push(i64::from(x));
                }
            }
            return WmiValue::IntList(out);
        }
        match inner.vt {
            VT_BSTR => WmiValue::Str((*u.bstrVal).to_string()),
            VT_BOOL => WmiValue::Bool(u.boolVal.as_bool()),
            VT_I2 => WmiValue::Int(i64::from(u.iVal)),
            VT_I4 => WmiValue::Int(i64::from(u.lVal)),
            VT_I8 => WmiValue::Int(u.llVal),
            VT_UI1 => WmiValue::Int(i64::from(u.bVal)),
            VT_UI2 => WmiValue::Int(i64::from(u.uiVal)),
            VT_UI4 => WmiValue::Int(i64::from(u.ulVal)),
            VT_UI8 => WmiValue::Int(u.ullVal as i64),
            VT_R4 => WmiValue::Float(f64::from(u.fltVal)),
            VT_R8 => WmiValue::Float(u.dblVal),
            VT_NULL => WmiValue::Null,
            _ => WmiValue::Null,
        }
    }
}

/// `SELECT <fields> FROM <class>` in `namespace` (e.g. `ROOT\CIMV2`). Only `fields` are read.
pub fn query(namespace: &str, wql: &str, fields: &[&str]) -> Result<Vec<Row>, String> {
    let _com = Com::init().map_err(|e| e.to_string())?;
    // SAFETY: standard WMI client sequence; COM objects are released on drop.
    unsafe {
        let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).map_err(|e| e.message())?;
        let empty = BSTR::new();
        let services = locator
            .ConnectServer(&BSTR::from(namespace), &empty, &empty, &empty, 0, &empty, None)
            .map_err(|e| e.message())?;
        CoSetProxyBlanket(&services, RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE, PCWSTR::null(), RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE, None, EOAC_NONE)
            .map_err(|e| e.message())?;
        let flags = WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_FORWARD_ONLY.0 | WBEM_FLAG_RETURN_IMMEDIATELY.0);
        let rows = services.ExecQuery(&BSTR::from("WQL"), &BSTR::from(wql), flags, None).map_err(|e| e.message())?;
        let mut out = Vec::new();
        loop {
            let mut objs: [Option<IWbemClassObject>; 1] = [None];
            let mut returned = 0u32;
            if rows.Next(WBEM_INFINITE, &mut objs, &mut returned).is_err() || returned == 0 {
                break;
            }
            let Some(obj) = objs[0].take() else {
                break;
            };
            let mut row = Row::new();
            for field in fields {
                let name = wide(field);
                let mut v = VARIANT::default();
                if obj.Get(PCWSTR(name.as_ptr()), 0, &mut v, None, None).is_ok() {
                    row.insert((*field).to_string(), variant_value(&v));
                    let _ = VariantClear(&mut v);
                }
            }
            out.push(row);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cimv2_answers_without_admin() {
        let rows = query(r"ROOT\CIMV2", "SELECT Caption, BuildNumber FROM Win32_OperatingSystem", &["Caption", "BuildNumber"]).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0]["Caption"].as_str().is_some_and(|c| c.contains("Windows")));
        assert!(rows[0]["BuildNumber"].as_str().is_some());
        assert!(query(r"ROOT\NOPE", "SELECT * FROM X", &[]).is_err());
    }
}
