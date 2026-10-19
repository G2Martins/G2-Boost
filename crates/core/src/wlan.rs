//! Wi-Fi status and neighbor scan through the native WLAN API (`wlanapi.dll`: `WlanOpenHandle`,
//! `WlanEnumInterfaces`, `WlanQueryInterface`, `WlanGetNetworkBssList`,
//! `WlanGetAvailableNetworkList`, `WlanScan`). Read-only, no admin, no process spawned.
//!
//! Windows 11 24H2+ hides SSID/BSSID from desktop apps unless location access is allowed for them
//! (Settings > Privacy > Location). In that case the WLAN calls fail with `ERROR_ACCESS_DENIED` or
//! return empty SSIDs and all-zero BSSIDs; both map to [`WlanError::LocationDenied`]. When the WLAN
//! AutoConfig service is off or absent (desktop PC without Wi-Fi), the error is
//! [`WlanError::NoService`]; a PC with the service but no wireless adapter yields `Ok(None)` / `Ok(vec![])`.
//!
//! Everything the API returns (lengths, counts, SSIDs) is hostile input: counts are capped, reads
//! are bounds-checked against the returned buffer size, and SSIDs are sanitized (max 32 bytes,
//! lossy UTF-8, control characters removed).

use std::collections::BTreeMap;
use std::ffi::c_void;
use std::mem::{offset_of, size_of};
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WiFi::{
    DOT11_SSID, WLAN_AVAILABLE_NETWORK, WLAN_AVAILABLE_NETWORK_LIST, WLAN_BSS_ENTRY, WLAN_BSS_LIST,
    WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO, WLAN_INTERFACE_INFO_LIST, WlanCloseHandle,
    WlanEnumInterfaces, WlanFreeMemory, WlanGetAvailableNetworkList, WlanGetNetworkBssList,
    WlanOpenHandle, WlanQueryInterface, WlanScan, dot11_BSS_type_any, wlan_interface_state_connected,
    wlan_intf_opcode_current_connection, wlan_intf_opcode_rssi,
};
use windows::core::GUID;

const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_SERVICE_NOT_ACTIVE: u32 = 1062;
const ERROR_SERVICE_DEPENDENCY_FAIL: u32 = 1068;
const RPC_S_SERVER_UNAVAILABLE: u32 = 1722;
const ERROR_SERVICE_DOES_NOT_EXIST: u32 = 1060;

/// Sanity caps on counts reported by the API.
const MAX_INTERFACES: usize = 16;
const MAX_NETWORKS: usize = 2048;

#[derive(Debug, thiserror::Error)]
pub enum WlanError {
    /// Windows hides SSID/BSSID from this app: location access is off (24H2+).
    #[error("location access denied: Windows hides Wi-Fi names from apps without it")]
    LocationDenied,
    /// WLAN AutoConfig service is stopped or missing (usually no Wi-Fi adapter).
    #[error("WLAN service is not running")]
    NoService,
    #[error("WLAN API error {0}")]
    Api(u32),
}

