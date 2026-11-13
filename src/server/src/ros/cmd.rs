//! Typed RouterOS API commands. Words are built from validated parts, never from joined strings,
//! so a hostname or comment from another device can't turn into an extra attribute or command.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmd {
    words: Vec<String>,
    err: Option<String>,
}

pub const MAX_VALUE: usize = 4096;

fn valid_path(p: &str) -> bool {
    p.starts_with('/')
        && p.len() > 1
        && p.len() < 128
        && !p.contains("//")
        && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"/-".contains(&b))
}

/// Attribute names: `name`, `mac-address`, `channel.band`, `.id`.
pub fn valid_key(k: &str) -> bool {
    !k.is_empty()
        && k.len() < 64
        && k.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-._".contains(&b))
        && k != "."
}

/// Values may contain '=' and spaces (the framing is length-prefixed), but no control bytes.
pub fn valid_value(v: &str) -> bool {
    v.len() <= MAX_VALUE && !v.chars().any(|c| c.is_control() && c != '\t')
}

/// A PEM block for `/file` `contents` (certificate import): the only value that may carry newlines. The API
/// framing is length-prefixed, and `cli_quote` escapes them, so they can't start a new command anywhere.
pub fn valid_pem(v: &str) -> bool {
    v.len() <= MAX_VALUE && v.starts_with("-----BEGIN ") && v.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=- \n".contains(&b))
}

/// `valid_value`, plus PEM text when the argument is `contents`.
pub fn valid_arg(k: &str, v: &str) -> bool {
    valid_value(v) || (k == "contents" && valid_pem(v))
}

impl Cmd {
    pub fn new(path: &str) -> Self {
        let err = (!valid_path(path)).then(|| format!("invalid command path {path:?}"));
        Cmd { words: vec![path.to_string()], err }
    }

    fn push(mut self, word: String, ok: bool, what: &str) -> Self {
        if !ok && self.err.is_none() {
            self.err = Some(format!("invalid {what}"));
        }
        self.words.push(word);
        self
    }

    /// `=key=value`
    pub fn arg(self, k: &str, v: &str) -> Self {
        let ok = valid_key(k) && valid_arg(k, v);
        self.push(format!("={k}={v}"), ok, &format!("argument {k:?}"))
    }

    /// `=.proplist=a,b,c` — every read must name its fields so secrets never come back.
    pub fn proplist(self, keys: &[&str]) -> Self {
        let ok = !keys.is_empty() && keys.iter().all(|k| valid_key(k));
        self.push(format!("=.proplist={}", keys.join(",")), ok, "proplist")
    }

    /// `?key=value` exact-match query.
    pub fn query(self, k: &str, v: &str) -> Self {
        let ok = valid_key(k) && valid_value(v);
        self.push(format!("?{k}={v}"), ok, &format!("query {k:?}"))
    }

    pub fn tag(self, t: u32) -> Self {
        self.push(format!(".tag={t}"), true, "tag")
    }

    pub fn path(&self) -> &str {
        &self.words[0]
    }

    pub fn words(&self) -> Result<&[String], String> {
        match &self.err {
            Some(e) => Err(e.clone()),
            None => Ok(&self.words),
        }
    }

    /// RouterOS CLI form, for "Show in RouterOS" and the Safe apply preview. Display only, never executed.
    pub fn to_cli(&self) -> String {
        let mut s = self.words[0].clone();
        for w in &self.words[1..] {
            if let Some(kv) = w.strip_prefix('=') {
                let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                if k == ".proplist" {
                    continue;
                }
                s.push(' ');
                s.push_str(k);
                s.push('=');
                s.push_str(&cli_quote(v));
            } else if let Some(kv) = w.strip_prefix('?') {
                s.push_str(&format!(" where {kv}"));
            }
        }
        s
    }
}

pub fn cli_quote(v: &str) -> String {
    let plain = !v.is_empty() && v.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._:/,*".contains(&b));
    if plain {
        return v.to_string();
    }
    let mut s = String::from("\"");
    for c in v.chars() {
        match c {
            '"' | '\\' | '$' | '?' => {
                s.push('\\');
                s.push(c)
            }
            '\t' => s.push_str("\\t"),
            '\n' => s.push_str("\\n"),
            _ => s.push(c),
        }
    }
    s.push('"');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_words() {
        let c = Cmd::new("/ip/dhcp-server/lease/print").proplist(&["address", "mac-address"]).query("dynamic", "true");
        assert_eq!(c.words().unwrap(), ["/ip/dhcp-server/lease/print", "=.proplist=address,mac-address", "?dynamic=true"]);
    }

    #[test]
    fn values_with_equals_and_spaces_stay_one_word() {
        let c = Cmd::new("/ip/dns/static/add").arg("comment", "a=b ?x=y");
        assert_eq!(c.words().unwrap()[1], "=comment=a=b ?x=y");
    }

    #[test]
    fn rejects_injection_attempts() {
        assert!(Cmd::new("/ip/x").arg("comment", "x\n/system/reboot").words().is_err());
        assert!(Cmd::new("/ip/x").arg("a=b", "c").words().is_err());
        assert!(Cmd::new("/ip/x").arg("", "c").words().is_err());
        assert!(Cmd::new("/ip/x").arg("a", "\0").words().is_err());
        assert!(Cmd::new("ip/x").words().is_err());
        assert!(Cmd::new("/ip x").words().is_err());
        assert!(Cmd::new("/ip/x").proplist(&[]).words().is_err());
        assert!(Cmd::new("/ip/x").proplist(&["a,b"]).words().is_err());
        assert!(Cmd::new("/ip/x").arg("a", &"x".repeat(MAX_VALUE + 1)).words().is_err());
        // newlines only in PEM `contents`, and only PEM characters there
        let pem = "-----BEGIN CERTIFICATE-----\nMIIB+/=\n-----END CERTIFICATE-----\n";
        assert!(Cmd::new("/file/add").arg("contents", pem).words().is_ok());
        assert!(Cmd::new("/file/add").arg("comment", pem).words().is_err());
        assert!(Cmd::new("/file/add").arg("contents", "-----BEGIN x\n:do { /system/reboot }\n").words().is_err());
        assert!(Cmd::new("/file/add").arg("contents", "x\n/system/reboot").words().is_err());
        assert_eq!(cli_quote(pem), r#""-----BEGIN CERTIFICATE-----\nMIIB+/=\n-----END CERTIFICATE-----\n""#);
    }

    #[test]
    fn cli_preview_quotes_untrusted_text() {
        let c = Cmd::new("/ip/dns/static/add").arg("name", "nas.lan").arg("comment", "say \"hi\" $x");
        assert_eq!(c.to_cli(), r#"/ip/dns/static/add name=nas.lan comment="say \"hi\" \$x""#);
    }
}
