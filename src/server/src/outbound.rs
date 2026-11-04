//! The only outbound HTTP client in tikontrol: one POST per call, used by opt-in alert channels.
//!
//! - HTTPS verifies the server certificate against the bundled Mozilla roots (`webpki-roots`).
//! - Plain `http://` is refused unless the host is a loopback or private address (a local ntfy),
//!   checked both on the URL and on the resolved address.
//! - 5 s timeouts, no redirects, the response is capped and only its status line is read.
//! - Error messages never contain the URL path or query (tokens live there for some services).

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

pub const MAX_URL: usize = 512;
pub const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_RESPONSE: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    pub https: bool,
    /// host as written (IPv6 without brackets)
    pub host: String,
    pub port: u16,
    /// path and query, always starting with '/'
    pub path: String,
}

impl Url {
    fn authority(&self) -> String {
        let h = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        let default = if self.https { 443 } else { 80 };
        if self.port == default {
            h
        } else {
            format!("{h}:{}", self.port)
        }
    }
}

/// Loopback, RFC 1918, link-local and IPv6 unique-local addresses.
pub fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_loopback() || v.is_private() || v.is_link_local(),
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return is_local(IpAddr::V4(v4));
            }
            let s = v.segments()[0];
            v.is_loopback() || (s & 0xfe00) == 0xfc00 || (s & 0xffc0) == 0xfe80
        }
    }
}

fn valid_hostname(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.split('.').all(|l| !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-') && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
}

/// Strict URL check at the trust boundary: http(s) only, no credentials, no fragment, a valid
/// host, bounded length, printable ASCII. Plain http only to a local address.
pub fn parse_url(s: &str) -> Result<Url, String> {
    if s.len() > MAX_URL {
        return Err(format!("URL is longer than {MAX_URL} characters"));
    }
    if !s.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("URL may only contain printable ASCII without spaces".into());
    }
    let lower = s.to_ascii_lowercase();
    let (https, rest) = if lower.starts_with("https://") {
        (true, &s[8..])
    } else if lower.starts_with("http://") {
        (false, &s[7..])
    } else {
        return Err("URL must start with https:// (or http:// for a local address)".into());
    };
    if rest.contains('#') {
        return Err("URL must not contain a fragment".into());
    }
    let cut = rest.find(['/', '?']).unwrap_or(rest.len());
    let (auth, path) = rest.split_at(cut);
    if auth.contains('@') {
        return Err("URL must not contain a user name or password".into());
    }
    let (host, port) = if let Some(r) = auth.strip_prefix('[') {
        let (h, after) = r.split_once(']').ok_or("bad IPv6 address in URL")?;
        h.parse::<std::net::Ipv6Addr>().map_err(|_| "bad IPv6 address in URL")?;
        (h.to_string(), after.strip_prefix(':'))
    } else {
        match auth.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), Some(p)),
            None => (auth.to_string(), None),
        }
    };
    let port = match port {
        None => if https { 443 } else { 80 },
        Some(p) => p.parse::<u16>().ok().filter(|p| *p > 0).ok_or("bad port in URL")?,
    };
    let host = host.to_ascii_lowercase();
    let ip = host.parse::<IpAddr>().ok();
    if ip.is_none() && !valid_hostname(&host) {
        return Err("URL has an invalid host".into());
    }
    if !https && host != "localhost" && !ip.is_some_and(is_local) {
        return Err("plain http:// is only allowed to a local address; use https://".into());
    }
    let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
    Ok(Url { https, host, port, path })
}

