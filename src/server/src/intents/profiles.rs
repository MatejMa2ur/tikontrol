//! Profiles: client groups (address lists), per-device speed limits for a group (PCQ + queue tree),
//! QoS rules (DSCP + priority) and RADIUS servers.

use super::internet::{parse, readd, rm, rows, slug, with, FASTTRACK};
use crate::apply::{ipv4, norm_mac, plan, rate, Op, Plan, Risk};
use crate::ros::cmd::valid_value;
use crate::ros::conn::Router;
use serde::Deserialize;
use serde_json::Value;
use std::net::IpAddr;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Member {
    mac: String,
    ip: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum I {
    ClientGroup { group: String, members: Vec<Member> },
    GroupLimit { group: String, down: String, up: String },
    RemoveGroupLimit { group: String },
    QosRule { name: String, protocol: String, ports: String, dscp: u8, priority: u8 },
    RemoveQosRule { name: String },
    RadiusServer { address: String, secret: String, services: Vec<String> },
    RemoveRadius { address: String },
}

const KINDS: &[&str] = &["client-group", "group-limit", "remove-group-limit", "qos-rule", "remove-qos-rule", "radius-server", "remove-radius"];

pub fn compile(v: &Value, r: &mut dyn Router) -> Option<Result<Plan, String>> {
    parse::<I>(v, KINDS).map(|i| i.and_then(|i| build(i, r)))
}

const AL: &str = "/ip/firewall/address-list";
const MANGLE: &str = "/ip/firewall/mangle";
const TREE: &str = "/queue/tree";
const QTYPE: &str = "/queue/type";

/// "80,443,8000-8010"
fn ports(s: &str) -> Result<String, String> {
    let s = s.trim();
    let ok = !s.is_empty()
        && s.len() <= 64
        && s.split(',').all(|p| {
            let mut it = p.splitn(2, '-');
            it.all(|n| n.parse::<u16>().is_ok_and(|n| n > 0))
        });
    ok.then(|| s.to_string()).ok_or_else(|| format!("invalid ports {s:?} (e.g. 5060,3478-3481)"))
}

fn build(i: I, r: &mut dyn Router) -> Result<Plan, String> {
    use I::*;
    Ok(match i {
        ClientGroup { group, members } => {
            let g = slug(&group)?;
            if members.len() > 64 {
                return Err("at most 64 devices per group".into());
            }
            let list = format!("tik-{g}");
            let mut ops = vec![rm(AL, "list", &list)];
            for m in &members {
                let (mac, ip) = (norm_mac(&m.mac)?, ipv4(&m.ip)?);
                if !ip.is_private() {
                    return Err(format!("{ip} is not a LAN address"));
                }
                ops.push(Op::add(AL, vec![("list", list.clone()), ("address", ip.to_string()), ("comment", format!("tik:group:{g}:{mac}"))]));
            }
            let mut undo = vec![rm(AL, "list", &list)];
            undo.extend(readd(r, AL, "list", &list, &["list", "address", "comment"])?);
            let p = plan(format!("Group {g}: {} device{}", members.len(), if members.len() == 1 { "" } else { "s" }), Risk::Low, ops, undo);
            with(p, &["Membership is by IP address: give group members a fixed IP so it stays right."])
        }
        GroupLimit { group, down, up } => {
            let (g, down, up) = (slug(&group)?, rate(&down)?, rate(&up)?);
            let (list, c) = (format!("tik-{g}"), format!("tik:profile:{g}"));
            let (qd, qu) = (format!("tik-{g}-down"), format!("tik-{g}-up"));
            for (path, k, v) in [(TREE, "comment", c.as_str()), (QTYPE, "name", qd.as_str()), (QTYPE, "name", qu.as_str())] {
                if !rows(r, path, &[".id"], k, v)?.is_empty() {
                    return Err(format!("group {g} already has a speed limit: remove it first"));
                }
            }
            let ops = vec![
                Op::add(QTYPE, vec![("name", qd.clone()), ("kind", "pcq".into()), ("pcq-rate", down.clone()), ("pcq-classifier", "dst-address".into())]),
                Op::add(QTYPE, vec![("name", qu.clone()), ("kind", "pcq".into()), ("pcq-rate", up.clone()), ("pcq-classifier", "src-address".into())]),
                Op::add(MANGLE, vec![("chain", "forward".into()), ("dst-address-list", list.clone()), ("action", "mark-packet".into()), ("new-packet-mark", qd.clone()), ("passthrough", "no".into()), ("comment", c.clone())]),
                Op::add(MANGLE, vec![("chain", "forward".into()), ("src-address-list", list.clone()), ("action", "mark-packet".into()), ("new-packet-mark", qu.clone()), ("passthrough", "no".into()), ("comment", c.clone())]),
                Op::add(TREE, vec![("name", qd.clone()), ("parent", "global".into()), ("packet-mark", qd.clone()), ("queue", qd.clone()), ("comment", c.clone())]),
                Op::add(TREE, vec![("name", qu.clone()), ("parent", "global".into()), ("packet-mark", qu.clone()), ("queue", qu.clone()), ("comment", c.clone())]),
            ];
            let undo = vec![rm(TREE, "comment", &c), rm(MANGLE, "comment", &c), rm(QTYPE, "name", &qd), rm(QTYPE, "name", &qu)];
            with(plan(format!("Limit each device in {g} to {down} down / {up} up"), Risk::Low, ops, undo), &[FASTTRACK, "Applies to the devices in the group's address list (set them with the group's Apply)."])
        }
        RemoveGroupLimit { group } => {
            let g = slug(&group)?;
            let c = format!("tik:profile:{g}");
            let (qd, qu) = (format!("tik-{g}-down"), format!("tik-{g}-up"));
            let tree = readd(r, TREE, "comment", &c, &["name", "parent", "packet-mark", "queue"])?;
            if tree.is_empty() {
                return Err("only limits set by tikontrol can be removed here".into());
            }
            let mut undo = readd(r, QTYPE, "name", &qd, &["name", "kind", "pcq-rate", "pcq-classifier"])?;
            undo.extend(readd(r, QTYPE, "name", &qu, &["name", "kind", "pcq-rate", "pcq-classifier"])?);
            undo.extend(readd(r, MANGLE, "comment", &c, &["chain", "dst-address-list", "src-address-list", "action", "new-packet-mark", "passthrough"])?);
            undo.extend(tree);
            plan(format!("Remove the speed limit of {g}"), Risk::Low, vec![rm(TREE, "comment", &c), rm(MANGLE, "comment", &c), rm(QTYPE, "name", &qd), rm(QTYPE, "name", &qu)], undo)
        }
        QosRule { name, protocol, ports: p, dscp, priority } => {
            let n = slug(&name)?;
            if !matches!(protocol.as_str(), "tcp" | "udp") {
                return Err("protocol must be tcp or udp".into());
            }
            if dscp > 63 || !(1..=8).contains(&priority) {
                return Err("DSCP must be 0-63 and priority 1-8".into());
            }
            let p = ports(&p)?;
            let (c, mark) = (format!("tik:qos:{n}"), format!("tik-qos-{n}"));
            if !rows(r, MANGLE, &[".id"], "comment", &c)?.is_empty() {
                return Err(format!("QoS rule {n} already exists: remove it first"));
            }
            let m = |extra: Vec<(&'static str, String)>| {
                let mut a = vec![("chain", "postrouting".into()), ("protocol", protocol.clone()), ("dst-port", p.clone())];
                a.extend(extra);
                a.push(("comment", c.clone()));
                Op::add(MANGLE, a)
            };
            let ops = vec![
                m(vec![("action", "change-dscp".into()), ("new-dscp", dscp.to_string()), ("passthrough", "yes".into())]),
                m(vec![("action", "mark-packet".into()), ("new-packet-mark", mark.clone()), ("passthrough", "no".into())]),
                Op::add(TREE, vec![("name", mark.clone()), ("parent", "global".into()), ("packet-mark", mark.clone()), ("priority", priority.to_string()), ("comment", c.clone())]),
            ];
            with(plan(format!("QoS {n}: {protocol} {p} → DSCP {dscp}, priority {priority}"), Risk::Medium, ops, vec![rm(TREE, "comment", &c), rm(MANGLE, "comment", &c)]), &[
                "Priority only matters when the line is full and a parent limit exists (Smart Queues); DSCP marks only help where the next hop honours them.",
                FASTTRACK,
            ])
        }
        RemoveQosRule { name } => {
            let n = slug(&name)?;
            let c = format!("tik:qos:{n}");
            let mut undo = readd(r, MANGLE, "comment", &c, &["chain", "protocol", "dst-port", "action", "new-dscp", "new-packet-mark", "passthrough"])?;
            if undo.is_empty() {
                return Err("only QoS rules created by tikontrol can be removed here".into());
            }
            undo.extend(readd(r, TREE, "comment", &c, &["name", "parent", "packet-mark", "priority"])?);
            plan(format!("Remove QoS rule {n}"), Risk::Medium, vec![rm(TREE, "comment", &c), rm(MANGLE, "comment", &c)], undo)
        }
        RadiusServer { address, secret, services } => {
            const ALLOWED: &[&str] = &["wireless", "dot1x", "hotspot", "ppp", "dhcp"];
            let addr = address.trim().parse::<IpAddr>().map_err(|_| format!("invalid RADIUS server address {address:?}"))?;
            if secret.is_empty() || secret.len() > 128 || !valid_value(&secret) {
                return Err("the RADIUS secret must be 1-128 characters".into());
            }
            if services.is_empty() || services.iter().any(|s| !ALLOWED.contains(&s.as_str())) {
                return Err("services must be some of wireless, dot1x, hotspot, ppp, dhcp (router login over RADIUS isn't offered)".into());
            }
            let c = format!("tik:radius:{addr}");
            if !rows(r, "/radius", &[".id"], "comment", &c)?.is_empty() {
                return Err(format!("RADIUS server {addr} already exists: remove it first"));
            }
            let ops = vec![Op::add("/radius", vec![("address", addr.to_string()), ("secret", secret), ("service", services.join(",")), ("comment", c.clone())])];
            with(plan(format!("Add RADIUS server {addr} for {}", services.join(", ")), Risk::Medium, ops, vec![rm("/radius", "comment", &c)]), &[
                "The shared secret goes to the router only: tikontrol doesn't store it, show it or put it in the revert job.",
                "Wi-Fi or ports set to use RADIUS stop letting devices in if the server doesn't answer.",
            ])
        }
        RemoveRadius { address } => {
            let addr = address.trim().parse::<IpAddr>().map_err(|_| format!("invalid RADIUS server address {address:?}"))?;
            let c = format!("tik:radius:{addr}");
            if rows(r, "/radius", &[".id"], "comment", &c)?.is_empty() {
                return Err("only RADIUS servers added by tikontrol can be removed here".into());
            }
            // the inverse would have to carry the shared secret, which tikontrol never keeps: not revertible
            Plan {
                title: format!("Remove RADIUS server {addr}"),
                risk: Risk::Medium,
                warnings: vec![
                    "This can't be undone automatically: tikontrol never keeps the shared secret. To bring the server back, add it again with its secret.".into(),
                    "Wi-Fi or ports that use RADIUS stop letting new devices in until another server is set.".into(),
                ],
                ops: vec![rm("/radius", "comment", &c)],
                undo: vec![],
                revertible: false,
            }
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
    fn every_kind_is_registered() {
        assert!(compile(&json!({"kind": "wan-dns"}), &mut Fake::default()).is_none());
        for k in KINDS {
            if let Err(e) = compile(&json!({"kind": k}), &mut Fake::default()).unwrap() {
                assert!(!e.contains("unknown variant"), "{k}: {e}");
            }
        }
    }

    #[test]
    fn group_replaces_members_and_restores_old_ones() {
        let mut f = Fake::default();
        f.answers.insert("/ip/firewall/address-list/print".into(), Ok(vec![row(&[("list", "tik-IoT"), ("address", "192.168.88.40"), ("comment", "tik:group:IoT:AA:BB:CC:00:00:01")])]));
        let p = ok(json!({"kind": "client-group", "group": "IoT", "members": [{"mac": "aa-bb-cc-00-00-02", "ip": "192.168.88.41"}]}), &mut f);
        assert_eq!(p.cli()[0], r#"/ip/firewall/address-list/remove [find list="tik-IoT"]"#);
        assert!(p.cli()[1].contains("address=192.168.88.41 comment=tik:group:IoT:AA:BB:CC:00:00:02"));
        assert!(p.undo_cli()[1].contains("address=192.168.88.40"));
        assert!(compile(&json!({"kind": "client-group", "group": "a b", "members": []}), &mut f).unwrap().is_err());
        assert!(compile(&json!({"kind": "client-group", "group": "x", "members": [{"mac": "AA:BB:CC:00:00:02", "ip": "8.8.8.8"}]}), &mut f).unwrap().is_err());
    }

    #[test]
    fn group_limit_is_pcq_and_refuses_duplicates() {
        let p = ok(json!({"kind": "group-limit", "group": "Kids", "down": "20M", "up": "5M"}), &mut Fake::default());
        assert_eq!(p.ops.len(), 6);
        assert!(p.cli()[0].contains("kind=pcq pcq-rate=20M pcq-classifier=dst-address"));
        assert!(p.cli()[2].contains("dst-address-list=tik-Kids"));
        let mut f = Fake::default();
        f.answers.insert("/queue/tree/print".into(), Ok(vec![row(&[(".id", "*1")])]));
        assert!(compile(&json!({"kind": "group-limit", "group": "Kids", "down": "20M", "up": "5M"}), &mut f).unwrap().is_err());
    }

    #[test]
    fn qos_validates_ports() {
        let p = ok(json!({"kind": "qos-rule", "name": "voip", "protocol": "udp", "ports": "5060,3478-3481", "dscp": 46, "priority": 1}), &mut Fake::default());
        assert!(p.cli()[0].contains("action=change-dscp new-dscp=46"));
        for bad in ["", "0", "80;x", "70000", "1-2-3"] {
            assert!(ports(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn radius_secret_is_masked_and_not_in_revert() {
        let p = ok(json!({"kind": "radius-server", "address": "10.0.0.50", "secret": "s3cr3t-value", "services": ["wireless"]}), &mut Fake::default());
        assert!(!p.cli()[0].contains("s3cr3t"), "{}", p.cli()[0]);
        assert!(p.cli()[0].contains(r#"secret="********""#));
        assert!(!revert_script("x", &p).unwrap().contains("s3cr3t"));
        assert!(compile(&json!({"kind": "radius-server", "address": "10.0.0.50", "secret": "x", "services": ["login"]}), &mut Fake::default()).unwrap().is_err());
    }

    #[test]
    fn remove_radius_only_touches_own_servers_and_is_one_shot() {
        assert!(compile(&json!({"kind": "remove-radius", "address": "10.0.0.50"}), &mut Fake::default()).unwrap().is_err(), "not ours");
        assert!(compile(&json!({"kind": "remove-radius", "address": "10.0.0.50\"]"}), &mut Fake::default()).unwrap().is_err());
        let mut f = Fake::default();
        f.answers.insert("/radius/print".into(), Ok(vec![row(&[(".id", "*3")])]));
        let p = ok(json!({"kind": "remove-radius", "address": " 10.0.0.50 "}), &mut f);
        assert_eq!(p.cli(), [r#"/radius/remove [find comment="tik:radius:10.0.0.50"]"#]);
        assert!(!p.revertible && p.undo.is_empty() && !p.warnings.is_empty());
        assert!(f.seen.iter().flatten().all(|w| !w.contains("secret")), "never reads the secret");
    }

    #[test]
    fn remove_limit_and_qos_restore_what_they_remove() {
        let mut f = Fake::default();
        f.answers.insert("/queue/tree/print".into(), Ok(vec![row(&[("name", "tik-Kids-down"), ("parent", "global"), ("packet-mark", "tik-Kids-down"), ("queue", "tik-Kids-down")])]));
        let p = ok(json!({"kind": "remove-group-limit", "group": "Kids"}), &mut f);
        assert_eq!(p.cli()[0], r#"/queue/tree/remove [find comment="tik:profile:Kids"]"#);
        assert!(p.undo_cli().last().unwrap().contains("comment=tik:profile:Kids"));
        assert!(compile(&json!({"kind": "remove-group-limit", "group": "Kids"}), &mut Fake::default()).unwrap().is_err(), "not ours");
        let mut f = Fake::default();
        f.answers.insert("/ip/firewall/mangle/print".into(), Ok(vec![row(&[("chain", "postrouting"), ("protocol", "udp"), ("dst-port", "5060"), ("action", "change-dscp"), ("new-dscp", "46"), ("passthrough", "true")])]));
        let p = ok(json!({"kind": "remove-qos-rule", "name": "voip"}), &mut f);
        assert_eq!(p.cli(), [r#"/queue/tree/remove [find comment="tik:qos:voip"]"#, r#"/ip/firewall/mangle/remove [find comment="tik:qos:voip"]"#]);
        assert!(p.undo_cli()[0].contains("passthrough=yes comment=tik:qos:voip"), "{:?}", p.undo_cli());
        assert!(compile(&json!({"kind": "remove-qos-rule", "name": "voip"}), &mut Fake::default()).unwrap().is_err(), "not ours");
    }
}
