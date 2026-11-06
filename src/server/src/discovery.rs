//! Opt-in LAN discovery: asks devices on tikontrol's own LAN what they are, over mDNS/DNS-SD
//! (224.0.0.251:5353) and SSDP/UPnP (239.255.255.250:1900). Off by default: an admin runs it on demand
//! (at most once per `MIN_GAP`) or turns on the `EVERY` schedule.
//!
//! - Nothing leaves the LAN: both groups are link-local multicast, answers are only taken from private or
//!   link-local IPv4 sources, and a UPnP description is fetched only from the responder's own private
//!   address over plain HTTP, without redirects, capped at 64 KB and 2 s.
//! - Every value is untrusted device text: control characters stripped, `MAX_KEYS` keys per device and
//!   `MAX_VAL` chars per value. It is shown as text only and never used to build RouterOS commands.
//! - Results live in `discovery.json` next to `history.json`, keyed by IP, with the MAC from ARP/leases.

use crate::api::body;
use crate::app::{lock, now, App};
use crate::auth::Session;
use crate::http::{Req, Resp};
use crate::store::{write_private, Role};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const MIN_GAP: u64 = 60;
pub const EVERY: u64 = 600;
const MAX_KEYS: usize = 40;
const MAX_VAL: usize = 256;
const MAX_DEVICES: usize = 512;
const LISTEN: Duration = Duration::from_millis(2500);
const FETCH: Duration = Duration::from_secs(2);
const MAX_XML: u64 = 64 * 1024;
const MDNS: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const SSDP: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const META: &str = "_services._dns-sd._udp.local";
const TYPES: &[&str] = &[
    "_device-info._tcp", "_airplay._tcp", "_raop._tcp", "_googlecast._tcp", "_ipp._tcp", "_printer._tcp", "_ssh._tcp",
    "_smb._tcp", "_hap._tcp", "_spotify-connect._tcp", "_companion-link._tcp", "_homekit._tcp", "_http._tcp",
];
/// TXT keys worth showing: models and names, not ids or capability flags
const TXT_KEYS: &[&str] = &["model", "md", "fn", "ty", "product", "usb_MFG", "usb_MDL", "manufacturer", "mfr", "rpMd", "am", "note"];
const UPNP: &[&str] = &["friendlyName", "manufacturer", "modelName", "modelNumber", "deviceType"];

/// (source, key, value)
pub type Kv = (String, String, String);

#[derive(Serialize, Deserialize, Clone)]
pub struct Dev {
    pub ip: String,
    pub mac: Option<String>,
    pub t: u64,
    pub kv: Vec<Kv>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Saved {
    auto: bool,
    devices: Vec<Dev>,
}

#[derive(Default)]
struct St {
    saved: Option<Saved>,
    running: bool,
    last: u64,
}

fn st() -> &'static Mutex<St> {
    static S: OnceLock<Mutex<St>> = OnceLock::new();
    S.get_or_init(Mutex::default)
}

fn path(app: &App) -> PathBuf {
    lock(&app.store).dir().join("discovery.json")
}

fn saved<'a>(app: &App, g: &'a mut St) -> &'a mut Saved {
    g.saved.get_or_insert_with(|| std::fs::read(path(app)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default())
}

