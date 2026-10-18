//! Real-time monitor: CPU / GPU / RAM / disk / network, one sample per second.
//! PDH counters by English name (locale-independent), `CallNtPowerInformation` for the
//! clock, `GetIfTable2` deltas for the network, NVML (user-mode, loaded dynamically) for
//! the GPU temperature. CPU temperature is deliberately absent: it needs a kernel driver.

use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::null_mut;
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, IF_TYPE_SOFTWARE_LOOPBACK, MIB_IF_TABLE2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::System::Performance::{
    PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhGetFormattedCounterValue,
    PdhOpenQueryW,
};
use windows::Win32::System::Power::{CallNtPowerInformation, PROCESSOR_POWER_INFORMATION, ProcessorInformation};
use windows::Win32::System::Threading::{ALL_PROCESSOR_GROUPS, GetActiveProcessorCount};
use windows::core::PCWSTR;

use crate::nvml::{self, Nvml};
use crate::registry::{now_unix, wide};
use crate::system;

#[derive(Debug, Clone, Serialize, Default)]
pub struct Sample {
    pub ts: u64,
    pub cpu_pct: f32,
    pub cpu_mhz: u32,
    /// Package power from the RAPL energy meter (PDH `Energy Meter`); None when the counter is absent.
    pub cpu_power_w: Option<f32>,
    /// From HWiNFO / LibreHardwareMonitor when running (no driver of ours).
    pub cpu_temp_c: Option<f64>,
    pub gpu_pct: f32,
    pub gpu_mem_bytes: u64,
    pub gpu_temp_c: Option<u32>,
    pub gpu_core_mhz: Option<u32>,
    pub gpu_mem_mhz: Option<u32>,
    pub gpu_power_w: Option<f32>,
    pub ram_used: u64,
    pub ram_total: u64,
    pub disk_pct: f32,
    pub disk_bytes_s: u64,
    pub net_rx_bytes_s: u64,
    pub net_tx_bytes_s: u64,
}

struct Counter(PDH_HCOUNTER);

fn add_counter(query: PDH_HQUERY, path: &str) -> Option<Counter> {
    let w = wide(path);
    let mut c = PDH_HCOUNTER::default();
    // SAFETY: valid query handle and null-terminated English counter path.
    let rc = unsafe { PdhAddEnglishCounterW(query, PCWSTR(w.as_ptr()), 0, &mut c) };
    (rc == 0).then_some(Counter(c))
}

fn value(c: &Option<Counter>) -> Option<f64> {
    let c = c.as_ref()?;
    let mut v = PDH_FMT_COUNTERVALUE::default();
    // SAFETY: sized out-param; non-zero status (e.g. PDH_INVALID_DATA on the first sample) → None.
    let rc = unsafe { PdhGetFormattedCounterValue(c.0, PDH_FMT_DOUBLE, None, &mut v) };
    // SAFETY: doubleValue is the active union member after PDH_FMT_DOUBLE.
    (rc == 0).then_some(unsafe { v.Anonymous.doubleValue })
}

/// All instances of a wildcard counter as (instance name, value).
fn values(c: &Option<Counter>) -> Vec<(String, f64)> {
    let Some(c) = c.as_ref() else {
        return Vec::new();
    };
    let mut size = 0u32;
    let mut count = 0u32;
    // SAFETY: size probe.
    let rc = unsafe { PdhGetFormattedCounterArrayW(c.0, PDH_FMT_DOUBLE, &mut size, &mut count, None) };
    if rc != PDH_MORE_DATA || size == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u64; (size as usize).div_ceil(8) + 1];
    // SAFETY: buffer of `size` bytes (rounded up), aligned for the item array that starts at offset 0.
    let rc = unsafe {
        PdhGetFormattedCounterArrayW(c.0, PDH_FMT_DOUBLE, &mut size, &mut count, Some(buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>()))
    };
    if rc != 0 {
        return Vec::new();
    }
    // SAFETY: `count` items were written at the start of the buffer; names point inside the same buffer.
    let items = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>(), count as usize) };
    items
        .iter()
        .map(|it| {
            // SAFETY: szName is a NUL-terminated string inside `buf`.
            let name = unsafe { it.szName.to_string() }.unwrap_or_default();
            // SAFETY: doubleValue active after PDH_FMT_DOUBLE.
            let v = unsafe { it.FmtValue.Anonymous.doubleValue };
            (name, v)
        })
        .collect()
}

