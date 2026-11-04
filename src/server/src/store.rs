//! Persistent state: one JSON file, written atomically with mode 0600. Router passwords and other
//! secrets are encrypted with ChaCha20-Poly1305 under a key kept in a separate 0600 file, so a
//! copied state file or backup alone reveals nothing.

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    SystemRandom::new().fill(&mut b).expect("system RNG unavailable");
    b
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// Constant-time equality for secrets (tokens, CSRF values).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut o = fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let mut f = o.open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(tmp, path)
}

pub struct Sealer(LessSafeKey);

impl Sealer {
    /// Loads the key file, creating it (0600) on first start.
    pub fn open(path: &Path) -> std::io::Result<Sealer> {
        let key = match fs::read(path) {
            Ok(k) if k.len() == 32 => k,
            Ok(_) => return Err(std::io::Error::other("key file has the wrong size")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let k = random_bytes::<32>().to_vec();
                write_private(path, &k)?;
                k
            }
            Err(e) => return Err(e),
        };
        Ok(Sealer::from_key(&key))
    }

    pub fn from_key(key: &[u8]) -> Sealer {
        Sealer(LessSafeKey::new(UnboundKey::new(&CHACHA20_POLY1305, key).expect("32-byte key")))
    }

    /// `label` is bound in as associated data, so a ciphertext can't be moved to another field.
    pub fn seal(&self, label: &str, plain: &str) -> String {
        let n = random_bytes::<NONCE_LEN>();
        let mut buf = plain.as_bytes().to_vec();
        self.0.seal_in_place_append_tag(Nonce::assume_unique_for_key(n), Aad::from(label.as_bytes()), &mut buf).expect("seal");
        format!("{}{}", hex(&n), hex(&buf))
    }

    pub fn open_str(&self, label: &str, sealed: &str) -> Option<String> {
        let raw = unhex(sealed)?;
        if raw.len() < NONCE_LEN + 16 {
            return None;
        }
        let (n, ct) = raw.split_at(NONCE_LEN);
        let mut ct = ct.to_vec();
        let nonce = Nonce::try_assume_unique_for_key(n).ok()?;
        let plain = self.0.open_in_place(nonce, Aad::from(label.as_bytes()), &mut ct).ok()?;
        String::from_utf8(plain.to_vec()).ok()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Viewer,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Admin {
    pub name: String,
    pub salt: String,
    pub hash: String,
    pub role: Role,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    #[default]
    ReadOnly,
    ReadWrite,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Cred {
    pub user: String,
    /// sealed with label "router-password:<role>"
    pub password: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RouterCfg {
    pub host: String,
    pub port: u16,
    pub pin: String,
    pub mode: Mode,
    pub reader: Cred,
    /// only present in read-write mode
    pub writer: Option<Cred>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Prefs {
    pub aliases: BTreeMap<String, String>,
    pub icons: BTreeMap<String, String>,
    pub notes: BTreeMap<String, String>,
    pub groups: Vec<serde_json::Value>,
    pub hidden: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct McpCfg {
    pub enabled: bool,
    /// SHA-256 of the bearer token; the token itself is shown once
    pub token_hash: String,
    pub privacy: bool,
    /// HMAC key for privacy-mode pseudonyms, sealed with label "mcp-pseudonym-key"
    pub key: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct State {
    pub admins: Vec<Admin>,
    pub router: Option<RouterCfg>,
    pub prefs: Prefs,
    pub mcp: McpCfg,
    pub alerts: serde_json::Value,
    pub site: serde_json::Value,
}

pub struct Store {
    path: PathBuf,
    pub sealer: Sealer,
    pub state: State,
}

impl Store {
    pub fn open(dir: &Path) -> std::io::Result<Store> {
        fs::create_dir_all(dir)?;
        let sealer = Sealer::open(&dir.join("key"))?;
        let path = dir.join("state.json");
        let state = match fs::read(&path) {
            Ok(b) => serde_json::from_slice(&b).map_err(|e| std::io::Error::other(format!("state.json is corrupt: {e}")))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(e) => return Err(e),
        };
        Ok(Store { path, sealer, state })
    }

    pub fn dir(&self) -> PathBuf {
        self.path.parent().map(Path::to_path_buf).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        write_private(&self.path, &serde_json::to_vec_pretty(&self.state).map_err(std::io::Error::other)?)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tik-test-{name}-{}", hex(&random_bytes::<6>())));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn seal_round_trips_and_binds_the_label() {
        let s = Sealer::from_key(&[7u8; 32]);
        let c = s.seal("router-password:reader", "hunter2");
        assert!(!c.contains("hunter2"));
        assert_eq!(s.open_str("router-password:reader", &c).as_deref(), Some("hunter2"));
        assert_eq!(s.open_str("router-password:writer", &c), None);
        assert_eq!(Sealer::from_key(&[8u8; 32]).open_str("router-password:reader", &c), None);
        assert_ne!(s.seal("l", "x"), s.seal("l", "x"), "fresh nonce each time");
        assert_eq!(s.open_str("l", "zz"), None);
    }

    #[test]
    fn store_persists_privately_and_never_in_plain_text() {
        let d = tmpdir("store");
        let mut st = Store::open(&d).unwrap();
        let pw = st.sealer.seal("router-password:reader", "pa55word");
        st.state.router = Some(RouterCfg { host: "192.0.2.1".into(), port: 8729, pin: "ab".into(), mode: Mode::ReadOnly, reader: Cred { user: "tik-read".into(), password: pw }, writer: None });
        st.save().unwrap();
        let raw = fs::read_to_string(d.join("state.json")).unwrap();
        assert!(!raw.contains("pa55word"));
        let again = Store::open(&d).unwrap();
        let r = again.state.router.unwrap();
        assert_eq!(again.sealer.open_str("router-password:reader", &r.reader.password).as_deref(), Some("pa55word"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(d.join("state.json")).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(fs::metadata(d.join("key")).unwrap().permissions().mode() & 0o777, 0o600);
        }
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn helpers() {
        assert_eq!(unhex(&hex(&[0, 255, 16])).unwrap(), [0, 255, 16]);
        assert_eq!(unhex("abc"), None);
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }
}