pub fn route(app: &Arc<App>, req: &Req, s: &Session) -> Option<Resp> {
    if req.path != "/api/v1/discovery" && req.path != "/api/v1/discovery/run" {
        return None;
    }
    let admin = s.role == Role::Admin;
    Some(match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/v1/discovery") => {
            let mut g = lock(st());
            let (running, last) = (g.running, g.last);
            let sv = saved(app, &mut g);
            Resp::json(200, &json!({ "auto": sv.auto, "running": running, "last": last, "wait": (last + MIN_GAP).saturating_sub(now()), "minGap": MIN_GAP, "devices": sv.devices }))
        }
        ("PUT" | "POST", _) if !admin => Resp::error(403, "admin role required"),
        ("PUT", "/api/v1/discovery") => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Cfg {
                auto: bool,
            }
            let b: Cfg = match body(req) {
                Ok(b) => b,
                Err(r) => return Some(r),
            };
            let mut g = lock(st());
            let sv = saved(app, &mut g);
            sv.auto = b.auto;
            crate::api::saved(write_private(&path(app), &serde_json::to_vec(sv).unwrap_or_default()))
        }
        ("POST", "/api/v1/discovery/run") => match run(app) {
            Ok(n) => Resp::json(200, &json!({ "found": n })),
            Err(wait) => Resp::error(429, "discovery ran recently or is running; try again later").with("retry-after", wait.to_string()),
        },
        _ => Resp::error(405, "use GET or PUT on /discovery, POST on /discovery/run"),
    })
}

/// Runs one discovery; Err(seconds to wait) when one ran less than `MIN_GAP` ago or is running.
fn run(app: &Arc<App>) -> Result<usize, u64> {
    {
        let mut g = lock(st());
        let t = now();
        if g.running {
            return Err(MIN_GAP);
        }
        if g.last > 0 && t < g.last + MIN_GAP {
            return Err(g.last + MIN_GAP - t);
        }
        g.running = true;
        g.last = t;
    }
    let found = discover();
    let macs = macs(app);
    let mut g = lock(st());
    g.running = false;
    let t = now();
    let sv = saved(app, &mut g);
    for (ip, kv) in &found {
        let ip = ip.to_string();
        sv.devices.retain(|d| d.ip != ip);
        sv.devices.push(Dev { mac: macs.get(&ip).cloned(), ip, t, kv: kv.clone() });
    }
    sv.devices.sort_by_key(|d| std::cmp::Reverse(d.t));
    sv.devices.truncate(MAX_DEVICES);
    let _ = write_private(&path(app), &serde_json::to_vec(sv).unwrap_or_default());
    Ok(found.len())
}

/// IP -> MAC from the router's ARP table and DHCP leases.
fn macs(app: &App) -> HashMap<String, String> {
    let snap = app.snap.read().map(|s| s.data.clone()).unwrap_or_default();
    let mut m = HashMap::new();
    if let Some(d) = snap.as_deref().or(app.demo.as_deref()) {
        for k in ["arp", "leases"] {
            for e in d[k].as_array().into_iter().flatten() {
                if let (Some(ip), Some(mac)) = (e["address"].as_str(), e["mac-address"].as_str()) {
                    m.entry(ip.to_string()).or_insert_with(|| mac.to_string());
                }
            }
        }
    }
    m
}

/// The optional schedule: every `EVERY` seconds while the setting is on.
pub fn spawn(app: Arc<App>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(60));
        let due = {
            let mut g = lock(st());
            let last = g.last;
            saved(&app, &mut g).auto && now() >= last + EVERY
        };
        if due {
            let _ = run(&app);
        }
    });
}

fn lan(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_link_local()
}

#[derive(Default)]
struct Found(BTreeMap<Ipv4Addr, Vec<Kv>>);

impl Found {
    fn add(&mut self, ip: Ipv4Addr, src: &str, key: &str, val: &str) {
        let clean = |s: &str, n: usize| s.chars().filter(|c| !c.is_control()).take(n).collect::<String>().trim().to_string();
        let (k, v) = (clean(key, 64), clean(val, MAX_VAL));
        if !lan(ip) || k.is_empty() || v.is_empty() {
            return;
        }
        let kv = self.0.entry(ip).or_default();
        if kv.len() < MAX_KEYS && !kv.iter().any(|(s, a, b)| s == src && *a == k && *b == v) {
            kv.push((src.into(), k, v));
        }
    }
}

