//! A minimal SMTP client for the opt-in e-mail alert channel. One message per call.
//!
//! - Implicit TLS (port 465) or STARTTLS (port 587). The certificate is always verified against
//!   the bundled Mozilla roots, the same as `outbound`. Plain SMTP only to a local relay, and then
//!   never with a password: AUTH is only sent inside TLS.
//! - Addresses, host and user name are validated here, at the trust boundary; the subject and the
//!   body are encoded (RFC 2047 / base64), so alert text from other devices (host names, SSIDs)
//!   cannot inject headers or end the DATA block.
//! - 5 s timeouts per step, a capped reply size, and errors that never echo the password.

use crate::outbound::{is_local, tls_config, TIMEOUT};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};
use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;

const MAX_REPLY: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    /// TLS from the first byte (port 465)
    Tls,
    /// plain greeting, then STARTTLS before anything else (port 587)
    StartTls,
    /// no encryption: only allowed to a local relay, and never with a password
    None,
}

impl Security {
    pub fn parse(s: &str) -> Option<Security> {
        match s {
            "tls" => Some(Security::Tls),
            "starttls" => Some(Security::StartTls),
            "none" => Some(Security::None),
            _ => None,
        }
    }
}

pub struct Mail<'a> {
    pub host: &'a str,
    pub port: u16,
    pub security: Security,
    pub user: Option<&'a str>,
    pub pass: Option<&'a str>,
    pub from: &'a str,
    pub to: &'a [String],
    pub subject: &'a str,
    pub body: &'a str,
}

