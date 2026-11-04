//! tikontrol's own admins: PBKDF2-SHA256 password hashes, server-side sessions, CSRF tokens and
//! login rate limiting. Everything is denied unless a valid session (or MCP token) says otherwise.

use crate::store::{ct_eq, hex, random_bytes, unhex, Admin, Role};
use ring::pbkdf2;
use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::num::NonZeroU32;
use std::time::{Duration, Instant};

/// OWASP's 2023 figure for PBKDF2-HMAC-SHA256. Tests use fewer so they stay fast.
#[cfg(not(test))]
const ITER: u32 = 600_000;
#[cfg(test)]
const ITER: u32 = 1_000;

pub const MIN_PASSWORD: usize = 12;
pub const MAX_PASSWORD: usize = 256;

fn derive(salt: &[u8], pw: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    pbkdf2::derive(pbkdf2::PBKDF2_HMAC_SHA256, NonZeroU32::new(ITER).unwrap(), salt, pw.as_bytes(), &mut out);
    out
}

pub fn check_password_policy(pw: &str) -> Result<(), String> {
    let n = pw.chars().count();
    if n < MIN_PASSWORD {
        return Err(format!("password must be at least {MIN_PASSWORD} characters"));
    }
    if pw.len() > MAX_PASSWORD {
        return Err("password is too long".into());
    }
    Ok(())
}

pub fn valid_username(u: &str) -> bool {
    !u.is_empty() && u.len() <= 32 && u.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

pub fn new_admin(name: &str, pw: &str, role: Role) -> Result<Admin, String> {
    if !valid_username(name) {
        return Err("username: 1-32 letters, digits, '-', '_' or '.'".into());
    }
    check_password_policy(pw)?;
    let salt = random_bytes::<16>();
    Ok(Admin { name: name.into(), salt: hex(&salt), hash: hex(&derive(&salt, pw)), role })
}

/// Verifies a login. Unknown users still pay the full hashing cost, so timing doesn't reveal names.
pub fn verify<'a>(admins: &'a [Admin], name: &str, pw: &str) -> Option<&'a Admin> {
    if pw.len() > MAX_PASSWORD {
        return None;
    }
    match admins.iter().find(|a| a.name == name) {
        Some(a) => {
            let salt = unhex(&a.salt)?;
            let want = unhex(&a.hash)?;
            ct_eq(&derive(&salt, pw), &want).then_some(a)
        }
        None => {
            let _ = derive(&[0u8; 16], pw);
            None
        }
    }
}

pub fn sha256_hex(s: &str) -> String {
    hex(ring::digest::digest(&ring::digest::SHA256, s.as_bytes()).as_ref())
}

pub const IDLE: Duration = Duration::from_secs(30 * 60);
pub const ABSOLUTE: Duration = Duration::from_secs(12 * 3600);
pub const COOKIE: &str = "tik_s";

