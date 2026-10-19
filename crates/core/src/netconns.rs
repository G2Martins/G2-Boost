//! "Who is using my network": live TCP/UDP endpoints with their owning process.
//!
//! Source: IP Helper `GetExtendedTcpTable` (`TCP_TABLE_OWNER_PID_ALL`) and
//! `GetExtendedUdpTable` (`UDP_TABLE_OWNER_PID`), IPv4 + IPv6. Read-only, no admin, no driver.
//! No reverse DNS on purpose (privacy and speed). Table contents are treated as hostile input:
//! row counts are bounds-checked against the buffer, process strings are sanitized and capped.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use serde::Serialize;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, MIB_UDP6ROW_OWNER_PID,
    MIB_UDPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::PWSTR;

const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const MAX_STR: usize = 260;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TcpState {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
    DeleteTcb,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Unspecified,
    Loopback,
    Multicast,
    /// RFC 1918, link-local, IPv6 ULA / fe80::/10.
    Lan,
    /// 100.64.0.0/10 (carrier-grade NAT): neither home LAN nor a real public host.
    Cgnat,
    Public,
}

#[derive(Debug, Clone, Serialize)]
pub struct Conn {
    pub protocol: Protocol,
    pub pid: u32,
    pub process: String,
    pub exe_path: String,
    pub local_addr: IpAddr,
    pub local_port: u16,
    pub remote_addr: Option<IpAddr>,
    pub remote_port: u16,
    pub state: Option<TcpState>,
    pub remote_scope: Scope,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcNet {
    pub process: String,
    pub exe_path: String,
    pub established: usize,
    /// Distinct remote IPs with `Scope::Public` over established TCP.
    pub public_remotes: usize,
    pub listening_ports: Vec<u16>,
    pub udp_endpoints: usize,
}

fn tcp_state(s: u32) -> TcpState {
    match s {
        1 => TcpState::Closed,
        2 => TcpState::Listen,
        3 => TcpState::SynSent,
        4 => TcpState::SynReceived,
        5 => TcpState::Established,
        6 => TcpState::FinWait1,
        7 => TcpState::FinWait2,
        8 => TcpState::CloseWait,
        9 => TcpState::Closing,
        10 => TcpState::LastAck,
        11 => TcpState::TimeWait,
        12 => TcpState::DeleteTcb,
        _ => TcpState::Unknown,
    }
}

fn scope_v4(a: Ipv4Addr) -> Scope {
    let o = a.octets();
    if a.is_unspecified() {
        Scope::Unspecified
    } else if a.is_loopback() {
        Scope::Loopback
    } else if a.is_multicast() {
        Scope::Multicast
    } else if o[0] == 100 && (64..=127).contains(&o[1]) {
        Scope::Cgnat
    } else if a.is_private() || a.is_link_local() {
        Scope::Lan
    } else {
        Scope::Public
    }
}

/// Classify an address. Pure.
pub fn remote_scope(ip: IpAddr) -> Scope {
    match ip {
        IpAddr::V4(a) => scope_v4(a),
        IpAddr::V6(a) => {
            if let Some(v4) = a.to_ipv4_mapped() {
                return scope_v4(v4);
            }
            let seg0 = a.segments()[0];
            if a.is_unspecified() {
                Scope::Unspecified
            } else if a.is_loopback() {
                Scope::Loopback
            } else if a.is_multicast() {
                Scope::Multicast
            } else if seg0 & 0xffc0 == 0xfe80 || seg0 & 0xfe00 == 0xfc00 {
                Scope::Lan
            } else {
                Scope::Public
            }
        }
    }
}

fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(MAX_STR).collect()
}

/// (process name, exe path) of a pid; falls back to a fixed name or `pid N` when it cannot be opened.
fn proc_info(pid: u32) -> (String, String) {
    let mut path = String::new();
    // SAFETY: limited-information handle, closed below; buffer sized for the call.
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
                path = String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())]);
            }
            let _ = CloseHandle(h);
        }
    }
    let path = clean(&path);
    let name = match path.rsplit('\\').next().filter(|n| !n.is_empty()) {
        Some(n) => n.to_string(),
        None => match pid {
            0 => "System Idle".into(),
            4 => "System".into(),
            _ => format!("pid {pid}"),
        },
    };
    (name, path)
}

