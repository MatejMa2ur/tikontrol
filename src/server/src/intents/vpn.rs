//! VPN (docs/FEATURES.md "VPN"): WireGuard server, peers, client to a provider (with policy routing),
//! site-to-site, Back To Home, L2TP/IPsec and OpenVPN servers, and VPN users (`/ppp/secret`). Adding a
//! WireGuard peer is the core `wireguard-peer` intent in apply.rs; this area adds removing and pausing peers.
//!
//! Everything created is tagged `comment=tik:<kind>:<name>` and the inverse removes exactly that, in
//! reverse order. Secrets (a provider's private key, the L2TP IPsec secret, a VPN user's password) are
//! sent to the router once: they are masked in every CLI text (`apply::SECRET_KEYS`), never echoed in
//! errors, and never part of an inverse. tikontrol never reads them back, so an inverse that would need
//! an old secret restores only the non-secret fields, and removing a VPN user can't be reverted.
//!
//! OpenVPN certificates (`ovpn-certificates`): a CA and a server certificate are created and signed on
//! the router, so their private keys never leave it; only the CA's public certificate is exported (no
//! passphrase, so RouterOS writes no key) for the client profile the VPN page builds. The plan is
//! revertible: nothing else can use the new certificates before the change is kept, so the inverse
//! removes the export and both certificates by their tik: tags (refused if those names or tags exist).

use crate::apply::{cidr, dns_name, ipv4, plan, ros_name, wg_key, yn, Op, Plan, Risk};
use crate::ros::cmd::Cmd;
use crate::ros::conn::{Router, Row};
use serde::Deserialize;
use serde_json::Value;
use std::net::Ipv4Addr;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum V {
    /// Interface + router address + UDP accept on input (+ LAN list membership).
    WireguardServer {
        name: String,
        listen_port: u16,
        address: String,
        #[serde(default = "yes")]
        lan: bool,
    },
    RemoveWireguardPeer { public_key: String },
    WireguardPeerEnabled { public_key: String, enabled: bool },
    BackToHome { enabled: bool },
    /// `server`: "l2tp" or "ovpn"; `name` picks one OpenVPN server on RouterOS versions that have several.
    /// For L2TP, enabling also sets `use-ipsec=required`; `ipsec_secret` (the pre-shared key) is needed
    /// unless IPsec is already required, and is only sent when enabling.
    VpnServer {
        server: String,
        enabled: bool,
        #[serde(default)]
        name: String,
        #[serde(default)]
        ipsec_secret: String,
    },
    /// A `/ppp/secret` for L2TP or OpenVPN logins, tagged `tik:vpn-user:<name>`.
    VpnUser {
        name: String,
        password: String,
        service: String,
        #[serde(default = "default_profile")]
        profile: String,
    },
    RemoveVpnUser { name: String },
    /// Tunnel to a provider; `sources` (LAN subnets or device /32s) are routed through it.
    WireguardClient {
        name: String,
        #[serde(default)]
        private_key: String,
        address: String,
        public_key: String,
        endpoint: String,
        endpoint_port: u16,
        #[serde(default)]
        sources: Vec<String>,
    },
    SiteToSite {
        name: String,
        listen_port: u16,
        address: String,
        public_key: String,
        #[serde(default)]
        endpoint: String,
        #[serde(default)]
        endpoint_port: u16,
        remote_subnets: Vec<String>,
    },
    /// A CA (`tik-ovpn-ca`) and a server certificate (`tik-ovpn-server`) for the OpenVPN server, signed
    /// on the router, plus an export of the CA's public certificate for client profiles.
    OvpnCertificates {
        common_name: String,
        #[serde(default)]
        ca_common_name: String,
        #[serde(default = "rsa_bits")]
        key_size: u16,
        #[serde(default = "ten_years")]
        days_valid: u16,
    },
    /// Points an OpenVPN server at `tik-ovpn-server`; usually batched after `ovpn-certificates`.
    OvpnServerCertificate {
        #[serde(default)]
        name: String,
    },
    /// Writes the CA's public certificate to `cert_export_tik-ovpn-ca.crt` again (it was deleted).
    ExportOvpnCa {},
}

fn yes() -> bool {
    true
}

fn rsa_bits() -> u16 {
    2048
}

fn ten_years() -> u16 {
    3650
}

fn default_profile() -> String {
    "default-encryption".into()
}

const KINDS: &[&str] = &["wireguard-server", "remove-wireguard-peer", "wireguard-peer-enabled", "back-to-home", "vpn-server", "wireguard-client", "site-to-site", "vpn-user", "remove-vpn-user", "ovpn-certificates", "ovpn-server-certificate", "export-ovpn-ca"];

pub fn compile(v: &Value, r: &mut dyn Router) -> Option<Result<Plan, String>> {
    let kind = v.get("kind")?.as_str()?;
    if !KINDS.contains(&kind) {
        return None;
    }
    Some(serde_json::from_value::<V>(v.clone()).map_err(|e| format!("invalid {kind} intent: {e}")).and_then(|x| build(x, r)))
}

const WG: &str = "/interface/wireguard";
const PEERS: &str = "/interface/wireguard/peers";
const ADDR: &str = "/ip/address";
const FILTER: &str = "/ip/firewall/filter";
const PRIVATE: [&str; 3] = ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16"];

fn ifname(s: &str) -> Result<String, String> {
    let s = ros_name(s)?;
    if s.contains(' ') || s == "back-to-home-vpn" {
        return Err("invalid interface name".into());
    }
    Ok(s)
}

fn port(p: u16) -> Result<String, String> {
    if p == 0 { Err("ports must be 1-65535".into()) } else { Ok(p.to_string()) }
}

fn host(s: &str) -> Result<String, String> {
    ipv4(s).map(|a| a.to_string()).or_else(|_| dns_name(s)).map_err(|_| "endpoint must be an IPv4 address or a host name".into())
}

fn subnets(v: &[String], what: &str) -> Result<Vec<String>, String> {
    if v.len() > 16 {
        return Err(format!("at most 16 {what}"));
    }
    v.iter().map(|s| cidr(s).and_then(|c| if c == "0.0.0.0/0" { Err(format!("0.0.0.0/0 is not allowed as {what}: name the subnets or devices")) } else { Ok(network(&c)) })).collect()
}

/// "10.9.0.1/30" → "10.9.0.0/30"
pub fn network(c: &str) -> String {
    let (a, n) = c.split_once('/').unwrap_or((c, "32"));
    let (a, n): (Ipv4Addr, u32) = (a.parse().unwrap_or(Ipv4Addr::UNSPECIFIED), n.parse().unwrap_or(32));
    let mask = if n == 0 { 0 } else { u32::MAX << (32 - n) };
    format!("{}/{n}", Ipv4Addr::from(u32::from(a) & mask))
}

fn rm(path: &'static str, c: &str) -> Op {
    Op::on(path, "remove", "comment", c, vec![])
}

