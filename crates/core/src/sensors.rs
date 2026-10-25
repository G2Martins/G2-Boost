//! Motherboard/CPU sensors **without a driver of ours**: when the user already runs HWiNFO
//! (shared memory enabled) or LibreHardwareMonitor (WMI), read their published values.
//! Both are read-only views of data those tools expose on purpose. Nothing is installed,
//! nothing is written. See Docs/security/kernel-driver-options.md for why we ship no driver.

use serde::Serialize;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Memory::{
    FILE_MAP_READ, MEMORY_BASIC_INFORMATION, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile,
    VirtualQuery,
};
use windows::core::w;

use crate::wmi;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Hwinfo,
    LibreHardwareMonitor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Temperature,
    Voltage,
    Fan,
    Power,
    Clock,
    Other,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Reading {
    /// Device the reading belongs to ("CPU [#0]: Intel Core i5-10400", "/intelcpu/0").
    pub sensor: String,
    pub label: String,
    pub kind: Kind,
    pub value: f64,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct External {
    pub source: Source,
    pub cpu_temp_c: Option<f64>,
    pub readings: Vec<Reading>,
}

/* ---------------- HWiNFO shared memory (HWiNFO_SENS_SM2) ---------------- */

const HWINFO_SIG: u32 = 0x5369_5748; // "HWiS"
const HEADER_LEN: usize = 44;
const LABEL: usize = 128;
const UNIT: usize = 16;
const READING_MIN: usize = 12 + LABEL * 2 + UNIT + 8 * 4; // 316
const SENSOR_MIN: usize = 8 + LABEL * 2; // 264

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn f64_at(b: &[u8], off: usize) -> Option<f64> {
    let s = b.get(off..off + 8)?;
    Some(f64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}

fn cstr(b: &[u8], off: usize, len: usize) -> String {
    let Some(s) = b.get(off..off + len) else {
        return String::new();
    };
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    // HWiNFO writes ANSI (code page of the machine); lossy UTF-8 keeps ASCII labels intact.
    String::from_utf8_lossy(&s[..end]).trim().to_string()
}

/// Parse an `HWiNFO_SENSORS_SHARED_MEM2` blob (bounds-checked; sizes come from the header).
pub(crate) fn parse_hwinfo(b: &[u8]) -> Option<Vec<Reading>> {
    if b.len() < HEADER_LEN || u32_at(b, 0)? != HWINFO_SIG {
        return None;
    }
    let (s_off, s_size, s_num) = (u32_at(b, 20)? as usize, u32_at(b, 24)? as usize, u32_at(b, 28)? as usize);
    let (r_off, r_size, r_num) = (u32_at(b, 32)? as usize, u32_at(b, 36)? as usize, u32_at(b, 40)? as usize);
    if s_size < SENSOR_MIN || r_size < READING_MIN || s_num > 4096 || r_num > 65_536 {
        return None;
    }
    let sensors: Vec<String> = (0..s_num)
        .map(|i| {
            let base = s_off + i * s_size;
            let user = cstr(b, base + 8 + LABEL, LABEL);
            if user.is_empty() { cstr(b, base + 8, LABEL) } else { user }
        })
        .collect();
    let mut out = Vec::new();
    for i in 0..r_num {
        let base = r_off + i * r_size;
        if base + READING_MIN > b.len() {
            break;
        }
        let kind = match u32_at(b, base)? {
            1 => Kind::Temperature,
            2 => Kind::Voltage,
            3 => Kind::Fan,
            5 => Kind::Power,
            6 => Kind::Clock,
            _ => Kind::Other,
        };
        let sensor_idx = u32_at(b, base + 4)? as usize;
        let user = cstr(b, base + 12 + LABEL, LABEL);
        let label = if user.is_empty() { cstr(b, base + 12, LABEL) } else { user };
        out.push(Reading {
            sensor: sensors.get(sensor_idx).cloned().unwrap_or_default(),
            label,
            kind,
            value: f64_at(b, base + 12 + LABEL * 2 + UNIT)?,
            unit: cstr(b, base + 12 + LABEL * 2, UNIT),
        });
    }
    Some(out)
}

fn read_hwinfo() -> Option<Vec<Reading>> {
    // SAFETY: read-only view of a named section; size bounded by VirtualQuery; unmapped below.
    unsafe {
        let h = OpenFileMappingW(FILE_MAP_READ.0, false, w!("Global\\HWiNFO_SENS_SM2")).ok()?;
        let view = MapViewOfFile(h, FILE_MAP_READ, 0, 0, 0);
        let _ = CloseHandle(h);
        if view.Value.is_null() {
            return None;
        }
        let mut mbi = MEMORY_BASIC_INFORMATION::default();
        let got = VirtualQuery(Some(view.Value), &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>());
        let size = if got == 0 { 0 } else { mbi.RegionSize.min(64 * 1024 * 1024) };
        let bytes = std::slice::from_raw_parts(view.Value as *const u8, size).to_vec();
        let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: view.Value });
        parse_hwinfo(&bytes)
    }
}

