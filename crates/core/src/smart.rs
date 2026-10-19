//! Drive health (SMART) for physical disks, read-only and without spawning anything.
//!
//! Data sources, all plain `DeviceIoControl` on `\\.\PhysicalDriveN`:
//! - Identity (model, serial, bus): `IOCTL_STORAGE_QUERY_PROPERTY` / `StorageDeviceProperty`
//!   (no access rights needed, same as `hardware::disks`).
//! - NVMe: the SMART/Health Information log (page 0x02, 512 bytes) through
//!   `IOCTL_STORAGE_QUERY_PROPERTY` with a protocol-specific query (adapter first, device as
//!   fallback). Tried without rights first, then with read/write (admin).
//! - SATA: `SMART_RCV_DRIVE_DATA` (SMART READ DATA, feature 0xD0). Needs read+write access, i.e.
//!   admin; without it the drive is returned with `needs_admin: true`.
//! - USB bridges generally block both: status `unknown`.
//!
//! Only "read" commands are ever issued. Every buffer coming back from the drive is hostile
//! input: all offsets are bounds-checked and the parsers are pure functions over byte slices.
//! History lives in `registry::data_dir()/disk_health.json`, one sample per drive per day,
//! keyed by a hash of model+serial (the raw serial is never stored).

use std::collections::BTreeMap;
use std::ffi::c_void;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, HANDLE};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::IOCTL_STORAGE_QUERY_PROPERTY;
use windows::core::PCWSTR;

use crate::registry::{self, wide};

const GENERIC_RW: u32 = 0xC000_0000;
const SMART_RCV_DRIVE_DATA: u32 = 0x0007_C088;
const MAX_SAMPLES: usize = 120;
const DAY: u64 = 86_400;

