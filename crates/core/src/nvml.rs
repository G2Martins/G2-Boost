//! NVIDIA NVML: `nvml.dll` ships with the driver and runs in user mode (no kernel driver of
//! ours). Loaded once, lazily; every call returns `None` on non-NVIDIA machines.
//! AMD (ADLX) and Intel (IGCL) equivalents are backlog.

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::OnceLock;

use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::core::PCWSTR;

use crate::registry::wide;

type FnInit = unsafe extern "C" fn() -> i32;
type FnHandle = unsafe extern "C" fn(u32, *mut *mut c_void) -> i32;
type FnDevSelU32 = unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> i32;
type FnDevU32 = unsafe extern "C" fn(*mut c_void, *mut u32) -> i32;
type FnStr = unsafe extern "C" fn(*mut u8, u32) -> i32;

pub struct Nvml {
    device: *mut c_void,
    temperature: FnDevSelU32,
    clock: FnDevSelU32,
    power_usage: Option<FnDevU32>,
    power_limit: Option<FnDevU32>,
    pcie_gen: Option<FnDevU32>,
    pcie_width: Option<FnDevU32>,
    driver: Option<FnStr>,
}

// SAFETY: NVML is documented thread-safe; the device handle is an opaque, process-wide token.
unsafe impl Send for Nvml {}
unsafe impl Sync for Nvml {}

static NVML: OnceLock<Option<Nvml>> = OnceLock::new();

/// Shared instance (loads on first use).
pub fn get() -> Option<&'static Nvml> {
    NVML.get_or_init(Nvml::load).as_ref()
}

macro_rules! sym {
    ($h:expr, $name:literal, $ty:ty) => {
        // SAFETY: the symbol has the documented NVML C signature.
        GetProcAddress($h, windows::core::s!($name)).map(|f| std::mem::transmute::<_, $ty>(f))
    };
}

impl Nvml {
    fn load() -> Option<Self> {
        let lib = wide("nvml.dll");
        // SAFETY: well-known NVIDIA user-mode library; symbols resolved by name with the C ABI.
        unsafe {
            let h = LoadLibraryW(PCWSTR(lib.as_ptr())).ok()?;
            let init: FnInit = sym!(h, "nvmlInit_v2", FnInit)?;
            let by_index: FnHandle = sym!(h, "nvmlDeviceGetHandleByIndex_v2", FnHandle)?;
            let temperature: FnDevSelU32 = sym!(h, "nvmlDeviceGetTemperature", FnDevSelU32)?;
            let clock: FnDevSelU32 = sym!(h, "nvmlDeviceGetClockInfo", FnDevSelU32)?;
            if init() != 0 {
                return None;
            }
            let mut device = null_mut();
            if by_index(0, &mut device) != 0 {
                return None;
            }
            Some(Self {
                device,
                temperature,
                clock,
                power_usage: sym!(h, "nvmlDeviceGetPowerUsage", FnDevU32),
                power_limit: sym!(h, "nvmlDeviceGetPowerManagementLimit", FnDevU32),
                pcie_gen: sym!(h, "nvmlDeviceGetCurrPcieLinkGeneration", FnDevU32),
                pcie_width: sym!(h, "nvmlDeviceGetCurrPcieLinkWidth", FnDevU32),
                driver: sym!(h, "nvmlSystemGetDriverVersion", FnStr),
            })
        }
    }

    fn dev_u32(&self, f: Option<FnDevU32>) -> Option<u32> {
        let f = f?;
        let mut v = 0u32;
        // SAFETY: valid device handle; out-param.
        (unsafe { f(self.device, &mut v) } == 0).then_some(v)
    }

    /// GPU die temperature (sensor 0).
    pub fn temperature_c(&self) -> Option<u32> {
        let mut t = 0u32;
        // SAFETY: valid device handle; NVML_TEMPERATURE_GPU = 0.
        (unsafe { (self.temperature)(self.device, 0, &mut t) } == 0).then_some(t)
    }

    /// `kind`: 0 = graphics, 1 = SM, 2 = memory.
    pub fn clock_mhz(&self, kind: u32) -> Option<u32> {
        let mut v = 0u32;
        // SAFETY: valid device handle; documented clock type ids.
        (unsafe { (self.clock)(self.device, kind, &mut v) } == 0).then_some(v)
    }

    pub fn power_w(&self) -> Option<f32> {
        self.dev_u32(self.power_usage).map(|mw| mw as f32 / 1000.0)
    }

    pub fn power_limit_w(&self) -> Option<u32> {
        self.dev_u32(self.power_limit).map(|mw| mw / 1000)
    }

    /// (generation, lanes), e.g. (3, 8) for "PCIe 3.0 x8".
    pub fn pcie(&self) -> Option<(u32, u32)> {
        Some((self.dev_u32(self.pcie_gen)?, self.dev_u32(self.pcie_width)?))
    }

    pub fn driver_version(&self) -> Option<String> {
        let f = self.driver?;
        let mut buf = [0u8; 80];
        // SAFETY: buffer of 80 bytes (NVML_SYSTEM_DRIVER_VERSION_BUFFER_SIZE).
        if unsafe { f(buf.as_mut_ptr(), buf.len() as u32) } != 0 {
            return None;
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..end]).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_or_is_absent_consistently() {
        // On an NVIDIA machine every accessor answers; elsewhere `get()` is None. Both are fine.
        if let Some(n) = get() {
            assert!(n.temperature_c().is_some_and(|t| (10..=120).contains(&t)));
            assert!(n.clock_mhz(0).is_some());
            assert!(n.driver_version().is_some_and(|v| !v.is_empty()));
        }
    }
}
