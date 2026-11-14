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
//!
//! Device identification (Settings · System) is the master switch, on by default: off, the schedule and
//! "run now" are refused and the DHCP listener stops. The UI then shows no type/OS guesses either.
//! The optional DHCP listener (off by default) binds UDP 67 receive-only and keeps option 55/60/12/81 of
//! broadcast DISCOVER/REQUEST packets per MAC. It never sends. Port 67 is privileged: as UID 65532 the bind
//! only works where the network namespace allows it (Docker's own networks set
//! `net.ipv4.ip_unprivileged_port_start=0`) or where the image's permitted-only CAP_NET_BIND_SERVICE survives
//! (`bind_cap`); otherwise the error is shown in Settings and nothing else changes.

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

/// What one DHCP client asked for (from its broadcast DISCOVER/REQUEST). Untrusted device text, capped.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Fp {
    pub mac: String,
    pub t: u64,
    /// option 55, the parameter request list in the client's order, e.g. "1,3,6,15"
    pub prl: String,
    /// option 60, vendor class
    pub class: String,
    /// option 12, host name
    pub host: String,
    /// option 81, client FQDN
    pub fqdn: String,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    auto: bool,
    /// device identification (guesses in the UI, discovery, the DHCP listener); on by default
    ident: bool,
    /// passive DHCP listener on UDP 67; off by default
    dhcp: bool,
    devices: Vec<Dev>,
    fps: Vec<Fp>,
}

impl Default for Saved {
    fn default() -> Self {
        Saved { auto: false, ident: true, dhcp: false, devices: vec![], fps: vec![] }
    }
}

#[derive(Default)]
struct St {
    saved: Option<Saved>,
    running: bool,
    last: u64,
    /// the DHCP listener is bound, or why it could not bind
    listening: bool,
    dhcp_err: Option<String>,
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
            let (listening, err) = (g.listening, g.dhcp_err.clone());
            let sv = saved(app, &mut g);
            Resp::json(200, &json!({ "auto": sv.auto, "ident": sv.ident, "dhcp": sv.dhcp, "listening": listening, "dhcpError": err, "running": running, "last": last, "wait": (last + MIN_GAP).saturating_sub(now()), "minGap": MIN_GAP, "devices": sv.devices, "fingerprints": sv.fps }))
        }
        ("PUT" | "POST", _) if !admin => Resp::error(403, "admin role required"),
        ("PUT", "/api/v1/discovery") => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Cfg {
                auto: Option<bool>,
                ident: Option<bool>,
                dhcp: Option<bool>,
            }
            let b: Cfg = match body(req) {
                Ok(b) => b,
                Err(r) => return Some(r),
            };
            let mut g = lock(st());
            let sv = saved(app, &mut g);
            sv.auto = b.auto.unwrap_or(sv.auto);
            sv.ident = b.ident.unwrap_or(sv.ident);
            sv.dhcp = b.dhcp.unwrap_or(sv.dhcp);
            if b.dhcp == Some(false) || b.ident == Some(false) {
                sv.fps.clear(); // turning it off forgets what was heard
            }
            crate::api::saved(write_private(&path(app), &serde_json::to_vec(sv).unwrap_or_default()))
        }
        ("POST", "/api/v1/discovery/run") if !saved(app, &mut lock(st())).ident => Resp::error(409, "device identification is off (Settings · System)"),
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

