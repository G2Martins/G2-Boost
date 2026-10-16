//! ICMP diagnostics via `iphlpapi` (`IcmpSendEcho2`): no raw sockets, no admin, no `ping.exe`.
//! Same mechanism ExitLag uses in-process (Docs/references/binary-analysis.md).

use std::ffi::c_void;
use std::mem::size_of;
use std::net::{Ipv4Addr, ToSocketAddrs};

use serde::Serialize;
use windows::Win32::NetworkManagement::IpHelper::{
    ICMP_ECHO_REPLY, IP_OPTION_INFORMATION, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho2,
};

const IP_SUCCESS: u32 = 0;
const IP_REQ_TIMED_OUT: u32 = 11010;
const IP_TTL_EXPIRED_TRANSIT: u32 = 11013;
const PAYLOAD: &[u8] = b"g2boost-probe-32-bytes-of-data!!";

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("could not resolve host")]
    Resolve,
    #[error("ICMP handle: {0}")]
    Icmp(String),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Probe {
    Reply { from: Ipv4Addr, rtt_ms: u32 },
    TtlExpired { from: Ipv4Addr, rtt_ms: u32 },
    Timeout,
    Error { status: u32 },
}

/// Resolve a host name or dotted IPv4 to an address (first IPv4 answer).
pub fn resolve_v4(host: &str) -> Result<Ipv4Addr, NetError> {
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return Ok(ip);
    }
    let host = host.trim();
    if host.is_empty() || host.len() > 253 {
        return Err(NetError::Resolve);
    }
    (host, 0u16)
        .to_socket_addrs()
        .map_err(|_| NetError::Resolve)?
        .find_map(|a| match a {
            std::net::SocketAddr::V4(v4) => Some(*v4.ip()),
            _ => None,
        })
        .ok_or(NetError::Resolve)
}

/// One echo request with the given TTL.
pub fn probe_v4(dst: Ipv4Addr, ttl: u8, timeout_ms: u32) -> Result<Probe, NetError> {
    // SAFETY: every pointer handed to IcmpSendEcho2 points into buffers that outlive the call.
    unsafe {
        let handle = IcmpCreateFile().map_err(|e| NetError::Icmp(e.to_string()))?;
        let opts = IP_OPTION_INFORMATION {
            Ttl: ttl,
            Tos: 0,
            Flags: 0,
            OptionsSize: 0,
            OptionsData: std::ptr::null_mut(),
        };
        // Reply buffer: one ICMP_ECHO_REPLY + echoed payload + 8 bytes for ICMP error data.
        let reply_size = size_of::<ICMP_ECHO_REPLY>() + PAYLOAD.len() + 8;
        let mut reply = vec![0u8; reply_size];
        let dest = u32::from_ne_bytes(dst.octets());
        let n = IcmpSendEcho2(
            handle,
            None,
            None,
            None,
            dest,
            PAYLOAD.as_ptr() as *const c_void,
            PAYLOAD.len() as u16,
            Some(&opts),
            reply.as_mut_ptr() as *mut c_void,
            reply_size as u32,
            timeout_ms,
        );
        let _ = IcmpCloseHandle(handle);
        if n == 0 {
            // No reply records: timeout (ERROR 11010) or another failure code in GetLastError.
            let err = windows::Win32::Foundation::GetLastError().0;
            return Ok(if err == IP_REQ_TIMED_OUT { Probe::Timeout } else { Probe::Error { status: err } });
        }
        let r = &*(reply.as_ptr() as *const ICMP_ECHO_REPLY);
        let from = Ipv4Addr::from(r.Address.to_ne_bytes());
        Ok(match r.Status {
            IP_SUCCESS => Probe::Reply { from, rtt_ms: r.RoundTripTime },
            IP_TTL_EXPIRED_TRANSIT => Probe::TtlExpired { from, rtt_ms: r.RoundTripTime },
            IP_REQ_TIMED_OUT => Probe::Timeout,
            status => Probe::Error { status },
        })
    }
}

/// Plain ping (TTL 128).
pub fn ping(dst: Ipv4Addr, timeout_ms: u32) -> Result<Probe, NetError> {
    probe_v4(dst, 128, timeout_ms)
}

#[derive(Debug, Clone, Serialize)]
pub struct Hop {
    pub ttl: u8,
    pub addr: Option<Ipv4Addr>,
    /// One entry per probe; `None` = timeout.
    pub rtts_ms: Vec<Option<u32>>,
}

