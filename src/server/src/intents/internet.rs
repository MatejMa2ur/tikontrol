//! Internet (WAN) intents: upstream DNS + DoH, dynamic DNS, WAN connection type, VLAN-tagged WAN,
//! failover to a second WAN, Smart Queues (CAKE), IPv6 prefix delegation. Domain blocking lives in security.rs.
//! Also the small helpers the profiles and hotspot areas share.

use crate::apply::{cidr, ipv4, plan, rate, ros_name, yn, Op, Plan, Risk};
use crate::ros::cmd::{valid_value, Cmd};
use crate::ros::conn::Router;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;
use std::net::IpAddr;

// ---------- shared helpers (pub(super) for the other areas) ----------

/// Parses `v` as `T` when its kind is one of `kinds`; None leaves it to the next area.
pub(super) fn parse<T: DeserializeOwned>(v: &Value, kinds: &[&str]) -> Option<Result<T, String>> {
    let kind = v.get("kind")?.as_str()?;
    kinds.contains(&kind).then(|| serde_json::from_value(v.clone()).map_err(|e| format!("invalid {kind} intent: {e}")))
}

pub(super) fn truthy(v: &str) -> bool {
    v == "true" || v == "yes"
}

pub(super) fn rm(path: &'static str, k: &'static str, v: &str) -> Op {
    Op::on(path, "remove", k, v, vec![])
}

pub(super) fn set(path: &'static str, args: Vec<(&'static str, String)>) -> Op {
    Op { path, verb: "set", find: None, args }
}

pub(super) fn rows(r: &mut dyn Router, path: &str, keys: &[&str], k: &str, v: &str) -> Result<Vec<crate::ros::conn::Row>, String> {
    r.run(&Cmd::new(&format!("{path}/print")).proplist(keys).query(k, v))
}

/// Refuses to create an object that already exists: if the add then failed, the undo (remove by
/// name/comment) would delete the existing one too.
pub(super) fn fresh(r: &mut dyn Router, path: &str, k: &str, v: &str, what: &str) -> Result<(), String> {
    if rows(r, path, &[".id"], k, v)?.is_empty() { Ok(()) } else { Err(format!("{what} already exists: remove it first")) }
}

