//! Which GPU page section applies: chosen by the DXGI vendor id. NVIDIA → driver profiles
//! (`nvapi`), AMD → beta (`adlx`), Intel only → basic info; hybrid laptops show the discrete
//! card (an NVIDIA or AMD GPU always wins over the Intel/AMD integrated one).

use serde::Serialize;

use crate::hardware::{self, Gpu};
use crate::nvapi::{self, DriverInfo};

#[derive(Debug, Clone, Serialize)]
pub struct Overview {
    /// Hardware adapters (DXGI, software adapter excluded).
    pub gpus: Vec<Gpu>,
    /// `nvidia` | `amd` | `intel`; `None` = no supported GPU (page hidden).
    pub section: Option<&'static str>,
    /// Driver version + branch from NVAPI (NVIDIA only).
    pub nvidia: Option<DriverInfo>,
}

pub fn section(gpus: &[Gpu]) -> Option<&'static str> {
    ["nvidia", "amd", "intel"].into_iter().find(|v| gpus.iter().any(|g| g.vendor == *v))
}

pub fn overview() -> Overview {
    let gpus = hardware::gpus();
    let section = section(&gpus);
    let nvidia = if section == Some("nvidia") { nvapi::driver() } else { None };
    Overview { gpus, section, nvidia }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(vendor: &'static str) -> Gpu {
        Gpu { name: vendor.into(), vram_bytes: 0, vendor, driver: None, pcie: None, power_limit_w: None }
    }

    #[test]
    fn discrete_vendor_wins_on_hybrid_machines() {
        assert_eq!(section(&[gpu("intel"), gpu("nvidia")]), Some("nvidia"));
        assert_eq!(section(&[gpu("amd"), gpu("nvidia")]), Some("nvidia")); // Ryzen APU + GeForce
        assert_eq!(section(&[gpu("intel"), gpu("amd")]), Some("amd"));
        assert_eq!(section(&[gpu("intel")]), Some("intel"));
        assert_eq!(section(&[gpu("other")]), None);
        assert_eq!(section(&[]), None);
    }
}