/// Fetch a table into a 4-aligned buffer; returns the buffer and the entry count (bounds-checked).
fn fetch<R>(call: impl Fn(*mut std::ffi::c_void, &mut u32) -> u32) -> Vec<R>
where
    R: Copy,
{
    let mut size = 0u32;
    let mut buf: Vec<u32> = Vec::new();
    for _ in 0..8 {
        let ptr = if buf.is_empty() { std::ptr::null_mut() } else { buf.as_mut_ptr().cast() };
        match call(ptr, &mut size) {
            0 => break,
            ERROR_INSUFFICIENT_BUFFER => {
                // table may grow between calls: add slack
                buf = vec![0u32; (size as usize).div_ceil(4) + 64];
                size = (buf.len() * 4) as u32;
            }
            _ => return Vec::new(),
        }
        if buf.is_empty() {
            return Vec::new();
        }
    }
    if buf.is_empty() {
        return Vec::new();
    }
    let total = (size as usize).min(buf.len() * 4);
    if total < 4 {
        return Vec::new();
    }
    let base = buf.as_ptr().cast::<u8>();
    // SAFETY: buffer holds at least 4 bytes (checked); dwNumEntries is the first u32.
    let n = unsafe { std::ptr::read_unaligned(base.cast::<u32>()) } as usize;
    let rs = std::mem::size_of::<R>();
    let n = n.min((total - 4) / rs.max(1));
    (0..n)
        // SAFETY: row i lies fully inside the first `total` bytes (n clamped above); read_unaligned.
        .map(|i| unsafe { std::ptr::read_unaligned(base.add(4 + i * rs).cast::<R>()) })
        .collect()
}

fn port(p: u32) -> u16 {
    u16::from_be(p as u16)
}

fn v4(a: u32) -> IpAddr {
    IpAddr::V4(Ipv4Addr::from(a.to_le_bytes()))
}

/// All TCP (v4+v6) connections/listeners and UDP (v4+v6) endpoints with their owning process.
pub fn connections() -> Vec<Conn> {
    let mut cache: HashMap<u32, (String, String)> = HashMap::new();
    let mut out = Vec::new();
    let mut push = |protocol, pid: u32, local: IpAddr, lp: u32, remote: Option<(IpAddr, u32)>, state: Option<u32>| {
        let (process, exe_path) = cache.entry(pid).or_insert_with(|| proc_info(pid)).clone();
        let (remote_addr, remote_port) = match remote {
            Some((a, p)) => (Some(a), port(p)),
            None => (None, 0),
        };
        out.push(Conn {
            protocol,
            pid,
            process,
            exe_path,
            local_addr: local,
            local_port: port(lp),
            remote_scope: remote_addr.map_or(Scope::Unspecified, remote_scope),
            remote_addr,
            remote_port,
            state: state.map(tcp_state),
        });
    };

    let t4: Vec<MIB_TCPROW_OWNER_PID> = fetch(|p, sz| {
        // SAFETY: p is null (size query) or a buffer of *sz bytes; sz is a valid out-param.
        unsafe { GetExtendedTcpTable(if p.is_null() { None } else { Some(p) }, sz, false, AF_INET.0 as u32, TCP_TABLE_OWNER_PID_ALL, 0) }
    });
    for r in t4 {
        push(Protocol::Tcp, r.dwOwningPid, v4(r.dwLocalAddr), r.dwLocalPort, Some((v4(r.dwRemoteAddr), r.dwRemotePort)), Some(r.dwState));
    }
    let t6: Vec<MIB_TCP6ROW_OWNER_PID> = fetch(|p, sz| {
        // SAFETY: as above.
        unsafe { GetExtendedTcpTable(if p.is_null() { None } else { Some(p) }, sz, false, AF_INET6.0 as u32, TCP_TABLE_OWNER_PID_ALL, 0) }
    });
    for r in t6 {
        push(
            Protocol::Tcp,
            r.dwOwningPid,
            IpAddr::V6(Ipv6Addr::from(r.ucLocalAddr)),
            r.dwLocalPort,
            Some((IpAddr::V6(Ipv6Addr::from(r.ucRemoteAddr)), r.dwRemotePort)),
            Some(r.dwState),
        );
    }
    let u4: Vec<MIB_UDPROW_OWNER_PID> = fetch(|p, sz| {
        // SAFETY: as above.
        unsafe { GetExtendedUdpTable(if p.is_null() { None } else { Some(p) }, sz, false, AF_INET.0 as u32, UDP_TABLE_OWNER_PID, 0) }
    });
    for r in u4 {
        push(Protocol::Udp, r.dwOwningPid, v4(r.dwLocalAddr), r.dwLocalPort, None, None);
    }
    let u6: Vec<MIB_UDP6ROW_OWNER_PID> = fetch(|p, sz| {
        // SAFETY: as above.
        unsafe { GetExtendedUdpTable(if p.is_null() { None } else { Some(p) }, sz, false, AF_INET6.0 as u32, UDP_TABLE_OWNER_PID, 0) }
    });
    for r in u6 {
        push(Protocol::Udp, r.dwOwningPid, IpAddr::V6(Ipv6Addr::from(r.ucLocalAddr)), r.dwLocalPort, None, None);
    }
    out
}