/// The optional schedule (every `EVERY` seconds while the setting is on) and the DHCP listener.
pub fn spawn(app: Arc<App>) {
    let a = app.clone();
    std::thread::spawn(move || listen(&a));
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(60));
        let due = {
            let mut g = lock(st());
            let last = g.last;
            let sv = saved(&app, &mut g);
            sv.ident && sv.auto && now() >= last + EVERY
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

// ---------- passive DHCP listener ----------

const MAX_FPS: usize = 512;

fn wanted(app: &App) -> bool {
    let mut g = lock(st());
    let sv = saved(app, &mut g);
    sv.ident && sv.dhcp
}

/// Switches CAP_NET_BIND_SERVICE on or off in this thread's effective set, for the port 67 bind only. The image
/// grants it as permitted-only (`setcap cap_net_bind_service=+p`), so a runtime that drops the capability still
/// starts tikontrol; the bind then fails with the reason shown in Settings. No-op where it isn't permitted.
#[cfg(target_os = "linux")]
fn bind_cap(on: bool) {
    #[repr(C)]
    struct Header {
        version: u32,
        pid: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Data {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }
    extern "C" {
        fn capget(h: *mut Header, d: *mut Data) -> i32;
        fn capset(h: *mut Header, d: *const Data) -> i32;
    }
    const CAP_NET_BIND_SERVICE: u32 = 1 << 10;
    let mut h = Header { version: 0x2008_0522, pid: 0 }; // _LINUX_CAPABILITY_VERSION_3, this thread
    let mut d = [Data::default(); 2];
    // SAFETY: both pointers are valid for the duration of the call; version 3 uses two Data structs.
    unsafe {
        if capget(&mut h, d.as_mut_ptr()) != 0 || d[0].permitted & CAP_NET_BIND_SERVICE == 0 {
            return;
        }
        if on {
            d[0].effective |= CAP_NET_BIND_SERVICE
        } else {
            d[0].effective &= !CAP_NET_BIND_SERVICE
        }
        capset(&mut h, d.as_ptr());
    }
}

#[cfg(not(target_os = "linux"))]
fn bind_cap(_: bool) {}

/// Binds UDP 67 while the setting is on and records what clients ask for. Receive-only: never sends.
fn listen(app: &App) {
    loop {
        if !wanted(app) {
            let mut g = lock(st());
            (g.listening, g.dhcp_err) = (false, None);
            drop(g);
            std::thread::sleep(Duration::from_secs(5));
            continue;
        }
        bind_cap(true);
        let bound = UdpSocket::bind("0.0.0.0:67");
        bind_cap(false);
        let sock = match bound {
            Ok(s) => s,
            Err(e) => {
                let mut g = lock(st());
                (g.listening, g.dhcp_err) = (false, Some(format!("cannot listen on UDP port 67: {e}")));
                drop(g);
                std::thread::sleep(Duration::from_secs(60));
                continue;
            }
        };
        let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
        {
            let mut g = lock(st());
            (g.listening, g.dhcp_err) = (true, None);
        }
        let mut buf = [0u8; 1500];
        while wanted(app) {
            let Ok((len, std::net::SocketAddr::V4(from))) = sock.recv_from(&mut buf) else { continue };
            // clients send from port 68, from 0.0.0.0 before they have an address
            if from.port() != 68 || !(from.ip().is_unspecified() || lan(*from.ip())) {
                continue;
            }
            if let Some(fp) = dhcp(&buf[..len]) {
                remember(app, fp);
            }
        }
    }
}

fn remember(app: &App, mut fp: Fp) {
    let mut g = lock(st());
    let sv = saved(app, &mut g);
    let same = sv.fps.iter().find(|f| f.mac == fp.mac);
    // a REQUEST often drops fields its DISCOVER had: keep the earlier ones
    if let Some(old) = same {
        for (new, was) in [(&mut fp.prl, &old.prl), (&mut fp.class, &old.class), (&mut fp.host, &old.host), (&mut fp.fqdn, &old.fqdn)] {
            if new.is_empty() {
                new.clone_from(was);
            }
        }
    }
    let changed = same.is_none_or(|o| Fp { t: fp.t, ..o.clone() } != fp);
    sv.fps.retain(|f| f.mac != fp.mac);
    sv.fps.insert(0, fp);
    sv.fps.truncate(MAX_FPS);
    if changed {
        let _ = write_private(&path(app), &serde_json::to_vec(sv).unwrap_or_default());
    }
}

/// Device text from a DHCP option: control characters stripped, at most 64 chars.
fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).chars().filter(|c| !c.is_control()).take(64).collect::<String>().trim().to_string()
}