fn map_err(code: u32) -> WlanError {
    match code {
        ERROR_ACCESS_DENIED => WlanError::LocationDenied,
        ERROR_SERVICE_NOT_ACTIVE
        | ERROR_SERVICE_DEPENDENCY_FAIL
        | ERROR_SERVICE_DOES_NOT_EXIST
        | RPC_S_SERVER_UNAVAILABLE => WlanError::NoService,
        c => WlanError::Api(c),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Band {
    #[serde(rename = "ghz_2_4")]
    Ghz2_4,
    #[serde(rename = "ghz_5")]
    Ghz5,
    #[serde(rename = "ghz_6")]
    Ghz6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Auth {
    Open,
    Wep,
    WpaPersonal,
    WpaEnterprise,
    Wpa2Personal,
    Wpa2Enterprise,
    Wpa3Personal,
    Wpa3Enterprise,
    Owe,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cipher {
    None,
    Wep,
    Tkip,
    Ccmp,
    Gcmp,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phy {
    A,
    B,
    G,
    N,
    Ac,
    Ax,
    Be,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct WifiStatus {
    pub ssid: String,
    pub bssid: String,
    pub auth: Auth,
    pub cipher: Cipher,
    pub signal_quality: u32,
    pub rssi_dbm: Option<i32>,
    pub channel: Option<u32>,
    pub band: Option<Band>,
    pub phy: Phy,
    pub rx_mbps: u32,
    pub tx_mbps: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct WifiNetwork {
    pub ssid: String,
    pub bssid: String,
    pub rssi_dbm: i32,
    pub signal_quality: u32,
    pub channel: Option<u32>,
    pub band: Option<Band>,
    pub phy: Phy,
    /// From the available-network list (by SSID); `None` when Windows did not list it.
    pub auth: Option<Auth>,
    pub cipher: Option<Cipher>,
    pub connected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiIssueKind {
    OpenNetwork,
    Wep,
    Tkip,
    Wpa1,
    WeakSignal,
    /// Connected on 2.4 GHz while the same SSID is also seen on 5/6 GHz.
    Band24With5Available,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize)]
pub struct WifiIssue {
    pub kind: WifiIssueKind,
    pub severity: Severity,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelLoad {
    pub channel: u32,
    /// Networks centered exactly on this channel.
    pub networks: u32,
    /// Interference score: sum of signal weights (0..1 each), spill-over included on 2.4 GHz.
    pub load: f32,
}

// ---------------------------------------------------------------- pure helpers

/// Center frequency (kHz, as `WLAN_BSS_ENTRY::ulChCenterFrequency`) to channel number and band.
pub fn channel_of(freq_khz: u32) -> Option<(u32, Band)> {
    if !freq_khz.is_multiple_of(1000) {
        return None;
    }
    let mhz = freq_khz / 1000;
    match mhz {
        2484 => Some((14, Band::Ghz2_4)),
        2412..=2472 if (mhz - 2407).is_multiple_of(5) => Some(((mhz - 2407) / 5, Band::Ghz2_4)),
        5160..=5885 if mhz.is_multiple_of(5) => Some(((mhz - 5000) / 5, Band::Ghz5)),
        5955..=7115 if (mhz - 5950).is_multiple_of(5) => Some(((mhz - 5950) / 5, Band::Ghz6)),
        _ => None,
    }
}

fn weight(rssi_dbm: i32) -> f32 {
    (rssi_dbm + 100).clamp(0, 60) as f32 / 60.0
}

/// Per-channel load on `band`. 2.4 GHz: a network also hits channels within +-4 (linear falloff).
/// 5/6 GHz: only its own channel (ponytail: ignores 40/80/160 MHz bonding).
pub fn congestion(nets: &[WifiNetwork], band: Band) -> Vec<ChannelLoad> {
    let channels: Vec<u32> = match band {
        Band::Ghz2_4 => (1..=13).collect(),
        Band::Ghz5 => (36..=64).step_by(4).chain((100..=144).step_by(4)).chain((149..=165).step_by(4)).collect(),
        Band::Ghz6 => (1..=233).step_by(4).collect(),
    };
    channels
        .into_iter()
        .map(|ch| {
            let (mut count, mut load) = (0u32, 0f32);
            for n in nets.iter().filter(|n| n.band == Some(band)) {
                let Some(c) = n.channel else { continue };
                let d = c.abs_diff(ch);
                if d == 0 {
                    count += 1;
                }
                let spill = if band == Band::Ghz2_4 { 1.0 - d as f32 / 5.0 } else if d == 0 { 1.0 } else { 0.0 };
                if spill > 0.0 {
                    load += weight(n.rssi_dbm) * spill;
                }
            }
            ChannelLoad { channel: ch, networks: count, load }
        })
        .collect()
}

/// Least loaded recommended channel: 1/6/11 on 2.4 GHz, any 20 MHz channel on 5/6 GHz. Ties: lowest.
pub fn best_channel(nets: &[WifiNetwork], band: Band) -> Option<u32> {
    congestion(nets, band)
        .into_iter()
        .filter(|l| band != Band::Ghz2_4 || matches!(l.channel, 1 | 6 | 11))
        .min_by(|a, b| a.load.total_cmp(&b.load))
        .map(|l| l.channel)
}

/// Security and quality findings for the connected network. `scan` is the visible-network list.
pub fn security_issues(s: &WifiStatus, scan: &[WifiNetwork]) -> Vec<WifiIssue> {
    let mut v = Vec::new();
    let mut add = |kind, severity| v.push(WifiIssue { kind, severity });
    if s.auth == Auth::Open {
        add(WifiIssueKind::OpenNetwork, Severity::Critical);
    }
    if s.auth == Auth::Wep || s.cipher == Cipher::Wep {
        add(WifiIssueKind::Wep, Severity::Critical);
    }
    if s.cipher == Cipher::Tkip {
        add(WifiIssueKind::Tkip, Severity::Warning);
    }
    if matches!(s.auth, Auth::WpaPersonal | Auth::WpaEnterprise) {
        add(WifiIssueKind::Wpa1, Severity::Warning);
    }
    if s.rssi_dbm.is_some_and(|r| r < -75) {
        add(WifiIssueKind::WeakSignal, Severity::Warning);
    }
    if s.band == Some(Band::Ghz2_4)
        && !s.ssid.is_empty()
        && scan.iter().any(|n| n.ssid == s.ssid && matches!(n.band, Some(Band::Ghz5 | Band::Ghz6)))
    {
        add(WifiIssueKind::Band24With5Available, Severity::Info);
    }
    v
}

// ---------------------------------------------------------------- mapping of API values

fn auth_of(v: i32) -> Auth {
    match v {
        1 => Auth::Open,
        2 => Auth::Wep,
        3 => Auth::WpaEnterprise,
        4 => Auth::WpaPersonal,
        6 => Auth::Wpa2Enterprise,
        7 => Auth::Wpa2Personal,
        8 | 11 => Auth::Wpa3Enterprise,
        9 => Auth::Wpa3Personal,
        10 => Auth::Owe,
        _ => Auth::Other,
    }
}

fn cipher_of(v: i32) -> Cipher {
    match v {
        0 => Cipher::None,
        1 | 5 | 257 => Cipher::Wep,
        2 => Cipher::Tkip,
        4 | 10 => Cipher::Ccmp,
        8 | 9 => Cipher::Gcmp,
        _ => Cipher::Other,
    }
}

fn phy_of(v: i32) -> Phy {
    match v {
        2 | 5 => Phy::B,
        4 => Phy::A,
        6 => Phy::G,
        7 => Phy::N,
        8 => Phy::Ac,
        10 => Phy::Ax,
        11 => Phy::Be,
        _ => Phy::Other,
    }
}

fn ssid_string(s: &DOT11_SSID) -> String {
    let n = (s.uSSIDLength as usize).min(32);
    String::from_utf8_lossy(&s.ucSSID[..n]).chars().filter(|c| !c.is_control()).collect()
}

fn mac_string(m: &[u8; 6]) -> String {
    m.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
}

// ---------------------------------------------------------------- Win32 plumbing

struct Client(HANDLE);

impl Client {
    fn open() -> Result<Self, WlanError> {
        let (mut ver, mut h) = (0u32, HANDLE::default());
        // SAFETY: out-pointers reference live locals; reserved is None.
        let r = unsafe { WlanOpenHandle(2, None, &mut ver, &mut h) };
        if r != 0 {
            return Err(map_err(r));
        }
        Ok(Self(h))
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // SAFETY: handle came from a successful WlanOpenHandle and is closed once.
        unsafe { WlanCloseHandle(self.0, None) };
    }
}

/// Buffer allocated by the WLAN API, freed with `WlanFreeMemory`.
struct Mem(*mut c_void);

impl Drop for Mem {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: pointer was allocated by the WLAN API and is freed exactly once.
            unsafe { WlanFreeMemory(self.0) };
        }
    }
}

struct Iface {
    guid: GUID,
    connected: bool,
}

fn interfaces(c: &Client) -> Result<Vec<Iface>, WlanError> {
    let mut p: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
    // SAFETY: valid handle; out-pointer is a live local.
    let r = unsafe { WlanEnumInterfaces(c.0, None, &mut p) };
    if r != 0 {
        return Err(map_err(r));
    }
    let _guard = Mem(p.cast());
    if p.is_null() {
        return Ok(vec![]);
    }
    // SAFETY: p is non-null and API-allocated; the count is capped and items are read unaligned
    // from the trailing array the API sized for `dwNumberOfItems` entries.
    unsafe {
        let n = ((*p).dwNumberOfItems as usize).min(MAX_INTERFACES);
        let base = std::ptr::addr_of!((*p).InterfaceInfo).cast::<WLAN_INTERFACE_INFO>();
        Ok((0..n)
            .map(|i| {
                let it = base.add(i).read_unaligned();
                Iface { guid: it.InterfaceGuid, connected: it.isState == wlan_interface_state_connected }
            })
            .collect())
    }
}

/// Raw bytes of a `WlanQueryInterface` result, at least `min` bytes long.
fn query(c: &Client, guid: &GUID, op: windows::Win32::NetworkManagement::WiFi::WLAN_INTF_OPCODE, min: usize) -> Result<Vec<u8>, WlanError> {
    let (mut size, mut data) = (0u32, std::ptr::null_mut::<c_void>());
    // SAFETY: valid handle/guid; out-pointers reference live locals.
    let r = unsafe { WlanQueryInterface(c.0, guid, op, None, &mut size, &mut data, None) };
    if r != 0 {
        return Err(map_err(r));
    }
    let _guard = Mem(data);
    if data.is_null() || (size as usize) < min {
        return Err(WlanError::Api(0));
    }
    // SAFETY: the API returned `size` readable bytes at `data`.
    Ok(unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size as usize) }.to_vec())
}

struct Bss {
    ssid: String,
    bssid: [u8; 6],
    rssi: i32,
    quality: u32,
    freq: u32,
    phy: i32,
    host_ts: u64,
}

fn bss_list(c: &Client, guid: &GUID) -> Result<Vec<Bss>, WlanError> {
    let mut p: *mut WLAN_BSS_LIST = std::ptr::null_mut();
    // SAFETY: valid handle/guid; null SSID = all networks; out-pointer is a live local.
    let r = unsafe { WlanGetNetworkBssList(c.0, guid, None, dot11_BSS_type_any, false, None, &mut p) };
    if r != 0 {
        return Err(map_err(r));
    }
    let _guard = Mem(p.cast());
    if p.is_null() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    // SAFETY: p is API-allocated with `dwTotalSize` readable bytes; each entry is bounds-checked
    // against that size before an unaligned read.
    unsafe {
        let total = (*p).dwTotalSize as usize;
        let n = ((*p).dwNumberOfItems as usize).min(MAX_NETWORKS);
        let off = offset_of!(WLAN_BSS_LIST, wlanBssEntries);
        for i in 0..n {
            let end = off.saturating_add((i + 1).saturating_mul(size_of::<WLAN_BSS_ENTRY>()));
            if end > total {
                break;
            }
            let e = p.cast::<u8>().add(off + i * size_of::<WLAN_BSS_ENTRY>()).cast::<WLAN_BSS_ENTRY>().read_unaligned();
            out.push(Bss {
                ssid: ssid_string(&e.dot11Ssid),
                bssid: e.dot11Bssid,
                rssi: e.lRssi,
                quality: e.uLinkQuality.min(100),
                freq: e.ulChCenterFrequency,
                phy: e.dot11BssPhyType.0,
                host_ts: e.ullHostTimestamp,
            });
        }
    }
    Ok(out)
}

fn available(c: &Client, guid: &GUID) -> BTreeMap<String, (Auth, Cipher)> {
    let mut p: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
    // SAFETY: valid handle/guid; out-pointer is a live local.
    let r = unsafe { WlanGetAvailableNetworkList(c.0, guid, 0, None, &mut p) };
    let mut map = BTreeMap::new();
    if r != 0 {
        return map;
    }
    let _guard = Mem(p.cast());
    if p.is_null() {
        return map;
    }
    // SAFETY: API-allocated list; count capped; entries read unaligned from the trailing array.
    unsafe {
        let n = ((*p).dwNumberOfItems as usize).min(MAX_NETWORKS);
        let base = std::ptr::addr_of!((*p).Network).cast::<WLAN_AVAILABLE_NETWORK>();
        for i in 0..n {
            let it = base.add(i).read_unaligned();
            map.entry(ssid_string(&it.dot11Ssid))
                .or_insert((auth_of(it.dot11DefaultAuthAlgorithm.0), cipher_of(it.dot11DefaultCipherAlgorithm.0)));
        }
    }
    map
}

fn to_network(b: &Bss, avail: &BTreeMap<String, (Auth, Cipher)>, connected: bool) -> WifiNetwork {
    let ch = channel_of(b.freq);
    let a = avail.get(&b.ssid);
    WifiNetwork {
        ssid: b.ssid.clone(),
        bssid: mac_string(&b.bssid),
        rssi_dbm: b.rssi,
        signal_quality: b.quality,
        channel: ch.map(|c| c.0),
        band: ch.map(|c| c.1),
        phy: phy_of(b.phy),
        auth: a.map(|x| x.0),
        cipher: a.map(|x| x.1),
        connected,
    }
}

/// Current connection of an interface, bounds-checked.
fn connection(c: &Client, guid: &GUID) -> Result<WLAN_CONNECTION_ATTRIBUTES, WlanError> {
    let buf = query(c, guid, wlan_intf_opcode_current_connection, size_of::<WLAN_CONNECTION_ATTRIBUTES>())?;
    // SAFETY: buffer length checked >= size_of the struct; read_unaligned tolerates any alignment.
    Ok(unsafe { buf.as_ptr().cast::<WLAN_CONNECTION_ATTRIBUTES>().read_unaligned() })
}

// ---------------------------------------------------------------- public API

/// Connected Wi-Fi of the first connected wireless interface. `Ok(None)`: no adapter or not connected.
pub fn status() -> Result<Option<WifiStatus>, WlanError> {
    let c = Client::open()?;
    let Some(i) = interfaces(&c)?.into_iter().find(|i| i.connected) else {
        return Ok(None);
    };
    let conn = connection(&c, &i.guid)?;
    let a = conn.wlanAssociationAttributes;
    let ssid = ssid_string(&a.dot11Ssid);
    if ssid.is_empty() && a.dot11Bssid == [0; 6] {
        return Err(WlanError::LocationDenied);
    }
    let bss = bss_list(&c, &i.guid).ok().and_then(|l| l.into_iter().find(|b| b.bssid == a.dot11Bssid));
    let ch = bss.as_ref().and_then(|b| channel_of(b.freq));
    let rssi_dbm = bss.as_ref().map(|b| b.rssi).or_else(|| {
        let b = query(&c, &i.guid, wlan_intf_opcode_rssi, 4).ok()?;
        Some(i32::from_le_bytes(b[..4].try_into().ok()?))
    });
    let sec = conn.wlanSecurityAttributes;
    Ok(Some(WifiStatus {
        ssid,
        bssid: mac_string(&a.dot11Bssid),
        auth: auth_of(sec.dot11AuthAlgorithm.0),
        cipher: cipher_of(sec.dot11CipherAlgorithm.0),
        signal_quality: a.wlanSignalQuality.min(100),
        rssi_dbm,
        channel: ch.map(|c| c.0),
        band: ch.map(|c| c.1),
        phy: phy_of(a.dot11PhyType.0),
        rx_mbps: a.ulRxRate / 1000,
        tx_mbps: a.ulTxRate / 1000,
    }))
}

/// Visible BSSs on all wireless interfaces. `refresh` asks the driver for a fresh scan and waits
/// up to 4 s (polling every 250 ms) for new results.
pub fn scan(refresh: bool) -> Result<Vec<WifiNetwork>, WlanError> {
    let c = Client::open()?;
    let ifaces = interfaces(&c)?;
    if refresh {
        let before: u64 = ifaces
            .iter()
            .filter_map(|i| bss_list(&c, &i.guid).ok())
            .flat_map(|l| l.into_iter().map(|b| b.host_ts))
            .max()
            .unwrap_or(0);
        for i in &ifaces {
            // SAFETY: valid handle/guid; all optional pointers are None. Failure (busy) is ignored.
            let _ = unsafe { WlanScan(c.0, &i.guid, None, None, None) };
        }
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(4) {
            std::thread::sleep(Duration::from_millis(250));
            let now = ifaces
                .iter()
                .filter_map(|i| bss_list(&c, &i.guid).ok())
                .flat_map(|l| l.into_iter().map(|b| b.host_ts))
                .max()
                .unwrap_or(0);
            if now > before {
                std::thread::sleep(Duration::from_millis(500)); // results arrive in a burst
                break;
            }
        }
    }
    let mut out = Vec::new();
    let mut seen = 0usize;
    for i in &ifaces {
        let cur = if i.connected { connection(&c, &i.guid).ok().map(|x| x.wlanAssociationAttributes.dot11Bssid) } else { None };
        let avail = available(&c, &i.guid);
        for b in bss_list(&c, &i.guid)? {
            seen += 1;
            let conn = cur == Some(b.bssid);
            out.push(to_network(&b, &avail, conn));
        }
    }
    if seen > 0 && out.iter().all(|n| n.bssid == "00:00:00:00:00:00") {
        return Err(WlanError::LocationDenied);
    }
    out.sort_by_key(|n| std::cmp::Reverse(n.rssi_dbm));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(ssid: &str, rssi: i32, freq_khz: u32) -> WifiNetwork {
        let ch = channel_of(freq_khz);
        WifiNetwork {
            ssid: ssid.into(),
            bssid: "aa:bb:cc:dd:ee:ff".into(),
            rssi_dbm: rssi,
            signal_quality: 50,
            channel: ch.map(|c| c.0),
            band: ch.map(|c| c.1),
            phy: Phy::Ax,
            auth: None,
            cipher: None,
            connected: false,
        }
    }

    fn status_of(auth: Auth, cipher: Cipher, rssi: i32, freq: u32) -> WifiStatus {
        let ch = channel_of(freq);
        WifiStatus {
            ssid: "casa".into(),
            bssid: "aa:bb:cc:dd:ee:ff".into(),
            auth,
            cipher,
            signal_quality: 80,
            rssi_dbm: Some(rssi),
            channel: ch.map(|c| c.0),
            band: ch.map(|c| c.1),
            phy: Phy::Ax,
            rx_mbps: 100,
            tx_mbps: 100,
        }
    }

    #[test]
    fn channels() {
        assert_eq!(channel_of(2412000), Some((1, Band::Ghz2_4)));
        assert_eq!(channel_of(2437000), Some((6, Band::Ghz2_4)));
        assert_eq!(channel_of(2484000), Some((14, Band::Ghz2_4)));
        assert_eq!(channel_of(5180000), Some((36, Band::Ghz5)));
        assert_eq!(channel_of(5825000), Some((165, Band::Ghz5)));
        assert_eq!(channel_of(5955000), Some((1, Band::Ghz6)));
        assert_eq!(channel_of(7115000), Some((233, Band::Ghz6)));
        for bad in [0, 1, 2413000, 2412500, 3000000, 5900000, 9999999, u32::MAX] {
            assert_eq!(channel_of(bad), None, "{bad}");
        }
    }

    #[test]
    fn ssid_is_sanitized() {
        let mut s = DOT11_SSID { uSSIDLength: 999, ucSSID: [b'a'; 32] };
        s.ucSSID[1] = 0x1b;
        s.ucSSID[2] = b'\n';
        assert_eq!(ssid_string(&s).len(), 30);
    }

    #[test]
    fn issues() {
        let none = |s: &WifiStatus| security_issues(s, &[]).into_iter().map(|i| i.kind).collect::<Vec<_>>();
        assert_eq!(none(&status_of(Auth::Open, Cipher::None, -50, 5180000)), [WifiIssueKind::OpenNetwork]);
        assert_eq!(none(&status_of(Auth::Wep, Cipher::Wep, -50, 5180000)), [WifiIssueKind::Wep]);
        assert_eq!(none(&status_of(Auth::Wpa2Personal, Cipher::Tkip, -80, 5180000)), [WifiIssueKind::Tkip, WifiIssueKind::WeakSignal]);
        assert_eq!(none(&status_of(Auth::WpaPersonal, Cipher::Ccmp, -50, 5180000)), [WifiIssueKind::Wpa1]);
        assert!(none(&status_of(Auth::Wpa3Personal, Cipher::Ccmp, -50, 5180000)).is_empty());

        let s = status_of(Auth::Wpa2Personal, Cipher::Ccmp, -50, 2437000);
        assert!(security_issues(&s, &[net("casa", -60, 2437000)]).is_empty());
        let k = security_issues(&s, &[net("casa", -60, 5180000)]);
        assert_eq!(k[0].kind, WifiIssueKind::Band24With5Available);
    }

    #[test]
    fn congestion_and_best() {
        // Crowded 1 and 6; 11 is free.
        let nets = [net("a", -40, 2412000), net("b", -50, 2412000), net("c", -45, 2437000), net("d", -90, 2462000)];
        let c = congestion(&nets, Band::Ghz2_4);
        assert_eq!(c.iter().find(|l| l.channel == 1).unwrap().networks, 2);
        assert!(c.iter().find(|l| l.channel == 3).unwrap().load > 0.0); // spill from 1
        assert_eq!(best_channel(&nets, Band::Ghz2_4), Some(11));

        let n5 = [net("a", -40, 5180000), net("b", -40, 5200000)];
        let best = best_channel(&n5, Band::Ghz5).unwrap();
        assert!(best != 36 && best != 40);
        assert_eq!(best_channel(&[], Band::Ghz2_4), Some(1));
    }

    #[test]
    fn enum_serde() {
        assert_eq!(serde_json::to_string(&Band::Ghz2_4).unwrap(), "\"ghz_2_4\"");
        assert_eq!(serde_json::to_string(&Auth::Wpa2Personal).unwrap(), "\"wpa2_personal\"");
    }

    #[test]
    #[ignore = "needs a Wi-Fi adapter; prints live data"]
    fn live() {
        match status() {
            Ok(Some(s)) => println!("status: ssid_len={} band={:?} ch={:?} rssi={:?} auth={:?}", s.ssid.len(), s.band, s.channel, s.rssi_dbm, s.auth),
            Ok(None) => println!("status: none"),
            Err(e) => println!("status error: {e}"),
        }
        match scan(true) {
            Ok(n) => println!("scan: {} BSS, best 2.4={:?} 5={:?}", n.len(), best_channel(&n, Band::Ghz2_4), best_channel(&n, Band::Ghz5)),
            Err(e) => println!("scan error: {e}"),
        }
    }
}