/* ---------------- monitor ---------------- */

pub struct Monitor {
    query: PDH_HQUERY,
    cpu: Option<Counter>,
    cpu_perf: Option<Counter>,
    cpu_freq: Option<Counter>,
    cpu_power: Option<Counter>,
    gpu_util: Option<Counter>,
    gpu_mem: Option<Counter>,
    disk_time: Option<Counter>,
    disk_bytes: Option<Counter>,
    last_net: Option<(Instant, u64, u64)>,
    primed: bool,
    ext_tick: u32,
    /// Sensor reads are skipped until this tick after a slow one (hung provider).
    ext_skip_until: u32,
    cpu_temp: Option<f64>,
}

// SAFETY: PDH handles are only used from the thread that owns the Monitor.
unsafe impl Send for Monitor {}

impl Monitor {
    pub fn new() -> Option<Self> {
        let mut query = PDH_HQUERY::default();
        // SAFETY: real-time query (no data source).
        if unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } != 0 {
            return None;
        }
        let cpu = add_counter(query, r"\Processor Information(_Total)\% Processor Utility")
            .or_else(|| add_counter(query, r"\Processor(_Total)\% Processor Time"));
        let mut m = Self {
            query,
            cpu,
            // Effective clock = nominal frequency × % performance (turbo shows > 100 %), like Task Manager.
            cpu_perf: add_counter(query, r"\Processor Information(_Total)\% Processor Performance"),
            cpu_freq: add_counter(query, r"\Processor Information(_Total)\Processor Frequency"),
            cpu_power: add_counter(query, r"\Energy Meter(RAPL_Package0_PKG)\Power"),
            gpu_util: add_counter(query, r"\GPU Engine(*)\Utilization Percentage"),
            gpu_mem: add_counter(query, r"\GPU Adapter Memory(*)\Dedicated Usage"),
            disk_time: add_counter(query, r"\PhysicalDisk(_Total)\% Disk Time"),
            disk_bytes: add_counter(query, r"\PhysicalDisk(_Total)\Disk Bytes/sec"),
            last_net: None,
            primed: false,
            ext_tick: 0,
            ext_skip_until: 0,
            cpu_temp: None,
        };
        // Rate counters need two collections; prime now so the first real sample is meaningful.
        m.collect();
        m.net_counters();
        Some(m)
    }

    fn collect(&mut self) {
        // SAFETY: valid query.
        let _ = unsafe { PdhCollectQueryData(self.query) };
        self.primed = true;
    }

    /// Bytes in/out summed over up, non-loopback interfaces.
    fn net_counters(&mut self) -> (u64, u64) {
        let mut table: *mut MIB_IF_TABLE2 = null_mut();
        // SAFETY: GetIfTable2 allocates the table; FreeMibTable releases it.
        unsafe {
            if GetIfTable2(&mut table) != ERROR_SUCCESS || table.is_null() {
                return (0, 0);
            }
            let n = (*table).NumEntries as usize;
            let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), n);
            let (mut rx, mut tx) = (0u64, 0u64);
            for r in rows {
                if r.OperStatus == IfOperStatusUp && r.Type != IF_TYPE_SOFTWARE_LOOPBACK {
                    rx += r.InOctets;
                    tx += r.OutOctets;
                }
            }
            FreeMibTable(table.cast::<c_void>());
            (rx, tx)
        }
    }

    fn cpu_mhz() -> u32 {
        // SAFETY: one PROCESSOR_POWER_INFORMATION per logical processor.
        unsafe {
            let n = GetActiveProcessorCount(ALL_PROCESSOR_GROUPS).max(1) as usize;
            let mut buf = vec![PROCESSOR_POWER_INFORMATION::default(); n];
            let len = (n * size_of::<PROCESSOR_POWER_INFORMATION>()) as u32;
            let st = CallNtPowerInformation(ProcessorInformation, None, 0, Some(buf.as_mut_ptr().cast::<c_void>()), len);
            if st.is_err() {
                return 0;
            }
            buf.iter().map(|p| p.CurrentMhz).max().unwrap_or(0)
        }
    }

    pub fn sample(&mut self) -> Sample {
        self.collect();
        let now = Instant::now();
        let (rx, tx) = self.net_counters();
        let (net_rx, net_tx) = match self.last_net {
            Some((t0, rx0, tx0)) => {
                let dt = now.duration_since(t0).as_secs_f64().max(0.001);
                (((rx.saturating_sub(rx0)) as f64 / dt) as u64, ((tx.saturating_sub(tx0)) as f64 / dt) as u64)
            }
            None => (0, 0),
        };
        self.last_net = Some((now, rx, tx));

        // GPU %: like Task Manager, the busiest engine type (3D, Copy, VideoDecode…) summed over processes.
        let mut per_engine: HashMap<String, f64> = HashMap::new();
        for (name, v) in values(&self.gpu_util) {
            let eng = name.rsplit("engtype_").next().unwrap_or("?").to_string();
            *per_engine.entry(eng).or_default() += v;
        }
        let gpu_pct = per_engine.values().cloned().fold(0.0, f64::max).min(100.0) as f32;
        let gpu_mem_bytes = values(&self.gpu_mem).iter().map(|(_, v)| *v as u64).sum();
        let mem = system::memory_status();
        let nv = nvml::get();
        if self.ext_tick.is_multiple_of(5) && self.ext_tick >= self.ext_skip_until {
            let t0 = Instant::now();
            self.cpu_temp = crate::sensors::read().and_then(|x| x.cpu_temp_c);
            // A slow provider (hung LHM) is retried once a minute instead of stalling every 5 s.
            if t0.elapsed() > Duration::from_millis(800) {
                self.ext_skip_until = self.ext_tick.wrapping_add(60);
            }
        }
        self.ext_tick = self.ext_tick.wrapping_add(1);

        Sample {
            ts: now_unix(),
            cpu_pct: value(&self.cpu).unwrap_or(0.0).clamp(0.0, 100.0) as f32,
            cpu_temp_c: self.cpu_temp,
            cpu_mhz: match (value(&self.cpu_freq), value(&self.cpu_perf)) {
                (Some(f), Some(p)) if f > 0.0 && p > 0.0 => (f * p / 100.0).round() as u32,
                _ => Self::cpu_mhz(),
            },
            // The counter reports milliwatts on current builds; tolerate a watts-scaled provider too.
            cpu_power_w: value(&self.cpu_power).filter(|v| *v > 0.0).map(|v| (if v > 500.0 { v / 1000.0 } else { v }) as f32),
            gpu_pct,
            gpu_mem_bytes,
            gpu_temp_c: nv.and_then(Nvml::temperature_c),
            gpu_core_mhz: nv.and_then(|n| n.clock_mhz(0)),
            gpu_mem_mhz: nv.and_then(|n| n.clock_mhz(2)),
            gpu_power_w: nv.and_then(Nvml::power_w),
            ram_used: mem.as_ref().map(|m| m.total_bytes - m.avail_bytes).unwrap_or(0),
            ram_total: mem.as_ref().map(|m| m.total_bytes).unwrap_or(0),
            disk_pct: value(&self.disk_time).unwrap_or(0.0).clamp(0.0, 100.0) as f32,
            disk_bytes_s: value(&self.disk_bytes).unwrap_or(0.0).max(0.0) as u64,
            net_rx_bytes_s: net_rx,
            net_tx_bytes_s: net_tx,
        }
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        // SAFETY: closes the query (and its counters) exactly once.
        unsafe {
            let _ = PdhCloseQuery(self.query);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_samples_are_in_range() {
        let mut m = Monitor::new().expect("PDH query");
        std::thread::sleep(std::time::Duration::from_millis(400));
        let _ = m.sample();
        std::thread::sleep(std::time::Duration::from_millis(400));
        let s = m.sample();
        assert!((0.0..=100.0).contains(&s.cpu_pct));
        assert!((0.0..=100.0).contains(&s.gpu_pct));
        assert!((0.0..=100.0).contains(&s.disk_pct));
        assert!(s.ram_total > 0 && s.ram_used <= s.ram_total);
        assert!(s.cpu_mhz > 0, "CallNtPowerInformation gives the current clock");
        assert!(s.ts > 0);
    }
}