/* ---------------- LibreHardwareMonitor WMI ---------------- */

fn read_lhm() -> Option<Vec<Reading>> {
    // Bounded: a hung LHM must not hang the 1 s monitor nor the MCP server.
    let rows = wmi::query_timeout(
        r"ROOT\LibreHardwareMonitor",
        "SELECT Name, SensorType, Value, Parent FROM Sensor",
        &["Name", "SensorType", "Value", "Parent"],
        std::time::Duration::from_millis(1500),
    )
    .ok()?;
    if rows.is_empty() {
        return None;
    }
    Some(
        rows.iter()
            .filter_map(|r| {
                let (kind, unit) = match r.get("SensorType")?.as_str()? {
                    "Temperature" => (Kind::Temperature, "°C"),
                    "Voltage" => (Kind::Voltage, "V"),
                    "Fan" => (Kind::Fan, "RPM"),
                    "Power" => (Kind::Power, "W"),
                    "Clock" => (Kind::Clock, "MHz"),
                    _ => return None,
                };
                Some(Reading {
                    sensor: r.get("Parent").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    label: r.get("Name")?.as_str()?.to_string(),
                    kind,
                    value: r.get("Value")?.as_f64()?,
                    unit: unit.to_string(),
                })
            })
            .collect(),
    )
}

/* ---------------- selection ---------------- */

/// Best CPU temperature among the readings (package / Tctl / hottest core).
pub(crate) fn pick_cpu_temp(readings: &[Reading]) -> Option<f64> {
    const PREFERRED: &[&str] = &["cpu package", "core (tctl/tdie)", "cpu (tctl/tdie)", "core max", "cpu die (average)", "core average"];
    let cpu_temps: Vec<&Reading> = readings
        .iter()
        .filter(|r| r.kind == Kind::Temperature && (r.sensor.to_ascii_lowercase().contains("cpu") || r.label.to_ascii_lowercase().contains("cpu")))
        .filter(|r| (1.0..=125.0).contains(&r.value))
        .collect();
    for want in PREFERRED {
        if let Some(r) = cpu_temps.iter().find(|r| r.label.to_ascii_lowercase() == *want) {
            return Some(r.value);
        }
    }
    None
}

/// Live readings from whichever tool is running (HWiNFO first), else `None`.
pub fn read() -> Option<External> {
    let (source, all) = match read_hwinfo() {
        Some(r) if !r.is_empty() => (Source::Hwinfo, r),
        _ => (Source::LibreHardwareMonitor, read_lhm()?),
    };
    let cpu_temp_c = pick_cpu_temp(&all);
    let mut readings: Vec<Reading> = all.into_iter().filter(|r| matches!(r.kind, Kind::Temperature | Kind::Fan | Kind::Power | Kind::Voltage)).collect();
    readings.truncate(80);
    Some(External { source, cpu_temp_c, readings })
}

/* ---------------- everything the tool publishes (hardware monitor page) ---------------- */

/// LibreHardwareMonitor device names by identifier (`/intelcpu/0` → `Intel Core i5-10400`).
fn lhm_names() -> std::collections::HashMap<String, String> {
    let rows = wmi::query_timeout(
        r"ROOT\LibreHardwareMonitor",
        "SELECT Name, Identifier FROM Hardware",
        &["Name", "Identifier"],
        std::time::Duration::from_millis(1500),
    )
    .unwrap_or_default();
    rows.iter()
        .filter_map(|r| Some((r.get("Identifier")?.as_str()?.to_string(), r.get("Name")?.as_str()?.to_string())))
        .collect()
}