#[derive(Debug, Error)]
pub enum SmartError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Unknown,
    Good,
    Caution,
    Bad,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub severity: Status,
    /// Stable id for the UI to translate (e.g. `reallocated`, `crc_errors`).
    pub code: String,
    /// Raw number behind the finding (count, percent or degrees).
    pub value: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NvmeHealth {
    pub critical_warning: u8,
    pub temperature_c: Option<i32>,
    pub available_spare: u8,
    pub spare_threshold: u8,
    pub percentage_used: u8,
    pub data_read_bytes: u64,
    pub data_written_bytes: u64,
    pub power_cycles: u64,
    pub power_on_hours: u64,
    pub unsafe_shutdowns: u64,
    pub media_errors: u64,
    pub error_log_entries: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtaAttr {
    pub id: u8,
    pub flags: u16,
    pub current: u8,
    pub worst: u8,
    pub raw: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveHealth {
    pub index: u32,
    pub model: String,
    /// `nvme`, `sata`, `usb` or `other`.
    pub bus: String,
    /// Hash of model+serial; identifies the drive in the history without exposing the serial.
    pub key: String,
    pub needs_admin: bool,
    pub nvme: Option<NvmeHealth>,
    /// Empty when not SATA or not readable.
    pub ata: Vec<AtaAttr>,
    pub status: Status,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub code: &'static str,
    pub from: u64,
    pub to: u64,
}

/* ---------------- pure parsers ---------------- */

fn le(buf: &[u8], off: usize, n: usize) -> Option<u128> {
    let s = buf.get(off..off.checked_add(n)?)?;
    Some(s.iter().rev().fold(0u128, |a, &b| (a << 8) | u128::from(b)))
}

fn le64(buf: &[u8], off: usize, n: usize) -> Option<u64> {
    le(buf, off, n).map(|v| u64::try_from(v).unwrap_or(u64::MAX))
}

/// NVMe SMART / Health Information log page (0x02).
pub fn parse_nvme_health(buf: &[u8]) -> Option<NvmeHealth> {
    if buf.len() < 512 {
        return None;
    }
    let kelvin = le64(buf, 1, 2)? as i32;
    let units = |off| le(buf, off, 16).map(|u| u64::try_from(u.saturating_mul(512_000)).unwrap_or(u64::MAX));
    Some(NvmeHealth {
        critical_warning: buf[0],
        temperature_c: (kelvin > 0).then(|| kelvin - 273),
        available_spare: buf[3],
        spare_threshold: buf[4],
        percentage_used: buf[5],
        data_read_bytes: units(32)?,
        data_written_bytes: units(48)?,
        power_cycles: le64(buf, 112, 16)?,
        power_on_hours: le64(buf, 128, 16)?,
        unsafe_shutdowns: le64(buf, 144, 16)?,
        media_errors: le64(buf, 160, 16)?,
        error_log_entries: le64(buf, 176, 16)?,
    })
}

/// ATA SMART READ DATA table: 2-byte revision then 30 entries of 12 bytes.
pub fn parse_ata_attributes(buf: &[u8]) -> Vec<AtaAttr> {
    if buf.len() < 2 + 30 * 12 {
        return Vec::new();
    }
    (0..30)
        .filter_map(|i| {
            let o = 2 + i * 12;
            let id = buf[o];
            (id != 0).then(|| AtaAttr {
                id,
                flags: u16::from_le_bytes([buf[o + 1], buf[o + 2]]),
                current: buf[o + 3],
                worst: buf[o + 4],
                raw: le64(buf, o + 5, 6).unwrap_or(0),
            })
        })
        .collect()
}

fn attr(d: &DriveHealth, id: u8) -> Option<u64> {
    d.ata.iter().find(|a| a.id == id).map(|a| a.raw)
}

fn ata_temp(d: &DriveHealth) -> Option<u64> {
    attr(d, 194).map(|r| r & 0xFF)
}

/// Turn raw numbers into a verdict. Pure.
pub fn assess(d: &DriveHealth) -> (Status, Vec<Finding>) {
    let mut f = Vec::new();
    let mut add = |severity, code: &str, value: u64| f.push(Finding { severity, code: code.into(), value });
    let have_data = d.nvme.is_some() || !d.ata.is_empty();
    if let Some(n) = &d.nvme {
        if n.critical_warning != 0 {
            add(Status::Bad, "critical_warning", u64::from(n.critical_warning));
        }
        if n.available_spare < n.spare_threshold {
            add(Status::Bad, "spare_below_threshold", u64::from(n.available_spare));
        }
        if n.percentage_used >= 90 {
            add(Status::Caution, "worn_out", u64::from(n.percentage_used));
        }
        if n.media_errors > 0 {
            add(Status::Caution, "media_errors", n.media_errors);
        }
        if let Some(t) = n.temperature_c.filter(|&t| t >= 60) {
            add(Status::Caution, "hot", t as u64);
        }
    }
    for (id, code) in [(5u8, "reallocated"), (197, "pending"), (198, "uncorrectable")] {
        if let Some(v) = attr(d, id).filter(|&v| v > 0) {
            add(if v >= 50 { Status::Bad } else { Status::Caution }, code, v);
        }
    }
    if let Some(v) = attr(d, 199).filter(|&v| v > 0) {
        add(Status::Caution, "crc_errors", v); // usually the SATA cable
    }
    if let Some(t) = ata_temp(d).filter(|&t| t >= 60) {
        add(Status::Caution, "hot", t);
    }
    let status = if !have_data { Status::Unknown } else { f.iter().map(|x| x.severity).max().unwrap_or(Status::Good) };
    (status, f)
}

/// Compare two samples of the same drive. Pure.
pub fn worsened(prev: &DriveHealth, now: &DriveHealth) -> Vec<Change> {
    let mut out = Vec::new();
    for (id, code) in [(5u8, "reallocated"), (197, "pending")] {
        if let (Some(a), Some(b)) = (attr(prev, id), attr(now, id))
            && b > a
        {
            out.push(Change { code, from: a, to: b });
        }
    }
    if let (Some(a), Some(b)) = (&prev.nvme, &now.nvme) {
        if b.percentage_used >= a.percentage_used.saturating_add(5) {
            out.push(Change { code: "percentage_used", from: a.percentage_used.into(), to: b.percentage_used.into() });
        }
        if b.available_spare.saturating_add(10) <= a.available_spare {
            out.push(Change { code: "available_spare", from: a.available_spare.into(), to: b.available_spare.into() });
        }
        if b.critical_warning & !a.critical_warning != 0 {
            out.push(Change { code: "critical_warning", from: a.critical_warning.into(), to: b.critical_warning.into() });
        }
    }
    out
}

/* ---------------- device access ---------------- */

struct Device(HANDLE);

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: handle from CreateFileW, closed once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

enum OpenErr {
    Missing,
    Denied,
}

fn open(index: u32, access: u32) -> Result<Device, OpenErr> {
    let w = wide(&format!(r"\\.\PhysicalDrive{index}"));
    // SAFETY: valid NUL-terminated path; no security attributes or template.
    unsafe { CreateFileW(PCWSTR(w.as_ptr()), access, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) }
        .map(Device)
        .map_err(|e| if e.code() == ERROR_ACCESS_DENIED.to_hresult() { OpenErr::Denied } else { OpenErr::Missing })
}

/// Ok(bytes returned) or Err(access denied?).
fn ioctl(dev: &Device, code: u32, input: &[u8], out: &mut [u8]) -> Result<usize, bool> {
    let mut ret = 0u32;
    // SAFETY: both buffers are valid for the lengths passed; synchronous call.
    unsafe {
        DeviceIoControl(
            dev.0,
            code,
            Some(input.as_ptr().cast::<c_void>()),
            input.len() as u32,
            Some(out.as_mut_ptr().cast::<c_void>()),
            out.len() as u32,
            Some(&mut ret),
            None,
        )
    }
    .map(|()| ret as usize)
    .map_err(|e| e.code() == ERROR_ACCESS_DENIED.to_hresult())
}

fn u32s(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn cstr_at(buf: &[u8], off: usize) -> String {
    if off == 0 || off >= buf.len() {
        return String::new();
    }
    let end = buf[off..].iter().position(|&b| b == 0).map_or(buf.len(), |p| off + p);
    String::from_utf8_lossy(&buf[off..end]).trim().to_string()
}

/// (model, serial, bus) from STORAGE_DEVICE_DESCRIPTOR.
fn identity(dev: &Device, index: u32) -> Option<(String, String, &'static str)> {
    let mut buf = vec![0u8; 2048];
    // PropertyId 0 = StorageDeviceProperty, QueryType 0 = standard, 4 bytes of AdditionalParameters.
    let n = ioctl(dev, IOCTL_STORAGE_QUERY_PROPERTY, &u32s(&[0, 0, 0]), &mut buf).ok()?;
    buf.truncate(n.min(2048));
    let off = |at| le64(&buf, at, 4).unwrap_or(0) as usize;
    let model = [cstr_at(&buf, off(12)), cstr_at(&buf, off(16))].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    let bus = match le64(&buf, 28, 4)? {
        17 => "nvme",
        11 => "sata",
        7 => "usb",
        _ => "other",
    };
    let model = if model.is_empty() { format!("Disk {index}") } else { model };
    Some((model, cstr_at(&buf, off(24)), bus))
}

fn fnv(parts: &[&str]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in parts.iter().flat_map(|p| p.bytes().chain(std::iter::once(0))) {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Ok(Some) = parsed, Ok(None) = drive refused, Err = access denied.
fn read_nvme(dev: &Device) -> Result<Option<NvmeHealth>, ()> {
    let mut denied = false;
    for property_id in [49u32, 50] {
        // header (PropertyId, QueryType) + STORAGE_PROTOCOL_SPECIFIC_DATA:
        // ProtocolType NVMe=3, DataType log page=2, log id 2, sub 0, offset 40, length 512, 4 reserved.
        let q = u32s(&[property_id, 0, 3, 2, 2, 0, 40, 512, 0, 0, 0, 0]);
        let mut out = vec![0u8; 1024];
        match ioctl(dev, IOCTL_STORAGE_QUERY_PROPERTY, &q, &mut out) {
            Ok(n) => {
                // STORAGE_PROTOCOL_DATA_DESCRIPTOR: 8-byte header, then the specific-data struct.
                let (Some(off), Some(len)) = (le64(&out, 24, 4), le64(&out, 28, 4)) else { continue };
                let (Some(start), true) = (8usize.checked_add(off as usize), len >= 512) else { continue };
                let Some(end) = start.checked_add(512).filter(|&e| e <= n.min(out.len())) else { continue };
                if let Some(h) = parse_nvme_health(&out[start..end]) {
                    return Ok(Some(h));
                }
            }
            Err(d) => denied |= d,
        }
    }
    if denied { Err(()) } else { Ok(None) }
}

fn read_ata(dev: &Device) -> Option<Vec<AtaAttr>> {
    // SENDCMDINPARAMS: cBufferSize, IDEREGS{features 0xD0, count 1, sector 1, cyl 0x4F/0xC2, head 0xA0, cmd 0xB0}, drive 0, pads.
    let mut inp = vec![0u8; 32];
    inp[4..12].copy_from_slice(&[0xD0, 1, 1, 0x4F, 0xC2, 0xA0, 0xB0, 0]);
    let mut out = vec![0u8; 16 + 512];
    let n = ioctl(dev, SMART_RCV_DRIVE_DATA, &inp, &mut out).ok()?;
    // SENDCMDOUTPARAMS: cBufferSize(4) + DRIVERSTATUS(12), then the 512-byte table.
    if n < 16 + 512 || out[4] != 0 {
        return None;
    }
    let v = parse_ata_attributes(&out[16..16 + 512]);
    (!v.is_empty()).then_some(v)
}

fn read_drive(index: u32) -> Option<DriveHealth> {
    let probe = open(index, 0).ok()?;
    let (model, serial, bus) = identity(&probe, index)?;
    let mut d = DriveHealth {
        index,
        key: fnv(&[&model, &serial]),
        model,
        bus: bus.into(),
        needs_admin: false,
        nvme: None,
        ata: Vec::new(),
        status: Status::Unknown,
        findings: Vec::new(),
    };
    match bus {
        "nvme" => {
            let r = match read_nvme(&probe) {
                Err(()) => match open(index, GENERIC_RW) {
                    Ok(rw) => read_nvme(&rw),
                    Err(_) => Err(()),
                },
                ok => ok,
            };
            match r {
                Ok(h) => d.nvme = h,
                Err(()) => d.needs_admin = true,
            }
        }
        "sata" => match open(index, GENERIC_RW) {
            Ok(rw) => d.ata = read_ata(&rw).unwrap_or_default(),
            Err(OpenErr::Denied) => d.needs_admin = true,
            Err(OpenErr::Missing) => {}
        },
        _ => {}
    }
    (d.status, d.findings) = assess(&d);
    Some(d)
}

/// Health of every present physical drive (indexes 0..16). Never fails as a whole.
pub fn health() -> Vec<DriveHealth> {
    (0..16).filter_map(read_drive).collect()
}

/* ---------------- history ---------------- */

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Sample {
    day: u64,
    health: DriveHealth,
}

type History = BTreeMap<String, Vec<Sample>>;

fn history_path() -> PathBuf {
    registry::data_dir().join("disk_health.json")
}

fn load() -> History {
    std::fs::read(history_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Most recent stored sample of a drive (by `DriveHealth::key`), for `worsened`.
pub fn last_sample(key: &str) -> Option<DriveHealth> {
    load().get(key)?.last().map(|s| s.health.clone())
}

fn try_record(now: u64, drives: &[DriveHealth]) -> Result<(), SmartError> {
    let mut h = load();
    let day = now / DAY;
    for d in drives.iter().filter(|d| d.nvme.is_some() || !d.ata.is_empty()) {
        let v = h.entry(d.key.clone()).or_default();
        let sample = Sample { day, health: d.clone() };
        match v.last_mut() {
            Some(last) if last.day == day => *last = sample,
            _ => v.push(sample),
        }
        if v.len() > MAX_SAMPLES {
            v.drain(..v.len() - MAX_SAMPLES);
        }
    }
    let p = history_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&h)?)?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

/// Best-effort: keep at most one sample per drive per day (120 per drive). Errors are ignored,
/// history is a convenience.
pub fn record(now: u64, drives: &[DriveHealth]) {
    let _ = try_record(now, drives);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(buf: &mut [u8], off: usize, v: u128, n: usize) {
        buf[off..off + n].copy_from_slice(&v.to_le_bytes()[..n]);
    }

    fn nvme_page() -> Vec<u8> {
        let mut b = vec![0u8; 512];
        b[0] = 0;
        put(&mut b, 1, 310, 2);
        b[3] = 100;
        b[4] = 10;
        b[5] = 3;
        put(&mut b, 32, 1000, 16);
        put(&mut b, 48, 2000, 16);
        put(&mut b, 112, 55, 16);
        put(&mut b, 128, 1234, 16);
        put(&mut b, 144, 7, 16);
        put(&mut b, 160, 0, 16);
        put(&mut b, 176, 9, 16);
        b
    }

    fn drive() -> DriveHealth {
        DriveHealth {
            index: 0,
            model: "X".into(),
            bus: "sata".into(),
            key: "k".into(),
            needs_admin: false,
            nvme: None,
            ata: Vec::new(),
            status: Status::Unknown,
            findings: Vec::new(),
        }
    }

    fn ata(pairs: &[(u8, u64)]) -> Vec<AtaAttr> {
        pairs.iter().map(|&(id, raw)| AtaAttr { id, flags: 0, current: 100, worst: 100, raw }).collect()
    }

    #[test]
    fn nvme_parse() {
        let h = parse_nvme_health(&nvme_page()).unwrap();
        assert_eq!(h.temperature_c, Some(37));
        assert_eq!((h.available_spare, h.spare_threshold, h.percentage_used), (100, 10, 3));
        assert_eq!(h.data_read_bytes, 512_000_000);
        assert_eq!(h.data_written_bytes, 1_024_000_000);
        assert_eq!((h.power_cycles, h.power_on_hours, h.unsafe_shutdowns), (55, 1234, 7));
        assert_eq!((h.media_errors, h.error_log_entries), (0, 9));
        assert!(parse_nvme_health(&[0u8; 100]).is_none());
    }

    #[test]
    fn ata_parse() {
        let mut b = vec![0u8; 512];
        b[2] = 5;
        b[3] = 0x33;
        b[5] = 100;
        b[6] = 99;
        b[7] = 12;
        b[14] = 194;
        b[19] = 41;
        let v = parse_ata_attributes(&b);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].id, v[0].raw, v[0].current, v[0].worst, v[0].flags), (5, 12, 100, 99, 0x33));
        assert_eq!((v[1].id, v[1].raw), (194, 41));
        assert!(parse_ata_attributes(&[1u8; 100]).is_empty());
        assert!(parse_ata_attributes(&[]).is_empty());
    }

    #[test]
    fn assess_cases() {
        let mut d = drive();
        assert_eq!(assess(&d).0, Status::Unknown);
        d.nvme = parse_nvme_health(&nvme_page());
        assert_eq!(assess(&d).0, Status::Good);
        d.nvme.as_mut().unwrap().critical_warning = 1;
        assert_eq!(assess(&d).0, Status::Bad);
        d.nvme.as_mut().unwrap().critical_warning = 0;
        d.nvme.as_mut().unwrap().available_spare = 5;
        assert_eq!(assess(&d).0, Status::Bad);
        d.nvme.as_mut().unwrap().available_spare = 100;
        d.nvme.as_mut().unwrap().percentage_used = 90;
        assert_eq!(assess(&d).0, Status::Caution);
        d.nvme.as_mut().unwrap().percentage_used = 1;
        d.nvme.as_mut().unwrap().media_errors = 2;
        assert_eq!(assess(&d).0, Status::Caution);

        let mut a = drive();
        a.ata = ata(&[(5, 0), (194, 35)]);
        assert_eq!(assess(&a).0, Status::Good);
        a.ata = ata(&[(197, 3)]);
        assert_eq!(assess(&a).0, Status::Caution);
        a.ata = ata(&[(5, 50)]);
        assert_eq!(assess(&a).0, Status::Bad);
        a.ata = ata(&[(199, 4)]);
        let (s, f) = assess(&a);
        assert_eq!((s, f[0].code.as_str()), (Status::Caution, "crc_errors"));
        a.ata = ata(&[(194, 0x0001_0000_0000 | 61)]);
        assert_eq!(assess(&a).0, Status::Caution);
    }

    #[test]
    fn worsened_cases() {
        let mut p = drive();
        p.ata = ata(&[(5, 1), (197, 0)]);
        let mut n = drive();
        n.ata = ata(&[(5, 2), (197, 1)]);
        assert_eq!(worsened(&p, &n).len(), 2);
        assert!(worsened(&n, &p).is_empty());

        let base = parse_nvme_health(&nvme_page()).unwrap();
        let (mut p, mut n) = (drive(), drive());
        p.nvme = Some(base);
        n.nvme = Some(NvmeHealth { percentage_used: 8, available_spare: 90, critical_warning: 4, ..base });
        let codes: Vec<_> = worsened(&p, &n).iter().map(|c| c.code).collect();
        assert_eq!(codes, ["percentage_used", "available_spare", "critical_warning"]);
        n.nvme = Some(NvmeHealth { percentage_used: 7, available_spare: 95, ..base });
        assert!(worsened(&p, &n).is_empty());
    }

    #[test]
    fn history_one_per_day_and_bounded() {
        // SAFETY: same value every data-dir test sets (see `registry::tests::test_data_dir`).
        unsafe { std::env::set_var("G2BOOST_DATA_DIR", std::env::temp_dir().join("g2boost-core-tests")) };
        let mut d = drive();
        d.key = "smart-test-key".into();
        d.ata = ata(&[(5, 0)]);
        record(DAY * 10, std::slice::from_ref(&d));
        record(DAY * 10 + 5, std::slice::from_ref(&d));
        assert_eq!(load()["smart-test-key"].iter().filter(|s| s.day == 10).count(), 1);
        for day in 11..200 {
            record(DAY * day, std::slice::from_ref(&d));
        }
        let h = load();
        assert_eq!(h["smart-test-key"].len(), MAX_SAMPLES);
        assert_eq!(h["smart-test-key"].last().unwrap().day, 199);
        assert!(last_sample("smart-test-key").is_some());
        let raw = std::fs::read_to_string(history_path()).unwrap();
        assert!(!raw.contains("serial"));
    }

    #[test]
    #[ignore = "live: reads this machine's drives"]
    fn live_health() {
        for d in health() {
            println!("{} [{}] {} needs_admin={} status={:?}", d.index, d.bus, d.model, d.needs_admin, assess(&d));
            println!("   nvme={:?} ata={}", d.nvme, d.ata.len());
        }
    }
}
