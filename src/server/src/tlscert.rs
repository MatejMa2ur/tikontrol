//! HTTPS for the web UI. Uses the operator's certificate when given (TIK_TLS_CERT / TIK_TLS_KEY).
//! Otherwise works like mkcert: a local CA is created once in the data directory (ca-cert.pem,
//! ca-key.pem), and it signs a short-lived server certificate (tls-cert.pem, tls-key.pem) for
//! tikontrol's names and the host's private IPv4 addresses. Devices that trust the CA trust
//! tikontrol. The CA carries name constraints, so it can only vouch for local names and private IPs.

use crate::store::write_private;
use rcgen::{BasicConstraints, CertificateParams, CidrSubnet, DistinguishedName, DnType, ExtendedKeyUsagePurpose, GeneralSubtree, IsCa, KeyPair, KeyUsagePurpose, NameConstraints};
use rustls::client::danger::ServerCertVerifier;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

const DAY: u64 = 86_400;
const LEAF_DAYS: u64 = 397; // Apple refuses longer server certificates, even manually trusted ones
const RENEW_DAYS: u64 = 30;
/// DNS subtrees the CA may vouch for (a constraint also covers its subdomains, e.g. "lan" covers tikontrol.lan).
const DNS_ZONES: &[&str] = &["tikontrol", "localhost", "lan", "home.arpa", "internal", "local"];
const PRIVATE_V4: &[([u8; 4], u8)] = &[([10, 0, 0, 0], 8), ([172, 16, 0, 0], 12), ([192, 168, 0, 0], 16), ([127, 0, 0, 0], 8)];

pub struct Tls {
    pub config: Arc<rustls::ServerConfig>,
    /// SHA-256 of the served certificate
    pub fingerprint: String,
    /// SHA-256 of the local CA, when tikontrol made its own certificate
    pub ca_fingerprint: Option<String>,
    swap: Option<Arc<Swap>>,
}

#[derive(Debug)]
struct Swap(RwLock<Arc<CertifiedKey>>);

impl ResolvesServerCert for Swap {
    fn resolve(&self, _: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(self.0.read().unwrap_or_else(|e| e.into_inner()).clone())
    }
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// `extra` is TIK_TLS_NAMES (comma-separated hostnames or private IPv4 addresses).
pub fn setup(data: &Path, cert: Option<&str>, key: Option<&str>, extra: Option<&str>) -> Result<Tls, String> {
    let (cert_path, key_path, ca_fingerprint) = match (cert, key) {
        (Some(c), Some(k)) => (PathBuf::from(c), PathBuf::from(k), None),
        (None, None) => {
            let names = names(&parse_extra(extra.unwrap_or(""))?);
            if ensure(data, &names)? {
                println!("tikontrol: new server certificate for {}", names.join(", "));
            }
            let ca = CertificateDer::from_pem_file(data.join("ca-cert.pem")).map_err(|e| format!("ca-cert.pem: {e}"))?;
            (data.join("tls-cert.pem"), data.join("tls-key.pem"), Some(colon_hex(&ca)))
        }
        _ => return Err("set both TIK_TLS_CERT and TIK_TLS_KEY, or neither".into()),
    };
    let ck = load(&cert_path, &key_path)?;
    let fingerprint = crate::ros::conn::fingerprint(ck.cert.first().ok_or("no certificate in cert file")?);
    let swap = Arc::new(Swap(RwLock::new(Arc::new(ck))));
    let config = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_no_client_auth()
        .with_cert_resolver(swap.clone());
    let swap = ca_fingerprint.is_some().then_some(swap);
    Ok(Tls { config: Arc::new(config), fingerprint, ca_fingerprint, swap })
}

impl Tls {
    /// Checks the generated certificate once a day and swaps in a renewed one (new IP, or < 30 days left).
    pub fn spawn_renewal(&self, data: PathBuf, extra: Option<String>) {
        let Some(swap) = self.swap.clone() else { return };
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(DAY));
            let names = names(&parse_extra(extra.as_deref().unwrap_or("")).unwrap_or_default());
            match ensure(&data, &names).and_then(|renewed| renewed.then(|| load(&data.join("tls-cert.pem"), &data.join("tls-key.pem"))).transpose()) {
                Ok(Some(ck)) => {
                    *swap.0.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(ck);
                    println!("tikontrol: renewed the server certificate for {}", names.join(", "));
                }
                Ok(None) => {}
                Err(e) => eprintln!("tikontrol: certificate renewal failed: {e}"),
            }
        });
    }
}