fn tls_config() -> Arc<ClientConfig> {
    static CFG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    CFG.get_or_init(|| {
        let roots = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
        let cfg = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("ring supports the default protocol versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
        Arc::new(cfg)
    })
    .clone()
}

/// Sends one POST and returns the HTTP status (only 2xx is Ok). `headers` values must be ours or
/// validated: CR/LF are refused so nothing can inject a header.
pub fn post(url: &Url, content_type: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<u16, String> {
    post_with(url, content_type, headers, body, tls_config())
}

fn post_with(url: &Url, content_type: &str, headers: &[(&str, &str)], body: &[u8], tls: Arc<ClientConfig>) -> Result<u16, String> {
    let host = &url.host;
    let addr = (host.as_str(), url.port).to_socket_addrs().map_err(|_| format!("cannot resolve {host}"))?.next().ok_or_else(|| format!("cannot resolve {host}"))?;
    if !url.https && !is_local(addr.ip()) {
        return Err(format!("{host} is not a local address; plain http:// refused"));
    }
    let mut req = format!("POST {} HTTP/1.1\r\nhost: {}\r\nuser-agent: tikontrol/{}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n", url.path, url.authority(), env!("CARGO_PKG_VERSION"), body.len());
    for (k, v) in headers {
        if k.bytes().chain(v.bytes()).any(|b| b == b'\r' || b == b'\n') {
            return Err("invalid header value".into());
        }
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    let mut msg = req.into_bytes();
    msg.extend_from_slice(body);

    let deadline = Instant::now() + TIMEOUT;
    let mut tcp = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| format!("cannot connect to {host}: {}", e.kind()))?;
    tcp.set_read_timeout(Some(TIMEOUT)).ok();
    tcp.set_write_timeout(Some(TIMEOUT)).ok();
    let head = if url.https {
        let name = ServerName::try_from(host.clone()).map_err(|_| "invalid host".to_string())?;
        let conn = ClientConnection::new(tls, name).map_err(|e| e.to_string())?;
        exchange(&mut StreamOwned::new(conn, tcp), &msg, deadline)
    } else {
        exchange(&mut tcp, &msg, deadline)
    }
    .map_err(|e| format!("{host}: {e}"))?;
    let status = status_of(&head).ok_or_else(|| format!("{host}: not an HTTP response"))?;
    if (200..300).contains(&status) {
        Ok(status)
    } else {
        Err(format!("{host} answered HTTP {status}"))
    }
}

fn exchange(s: &mut (impl Read + Write), msg: &[u8], deadline: Instant) -> Result<Vec<u8>, String> {
    s.write_all(msg).and_then(|_| s.flush()).map_err(describe)?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    // the status line is all we need; stop at the first line or the cap
    while !buf.contains(&b'\n') && buf.len() < MAX_RESPONSE {
        if Instant::now() > deadline {
            return Err("timed out".into());
        }
        match s.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n.min(MAX_RESPONSE - buf.len())]),
            Err(e) => return Err(describe(e)),
        }
    }
    Ok(buf)
}

fn describe(e: std::io::Error) -> String {
    // rustls puts certificate errors in the io::Error; keep those, drop anything else verbose
    match e.get_ref().and_then(|i| i.downcast_ref::<rustls::Error>()) {
        Some(t) => format!("TLS error: {t}"),
        None => format!("{}", e.kind()),
    }
}