/// mDNS and SSDP side by side for `LISTEN`, then the UPnP descriptions.
fn discover() -> BTreeMap<Ipv4Addr, Vec<Kv>> {
    let deadline = Instant::now() + LISTEN;
    let (mut a, mut b) = (Found::default(), Found::default());
    let locs = std::thread::scope(|sc| {
        let h = sc.spawn(|| mdns(&mut a, deadline));
        let l = ssdp(&mut b, deadline);
        let _ = h.join();
        l
    });
    for (ip, kv) in b.0 {
        for (s, k, v) in kv {
            a.add(ip, &s, &k, &v);
        }
    }
    let end = Instant::now() + Duration::from_secs(8);
    for (ip, addr, path) in locs.into_iter().take(32) {
        if Instant::now() > end {
            break;
        }
        if let Some(xml) = fetch(addr, &path) {
            for (k, v) in upnp(&xml) {
                a.add(ip, "ssdp", k, &v);
            }
        }
    }
    a.0
}

// ---------- mDNS / DNS-SD ----------

pub fn query(name: &str, qtype: u16) -> Vec<u8> {
    let mut p = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
    for l in name.split('.') {
        p.push(l.len() as u8);
        p.extend(l.as_bytes());
    }
    p.push(0);
    p.extend(qtype.to_be_bytes());
    p.extend(1u16.to_be_bytes());
    p
}

#[derive(Debug, PartialEq)]
pub enum Rd {
    Ptr(String),
    Srv(u16, String),
    Txt(Vec<String>),
    A(Ipv4Addr),
    Other,
}

/// A name at `i`, following compression pointers (at most 16 jumps, 255 chars); the offset after it.
fn name(p: &[u8], mut i: usize) -> Option<(String, usize)> {
    let (mut out, mut end, mut jumps) = (String::new(), None, 0);
    loop {
        let len = *p.get(i)? as usize;
        if len & 0xC0 == 0xC0 {
            end.get_or_insert(i + 2);
            jumps += 1;
            if jumps > 16 {
                return None;
            }
            i = ((len & 0x3F) << 8) | *p.get(i + 1)? as usize;
            continue;
        }
        if len & 0xC0 != 0 {
            return None;
        }
        if len == 0 {
            return Some((out, end.unwrap_or(i + 1)));
        }
        if !out.is_empty() {
            out.push('.');
        }
        out.push_str(&String::from_utf8_lossy(p.get(i + 1..i + 1 + len)?));
        if out.len() > 255 {
            return None;
        }
        i += 1 + len;
    }
}

/// The resource records of a DNS response (answers + authority + additional); None if malformed.
pub fn parse(p: &[u8]) -> Option<Vec<(String, Rd)>> {
    let n = |i: usize| -> Option<usize> { Some(u16::from_be_bytes([*p.get(i)?, *p.get(i + 1)?]) as usize) };
    if n(2)? & 0x8000 == 0 {
        return Some(vec![]); // a query (maybe our own), not an answer
    }
    let (qd, rr) = (n(4)?, n(6)? + n(8)? + n(10)?);
    let mut i = 12;
    for _ in 0..qd {
        i = name(p, i)?.1 + 4;
    }
    let mut out = Vec::new();
    for _ in 0..rr.min(256) {
        let (nm, j) = name(p, i)?;
        let (ty, len) = (n(j)?, n(j + 8)?);
        let s = j + 10;
        let rd = p.get(s..s + len)?;
        out.push((nm, match ty {
            12 => Rd::Ptr(name(p, s)?.0),
            33 if len >= 6 => Rd::Srv(n(s + 4)? as u16, name(p, s + 6)?.0),
            16 => {
                let (mut v, mut k) = (Vec::new(), 0);
                while k < rd.len() {
                    let l = rd[k] as usize;
                    v.push(String::from_utf8_lossy(rd.get(k + 1..k + 1 + l)?).into_owned());
                    k += 1 + l;
                }
                Rd::Txt(v)
            }
            1 if len == 4 => Rd::A(Ipv4Addr::new(rd[0], rd[1], rd[2], rd[3])),
            _ => Rd::Other,
        }));
        i = s + len;
    }
    Some(out)
}

