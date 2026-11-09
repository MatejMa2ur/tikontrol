//! Device SSH credentials: public keys for RouterOS users (`/user/ssh-keys`).
//!
//! Only public keys are accepted, checked by parsing their wire format: ssh-ed25519, ecdsa-sha2-nistp256/384/521,
//! or ssh-rsa with at least 3072 bits. Anything that looks like a private key is refused. Keys we add carry
//! `comment=tik:ssh-key:<user>:<label>`; removal only ever finds such a comment, so keys tikontrol didn't add
//! are never touched. The key's own comment is dropped (the label replaces it).
//!
//! Router facts (CHR 7.25.1): adding needs the writer's group to have the `policy` policy (`write` alone gives
//! "not enough permissions"); listing works for the read-only reader. With the default
//! `/ip/ssh password-authentication=yes-if-no-key`, a user with a key can no longer log in over SSH with a
//! password, so the plan warns about it.

use crate::apply::{plan, ros_name, Op, Plan, Risk};
use crate::ros::cmd::Cmd;
use crate::ros::conn::Router;
use serde::Deserialize;

pub const KINDS: &[&str] = &["ssh-key", "remove-ssh-key"];
const PATH: &str = "/user/ssh-keys";

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum UserIntent {
    SshKey { user: String, label: String, key: String },
    RemoveSshKey { user: String, label: String },
}

pub fn compile(v: &serde_json::Value, r: &mut dyn Router) -> Option<Result<Plan, String>> {
    let kind = v.get("kind")?.as_str()?;
    if !KINDS.contains(&kind) {
        return None;
    }
    Some(serde_json::from_value::<UserIntent>(v.clone()).map_err(|e| format!("invalid {kind} intent: {e}")).and_then(|i| build(&i, r)))
}

fn label(s: &str) -> Result<String, String> {
    let s = s.trim();
    let ok = !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.@".contains(&b));
    ok.then(|| s.to_string()).ok_or_else(|| "invalid label (1-32 letters, digits, - _ . @)".into())
}

fn b64(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let s = s.as_bytes();
    if s.is_empty() || !s.len().is_multiple_of(4) {
        return None;
    }
    let body = s.strip_suffix(b"==").or_else(|| s.strip_suffix(b"=")).unwrap_or(s);
    let (mut out, mut acc, mut n) = (Vec::new(), 0u32, 0);
    for &c in body {
        acc = (acc << 6) | val(c)? as u32;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((acc >> n) as u8);
        }
    }
    Some(out)
}

/// SSH wire-format strings: u32 length + bytes.
fn fields(mut b: &[u8]) -> Option<Vec<&[u8]>> {
    let mut v = vec![];
    while !b.is_empty() {
        let n = u32::from_be_bytes(b.get(..4)?.try_into().ok()?) as usize;
        v.push(b.get(4..4 + n)?);
        b = &b[4 + n..];
    }
    Some(v)
}

/// Validates an OpenSSH public key line and returns "type base64" (its comment dropped).
pub fn public_key(s: &str) -> Result<String, String> {
    let s = s.trim();
    if s.contains("PRIVATE KEY") || s.contains("PuTTY-User-Key") || s.contains('\n') {
        return Err("that looks like a private key: paste the public key (the .pub file), never the private one".into());
    }
    let mut parts = s.split_whitespace();
    let (t, data) = (parts.next().unwrap_or_default(), parts.next().unwrap_or_default());
    let bad = || "invalid public key (expected one line like \"ssh-ed25519 AAAA… comment\")".to_string();
    let raw = b64(data).ok_or_else(bad)?;
    let f = fields(&raw).ok_or_else(bad)?;
    if f.first().copied() != Some(t.as_bytes()) {
        return Err(bad());
    }
    let ok = match (t, f.len()) {
        ("ssh-ed25519", 2) => f[1].len() == 32,
        ("ecdsa-sha2-nistp256", 3) => f[1] == b"nistp256" && f[2].len() == 65,
        ("ecdsa-sha2-nistp384", 3) => f[1] == b"nistp384" && f[2].len() == 97,
        ("ecdsa-sha2-nistp521", 3) => f[1] == b"nistp521" && f[2].len() == 133,
        ("ssh-rsa", 3) => {
            let n = f[2].iter().skip_while(|&&b| b == 0).count();
            if n * 8 < 3072 - 7 {
                return Err("RSA keys need at least 3072 bits: use ssh-ed25519 instead".into());
            }
            true
        }
        ("ssh-rsa" | "ssh-ed25519", _) | ("ecdsa-sha2-nistp256" | "ecdsa-sha2-nistp384" | "ecdsa-sha2-nistp521", _) => false,
        _ => return Err("unsupported key type: use ssh-ed25519, ecdsa-sha2-nistp256/384/521 or ssh-rsa (3072+ bits)".into()),
    };
    ok.then(|| format!("{t} {data}")).ok_or_else(bad)
}