/// Traceroute/MTR by increasing TTL. Stops at the first hop that answers with an echo reply.
pub fn trace_route(dst: Ipv4Addr, max_hops: u8, probes: u8, timeout_ms: u32) -> Result<Vec<Hop>, NetError> {
    let mut hops = Vec::new();
    for ttl in 1..=max_hops.max(1) {
        let mut hop = Hop { ttl, addr: None, rtts_ms: Vec::new() };
        let mut reached = false;
        for _ in 0..probes.max(1) {
            match probe_v4(dst, ttl, timeout_ms)? {
                Probe::Reply { from, rtt_ms } => {
                    hop.addr = Some(from);
                    hop.rtts_ms.push(Some(rtt_ms));
                    reached = true;
                }
                Probe::TtlExpired { from, rtt_ms } => {
                    hop.addr = Some(from);
                    hop.rtts_ms.push(Some(rtt_ms));
                }
                Probe::Timeout | Probe::Error { .. } => hop.rtts_ms.push(None),
            }
        }
        hops.push(hop);
        if reached {
            break;
        }
    }
    Ok(hops)
}

/// Connection-health numbers the UI shows (latency / loss / jitter / spikes).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LatencyStats {
    pub samples: usize,
    pub avg_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    /// Mean absolute difference between consecutive replies.
    pub jitter_ms: f64,
    pub loss_pct: f64,
    /// Replies above 2× the median (and at least 10 ms above it).
    pub spikes: usize,
}

pub fn latency_stats(samples: &[Option<f64>]) -> LatencyStats {
    let ok: Vec<f64> = samples.iter().filter_map(|s| *s).collect();
    let n = samples.len();
    if ok.is_empty() {
        return LatencyStats {
            samples: n,
            avg_ms: f64::NAN,
            min_ms: f64::NAN,
            max_ms: f64::NAN,
            jitter_ms: f64::NAN,
            loss_pct: if n == 0 { 0.0 } else { 100.0 },
            spikes: 0,
        };
    }
    let mut sorted = ok.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let median = if sorted.len() % 2 == 1 {
        sorted[sorted.len() / 2]
    } else {
        (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
    };
    let avg = ok.iter().sum::<f64>() / ok.len() as f64;
    let jitter = if ok.len() < 2 {
        0.0
    } else {
        ok.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (ok.len() - 1) as f64
    };
    LatencyStats {
        samples: n,
        avg_ms: avg,
        min_ms: sorted[0],
        max_ms: sorted[sorted.len() - 1],
        jitter_ms: jitter,
        loss_pct: 100.0 * (n - ok.len()) as f64 / n as f64,
        spikes: ok.iter().filter(|&&v| v > median * 2.0 && v > median + 10.0).count(),
    }
}

/// TCPing: time to complete a TCP handshake with `ip:port` (works where ICMP is filtered,
/// e.g. game servers). `None` = timeout/refused. No admin, no raw sockets.
pub fn tcping(ip: Ipv4Addr, port: u16, timeout_ms: u32) -> Option<f64> {
    let addr = std::net::SocketAddr::from((ip, port));
    let t0 = std::time::Instant::now();
    let stream = std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(u64::from(timeout_ms))).ok()?;
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Some(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcping_reaches_a_local_listener() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(tcping(Ipv4Addr::LOCALHOST, port, 1000).is_some());
        drop(l);
        assert!(tcping(Ipv4Addr::LOCALHOST, port, 300).is_none(), "closed port");
    }

    #[test]
    fn loopback_replies() {
        match ping(Ipv4Addr::LOCALHOST, 1000).expect("icmp handle") {
            Probe::Reply { from, rtt_ms } => {
                assert_eq!(from, Ipv4Addr::LOCALHOST);
                assert!(rtt_ms < 100);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn trace_to_loopback_is_one_hop() {
        let hops = trace_route(Ipv4Addr::LOCALHOST, 5, 1, 500).unwrap();
        assert_eq!(hops.len(), 1);
        assert_eq!(hops[0].addr, Some(Ipv4Addr::LOCALHOST));
    }

    #[test]
    fn resolve_rejects_garbage() {
        assert!(resolve_v4("").is_err());
        assert_eq!(resolve_v4("8.8.8.8").unwrap(), Ipv4Addr::new(8, 8, 8, 8));
        assert_eq!(resolve_v4("localhost").unwrap(), Ipv4Addr::LOCALHOST);
    }

    #[test]
    fn stats_count_loss_jitter_and_spikes() {
        let s = latency_stats(&[Some(20.0), Some(22.0), None, Some(21.0), Some(90.0)]);
        assert_eq!(s.samples, 5);
        assert!((s.loss_pct - 20.0).abs() < 1e-9);
        assert_eq!(s.min_ms, 20.0);
        assert_eq!(s.max_ms, 90.0);
        assert_eq!(s.spikes, 1);
        assert!(s.jitter_ms > 0.0);
        let empty = latency_stats(&[None, None]);
        assert_eq!(empty.loss_pct, 100.0);
        assert!(empty.avg_ms.is_nan());
    }
}