fn load(cert: &Path, key: &Path) -> Result<CertifiedKey, String> {
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(cert)
        .and_then(|i| i.collect::<Result<_, _>>())
        .map_err(|e| format!("{}: {e}", cert.display()))?;
    let k = PrivateKeyDer::from_pem_file(key).map_err(|e| format!("{}: {e}", key.display()))?;
    CertifiedKey::from_der(certs, k, &provider()).map_err(|e| format!("{}: {e}", cert.display()))
}

fn colon_hex(der: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, der).as_ref().iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

fn private_v4(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_loopback()
}

fn parse_extra(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for n in s.split(',').map(|n| n.trim().to_ascii_lowercase()).filter(|n| !n.is_empty()) {
        let ok = match n.parse::<Ipv4Addr>() {
            Ok(ip) => private_v4(ip),
            Err(_) => {
                let label = |l: &str| !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-') && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
                n.len() <= 253 && n.split('.').all(label) && DNS_ZONES.iter().any(|z| n == *z || n.ends_with(&format!(".{z}")))
            }
        };
        if !ok {
            return Err(format!("TIK_TLS_NAMES: {n:?} is not allowed. Use private IPv4 addresses or names under {}", DNS_ZONES.join(", ")));
        }
        out.push(n);
    }
    Ok(out)
}

/// Private IPv4 addresses of this host. ponytail: Linux only (/proc/net/fib_trie); elsewhere use TIK_TLS_NAMES.
fn host_ips() -> Vec<String> {
    let t = std::fs::read_to_string("/proc/net/fib_trie").unwrap_or_default();
    let mut last = "";
    let mut out = Vec::new();
    for line in t.lines() {
        let l = line.trim();
        if let Some(ip) = l.strip_prefix("|-- ") {
            last = ip;
        } else if l.starts_with("/32 host LOCAL") && last.parse::<Ipv4Addr>().is_ok_and(|ip| private_v4(ip) && !ip.is_loopback()) {
            out.push(last.to_string());
        }
    }
    out
}

fn names(extra: &[String]) -> Vec<String> {
    let mut v: Vec<String> = ["tikontrol", "tikontrol.lan", "localhost", "127.0.0.1"].map(String::from).into();
    v.extend(host_ips());
    v.extend(extra.iter().cloned());
    v.sort();
    v.dedup();
    v
}

fn ca_params(key: &KeyPair) -> CertificateParams {
    // the name is derived from the key, so the issuer can be rebuilt from ca-key.pem alone
    let id = ring::digest::digest(&ring::digest::SHA256, &key.public_key_der());
    let mut p = CertificateParams::default();
    p.distinguished_name = DistinguishedName::new();
    p.distinguished_name.push(DnType::OrganizationName, "tikontrol");
    p.distinguished_name.push(DnType::CommonName, format!("tikontrol local CA ({:02x}{:02x})", id.as_ref()[0], id.as_ref()[1]));
    p.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    p.name_constraints = Some(NameConstraints {
        permitted_subtrees: DNS_ZONES
            .iter()
            .map(|z| GeneralSubtree::DnsName(z.to_string()))
            .chain(PRIVATE_V4.iter().map(|(a, n)| GeneralSubtree::IpAddress(CidrSubnet::from_v4_prefix(*a, *n))))
            .collect(),
        excluded_subtrees: vec![],
    });
    p.not_before = (SystemTime::now() - Duration::from_secs(3600)).into();
    p.not_after = p.not_before + Duration::from_secs(3650 * DAY);
    p
}

