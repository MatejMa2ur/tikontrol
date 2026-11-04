//! Opt-in speed test, run by the tikontrol server against a Cloudflare-style HTTPS endpoint
//! (`GET <base>/__down?bytes=N`, `POST <base>/__up`). Nothing is contacted until an admin presses
//! Run. It measures tikontrol's own host connection, which is the router's line only when
//! tikontrol runs on the router (container).
//!
//! - Admin only, one at a time, at most one every `MIN_GAP` seconds (failed runs count too).
//! - HTTPS only, certificate verified (outbound.rs roots); bytes and time per phase are capped.
//! - The last `MAX_RESULTS` results live in `speedtest.json` next to `history.json`.

use crate::api::body;
use crate::app::{lock, now, App};
use crate::auth::Session;
use crate::http::{Req, Resp};
use crate::live::Ev;
use crate::outbound::{self, Url};
use crate::store::{write_private, Role};
use rustls::pki_types::ServerName;
use rustls::{ClientConnection, StreamOwned};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const DEFAULT_URL: &str = "https://speed.cloudflare.com";
pub const DOWN_BYTES: u64 = 25_000_000;
pub const UP_BYTES: usize = 10_000_000;
pub const MIN_GAP: u64 = 180;
pub const MAX_RESULTS: usize = 50;
/// each of download and upload stops after this, and the rate is computed from what moved
const PHASE: Duration = Duration::from_secs(10);

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Run {
    pub t: u64,
    /// bit/s
    pub down: f64,
    pub up: f64,
    /// ms, best of 3 TCP connects
    pub ping: f64,
    pub host: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Saved {
    /// None = DEFAULT_URL
    url: Option<String>,
    results: VecDeque<Run>,
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
    lock(&app.store).dir().join("speedtest.json")
}

/// The saved state, loaded on first use.
fn saved<'a>(app: &App, g: &'a mut St) -> &'a mut Saved {
    g.saved.get_or_insert_with(|| std::fs::read(path(app)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default())
}

fn save(app: &App, s: &Saved) -> std::io::Result<()> {
    write_private(&path(app), &serde_json::to_vec(s).unwrap_or_default())
}

/// Validates a base URL: https only, no query, trailing slash dropped.
pub fn base_url(s: &str) -> Result<Url, String> {
    let u = outbound::parse_url(s.trim())?;
    if !u.https {
        return Err("the speed test endpoint must use https://".into());
    }
    if u.path.contains('?') {
        return Err("the speed test endpoint must not have a query".into());
    }
    Ok(Url { path: u.path.trim_end_matches('/').to_string(), ..u })
}

/// Ok, or the seconds to wait.
fn begin(g: &mut St, t: u64) -> Result<(), u64> {
    if g.running {
        return Err(MIN_GAP);
    }
    if g.last > 0 && t < g.last + MIN_GAP {
        return Err(g.last + MIN_GAP - t);
    }
    g.running = true;
    g.last = t;
    Ok(())
}

pub fn route(app: &Arc<App>, req: &Req, s: &Session) -> Option<Resp> {
    if req.path != "/api/v1/speedtest" {
        return None;
    }
    let admin = s.role == Role::Admin;
    Some(match req.method.as_str() {
        "GET" => {
            let mut g = lock(st());
            let (running, last) = (g.running, g.last);
            let sv = saved(app, &mut g);
            let wait = if last > 0 { (last + MIN_GAP).saturating_sub(now()) } else { 0 };
            Resp::json(200, &json!({ "url": sv.url.as_deref().unwrap_or(DEFAULT_URL), "default": DEFAULT_URL, "results": sv.results, "running": running, "wait": wait, "minGap": MIN_GAP }))
        }
        "PUT" | "POST" if !admin => Resp::error(403, "admin role required"),
        "PUT" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Cfg {
                url: Option<String>,
            }
            let b: Cfg = match body(req) {
                Ok(b) => b,
                Err(r) => return Some(r),
            };
            let url = match b.url.filter(|u| !u.trim().is_empty()) {
                None => None,
                Some(u) => match base_url(&u) {
                    Ok(_) => Some(u.trim().to_string()),
                    Err(e) => return Some(Resp::error(400, &e)),
                },
            };
            let mut g = lock(st());
            let sv = saved(app, &mut g);
            sv.url = url;
            crate::api::saved(save(app, sv))
        }
        "POST" => run(app, s),
        _ => Resp::error(405, "use GET, PUT or POST"),
    })
}

fn run(app: &Arc<App>, s: &Session) -> Resp {
    let url = {
        let mut g = lock(st());
        let u = saved(app, &mut g).url.clone().unwrap_or_else(|| DEFAULT_URL.into());
        if let Err(wait) = begin(&mut g, now()) {
            return Resp::error(429, "a speed test ran recently or is running; try again later").with("retry-after", wait.to_string());
        }
        u
    };
    // the network part runs without the lock, so GET keeps answering
    let result = base_url(&url).and_then(|u| measure(&u));
    let mut g = lock(st());
    g.running = false;
    let r = match result {
        Ok(r) => r,
        Err(e) => return Resp::error(502, &format!("speed test failed: {e}")),
    };
    let sv = saved(app, &mut g);
    sv.results.push_back(r.clone());
    while sv.results.len() > MAX_RESULTS {
        sv.results.pop_front();
    }
    let ok = save(app, sv);
    drop(g);
    let mbps = |b: f64| format!("{:.1} Mbit/s", b / 1e6);
    lock(&app.live.history).event(Ev { t: r.t, cat: "System".into(), level: "info".into(), text: format!("{}: speed test via {}: down {}, up {}, {:.0} ms", s.user, r.host, mbps(r.down), mbps(r.up), r.ping), mac: None });
    match ok {
        Ok(()) => Resp::json(200, &json!(r)),
        Err(_) => Resp::error(500, "could not save state"),
    }
}

