//! Ports: advanced switching (docs/FEATURES.md "Ports"): port mirroring, link aggregation (bonding),
//! STP / loop protection, storm control, 802.1X port auth and LLDP / neighbour discovery.
//!
//! Everything is feature-detected on the router first: a switch chip without mirroring or storm
//! control answers without those fields, and the intent refuses with an explanation instead of
//! sending a command the chip ignores. Settings read their current values so the inverse restores
//! exactly them; objects we create carry `comment=tik:<kind>:<key>`.
//! ponytail: tikontrol can't see which port its own session enters through, so every change that can cut
//! a port off is High risk with a "your management port" warning; detect the ingress port if this bites.

use super::networks::{ifname, print, remove, truthy};
use crate::apply::{plan, yn, Op, Plan, Risk};
use crate::ros::conn::{Router, Row};
use serde::Deserialize;

const SWITCH: &str = "/interface/ethernet/switch";
const SWITCH_PORT: &str = "/interface/ethernet/switch/port";
const ETHER: &str = "/interface/ethernet";
const BOND: &str = "/interface/bonding";
const BRIDGE: &str = "/interface/bridge";
const BPORT: &str = "/interface/bridge/port";
const DOT1X: &str = "/interface/dot1x/server";
const DISCOVERY: &str = "/ip/neighbor/discovery-settings";

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum P {
    SetPortMirror { source: String, target: String },
    RemovePortMirror {},
    CreateBond { name: String, slaves: Vec<String>, #[serde(default = "lacp")] mode: String },
    RemoveBond { name: String },
    SetBridgeStp { bridge: String, protocol_mode: String },
    SetPortStp { interface: String, edge: String, bpdu_guard: bool },
    SetStormRate { interface: String, percent: u8 },
    AddDot1xPort { interface: String },
    RemoveDot1xPort { interface: String },
    SetDiscovery { interface_list: String, #[serde(default)] protocols: Option<Vec<String>>, #[serde(default)] voice_vlan: Option<u16> },
}

fn lacp() -> String {
    "802.3ad".into()
}

pub const KINDS: &[&str] = &["set-port-mirror", "remove-port-mirror", "create-bond", "remove-bond", "set-bridge-stp", "set-port-stp", "set-storm-rate", "add-dot1x-port", "remove-dot1x-port", "set-discovery"];
const BOND_MODES: &[&str] = &["802.3ad", "active-backup", "balance-rr", "balance-xor", "balance-tlb", "balance-alb", "broadcast"];
const STP_MODES: &[&str] = &["none", "stp", "rstp", "mstp"];
const EDGE: &[&str] = &["auto", "yes", "no", "no-discover", "yes-discover"];
const DISCOVERY_PROTOS: &[&str] = &["cdp", "lldp", "mndp"];

pub fn compile(v: &serde_json::Value, r: &mut dyn Router) -> Option<Result<Plan, String>> {
    let kind = v.get("kind")?.as_str()?;
    if !KINDS.contains(&kind) {
        return None;
    }
    Some(serde_json::from_value::<P>(v.clone()).map_err(|e| format!("invalid {kind} intent: {e}")).and_then(|i| build(&i, r)))
}

fn set_on(path: &'static str, k: &'static str, v: &str, args: Vec<(&'static str, String)>) -> Op {
    Op::on(path, "set", k, v, args)
}

fn set(path: &'static str, args: Vec<(&'static str, String)>) -> Op {
    Op { path, verb: "set", find: None, args }
}

fn mgmt(ifn: &str) -> String {
    format!("High risk: if tikontrol or this browser reaches the router through {ifn}, this change can cut you off until it reverts in 5 minutes.")
}

/// An existing ethernet port, by name.
fn ether(r: &mut dyn Router, s: &str) -> Result<String, String> {
    let n = ifname(s)?;
    if print(r, ETHER, &["name"], Some(("name", &n)))?.is_empty() {
        return Err(format!("{n} is not an ethernet port on this router"));
    }
    Ok(n)
}

/// The (first) switch chip, if its menu reports mirroring.
fn mirror_switch(r: &mut dyn Router) -> Result<Row, String> {
    let sw = print(r, SWITCH, &["name", "type", "mirror-source", "mirror-target"], None)?.into_iter().next();
    sw.filter(|s| s.contains_key("mirror-source")).ok_or_else(|| "this router's switch chip has no port mirroring: RouterOS reports no mirror-source for it. Use /tool/sniffer instead (CPU-based).".into())
}

fn build(i: &P, r: &mut dyn Router) -> Result<Plan, String> {
    use P::*;
    Ok(match i {
        SetPortMirror { source, target } => {
            let (src, tgt) = (ether(r, source)?, ether(r, target)?);
            if src == tgt {
                return Err("source and target must be different ports".into());
            }
            let sw = mirror_switch(r)?;
            let name = sw.get("name").cloned().unwrap_or_else(|| "switch1".into());
            let was = |k: &str| sw.get(k).cloned().filter(|v| !v.is_empty()).unwrap_or_else(|| "none".into());
            let mut p = plan(
                format!("Mirror {src} to {tgt}"),
                Risk::High,
                vec![set_on(SWITCH, "name", &name, vec![("mirror-source", src.clone()), ("mirror-target", tgt.clone())])],
                vec![set_on(SWITCH, "name", &name, vec![("mirror-source", was("mirror-source")), ("mirror-target", was("mirror-target"))])],
            );
            p.warnings.push(format!("{tgt} receives a copy of all of {src}'s traffic: plug only a capture machine you trust into it. Mirroring works in the switch chip, so only between ports of the same chip."));
            p.warnings.push(mgmt(&tgt));
            p
        }
        RemovePortMirror {} => {
            let sw = mirror_switch(r)?;
            let name = sw.get("name").cloned().unwrap_or_else(|| "switch1".into());
            let was = |k: &str| sw.get(k).cloned().filter(|v| !v.is_empty()).unwrap_or_else(|| "none".into());
            plan(
                "Stop port mirroring".into(),
                Risk::Low,
                vec![set_on(SWITCH, "name", &name, vec![("mirror-source", "none".into()), ("mirror-target", "none".into())])],
                vec![set_on(SWITCH, "name", &name, vec![("mirror-source", was("mirror-source")), ("mirror-target", was("mirror-target"))])],
            )
        }
        CreateBond { name, slaves, mode } => {
            let name = ifname(name)?;
            if !BOND_MODES.contains(&mode.as_str()) {
                return Err(format!("mode must be one of {}", BOND_MODES.join(", ")));
            }
            if !(2..=4).contains(&slaves.len()) {
                return Err("a bond takes 2 to 4 ports".into());
            }
            let slaves = slaves.iter().map(|s| ether(r, s)).collect::<Result<Vec<_>, _>>()?;
            if slaves.iter().enumerate().any(|(i, s)| slaves[..i].contains(s)) {
                return Err("each port can be in the bond once".into());
            }
            if !print(r, "/interface", &["name"], Some(("name", &name)))?.is_empty() {
                return Err(format!("an interface named {name} already exists"));
            }
            let c = format!("tik:bond:{name}");
            // slaves must leave the bridge first; the bond takes their place with the first port's VLAN
            let (mut ops, mut undo) = (vec![], vec![]);
            let mut bridge: Option<(String, String)> = None;
            for s in &slaves {
                if let Some(bp) = print(r, BPORT, &["bridge", "pvid", "frame-types"], Some(("interface", s)))?.into_iter().next() {
                    let b = bp.get("bridge").cloned().unwrap_or_default();
                    let pvid = bp.get("pvid").cloned().unwrap_or_else(|| "1".into());
                    bridge.get_or_insert((b.clone(), pvid.clone()));
                    ops.push(Op::on(BPORT, "remove", "interface", s, vec![]));
                    undo.insert(0, Op::add(BPORT, vec![("bridge", b), ("interface", s.clone()), ("pvid", pvid), ("frame-types", bp.get("frame-types").cloned().unwrap_or_else(|| "admit-all".into()))]));
                }
            }
            ops.push(Op::add(BOND, vec![("name", name.clone()), ("slaves", slaves.join(",")), ("mode", mode.clone()), ("comment", c.clone())]));
            undo.insert(0, remove(BOND, &c));
            if let Some((b, pvid)) = bridge {
                ops.push(Op::add(BPORT, vec![("bridge", b), ("interface", name.clone()), ("pvid", pvid), ("comment", c.clone())]));
                undo.insert(0, remove(BPORT, &c));
            }
            let mut p = plan(format!("Bond {} as {name} ({mode})", slaves.join(" + ")), Risk::High, ops, undo);
            if mode == "802.3ad" {
                p.warnings.push("LACP (802.3ad) needs the other end configured as a LACP group on the same ports, or the link stays down.".into());
            }
            p.warnings.push(mgmt(&slaves.join(", ")));
            p.warnings.push("Undo puts the ports back in the bridge with their VLAN and frame types; other bridge-port settings (horizon, edge, cost) return to defaults.".into());
            p
        }
        RemoveBond { name } => {
            let name = ifname(name)?;
            let c = format!("tik:bond:{name}");
            let b = print(r, BOND, &["name", "slaves", "mode", "comment"], Some(("name", &name)))?.into_iter().next().filter(|b| b.get("comment") == Some(&c));
            let b = b.ok_or("only bonds created by tikontrol can be removed here")?;
            let slaves: Vec<String> = b.get("slaves").map(|s| s.split(',').filter(|x| !x.is_empty()).map(String::from).collect()).unwrap_or_default();
            let bp = print(r, BPORT, &["bridge", "pvid"], Some(("interface", &name)))?.into_iter().next();
            let (mut ops, mut undo) = (vec![], vec![]);
            if let Some(bp) = &bp {
                ops.push(Op::on(BPORT, "remove", "interface", &name, vec![]));
                undo.push(Op::add(BPORT, vec![("bridge", bp.get("bridge").cloned().unwrap_or_default()), ("interface", name.clone()), ("pvid", bp.get("pvid").cloned().unwrap_or_else(|| "1".into())), ("comment", c.clone())]));
            }
            ops.push(remove(BOND, &c));
            undo.insert(0, Op::add(BOND, vec![("name", name.clone()), ("slaves", slaves.join(",")), ("mode", b.get("mode").cloned().unwrap_or_else(lacp)), ("comment", c.clone())]));
            if let Some(bp) = &bp {
                for s in &slaves {
                    ops.push(Op::add(BPORT, vec![("bridge", bp.get("bridge").cloned().unwrap_or_default()), ("interface", s.clone()), ("pvid", bp.get("pvid").cloned().unwrap_or_else(|| "1".into()))]));
                    undo.insert(0, Op::on(BPORT, "remove", "interface", s, vec![]));
                }
            }
            let mut p = plan(format!("Remove bond {name}"), Risk::High, ops, undo);
            p.warnings.push(mgmt(&slaves.join(", ")));
            p
        }
        SetBridgeStp { bridge, protocol_mode } => {
            let bridge = ifname(bridge)?;
            if !STP_MODES.contains(&protocol_mode.as_str()) {
                return Err(format!("protocol_mode must be one of {}", STP_MODES.join(", ")));
            }
            let was = print(r, BRIDGE, &["protocol-mode"], Some(("name", &bridge)))?.into_iter().next().ok_or("no such bridge")?;
            let was = was.get("protocol-mode").cloned().unwrap_or_else(|| "rstp".into());
            let mut p = plan(format!("Bridge {bridge}: loop protection {protocol_mode}"), Risk::High, vec![set_on(BRIDGE, "name", &bridge, vec![("protocol-mode", protocol_mode.clone())])], vec![set_on(BRIDGE, "name", &bridge, vec![("protocol-mode", was)])]);
            p.warnings.push("All bridge ports re-converge: traffic can pause for a few seconds (up to 30 s with classic STP).".into());
            if protocol_mode == "none" {
                p.warnings.push("Without STP a cable plugged back into the same network (a loop) floods it and can take everything down.".into());
            }
            p.warnings.push(mgmt(&bridge));
            p
        }
        SetPortStp { interface, edge, bpdu_guard } => {
            let ifn = ifname(interface)?;
            if !EDGE.contains(&edge.as_str()) {
                return Err(format!("edge must be one of {}", EDGE.join(", ")));
            }
            let was = print(r, BPORT, &["edge", "bpdu-guard"], Some(("interface", &ifn)))?.into_iter().next().ok_or_else(|| format!("{ifn} is not a bridge port"))?;
            let mut p = plan(
                format!("Port {ifn}: edge {edge}, BPDU guard {}", if *bpdu_guard { "on" } else { "off" }),
                if *bpdu_guard { Risk::High } else { Risk::Medium },
                vec![set_on(BPORT, "interface", &ifn, vec![("edge", edge.clone()), ("bpdu-guard", yn(*bpdu_guard))])],
                vec![set_on(BPORT, "interface", &ifn, vec![("edge", was.get("edge").cloned().unwrap_or_else(|| "auto".into())), ("bpdu-guard", yn(truthy(was.get("bpdu-guard"))))])],
            );
            if *bpdu_guard {
                p.warnings.push(format!("BPDU guard disables {ifn} as soon as a switch or bridge is plugged into it. Use it on ports for end devices only."));
                p.warnings.push(mgmt(&ifn));
            }
            p
        }
        SetStormRate { interface, percent } => {
            let ifn = ifname(interface)?;
            if *percent > 100 {
                return Err("percent must be 0-100".into());
            }
            let was = print(r, SWITCH_PORT, &["name", "storm-rate"], Some(("name", &ifn)))?.into_iter().next();
            let was = was.and_then(|w| w.get("storm-rate").cloned()).ok_or("this router's switch chip has no storm control: RouterOS reports no storm-rate for its ports (it exists on CRS3xx switches)")?;
            let mut p = plan(format!("Port {ifn}: limit broadcast/unknown traffic to {percent}%"), Risk::Medium, vec![set_on(SWITCH_PORT, "name", &ifn, vec![("storm-rate", percent.to_string())])], vec![set_on(SWITCH_PORT, "name", &ifn, vec![("storm-rate", was)])]);
            if *percent < 5 {
                p.warnings.push("A very low storm rate also drops normal broadcasts (ARP, DHCP): devices on this port may fail to get an address.".into());
            }
            p
        }
        AddDot1xPort { interface } => {
            let ifn = ifname(interface)?;
            let radius = print(r, "/radius", &["service", "disabled"], None)?;
            if !radius.iter().any(|x| x.get("service").is_some_and(|s| s.split(',').any(|v| v == "dot1x")) && !truthy(x.get("disabled"))) {
                return Err("802.1X needs a RADIUS server with service=dot1x under /radius first; tikontrol doesn't handle RADIUS secrets".into());
            }
            if !print(r, DOT1X, &["interface"], Some(("interface", &ifn)))?.is_empty() {
                return Err(format!("{ifn} already has 802.1X"));
            }
            let c = format!("tik:dot1x:{ifn}");
            let mut p = plan(format!("Require 802.1X login on {ifn}"), Risk::High, vec![Op::add(DOT1X, vec![("interface", ifn.clone()), ("auth-types", "dot1x".into()), ("comment", c.clone())])], vec![remove(DOT1X, &c)]);
            p.warnings.push(format!("Devices on {ifn} get no network until they log in through RADIUS (printers, consoles and cameras often can't)."));
            p.warnings.push(mgmt(&ifn));
            p
        }
        RemoveDot1xPort { interface } => {
            let ifn = ifname(interface)?;
            let c = format!("tik:dot1x:{ifn}");
            if print(r, DOT1X, &["interface", "comment"], Some(("comment", &c)))?.is_empty() {
                return Err("only 802.1X set up by tikontrol can be removed here".into());
            }
            plan(format!("Stop 802.1X on {ifn}"), Risk::Medium, vec![remove(DOT1X, &c)], vec![Op::add(DOT1X, vec![("interface", ifn.clone()), ("auth-types", "dot1x".into()), ("comment", c)])])
        }
        SetDiscovery { interface_list, protocols, voice_vlan } => {
            let list = ifname(interface_list)?;
            let was = print(r, DISCOVERY, &["discover-interface-list", "protocol", "lldp-med-net-policy-vlan"], None)?.into_iter().next().unwrap_or_default();
            let mut args = vec![("discover-interface-list", list.clone())];
            let mut back = vec![("discover-interface-list", was.get("discover-interface-list").cloned().unwrap_or_else(|| "all".into()))];
            if let Some(ps) = protocols {
                if ps.iter().any(|x| !DISCOVERY_PROTOS.contains(&x.as_str())) {
                    return Err(format!("protocols: any of {}", DISCOVERY_PROTOS.join(", ")));
                }
                let old = was.get("protocol").ok_or("this RouterOS version can't choose discovery protocols (7.7+)")?;
                args.push(("protocol", ps.join(",")));
                back.push(("protocol", old.clone()));
            }
            if let Some(v) = voice_vlan {
                if *v > 4094 {
                    return Err("voice VLAN must be 1-4094 (0 = off)".into());
                }
                let old = was.get("lldp-med-net-policy-vlan").ok_or("this RouterOS version has no LLDP-MED voice VLAN")?;
                args.push(("lldp-med-net-policy-vlan", if *v == 0 { "disabled".into() } else { v.to_string() }));
                back.push(("lldp-med-net-policy-vlan", old.clone()));
            }
            let mut p = plan(format!("Neighbour discovery on {list}"), Risk::Low, vec![set(DISCOVERY, args)], vec![set(DISCOVERY, back)]);
            p.warnings.push("WinBox finds the router by MNDP: turning discovery off on the LAN only hides it from the neighbour list, access stays.".into());
            p
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ros::queries::tests::{row, Fake};
    use serde_json::json;

    #[test]
    fn ports_compile_undo_and_reject_hostile_input() {
        let mut f = Fake::default();
        f.answers.insert("/interface/ethernet/print".into(), Ok(vec![row(&[("name", "ether2")])]));
        f.answers.insert("/interface/ethernet/switch/print".into(), Ok(vec![row(&[("name", "switch1"), ("mirror-source", "none"), ("mirror-target", "none")])]));
        f.answers.insert("/interface/bridge/port/print".into(), Ok(vec![row(&[("bridge", "bridge"), ("pvid", "1"), ("frame-types", "admit-all"), ("edge", "auto"), ("bpdu-guard", "false")])]));
        // the fake answers every name lookup with ether2, so mirror source/target differ only by input
        let go = |v: serde_json::Value, f: &mut Fake| compile(&v, f).expect("own kind");
        let p = go(json!({"kind":"set-port-mirror","source":"ether2","target":"ether5"}), &mut f).unwrap();
        assert_eq!(p.cli(), [r#"/interface/ethernet/switch/set [find name="switch1"] mirror-source=ether2 mirror-target=ether5"#]);
        assert_eq!(p.undo_cli(), [r#"/interface/ethernet/switch/set [find name="switch1"] mirror-source=none mirror-target=none"#]);
        assert_eq!(p.risk, Risk::High);

        let p = go(json!({"kind":"set-port-stp","interface":"ether2","edge":"yes","bpdu_guard":true}), &mut f).unwrap();
        assert_eq!(p.undo_cli(), [r#"/interface/bridge/port/set [find interface="ether2"] edge=auto bpdu-guard=no"#]);
        assert!(p.warnings.iter().any(|w| w.contains("reaches the router through ether2")));

        f.answers.insert("/interface/print".into(), Ok(vec![]));
        let p = go(json!({"kind":"create-bond","name":"bond1","slaves":["ether2","ether3"]}), &mut f).unwrap();
        assert_eq!(p.cli()[0], r#"/interface/bridge/port/remove [find interface="ether2"]"#);
        assert!(p.cli().contains(&"/interface/bonding/add name=bond1 slaves=ether2,ether3 mode=802.3ad comment=tik:bond:bond1".into()));
        assert_eq!(p.undo_cli()[0], r#"/interface/bridge/port/remove [find comment="tik:bond:bond1"]"#);
        assert!(p.undo_cli().last().unwrap().starts_with("/interface/bridge/port/add bridge=bridge interface=ether2"));

        // switch chip without storm control / mirroring, no RADIUS: refused with a reason
        let mut bare = Fake::default();
        bare.answers.insert("/interface/ethernet/print".into(), Ok(vec![row(&[("name", "ether2")])]));
        assert!(go(json!({"kind":"set-storm-rate","interface":"ether2","percent":10}), &mut bare).unwrap_err().contains("no storm control"));
        assert!(go(json!({"kind":"set-port-mirror","source":"ether2","target":"ether3"}), &mut bare).unwrap_err().contains("no port mirroring"));
        assert!(go(json!({"kind":"add-dot1x-port","interface":"ether2"}), &mut bare).unwrap_err().contains("RADIUS"));
        for bad in [
            json!({"kind":"set-port-mirror","source":"ether2\"; /system/reboot","target":"ether5"}),
            json!({"kind":"set-port-mirror","source":"ether2","target":"ether2"}),
            json!({"kind":"create-bond","name":"bond1","slaves":["ether2"]}),
            json!({"kind":"create-bond","name":"bond1","slaves":["ether2","ether2"]}),
            json!({"kind":"create-bond","name":"bond1","slaves":["ether2","ether3"],"mode":"evil"}),
            json!({"kind":"remove-bond","name":"bond1"}),
            json!({"kind":"set-bridge-stp","bridge":"bridge","protocol_mode":"off"}),
            json!({"kind":"set-port-stp","interface":"ether2","edge":"maybe","bpdu_guard":false}),
            json!({"kind":"set-storm-rate","interface":"ether2","percent":101}),
            json!({"kind":"set-discovery","interface_list":"LAN","protocols":["ssh"]}),
            json!({"kind":"set-discovery","interface_list":"LAN","extra":1}),
        ] {
            assert!(go(bad.clone(), &mut f).is_err(), "{bad}");
        }
        assert!(compile(&json!({"kind":"reboot"}), &mut f).is_none());
    }
}