/// UDP accept on input, placed before the first input rule so the default drop can't shadow it.
fn accept_udp(r: &mut dyn Router, port: &str, c: &str) -> Result<Op, String> {
    let first = r.run(&Cmd::new("/ip/firewall/filter/print").proplist(&[".id"]).query("chain", "input"))?;
    let mut args = vec![("chain", "input".into()), ("protocol", "udp".into()), ("dst-port", port.to_string()), ("action", "accept".into()), ("comment", c.to_string())];
    if let Some(id) = first.first().and_then(|x| x.get(".id")) {
        args.push(("place-before", id.clone()));
    }
    Ok(Op::add(FILTER, args))
}

const SECRETS: &str = "/ppp/secret";
const NO_CERT: &str = "the OpenVPN server has no certificate, so it can't accept connections: create the certificates first (VPN, OpenVPN server, Create certificates)";

const CERTS: &str = "/certificate";
pub const OVPN_CA: &str = "tik-ovpn-ca";
pub const OVPN_SERVER_CERT: &str = "tik-ovpn-server";
/// What `/certificate/export-certificate tik-ovpn-ca file-name=cert_export_tik-ovpn-ca` writes (PEM, `.crt`
/// added by RouterOS). Without `export-passphrase` only the public certificate is written, never the key.
pub const OVPN_CA_FILE: &str = "cert_export_tik-ovpn-ca.crt";

/// Certificate common names: 1-64 letters, digits, `.` and `-`, starting and ending with a letter or digit
/// (a host name, an IPv4 address or a plain label).
fn common_name(s: &str) -> Result<String, String> {
    let s = s.trim();
    let b = s.as_bytes();
    let ok = (1..=64).contains(&b.len()) && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'.' || *c == b'-') && b[0].is_ascii_alphanumeric() && b[b.len() - 1].is_ascii_alphanumeric();
    ok.then(|| s.to_string()).ok_or_else(|| "invalid common name (1-64 letters, digits, . and -)".into())
}

/// The CA export: an old file of ours is removed first (export-certificate won't overwrite it), then the
/// public certificate is written in PEM. Never `export-passphrase`: with one, RouterOS also writes the key.
fn export_ca_ops() -> Vec<Op> {
    vec![
        Op::on("/file", "remove", "name", OVPN_CA_FILE, vec![]),
        Op::on(CERTS, "export-certificate", "name", OVPN_CA, vec![("type", "pem".into()), ("file-name", OVPN_CA_FILE.trim_end_matches(".crt").into())]),
    ]
}

/// VPN user names: 1-32 of [A-Za-z0-9._-], starting with a letter or digit.
fn vpn_user(s: &str) -> Result<String, String> {
    let ok = (1..=32).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) && s.as_bytes()[0].is_ascii_alphanumeric();
    ok.then(|| s.to_string()).ok_or_else(|| "invalid VPN user name (1-32 letters, digits, . _ -, starting with a letter or digit)".into())
}

/// Passwords and pre-shared secrets: 12-64 printable ASCII characters, no spaces. The value is never
/// part of the error, so it can't reach a log or the browser.
fn vpn_secret(s: &str, what: &str) -> Result<String, String> {
    let ok = (12..=64).contains(&s.len()) && s.bytes().all(|b| (0x21..0x7f).contains(&b));
    ok.then(|| s.to_string()).ok_or_else(|| format!("{what} must be 12-64 printable ASCII characters without spaces"))
}

fn is_true(v: &str) -> bool {
    v == "true" || v == "yes"
}

const PEER_KEYS: [&str; 8] = ["interface", "name", "allowed-address", "endpoint-address", "endpoint-port", "persistent-keepalive", "comment", "disabled"];

fn peer(r: &mut dyn Router, key: &str) -> Result<Row, String> {
    let rows = r.run(&Cmd::new("/interface/wireguard/peers/print").proplist(&PEER_KEYS).query("public-key", key))?;
    let p = rows.into_iter().next().ok_or("no WireGuard peer with that public key")?;
    if p.get("interface").is_some_and(|i| i == "back-to-home-vpn") {
        return Err("Back To Home peers are managed by /ip/cloud, not here".into());
    }
    Ok(p)
}

