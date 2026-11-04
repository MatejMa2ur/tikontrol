//! HTTPS for the web UI. Uses the operator's certificate when given (TIK_TLS_CERT / TIK_TLS_KEY),
//! otherwise generates a self-signed one on first start and keeps it in the data directory.

use crate::store::write_private;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::path::Path;
use std::sync::Arc;

pub fn server_config(data: &Path, cert: Option<&str>, key: Option<&str>) -> Result<(Arc<rustls::ServerConfig>, String), String> {
    let (cert_path, key_path) = match (cert, key) {
        (Some(c), Some(k)) => (Path::new(c).to_path_buf(), Path::new(k).to_path_buf()),
        (None, None) => {
            let (c, k) = (data.join("tls-cert.pem"), data.join("tls-key.pem"));
            if !c.exists() || !k.exists() {
                let gen = rcgen::generate_simple_self_signed(vec!["tikontrol".into(), "tikontrol.lan".into(), "localhost".into()]).map_err(|e| e.to_string())?;
                write_private(&c, gen.cert.pem().as_bytes()).map_err(|e| e.to_string())?;
                write_private(&k, gen.key_pair.serialize_pem().as_bytes()).map_err(|e| e.to_string())?;
            }
            (c, k)
        }
        _ => return Err("set both TIK_TLS_CERT and TIK_TLS_KEY, or neither".into()),
    };
    let certs: Vec<CertificateDer> = rustls_pemfile::certs(&mut std::io::BufReader::new(std::fs::File::open(&cert_path).map_err(|e| format!("{}: {e}", cert_path.display()))?))
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let key: PrivateKeyDer = rustls_pemfile::private_key(&mut std::io::BufReader::new(std::fs::File::open(&key_path).map_err(|e| format!("{}: {e}", key_path.display()))?))
        .map_err(|e| e.to_string())?
        .ok_or("no private key in key file")?;
    let fp = certs.first().map(|c| crate::ros::conn::fingerprint(c)).ok_or("no certificate in cert file")?;
    let cfg = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| e.to_string())?;
    Ok((Arc::new(cfg), fp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_once_and_reuses() {
        let d = crate::store::tests::tmpdir("tls");
        let (_, a) = server_config(&d, None, None).unwrap();
        let (_, b) = server_config(&d, None, None).unwrap();
        assert_eq!(a, b);
        assert!(server_config(&d, Some("x"), None).is_err());
    }
}
