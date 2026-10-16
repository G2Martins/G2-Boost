//! Windows System Assessment Tool (the old "Windows Experience Index", still computed on
//! Windows 10/11). Last scores come from WMI `Win32_WinSAT` (no admin); a new formal
//! assessment is started through the COM API (`CInitiateWinSAT`), never `winsat.exe`.

use serde::Serialize;
use windows::Win32::System::AssessmentTool::{CInitiateWinSAT, IInitiateWinSATAssessment, IWinSATInitiateEvents};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};

use crate::system;
use crate::tasksched::Com;
use crate::wmi;

#[derive(Debug, Clone, Serialize, Default)]
pub struct WinSat {
    /// 0 = unknown, 1 = valid, 2 = incoherent with hardware, 3 = never run, 4 = invalid.
    pub state: u32,
    pub base: Option<f32>,
    pub cpu: Option<f32>,
    pub memory: Option<f32>,
    pub graphics: Option<f32>,
    pub d3d: Option<f32>,
    pub disk: Option<f32>,
    /// "YYYY-MM-DD HH:MM" local, from `TimeTaken`.
    pub taken: Option<String>,
}

pub fn last() -> Result<WinSat, String> {
    let fields = ["WinSATAssessmentState", "WinSPRLevel", "CPUScore", "MemoryScore", "GraphicsScore", "D3DScore", "DiskScore", "TimeTaken"];
    let rows = wmi::query(r"ROOT\CIMV2", "SELECT * FROM Win32_WinSAT", &fields)?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(WinSat::default());
    };
    let f = |k: &str| row.get(k).and_then(|v| v.as_f64()).map(|v| v as f32).filter(|v| *v > 0.0);
    Ok(WinSat {
        state: row.get("WinSATAssessmentState").and_then(|v| v.as_f64()).unwrap_or(0.0) as u32,
        base: f("WinSPRLevel"),
        cpu: f("CPUScore"),
        memory: f("MemoryScore"),
        graphics: f("GraphicsScore"),
        d3d: f("D3DScore"),
        disk: f("DiskScore"),
        taken: row.get("TimeTaken").and_then(|v| v.as_str()).and_then(crate::restore::wmi_datetime),
    })
}

#[derive(Debug, thiserror::Error)]
pub enum WinSatError {
    #[error("needs_admin")]
    NeedsAdmin,
    #[error("com: {0}")]
    Com(String),
}

/// Start a formal assessment (CPU, memory, graphics, D3D, disk: 2–5 min, full screen tests).
/// Returns as soon as WinSAT accepted the request; poll [`last`] for the new scores.
pub fn start_formal() -> Result<(), WinSatError> {
    if !system::is_elevated() {
        return Err(WinSatError::NeedsAdmin);
    }
    let _com = Com::init().map_err(|e| WinSatError::Com(e.to_string()))?;
    // SAFETY: standard COM activation; no callbacks, no window.
    unsafe {
        let init: IInitiateWinSATAssessment =
            CoCreateInstance(&CInitiateWinSAT, None, CLSCTX_INPROC_SERVER).map_err(|e| WinSatError::Com(e.message()))?;
        init.InitiateFormalAssessment(None::<&IWinSATInitiateEvents>, None).map_err(|e| WinSatError::Com(e.message()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_scores_read_without_admin() {
        let w = last().unwrap();
        assert!(w.state <= 4);
        if w.state == 1 {
            assert!(w.base.is_some_and(|b| (1.0..=9.9).contains(&b)));
        }
        if !system::is_elevated() {
            assert!(matches!(start_formal(), Err(WinSatError::NeedsAdmin)));
        }
    }
}