/// A RouterOS password kept only in memory for one session; never printed, logged or stored.
#[derive(Clone)]
pub struct Secret(pub String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

#[derive(Clone, Debug)]
pub struct Session {
    pub user: String,
    pub role: Role,
    pub csrf: String,
    /// set when the user logged in with their MikroTik account: their changes run as them
    pub ros: Option<Secret>,
    created: Instant,
    last: Instant,
}

#[derive(Default)]
pub struct Sessions {
    // keyed by SHA-256 of the token: a memory dump doesn't hand out live cookies
    map: HashMap<String, Session>,
}

impl Sessions {
    pub fn create(&mut self, user: &str, role: Role) -> (String, Session) {
        self.create_with(user, role, None)
    }

    pub fn create_with(&mut self, user: &str, role: Role, ros: Option<Secret>) -> (String, Session) {
        self.sweep();
        let token = hex(&random_bytes::<32>());
        let now = Instant::now();
        let s = Session { user: user.into(), role, csrf: hex(&random_bytes::<32>()), ros, created: now, last: now };
        self.map.insert(sha256_hex(&token), s.clone());
        (token, s)
    }

    pub fn get(&mut self, token: &str) -> Option<Session> {
        let k = sha256_hex(token);
        let now = Instant::now();
        let s = self.map.get_mut(&k)?;
        if now.duration_since(s.last) > IDLE || now.duration_since(s.created) > ABSOLUTE {
            self.map.remove(&k);
            return None;
        }
        s.last = now;
        Some(s.clone())
    }

    pub fn remove(&mut self, token: &str) {
        self.map.remove(&sha256_hex(token));
    }

    /// Ends every session of a user (password change, user removed, mode switch).
    pub fn remove_user(&mut self, user: &str) {
        self.map.retain(|_, s| s.user != user);
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    fn sweep(&mut self) {
        let now = Instant::now();
        self.map.retain(|_, s| now.duration_since(s.last) <= IDLE && now.duration_since(s.created) <= ABSOLUTE);
    }
}

pub fn session_cookie(token: &str, tls: bool) -> String {
    format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{}", ABSOLUTE.as_secs(), if tls { "; Secure" } else { "" })
}

pub fn clear_cookie(tls: bool) -> String {
    format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{}", if tls { "; Secure" } else { "" })
}

/// Same-origin check for state-changing requests, on top of the CSRF token and SameSite cookie.
/// Browsers always send Origin on POST; Sec-Fetch-Site is a second signal when present.
pub fn same_origin(origin: Option<&str>, host: Option<&str>, fetch_site: Option<&str>) -> bool {
    if matches!(fetch_site, Some(s) if s != "same-origin" && s != "none") {
        return false;
    }
    match (origin, host) {
        (Some(o), Some(h)) => o.strip_prefix("https://").or_else(|| o.strip_prefix("http://")) == Some(h),
        (None, _) => fetch_site.is_none() || fetch_site == Some("same-origin"),
        _ => false,
    }
}

const PER_IP_FAILS: usize = 5;
const WINDOW: Duration = Duration::from_secs(15 * 60);
const GLOBAL_FAILS_PER_MIN: usize = 30;

#[derive(Default)]
pub struct Limiter {
    per_ip: HashMap<IpAddr, VecDeque<Instant>>,
    global: VecDeque<Instant>,
}

impl Limiter {
    /// Ok, or how many seconds to wait. Called before the password is checked.
    pub fn check(&mut self, ip: IpAddr) -> Result<(), u64> {
        self.check_at(ip, Instant::now())
    }

    fn check_at(&mut self, ip: IpAddr, now: Instant) -> Result<(), u64> {
        self.global.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
        if self.global.len() >= GLOBAL_FAILS_PER_MIN {
            return Err(60);
        }
        let q = self.per_ip.entry(ip).or_default();
        q.retain(|t| now.duration_since(*t) < WINDOW);
        if q.len() >= PER_IP_FAILS {
            let oldest = *q.front().unwrap();
            return Err((WINDOW - now.duration_since(oldest)).as_secs().max(1));
        }
        Ok(())
    }

    pub fn fail(&mut self, ip: IpAddr) {
        self.fail_at(ip, Instant::now())
    }

    fn fail_at(&mut self, ip: IpAddr, now: Instant) {
        self.per_ip.entry(ip).or_default().push_back(now);
        self.global.push_back(now);
        if self.per_ip.len() > 10_000 {
            self.per_ip.retain(|_, q| q.back().is_some_and(|t| now.duration_since(*t) < WINDOW));
        }
    }

    pub fn success(&mut self, ip: IpAddr) {
        self.per_ip.remove(&ip);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_verifies() {
        let a = new_admin("admin", "correct horse battery", Role::Admin).unwrap();
        assert!(!a.hash.contains("correct"));
        let all = vec![a];
        assert!(verify(&all, "admin", "correct horse battery").is_some());
        assert!(verify(&all, "admin", "wrong password here").is_none());
        assert!(verify(&all, "nobody", "correct horse battery").is_none());
        assert!(verify(&all, "admin", &"x".repeat(MAX_PASSWORD + 1)).is_none());
    }

    #[test]
    fn password_and_username_policy() {
        assert!(new_admin("admin", "short", Role::Admin).is_err());
        assert!(new_admin("ad min", "long enough pass", Role::Admin).is_err());
        assert!(new_admin("", "long enough pass", Role::Admin).is_err());
        assert!(new_admin("<script>", "long enough pass", Role::Admin).is_err());
    }

    #[test]
    fn sessions_expire_and_are_stored_hashed() {
        let mut s = Sessions::default();
        let (tok, sess) = s.create("admin", Role::Admin);
        assert_eq!(tok.len(), 64);
        assert!(!s.map.contains_key(&tok));
        assert_eq!(s.get(&tok).unwrap().csrf, sess.csrf);
        assert!(s.get("nope").is_none());
        s.map.values_mut().for_each(|x| x.last -= IDLE + Duration::from_secs(1));
        assert!(s.get(&tok).is_none());
        let (t2, _) = s.create("admin", Role::Admin);
        s.remove_user("admin");
        assert!(s.get(&t2).is_none());
    }

    #[test]
    fn cookie_flags() {
        let c = session_cookie("t", true);
        assert!(c.contains("HttpOnly") && c.contains("SameSite=Strict") && c.contains("Secure"));
    }

    #[test]
    fn origin_checks() {
        assert!(same_origin(Some("https://tik.lan:8443"), Some("tik.lan:8443"), Some("same-origin")));
        assert!(!same_origin(Some("https://evil.example"), Some("tik.lan:8443"), None));
        assert!(!same_origin(Some("https://tik.lan:8443"), Some("tik.lan:8443"), Some("cross-site")));
        assert!(!same_origin(Some("https://tik.lan:8443.evil"), Some("tik.lan:8443"), None));
        assert!(same_origin(None, Some("tik.lan"), None)); // non-browser client; still needs the CSRF token
    }

    #[test]
    fn rate_limit_locks_out_per_ip_and_globally() {
        let mut l = Limiter::default();
        let ip: IpAddr = "192.0.2.10".parse().unwrap();
        let now = Instant::now();
        for _ in 0..PER_IP_FAILS {
            assert!(l.check_at(ip, now).is_ok());
            l.fail_at(ip, now);
        }
        assert!(l.check_at(ip, now).is_err());
        assert!(l.check_at(ip, now + WINDOW).is_ok());
        let other: IpAddr = "192.0.2.11".parse().unwrap();
        assert!(l.check_at(other, now).is_ok());
        for i in 0..GLOBAL_FAILS_PER_MIN {
            l.fail_at(format!("198.51.100.{i}").parse().unwrap(), now);
        }
        assert_eq!(l.check_at(other, now), Err(60));
    }
}
