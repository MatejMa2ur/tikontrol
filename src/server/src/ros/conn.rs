//! TLS connection to the RouterOS API (8729 only; plain 8728 is never used).
//!
//! RouterOS ships self-signed certificates, so a public CA chain proves nothing. Instead the
//! leaf certificate's SHA-256 fingerprint is pinned: shown to the user once at setup (`probe`),
//! confirmed in the browser, stored, and required on every connection after that.

use super::cmd::Cmd;
use super::proto::{decode_sentence, encode_sentence, parse_reply, Kind, MAX_WORD};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub type Row = BTreeMap<String, String>;

/// Anything that can run a RouterOS command. The real connection and test fakes implement it.
pub trait Router {
    fn run(&mut self, cmd: &Cmd) -> Result<Vec<Row>, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub port: u16,
    /// lowercase hex SHA-256 of the router's leaf certificate
    pub pin: String,
}

pub fn fingerprint(der: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, der).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug)]
struct Pinned {
    pin: Option<String>,
    seen: Arc<Mutex<Option<String>>>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let fp = fingerprint(end_entity);
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = Some(fp.clone());
        match &self.pin {
            Some(p) if *p == fp => {
                Ok(ServerCertVerified::assertion())
            }
            _ => Err(rustls::Error::General("router certificate does not match the pinned fingerprint".into())),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

const TIMEOUT: Duration = Duration::from_secs(15);

type Tls = StreamOwned<ClientConnection, TcpStream>;
type Seen = Arc<Mutex<Option<String>>>;

fn tls(host: &str, port: u16, pin: Option<String>) -> Result<(Tls, Seen), String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let seen = Arc::new(Mutex::new(None));
    let verifier = Pinned { pin, seen: seen.clone(), provider: provider.clone() };
    let cfg = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    let name = ServerName::try_from(host.to_string()).map_err(|_| "invalid router address".to_string())?;
    let addr = (host, port).to_socket_addrs().map_err(|e| format!("cannot resolve router: {e}"))?.next().ok_or("cannot resolve router")?;
    let tcp = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| format!("cannot reach router API-SSL: {e}"))?;
    tcp.set_read_timeout(Some(TIMEOUT)).ok();
    tcp.set_write_timeout(Some(TIMEOUT)).ok();
    let conn = ClientConnection::new(Arc::new(cfg), name).map_err(|e| e.to_string())?;
    Ok((StreamOwned::new(conn, tcp), seen))
}

/// Handshake only, sends no credentials: returns the certificate fingerprint for the user to confirm.
pub fn probe(host: &str, port: u16) -> Result<String, String> {
    let (mut s, seen) = tls(host, port, None)?;
    let _ = s.flush(); // drives the handshake; it fails by design because nothing is pinned yet
    let _ = s.conn.complete_io(&mut s.sock);
    let fp = seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
    fp.ok_or_else(|| "router sent no certificate: give api-ssl a certificate (see setup commands)".into())
}

pub struct Conn {
    s: Tls,
    buf: Vec<u8>,
}

impl Conn {
    pub fn connect(t: &Target, user: &str, password: &str) -> Result<Conn, String> {
        let (s, _) = tls(&t.host, t.port, Some(t.pin.clone()))?;
        let mut c = Conn { s, buf: Vec::new() };
        // credentials are passed as validated words and never logged or echoed in errors
        c.run(&Cmd::new("/login").arg("name", user).arg("password", password))
            .map_err(|e| if e.contains("invalid user") || e.contains("cannot log in") { "router rejected the login".into() } else { e })?;
        Ok(c)
    }

    fn send(&mut self, words: &[String]) -> Result<(), String> {
        self.s.write_all(&encode_sentence(words)).map_err(|e| format!("router write failed: {e}"))?;
        self.s.flush().map_err(|e| format!("router write failed: {e}"))
    }