fn status_of(head: &[u8]) -> Option<u16> {
    let line = head.split(|b| *b == b'\n').next()?;
    let line = std::str::from_utf8(line).ok()?;
    let mut it = line.split(' ');
    if !it.next()?.starts_with("HTTP/1.") {
        return None;
    }
    it.next()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn url_validation() {
        let u = parse_url("https://ntfy.example.com/alerts?x=1").unwrap();
        assert_eq!((u.https, u.host.as_str(), u.port, u.path.as_str()), (true, "ntfy.example.com", 443, "/alerts?x=1"));
        assert_eq!(parse_url("HTTPS://Example.com").unwrap().path, "/");
        assert_eq!(parse_url("http://192.168.88.10:8080/topic").unwrap().port, 8080);
        assert!(parse_url("http://127.0.0.1/").is_ok());
        assert!(parse_url("http://localhost:2586").is_ok());
        assert!(parse_url("http://[fd00::5]/x").is_ok());
        assert!(parse_url("https://[2001:db8::1]:8443/x").is_ok());
        for bad in [
            "file:///etc/passwd",
            "ftp://example.com/",
            "http://example.com/hook",
            "http://8.8.8.8/",
            "http://[2001:db8::1]/",
            "https://user:pw@example.com/",
            "https://example.com/a b",
            "https://example.com/#frag",
            "https://exa_mple.com/",
            "https://-bad.com/",
            "https://example.com:0/",
            "https://example.com:99999/",
            "https:///path",
            "https://[nope]/",
            "javascript:alert(1)",
            "https://example.com/\r\nx: y",
        ] {
            assert!(parse_url(bad).is_err(), "{bad}");
        }
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL));
        assert!(parse_url(&long).unwrap_err().contains("longer"));
    }

    #[test]
    fn local_addresses() {
        for ip in ["127.0.0.1", "10.1.2.3", "172.16.0.1", "192.168.1.1", "169.254.1.1", "::1", "fd12::1", "fe80::1", "::ffff:192.168.1.1"] {
            assert!(is_local(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "172.32.0.1", "2001:db8::1", "::ffff:8.8.8.8"] {
            assert!(!is_local(ip.parse().unwrap()), "{ip}");
        }
    }

    /// A one-shot local HTTP server that records the request and answers with `status`.
    fn server(status: &'static str) -> (u16, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let mut buf = vec![0u8; 4096];
            let mut got = Vec::new();
            while let Ok(n) = s.read(&mut buf) {
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
                let txt = String::from_utf8_lossy(&got);
                if let Some((head, body)) = txt.split_once("\r\n\r\n") {
                    let len = head.lines().find_map(|l| l.strip_prefix("content-length: ")).and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
                    if body.len() >= len {
                        break;
                    }
                }
            }
            let _ = s.write_all(format!("HTTP/1.1 {status}\r\ncontent-length: 0\r\n\r\n").as_bytes());
            String::from_utf8_lossy(&got).into_owned()
        });
        (port, h)
    }

    #[test]
    fn posts_to_a_local_http_server_and_checks_the_status() {
        let (port, h) = server("204 No Content");
        let u = parse_url(&format!("http://127.0.0.1:{port}/hook?k=v")).unwrap();
        assert_eq!(post(&u, "application/json", &[("authorization", "Bearer t")], b"{\"a\":1}").unwrap(), 204);
        let got = h.join().unwrap();
        assert!(got.starts_with("POST /hook?k=v HTTP/1.1\r\n"));
        assert!(got.contains(&format!("host: 127.0.0.1:{port}\r\n")));
        assert!(got.contains("authorization: Bearer t\r\n"));
        assert!(got.ends_with("{\"a\":1}"));

        let (port, h) = server("403 Forbidden");
        let u = parse_url(&format!("http://127.0.0.1:{port}/secret-path?token=abc")).unwrap();
        let e = post(&u, "text/plain", &[], b"x").unwrap_err();
        h.join().unwrap();
        assert!(e.contains("403"));
        assert!(!e.contains("secret-path") && !e.contains("abc"), "errors never echo the path or query");
    }

    #[test]
    fn refuses_header_injection_and_unverified_tls() {
        let u = parse_url("http://127.0.0.1:1/").unwrap();
        assert!(post(&u, "text/plain", &[("title", "a\r\nx: y")], b"").unwrap_err().contains("header"));
        // a TLS server with a self-signed certificate must fail verification
        use rustls::pki_types::PrivateKeyDer;
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let key = PrivateKeyDer::Pkcs8(cert.key_pair.serialize_der().into());
        let cfg = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert.cert.der().clone()], key)
            .unwrap();
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (tcp, _) = l.accept().unwrap();
            tcp.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let mut s = StreamOwned::new(rustls::ServerConnection::new(Arc::new(cfg)).unwrap(), tcp);
            let mut b = [0u8; 64];
            let _ = s.read(&mut b);
        });
        let u = parse_url(&format!("https://127.0.0.1:{port}/")).unwrap();
        let e = post(&u, "text/plain", &[], b"x").unwrap_err();
        h.join().unwrap();
        assert!(e.contains("TLS") || e.contains("certificate") || e.contains("Invalid"), "{e}");
    }

    #[test]
    fn status_line_parsing() {
        assert_eq!(status_of(b"HTTP/1.1 200 OK\r\n"), Some(200));
        assert_eq!(status_of(b"HTTP/1.0 502"), Some(502));
        assert_eq!(status_of(b"SSH-2.0-OpenSSH\r\n"), None);
        assert_eq!(status_of(b""), None);
    }
}