/// What one mDNS answer from `src` says, attributed to the address in its own SRV/A records when it
/// answers for another device (e.g. a sleep proxy). Returns service types it advertised.
fn absorb(f: &mut Found, src: Ipv4Addr, recs: &[(String, Rd)]) -> Vec<String> {
    let a: HashMap<&str, Ipv4Addr> = recs.iter().filter_map(|(n, r)| if let Rd::A(ip) = r { Some((n.as_str(), *ip)) } else { None }).collect();
    let srv: HashMap<&str, &str> = recs.iter().filter_map(|(n, r)| if let Rd::Srv(_, t) = r { Some((n.as_str(), t.as_str())) } else { None }).collect();
    let owner = |inst: &str| srv.get(inst).and_then(|t| a.get(t)).copied().filter(|ip| lan(*ip)).unwrap_or(src);
    let mut types = Vec::new();
    for (n, r) in recs {
        match r {
            Rd::Ptr(t) if n == META => types.push(t.clone()),
            Rd::Ptr(inst) if n.ends_with("._tcp.local") || n.ends_with("._udp.local") => {
                let ty = n.trim_end_matches(".local");
                f.add(owner(inst), "mdns", "service", ty);
                f.add(owner(inst), "mdns", "name", inst.strip_suffix(&format!(".{n}")).unwrap_or(inst));
            }
            Rd::Srv(_, target) => f.add(owner(n), "mdns", "host", target.trim_end_matches(".local")),
            Rd::Txt(v) => {
                for (k, val) in v.iter().filter_map(|s| s.split_once('=')) {
                    if TXT_KEYS.contains(&k) {
                        f.add(owner(n), "mdns", k, val);
                    }
                }
            }
            _ => {}
        }
    }
    types
}

fn mdns(f: &mut Found, deadline: Instant) {
    // on 5353 we also hear announcements; if it's taken, answers still come back to our own port
    let Ok(sock) = UdpSocket::bind("0.0.0.0:5353").or_else(|_| UdpSocket::bind("0.0.0.0:0")) else { return };
    let _ = sock.join_multicast_v4(&MDNS, &Ipv4Addr::UNSPECIFIED);
    let _ = sock.set_multicast_ttl_v4(255);
    let _ = sock.set_multicast_loop_v4(false);
    let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));
    let mut asked: Vec<String> = TYPES.iter().map(|t| format!("{t}.local")).collect();
    let _ = sock.send_to(&query(META, 12), (MDNS, 5353));
    for t in &asked {
        let _ = sock.send_to(&query(t, 12), (MDNS, 5353));
    }
    let mut buf = [0u8; 9000];
    while Instant::now() < deadline {
        let Ok((len, std::net::SocketAddr::V4(from))) = sock.recv_from(&mut buf) else { continue };
        if !lan(*from.ip()) {
            continue;
        }
        let Some(recs) = parse(&buf[..len]) else { continue };
        for t in absorb(f, *from.ip(), &recs) {
            if asked.len() < 40 && !asked.contains(&t) && t.ends_with(".local") && t.len() < 100 {
                let _ = sock.send_to(&query(&t, 12), (MDNS, 5353));
                asked.push(t);
            }
        }
    }
}

// ---------- SSDP / UPnP ----------

pub fn header<'a>(msg: &'a str, key: &str) -> Option<&'a str> {
    msg.lines().filter_map(|l| l.split_once(':')).find(|(k, _)| k.trim().eq_ignore_ascii_case(key)).map(|(_, v)| v.trim())
}