/// Like `read_lhm` but with loads and currents too, and the devices' own names.
fn read_lhm_all() -> Option<Vec<Reading>> {
    let rows = wmi::query_timeout(
        r"ROOT\LibreHardwareMonitor",
        "SELECT Name, SensorType, Value, Parent FROM Sensor",
        &["Name", "SensorType", "Value", "Parent"],
        std::time::Duration::from_millis(1500),
    )
    .ok()?;
    if rows.is_empty() {
        return None;
    }
    let names = lhm_names();
    Some(
        rows.iter()
            .filter_map(|r| {
                let (kind, unit) = match r.get("SensorType")?.as_str()? {
                    "Temperature" => (Kind::Temperature, "°C"),
                    "Voltage" => (Kind::Voltage, "V"),
                    "Fan" => (Kind::Fan, "RPM"),
                    "Power" => (Kind::Power, "W"),
                    "Clock" => (Kind::Clock, "MHz"),
                    "Load" | "Control" | "Level" => (Kind::Other, "%"),
                    "Current" => (Kind::Other, "A"),
                    _ => return None,
                };
                let parent = r.get("Parent").and_then(|v| v.as_str()).unwrap_or("").to_string();
                Some(Reading {
                    sensor: names.get(&parent).cloned().unwrap_or(parent),
                    label: r.get("Name")?.as_str()?.to_string(),
                    kind,
                    value: r.get("Value")?.as_f64()?,
                    unit: unit.to_string(),
                })
            })
            .collect(),
    )
}

/// Every reading of whichever tool is running (HWiNFO first), unfiltered: the hardware monitor
/// groups them by device. `None` when neither runs.
pub fn read_all() -> Option<(Source, Vec<Reading>)> {
    match read_hwinfo() {
        Some(r) if !r.is_empty() => Some((Source::Hwinfo, r)),
        _ => Some((Source::LibreHardwareMonitor, read_lhm_all()?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob() -> Vec<u8> {
        let (s_size, r_size) = (SENSOR_MIN, READING_MIN);
        let s_off = HEADER_LEN;
        let r_off = s_off + s_size;
        let mut b = vec![0u8; r_off + 2 * r_size];
        let put = |b: &mut Vec<u8>, off: usize, v: u32| b[off..off + 4].copy_from_slice(&v.to_le_bytes());
        put(&mut b, 0, HWINFO_SIG);
        put(&mut b, 20, s_off as u32);
        put(&mut b, 24, s_size as u32);
        put(&mut b, 28, 1);
        put(&mut b, 32, r_off as u32);
        put(&mut b, 36, r_size as u32);
        put(&mut b, 40, 2);
        let text = |b: &mut Vec<u8>, off: usize, t: &[u8]| b[off..off + t.len()].copy_from_slice(t);
        text(&mut b, s_off + 8, b"CPU [#0]: i5-10400");
        // reading 0: CPU Package 61.5 C
        put(&mut b, r_off, 1);
        text(&mut b, r_off + 12, b"CPU Package");
        text(&mut b, r_off + 12 + 256, b"C");
        b[r_off + 284..r_off + 292].copy_from_slice(&61.5f64.to_le_bytes());
        // reading 1: fan 1200 RPM
        let r1 = r_off + r_size;
        put(&mut b, r1, 3);
        text(&mut b, r1 + 12, b"CPU FAN");
        b[r1 + 284..r1 + 292].copy_from_slice(&1200f64.to_le_bytes());
        b
    }

    #[test]
    fn parses_hwinfo_blob_and_picks_cpu_temp() {
        let r = parse_hwinfo(&blob()).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].label, "CPU Package");
        assert_eq!(r[0].kind, Kind::Temperature);
        assert!(r[0].sensor.starts_with("CPU [#0]"));
        assert_eq!(r[1].kind, Kind::Fan);
        assert_eq!(pick_cpu_temp(&r), Some(61.5));
        assert!(parse_hwinfo(&[0u8; 10]).is_none());
        let mut dead = blob();
        dead[0..4].copy_from_slice(b"DEAD");
        assert!(parse_hwinfo(&dead).is_none(), "shared memory disabled marker");
        let _ = read(); // None unless HWiNFO/LHM is running; must not panic
    }
}