fn build(i: &UserIntent, r: &mut dyn Router) -> Result<Plan, String> {
    let (user, lbl) = match i {
        UserIntent::SshKey { user, label: l, .. } | UserIntent::RemoveSshKey { user, label: l } => (ros_name(user)?, label(l)?),
    };
    let tag = format!("tik:ssh-key:{user}:{lbl}");
    let existing = r.run(&Cmd::new(&format!("{PATH}/print")).proplist(&["user", "key", "comment"]).query("comment", &tag))?;
    match i {
        UserIntent::SshKey { key, .. } => {
            let key = public_key(key)?;
            if r.run(&Cmd::new("/user/print").proplist(&["name"]).query("name", &user))?.is_empty() {
                return Err(format!("the router has no user {user:?}"));
            }
            if !existing.is_empty() {
                return Err(format!("{user} already has a key labelled {lbl:?}: remove it first"));
            }
            let ssh = r.run(&Cmd::new("/ip/ssh/print").proplist(&["password-authentication"]))?;
            let pw = ssh.first().and_then(|x| x.get("password-authentication")).map_or("yes-if-no-key", String::as_str);
            let mut p = plan(
                format!("Add SSH key {lbl} for {user}"),
                Risk::Medium,
                vec![Op::add(PATH, vec![("user", user.clone()), ("key", key), ("comment", tag.clone())])],
                vec![Op::on(PATH, "remove", "comment", &tag, vec![])],
            );
            if pw == "yes-if-no-key" {
                p.warnings.push(format!("{user} can then no longer log in over SSH with a password (RouterOS /ip/ssh password-authentication=yes-if-no-key): keep the private key safe"));
            }
            p.warnings.push("The router user making this change needs the 'policy' policy".into());
            Ok(p)
        }
        UserIntent::RemoveSshKey { .. } => {
            let Some(row) = existing.first() else { return Err(format!("no key labelled {lbl:?} for {user} that tikontrol added")) };
            let key = row.get("key").cloned().ok_or("the router did not return the key, so the removal can't be undone")?;
            let key = public_key(&key)?;
            Ok(plan(
                format!("Remove SSH key {lbl} from {user}"),
                Risk::Low,
                vec![Op::on(PATH, "remove", "comment", &tag, vec![])],
                vec![Op::add(PATH, vec![("user", user), ("key", key), ("comment", tag.clone())])],
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply::compile_value;
    use crate::ros::queries::tests::{row, Fake};
    use serde_json::json;

    // throwaway test keys (public halves only)
    const ED: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBv30pa9ZGPrTRuTgYYp3OGtlBnqE/BtqvZvn9Z5daiJ";
    const EC: &str = "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBG+EfQGrvCIUUsHp6+RfCqflp6YkDA4/1rdFiJDISSBw0XD9M9U1AWHT9wAIFl0Y0Ep+xfClwaiwPWSeyPhHOt8=";
    const RSA2048: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQC6rvtXcTXWl3ak9p5K3PfC0dHQ6QDmvDwcCbID6PSz3URJoMKxVZxiNgznkTTz1QR1P8FLUFaEQ2yMqxuAOqxxH999M/skpuwsbS2NQmAJMWDQptCoM2myWRLR41+O3OiSe1cz8R9MWR2FiI6h4ALdDrL1frrd0jbEUVw0OZVuVp2q/UVwOSz4bD7bZQqGcW3gK+Ql4F6UBueg25Pkar49p8QEr/hczAfnjbtUHmxXuCOgfLmtnbdhJ/lR0ugOUGWRq8dWKEi71P+OO5xjJLNvrm0gbRN9nAJynj24bLtkZ4PlGpmiKfRXcZ7VdmbXa0MaOQt2H+mODUwR8/22gHAx";
    const RSA3072: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC8uVrlrSBKW1CPibPsAQgbTW3ORsce0aUXpDvIOSpJNxIR6BBzxJO2/gZn/SEJr8gHmFpmNGEy4bR4cSRPcPSEH1HKThQE3aslTc7MzJe+dPF3YOmbqi0HGBbOAk519c5y/gyJU2ExGz0us7aES0dnBrKLlesqZDNzIAVmC9jmYq4v659qelPprk2HD4F/DlD3Qv5OQ9/FASCrlAU1dToUFeH7C42JUoYHBGRx6Luf51a3oOZ7SNyE43970biR0b2T1EWKDHL6XtacH3DOtVsBs0+7AT4KO/Rf4sszfEJsmPA7VfdYDGlGuBF39vHCoOnn0LZ4M6PIokvtDYR9sVWhljLqX+ZgJSY7NiiviJgPu16nMmXq3rkCxzKRbRteyn7Ht3mICvciziyUmLbtnavb/lOzLFG/HiOUnRUImZFXb33y0HHaKWkO7aAznTdWaTTNRRTauNffXVMmTZu2sWPkpBEQF6DxbGMMhs5JFNM1dXC97HwV8l2Z6Z0hhzovEfE=";

    fn router() -> Fake {
        let mut f = Fake::default();
        f.answers.insert("/user/print".into(), Ok(vec![row(&[("name", "ops")])]));
        f
    }

    #[test]
    fn every_kind_is_registered() {
        assert!(compile(&json!({"kind": "dns-record"}), &mut Fake::default()).is_none());
        for k in KINDS {
            if let Err(e) = compile(&json!({"kind": k}), &mut Fake::default()).unwrap() {
                assert!(!e.contains("unknown variant"), "{k}: {e}");
            }
        }
    }

    #[test]
    fn only_strong_public_keys_pass() {
        assert_eq!(public_key(&format!("{ED} me@example.com")).unwrap(), ED);
        assert_eq!(public_key(EC).unwrap(), EC);
        assert_eq!(public_key(RSA3072).unwrap(), RSA3072);
        assert!(public_key(RSA2048).unwrap_err().contains("3072"));
        assert!(public_key("-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\n-----END OPENSSH PRIVATE KEY-----").unwrap_err().contains("private"));
        assert!(public_key("ssh-dss AAAAB3NzaC1kc3MAAACBAP").is_err());
        assert!(public_key("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBv30pa9").is_err(), "truncated");
        assert!(public_key(&ED.replace("ssh-ed25519", "ecdsa-sha2-nistp256")).is_err(), "type must match the blob");
        assert!(public_key("ssh-ed25519 not*base64").is_err());
        assert!(public_key("").is_err());
    }

    #[test]
    fn add_tags_the_key_and_removes_by_tag() {
        let p = compile_value(&json!({"kind": "ssh-key", "user": "ops", "label": "laptop", "key": format!("{ED} me@example.com")}), &mut router()).unwrap();
        assert_eq!(p.cli(), [format!("/user/ssh-keys/add user=ops key=\"{ED}\" comment=tik:ssh-key:ops:laptop")]);
        assert_eq!(p.undo_cli(), [r#"/user/ssh-keys/remove [find comment="tik:ssh-key:ops:laptop"]"#]);
        assert!(p.warnings[0].contains("password"));
        assert!(compile_value(&json!({"kind": "ssh-key", "user": "nobody", "label": "x", "key": ED}), &mut Fake::default()).unwrap_err().contains("no user"));
        assert!(compile_value(&json!({"kind": "ssh-key", "user": "ops", "label": "a b", "key": ED}), &mut router()).is_err());
        assert!(compile_value(&json!({"kind": "ssh-key", "user": "ops\"; /system/reboot", "label": "x", "key": ED}), &mut router()).is_err());
    }

    #[test]
    fn remove_only_touches_our_keys_and_restores_them() {
        assert!(compile_value(&json!({"kind": "remove-ssh-key", "user": "ops", "label": "laptop"}), &mut router()).unwrap_err().contains("tikontrol added"));
        let mut f = router();
        f.answers.insert("/user/ssh-keys/print".into(), Ok(vec![row(&[("user", "ops"), ("key", &format!("{ED} me@example.com")), ("comment", "tik:ssh-key:ops:laptop")])]));
        let p = compile_value(&json!({"kind": "remove-ssh-key", "user": "ops", "label": "laptop"}), &mut f).unwrap();
        assert_eq!(p.cli(), [r#"/user/ssh-keys/remove [find comment="tik:ssh-key:ops:laptop"]"#]);
        assert_eq!(p.undo_cli(), [format!("/user/ssh-keys/add user=ops key=\"{ED}\" comment=tik:ssh-key:ops:laptop")]);
        assert!(f.seen.iter().any(|w| w.iter().any(|x| x == "?comment=tik:ssh-key:ops:laptop")), "found by our tag only");
    }
}