fn build(x: V, r: &mut dyn Router) -> Result<Plan, String> {
    match x {
        V::WireguardServer { name, listen_port, address, lan } => {
            let (n, lp, a) = (ifname(&name)?, port(listen_port)?, cidr(&address)?);
            let c = format!("tik:wgsrv:{n}");
            let mut ops = vec![Op::add(WG, vec![("name", n.clone()), ("listen-port", lp.clone()), ("comment", c.clone())]), Op::add(ADDR, vec![("address", a.clone()), ("interface", n.clone()), ("comment", c.clone())])];
            if lan {
                ops.push(Op::add("/interface/list/member", vec![("list", "LAN".into()), ("interface", n.clone()), ("comment", c.clone())]));
            }
            ops.push(accept_udp(r, &lp, &c)?);
            let undo = vec![rm(FILTER, &c), rm("/interface/list/member", &c), rm(ADDR, &c), rm(WG, &c)];
            let mut p = plan(format!("WireGuard server {n} on UDP {lp} ({a})"), Risk::High, ops, undo);
            p.warnings.push(format!("High risk: changes the input firewall. UDP {lp} becomes reachable from the internet; WireGuard answers only peers with a known key."));
            if lan {
                p.warnings.push("The tunnel joins the LAN interface list: VPN devices get the same access as LAN devices, including the router's management.".into());
            }
            Ok(p)
        }
        V::RemoveWireguardPeer { public_key } => {
            let key = wg_key(&public_key)?;
            let old = peer(r, &key)?;
            let mut back: Vec<(&'static str, String)> = PEER_KEYS.iter().filter_map(|k| old.get(*k).filter(|v| !v.is_empty()).map(|v| (*k, if *k == "disabled" { yn(is_true(v)) } else { v.clone() }))).collect();
            back.push(("public-key", key.clone()));
            let who = old.get("name").cloned().unwrap_or_else(|| key[..8].to_string());
            let mut p = plan(format!("Remove WireGuard peer {who}"), Risk::Medium, vec![Op::on(PEERS, "remove", "public-key", &key, vec![])], vec![Op::add(PEERS, back)]);
            p.warnings.push("The device can't connect any more. If it used a preshared key, an automatic revert restores the peer without it.".into());
            Ok(p)
        }
        V::WireguardPeerEnabled { public_key, enabled } => {
            let key = wg_key(&public_key)?;
            let old = peer(r, &key)?;
            let was = old.get("disabled").is_some_and(|v| is_true(v));
            let who = old.get("name").cloned().unwrap_or_else(|| key[..8].to_string());
            let set = |d: bool| Op::on(PEERS, "set", "public-key", &key, vec![("disabled", yn(d))]);
            Ok(plan(format!("{} WireGuard peer {who}", if enabled { "Enable" } else { "Disable" }), Risk::Low, vec![set(!enabled)], vec![set(was)]))
        }
        V::BackToHome { enabled } => {
            // explicit proplist: /ip/cloud also holds the Back To Home private key
            let rows = r.run(&Cmd::new("/ip/cloud/print").proplist(&["back-to-home-vpn"]))?;
            let was = rows.first().and_then(|x| x.get("back-to-home-vpn")).filter(|v| !v.is_empty()).cloned().ok_or("Back To Home isn't available on this router (RouterOS 7.12+)")?;
            let val = |on: bool| if on { "enabled" } else { "revoked-and-disabled" }.to_string();
            let set = |v: String| Op { path: "/ip/cloud", verb: "set", find: None, args: vec![("back-to-home-vpn", v)] };
            let mut p = plan(format!("{} Back To Home", if enabled { "Enable" } else { "Disable" }), Risk::Medium, vec![set(val(enabled))], vec![set(if was == "enabled" { was } else { val(false) })]);
            if enabled {
                p.warnings.push("Back To Home relays through MikroTik's servers when a direct connection isn't possible. Devices are added in the MikroTik app.".into());
            } else {
                p.warnings.push("Disabling revokes the Back To Home keys: every device must be added again, even if this change is reverted.".into());
            }
            Ok(p)
        }
        V::VpnServer { server, enabled, name, ipsec_secret } => match server.as_str() {
            "l2tp" => {
                let path = "/interface/l2tp-server/server";
                // never ipsec-secret: tikontrol doesn't read secrets back
                let rows = r.run(&Cmd::new("/interface/l2tp-server/server/print").proplist(&["enabled", "use-ipsec"]))?;
                let cur = rows.first().ok_or("no L2TP server on this router")?;
                let was = cur.get("enabled").is_some_and(|v| is_true(v));
                let was_ipsec = cur.get("use-ipsec").filter(|v| !v.is_empty()).cloned();
                let secret = if ipsec_secret.is_empty() { None } else { Some(vpn_secret(&ipsec_secret, "the IPsec secret")?) };
                let required = was_ipsec.as_deref() == Some("required");
                if !enabled && secret.is_some() {
                    return Err("the IPsec secret is only sent when enabling L2TP".into());
                }
                if enabled && secret.is_none() && !required {
                    return Err("L2TP needs an IPsec pre-shared secret (12-64 characters): without IPsec it is unencrypted".into());
                }
                let mut args = vec![("enabled", yn(enabled))];
                let mut back = vec![("enabled", yn(was))];
                if enabled {
                    args.push(("use-ipsec", "required".into()));
                    if let Some(old) = was_ipsec {
                        back.push(("use-ipsec", old));
                    }
                }
                let has_secret = secret.is_some();
                if let Some(s) = secret {
                    args.push(("ipsec-secret", s));
                }
                let set = |args| Op { path, verb: "set", find: None, args };
                let mut p = plan(format!("{} the L2TP/IPsec server", if enabled { "Enable" } else { "Disable" }), Risk::Medium, vec![set(args)], vec![set(back)]);
                if enabled {
                    p.warnings.push("Remote devices can try to log in with VPN users: allow UDP 500, 4500 and 1701 on input only if you need it.".into());
                    if has_secret {
                        p.warnings.push("The IPsec secret is sent to the router once; tikontrol doesn't store it and can't show it again. Give it to each device yourself.".into());
                        if required {
                            p.warnings.push("This replaces the current IPsec secret. An automatic revert can't bring the old one back (tikontrol never reads secrets): devices must use the new one.".into());
                        }
                    }
                }
                Ok(p)
            }
            "ovpn" => {
                let path = "/interface/ovpn-server/server";
                let rows = r.run(&Cmd::new("/interface/ovpn-server/server/print").proplist(&["name", "disabled", "enabled", "certificate"]))?;
                let named = rows.iter().any(|x| x.contains_key("name"));
                let no_cert = |x: &Row| x.get("certificate").is_some_and(|c| c.is_empty() || c == "none");
                let (ops, undo, label) = if named {
                    // 7.17+: several servers, each with disabled=
                    let n = ifname(&name).map_err(|_| "pick the OpenVPN server by name")?;
                    let cur = rows.iter().find(|x| x.get("name") == Some(&n)).ok_or("no OpenVPN server with that name")?;
                    if enabled && no_cert(cur) {
                        return Err(NO_CERT.into());
                    }
                    let was_on = !cur.get("disabled").is_some_and(|v| is_true(v));
                    let set = |on: bool| Op::on(path, "set", "name", &n, vec![("disabled", yn(!on))]);
                    (vec![set(enabled)], vec![set(was_on)], format!("OpenVPN server {n}"))
                } else {
                    let cur = rows.first().ok_or("no OpenVPN server on this router: create one with a certificate in RouterOS first")?;
                    if enabled && no_cert(cur) {
                        return Err(NO_CERT.into());
                    }
                    let was = cur.get("enabled").is_some_and(|v| is_true(v));
                    let set = |on: bool| Op { path, verb: "set", find: None, args: vec![("enabled", yn(on))] };
                    (vec![set(enabled)], vec![set(was)], "the OpenVPN server".to_string())
                };
                let mut p = plan(format!("{} {label}", if enabled { "Enable" } else { "Disable" }), Risk::Medium, ops, undo);
                if enabled {
                    p.warnings.push("The OpenVPN port must be allowed on input to be reachable; remote devices can then try PPP logins.".into());
                }
                Ok(p)
            }
            _ => Err("server must be l2tp or ovpn".into()),
        },
        V::VpnUser { name, password, service, profile } => {
            let n = vpn_user(&name)?;
            let pw = vpn_secret(&password, "the password")?;
            if pw.to_ascii_lowercase().contains(&n.to_ascii_lowercase()) {
                return Err("the password must not contain the user name".into());
            }
            let svc = match service.as_str() {
                "l2tp" | "ovpn" => service.clone(),
                _ => return Err("service must be l2tp or ovpn".into()),
            };
            let prof = ros_name(&profile)?;
            if r.run(&Cmd::new("/ppp/profile/print").proplist(&["name"]).query("name", &prof))?.is_empty() {
                return Err(format!("the router has no PPP profile {prof:?}"));
            }
            if !r.run(&Cmd::new("/ppp/secret/print").proplist(&["name"]).query("name", &n))?.is_empty() {
                return Err(format!("a VPN user {n:?} already exists"));
            }
            let c = format!("tik:vpn-user:{n}");
            let op = Op::add(SECRETS, vec![("name", n.clone()), ("password", pw), ("service", svc.clone()), ("profile", prof.clone()), ("comment", c.clone())]);
            let mut p = plan(format!("Add {} VPN user {n}", if svc == "l2tp" { "L2TP" } else { "OpenVPN" }), Risk::Medium, vec![op], vec![rm(SECRETS, &c)]);
            p.warnings.push("The password is sent to the router once; tikontrol doesn't store it and can't show it again.".into());
            if prof == "default" {
                p.warnings.push("The \"default\" profile doesn't require encryption: prefer \"default-encryption\".".into());
            }
            Ok(p)
        }
        V::RemoveVpnUser { name } => {
            let n = vpn_user(&name)?;
            let c = format!("tik:vpn-user:{n}");
            if r.run(&Cmd::new("/ppp/secret/print").proplist(&["name"]).query("comment", &c))?.is_empty() {
                return Err("only VPN users created by tikontrol can be removed here".into());
            }
            Ok(Plan {
                title: format!("Remove VPN user {n}"),
                risk: Risk::Medium,
                warnings: vec!["The user is disconnected and can't log in any more. This can't be undone automatically: tikontrol never reads passwords, add the user again to bring it back.".into()],
                // the secret first, so the kicked session can't log in again
                ops: vec![rm(SECRETS, &c), Op::on("/ppp/active", "remove", "name", &n, vec![])],
                undo: vec![],
                revertible: false,
            })
        }
        V::WireguardClient { name, private_key, address, public_key, endpoint, endpoint_port, sources } => {
            let (n, a, key, ep, epp) = (ifname(&name)?, cidr(&address)?, wg_key(&public_key)?, host(&endpoint)?, port(endpoint_port)?);
            let priv_key = if private_key.is_empty() { None } else { Some(wg_key(&private_key).map_err(|_| "invalid WireGuard private key")?) };
            let src = subnets(&sources, "sources")?;
            let c = format!("tik:vpnc:{n}");
            let table = format!("tik-vpn-{n}");
            let mut wg = vec![("name", n.clone())];
            if let Some(k) = &priv_key {
                wg.push(("private-key", k.clone()));
            }
            wg.push(("comment", c.clone()));
            let mut ops = vec![
                Op::add(WG, wg),
                Op::add(ADDR, vec![("address", a.clone()), ("interface", n.clone()), ("comment", c.clone())]),
                Op::add(PEERS, vec![("interface", n.clone()), ("public-key", key), ("endpoint-address", ep.clone()), ("endpoint-port", epp.clone()), ("allowed-address", "0.0.0.0/0".into()), ("persistent-keepalive", "25s".into()), ("comment", c.clone())]),
                Op::add("/ip/firewall/nat", vec![("chain", "srcnat".into()), ("out-interface", n.clone()), ("action", "masquerade".into()), ("comment", c.clone())]),
            ];
            if !src.is_empty() {
                ops.push(Op::add("/routing/table", vec![("name", table.clone()), ("fib", String::new()), ("comment", c.clone())]));
                ops.push(Op::add("/ip/route", vec![("dst-address", "0.0.0.0/0".into()), ("gateway", n.clone()), ("routing-table", table.clone()), ("comment", c.clone())]));
                // local and LAN-to-LAN traffic keeps using the main table
                for d in PRIVATE {
                    ops.push(Op::add("/routing/rule", vec![("dst-address", d.into()), ("action", "lookup".into()), ("table", "main".into()), ("comment", c.clone())]));
                }
                for s in &src {
                    ops.push(Op::add("/routing/rule", vec![("src-address", s.clone()), ("action", "lookup".into()), ("table", table.clone()), ("comment", c.clone())]));
                }
            }
            let undo = vec![rm("/routing/rule", &c), rm("/ip/route", &c), rm("/routing/table", &c), rm("/ip/firewall/nat", &c), rm(PEERS, &c), rm(ADDR, &c), rm(WG, &c)];
            let mut p = plan(format!("VPN client {n} to {ep}:{epp}"), Risk::Medium, ops, undo);
            if src.is_empty() {
                p.warnings.push("No devices are routed through the tunnel yet: add sources to send their traffic through it.".into());
            } else {
                p.warnings.push(format!("Internet traffic from {} goes through the VPN. If the tunnel is down it falls back to your normal connection (no kill switch), and DNS still uses the router.", src.join(", ")));
            }
            if priv_key.is_none() {
                p.warnings.push("The router generates its own key: register its public key (shown after the change) with the provider.".into());
            } else {
                p.warnings.push("The provider's private key is sent to the router once; tikontrol doesn't store it and can't read it back.".into());
            }
            Ok(p)
        }
        V::SiteToSite { name, listen_port, address, public_key, endpoint, endpoint_port, remote_subnets } => {
            let (n, lp, a, key) = (ifname(&name)?, port(listen_port)?, cidr(&address)?, wg_key(&public_key)?);
            let remote = subnets(&remote_subnets, "remote subnets")?;
            if remote.is_empty() {
                return Err("name at least one remote subnet".into());
            }
            let c = format!("tik:s2s:{n}");
            let mut allowed = vec![network(&a)];
            allowed.extend(remote.iter().cloned());
            let mut peer = vec![("interface", n.clone()), ("public-key", key), ("allowed-address", allowed.join(","))];
            if !endpoint.is_empty() {
                peer.extend([("endpoint-address", host(&endpoint)?), ("endpoint-port", port(endpoint_port)?), ("persistent-keepalive", "25s".into())]);
            }
            peer.push(("comment", c.clone()));
            let mut ops = vec![Op::add(WG, vec![("name", n.clone()), ("listen-port", lp.clone()), ("comment", c.clone())]), Op::add(ADDR, vec![("address", a.clone()), ("interface", n.clone()), ("comment", c.clone())]), Op::add(PEERS, peer)];
            for s in &remote {
                ops.push(Op::add("/ip/route", vec![("dst-address", s.clone()), ("gateway", n.clone()), ("comment", c.clone())]));
            }
            ops.push(accept_udp(r, &lp, &c)?);
            let undo = vec![rm(FILTER, &c), rm("/ip/route", &c), rm(PEERS, &c), rm(ADDR, &c), rm(WG, &c)];
            let mut p = plan(format!("Site-to-site {n} to {}", remote.join(", ")), Risk::High, ops, undo);
            p.warnings.push(format!("High risk: changes the input firewall (UDP {lp} open to the internet) and adds routes. A remote subnet that overlaps your LAN makes it unreachable until it reverts."));
            p.warnings.push("The other side needs this router's public key (shown after the change) and the same tunnel subnet.".into());
            Ok(p)
        }
        V::OvpnCertificates { common_name: cn, ca_common_name, key_size, days_valid } => {
            let cn = common_name(&cn)?;
            let ca_cn = if ca_common_name.trim().is_empty() { "tikontrol-ovpn-ca".to_string() } else { common_name(&ca_common_name)? };
            if !matches!(key_size, 2048 | 4096) {
                return Err("key size must be 2048 or 4096".into());
            }
            if !(30..=3650).contains(&days_valid) {
                return Err("days valid must be 30-3650".into());
            }
            // the undo removes by name, so a certificate that already has one of our names (made by hand or
            // left from an earlier run) must not exist: it would be deleted on revert
            let have = r.run(&Cmd::new("/certificate/print").proplist(&["name"]))?;
            if have.iter().any(|c| c.get("name").is_some_and(|v| [OVPN_CA, OVPN_SERVER_CERT].contains(&v.as_str()))) {
                return Err(format!("certificates {OVPN_CA} / {OVPN_SERVER_CERT} already exist on this router: use them, or remove them in RouterOS (/certificate) first"));
            }
            let (bits, days) = (key_size.to_string(), days_valid.to_string());
            // /certificate has no comment (RouterOS 7.25 refuses one): ours are found by their tik- names
            let cert = |name: &str, cn: &str, usage: &str| Op::add(CERTS, vec![("name", name.into()), ("common-name", cn.into()), ("key-size", bits.clone()), ("days-valid", days.clone()), ("key-usage", usage.into())]);
            let mut ops = vec![
                cert(OVPN_CA, &ca_cn, "key-cert-sign,crl-sign"),
                Op::on(CERTS, "sign", "name", OVPN_CA, vec![]),
                cert(OVPN_SERVER_CERT, &cn, "digital-signature,key-encipherment,tls-server"),
                Op::on(CERTS, "sign", "name", OVPN_SERVER_CERT, vec![("ca", OVPN_CA.into())]),
            ];
            ops.extend(export_ca_ops());
            // the server certificate goes before its CA
            let undo = vec![Op::on("/file", "remove", "name", OVPN_CA_FILE, vec![]), Op::on(CERTS, "remove", "name", OVPN_SERVER_CERT, vec![]), Op::on(CERTS, "remove", "name", OVPN_CA, vec![])];
            let mut p = plan(format!("OpenVPN certificates for {cn}"), Risk::Medium, ops, undo);
            p.warnings.push(format!("Creates a private certificate authority ({OVPN_CA}) and a server certificate ({OVPN_SERVER_CERT}, valid {days_valid} days) on the router. Their private keys stay on the router; only the CA's public certificate is written to {OVPN_CA_FILE} for client profiles."));
            p.warnings.push(if key_size > 2048 { "Signing 4096-bit keys can take minutes on small routers: keep this page open until it finishes, and keep the change soon after.".to_string() } else { "Signing takes a few seconds per certificate, longer on small routers.".to_string() });
            p.warnings.push("If the change isn't kept, the automatic revert deletes both certificates and their keys: client profiles downloaded in the meantime stop working, and new certificates must be created again (signing can't be undone in any other way).".into());
            Ok(p)
        }
        V::OvpnServerCertificate { name } => {
            let path = "/interface/ovpn-server/server";
            let rows = r.run(&Cmd::new("/interface/ovpn-server/server/print").proplist(&["name", "certificate", "require-client-certificate"]))?;
            let named = rows.iter().any(|x| x.contains_key("name"));
            let (cur, find) = if named {
                let n = ifname(&name).map_err(|_| "pick the OpenVPN server by name")?;
                (rows.iter().find(|x| x.get("name") == Some(&n)).ok_or("no OpenVPN server with that name")?, Some(n))
            } else {
                (rows.first().ok_or("no OpenVPN server on this router")?, None)
            };
            let old = cur.get("certificate").filter(|c| !c.is_empty()).cloned().unwrap_or_else(|| "none".into());
            if old == OVPN_SERVER_CERT {
                return Err(format!("the OpenVPN server already uses {OVPN_SERVER_CERT}"));
            }
            let needs_client_cert = cur.get("require-client-certificate").is_some_and(|v| is_true(v));
            let set = |c: String| match &find {
                Some(n) => Op::on(path, "set", "name", n, vec![("certificate", c)]),
                None => Op { path, verb: "set", find: None, args: vec![("certificate", c)] },
            };
            let label = find.as_ref().map_or("the OpenVPN server".to_string(), |n| format!("OpenVPN server {n}"));
            let mut p = plan(format!("Use {OVPN_SERVER_CERT} on {label}"), Risk::Medium, vec![set(OVPN_SERVER_CERT.into())], vec![set(old.clone())]);
            if old != "none" {
                p.warnings.push(format!("Replaces the certificate {old}: devices that trust only its CA need a new profile."));
            }
            p.warnings.push("Users connected over OpenVPN may be disconnected while the server switches certificates.".into());
            if needs_client_cert {
                p.warnings.push("The server requires client certificates, but tikontrol's client profile logs in with a VPN user only: set require-client-certificate=no in RouterOS, or devices can't connect.".into());
            }
            Ok(p)
        }
        V::ExportOvpnCa {} => {
            let have = r.run(&Cmd::new("/certificate/print").proplist(&["name"]).query("name", OVPN_CA))?;
            if have.is_empty() {
                return Err(format!("there is no {OVPN_CA} certificate created by tikontrol on this router"));
            }
            let mut p = plan(format!("Export the public certificate of {OVPN_CA}"), Risk::Low, export_ca_ops(), vec![Op::on("/file", "remove", "name", OVPN_CA_FILE, vec![])]);
            p.warnings.push(format!("Writes only the public certificate to {OVPN_CA_FILE}, never the private key."));
            Ok(p)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply::{compile_value, revert_script};
    use crate::ros::queries::tests::{row, Fake};
    use serde_json::json;

    const K: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    const K2: &str = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=";

    #[test]
    fn server_is_tagged_and_undone_in_reverse() {
        let mut f = Fake::default();
        f.answers.insert("/ip/firewall/filter/print".into(), Ok(vec![row(&[(".id", "*3")])]));
        let p = compile_value(&json!({"kind": "wireguard-server", "name": "wg-tik", "listen_port": 13231, "address": "10.66.0.1/24"}), &mut f).unwrap();
        assert_eq!(p.risk, Risk::High);
        assert_eq!(p.cli()[3], "/ip/firewall/filter/add chain=input protocol=udp dst-port=13231 action=accept comment=tik:wgsrv:wg-tik place-before=*3");
        assert_eq!(p.undo_cli().last().unwrap(), r#"/interface/wireguard/remove [find comment="tik:wgsrv:wg-tik"]"#);
    }

    #[test]
    fn removing_a_peer_restores_it() {
        let mut f = Fake::default();
        f.answers.insert("/interface/wireguard/peers/print".into(), Ok(vec![row(&[("interface", "wg-tik"), ("name", "Phone"), ("allowed-address", "10.66.0.10/32"), ("disabled", "false")])]));
        let p = compile_value(&json!({"kind": "remove-wireguard-peer", "public_key": K}), &mut f).unwrap();
        assert_eq!(p.cli(), [format!(r#"/interface/wireguard/peers/remove [find public-key="{K}"]"#)]);
        assert_eq!(p.undo_cli(), [format!(r#"/interface/wireguard/peers/add interface=wg-tik name=Phone allowed-address=10.66.0.10/32 disabled=no public-key="{K}""#)]);
        f.answers.insert("/interface/wireguard/peers/print".into(), Ok(vec![row(&[("interface", "back-to-home-vpn")])]));
        assert!(compile_value(&json!({"kind": "remove-wireguard-peer", "public_key": K}), &mut f).is_err());
    }

    #[test]
    fn back_to_home_reads_only_its_field() {
        let mut f = Fake::default();
        f.answers.insert("/ip/cloud/print".into(), Ok(vec![row(&[("back-to-home-vpn", "revoked-and-disabled")])]));
        let p = compile_value(&json!({"kind": "back-to-home", "enabled": true}), &mut f).unwrap();
        assert_eq!(p.cli(), ["/ip/cloud/set back-to-home-vpn=enabled"]);
        assert_eq!(p.undo_cli(), ["/ip/cloud/set back-to-home-vpn=revoked-and-disabled"]);
        assert_eq!(f.seen[0][1], "=.proplist=back-to-home-vpn");
    }

    #[test]
    fn vpn_servers_restore_their_state() {
        let mut f = Fake::default();
        f.answers.insert("/interface/l2tp-server/server/print".into(), Ok(vec![row(&[("enabled", "true"), ("use-ipsec", "required")])]));
        let p = compile_value(&json!({"kind": "vpn-server", "server": "l2tp", "enabled": false}), &mut f).unwrap();
        assert_eq!((p.cli()[0].as_str(), p.undo_cli()[0].as_str(), p.risk), ("/interface/l2tp-server/server/set enabled=no", "/interface/l2tp-server/server/set enabled=yes", Risk::Medium));
        f.answers.insert("/interface/ovpn-server/server/print".into(), Ok(vec![row(&[("name", "ovpn1"), ("disabled", "true"), ("certificate", "srv")])]));
        let p = compile_value(&json!({"kind": "vpn-server", "server": "ovpn", "name": "ovpn1", "enabled": true}), &mut f).unwrap();
        assert_eq!(p.undo_cli(), [r#"/interface/ovpn-server/server/set [find name="ovpn1"] disabled=yes"#]);
        // no certificate: enabling is refused in plain words, disabling still works
        for rows in [vec![row(&[("name", "ovpn1"), ("disabled", "true"), ("certificate", "none")])], vec![row(&[("enabled", "false"), ("certificate", "none")])]] {
            f.answers.insert("/interface/ovpn-server/server/print".into(), Ok(rows));
            let e = compile_value(&json!({"kind": "vpn-server", "server": "ovpn", "name": "ovpn1", "enabled": true}), &mut f).unwrap_err();
            assert!(e.contains("no certificate"), "{e}");
            assert!(compile_value(&json!({"kind": "vpn-server", "server": "ovpn", "name": "ovpn1", "enabled": false}), &mut f).is_ok());
        }
    }

    const PSK: &str = "Psk-not-real-4711";
    const PW: &str = "Pw-not-real-0815";

    fn l2tp(enabled: &str, ipsec: &str) -> Fake {
        let mut f = Fake::default();
        f.answers.insert("/interface/l2tp-server/server/print".into(), Ok(vec![row(&[("enabled", enabled), ("use-ipsec", ipsec)])]));
        f
    }

    #[test]
    fn l2tp_requires_ipsec_and_masks_its_secret() {
        let mut f = l2tp("false", "no");
        let p = compile_value(&json!({"kind": "vpn-server", "server": "l2tp", "enabled": true, "ipsec_secret": PSK}), &mut f).unwrap();
        assert_eq!(p.cli(), [r#"/interface/l2tp-server/server/set enabled=yes use-ipsec=required ipsec-secret="********""#]);
        assert_eq!(p.undo_cli(), ["/interface/l2tp-server/server/set enabled=no use-ipsec=no"]);
        assert!(p.ops[0].args.contains(&("ipsec-secret", PSK.to_string())), "the real secret still goes to the router");
        let script = revert_script("c", &p).unwrap();
        assert!(!script.contains(PSK) && !script.contains("ipsec-secret"));
        assert!(!format!("{:?}{:?}{}", p.warnings, p.undo, p.title).contains(PSK));
        // the secret is never read back
        assert_eq!(f.seen[0][1], "=.proplist=enabled,use-ipsec");
        // without IPsec already required, enabling needs a secret
        let e = compile_value(&json!({"kind": "vpn-server", "server": "l2tp", "enabled": true}), &mut l2tp("false", "yes")).unwrap_err();
        assert!(e.contains("IPsec pre-shared secret"), "{e}");
        // IPsec already required: enabling keeps the router's secret
        let p = compile_value(&json!({"kind": "vpn-server", "server": "l2tp", "enabled": true}), &mut l2tp("false", "required")).unwrap();
        assert_eq!(p.cli(), ["/interface/l2tp-server/server/set enabled=yes use-ipsec=required"]);
        // replacing an existing secret says the old one can't come back
        let p = compile_value(&json!({"kind": "vpn-server", "server": "l2tp", "enabled": true, "ipsec_secret": PSK}), &mut l2tp("true", "required")).unwrap();
        assert!(p.warnings.iter().any(|w| w.contains("can't bring the old one back")));
    }

    #[test]
    fn l2tp_secret_is_validated_without_echoing_it() {
        for bad in ["short-1", "has a space in it", "tab\tinside-secret", "newline\n/system/reboot", "ünïcödé-secret-x", &"x".repeat(65)] {
            let e = compile_value(&json!({"kind": "vpn-server", "server": "l2tp", "enabled": true, "ipsec_secret": bad}), &mut l2tp("false", "no")).unwrap_err();
            assert!(!e.contains(bad), "{e}");
        }
        assert!(compile_value(&json!({"kind": "vpn-server", "server": "l2tp", "enabled": false, "ipsec_secret": PSK}), &mut l2tp("true", "required")).is_err());
    }

    fn ppp(profile: bool, existing: bool) -> Fake {
        let mut f = Fake::default();
        if profile {
            f.answers.insert("/ppp/profile/print".into(), Ok(vec![row(&[("name", "default-encryption")])]));
        }
        if existing {
            f.answers.insert("/ppp/secret/print".into(), Ok(vec![row(&[("name", "alice")])]));
        }
        f
    }

    #[test]
    fn vpn_user_is_tagged_masked_and_removed_by_its_tag() {
        let mut f = ppp(true, false);
        let p = compile_value(&json!({"kind": "vpn-user", "name": "alice", "password": PW, "service": "l2tp"}), &mut f).unwrap();
        assert_eq!(p.cli(), [r#"/ppp/secret/add name=alice password="********" service=l2tp profile=default-encryption comment=tik:vpn-user:alice"#]);
        assert_eq!(p.undo_cli(), [r#"/ppp/secret/remove [find comment="tik:vpn-user:alice"]"#]);
        assert!(p.revertible);
        assert!(p.ops[0].args.contains(&("password", PW.to_string())));
        let script = revert_script("c", &p).unwrap();
        assert!(!script.contains(PW) && !script.contains("password"));
        assert!(!format!("{:?}{:?}{}", p.warnings, p.undo, p.title).contains(PW));
        // never asks for the password
        assert!(f.seen.iter().all(|w| w.iter().all(|x| !x.contains("password"))));
        let p = compile_value(&json!({"kind": "vpn-user", "name": "bob.m-1", "password": PW, "service": "ovpn", "profile": "default-encryption"}), &mut ppp(true, false)).unwrap();
        assert_eq!(p.title, "Add OpenVPN VPN user bob.m-1");
        // an existing name, or a profile the router doesn't have, is refused
        assert!(compile_value(&json!({"kind": "vpn-user", "name": "alice", "password": PW, "service": "l2tp"}), &mut ppp(true, true)).unwrap_err().contains("already exists"));
        assert!(compile_value(&json!({"kind": "vpn-user", "name": "alice", "password": PW, "service": "l2tp"}), &mut ppp(false, false)).unwrap_err().contains("profile"));
    }

    #[test]
    fn removing_a_vpn_user_only_touches_ours_and_is_not_revertible() {
        let mut f = ppp(true, true);
        let p = compile_value(&json!({"kind": "remove-vpn-user", "name": "alice"}), &mut f).unwrap();
        assert_eq!(p.cli(), [r#"/ppp/secret/remove [find comment="tik:vpn-user:alice"]"#, r#"/ppp/active/remove [find name="alice"]"#]);
        assert!(!p.revertible && p.undo.is_empty());
        assert_eq!(f.seen[0], ["/ppp/secret/print", "=.proplist=name", "?comment=tik:vpn-user:alice"]);
        assert!(compile_value(&json!({"kind": "remove-vpn-user", "name": "alice"}), &mut ppp(true, false)).unwrap_err().contains("created by tikontrol"));
    }

    #[test]
    fn vpn_user_rejects_injection_and_weak_input() {
        let bad = [
            json!({"kind": "vpn-user", "name": "a b", "password": PW, "service": "l2tp"}),
            json!({"kind": "vpn-user", "name": "x\" password=1", "password": PW, "service": "l2tp"}),
            json!({"kind": "vpn-user", "name": "x;/system/reboot", "password": PW, "service": "l2tp"}),
            json!({"kind": "vpn-user", "name": "x]", "password": PW, "service": "l2tp"}),
            json!({"kind": "vpn-user", "name": "-x", "password": PW, "service": "l2tp"}),
            json!({"kind": "vpn-user", "name": "", "password": PW, "service": "l2tp"}),
            json!({"kind": "vpn-user", "name": "a".repeat(33), "password": PW, "service": "l2tp"}),
            json!({"kind": "vpn-user", "name": "alice", "password": "short-pw", "service": "l2tp"}),
            json!({"kind": "vpn-user", "name": "alice", "password": "p".repeat(65), "service": "l2tp"}),
            json!({"kind": "vpn-user", "name": "alice", "password": "line\nbreak-password", "service": "l2tp"}),
            json!({"kind": "vpn-user", "name": "alice", "password": "my-alice-password", "service": "l2tp"}),
            json!({"kind": "vpn-user", "name": "alice", "password": PW, "service": "pptp"}),
            json!({"kind": "vpn-user", "name": "alice", "password": PW, "service": "any"}),
            json!({"kind": "vpn-user", "name": "alice", "password": PW, "service": "l2tp", "profile": "x\"y"}),
            json!({"kind": "vpn-user", "name": "alice", "password": PW, "service": "l2tp", "group": "full"}),
            json!({"kind": "remove-vpn-user", "name": "x\"; /system/reboot"}),
        ];
        for b in bad {
            let e = compile_value(&b, &mut ppp(true, false)).unwrap_err();
            let pw = b.get("password").and_then(|v| v.as_str()).unwrap_or("\u{0}");
            assert!(!e.contains(pw), "error echoes the password: {e}");
        }
    }

    #[test]
    fn client_masks_the_private_key_and_routes_sources() {
        let v = json!({"kind": "wireguard-client", "name": "vpn1", "private_key": K2, "address": "10.2.0.2/32", "public_key": K, "endpoint": "vpn.example.com", "endpoint_port": 51820, "sources": ["192.168.88.50"]});
        let p = compile_value(&v, &mut Fake::default()).unwrap();
        assert!(p.cli().iter().all(|c| !c.contains(K2)));
        assert!(p.cli()[0].contains(r#"private-key="********""#));
        assert!(!revert_script("c", &p).unwrap().contains(K2));
        assert!(p.cli().last().unwrap().starts_with("/routing/rule/add src-address=192.168.88.50/32 action=lookup table=tik-vpn-vpn1"));
        assert_eq!(p.undo.len(), 7);
    }

    #[test]
    fn site_to_site_allows_tunnel_and_remote_subnets() {
        let p = compile_value(&json!({"kind": "site-to-site", "name": "s2s", "listen_port": 13232, "address": "10.255.0.1/30", "public_key": K, "endpoint": "203.0.113.7", "endpoint_port": 13232, "remote_subnets": ["10.0.1.0/24"]}), &mut Fake::default()).unwrap();
        assert!(p.cli()[2].contains("allowed-address=10.255.0.0/30,10.0.1.0/24"));
        assert!(p.cli().iter().any(|c| c == "/ip/route/add dst-address=10.0.1.0/24 gateway=s2s comment=tik:s2s:s2s"));
        assert_eq!(p.risk, Risk::High);
    }

    fn certs(rows: Vec<Row>) -> Fake {
        let mut f = Fake::default();
        f.answers.insert("/certificate/print".into(), Ok(rows));
        f
    }

    #[test]
    fn ovpn_certificates_are_created_signed_exported_and_removed_on_revert() {
        let mut f = certs(vec![row(&[("name", "other-cert"), ("comment", "made by hand")])]);
        let p = compile_value(&json!({"kind": "ovpn-certificates", "common_name": "vpn.example.net"}), &mut f).unwrap();
        assert_eq!(
            p.cli(),
            [
                "/certificate/add name=tik-ovpn-ca common-name=tikontrol-ovpn-ca key-size=2048 days-valid=3650 key-usage=key-cert-sign,crl-sign",
                r#"/certificate/sign [find name="tik-ovpn-ca"]"#,
                "/certificate/add name=tik-ovpn-server common-name=vpn.example.net key-size=2048 days-valid=3650 key-usage=digital-signature,key-encipherment,tls-server",
                r#"/certificate/sign [find name="tik-ovpn-server"] ca=tik-ovpn-ca"#,
                r#"/file/remove [find name="cert_export_tik-ovpn-ca.crt"]"#,
                r#"/certificate/export-certificate [find name="tik-ovpn-ca"] type=pem file-name=cert_export_tik-ovpn-ca"#,
            ]
        );
        // server before CA, the exported file too
        assert_eq!(p.undo_cli(), [r#"/file/remove [find name="cert_export_tik-ovpn-ca.crt"]"#, r#"/certificate/remove [find name="tik-ovpn-server"]"#, r#"/certificate/remove [find name="tik-ovpn-ca"]"#]);
        assert!(p.revertible);
        assert_eq!(p.risk, Risk::Medium);
        assert!(p.warnings.iter().any(|w| w.contains("deletes both certificates")));
        // the key is never exported: no passphrase anywhere
        assert!(p.cli().iter().all(|c| !c.contains("passphrase")));
        assert!(revert_script("c", &p).is_ok());
        // the existence check reads names only
        assert_eq!(f.seen[0], ["/certificate/print", "=.proplist=name"]);
        let p = compile_value(&json!({"kind": "ovpn-certificates", "common_name": "203.0.113.7", "ca_common_name": "Home-CA", "key_size": 4096, "days_valid": 825}), &mut Fake::default()).unwrap();
        assert!(p.cli()[0].contains("common-name=Home-CA key-size=4096 days-valid=825"));
        assert!(p.warnings.iter().any(|w| w.contains("minutes")));
    }

    #[test]
    fn ovpn_certificates_refuse_existing_names() {
        for existing in [row(&[("name", "tik-ovpn-ca")]), row(&[("name", "tik-ovpn-server")])] {
            let e = compile_value(&json!({"kind": "ovpn-certificates", "common_name": "vpn.example.net"}), &mut certs(vec![existing])).unwrap_err();
            assert!(e.contains("already exist"), "{e}");
        }
        // no /certificate menu: the read error is passed on, nothing is planned
        let mut f = Fake::default();
        f.answers.insert("/certificate/print".into(), Err("no such command prefix".into()));
        assert!(compile_value(&json!({"kind": "ovpn-certificates", "common_name": "vpn.example.net"}), &mut f).is_err());
    }

    #[test]
    fn ovpn_certificate_input_is_validated() {
        let bad = [
            json!({"kind": "ovpn-certificates", "common_name": ""}),
            json!({"kind": "ovpn-certificates", "common_name": "vpn example"}),
            json!({"kind": "ovpn-certificates", "common_name": "vpn\" key-usage=x"}),
            json!({"kind": "ovpn-certificates", "common_name": "vpn;/system/reboot"}),
            json!({"kind": "ovpn-certificates", "common_name": "vpn_host"}),
            json!({"kind": "ovpn-certificates", "common_name": "-vpn"}),
            json!({"kind": "ovpn-certificates", "common_name": "vpn."}),
            json!({"kind": "ovpn-certificates", "common_name": "ünï.example"}),
            json!({"kind": "ovpn-certificates", "common_name": "a".repeat(65)}),
            json!({"kind": "ovpn-certificates", "common_name": "vpn", "ca_common_name": "CA [x]"}),
            json!({"kind": "ovpn-certificates", "common_name": "vpn", "key_size": 1024}),
            json!({"kind": "ovpn-certificates", "common_name": "vpn", "key_size": 8192}),
            json!({"kind": "ovpn-certificates", "common_name": "vpn", "days_valid": 0}),
            json!({"kind": "ovpn-certificates", "common_name": "vpn", "days_valid": 3651}),
            json!({"kind": "ovpn-certificates", "common_name": "vpn", "export_passphrase": "x"}),
        ];
        for b in bad {
            assert!(compile_value(&b, &mut Fake::default()).is_err(), "{b}");
        }
    }

    #[test]
    fn ovpn_server_certificate_restores_the_old_one() {
        let mut f = Fake::default();
        f.answers.insert("/interface/ovpn-server/server/print".into(), Ok(vec![row(&[("name", "ovpn1"), ("certificate", "none"), ("require-client-certificate", "false")])]));
        let p = compile_value(&json!({"kind": "ovpn-server-certificate", "name": "ovpn1"}), &mut f).unwrap();
        assert_eq!(p.cli(), [r#"/interface/ovpn-server/server/set [find name="ovpn1"] certificate=tik-ovpn-server"#]);
        assert_eq!(p.undo_cli(), [r#"/interface/ovpn-server/server/set [find name="ovpn1"] certificate=none"#]);
        assert!(!p.warnings.iter().any(|w| w.contains("require-client-certificate")));
        assert!(compile_value(&json!({"kind": "ovpn-server-certificate", "name": "nope"}), &mut f).unwrap_err().contains("no OpenVPN server"));
        assert!(compile_value(&json!({"kind": "ovpn-server-certificate", "name": "ovpn1", "certificate": "other"}), &mut f).is_err(), "only tik-ovpn-server");
        // before 7.17: one unnamed server
        let mut f = Fake::default();
        f.answers.insert("/interface/ovpn-server/server/print".into(), Ok(vec![row(&[("certificate", "old-cert"), ("require-client-certificate", "true")])]));
        let p = compile_value(&json!({"kind": "ovpn-server-certificate"}), &mut f).unwrap();
        assert_eq!((p.cli()[0].as_str(), p.undo_cli()[0].as_str()), ("/interface/ovpn-server/server/set certificate=tik-ovpn-server", "/interface/ovpn-server/server/set certificate=old-cert"));
        assert!(p.warnings.iter().any(|w| w.contains("require-client-certificate=no")));
        assert!(p.warnings.iter().any(|w| w.contains("Replaces the certificate old-cert")));
        f.answers.insert("/interface/ovpn-server/server/print".into(), Ok(vec![row(&[("certificate", "tik-ovpn-server")])]));
        assert!(compile_value(&json!({"kind": "ovpn-server-certificate"}), &mut f).unwrap_err().contains("already uses"));
        assert!(compile_value(&json!({"kind": "ovpn-server-certificate"}), &mut Fake::default()).is_err());
    }

    #[test]
    fn certificates_and_server_switch_batch_and_revert_in_order() {
        let mut f = Fake::default();
        f.answers.insert("/interface/ovpn-server/server/print".into(), Ok(vec![row(&[("name", "ovpn1"), ("certificate", "none")])]));
        let v = json!({"kind": "batch", "title": "OpenVPN certificates", "intents": [
            {"kind": "ovpn-certificates", "common_name": "vpn.example.net"},
            {"kind": "ovpn-server-certificate", "name": "ovpn1"},
        ]});
        let p = compile_value(&v, &mut f).unwrap();
        assert!(p.revertible);
        assert_eq!(p.cli().last().unwrap(), r#"/interface/ovpn-server/server/set [find name="ovpn1"] certificate=tik-ovpn-server"#);
        // the server lets go of the certificate before it is deleted
        assert_eq!(p.undo_cli()[0], r#"/interface/ovpn-server/server/set [find name="ovpn1"] certificate=none"#);
        assert_eq!(p.undo_cli().last().unwrap(), r#"/certificate/remove [find name="tik-ovpn-ca"]"#);
    }

    #[test]
    fn ca_export_needs_our_ca_and_writes_only_the_public_certificate() {
        assert!(compile_value(&json!({"kind": "export-ovpn-ca"}), &mut Fake::default()).unwrap_err().contains("no tik-ovpn-ca"));
        let mut f = certs(vec![row(&[("name", "tik-ovpn-ca")])]);
        let p = compile_value(&json!({"kind": "export-ovpn-ca"}), &mut f).unwrap();
        assert_eq!(p.cli()[1], r#"/certificate/export-certificate [find name="tik-ovpn-ca"] type=pem file-name=cert_export_tik-ovpn-ca"#);
        assert_eq!(p.undo_cli(), [r#"/file/remove [find name="cert_export_tik-ovpn-ca.crt"]"#]);
        assert_eq!(f.seen[0], ["/certificate/print", "=.proplist=name", "?name=tik-ovpn-ca"]);
        // unknown fields are refused, so nothing can ask for another file or a passphrase
        assert!(compile_value(&json!({"kind": "export-ovpn-ca", "file": "x.p12"}), &mut f).is_err());
    }

    #[test]
    fn bad_input_is_rejected() {
        let bad = [
            json!({"kind": "wireguard-server", "name": "wg tik", "listen_port": 1, "address": "10.66.0.1/24"}),
            json!({"kind": "wireguard-server", "name": "wg", "listen_port": 0, "address": "10.66.0.1/24"}),
            json!({"kind": "wireguard-server", "name": "back-to-home-vpn", "listen_port": 5, "address": "10.66.0.1/24"}),
            json!({"kind": "remove-wireguard-peer", "public_key": "x"}),
            json!({"kind": "vpn-server", "server": "pptp", "enabled": true}),
            json!({"kind": "wireguard-client", "name": "v", "address": "10.2.0.2/32", "public_key": K, "endpoint": "bad host!", "endpoint_port": 51820}),
            json!({"kind": "wireguard-client", "name": "v", "address": "10.2.0.2/32", "public_key": K, "endpoint": "a.b", "endpoint_port": 51820, "sources": ["0.0.0.0/0"]}),
            json!({"kind": "wireguard-client", "name": "v", "private_key": "nope", "address": "10.2.0.2/32", "public_key": K, "endpoint": "a.b", "endpoint_port": 1}),
            json!({"kind": "site-to-site", "name": "s", "listen_port": 1, "address": "10.0.0.1/30", "public_key": K, "remote_subnets": []}),
            json!({"kind": "back-to-home", "enabled": true, "extra": 1}),
            json!({"kind": "back-to-home", "enabled": true}),
        ];
        for b in bad {
            assert!(compile_value(&b, &mut Fake::default()).is_err(), "{b}");
        }
    }
}