fn measure(u: &Url) -> Result<Run, String> {
    let host = &u.host;
    let addr = (host.as_str(), u.port).to_socket_addrs().map_err(|_| format!("cannot resolve {host}"))?.next().ok_or_else(|| format!("cannot resolve {host}"))?;
    let mut ping = f64::MAX;
    for _ in 0..3 {
        let t0 = Instant::now();
        TcpStream::connect_timeout(&addr, outbound::TIMEOUT).map_err(|e| format!("cannot connect to {host}: {}", e.kind()))?;
        ping = ping.min(t0.elapsed().as_secs_f64() * 1000.0);
    }
    let down = download(u, addr).map_err(|e| format!("download from {host}: {e}"))?;
    let up = upload(u, addr).map_err(|e| format!("upload to {host}: {e}"))?;
    Ok(Run { t: now(), down, up, ping: (ping * 10.0).round() / 10.0, host: host.clone() })
}

fn open(u: &Url, addr: SocketAddr) -> Result<StreamOwned<ClientConnection, TcpStream>, String> {
    let tcp = TcpStream::connect_timeout(&addr, outbound::TIMEOUT).map_err(|e| e.kind().to_string())?;
    tcp.set_read_timeout(Some(outbound::TIMEOUT)).ok();
    tcp.set_write_timeout(Some(outbound::TIMEOUT)).ok();
    let name = ServerName::try_from(u.host.clone()).map_err(|_| "invalid host".to_string())?;
    let conn = ClientConnection::new(outbound::tls_config(), name).map_err(|e| e.to_string())?;
    Ok(StreamOwned::new(conn, tcp))
}

fn head(method: &str, u: &Url, path: &str, len: usize) -> String {
    let host = if u.port == 443 { u.host.clone() } else { format!("{}:{}", u.host, u.port) };
    format!("{method} {}{path} HTTP/1.1\r\nhost: {host}\r\nuser-agent: tikontrol/{}\r\ncontent-length: {len}\r\nconnection: close\r\n\r\n", u.path, env!("CARGO_PKG_VERSION"))
}

/// Reads up to the end of the response headers; returns the status and any body bytes after them.
fn read_head(s: &mut impl Read) -> Result<(u16, usize), String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = s.read(&mut chunk).map_err(|e| e.kind().to_string())?;
        if n == 0 {
            return Err("connection closed".into());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let status = outbound::status_of(&buf).ok_or("not an HTTP response")?;
            return Ok((status, buf.len() - i - 4));
        }
        if buf.len() > 16 * 1024 {
            return Err("response headers too large".into());
        }
    }
}

fn download(u: &Url, addr: SocketAddr) -> Result<f64, String> {
    let mut s = open(u, addr)?;
    s.write_all(head("GET", u, &format!("/__down?bytes={DOWN_BYTES}"), 0).as_bytes()).map_err(|e| e.kind().to_string())?;
    let (status, mut got) = read_head(&mut s)?;
    if status != 200 {
        return Err(format!("HTTP {status}"));
    }
    let t0 = Instant::now();
    let mut chunk = vec![0u8; 64 * 1024];
    // stop at the cap (plus room for chunked framing) or the time limit; a dropped connection after data still counts
    while (got as u64) < DOWN_BYTES + 64 * 1024 && t0.elapsed() < PHASE {
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(_) if got > 0 => break,
            Err(e) => return Err(e.kind().to_string()),
        }
    }
    rate(got, t0.elapsed())
}

fn upload(u: &Url, addr: SocketAddr) -> Result<f64, String> {
    let mut s = open(u, addr)?;
    s.write_all(head("POST", u, "/__up", UP_BYTES).as_bytes()).map_err(|e| e.kind().to_string())?;
    let chunk = vec![0u8; 64 * 1024];
    let t0 = Instant::now();
    let mut sent = 0;
    while sent < UP_BYTES && t0.elapsed() < PHASE {
        let n = chunk.len().min(UP_BYTES - sent);
        s.write_all(&chunk[..n]).map_err(|e| e.kind().to_string())?;
        sent += n;
    }
    if sent == UP_BYTES {
        s.flush().map_err(|e| e.kind().to_string())?;
        let (status, _) = read_head(&mut s)?;
        if !(200..300).contains(&status) {
            return Err(format!("HTTP {status}"));
        }
    }
    rate(sent, t0.elapsed())
}

fn rate(bytes: usize, d: Duration) -> Result<f64, String> {
    if bytes == 0 {
        return Err("no data".into());
    }
    Ok((bytes as f64 * 8.0 / d.as_secs_f64().max(0.001)).round())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_must_be_https_without_query() {
        let u = base_url(" https://speed.example.com/ ").unwrap();
        assert_eq!((u.host.as_str(), u.path.as_str()), ("speed.example.com", ""));
        assert_eq!(base_url("https://example.com/st").unwrap().path, "/st");
        for bad in ["http://192.168.1.5/", "http://speed.example.com", "https://example.com/?x=1", "https://u:p@example.com", "file:///etc/passwd", "https://example.com/a b"] {
            assert!(base_url(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn one_at_a_time_and_spaced_out() {
        let mut g = St::default();
        assert!(begin(&mut g, 1000).is_ok());
        assert!(begin(&mut g, 2000).is_err(), "still running");
        g.running = false;
        assert_eq!(begin(&mut g, 1000 + MIN_GAP - 10), Err(10));
        assert!(begin(&mut g, 1000 + MIN_GAP).is_ok());
    }
}