/// `name@domain`, printable ASCII, no spaces, quotes, angle brackets or CR/LF.
pub fn valid_address(a: &str) -> bool {
    let Some((local, domain)) = a.rsplit_once('@') else { return false };
    a.len() <= 254
        && !local.is_empty()
        && local.len() <= 64
        && local.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_`{|}~.".contains(&b))
        && valid_host(domain)
        && domain.contains('.')
}

pub fn valid_host(h: &str) -> bool {
    h.parse::<IpAddr>().is_ok()
        || (!h.is_empty()
            && h.len() <= 253
            && h.split('.').all(|l| !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-') && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')))
}

pub fn valid_user(u: &str) -> bool {
    !u.is_empty() && u.len() <= 256 && u.bytes().all(|b| b.is_ascii_graphic())
}

pub fn valid_pass(p: &str) -> bool {
    !p.is_empty() && p.len() <= 256 && !p.contains(['\0', '\r', '\n'])
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Header-safe subject: control characters dropped, non-ASCII as an RFC 2047 encoded word.
fn encode_subject(s: &str) -> String {
    let clean: String = s.chars().filter(|c| !c.is_control()).take(200).collect();
    if clean.is_ascii() {
        clean
    } else {
        format!("=?utf-8?B?{}?=", base64(clean.as_bytes()))
    }
}

/// RFC 5322 date in UTC from a unix time.
fn rfc5322_date(t: u64) -> String {
    let days = (t / 86_400) as i64;
    let secs = t % 86_400;
    // civil-from-days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    const WD: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MO: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{}, {:02} {} {} {:02}:{:02}:{:02} +0000", WD[days.rem_euclid(7) as usize], d, MO[(m - 1) as usize], y, secs / 3600, secs % 3600 / 60, secs % 60)
}

/// The whole message, CRLF line endings, body as base64 (no dot-stuffing or 8-bit issues).
pub fn message(m: &Mail, t: u64, id: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!("Date: {}\r\n", rfc5322_date(t)));
    s.push_str(&format!("From: tikontrol <{}>\r\n", m.from));
    s.push_str(&format!("To: {}\r\n", m.to.join(", ")));
    s.push_str(&format!("Subject: {}\r\n", encode_subject(m.subject)));
    s.push_str(&format!("Message-ID: <{id}@tikontrol>\r\n"));
    s.push_str("MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\nAuto-Submitted: auto-generated\r\n\r\n");
    let body = base64(m.body.as_bytes());
    for line in body.as_bytes().chunks(76) {
        s.push_str(std::str::from_utf8(line).unwrap_or_default());
        s.push_str("\r\n");
    }
    s
}

enum Stream {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
    Gone,
}

impl Read for Stream {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(b),
            Stream::Tls(s) => s.read(b),
            Stream::Gone => Ok(0),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(b),
            Stream::Tls(s) => s.write(b),
            Stream::Gone => Err(std::io::ErrorKind::NotConnected.into()),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            Stream::Tls(s) => s.flush(),
            Stream::Gone => Ok(()),
        }
    }
}

struct Session {
    s: Stream,
    buf: Vec<u8>,
}

fn describe(e: std::io::Error) -> String {
    match e.get_ref().and_then(|i| i.downcast_ref::<rustls::Error>()) {
        Some(t) => format!("TLS error: {t}"),
        None => format!("{}", e.kind()),
    }
}

impl Session {
    /// Reads one (possibly multi-line) reply and returns its code and last line.
    fn reply(&mut self) -> Result<(u16, String), String> {
        loop {
            if let Some(pos) = self.buf.windows(2).position(|w| w == b"\r\n") {
                let line: Vec<u8> = self.buf.drain(..pos + 2).collect();
                let line = String::from_utf8_lossy(&line[..pos]).into_owned();
                let code = line.get(..3).and_then(|c| c.parse::<u16>().ok()).ok_or("not an SMTP reply")?;
                if line.as_bytes().get(3) == Some(&b'-') {
                    continue;
                }
                return Ok((code, line.chars().skip(4).take(160).collect()));
            }
            if self.buf.len() > MAX_REPLY {
                return Err("SMTP reply too long".into());
            }
            let mut chunk = [0u8; 1024];
            match self.s.read(&mut chunk) {
                Ok(0) => return Err("connection closed".into()),
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e) => return Err(describe(e)),
            }
        }
    }

    fn cmd(&mut self, line: &str, want: &[u16], what: &str) -> Result<(), String> {
        self.s.write_all(line.as_bytes()).and_then(|_| self.s.write_all(b"\r\n")).and_then(|_| self.s.flush()).map_err(describe)?;
        self.expect(want, what)
    }

    fn expect(&mut self, want: &[u16], what: &str) -> Result<(), String> {
        let (code, text) = self.reply()?;
        if want.contains(&code) {
            Ok(())
        } else {
            Err(format!("{what} refused: {code} {}", text.chars().filter(|c| !c.is_control()).collect::<String>()))
        }
    }
}

/// Validates everything that goes into SMTP commands or headers.
pub fn check(m: &Mail) -> Result<(), String> {
    if !valid_host(m.host) {
        return Err("SMTP server: invalid host name".into());
    }
    if m.port == 0 {
        return Err("SMTP server: invalid port".into());
    }
    if !valid_address(m.from) {
        return Err("sender address looks wrong".into());
    }
    if m.to.is_empty() || m.to.len() > 10 || !m.to.iter().all(|a| valid_address(a)) {
        return Err("give 1-10 valid recipient addresses".into());
    }
    if m.user.is_some_and(|u| !valid_user(u)) || m.pass.is_some_and(|p| !valid_pass(p)) {
        return Err("SMTP user name or password has invalid characters".into());
    }
    if m.security == Security::None && m.pass.is_some() {
        return Err("a password is only sent over TLS; choose TLS or STARTTLS".into());
    }
    Ok(())
}

pub fn send(m: &Mail) -> Result<(), String> {
    send_with(m, tls_config(), crate::app::now())
}

fn send_with(m: &Mail, tls: Arc<ClientConfig>, t: u64) -> Result<(), String> {
    check(m)?;
    let host = m.host;
    let addr = (host, m.port).to_socket_addrs().map_err(|_| format!("cannot resolve {host}"))?.next().ok_or_else(|| format!("cannot resolve {host}"))?;
    if m.security == Security::None && !is_local(addr.ip()) {
        return Err(format!("{host} is not a local address; unencrypted SMTP refused"));
    }
    let tcp = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| format!("cannot connect to {host}: {}", e.kind()))?;
    tcp.set_read_timeout(Some(TIMEOUT)).ok();
    tcp.set_write_timeout(Some(TIMEOUT)).ok();
    let name = || ServerName::try_from(host.to_string()).map_err(|_| "invalid host".to_string());
    let tls_wrap = |tcp: TcpStream| -> Result<Stream, String> {
        let conn = ClientConnection::new(tls.clone(), name()?).map_err(|e| e.to_string())?;
        Ok(Stream::Tls(Box::new(StreamOwned::new(conn, tcp))))
    };
    let s = if m.security == Security::Tls { tls_wrap(tcp)? } else { Stream::Plain(tcp) };
    let mut c = Session { s, buf: Vec::new() };
    let run = |c: &mut Session| -> Result<(), String> {
        c.expect(&[220], "greeting")?;
        c.cmd("EHLO tikontrol", &[250], "EHLO")?;
        if m.security == Security::StartTls {
            c.cmd("STARTTLS", &[220], "STARTTLS")?;
            if !c.buf.is_empty() {
                return Err("server sent data before the TLS handshake".into());
            }
            let Stream::Plain(tcp) = std::mem::replace(&mut c.s, Stream::Gone) else { return Err("STARTTLS on a TLS stream".into()) };
            c.s = tls_wrap(tcp)?;
            c.cmd("EHLO tikontrol", &[250], "EHLO")?;
        }
        if let (Some(u), Some(p)) = (m.user, m.pass) {
            let cred = base64(format!("\0{u}\0{p}").as_bytes());
            c.cmd(&format!("AUTH PLAIN {cred}"), &[235], "login")?;
        }
        c.cmd(&format!("MAIL FROM:<{}>", m.from), &[250], "sender")?;
        for to in m.to {
            c.cmd(&format!("RCPT TO:<{to}>"), &[250, 251], "recipient")?;
        }
        c.cmd("DATA", &[354], "DATA")?;
        let id = {
            let mut r = [0u8; 12];
            ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut r).map_err(|_| "no randomness")?;
            r.iter().map(|b| format!("{b:02x}")).collect::<String>()
        };
        c.s.write_all(message(m, t, &id).as_bytes()).map_err(describe)?;
        c.cmd(".", &[250], "message")?;
        let _ = c.cmd("QUIT", &[221], "QUIT");
        Ok(())
    };
    run(&mut c).map_err(|e| format!("{host}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::pki_types::PrivateKeyDer;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::time::Duration;

    #[test]
    fn base64_and_dates() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"\0u\0p"), "AHUAcA==");
        assert_eq!(rfc5322_date(0), "Thu, 01 Jan 1970 00:00:00 +0000");
        assert_eq!(rfc5322_date(1_709_210_096), "Thu, 29 Feb 2024 12:34:56 +0000");
    }

    #[test]
    fn validates_everything_that_reaches_a_command_or_header() {
        assert!(valid_address("alerts@example.com"));
        assert!(!valid_address("a@b"));
        assert!(!valid_address("a b@example.com"));
        assert!(!valid_address("a@example.com>\r\nRCPT TO:<x@y.z"));
        assert!(!valid_address("<a@example.com>"));
        let to = vec!["me@example.com".to_string()];
        let ok = Mail { host: "smtp.example.com", port: 587, security: Security::StartTls, user: Some("me"), pass: Some("pw"), from: "me@example.com", to: &to, subject: "s", body: "b" };
        assert!(check(&ok).is_ok());
        assert!(check(&Mail { host: "smtp.example.com\r\n", ..ok }).is_err());
        assert!(check(&Mail { pass: Some("a\r\nb"), ..ok }).is_err());
        assert!(check(&Mail { security: Security::None, ..ok }).unwrap_err().contains("TLS"));
        assert!(check(&Mail { to: &[], ..ok }).is_err());
        let bad = vec!["x@example.com\r\nBcc: y@example.com".to_string()];
        assert!(check(&Mail { to: &bad, ..ok }).is_err());
    }

    #[test]
    fn untrusted_subject_and_body_cannot_inject() {
        let to = vec!["me@example.com".to_string()];
        let m = Mail { host: "h.example.com", port: 465, security: Security::Tls, user: None, pass: None, from: "t@example.com", to: &to, subject: "new client \"evil\r\nBcc: x@y.z\"", body: "line\r\n.\r\nMAIL FROM:<x@y.z>\nčaj" };
        let msg = message(&m, 0, "abc");
        let (head, body) = msg.split_once("\r\n\r\n").unwrap();
        assert!(!head.contains("\r\nBcc"), "{head}");
        assert!(head.contains("Subject: new client \"evilBcc: x@y.z\"\r\n"));
        assert!(!body.contains("MAIL FROM") && !body.contains("\r\n.\r\n"));
        // non-ASCII subjects become an encoded word
        let m = Mail { subject: "Wi-Fi Kuchyňa", ..m };
        assert!(message(&m, 0, "x").contains("Subject: =?utf-8?B?"));
    }

    fn server_tls() -> (Arc<rustls::ServerConfig>, Arc<ClientConfig>) {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let key = PrivateKeyDer::Pkcs8(cert.key_pair.serialize_der().into());
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let srv = rustls::ServerConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions().unwrap().with_no_client_auth().with_single_cert(vec![cert.cert.der().clone()], key).unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert.cert.der().clone()).unwrap();
        let cli = ClientConfig::builder_with_provider(provider).with_safe_default_protocol_versions().unwrap().with_root_certificates(roots).with_no_client_auth();
        (Arc::new(srv), Arc::new(cli))
    }

    /// A tiny SMTP server: answers every command and records what it got.
    fn fake(starttls: bool, implicit: bool, srv: Arc<rustls::ServerConfig>) -> (u16, std::thread::JoinHandle<Vec<String>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (tcp, _) = l.accept().unwrap();
            tcp.set_read_timeout(Some(Duration::from_secs(3))).ok();
            let mut log = Vec::new();
            fn talk<S: Read + Write>(s: S, log: &mut Vec<String>, starttls: bool) -> Option<S> {
                let mut r = BufReader::new(s);
                let mut data = false;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).ok()? == 0 {
                        return None;
                    }
                    let line = line.trim_end().to_string();
                    log.push(line.clone());
                    let w = r.get_mut();
                    let answer = if data {
                        if line == "." {
                            data = false;
                            "250 queued"
                        } else {
                            continue;
                        }
                    } else if line.starts_with("EHLO") {
                        if starttls { "250-hi\r\n250 STARTTLS" } else { "250-hi\r\n250 AUTH PLAIN" }
                    } else if line == "STARTTLS" {
                        w.write_all(b"220 go ahead\r\n").ok()?;
                        return Some(r.into_inner());
                    } else if line.starts_with("AUTH PLAIN") {
                        if line == "AUTH PLAIN AHUAcA==" { "235 ok" } else { "535 no" }
                    } else if line == "DATA" {
                        data = true;
                        "354 send"
                    } else if line == "QUIT" {
                        w.write_all(b"221 bye\r\n").ok()?;
                        return None;
                    } else {
                        "250 ok"
                    };
                    w.write_all(format!("{answer}\r\n").as_bytes()).ok()?;
                }
            }
            if implicit {
                let mut s = StreamOwned::new(rustls::ServerConnection::new(srv).unwrap(), tcp);
                if s.write_all(b"220 fake\r\n").is_ok() {
                    talk(s, &mut log, false);
                }
            } else {
                let mut tcp = tcp;
                tcp.write_all(b"220 fake\r\n").unwrap();
                if let Some(tcp) = talk(tcp, &mut log, starttls) {
                    log.push("-- tls --".into());
                    let s = StreamOwned::new(rustls::ServerConnection::new(srv).unwrap(), tcp);
                    talk(s, &mut log, false);
                }
            }
            log
        });
        (port, h)
    }

    #[test]
    fn sends_over_starttls_and_implicit_tls_with_auth_inside_tls() {
        let to = vec!["me@example.com".to_string(), "you@example.com".to_string()];
        for implicit in [false, true] {
            let (srv, cli) = server_tls();
            let (port, h) = fake(!implicit, implicit, srv);
            let m = Mail { host: "localhost", port, security: if implicit { Security::Tls } else { Security::StartTls }, user: Some("u"), pass: Some("p"), from: "t@example.com", to: &to, subject: "tikontrol: warning", body: "CPU high" };
            send_with(&m, cli, 0).unwrap();
            let log = h.join().unwrap();
            let auth = log.iter().position(|l| l.starts_with("AUTH")).unwrap();
            if !implicit {
                assert!(log.iter().position(|l| l == "-- tls --").unwrap() < auth, "AUTH only after STARTTLS: {log:?}");
            }
            assert!(log.contains(&"RCPT TO:<you@example.com>".to_string()));
            assert!(log.contains(&base64(b"CPU high")));
            assert_eq!(log.last().unwrap(), "QUIT");
        }
    }

    #[test]
    fn refuses_unverified_certificates_and_reports_wrong_logins() {
        let to = vec!["me@example.com".to_string()];
        let (srv, _) = server_tls();
        let (port, h) = fake(false, true, srv);
        let m = Mail { host: "localhost", port, security: Security::Tls, user: Some("u"), pass: Some("secret-pw"), from: "t@example.com", to: &to, subject: "s", body: "b" };
        let e = send(&m).unwrap_err();
        h.join().unwrap();
        assert!(e.contains("TLS") || e.contains("certificate"), "{e}");
        let (srv, cli) = server_tls();
        let (port, h) = fake(false, true, srv);
        let e = send_with(&Mail { port, ..m }, cli, 0).unwrap_err();
        h.join().unwrap();
        assert!(e.contains("login refused: 535") && !e.contains("secret-pw"), "{e}");
    }
}
