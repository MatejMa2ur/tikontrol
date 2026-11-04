//! Opt-in NetFlow v5 collector for Insights (top talkers, destinations, ports). Off unless TIK_FLOWS
//! is set (e.g. `0.0.0.0:2055`); RouterOS then needs `/ip/traffic-flow` enabled with a target of
//! tikontrol's address and version=5, which costs router CPU. Packets from anyone but the configured
//! router are dropped. Only aggregates are kept in memory, nothing is written to disk.

use crate::app::{lock, now, App};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flow {
    pub src: Ipv4Addr,
    pub dst: Ipv4Addr,
    pub sport: u16,
    pub dport: u16,
    pub proto: u8,
    pub bytes: u64,
    pub packets: u64,
}

/// Parses one NetFlow v5 datagram (24-byte header + 48-byte records). Anything malformed is rejected.
pub fn parse_v5(b: &[u8]) -> Option<Vec<Flow>> {
    if b.len() < 24 || u16::from_be_bytes([b[0], b[1]]) != 5 {
        return None;
    }
    let count = u16::from_be_bytes([b[2], b[3]]) as usize;
    if count == 0 || count > 30 || b.len() != 24 + count * 48 {
        return None;
    }
    let ip = |o: usize| Ipv4Addr::new(b[o], b[o + 1], b[o + 2], b[o + 3]);
    let u32at = |o: usize| u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as u64;
    let u16at = |o: usize| u16::from_be_bytes([b[o], b[o + 1]]);
    Some(
        (0..count)
            .map(|i| {
                let r = 24 + i * 48;
                Flow { src: ip(r), dst: ip(r + 4), packets: u32at(r + 16), bytes: u32at(r + 20), sport: u16at(r + 32), dport: u16at(r + 34), proto: b[r + 38] }
            })
            .collect(),
    )
}

const MAX_KEYS: usize = 5000;

#[derive(Default)]
pub struct Agg {
    pub since: u64,
    pub talkers: HashMap<Ipv4Addr, (u64, u64)>,
    pub dests: HashMap<Ipv4Addr, u64>,
    pub ports: HashMap<(u8, u16), u64>,
    pub flows: u64,
}

impl Agg {
    pub fn add(&mut self, f: &Flow) {
        self.flows += 1;
        // the LAN side is the private address; count its upload and download
        let (lan, remote, up) = if f.src.is_private() && !f.dst.is_private() {
            (f.src, f.dst, true)
        } else if f.dst.is_private() && !f.src.is_private() {
            (f.dst, f.src, false)
        } else {
            return; // LAN-to-LAN or transit: not internet traffic
        };
        if self.talkers.len() < MAX_KEYS || self.talkers.contains_key(&lan) {
            let t = self.talkers.entry(lan).or_default();
            if up { t.1 += f.bytes } else { t.0 += f.bytes }
        }
        if self.dests.len() < MAX_KEYS || self.dests.contains_key(&remote) {
            *self.dests.entry(remote).or_default() += f.bytes;
        }
        let port = if up { f.dport } else { f.sport };
        if self.ports.len() < MAX_KEYS || self.ports.contains_key(&(f.proto, port)) {
            *self.ports.entry((f.proto, port)).or_default() += f.bytes;
        }
    }

    pub fn top(&self, n: usize) -> Value {
        let mut t: Vec<_> = self.talkers.iter().map(|(ip, (d, u))| json!({ "ip": ip.to_string(), "down": d, "up": u })).collect();
        t.sort_by_key(|v| std::cmp::Reverse(v["down"].as_u64().unwrap_or(0) + v["up"].as_u64().unwrap_or(0)));
        t.truncate(n);
        let mut d: Vec<_> = self.dests.iter().collect();
        d.sort_by_key(|x| std::cmp::Reverse(*x.1));
        let mut p: Vec<_> = self.ports.iter().collect();
        p.sort_by_key(|x| std::cmp::Reverse(*x.1));
        json!({
            "since": self.since, "flows": self.flows, "talkers": t,
            "destinations": d.iter().take(n).map(|(ip, b)| json!({ "ip": ip.to_string(), "bytes": b })).collect::<Vec<_>>(),
            "ports": p.iter().take(n).map(|((proto, port), b)| json!({ "proto": proto_name(*proto), "port": port, "service": service(*port), "bytes": b })).collect::<Vec<_>>(),
        })
    }
}

