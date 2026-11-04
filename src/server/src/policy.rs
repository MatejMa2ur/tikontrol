//! Read-only mode is enforced by the router: tikontrol reads the real policies of the RouterOS
//! user it logs in as, and refuses to run in read-only mode if that user could change anything.

use crate::ros::cmd::{cli_quote, Cmd};
use crate::ros::conn::Router;
use serde::Serialize;

/// None of these may be on the reader user. `read` alone can't change config, but these can
/// reboot, reveal secrets, change passwords or rewrite policies.
pub const FORBIDDEN_READ_ONLY: &[&str] = &["write", "sensitive", "password", "policy", "reboot"];
/// Off for both users: tikontrol never needs these, and each one widens what a leaked credential can do.
pub const NEVER: &[&str] = &["ftp", "telnet", "ssh", "winbox", "web", "sniff", "romon", "local", "rest-api"];
pub const READER: &[&str] = &["read", "api", "test"];
pub const WRITER: &[&str] = &["read", "write", "api", "test"];

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Policies {
    pub user: String,
    pub group: String,
    pub policy: Vec<String>,
}

impl Policies {
    pub fn has(&self, p: &str) -> bool {
        self.policy.iter().any(|x| x == p)
    }
    /// Policies that break read-only mode, if any.
    pub fn read_only_violations(&self) -> Vec<String> {
        FORBIDDEN_READ_ONLY.iter().filter(|p| self.has(p)).map(|p| p.to_string()).collect()
    }
    /// Policies the user has but tikontrol never needs.
    pub fn extra(&self) -> Vec<String> {
        self.policy.iter().filter(|p| NEVER.contains(&p.as_str())).cloned().collect()
    }
    pub fn can_read(&self) -> bool {
        self.has("read") && self.has("api")
    }
}

/// Parses a policy field: "read,api,test" or the form with negations ("read,!write,api").
pub fn parse_policy(field: &str) -> Vec<String> {
    let mut v: Vec<String> = field.split(',').map(str::trim).filter(|p| !p.is_empty() && !p.starts_with('!')).map(String::from).collect();
    v.sort();
    v.dedup();
    v
}

pub fn detect(r: &mut dyn Router, user: &str) -> Result<Policies, String> {
    let users = r.run(&Cmd::new("/user/print").proplist(&["name", "group"]).query("name", user))?;
    let group = users.first().and_then(|u| u.get("group")).cloned().ok_or("cannot read this user's group: it needs the read policy")?;
    // the built-in groups are a mistake here: `read` includes sensitive and reboot
    let groups = r.run(&Cmd::new("/user/group/print").proplist(&["name", "policy"]).query("name", &group))?;
    let policy = groups.first().and_then(|g| g.get("policy")).map(|p| parse_policy(p)).ok_or("cannot read the user's group policies")?;
    Ok(Policies { user: user.into(), group, policy })
}

/// RouterOS commands the user runs once (WinBox terminal or SSH) to create least-privilege users.
/// Passwords are left for the user to type; tikontrol never puts a secret in a command.
pub fn setup_commands(tik_ip: &str, write: bool) -> Vec<String> {
    let addr = if tik_ip.is_empty() { "<tikontrol-ip>/32".to_string() } else { format!("{tik_ip}/32") };
    let addr = cli_quote(&addr);
    let mut v = vec![
        "# 1. A certificate for API-SSL (skip if api-ssl already has one)".into(),
        // self-signed: RouterOS 7.26 refuses to sign without key-cert-sign ("CA not found")
        "/certificate/add name=tikontrol-api common-name=router key-usage=tls-server,key-cert-sign days-valid=3650".into(),
        "/certificate/sign tikontrol-api".into(),
        format!("/ip/service/set api-ssl certificate=tikontrol-api tls-version=only-1.2 address={addr} disabled=no"),
        "# 2. Turn off the plain-text API".into(),
        "/ip/service/set api disabled=yes".into(),
        "# 3. A read-only group and user: no write, sensitive, password, policy or reboot".into(),
        format!("/user/group/add name=tikontrol-read policy={}", READER.join(",")),
        format!("/user/add name=tikontrol-read group=tikontrol-read address={addr} password=<choose-a-long-password>"),
    ];
    if write {
        v.push("# 4. Read-and-write only: a separate writer. It can change the whole config, treat it like admin".into());
        v.push(format!("/user/group/add name=tikontrol-write policy={}", WRITER.join(",")));
        v.push(format!("/user/add name=tikontrol-write group=tikontrol-write address={addr} password=<another-long-password>"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ros::queries::tests::{row, Fake};

    #[test]
    fn parses_policy_fields() {
        assert_eq!(parse_policy("read,api,test"), ["api", "read", "test"]);
        assert_eq!(parse_policy("read,!write,api,!sensitive"), ["api", "read"]);
        assert_eq!(parse_policy(""), Vec::<String>::new());
    }

    #[test]
    fn detects_a_safe_reader() {
        let mut f = Fake::default();
        f.answers.insert("/user/print".into(), Ok(vec![row(&[("name", "tikontrol-read"), ("group", "tikontrol-read")])]));
        f.answers.insert("/user/group/print".into(), Ok(vec![row(&[("name", "tikontrol-read"), ("policy", "read,api,test,!write")])]));
        let p = detect(&mut f, "tikontrol-read").unwrap();
        assert!(p.read_only_violations().is_empty());
        assert!(p.can_read());
        assert_eq!(f.seen[0], ["/user/print", "=.proplist=name,group", "?name=tikontrol-read"]);
    }

    #[test]
    fn the_builtin_read_group_is_not_read_only() {
        let p = Policies { user: "u".into(), group: "read".into(), policy: parse_policy("local,telnet,ssh,reboot,read,test,winbox,password,web,sniff,sensitive,api,romon,rest-api") };
        assert_eq!(p.read_only_violations(), ["sensitive", "password", "reboot"]);
        assert!(p.extra().contains(&"telnet".to_string()));
    }

    #[test]
    fn missing_user_is_an_error() {
        let mut f = Fake::default();
        assert!(detect(&mut f, "x").is_err());
    }

    #[test]
    fn setup_commands_hold_no_secrets_and_quote_the_address() {
        let c = setup_commands("192.0.2.5", true).join("\n");
        assert!(c.contains("policy=read,api,test\n"));
        assert!(c.contains("policy=read,write,api,test"));
        assert!(c.contains("address=192.0.2.5/32"));
        assert!(!c.lines().filter(|l| l.contains("policy=")).any(|l| l.contains("sensitive")));
        assert!(setup_commands("", false).join("\n").contains("\"<tikontrol-ip>/32\""));
        assert!(!setup_commands("", false).join("\n").contains("tikontrol-write"));
    }
}