/// A client's BOOTREQUEST (DISCOVER, REQUEST or INFORM): its MAC and options 55/60/12/81. None for
/// anything else or anything malformed. Every read is bounds-checked; the option walk is bounded by the packet.
pub fn dhcp(p: &[u8]) -> Option<Fp> {
    // op=1 (request), htype=1 (Ethernet), hlen=6, magic cookie at 236
    if p.len() < 240 || p[0] != 1 || p[1] != 1 || p[2] != 6 || p[236..240] != [99, 130, 83, 99] {
        return None;
    }
    let mac = p[28..34].iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":");
    let mut fp = Fp { mac, t: now(), ..Default::default() };
    let (mut i, mut kind) = (240, 0);
    while let Some(&code) = p.get(i) {
        match code {
            0 => i += 1,
            255 => break,
            _ => {
                let len = *p.get(i + 1)? as usize;
                let v = p.get(i + 2..i + 2 + len)?;
                match code {
                    53 if len == 1 => kind = v[0],
                    55 => fp.prl = v.iter().take(64).map(u8::to_string).collect::<Vec<_>>().join(","),
                    60 => fp.class = text(v),
                    12 => fp.host = text(v),
                    // flags, 2 deprecated rcodes, then the name: DNS wire format if the E bit is set
                    81 if len > 3 => {
                        fp.fqdn = if v[0] & 4 != 0 {
                            let (mut out, mut k) = (Vec::new(), 3);
                            while let Some(&l) = v.get(k).filter(|&&l| l != 0 && l < 64) {
                                out.push(text(v.get(k + 1..k + 1 + l as usize)?));
                                k += 1 + l as usize;
                            }
                            text(out.join(".").as_bytes())
                        } else {
                            text(&v[3..])
                        }
                    }
                    _ => {}
                }
                i += 2 + len;
            }
        }
    }
    matches!(kind, 1 | 3 | 8).then_some(fp)
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

    /// A made-up DISCOVER: BOOTP header, cookie, then `opts`.
    fn packet(opts: &[u8]) -> Vec<u8> {
        let mut p = vec![0u8; 240];
        (p[0], p[1], p[2]) = (1, 1, 6);
        p[28..34].copy_from_slice(&[0x02, 0x00, 0x5E, 0x10, 0x20, 0x30]);
        p[236..240].copy_from_slice(&[99, 130, 83, 99]);
        p.extend(opts);
        p
    }

    #[test]
    fn dhcp_parser_reads_request_options_and_rejects_junk() {
        let mut o = vec![53, 1, 1, 0, 55, 4, 1, 3, 6, 15, 60, 8];
        o.extend(b"acme-os1");
        o.extend([12, 7]);
        o.extend(b"tv\x07-one");
        o.extend([81, 12, 4, 0, 0, 3]);
        o.extend(b"box");
        o.extend([3]);
        o.extend(b"lan");
        o.extend([0, 255, 1, 2, 3]);
        let f = dhcp(&packet(&o)).unwrap();
        assert_eq!(f.mac, "02:00:5E:10:20:30");
        assert_eq!((f.prl.as_str(), f.class.as_str(), f.host.as_str(), f.fqdn.as_str()), ("1,3,6,15", "acme-os1", "tv-one", "box.lan"));
        // FQDN in ASCII (E bit clear)
        let mut o2 = vec![53, 1, 3, 81, 7, 0, 0, 0];
        o2.extend(b"pc.x");
        assert_eq!(dhcp(&packet(&o2)).unwrap().fqdn, "pc.x");
        // not a request type we keep (OFFER from a server is op=2 anyway; DECLINE=4 here), no type at all
        assert_eq!(dhcp(&packet(&[53, 1, 4, 255])), None);
        assert_eq!(dhcp(&packet(&[55, 2, 1, 3, 255])), None);
        // an option running past the end, a short packet, a wrong cookie, a reply
        assert_eq!(dhcp(&packet(&[53, 1, 1, 55, 200, 1])), None);
        assert_eq!(dhcp(&[1u8; 100]), None);
        let mut bad = packet(&[53, 1, 1, 255]);
        bad[236] = 0;
        assert_eq!(dhcp(&bad), None);
        let mut reply = packet(&[53, 1, 1, 255]);
        reply[0] = 2;
        assert_eq!(dhcp(&reply), None);
        // long values are capped; a FQDN label pointing past the option ends the parse
        let mut long = vec![53, 1, 1, 12, 200];
        long.extend([b'a'; 200]);
        assert_eq!(dhcp(&packet(&long)).unwrap().host.len(), 64);
        assert_eq!(dhcp(&packet(&[53, 1, 1, 81, 5, 4, 0, 0, 9, b'a'])), None);
        // every truncation and every single-byte value of a real packet: never panics
        let p = packet(&o);
        for n in 0..p.len() {
            let _ = dhcp(&p[..n]);
        }
        for i in 240..p.len() {
            for b in 0..=255u8 {
                let mut q = p.clone();
                q[i] = b;
                let _ = dhcp(&q);
            }
        }
    }

    #[test]
    fn identification_settings_validate_and_switch_everything_off() {
        use crate::api::handle;
        use crate::api::tests::{admin_session, new_app, req, with_session};
        let app = new_app();
        let (c, csrf) = admin_session(&app);
        let call = |m: &str, p: &str, b: &str| handle(&app, with_session(req(m, p, b), &c, &csrf));
        let get = || match call("GET", "/api/v1/discovery", "").body {
            crate::http::Body::Bytes(b) => serde_json::from_slice::<serde_json::Value>(&b).unwrap(),
            _ => panic!("no body"),
        };
        assert_eq!((get()["ident"].as_bool(), get()["dhcp"].as_bool()), (Some(true), Some(false)), "guesses on, listening off by default");
        assert_eq!(call("PUT", "/api/v1/discovery", r#"{"dhcp":"yes"}"#).status, 400);
        assert_eq!(call("PUT", "/api/v1/discovery", r#"{"sniff":true}"#).status, 400);
        assert_eq!(call("PUT", "/api/v1/discovery", r#"{"dhcp":true}"#).status, 200);
        remember(&app, Fp { mac: "02:00:5E:00:00:01".into(), prl: "1,3".into(), ..Default::default() });
        assert_eq!(get()["fingerprints"][0]["prl"], "1,3");
        assert_eq!(call("PUT", "/api/v1/discovery", r#"{"ident":false}"#).status, 200);
        let off = get();
        assert_eq!((off["ident"].as_bool(), off["dhcp"].as_bool(), off["fingerprints"].as_array().map(Vec::len)), (Some(false), Some(true), Some(0)), "off forgets what was heard");
        assert!(!wanted(&app), "the listener stops");
        assert_eq!(call("POST", "/api/v1/discovery/run", "").status, 409);
        assert_eq!(call("PUT", "/api/v1/discovery", r#"{"ident":true,"dhcp":false}"#).status, 200);
    }
}