    fn recv(&mut self) -> Result<Vec<String>, String> {
        loop {
            if let Some((words, used)) = decode_sentence(&self.buf).map_err(|e| format!("bad reply from router: {e}"))? {
                self.buf.drain(..used);
                return Ok(words);
            }
            if self.buf.len() > MAX_WORD * 2 {
                return Err("router reply too large".into());
            }
            let mut chunk = [0u8; 8192];
            let n = self.s.read(&mut chunk).map_err(|e| format!("router read failed: {e}"))?;
            if n == 0 {
                return Err("router closed the connection".into());
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }

    /// Runs a command that streams until cancelled (`follow`, `monitor` without `once`), calling
    /// `each` per row until it returns false, then sends `/cancel`.
    pub fn stream(&mut self, cmd: &Cmd, mut each: impl FnMut(Row) -> bool) -> Result<(), String> {
        let tagged = cmd.clone().tag(1);
        self.send(tagged.words()?)?;
        loop {
            let r = parse_reply(&self.recv()?)?;
            match r.kind {
                Kind::Re => {
                    if !each(r.attrs) {
                        self.send(Cmd::new("/cancel").arg("tag", "1").words()?)?;
                    }
                }
                Kind::Trap if r.attrs.get("category").map(String::as_str) == Some("2") => {} // interrupted by our /cancel
                Kind::Trap => return Err(r.attrs.get("message").cloned().unwrap_or_else(|| "router error".into())),
                Kind::Fatal => return Err(format!("router ended the session: {}", r.attrs.get("message").map_or("", |s| s))),
                Kind::Done | Kind::Empty if r.tag.as_deref() == Some("1") => return Ok(()),
                _ => {}
            }
        }
    }
}

impl Router for Conn {
    fn run(&mut self, cmd: &Cmd) -> Result<Vec<Row>, String> {
        self.send(cmd.words()?)?;
        let mut rows = Vec::new();
        let mut err = None;
        loop {
            let r = parse_reply(&self.recv()?)?;
            match r.kind {
                Kind::Re => rows.push(r.attrs),
                Kind::Trap => err = Some(r.attrs.get("message").cloned().unwrap_or_else(|| "router error".into())),
                Kind::Fatal => return Err(format!("router ended the session: {}", r.attrs.get("message").map_or("", |s| s))),
                Kind::Done | Kind::Empty => {
                    // `/ping` and `/login` put values on !done; keep them as a row
                    if r.kind == Kind::Done && !r.attrs.is_empty() && rows.is_empty() {
                        rows.push(r.attrs);
                    }
                    return err.map_or(Ok(rows), Err);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_lowercase_hex_sha256() {
        assert_eq!(fingerprint(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    /// A one-shot TLS "router" on localhost with a fresh self-signed certificate. It answers
    /// `/login` with `!done` and records the words it received.
    fn fake_router(conns: usize) -> (u16, String, std::thread::JoinHandle<Vec<String>>) {
        use rustls::pki_types::PrivateKeyDer;
        let cert = rcgen::generate_simple_self_signed(vec!["router.test".into()]).unwrap();
        let der = cert.cert.der().clone();
        let fp = fingerprint(&der);
        let key = PrivateKeyDer::Pkcs8(cert.key_pair.serialize_der().into());
        let cfg = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![der], key)
            .unwrap();
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let mut got = Vec::new();
            for tcp in l.incoming().take(conns) {
                let tcp = tcp.unwrap();
                tcp.set_read_timeout(Some(Duration::from_secs(5))).ok();
                let mut s = StreamOwned::new(rustls::ServerConnection::new(Arc::new(cfg.clone())).unwrap(), tcp);
                let mut buf = Vec::new();
                let mut chunk = [0u8; 512];
                loop {
                    match s.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                    if let Ok(Some((w, _))) = decode_sentence(&buf) {
                        got = w;
                        let _ = s.write_all(&encode_sentence(&["!done".into()]));
                        let _ = s.flush();
                        break;
                    }
                }
            }
            got
        });
        (port, fp, h)
    }

    #[test]
    fn pin_is_enforced_and_probe_sends_no_credentials() {
        let (port, fp, h) = fake_router(2);
        assert_eq!(probe("127.0.0.1", port).unwrap(), fp);
        let bad = Target { host: "127.0.0.1".into(), port, pin: "0".repeat(64) };
        assert!(Conn::connect(&bad, "reader", "pw").is_err());
        // the two incoming connections were the probe and the bad pin; neither got a /login
        assert!(h.join().unwrap().is_empty());
        let (port2, fp2, h2) = fake_router(1);
        let ok = Target { host: "127.0.0.1".into(), port: port2, pin: fp2 };
        assert!(Conn::connect(&ok, "reader", "pw").is_ok());
        assert_eq!(h2.join().unwrap(), ["/login", "=name=reader", "=password=pw"]);
    }

    #[test]
    fn unreachable_router_gives_a_plain_error() {
        // port 1 on localhost is closed; the error must not contain credentials
        let t = Target { host: "127.0.0.1".into(), port: 1, pin: "00".into() };
        let e = Conn::connect(&t, "reader", "s3cret-pass").err().unwrap();
        assert!(!e.contains("s3cret-pass"));
    }
}