fn proto_name(p: u8) -> &'static str {
    match p {
        6 => "tcp",
        17 => "udp",
        1 => "icmp",
        _ => "other",
    }
}

/// Rough app names by well-known port: a hint, not DPI.
pub fn service(port: u16) -> &'static str {
    match port {
        443 => "HTTPS",
        80 => "HTTP",
        53 => "DNS",
        853 => "DNS over TLS",
        123 => "NTP",
        22 => "SSH",
        25 | 465 | 587 | 993 | 995 | 143 | 110 => "Mail",
        3478 | 3479 | 19302..=19309 => "Calls (STUN)",
        5228 => "Google push",
        5223 => "Apple push",
        1935 => "Live streaming",
        3074 | 3075 | 27015..=27030 => "Gaming",
        51820 => "WireGuard",
        1194 => "OpenVPN",
        _ => "",
    }
}

pub fn agg() -> &'static Mutex<Agg> {
    static A: OnceLock<Mutex<Agg>> = OnceLock::new();
    A.get_or_init(|| Mutex::new(Agg { since: now(), ..Default::default() }))
}

pub fn spawn(app: Arc<App>, addr: &str) -> std::io::Result<()> {
    let sock = UdpSocket::bind(addr)?;
    std::thread::spawn(move || {
        let mut buf = [0u8; 1500];
        loop {
            let Ok((n, from)) = sock.recv_from(&mut buf) else { continue };
            // only the configured router may feed the collector
            let router: Option<IpAddr> = app.router_cfg().and_then(|r| r.host.parse().ok());
            if router != Some(from.ip()) {
                continue;
            }
            if let Some(flows) = parse_v5(&buf[..n]) {
                let mut a = lock(agg());
                for f in &flows {
                    a.add(f);
                }
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    type Rec = ([u8; 4], [u8; 4], u16, u16, u8, u32);

    fn datagram(recs: &[Rec]) -> Vec<u8> {
        let mut b = vec![0u8; 24];
        b[1] = 5;
        b[3] = recs.len() as u8;
        for (s, d, sp, dp, proto, bytes) in recs {
            let mut r = vec![0u8; 48];
            r[0..4].copy_from_slice(s);
            r[4..8].copy_from_slice(d);
            r[20..24].copy_from_slice(&bytes.to_be_bytes());
            r[32..34].copy_from_slice(&sp.to_be_bytes());
            r[34..36].copy_from_slice(&dp.to_be_bytes());
            r[38] = *proto;
            b.extend(r);
        }
        b
    }

    #[test]
    fn parses_and_aggregates() {
        let d = datagram(&[([192, 168, 88, 10], [203, 0, 113, 5], 50000, 443, 6, 1000), ([203, 0, 113, 5], [192, 168, 88, 10], 443, 50000, 6, 9000), ([192, 168, 88, 10], [192, 168, 88, 11], 1, 2, 6, 5)]);
        let flows = parse_v5(&d).unwrap();
        assert_eq!(flows.len(), 3);
        let mut a = Agg::default();
        flows.iter().for_each(|f| a.add(f));
        let t = a.top(5);
        assert_eq!(t["talkers"][0]["down"], 9000);
        assert_eq!(t["talkers"][0]["up"], 1000);
        assert_eq!(t["ports"][0]["service"], "HTTPS");
        assert_eq!(t["destinations"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn rejects_malformed() {
        assert!(parse_v5(&[0; 10]).is_none());
        let mut d = datagram(&[([1, 1, 1, 1], [2, 2, 2, 2], 1, 1, 6, 1)]);
        d.pop();
        assert!(parse_v5(&d).is_none());
        let mut v9 = datagram(&[([1, 1, 1, 1], [2, 2, 2, 2], 1, 1, 6, 1)]);
        v9[1] = 9;
        assert!(parse_v5(&v9).is_none());
    }
}
