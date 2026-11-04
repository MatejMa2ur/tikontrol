//! Minimal HTTP/1.1 server on std threads: one request per connection, hard size limits,
//! timeouts and a cap on concurrent connections. Small enough to audit, no async runtime.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const MAX_LINE: usize = 8 * 1024;
pub const MAX_HEADERS: usize = 64;
pub const MAX_BODY: usize = 256 * 1024;
pub const MAX_CONNS: usize = 64;

#[derive(Debug, Default)]
pub struct Req {
    pub method: String,
    pub path: String,
    pub query: String,
    /// lowercase names
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub peer: Option<IpAddr>,
    pub tls: bool,
}

impl Req {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.header("cookie")?.split(';').filter_map(|c| c.trim().split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| v)
    }
    pub fn param(&self, name: &str) -> Option<String> {
        self.query.split('&').filter_map(|p| p.split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| pct_decode(v))
    }
}

pub fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("x"), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub type Stream = Box<dyn FnOnce(&mut dyn Write) + Send>;

pub enum Body {
    Bytes(Vec<u8>),
    Static(&'static [u8]),
    /// long-lived response (Server-Sent Events); the closure owns the socket until it returns
    Stream(Stream),
}

pub struct Resp {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Body,
}

impl Resp {
    pub fn new(status: u16, ctype: &str, body: Vec<u8>) -> Resp {
        Resp { status, headers: vec![("content-type", ctype.into())], body: Body::Bytes(body) }
    }
    pub fn json(status: u16, v: &serde_json::Value) -> Resp {
        let mut r = Resp::new(status, "application/json", serde_json::to_vec(v).unwrap_or_default());
        r.headers.push(("cache-control", "no-store".into()));
        r
    }
    pub fn error(status: u16, msg: &str) -> Resp {
        Resp::json(status, &serde_json::json!({ "error": msg }))
    }
    pub fn with(mut self, k: &'static str, v: String) -> Resp {
        self.headers.push((k, v));
        self
    }
}

fn reason(s: u16) -> &'static str {
    match s {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

fn read_line(r: &mut impl BufRead) -> Result<String, u16> {
    let mut buf = Vec::new();
    let n = r.take(MAX_LINE as u64 + 1).read_until(b'\n', &mut buf).map_err(|_| 400u16)?;
    if n == 0 {
        return Err(400);
    }
    if buf.len() > MAX_LINE || !buf.ends_with(b"\n") {
        return Err(431);
    }
    while buf.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        buf.pop();
    }
    String::from_utf8(buf).map_err(|_| 400)
}

/// Parses one request. Errors are HTTP status codes to answer with.
pub fn parse(r: &mut impl BufRead) -> Result<Req, u16> {
    let line = read_line(r)?;
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(ver), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else { return Err(400) };
    if !ver.starts_with("HTTP/1.") || !target.starts_with('/') || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(400);
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut req = Req { method: method.into(), path: pct_decode(path), query: query.into(), ..Default::default() };
    if req.path.contains("..") || req.path.contains('\0') {
        return Err(400);
    }
    loop {
        let h = read_line(r)?;
        if h.is_empty() {
            break;
        }
        if req.headers.len() >= MAX_HEADERS {
            return Err(431);
        }
        let (k, v) = h.split_once(':').ok_or(400u16)?;
        if k.is_empty() || k.contains(' ') {
            return Err(400);
        }
        req.headers.push((k.to_ascii_lowercase(), v.trim().to_string()));
    }
    if req.header("transfer-encoding").is_some() {
        return Err(501); // no chunked bodies: removes a whole class of smuggling bugs
    }
    let lens: Vec<&str> = req.headers.iter().filter(|(k, _)| k == "content-length").map(|(_, v)| v.as_str()).collect();
    let len = match lens.as_slice() {
        [] => 0,
        [one] => one.parse::<usize>().map_err(|_| 400u16)?,
        _ => return Err(400),
    };
    if len > MAX_BODY {
        return Err(413);
    }
    if len == 0 && matches!(req.method.as_str(), "POST" | "PUT" | "PATCH") && lens.is_empty() {
        return Err(411);
    }
    req.body = vec![0; len];
    r.read_exact(&mut req.body).map_err(|_| 400u16)?;
    Ok(req)
}

/// Headers on every response: no framing, no sniffing, strict CSP (the UI is self-contained).
pub const SECURITY_HEADERS: &[(&str, &str)] = &[
    ("x-content-type-options", "nosniff"),
    ("x-frame-options", "DENY"),
    ("referrer-policy", "no-referrer"),
    ("cross-origin-opener-policy", "same-origin"),
    ("cross-origin-resource-policy", "same-origin"),
    ("permissions-policy", "camera=(), microphone=(), geolocation=()"),
    (
        "content-security-policy",
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; font-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
    ),
];

pub fn write(w: &mut dyn Write, resp: Resp, tls: bool) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\nconnection: close\r\n", resp.status, reason(resp.status));
    for (k, v) in SECURITY_HEADERS {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    if tls {
        head.push_str("strict-transport-security: max-age=31536000\r\n");
    }
    for (k, v) in &resp.headers {
        // header values are ours, but never let a stray newline split the response
        head.push_str(&format!("{k}: {}\r\n", v.replace(['\r', '\n'], "")));
    }
    match resp.body {
        Body::Bytes(b) => {
            head.push_str(&format!("content-length: {}\r\n\r\n", b.len()));
            w.write_all(head.as_bytes())?;
            w.write_all(&b)?;
        }
        Body::Static(b) => {
            head.push_str(&format!("content-length: {}\r\n\r\n", b.len()));
            w.write_all(head.as_bytes())?;
            w.write_all(b)?;
        }
        Body::Stream(f) => {
            head.push_str("\r\n");
            w.write_all(head.as_bytes())?;
            w.flush()?;
            f(w);
        }
    }
    w.flush()
}

pub type Handler = Arc<dyn Fn(Req) -> Resp + Send + Sync>;

fn handle(tcp: TcpStream, tls: Option<Arc<rustls::ServerConfig>>, h: Handler) {
    let peer = tcp.peer_addr().ok().map(|a| a.ip());
    tcp.set_read_timeout(Some(Duration::from_secs(10))).ok();
    tcp.set_write_timeout(Some(Duration::from_secs(30))).ok();
    let mut io: Box<dyn ReadWrite> = match tls {
        Some(cfg) => match rustls::ServerConnection::new(cfg) {
            Ok(c) => Box::new(rustls::StreamOwned::new(c, tcp)),
            Err(_) => return,
        },
        None => Box::new(tcp),
    };
    let is_tls = io.is_tls();
    let parsed = {
        let mut r = BufReader::new(&mut io);
        parse(&mut r).map(|mut req| {
            req.peer = peer;
            req.tls = is_tls;
            req
        })
    };
    let resp = match parsed {
        Ok(req) => h(req),
        Err(code) => Resp::error(code, reason(code)),
    };
    let _ = write(&mut io, resp, is_tls);
}

trait ReadWrite: Read + Write + Send {
    fn is_tls(&self) -> bool;
}
impl ReadWrite for TcpStream {
    fn is_tls(&self) -> bool {
        false
    }
}
impl ReadWrite for rustls::StreamOwned<rustls::ServerConnection, TcpStream> {
    fn is_tls(&self) -> bool {
        true
    }
}

pub fn serve(l: TcpListener, tls: Option<Arc<rustls::ServerConfig>>, h: Handler) {
    let active = Arc::new(AtomicUsize::new(0));
    for tcp in l.incoming().flatten() {
        if active.load(Ordering::SeqCst) >= MAX_CONNS {
            continue; // dropping the socket is the cheapest refusal under a flood
        }
        active.fetch_add(1, Ordering::SeqCst);
        let (tls, h, done) = (tls.clone(), h.clone(), active.clone());
        let spawned = std::thread::Builder::new().stack_size(256 * 1024).spawn(move || {
            handle(tcp, tls, h);
            done.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(raw: &str) -> Result<Req, u16> {
        parse(&mut raw.as_bytes())
    }

    #[test]
    fn parses_a_request() {
        let r = p("POST /api/v1/login?x=a%20b HTTP/1.1\r\nHost: t\r\nCookie: a=1; tik_s=abc\r\nContent-Length: 2\r\n\r\n{}").unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.path, "/api/v1/login");
        assert_eq!(r.param("x").as_deref(), Some("a b"));
        assert_eq!(r.cookie("tik_s"), Some("abc"));
        assert_eq!(r.body, b"{}");
    }

    #[test]
    fn rejects_malformed_and_oversized() {
        assert_eq!(p("GET / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n").unwrap_err(), 501);
        assert_eq!(p("GET / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\nab").unwrap_err(), 400);
        assert_eq!(p(&format!("GET / HTTP/1.1\r\nContent-Length: {}\r\n\r\n", MAX_BODY + 1)).unwrap_err(), 413);
        assert_eq!(p(&format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(MAX_LINE))).unwrap_err(), 431);
        assert_eq!(p("GET /../etc/passwd HTTP/1.1\r\n\r\n").unwrap_err(), 400);
        assert_eq!(p("GET /%2e%2e/x HTTP/1.1\r\n\r\n").unwrap_err(), 400);
        assert_eq!(p("GET http://x/ HTTP/1.1\r\n\r\n").unwrap_err(), 400);
        assert_eq!(p("POST /x HTTP/1.1\r\n\r\n").unwrap_err(), 411);
        assert_eq!(p("GET / HTTP/1.1\r\nbad header\r\n\r\n").unwrap_err(), 400);
        let many: String = (0..=MAX_HEADERS).map(|i| format!("h{i}: v\r\n")).collect();
        assert_eq!(p(&format!("GET / HTTP/1.1\r\n{many}\r\n")).unwrap_err(), 431);
    }

    #[test]
    fn response_has_security_headers_and_no_header_injection() {
        let mut out = Vec::new();
        write(&mut out, Resp::json(200, &serde_json::json!({})).with("x-test", "a\r\nset-cookie: x".into()), true).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("content-security-policy: default-src 'self'"));
        assert!(s.contains("x-frame-options: DENY"));
        assert!(s.contains("strict-transport-security"));
        assert!(s.contains("cache-control: no-store"));
        assert!(!s.contains("\r\nset-cookie"));
    }

    #[test]
    fn pct_decode_handles_bad_escapes() {
        assert_eq!(pct_decode("a%2Fb"), "a/b");
        assert_eq!(pct_decode("%zz%"), "%zz%");
        assert_eq!(pct_decode("%4"), "%4");
    }
}
