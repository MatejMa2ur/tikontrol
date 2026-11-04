//! VPN (docs/FEATURES.md "VPN"): WireGuard server, peers, client to a provider (with policy routing),
//! site-to-site, Back To Home, and L2TP/OpenVPN server on/off. Adding a WireGuard peer is the core
//! `wireguard-peer` intent in apply.rs; this area adds removing and pausing peers.
//!
//! Everything created is tagged `comment=tik:<kind>:<name>` and the inverse removes exactly that, in
//! reverse order. A provider's private key may be sent once to create a client interface: it is
//! masked in every CLI text and never part of an inverse.

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
    VpnServer {
        server: String,
        enabled: bool,
        #[serde(default)]
        name: String,
    },
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
}

fn yes() -> bool {
    true
}

const KINDS: &[&str] = &["wireguard-server", "remove-wireguard-peer", "wireguard-peer-enabled", "back-to-home", "vpn-server", "wireguard-client", "site-to-site"];

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
fn network(c: &str) -> String {
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
        V::VpnServer { server, enabled, name } => match server.as_str() {
            "l2tp" => {
                let path = "/interface/l2tp-server/server";
                let rows = r.run(&Cmd::new("/interface/l2tp-server/server/print").proplist(&["enabled", "use-ipsec"]))?;
                let cur = rows.first().ok_or("no L2TP server on this router")?;
                let was = cur.get("enabled").is_some_and(|v| is_true(v));
                let set = |on: bool| Op { path, verb: "set", find: None, args: vec![("enabled", yn(on))] };
                let mut p = plan(format!("{} the L2TP/IPsec server", if enabled { "Enable" } else { "Disable" }), Risk::Medium, vec![set(enabled)], vec![set(was)]);
                if enabled {
                    p.warnings.push("Remote devices can try to log in with PPP secrets: allow UDP 500, 4500 and 1701 on input only if you need it.".into());
                    if cur.get("use-ipsec").is_none_or(|v| v != "required") {
                        p.warnings.push("use-ipsec is not \"required\": L2TP without IPsec is unencrypted. Set use-ipsec=required and an IPsec secret in RouterOS (tikontrol doesn't handle that secret).".into());
                    }
                }
                Ok(p)
            }
            "ovpn" => {
                let path = "/interface/ovpn-server/server";
                let rows = r.run(&Cmd::new("/interface/ovpn-server/server/print").proplist(&["name", "disabled", "enabled"]))?;
                let named = rows.iter().any(|x| x.contains_key("name"));
                let (ops, undo, label) = if named {
                    // 7.17+: several servers, each with disabled=
                    let n = ifname(&name).map_err(|_| "pick the OpenVPN server by name")?;
                    let cur = rows.iter().find(|x| x.get("name") == Some(&n)).ok_or("no OpenVPN server with that name")?;
                    let was_on = !cur.get("disabled").is_some_and(|v| is_true(v));
                    let set = |on: bool| Op::on(path, "set", "name", &n, vec![("disabled", yn(!on))]);
                    (vec![set(enabled)], vec![set(was_on)], format!("OpenVPN server {n}"))
                } else {
                    let cur = rows.first().ok_or("no OpenVPN server on this router: create one with a certificate in RouterOS first")?;
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
        f.answers.insert("/interface/l2tp-server/server/print".into(), Ok(vec![row(&[("enabled", "false"), ("use-ipsec", "no")])]));
        let p = compile_value(&json!({"kind": "vpn-server", "server": "l2tp", "enabled": true}), &mut f).unwrap();
        assert_eq!((p.cli()[0].as_str(), p.undo_cli()[0].as_str(), p.risk), ("/interface/l2tp-server/server/set enabled=yes", "/interface/l2tp-server/server/set enabled=no", Risk::Medium));
        assert!(p.warnings.iter().any(|w| w.contains("unencrypted")));
        f.answers.insert("/interface/ovpn-server/server/print".into(), Ok(vec![row(&[("name", "ovpn1"), ("disabled", "true")])]));
        let p = compile_value(&json!({"kind": "vpn-server", "server": "ovpn", "name": "ovpn1", "enabled": true}), &mut f).unwrap();
        assert_eq!(p.undo_cli(), [r#"/interface/ovpn-server/server/set [find name="ovpn1"] disabled=yes"#]);
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