/// A LOCATION URL we may fetch: plain http, an IPv4 literal equal to the responder's private address.
pub fn location(url: &str, src: Ipv4Addr) -> Option<(SocketAddrV4, String)> {
    let rest = url.get(..7).filter(|s| s.eq_ignore_ascii_case("http://")).and(url.get(7..))?;
    let (hostport, path) = rest.find('/').map_or((rest, "/"), |i| (&rest[..i], &rest[i..]));
    let (host, port) = hostport.split_once(':').map_or(Some((hostport, 80)), |(h, p)| p.parse().ok().map(|p| (h, p)))?;
    let ip: Ipv4Addr = host.parse().ok()?;
    if ip != src || !lan(ip) || path.len() > 512 || path.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    Some((SocketAddrV4::new(ip, port), path.to_string()))
}

fn ssdp(f: &mut Found, deadline: Instant) -> Vec<(Ipv4Addr, SocketAddrV4, String)> {
    let mut locs = Vec::new();
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else { return locs };
    let _ = sock.set_multicast_ttl_v4(2);
    let _ = sock.set_read_timeout(Some(Duration::from_millis(200)));
    let _ = sock.send_to(b"M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: ssdp:all\r\n\r\n", (SSDP, 1900));
    let mut buf = [0u8; 4096];
    while Instant::now() < deadline {
        let Ok((len, std::net::SocketAddr::V4(from))) = sock.recv_from(&mut buf) else { continue };
        let ip = *from.ip();
        let msg = String::from_utf8_lossy(&buf[..len]);
        if !lan(ip) {
            continue;
        }
        if let Some(s) = header(&msg, "server") {
            f.add(ip, "ssdp", "server", s);
        }
        if let Some(l) = header(&msg, "location").and_then(|u| location(u, ip)) {
            if !locs.iter().any(|(_, a, p)| *a == l.0 && *p == l.1) && locs.iter().filter(|(i, ..)| *i == ip).count() < 4 {
                locs.push((ip, l.0, l.1));
            }
        }
    }
    locs
}

fn fetch(addr: SocketAddrV4, path: &str) -> Option<String> {
    let end = Instant::now() + FETCH;
    let mut s = TcpStream::connect_timeout(&addr.into(), FETCH).ok()?;
    s.set_write_timeout(Some(FETCH)).ok()?;
    write!(s, "GET {path} HTTP/1.0\r\nHost: {addr}\r\nConnection: close\r\n\r\n").ok()?;
    let (mut out, mut buf) = (Vec::new(), [0u8; 4096]);
    let mut s = s.take(MAX_XML);
    while let Some(left) = end.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()) {
        s.get_ref().set_read_timeout(Some(left)).ok()?;
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
        }
    }
    let text = String::from_utf8_lossy(&out).into_owned();
    let status = text.lines().next()?;
    (status.starts_with("HTTP/") && status.split_whitespace().nth(1) == Some("200")).then(|| text.split_once("\r\n\r\n").map(|(_, b)| b.to_string()))?
}