/// The `add` ops that recreate the rows matching k=v with the given fields (for "remove" intents).
/// Booleans come back from the API as true/false and are written as yes/no.
pub(super) fn readd(r: &mut dyn Router, path: &'static str, k: &'static str, v: &str, keys: &[&'static str]) -> Result<Vec<Op>, String> {
    let found = rows(r, path, keys, k, v)?;
    Ok(found
        .iter()
        .map(|row| {
            let mut args: Vec<(&'static str, String)> = keys
                .iter()
                .filter_map(|key| row.get(*key).filter(|x| !x.is_empty()).map(|x| (*key, if x == "true" || x == "false" { yn(x == "true") } else { x.clone() })))
                .collect();
            if !keys.contains(&k) {
                args.push((k, v.to_string()));
            }
            Op::add(path, args)
        })
        .collect())
}

/// Short identifier for names we build (`tik-<slug>-down`): letters, digits, - and _.
pub(super) fn slug(s: &str) -> Result<String, String> {
    let s = s.trim();
    let ok = !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    ok.then(|| s.to_string()).ok_or_else(|| "invalid name (letters, digits, - and _, at most 32)".into())
}

/// RouterOS duration such as 30m, 8h, 1d, 7d.
pub(super) fn duration(s: &str) -> Result<String, String> {
    let s = s.trim();
    if !s.is_ascii() {
        return Err(format!("invalid duration {s:?} (e.g. 1h, 1d)"));
    }
    let (n, u) = s.split_at(s.len().saturating_sub(1));
    let ok = !n.is_empty() && n.len() <= 4 && n.bytes().all(|b| b.is_ascii_digit()) && n.bytes().any(|b| b != b'0') && matches!(u, "s" | "m" | "h" | "d" | "w");
    ok.then(|| s.to_string()).ok_or_else(|| format!("invalid duration {s:?} (e.g. 1h, 1d)"))
}

pub(super) fn with(mut p: Plan, warnings: &[&str]) -> Plan {
    p.warnings.extend(warnings.iter().map(|w| w.to_string()));
    p
}

pub(super) const FASTTRACK: &str = "Queues don't see FastTracked traffic: with FastTrack on (the default firewall) this only shapes new, non-FastTracked connections. Turning FastTrack off costs CPU.";

// ---------- intents ----------

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum I {
    WanDns { servers: Vec<String>, #[serde(default)] doh: String },
    Ddns { enabled: bool },
    WanType {
        interface: String,
        #[serde(rename = "type")]
        wan_type: String,
        #[serde(default)]
        address: String,
        #[serde(default)]
        gateway: String,
        #[serde(default)]
        user: String,
        #[serde(default)]
        password: String,
    },
    WanVlan { interface: String, vlan_id: u16 },
    RemoveWanVlan { vlan_id: u16 },
    WanFailover { interface: String, gateway: String, #[serde(default = "two")] distance: u8 },
    RemoveWanFailover { interface: String },
    /// ECMP: a second default route at the primary's distance, so new connections spread over both WANs.
    WanBalance { interface: String, gateway: String },
    SmartQueue { interface: String, down: String, up: String },
    RemoveSmartQueue {},
    Ipv6Wan { interface: String, #[serde(default = "bridge")] lan: String, prefix_len: u8 },
    RemoveIpv6Wan {},
}

fn two() -> u8 {
    2
}
fn bridge() -> String {
    "bridge".into()
}

const KINDS: &[&str] = &[
    "wan-dns", "ddns", "wan-type", "wan-vlan", "remove-wan-vlan", "wan-failover", "remove-wan-failover", "wan-balance", "smart-queue", "remove-smart-queue", "ipv6-wan", "remove-ipv6-wan",
];

pub fn compile(v: &Value, r: &mut dyn Router) -> Option<Result<Plan, String>> {
    parse::<I>(v, KINDS).map(|i| i.and_then(|i| build(i, r)))
}

const DHCP: &str = "/ip/dhcp-client";
const MEMBER: &str = "/interface/list/member";
const ROUTE: &str = "/ip/route";
const SQ: &str = "/queue/simple";
const HIGH_WAN: &str = "High risk: this changes how the router reaches the internet. If you reach tikontrol or the router over the internet you can lose access until the change reverts by itself (5 minutes).";

fn build(i: I, r: &mut dyn Router) -> Result<Plan, String> {
    use I::*;
    Ok(match i {
        WanDns { servers, doh } => {
            if servers.is_empty() || servers.len() > 4 {
                return Err("give 1 to 4 DNS servers".into());
            }
            let servers = servers.iter().map(|s| s.trim().parse::<IpAddr>().map(|a| a.to_string()).map_err(|_| format!("invalid DNS server {s:?}"))).collect::<Result<Vec<_>, _>>()?;
            let doh = doh.trim().to_string();
            if !doh.is_empty() && !(doh.starts_with("https://") && doh.len() <= 256 && doh.bytes().all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\')) {
                return Err("DoH server must be an https:// URL".into());
            }
            let cur = r.run(&Cmd::new("/ip/dns/print").proplist(&["servers", "use-doh-server", "verify-doh-cert"]))?;
            let get = |k: &str| cur.first().and_then(|x| x.get(k)).cloned().unwrap_or_default();
            let ops = vec![set("/ip/dns", vec![("servers", servers.join(",")), ("use-doh-server", doh.clone()), ("verify-doh-cert", yn(!doh.is_empty()))])];
            let undo = vec![set("/ip/dns", vec![("servers", get("servers")), ("use-doh-server", get("use-doh-server")), ("verify-doh-cert", yn(truthy(&get("verify-doh-cert"))))])];
            let mut p = plan(format!("Use DNS {}{}", servers.join(", "), if doh.is_empty() { String::new() } else { " with DoH".into() }), Risk::Medium, ops, undo);
            p.warnings.push("Wrong DNS servers stop name lookups for every device until the change reverts.".into());
            if !doh.is_empty() {
                p.warnings.push("With DoH and verify-doh-cert=yes the router must trust the server's certificate (built-in trust anchors, RouterOS 7.19+, or an imported CA). Otherwise DNS stops resolving: check a lookup before keeping.".into());
            }
            p
        }
        Ddns { enabled } => {
            let cur = r.run(&Cmd::new("/ip/cloud/print").proplist(&["ddns-enabled"]))?;
            let was = cur.first().and_then(|x| x.get("ddns-enabled")).map_or("no".to_string(), |v| if v == "auto" { v.clone() } else { yn(truthy(v)) });
            let p = plan(format!("{} dynamic DNS", if enabled { "Enable" } else { "Disable" }), Risk::Low, vec![set("/ip/cloud", vec![("ddns-enabled", yn(enabled))])], vec![set("/ip/cloud", vec![("ddns-enabled", was)])]);
            if enabled { with(p, &["The router will contact MikroTik's IP Cloud and publish your public IP under a *.sn.mynetname.net name."]) } else { p }
        }
        WanType { interface, wan_type, address, gateway, user, password } => {
            let ifn = ros_name(&interface)?;
            let c = format!("tik:wan:{ifn}");
            let dhcp = rows(r, DHCP, &["disabled"], "interface", &ifn)?;
            let had = dhcp.first().map(|x| x.get("disabled").is_some_and(|v| truthy(v)));
            let (mut ops, mut undo) = (vec![], vec![]);
            let restore_dhcp = |undo: &mut Vec<Op>| {
                if let Some(was) = had {
                    undo.push(Op::on(DHCP, "set", "interface", &ifn, vec![("disabled", yn(was))]));
                }
            };
            let mut warnings = vec![HIGH_WAN.to_string()];
            let title = match wan_type.as_str() {
                "dhcp" => {
                    if had.is_some() {
                        ops.push(Op::on(DHCP, "set", "interface", &ifn, vec![("disabled", "no".into())]));
                        restore_dhcp(&mut undo);
                    } else {
                        ops.push(Op::add(DHCP, vec![("interface", ifn.clone()), ("add-default-route", "yes".into()), ("use-peer-dns", "yes".into()), ("comment", c.clone())]));
                        undo.push(rm(DHCP, "comment", &c));
                    }
                    format!("Use DHCP on {ifn}")
                }
                "static" => {
                    if !address.contains('/') {
                        return Err("static address needs a prefix, e.g. 203.0.113.10/24".into());
                    }
                    let (addr, gw) = (cidr(&address)?, ipv4(&gateway)?);
                    fresh(r, "/ip/address", "comment", &c, "a static WAN address set by tikontrol")?;
                    if had.is_some() {
                        ops.push(Op::on(DHCP, "set", "interface", &ifn, vec![("disabled", "yes".into())]));
                    }
                    ops.push(Op::add("/ip/address", vec![("address", addr.clone()), ("interface", ifn.clone()), ("comment", c.clone())]));
                    ops.push(Op::add(ROUTE, vec![("dst-address", "0.0.0.0/0".into()), ("gateway", gw.to_string()), ("comment", c.clone())]));
                    undo.push(rm(ROUTE, "comment", &c));
                    undo.push(rm("/ip/address", "comment", &c));
                    restore_dhcp(&mut undo);
                    warnings.push("Set DNS servers too (Internet → DNS): a static WAN gets none from the ISP.".into());
                    format!("Use static {addr} via {gw} on {ifn}")
                }
                "pppoe" => {
                    let user = user.trim().to_string();
                    if user.is_empty() || user.len() > 128 || !valid_value(&user) || password.is_empty() || password.len() > 128 || !valid_value(&password) {
                        return Err("PPPoE needs a user name and a password (at most 128 characters)".into());
                    }
                    fresh(r, "/interface/pppoe-client", "name", "pppoe-wan", "a pppoe-wan interface")?;
                    if had.is_some() {
                        ops.push(Op::on(DHCP, "set", "interface", &ifn, vec![("disabled", "yes".into())]));
                    }
                    ops.push(Op::add("/interface/pppoe-client", vec![("name", "pppoe-wan".into()), ("interface", ifn.clone()), ("user", user.clone()), ("password", password), ("add-default-route", "yes".into()), ("use-peer-dns", "yes".into()), ("disabled", "no".into()), ("comment", c.clone())]));
                    ops.push(Op::add(MEMBER, vec![("list", "WAN".into()), ("interface", "pppoe-wan".into()), ("comment", c.clone())]));
                    undo.push(rm(MEMBER, "comment", &c));
                    undo.push(rm("/interface/pppoe-client", "comment", &c));
                    restore_dhcp(&mut undo);
                    warnings.push("The PPPoE password goes to the router only: tikontrol doesn't store it, show it or put it in the revert job. A previous PPPoE password can't be read back (no sensitive policy), so an older PPPoE setup isn't restored by a revert.".into());
                    format!("Use PPPoE ({user}) on {ifn}")
                }
                _ => return Err("type must be dhcp, static or pppoe".into()),
            };
            let mut p = plan(title, Risk::High, ops, undo);
            p.warnings = warnings;
            p
        }
        WanVlan { interface, vlan_id } => {
            let ifn = ros_name(&interface)?;
            if !(1..=4094).contains(&vlan_id) {
                return Err("VLAN ID must be 1-4094".into());
            }
            let (name, c) = (format!("wan-vlan{vlan_id}"), format!("tik:wanvlan:{vlan_id}"));
            fresh(r, "/interface/vlan", "name", &name, &format!("interface {name}"))?;
            let ops = vec![
                Op::add("/interface/vlan", vec![("name", name.clone()), ("interface", ifn.clone()), ("vlan-id", vlan_id.to_string()), ("comment", c.clone())]),
                Op::add(MEMBER, vec![("list", "WAN".into()), ("interface", name.clone()), ("comment", c.clone())]),
            ];
            let p = plan(format!("Tagged WAN: VLAN {vlan_id} on {ifn}"), Risk::High, ops, vec![rm(MEMBER, "comment", &c), rm("/interface/vlan", "comment", &c)]);
            with(p, &[HIGH_WAN, "This creates the VLAN interface and adds it to the WAN list. Then set the connection type (DHCP, static or PPPoE) on it as a second step."])
        }
        RemoveWanVlan { vlan_id } => {
            let c = format!("tik:wanvlan:{vlan_id}");
            let mut undo = readd(r, "/interface/vlan", "comment", &c, &["name", "interface", "vlan-id"])?;
            if undo.is_empty() {
                return Err("only VLAN WANs created by tikontrol can be removed here".into());
            }
            undo.extend(readd(r, MEMBER, "comment", &c, &["list", "interface"])?);
            with(plan(format!("Remove tagged WAN VLAN {vlan_id}"), Risk::High, vec![rm(MEMBER, "comment", &c), rm("/interface/vlan", "comment", &c)], undo), &[HIGH_WAN])
        }
        WanBalance { interface, gateway } => {
            // the primary's distance: the lowest of the active default routes (a DHCP client's is 1)
            let d = r.run(&Cmd::new("/ip/route/print").proplist(&["distance", "active"]).query("dst-address", "0.0.0.0/0"))?
                .iter().filter(|x| x.get("active").is_some_and(|a| truthy(a))).filter_map(|x| x.get("distance")?.parse::<u8>().ok()).min().ok_or("no active default route: set up the primary WAN first")?;
            let mut p = build(WanFailover { interface, gateway, distance: d.max(1) }, r)?;
            p.title = p.title.replace("Failover to", "Load balance with");
            p.warnings.push("Load balancing is per connection (ECMP hash): one download still uses one WAN, and sites that pin a login to your IP may sign you out. Remove it like failover (Remove second WAN).".into());
            p
        }
        WanFailover { interface, gateway, distance } => {
            let (ifn, gw) = (ros_name(&interface)?, ipv4(&gateway)?);
            if !(1..=250).contains(&distance) {
                return Err("distance must be 1-250".into());
            }
            let c = format!("tik:failover:{ifn}");
            fresh(r, ROUTE, "comment", &c, "a failover route for this port")?;
            let ops = vec![
                Op::add(MEMBER, vec![("list", "WAN".into()), ("interface", ifn.clone()), ("comment", c.clone())]),
                Op::add(ROUTE, vec![("dst-address", "0.0.0.0/0".into()), ("gateway", gw.to_string()), ("distance", distance.to_string()), ("check-gateway", "ping".into()), ("comment", c.clone())]),
                Op::add("/tool/netwatch", vec![("host", gw.to_string()), ("type", "icmp".into()), ("interval", "10s".into()), ("comment", c.clone())]),
            ];
            let undo = vec![rm("/tool/netwatch", "comment", &c), rm(ROUTE, "comment", &c), rm(MEMBER, "comment", &c)];
            with(plan(format!("Failover to {gw} on {ifn}"), Risk::High, ops, undo), &[
                HIGH_WAN,
                "The second port must already have an address (DHCP client or static) and must not be a bridge port.",
                "For the switch-over to happen the primary route must notice failures too: set check-gateway=ping on the primary DHCP client or route.",
            ])
        }
        RemoveWanFailover { interface } => {
            let ifn = ros_name(&interface)?;
            let c = format!("tik:failover:{ifn}");
            let mut undo = readd(r, MEMBER, "comment", &c, &["list", "interface"])?;
            let routes = readd(r, ROUTE, "comment", &c, &["dst-address", "gateway", "distance", "check-gateway"])?;
            if routes.is_empty() {
                return Err("only failover set up by tikontrol can be removed here".into());
            }
            undo.extend(routes);
            undo.extend(readd(r, "/tool/netwatch", "comment", &c, &["host", "type", "interval"])?);
            with(plan(format!("Remove failover on {ifn}"), Risk::High, vec![rm("/tool/netwatch", "comment", &c), rm(ROUTE, "comment", &c), rm(MEMBER, "comment", &c)], undo), &[HIGH_WAN])
        }
        SmartQueue { interface, down, up } => {
            let (ifn, down, up) = (ros_name(&interface)?, rate(&down)?, rate(&up)?);
            let c = "tik:smartqueue";
            let cur = rows(r, SQ, &["max-limit"], "comment", c)?;
            let p = if let Some(row) = cur.first() {
                let was = row.get("max-limit").cloned().unwrap_or_default();
                plan(format!("Smart Queues: {down} down / {up} up"), Risk::Medium, vec![Op::on(SQ, "set", "comment", c, vec![("max-limit", format!("{up}/{down}"))])], vec![Op::on(SQ, "set", "comment", c, vec![("max-limit", was)])])
            } else {
                let (mut ops, mut undo) = (vec![], vec![rm(SQ, "comment", c)]);
                if rows(r, "/queue/type", &[".id"], "name", "tik-cake")?.is_empty() {
                    ops.push(Op::add("/queue/type", vec![("name", "tik-cake".into()), ("kind", "cake".into()), ("cake-diffserv", "diffserv4".into()), ("cake-nat", "yes".into())]));
                    undo.push(rm("/queue/type", "name", "tik-cake"));
                }
                // simple queue max-limit is upload/download as seen from the LAN
                ops.push(Op::add(SQ, vec![("name", "tik-smartqueue".into()), ("target", ifn.clone()), ("max-limit", format!("{up}/{down}")), ("queue", "tik-cake/tik-cake".into()), ("comment", c.into())]));
                plan(format!("Smart Queues on {ifn}: {down} down / {up} up"), Risk::Medium, ops, undo)
            };
            with(p, &[FASTTRACK, "Set the limits to about 90% of your measured line speed, or the queue does nothing."])
        }
        RemoveSmartQueue {} => {
            let c = "tik:smartqueue";
            let undo = readd(r, SQ, "comment", c, &["name", "target", "max-limit", "queue"])?;
            if undo.is_empty() {
                return Err("Smart Queues aren't set up by tikontrol".into());
            }
            plan("Turn off Smart Queues".into(), Risk::Low, vec![rm(SQ, "comment", c)], undo)
        }
        Ipv6Wan { interface, lan, prefix_len } => {
            let (ifn, lan) = (ros_name(&interface)?, ros_name(&lan)?);
            if !(48..=64).contains(&prefix_len) {
                return Err("prefix length must be 48-64".into());
            }
            let c = "tik:v6wan";
            fresh(r, "/ipv6/dhcp-client", "comment", c, "an IPv6 WAN set by tikontrol")?;
            let ops = vec![
                Op::add("/ipv6/dhcp-client", vec![("interface", ifn.clone()), ("request", "prefix".into()), ("pool-name", "tik-v6".into()), ("pool-prefix-length", "64".into()), ("prefix-hint", format!("::/{prefix_len}")), ("add-default-route", "yes".into()), ("use-peer-dns", "yes".into()), ("comment", c.into())]),
                Op::add("/ipv6/address", vec![("interface", lan.clone()), ("from-pool", "tik-v6".into()), ("advertise", "yes".into()), ("comment", c.into())]),
            ];
            with(plan(format!("IPv6 on {ifn} (/{prefix_len} prefix) for {lan}"), Risk::Medium, ops, vec![rm("/ipv6/address", "comment", c), rm("/ipv6/dhcp-client", "comment", c)]), &[
                "Check /ipv6/firewall first: without input and forward drop rules for the WAN, devices on your LAN become reachable from the internet over IPv6.",
                "IPv6 must be enabled (/ipv6/settings disable-ipv6=no).",
            ])
        }
        RemoveIpv6Wan {} => {
            let c = "tik:v6wan";
            let mut undo = readd(r, "/ipv6/dhcp-client", "comment", c, &["interface", "request", "pool-name", "pool-prefix-length", "prefix-hint", "add-default-route", "use-peer-dns"])?;
            if undo.is_empty() {
                return Err("IPv6 WAN isn't set up by tikontrol".into());
            }
            undo.extend(readd(r, "/ipv6/address", "comment", c, &["interface", "from-pool", "advertise"])?);
            plan("Turn off IPv6 WAN".into(), Risk::Medium, vec![rm("/ipv6/address", "comment", c), rm("/ipv6/dhcp-client", "comment", c)], undo)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply::revert_script;
    use crate::ros::queries::tests::{row, Fake};
    use serde_json::json;

    fn ok(v: Value, f: &mut Fake) -> Plan {
        compile(&v, f).expect("own kind").expect("compiles")
    }

    #[test]
    fn other_kinds_are_left_alone() {
        assert!(compile(&json!({"kind": "block-client", "mac": "AA:BB:CC:DD:EE:FF"}), &mut Fake::default()).is_none());
        assert!(compile(&json!({"kind": "wan-dns", "servers": ["1.1.1.1"], "x": 1}), &mut Fake::default()).unwrap().is_err());
    }

    #[test]
    fn every_kind_is_registered() {
        for k in KINDS {
            let e = compile(&json!({"kind": k}), &mut Fake::default()).unwrap();
            if let Err(e) = e {
                assert!(!e.contains("unknown variant"), "{k}: {e}");
            }
        }
    }

    #[test]
    fn dns_restores_previous_servers() {
        let mut f = Fake::default();
        f.answers.insert("/ip/dns/print".into(), Ok(vec![row(&[("servers", "192.0.2.53"), ("use-doh-server", ""), ("verify-doh-cert", "false")])]));
        let p = ok(json!({"kind": "wan-dns", "servers": ["1.1.1.1", "9.9.9.9"], "doh": "https://cloudflare-dns.com/dns-query"}), &mut f);
        assert_eq!(p.cli(), [r#"/ip/dns/set servers=1.1.1.1,9.9.9.9 use-doh-server=https://cloudflare-dns.com/dns-query verify-doh-cert=yes"#]);
        assert_eq!(p.undo_cli(), [r#"/ip/dns/set servers=192.0.2.53 use-doh-server="" verify-doh-cert=no"#]);
        assert!(compile(&json!({"kind": "wan-dns", "servers": ["x"]}), &mut f).unwrap().is_err());
        assert!(compile(&json!({"kind": "wan-dns", "servers": ["1.1.1.1"], "doh": "http://x"}), &mut f).unwrap().is_err());
    }

    #[test]
    fn pppoe_password_never_reaches_preview_or_revert() {
        let mut f = Fake::default();
        f.answers.insert("/ip/dhcp-client/print".into(), Ok(vec![row(&[("disabled", "false")])]));
        let p = ok(json!({"kind": "wan-type", "interface": "ether1", "type": "pppoe", "user": "u@isp", "password": "hunter2-secret"}), &mut f);
        assert_eq!(p.risk, Risk::High);
        assert!(p.ops.iter().any(|o| o.args.iter().any(|(_, v)| v == "hunter2-secret")), "sent to the router");
        assert!(!p.cli().join(" ").contains("hunter2"));
        let s = revert_script("x", &p).unwrap();
        assert!(!s.contains("hunter2"), "{s}");
        assert!(s.contains(r#"/ip/dhcp-client/set [find interface="ether1"] disabled=no"#), "{s}");
        assert!(p.warnings.iter().any(|w| w.contains("can't be read back")));
    }

    #[test]
    fn static_wan_needs_prefix_and_gateway() {
        let mut f = Fake::default();
        assert!(compile(&json!({"kind": "wan-type", "interface": "ether1", "type": "static", "address": "203.0.113.10", "gateway": "203.0.113.1"}), &mut f).unwrap().is_err());
        let p = ok(json!({"kind": "wan-type", "interface": "ether1", "type": "static", "address": "203.0.113.10/24", "gateway": "203.0.113.1"}), &mut f);
        assert_eq!(p.ops.len(), 2, "no DHCP client to disable");
        assert!(compile(&json!({"kind": "wan-type", "interface": "ether1", "type": "lte"}), &mut f).unwrap().is_err());
    }

    #[test]
    fn failover_is_tagged_and_high_risk() {
        let p = ok(json!({"kind": "wan-failover", "interface": "ether5", "gateway": "192.0.2.1"}), &mut Fake::default());
        assert_eq!(p.risk, Risk::High);
        assert!(p.cli()[1].contains("distance=2 check-gateway=ping comment=tik:failover:ether5"));
        assert_eq!(p.undo.len(), 3);
        assert!(compile(&json!({"kind": "wan-failover", "interface": "ether5", "gateway": "192.0.2.1", "distance": 0}), &mut Fake::default()).unwrap().is_err());
    }

    #[test]
    fn balance_matches_the_primary_distance_and_undoes_like_failover() {
        // answers in call order: the default routes, then "no tik:failover route yet"
        struct Seq(Vec<Vec<crate::ros::conn::Row>>);
        impl Router for Seq {
            fn run(&mut self, _: &Cmd) -> Result<Vec<crate::ros::conn::Row>, String> {
                Ok(if self.0.is_empty() { vec![] } else { self.0.remove(0) })
            }
        }
        let mut f = Fake::default();
        assert!(compile(&json!({"kind": "wan-balance", "interface": "ether5", "gateway": "192.0.2.1"}), &mut f).unwrap().is_err(), "no primary route");
        let mut s = Seq(vec![vec![row(&[("distance", "1"), ("active", "true")]), row(&[("distance", "2"), ("active", "false")])]]);
        let p = compile(&json!({"kind": "wan-balance", "interface": "ether5", "gateway": "192.0.2.1"}), &mut s).unwrap().unwrap();
        assert!(p.cli()[1].contains("gateway=192.0.2.1 distance=1 check-gateway=ping comment=tik:failover:ether5"), "{:?}", p.cli());
        assert!(p.title.starts_with("Load balance"));
        assert_eq!(p.undo_cli()[1], r#"/ip/route/remove [find comment="tik:failover:ether5"]"#);
        for b in [json!({"kind": "wan-balance", "interface": "ether5;/system/reboot", "gateway": "192.0.2.1"}), json!({"kind": "wan-balance", "interface": "ether5", "gateway": "x"})] {
            assert!(compile(&b, &mut f).unwrap().is_err(), "{b}");
        }
    }

    #[test]
    fn smart_queue_adds_cake_once_and_warns_about_fasttrack() {
        let p = ok(json!({"kind": "smart-queue", "interface": "ether1", "down": "450M", "up": "45M"}), &mut Fake::default());
        assert!(p.cli()[0].starts_with("/queue/type/add name=tik-cake kind=cake"));
        assert!(p.cli()[1].contains("max-limit=45M/450M"));
        assert!(p.warnings[0].contains("FastTrack"));
        let mut f = Fake::default();
        f.answers.insert("/queue/simple/print".into(), Ok(vec![row(&[("max-limit", "10M/100M")])]));
        let p = ok(json!({"kind": "smart-queue", "interface": "ether1", "down": "450M", "up": "45M"}), &mut f);
        assert_eq!(p.undo_cli(), [r#"/queue/simple/set [find comment="tik:smartqueue"] max-limit=10M/100M"#]);
    }

    #[test]
    fn ipv6_ddns_and_vlan() {
        let p = ok(json!({"kind": "ipv6-wan", "interface": "ether1", "prefix_len": 56}), &mut Fake::default());
        assert!(p.cli()[0].contains("prefix-hint=::/56"));
        assert!(compile(&json!({"kind": "ipv6-wan", "interface": "ether1", "prefix_len": 40}), &mut Fake::default()).unwrap().is_err());
        let mut f = Fake::default();
        f.answers.insert("/ip/cloud/print".into(), Ok(vec![row(&[("ddns-enabled", "auto")])]));
        assert_eq!(ok(json!({"kind": "ddns", "enabled": true}), &mut f).undo_cli(), ["/ip/cloud/set ddns-enabled=auto"]);
        let p = ok(json!({"kind": "wan-vlan", "interface": "ether1", "vlan_id": 7}), &mut Fake::default());
        assert_eq!(p.risk, Risk::High);
        assert!(p.cli()[0].contains("name=wan-vlan7 interface=ether1 vlan-id=7"));
        assert!(compile(&json!({"kind": "wan-vlan", "interface": "ether1", "vlan_id": 0}), &mut Fake::default()).unwrap().is_err());
    }

    #[test]
    fn helpers_validate() {
        assert!(duration("1d").is_ok() && duration("30m").is_ok());
        assert!(duration("0d").is_err() && duration("1y").is_err() && duration("d").is_err() && duration("").is_err());
        assert!(slug("IoT_2").is_ok() && slug("a b").is_err() && slug("").is_err());
    }
}
