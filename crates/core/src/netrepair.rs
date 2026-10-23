//! "Reparar conexão": renew the DHCP lease of one adapter (`IpReleaseAddress` +
//! `IpRenewAddress`, iphlpapi, the calls behind `ipconfig /release` and `/renew`), flush the DNS
//! cache (`DnsFlushResolverCache`, shared with the cleanup catalog) and open Windows' own Network
//! reset page (the winsock/IP stack reset has no public API). Never `ipconfig`/`netsh`.
//!
//! Adapter indexes come from the caller and are matched against the live list (DHCP-enabled,
//! connected IPv4 adapters known to `GetInterfaceInfo`). Release/renew need admin (as the
//! `ipconfig` equivalents do); the flush does not.

use std::net::Ipv4Addr;

use serde::Serialize;
use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
use windows::Win32::NetworkManagement::IpHelper::{
    GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses, GetInterfaceInfo,
    IF_TYPE_SOFTWARE_LOOPBACK, IP_ADAPTER_ADDRESSES_LH, IP_ADAPTER_DHCP_ENABLED, IP_ADAPTER_INDEX_MAP, IP_INTERFACE_INFO,
    IpReleaseAddress, IpRenewAddress,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::AF_INET;
use windows::Win32::UI::Shell::{SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::PCWSTR;

use crate::dns::{best_if_index, pwstr, sockaddr_v4};
use crate::registry::wide;
use crate::system;

const ERROR_ACCESS_DENIED: u32 = 5;

#[derive(Debug, Clone, Serialize)]
pub struct DhcpAdapter {
    /// IPv4 interface index: the id `renew` takes.
    pub index: u32,
    pub name: String,
    pub description: String,
    pub ipv4: Option<Ipv4Addr>,
    /// The adapter Windows routes internet traffic through.
    pub primary: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum NetRepairError {
    #[error("needs_admin")]
    NeedsAdmin,
    #[error("unknown_adapter")]
    UnknownAdapter,
    #[error("win32 {0}")]
    Win32(u32),
    #[error("{0}")]
    Failed(String),
}

/// `(index, \DEVICE\TCPIP_{GUID})` of every IPv4 interface, as release/renew want it.
fn interface_maps() -> Vec<IP_ADAPTER_INDEX_MAP> {
    let mut len = 0u32;
    // SAFETY: size query with no buffer.
    let rc = unsafe { GetInterfaceInfo(None, &mut len) };
    if rc != ERROR_INSUFFICIENT_BUFFER.0 || len == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u32; (len as usize).div_ceil(4)];
    let info = buf.as_mut_ptr().cast::<IP_INTERFACE_INFO>();
    // SAFETY: 4-byte aligned buffer of `len` bytes.
    if unsafe { GetInterfaceInfo(Some(info), &mut len) } != ERROR_SUCCESS.0 {
        return Vec::new();
    }
    let room = (buf.len() * 4).saturating_sub(4) / size_of::<IP_ADAPTER_INDEX_MAP>();
    // SAFETY: the API wrote NumAdapters entries after the count; bounded by the buffer size.
    unsafe {
        let n = ((*info).NumAdapters.max(0) as usize).min(room);
        let first = (&raw const (*info).Adapter).cast::<IP_ADAPTER_INDEX_MAP>();
        (0..n).map(|i| *first.add(i)).collect()
    }
}

/// Connected IPv4 adapters with DHCP on (static ones have nothing to renew); routed one first.
pub fn adapters() -> Vec<DhcpAdapter> {
    let maps = interface_maps();
    let best = best_if_index();
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size = 16 * 1024u32;
    let mut buf: Vec<u64>;
    loop {
        buf = vec![0u64; (size as usize).div_ceil(8)];
        // SAFETY: 8-byte aligned buffer of `size` bytes; the API fills a linked list inside it.
        let rc = unsafe { GetAdaptersAddresses(u32::from(AF_INET.0), flags, None, Some(buf.as_mut_ptr().cast()), &mut size) };
        if rc == ERROR_BUFFER_OVERFLOW.0 {
            continue;
        }
        if rc != ERROR_SUCCESS.0 {
            return Vec::new();
        }
        break;
    }
    let mut out = Vec::new();
    let mut p = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    // SAFETY: walking the list the API wrote into `buf` (alive for the whole loop).
    unsafe {
        while !p.is_null() {
            let a = &*p;
            p = a.Next;
            let index = a.Anonymous1.Anonymous.IfIndex;
            if a.OperStatus != IfOperStatusUp
                || a.IfType == IF_TYPE_SOFTWARE_LOOPBACK
                || a.Anonymous2.Flags & IP_ADAPTER_DHCP_ENABLED == 0
                || !maps.iter().any(|m| m.Index == index)
            {
                continue;
            }
            let mut ipv4 = None;
            let mut u = a.FirstUnicastAddress;
            while !u.is_null() && ipv4.is_none() {
                ipv4 = sockaddr_v4(&(*u).Address);
                u = (*u).Next;
            }
            out.push(DhcpAdapter { index, name: pwstr(a.FriendlyName), description: pwstr(a.Description), ipv4, primary: best == Some(index) });
        }
    }
    out.sort_by_key(|a| !a.primary);
    out
}

/// Release and renew the DHCP lease of one live adapter (the connection drops for a few
/// seconds). Returns the address it has afterwards. A failed release (no lease yet) is not fatal:
/// the renew decides.
pub fn renew(index: u32) -> Result<Option<Ipv4Addr>, NetRepairError> {
    if !system::is_elevated() {
        return Err(NetRepairError::NeedsAdmin);
    }
    if !adapters().iter().any(|a| a.index == index) {
        return Err(NetRepairError::UnknownAdapter);
    }
    let map = interface_maps().into_iter().find(|m| m.Index == index).ok_or(NetRepairError::UnknownAdapter)?;
    // SAFETY: an IP_ADAPTER_INDEX_MAP exactly as GetInterfaceInfo returned it.
    if unsafe { IpReleaseAddress(&map) } == ERROR_ACCESS_DENIED {
        return Err(NetRepairError::NeedsAdmin);
    }
    // SAFETY: as above. Blocks until the DHCP server answers or the stack gives up.
    match unsafe { IpRenewAddress(&map) } {
        0 => Ok(adapters().into_iter().find(|a| a.index == index).and_then(|a| a.ipv4)),
        ERROR_ACCESS_DENIED => Err(NetRepairError::NeedsAdmin),
        c => Err(NetRepairError::Win32(c)),
    }
}

/// Empty Windows' DNS client cache (no admin).
pub fn flush_dns() -> Result<(), NetRepairError> {
    crate::cleanup::dns_flush().map_err(NetRepairError::Failed)
}

/// Where Windows keeps "Network reset": Advanced network settings on 11, the Status page on 10.
pub(crate) fn reset_page(windows11: bool) -> &'static str {
    if windows11 { "ms-settings:network-advancedsettings" } else { "ms-settings:network-status" }
}

/// Open Windows' own Network reset page (fixed URI, protocol activation; nothing is spawned).
pub fn open_network_reset() -> Result<(), NetRepairError> {
    let uri = wide(reset_page(system::os_version().starts_with("Windows 11")));
    let verb = wide("open");
    // SAFETY: zeroed struct with cbSize set; the strings outlive the call.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOASYNC;
        info.lpVerb = PCWSTR(verb.as_ptr());
        info.lpFile = PCWSTR(uri.as_ptr());
        info.nShow = SW_SHOWNORMAL.0;
        ShellExecuteExW(&mut info).map_err(|e| NetRepairError::Win32((e.code().0 as u32) & 0xFFFF))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_page_is_a_fixed_settings_uri() {
        assert_eq!(reset_page(true), "ms-settings:network-advancedsettings");
        assert_eq!(reset_page(false), "ms-settings:network-status");
    }

    #[test]
    fn renew_refused_without_admin_or_for_an_unknown_adapter() {
        // Never releases anything: non-elevated stops at the admin check, elevated at the index.
        let r = renew(u32::MAX);
        if system::is_elevated() {
            assert!(matches!(r, Err(NetRepairError::UnknownAdapter)));
        } else {
            assert!(matches!(r, Err(NetRepairError::NeedsAdmin)));
        }
    }

    #[test]
    fn interface_maps_name_the_tcpip_device() {
        for m in interface_maps() {
            let name = String::from_utf16_lossy(&m.Name[..m.Name.iter().position(|&c| c == 0).unwrap_or(m.Name.len())]);
            assert!(name.to_ascii_uppercase().starts_with(r"\DEVICE\TCPIP_{"), "{name}");
        }
    }

    /// Read-only: the DHCP adapters this PC would offer. `--ignored live_dhcp_adapters --nocapture`
    #[test]
    #[ignore]
    fn live_dhcp_adapters() {
        let a = adapters();
        for x in &a {
            println!("#{} {} ({}) ipv4={:?} primary={}", x.index, x.name, x.description, x.ipv4, x.primary);
        }
        assert!(a.iter().filter(|x| x.primary).count() <= 1);
    }
}