/// friendlyName, manufacturer, modelName, modelNumber and deviceType of the root device.
pub fn upnp(xml: &str) -> Vec<(&'static str, String)> {
    UPNP.iter()
        .filter_map(|t| {
            let a = xml.find(&format!("<{t}>"))? + t.len() + 2;
            let b = a + xml[a..].find("</")?;
            Some((*t, xml[a..b].replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_parser_reads_dns_sd_answers_and_rejects_loops() {
        // response: PTR _ipp._tcp.local -> "Office._ipp._tcp.local", SRV -> printer.local:631, TXT ty=, A 192.168.1.20
        let mut p = vec![0, 0, 0x84, 0, 0, 0, 0, 4, 0, 0, 0, 0];
        let ty = p.len();
        for l in ["_ipp", "_tcp", "local"] {
            p.push(l.len() as u8);
            p.extend(l.as_bytes());
        }
        p.push(0);
        p.extend([0, 12, 0, 1, 0, 0, 0, 120, 0, 9, 6]);
        let inst = p.len() - 1;
        p.extend(b"Office");
        p.extend([0xC0, ty as u8]);
        p.extend([0xC0, inst as u8, 0, 33, 0, 1, 0, 0, 0, 120, 0, 16, 0, 0, 0, 0, 2, 0x77, 7]);
        let host = p.len() - 1;
        p.extend(b"printer");
        p.extend([0xC0, (ty + 10) as u8]);
        p.extend([0xC0, inst as u8, 0, 16, 0, 1, 0, 0, 0, 120, 0, 17, 16]);
        p.extend(b"ty=Acme Laser 1\x01");
        p.extend([0xC0, host as u8, 0, 1, 0, 1, 0, 0, 0, 120, 0, 4, 192, 168, 1, 20]);
        let recs = parse(&p).unwrap();
        assert_eq!(recs[0], ("_ipp._tcp.local".into(), Rd::Ptr("Office._ipp._tcp.local".into())));
        assert_eq!(recs[1].1, Rd::Srv(631, "printer.local".into()));
        assert_eq!(recs[3].1, Rd::A(Ipv4Addr::new(192, 168, 1, 20)));
        let mut f = Found::default();
        absorb(&mut f, Ipv4Addr::new(192, 168, 1, 99), &recs); // answered by a proxy for .20
        let kv = &f.0[&Ipv4Addr::new(192, 168, 1, 20)];
        assert!(kv.contains(&("mdns".into(), "name".into(), "Office".into())));
        assert!(kv.contains(&("mdns".into(), "ty".into(), "Acme Laser 1".into()))); // control char stripped
        assert!(kv.contains(&("mdns".into(), "host".into(), "printer".into())));
        // a name pointing at itself, a pointer past the end, a truncated record: rejected, no panic or hang
        assert_eq!(parse(&[0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0xC0, 12, 0, 1]), None);
        assert_eq!(parse(&[0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0xC0, 200]), None);
        assert_eq!(parse(&p[..p.len() - 3]), None);
        assert_eq!(parse(&query("_ipp._tcp.local", 12)), Some(vec![])); // a query, not an answer
    }

    #[test]
    fn ssdp_headers_location_and_upnp_xml() {
        let src = Ipv4Addr::new(192, 168, 1, 30);
        let msg = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nLocation: http://192.168.1.30:49152/desc.xml\r\nSERVER: Linux UPnP/1.0 Acme/2\r\n\r\n";
        assert_eq!(header(msg, "server"), Some("Linux UPnP/1.0 Acme/2"));
        assert_eq!(location(header(msg, "location").unwrap(), src), Some((SocketAddrV4::new(src, 49152), "/desc.xml".into())));
        assert_eq!(location("http://192.168.1.31/x.xml", src), None); // not the responder
        assert_eq!(location("http://8.8.8.8/x.xml", Ipv4Addr::new(8, 8, 8, 8)), None); // not the LAN
        assert_eq!(location("https://192.168.1.30/x.xml", src), None);
        assert_eq!(location("http://evil.example/x.xml", src), None);
        assert_eq!(location("http://192.168.1.30/x\r\nHost: y", src), None);
        let xml = "<root><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><friendlyName>Living &amp; Room</friendlyName><manufacturer>Acme</manufacturer><modelName>Box 4K</modelName></device></root>";
        assert_eq!(upnp(xml), vec![("friendlyName", "Living & Room".into()), ("manufacturer", "Acme".into()), ("modelName", "Box 4K".into()), ("deviceType", "urn:schemas-upnp-org:device:MediaRenderer:1".into())]);
        let mut f = Found::default();
        f.add(src, "ssdp", "x", &"a\u{7}".repeat(400));
        f.add(Ipv4Addr::new(1, 1, 1, 1), "ssdp", "x", "y");
        assert_eq!(f.0[&src][0].2, "a".repeat(MAX_VAL)); // control chars stripped, then capped
        assert!(!f.0.contains_key(&Ipv4Addr::new(1, 1, 1, 1)));
    }
}