/// Makes sure the CA exists and the server certificate is signed by it, covers `names` and has
/// 30+ days left. Returns true when it wrote a new server certificate.
fn ensure(data: &Path, names: &[String]) -> Result<bool, String> {
    let e = |e: rcgen::Error| e.to_string();
    let io = |e: std::io::Error| e.to_string();
    let (ca_crt, ca_key) = (data.join("ca-cert.pem"), data.join("ca-key.pem"));
    let (crt, key) = (data.join("tls-cert.pem"), data.join("tls-key.pem"));
    let ca_kp = if ca_crt.exists() && ca_key.exists() {
        KeyPair::from_pem(&std::fs::read_to_string(&ca_key).map_err(io)?).map_err(e)?
    } else {
        if crt.exists() {
            println!("tikontrol: replacing the old self-signed certificate with one signed by a new local CA");
        }
        let kp = KeyPair::generate().map_err(e)?;
        let ca = ca_params(&kp).self_signed(&kp).map_err(e)?;
        write_private(&ca_key, kp.serialize_pem().as_bytes()).map_err(io)?;
        write_private(&ca_crt, ca.pem().as_bytes()).map_err(io)?;
        kp
    };
    let ca_der = CertificateDer::from_pem_file(&ca_crt).map_err(|e| format!("ca-cert.pem: {e}"))?;
    let soon = UnixTime::since_unix_epoch(Duration::from_secs(UnixTime::now().as_secs() + RENEW_DAYS * DAY));
    if key.exists() && CertificateDer::from_pem_file(&crt).is_ok_and(|c| [UnixTime::now(), soon].iter().all(|t| covers(&ca_der, &c, names, *t))) {
        return Ok(false);
    }
    let issuer = ca_params(&ca_kp).self_signed(&ca_kp).map_err(e)?;
    let mut p = CertificateParams::new(names.to_vec()).map_err(e)?;
    p.distinguished_name = DistinguishedName::new();
    p.distinguished_name.push(DnType::CommonName, "tikontrol");
    p.is_ca = IsCa::ExplicitNoCa;
    p.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    p.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    p.use_authority_key_identifier_extension = true;
    p.not_before = (SystemTime::now() - Duration::from_secs(3600)).into();
    p.not_after = p.not_before + Duration::from_secs(LEAF_DAYS * DAY);
    let kp = KeyPair::generate().map_err(e)?;
    let leaf = p.signed_by(&kp, &issuer, &ca_kp).map_err(e)?;
    write_private(&key, kp.serialize_pem().as_bytes()).map_err(io)?;
    write_private(&crt, leaf.pem().as_bytes()).map_err(io)?;
    Ok(true)
}

/// True when `leaf` chains to `ca` and is valid for every name at `at`.
/// ponytail: names dropped from the wanted set don't force a renewal; they age out within 397 days.
fn covers(ca: &CertificateDer, leaf: &CertificateDer, names: &[String], at: UnixTime) -> bool {
    let mut roots = rustls::RootCertStore::empty();
    if roots.add(ca.clone()).is_err() {
        return false;
    }
    let Ok(v) = rustls::client::WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider()).build() else { return false };
    names.iter().all(|n| ServerName::try_from(n.as_str()).is_ok_and(|sn| v.verify_server_cert(leaf, &[], &sn, &[], at).is_ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_ca_signs_a_short_server_cert_and_renews_on_new_names() {
        let d = crate::store::tests::tmpdir("tls");
        // an old self-signed certificate gets replaced
        let old = rcgen::generate_simple_self_signed(vec!["tikontrol".into()]).unwrap();
        write_private(&d.join("tls-cert.pem"), old.cert.pem().as_bytes()).unwrap();
        write_private(&d.join("tls-key.pem"), old.key_pair.serialize_pem().as_bytes()).unwrap();

        let a = setup(&d, None, None, Some("10.9.8.7, tikontrol.home.arpa")).unwrap();
        let b = setup(&d, None, None, Some("10.9.8.7, tikontrol.home.arpa")).unwrap();
        assert_eq!(a.ca_fingerprint, b.ca_fingerprint, "CA is made once");
        assert_eq!(a.fingerprint, b.fingerprint, "server cert reused while it fits");

        let ca = CertificateDer::from_pem_file(d.join("ca-cert.pem")).unwrap();
        let leaf = CertificateDer::from_pem_file(d.join("tls-cert.pem")).unwrap();
        let names = names(&parse_extra("10.9.8.7,tikontrol.home.arpa").unwrap());
        let day = |n: u64| UnixTime::since_unix_epoch(Duration::from_secs(UnixTime::now().as_secs() + n * DAY));
        assert!(covers(&ca, &leaf, &names, UnixTime::now()));
        assert!(covers(&ca, &leaf, &names, day(LEAF_DAYS - 2)));
        assert!(!covers(&ca, &leaf, &names, day(LEAF_DAYS + 1)), "valid at most 397 days");
        let server_auth = [0x06, 0x08, 0x2b, 0x06, 0x01, 0x05, 0x05, 0x07, 0x03, 0x01];
        assert!(leaf.windows(server_auth.len()).any(|w| w == server_auth), "EKU serverAuth");
        // the CA's name constraints refuse anything outside local names and private IPs
        assert!(parse_extra("example.com").is_err() && parse_extra("8.8.8.8").is_err() && parse_extra("bad_name.lan").is_err());

        let c = setup(&d, None, None, Some("10.9.8.6")).unwrap();
        assert_ne!(a.fingerprint, c.fingerprint, "new IP renews the server cert");
        assert_eq!(a.ca_fingerprint, c.ca_fingerprint, "with the same CA");
        assert!(setup(&d, Some("x"), None, None).is_err());
    }
}