/// Group by executable (path, or process name when the path is unreadable). Sorted by established desc.
pub fn by_process(conns: &[Conn]) -> Vec<ProcNet> {
    #[derive(Default)]
    struct Acc {
        process: String,
        exe_path: String,
        established: usize,
        remotes: HashSet<IpAddr>,
        listen: BTreeSet<u16>,
        udp: usize,
    }
    let mut map: HashMap<String, Acc> = HashMap::new();
    for c in conns {
        let key = if c.exe_path.is_empty() { c.process.to_ascii_lowercase() } else { c.exe_path.to_ascii_lowercase() };
        let a = map.entry(key).or_default();
        if a.process.is_empty() {
            a.process = c.process.clone();
            a.exe_path = c.exe_path.clone();
        }
        match (c.protocol, c.state) {
            (Protocol::Tcp, Some(TcpState::Established)) => {
                a.established += 1;
                if let Some(ip) = c.remote_addr
                    && c.remote_scope == Scope::Public
                {
                    a.remotes.insert(ip);
                }
            }
            (Protocol::Tcp, Some(TcpState::Listen)) => {
                a.listen.insert(c.local_port);
            }
            (Protocol::Udp, _) => a.udp += 1,
            _ => {}
        }
    }
    let mut v: Vec<ProcNet> = map
        .into_values()
        .map(|a| ProcNet {
            process: a.process,
            exe_path: a.exe_path,
            established: a.established,
            public_remotes: a.remotes.len(),
            listening_ports: a.listen.into_iter().collect(),
            udp_endpoints: a.udp,
        })
        .collect();
    v.sort_by(|a, b| b.established.cmp(&a.established).then_with(|| a.process.to_ascii_lowercase().cmp(&b.process.to_ascii_lowercase())));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn scopes() {
        for (s, want) in [
            ("0.0.0.0", Scope::Unspecified),
            ("::", Scope::Unspecified),
            ("127.0.0.1", Scope::Loopback),
            ("::1", Scope::Loopback),
            ("224.0.0.251", Scope::Multicast),
            ("ff02::fb", Scope::Multicast),
            ("10.1.2.3", Scope::Lan),
            ("172.16.0.1", Scope::Lan),
            ("172.31.255.255", Scope::Lan),
            ("172.32.0.1", Scope::Public),
            ("192.168.1.1", Scope::Lan),
            ("169.254.1.1", Scope::Lan),
            ("100.64.0.1", Scope::Cgnat),
            ("100.127.255.255", Scope::Cgnat),
            ("100.128.0.1", Scope::Public),
            ("8.8.8.8", Scope::Public),
            ("fe80::1", Scope::Lan),
            ("febf::1", Scope::Lan),
            ("fec0::1", Scope::Public),
            ("fc00::1", Scope::Lan),
            ("fdff::1", Scope::Lan),
            ("2606:4700::1111", Scope::Public),
            ("::ffff:192.168.0.5", Scope::Lan),
        ] {
            assert_eq!(remote_scope(ip(s)), want, "{s}");
        }
    }

    fn conn(proto: Protocol, state: Option<TcpState>, remote: Option<&str>, lport: u16, exe: &str) -> Conn {
        let r = remote.map(ip);
        Conn {
            protocol: proto,
            pid: 1,
            process: exe.rsplit('\\').next().unwrap().into(),
            exe_path: exe.into(),
            local_addr: ip("0.0.0.0"),
            local_port: lport,
            remote_addr: r,
            remote_port: 443,
            state,
            remote_scope: r.map_or(Scope::Unspecified, remote_scope),
        }
    }

    #[test]
    fn grouping() {
        let e = Some(TcpState::Established);
        let a = r"C:\x\A.exe";
        let b = r"C:\x\b.exe";
        let c = vec![
            conn(Protocol::Tcp, e, Some("8.8.8.8"), 5000, a),
            conn(Protocol::Tcp, e, Some("8.8.8.8"), 5001, a),
            conn(Protocol::Tcp, e, Some("1.1.1.1"), 5002, a),
            conn(Protocol::Tcp, e, Some("192.168.1.9"), 5003, a),
            conn(Protocol::Tcp, Some(TcpState::Listen), Some("0.0.0.0"), 80, &a.to_ascii_lowercase()),
            conn(Protocol::Udp, None, None, 53, a),
            conn(Protocol::Tcp, e, Some("9.9.9.9"), 6000, b),
            conn(Protocol::Tcp, Some(TcpState::TimeWait), Some("9.9.9.9"), 6001, b),
        ];
        let v = by_process(&c);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].established, 4);
        assert_eq!(v[0].public_remotes, 2);
        assert_eq!(v[0].listening_ports, vec![80]);
        assert_eq!(v[0].udp_endpoints, 1);
        assert_eq!(v[1].established, 1);
        assert!(by_process(&[]).is_empty());
    }

    #[test]
    fn port_and_addr_byte_order() {
        assert_eq!(port(0x5000), 80);
        assert_eq!(v4(u32::from_le_bytes([192, 168, 1, 2])), ip("192.168.1.2"));
    }

    #[test]
    fn sanitizes() {
        assert_eq!(clean("a\u{0}b\nc"), "abc");
        assert_eq!(clean(&"x".repeat(1000)).len(), MAX_STR);
    }

    #[test]
    #[ignore]
    fn live_connections() {
        let c = connections();
        let by = by_process(&c);
        println!("conns={} tcp={} udp={} procs={}", c.len(), c.iter().filter(|x| x.protocol == Protocol::Tcp).count(), c.iter().filter(|x| x.protocol == Protocol::Udp).count(), by.len());
        for p in by.iter().take(5) {
            println!("{} established={} public_remotes={}", p.process, p.established, p.public_remotes);
        }
    }
}
