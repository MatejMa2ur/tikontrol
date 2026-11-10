//! Switch mode (docs/FEATURES.md "Devices"): turns a small router into a plain managed switch that stays
//! reachable on the management address it already has, and `leave-switch-mode` turns it back.
//!
//! Nothing the router had is deleted, so the inverse is short (it fits the revert job) and never needs a secret
//! (a PPPoE password stays where it is): DHCP servers and clients, PPPoE clients, masquerade rules, the other
//! addresses, static default routes, WAN list members and (wifi=off) the Wi-Fi interfaces are disabled with
//! `comment=tik:switch-off|<old comment>`; ports of other bridges move over with
//! `comment=tik:switch-moved:<old bridge>|<old comment>`. What we add (bridge ports, the LAN list member, three
//! input rules) carries `tik:switch`; our default route carries `tik:switch-dns|<old /ip/dns servers>`. Those tags
//! are all `leave-switch-mode` needs to put the router back exactly, long after the change was kept.

use super::networks::{ifname, print, truthy};
use crate::apply::{cidr, ipv4, plan, revert_script, Op, Plan, Risk};
use crate::ros::conn::{Router, Row};
use serde::Deserialize;
use std::net::Ipv4Addr;

pub const KINDS: &[&str] = &["switch-mode", "leave-switch-mode"];

const ETHER: &str = "/interface/ethernet";
const BRIDGE: &str = "/interface/bridge";
const BPORT: &str = "/interface/bridge/port";
const LIST: &str = "/interface/list";
const MEMBER: &str = "/interface/list/member";
const FILTER: &str = "/ip/firewall/filter";
const ROUTE: &str = "/ip/route";
const DNS: &str = "/ip/dns";
const ADDR: &str = "/ip/address";
const WIRELESS: &str = "/interface/wireless";
const WIFI: &str = "/interface/wifi";

const TAG: &str = "tik:switch";
const OFF: &str = "tik:switch-off|";
const MOVED: &str = "tik:switch-moved:";
const ROUTE_TAG: &str = "tik:switch-dns|";

/// What switch mode turns off: (path, fields shown in the preview). Which rows: see `turned_off`.
/// The last two are Wi-Fi (legacy `wireless`, or the newer `wifi` package): only with wifi=off.
const OFF_PATHS: &[(&str, &[&str])] = &[
    ("/ip/dhcp-server", &["name", "interface"]),
    ("/ip/dhcp-client", &["interface"]),
    ("/interface/pppoe-client", &["name", "interface"]),
    ("/ip/firewall/nat", &["chain", "action", "out-interface-list", "out-interface"]),
    (ADDR, &["address", "interface", "dynamic"]),
    (ROUTE, &["dst-address", "gateway", "dynamic"]),
    (MEMBER, &["list", "interface", "dynamic"]),
    (WIRELESS, &["name"]),
    (WIFI, &["name"]),
];

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum S {
    SwitchMode { bridge: String, address: String, gateway: String, dns: String, wifi: String },
    LeaveSwitchMode {},
}

pub fn compile(v: &serde_json::Value, r: &mut dyn Router) -> Option<Result<Plan, String>> {
    let kind = v.get("kind")?.as_str()?;
    if !KINDS.contains(&kind) {
        return None;
    }
    Some(serde_json::from_value::<S>(v.clone()).map_err(|e| format!("invalid {kind} intent: {e}")).and_then(|i| build(&i, r)))
}

fn set_id(path: &'static str, id: &str, args: Vec<(&'static str, String)>) -> Op {
    Op::on(path, "set", ".id", id, args)
}

fn get<'a>(row: &'a Row, k: &str) -> &'a str {
    row.get(k).map_or("", String::as_str)
}

/// Rows of an optional menu (no Wi-Fi or PPPoE package: none).
fn rows(r: &mut dyn Router, path: &str, fields: &[&str]) -> Result<Vec<Row>, String> {
    let props: Vec<&str> = [".id", "comment", "disabled"].iter().chain(fields).copied().collect();
    let got = print(r, path, &props, None);
    if path == WIRELESS || path == WIFI || path == "/interface/pppoe-client" {
        return Ok(got.unwrap_or_default());
    }
    got
}

/// Does switch mode turn this row off?
fn turned_off(path: &str, row: &Row, wifi_off: bool, bridge: &str, mgmt: &str) -> bool {
    if truthy(row.get("disabled")) || truthy(row.get("dynamic")) {
        return false;
    }
    match path {
        "/ip/firewall/nat" => get(row, "chain") == "srcnat" && get(row, "action") == "masquerade",
        ADDR => !(get(row, "interface") == bridge && get(row, "address") == mgmt),
        ROUTE => get(row, "dst-address") == "0.0.0.0/0",
        MEMBER => get(row, "list") == "WAN",
        WIRELESS | WIFI => wifi_off,
        _ => true,
    }
}

fn describe(path: &str, row: &Row, fields: &[&str]) -> String {
    let what: Vec<String> = fields.iter().filter(|k| **k != "dynamic").filter_map(|k| row.get(*k).filter(|v| !v.is_empty()).map(|v| format!("{k}={v}"))).collect();
    format!("{path} {}", what.join(" "))
}

/// The input rules switch mode adds (in this order) for the LAN subnet.
fn input_rules(subnet: &str) -> Vec<Vec<(&'static str, String)>> {
    vec![
        vec![("chain", "input".into()), ("action", "accept".into()), ("connection-state", "established,related,untracked".into())],
        vec![("chain", "input".into()), ("action", "accept".into()), ("src-address", subnet.to_string())],
        vec![("chain", "input".into()), ("action", "drop".into())],
    ]
}

fn build(i: &S, r: &mut dyn Router) -> Result<Plan, String> {
    let p = match i {
        S::SwitchMode { bridge, address, gateway, dns, wifi } => switch_mode(r, bridge, address, gateway, dns, wifi)?,
        S::LeaveSwitchMode {} => leave(r)?,
    };
    // the inverse runs from the router's own revert job, whose script has a length limit
    if revert_script("check", &p).is_err() {
        return Err("this router has too much to turn off for one revertible change (the automatic revert would be too long): turn some of it off by hand first (old addresses, DHCP servers)".into());
    }
    Ok(p)
}

fn switch_mode(r: &mut dyn Router, bridge: &str, address: &str, gateway: &str, dns: &str, wifi: &str) -> Result<Plan, String> {
    // ---- validate everything first ----
    let bridge = ifname(bridge)?;
    let mgmt = cidr(address)?;
    let (ip, len) = mgmt.split_once('/').ok_or("invalid address")?;
    let (ip, len): (Ipv4Addr, u8) = (ipv4(ip)?, len.parse().map_err(|_| "invalid address")?);
    if !(8..=30).contains(&len) {
        return Err("address needs a prefix /8 to /30, e.g. 192.168.88.2/24".into());
    }
    if !ip.is_private() {
        return Err("the management address must be private (10/8, 172.16/12, 192.168/16)".into());
    }
    let mask = u32::MAX << (32 - len);
    let net = u32::from(ip) & mask;
    if u32::from(ip) == net || u32::from(ip) == net | !mask {
        return Err("the management address can't be the network or broadcast address".into());
    }
    let gw = ipv4(gateway)?;
    if u32::from(gw) & mask != net || gw == ip || u32::from(gw) == net || u32::from(gw) == net | !mask {
        return Err(format!("the gateway must be another address in {}/{len}", Ipv4Addr::from(net)));
    }
    let dns = ipv4(dns)?;
    if dns.is_unspecified() || dns.is_broadcast() || dns.is_multicast() || dns.is_loopback() {
        return Err("invalid DNS server".into());
    }
    let wifi_off = match wifi {
        "off" => true,
        "on" => false,
        _ => return Err("wifi must be on or off".into()),
    };
    let subnet = format!("{}/{len}", Ipv4Addr::from(net));

    // ---- read the router ----
    if !print(r, BRIDGE, &["name"], None)?.iter().any(|b| get(b, "name") == bridge) {
        return Err(format!("there is no bridge {bridge} on this router"));
    }
    let addrs = rows(r, ADDR, &["address", "interface", "dynamic"])?;
    // tikontrol keeps talking to the router on this address: it must already be there, or the change cuts it off
    if !addrs.iter().any(|a| get(a, "address") == mgmt && get(a, "interface") == bridge && !truthy(a.get("disabled"))) {
        return Err(format!("{mgmt} must already be an address on {bridge}: switch mode never changes the address tikontrol and you manage the router through. Add it first, connect through it, then try again."));
    }
    let tagged = |row: &Row| get(row, "comment").starts_with(TAG);
    if rows(r, BPORT, &[])?.iter().chain(&rows(r, FILTER, &[])?).any(tagged) {
        return Err("this router is already in switch mode".into());
    }

    let (mut ops, mut undo, mut off_list) = (vec![], vec![], vec![]);

    // 1. input firewall, accepts before the drop and all before the existing input rules (defconf's stay, unreachable)
    let anchor = rows(r, FILTER, &["chain"])?.into_iter().find(|x| get(x, "chain") == "input").and_then(|x| x.get(".id").cloned());
    for mut args in input_rules(&subnet) {
        args.push(("comment", TAG.into()));
        if let Some(a) = &anchor {
            args.push(("place-before", a.clone()));
        }
        ops.push(Op::add(FILTER, args));
    }
    undo.push(Op::on(FILTER, "remove", "comment", TAG, vec![]));

    // 2. the bridge in LAN, so neighbour discovery and MAC access (defconf: LAN list) keep working
    if print(r, LIST, &["name"], None)?.iter().any(|l| get(l, "name") == "LAN") && !rows(r, MEMBER, &["list", "interface"])?.iter().any(|m| get(m, "list") == "LAN" && get(m, "interface") == bridge) {
        ops.push(Op::add(MEMBER, vec![("list", "LAN".into()), ("interface", bridge.clone()), ("comment", TAG.into())]));
        undo.insert(0, Op::on(MEMBER, "remove", "comment", TAG, vec![]));
    }

    // 3. every ethernet port into the bridge
    let ports = rows(r, BPORT, &["interface", "bridge", "dynamic"])?;
    let slaves = print(r, "/interface", &["name", "slave"], None)?;
    let mut added = false;
    for e in print(r, ETHER, &["name"], None)? {
        let e = get(&e, "name").to_string();
        match ports.iter().find(|p| get(p, "interface") == e) {
            Some(p) if get(p, "bridge") == bridge => {}
            Some(p) if truthy(p.get("dynamic")) => return Err(format!("{e} is a dynamic port of {}: remove what adds it first", get(p, "bridge"))),
            Some(p) => {
                let (id, old, c) = (get(p, ".id"), get(p, "bridge"), get(p, "comment"));
                ops.push(set_id(BPORT, id, vec![("bridge", bridge.clone()), ("comment", format!("{MOVED}{old}|{c}"))]));
                undo.insert(0, set_id(BPORT, id, vec![("bridge", old.to_string()), ("comment", c.to_string())]));
                off_list.push(format!("{e} moves from {old} to {bridge}"));
            }
            None if slaves.iter().any(|s| get(s, "name") == e && truthy(s.get("slave"))) => return Err(format!("{e} is part of a bond or another interface: take it out first")),
            None => {
                ops.push(Op::add(BPORT, vec![("bridge", bridge.clone()), ("interface", e.clone()), ("comment", TAG.into())]));
                added = true;
                off_list.push(format!("{e} joins {bridge}"));
            }
        }
    }
    if added {
        undo.insert(0, Op::on(BPORT, "remove", "comment", TAG, vec![]));
    }

    // 4. its own way out: default route via the gateway, DNS
    let old_dns = print(r, DNS, &["servers"], None)?.first().map(|d| get(d, "servers").to_string()).unwrap_or_default();
    let route_tag = format!("{ROUTE_TAG}{old_dns}");
    ops.push(Op::add(ROUTE, vec![("dst-address", "0.0.0.0/0".into()), ("gateway", gw.to_string()), ("comment", route_tag.clone())]));
    undo.insert(0, Op::on(ROUTE, "remove", "comment", &route_tag, vec![]));
    ops.push(Op { path: DNS, verb: "set", find: None, args: vec![("servers", dns.to_string())] });
    undo.insert(0, Op { path: DNS, verb: "set", find: None, args: vec![("servers", old_dns)] });

    // 5. turn the router parts off, last (the new route is already there)
    for (path, fields) in OFF_PATHS {
        for row in rows(r, path, fields)?.iter().filter(|x| turned_off(path, x, wifi_off, &bridge, &mgmt)) {
            let (id, c) = (get(row, ".id"), get(row, "comment"));
            ops.push(set_id(path, id, vec![("disabled", "yes".into()), ("comment", format!("{OFF}{c}"))]));
            undo.insert(0, set_id(path, id, vec![("disabled", "no".into()), ("comment", c.to_string())]));
            off_list.push(format!("off: {}", describe(path, row, fields)));
        }
    }

    let mut p = plan(format!("Use as a switch: all ports on {bridge}, managed at {mgmt}"), Risk::High, ops, undo);
    p.warnings = vec![
        "High risk: the router stops routing. No more DHCP, NAT or internet connection of its own; the upstream router serves the network.".into(),
        format!("Every ethernet port joins {bridge}. Unplug the internet/modem cable from the old WAN port first, or the ISP's network ends up on your LAN."),
        "Firewall rules and other settings that name a port (in-interface=ether1) stop matching once the port is in the bridge: use the bridge instead.".into(),
        format!("Management stays on {mgmt}; afterwards the router answers only devices in {subnet} (new connections from elsewhere, e.g. over a VPN, are dropped)."),
        format!("Nothing is deleted: turned-off items keep their settings, tagged {OFF}…, and \"Leave switch mode\" turns them back on."),
        format!("What changes: {}", if off_list.is_empty() { "only the firewall, route and DNS".into() } else { off_list.join("; ") }),
    ];
    Ok(p)
}

fn leave(r: &mut dyn Router) -> Result<Plan, String> {
    let (mut ops, mut undo) = (vec![], vec![]);
    // forward order reversed: router parts back on first, our firewall rules go last
    for (path, fields) in OFF_PATHS.iter().rev() {
        for row in rows(r, path, fields)? {
            let (id, c) = (get(&row, ".id"), get(&row, "comment"));
            if let Some(old) = c.strip_prefix(OFF) {
                ops.push(set_id(path, id, vec![("disabled", "no".into()), ("comment", old.to_string())]));
                undo.insert(0, set_id(path, id, vec![("disabled", "yes".into()), ("comment", c.to_string())]));
            }
        }
    }
    let servers = print(r, DNS, &["servers"], None)?.first().map(|d| get(d, "servers").to_string()).unwrap_or_default();
    for route in rows(r, ROUTE, &["dst-address", "gateway"])? {
        if let Some(old) = get(&route, "comment").strip_prefix(ROUTE_TAG) {
            ops.push(Op { path: DNS, verb: "set", find: None, args: vec![("servers", old.to_string())] });
            undo.insert(0, Op { path: DNS, verb: "set", find: None, args: vec![("servers", servers.clone())] });
            let c = get(&route, "comment");
            ops.push(Op::on(ROUTE, "remove", "comment", c, vec![]));
            undo.insert(0, Op::add(ROUTE, vec![("dst-address", get(&route, "dst-address").into()), ("gateway", get(&route, "gateway").into()), ("comment", c.into())]));
        }
    }
    let ports = rows(r, BPORT, &["interface", "bridge"])?;
    for p in &ports {
        let (id, c) = (get(p, ".id"), get(p, "comment"));
        if let Some((old, orig)) = c.strip_prefix(MOVED).and_then(|x| x.split_once('|')) {
            ops.push(set_id(BPORT, id, vec![("bridge", ifname(old)?), ("comment", orig.to_string())]));
            undo.insert(0, set_id(BPORT, id, vec![("bridge", get(p, "bridge").into()), ("comment", c.to_string())]));
        }
    }
    let ours = |path: &'static str, rows: &[Row], fields: &[&'static str], ops: &mut Vec<Op>, undo: &mut Vec<Op>| {
        let mine: Vec<&Row> = rows.iter().filter(|x| get(x, "comment") == TAG).collect();
        if !mine.is_empty() {
            ops.push(Op::on(path, "remove", "comment", TAG, vec![]));
            for (i, row) in mine.iter().enumerate() {
                let args = fields.iter().filter(|k| !get(row, k).is_empty()).map(|k| (*k, get(row, k).to_string())).chain([("comment", TAG.to_string())]);
                undo.insert(i, Op::add(path, args.collect()));
            }
        }
    };
    ours(BPORT, &ports, &["bridge", "interface"], &mut ops, &mut undo);
    ours(MEMBER, &rows(r, MEMBER, &["list", "interface"])?, &["list", "interface"], &mut ops, &mut undo);
    // our input rules go back before the first input rule that isn't ours
    let filter = rows(r, FILTER, &["chain", "action", "connection-state", "src-address"])?;
    let n = undo.len();
    ours(FILTER, &filter, &["chain", "action", "connection-state", "src-address"], &mut ops, &mut undo);
    if let Some(a) = filter.iter().find(|x| get(x, "chain") == "input" && get(x, "comment") != TAG).and_then(|x| x.get(".id")) {
        let k = undo.len() - n;
        for op in &mut undo[..k] {
            op.args.insert(op.args.len() - 1, ("place-before", a.clone()));
        }
    }
    if ops.is_empty() {
        return Err("this router is not in switch mode (nothing tagged tik:switch)".into());
    }
    let mut p = plan("Leave switch mode: back to a router".into(), Risk::High, ops, undo);
    p.warnings = vec![
        "High risk: the router routes again, with its DHCP servers, NAT, addresses and WAN port back as they were before switch mode.".into(),
        "Ports that joined the bridge leave it: if you manage the router through one of them (the old WAN port), you can lose access until the change reverts.".into(),
        "Two DHCP servers on one network hand out conflicting addresses: put the cables back the router way before keeping this.".into(),
    ];
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ros::conn::Row;
    use crate::ros::queries::tests::{row, Fake};
    use serde_json::json;

    fn intent() -> serde_json::Value {
        json!({"kind":"switch-mode","bridge":"bridge","address":"192.168.88.2/24","gateway":"192.168.88.1","dns":"192.168.88.1","wifi":"off"})
    }

    /// A hAP-lite-like router: defconf bridge with ether2-4, ether1 WAN with a DHCP client, one Wi-Fi radio.
    fn hap_lite() -> Fake {
        let mut f = Fake::default();
        let a = |f: &mut Fake, p: &str, rows: Vec<Row>| {
            f.answers.insert(format!("{p}/print"), Ok(rows));
        };
        a(&mut f, BRIDGE, vec![row(&[("name", "bridge")]), row(&[("name", "bridge2")])]);
        a(&mut f, ETHER, ["ether1", "ether2", "ether3", "ether4"].iter().map(|n| row(&[("name", n)])).collect());
        a(&mut f, "/interface", vec![row(&[("name", "ether1"), ("slave", "false")])]);
        a(&mut f, BPORT, vec![
            row(&[(".id", "*1"), ("interface", "ether2"), ("bridge", "bridge"), ("comment", "defconf"), ("disabled", "false")]),
            row(&[(".id", "*2"), ("interface", "ether3"), ("bridge", "bridge2"), ("comment", "defconf"), ("disabled", "false")]),
            row(&[(".id", "*3"), ("interface", "ether4"), ("bridge", "bridge"), ("disabled", "false")]),
        ]);
        a(&mut f, LIST, vec![row(&[("name", "WAN")]), row(&[("name", "LAN")])]);
        a(&mut f, MEMBER, vec![
            row(&[(".id", "*A"), ("list", "LAN"), ("interface", "bridge"), ("comment", "defconf"), ("disabled", "false")]),
            row(&[(".id", "*B"), ("list", "WAN"), ("interface", "ether1"), ("comment", "defconf"), ("disabled", "false")]),
        ]);
        a(&mut f, ADDR, vec![
            row(&[(".id", "*4"), ("address", "192.168.88.1/24"), ("interface", "bridge"), ("comment", "defconf"), ("disabled", "false"), ("dynamic", "false")]),
            row(&[(".id", "*5"), ("address", "192.168.88.2/24"), ("interface", "bridge"), ("disabled", "false"), ("dynamic", "false")]),
            row(&[(".id", "*6"), ("address", "10.0.2.15/24"), ("interface", "ether1"), ("disabled", "false"), ("dynamic", "true")]),
        ]);
        a(&mut f, "/ip/dhcp-server", vec![row(&[(".id", "*7"), ("name", "defconf"), ("interface", "bridge"), ("disabled", "false")])]);
        a(&mut f, "/ip/dhcp-client", vec![row(&[(".id", "*8"), ("interface", "ether1"), ("comment", "defconf"), ("disabled", "false")])]);
        a(&mut f, "/ip/firewall/nat", vec![
            row(&[(".id", "*9"), ("chain", "srcnat"), ("action", "masquerade"), ("comment", "defconf: masquerade"), ("disabled", "false")]),
            row(&[(".id", "*10"), ("chain", "dstnat"), ("action", "dst-nat"), ("disabled", "false")]),
        ]);
        a(&mut f, FILTER, vec![row(&[(".id", "*20"), ("chain", "forward")]), row(&[(".id", "*21"), ("chain", "input")])]);
        a(&mut f, ROUTE, vec![row(&[(".id", "*30"), ("dst-address", "0.0.0.0/0"), ("gateway", "10.0.2.2"), ("dynamic", "true"), ("disabled", "false")])]);
        a(&mut f, DNS, vec![row(&[("servers", "")])]);
        a(&mut f, WIRELESS, vec![row(&[(".id", "*40"), ("name", "wlan1"), ("disabled", "false")])]);
        f.answers.insert("/interface/wifi/print".into(), Err("no such command prefix".into()));
        f
    }

    fn go(v: serde_json::Value, f: &mut Fake) -> Result<Plan, String> {
        compile(&v, f).expect("own kind")
    }

    #[test]
    fn switch_mode_on_a_hap_lite() {
        let p = go(intent(), &mut hap_lite()).unwrap();
        assert_eq!(p.risk, Risk::High);
        let cli = p.cli();
        // the accepts come before the drop, all before the first existing input rule
        assert_eq!(cli[0], "/ip/firewall/filter/add chain=input action=accept connection-state=established,related,untracked comment=tik:switch place-before=*21");
        assert_eq!(cli[1], "/ip/firewall/filter/add chain=input action=accept src-address=192.168.88.0/24 comment=tik:switch place-before=*21");
        assert_eq!(cli[2], "/ip/firewall/filter/add chain=input action=drop comment=tik:switch place-before=*21");
        let want = [
            "/interface/bridge/port/add bridge=bridge interface=ether1 comment=tik:switch",
            r#"/interface/bridge/port/set [find .id="*2"] bridge=bridge comment="tik:switch-moved:bridge2|defconf""#,
            "/ip/route/add dst-address=0.0.0.0/0 gateway=192.168.88.1 comment=\"tik:switch-dns|\"",
            "/ip/dns/set servers=192.168.88.1",
            r#"/ip/dhcp-server/set [find .id="*7"] disabled=yes comment="tik:switch-off|""#,
            r#"/ip/dhcp-client/set [find .id="*8"] disabled=yes comment="tik:switch-off|defconf""#,
            r#"/ip/firewall/nat/set [find .id="*9"] disabled=yes comment="tik:switch-off|defconf: masquerade""#,
            r#"/ip/address/set [find .id="*4"] disabled=yes comment="tik:switch-off|defconf""#,
            r#"/interface/list/member/set [find .id="*B"] disabled=yes comment="tik:switch-off|defconf""#,
            r#"/interface/wireless/set [find .id="*40"] disabled=yes comment="tik:switch-off|""#,
        ];
        assert_eq!(&cli[3..], want, "{cli:#?}");
        // never the management address, a dynamic one, a dstnat rule or the dynamic DHCP route
        assert!(!cli.iter().any(|c| c.contains("\"*5\"") || c.contains("\"*6\"") || c.contains("\"*10\"") || c.contains("\"*30\"")));
        let undo = p.undo_cli();
        assert_eq!(undo[0], r#"/interface/wireless/set [find .id="*40"] disabled=no comment="""#);
        assert!(undo.contains(&r#"/interface/bridge/port/set [find .id="*2"] bridge=bridge2 comment=defconf"#.to_string()));
        assert!(undo.contains(&"/ip/dns/set servers=\"\"".to_string()));
        assert_eq!(undo.last().unwrap(), r#"/ip/firewall/filter/remove [find comment="tik:switch"]"#);
        assert_eq!(p.undo.iter().filter(|o| o.path == BPORT && o.verb == "remove").count(), 1);
        assert!(p.warnings.iter().any(|w| w.contains("Unplug")));
        // wifi=on leaves the radio alone
        let mut v = intent();
        v["wifi"] = json!("on");
        assert!(!go(v, &mut hap_lite()).unwrap().cli().iter().any(|c| c.contains("wireless")));
    }

    #[test]
    fn switch_mode_refuses_what_it_cant_do_safely() {
        let bad = [
            json!({"kind":"switch-mode","bridge":"bridge","address":"8.8.8.2/24","gateway":"8.8.8.1","dns":"8.8.8.8","wifi":"off"}),
            json!({"kind":"switch-mode","bridge":"bridge","address":"192.168.88.2/24","gateway":"192.168.89.1","dns":"192.168.88.1","wifi":"off"}),
            json!({"kind":"switch-mode","bridge":"bridge","address":"192.168.88.2/24","gateway":"192.168.88.2","dns":"192.168.88.1","wifi":"off"}),
            json!({"kind":"switch-mode","bridge":"bridge","address":"192.168.88.0/24","gateway":"192.168.88.1","dns":"192.168.88.1","wifi":"off"}),
            json!({"kind":"switch-mode","bridge":"bridge","address":"192.168.88.2/24","gateway":"192.168.88.1","dns":"x","wifi":"off"}),
            json!({"kind":"switch-mode","bridge":"bridge","address":"192.168.88.2/24","gateway":"192.168.88.1","dns":"192.168.88.1","wifi":"maybe"}),
            json!({"kind":"switch-mode","bridge":"bridge\"; /system/reboot","address":"192.168.88.2/24","gateway":"192.168.88.1","dns":"192.168.88.1","wifi":"off"}),
            json!({"kind":"switch-mode","bridge":"bridge","address":"192.168.88.2/24","gateway":"192.168.88.1","dns":"192.168.88.1","wifi":"off","x":1}),
        ];
        for b in bad {
            assert!(go(b.clone(), &mut hap_lite()).is_err(), "{b}");
        }
        // no such bridge; the management address not (yet) on the bridge
        let mut v = intent();
        v["bridge"] = json!("bridge9");
        assert!(go(v, &mut hap_lite()).unwrap_err().contains("no bridge"));
        let mut v = intent();
        v["address"] = json!("192.168.88.3/24");
        assert!(go(v, &mut hap_lite()).unwrap_err().contains("must already be an address"));
        // a bond member
        let mut f = hap_lite();
        f.answers.insert("/interface/print".into(), Ok(vec![row(&[("name", "ether1"), ("slave", "true")])]));
        assert!(go(intent(), &mut f).unwrap_err().contains("bond"));
        // twice
        let mut f = hap_lite();
        f.answers.insert("/ip/firewall/filter/print".into(), Ok(vec![row(&[(".id", "*1"), ("chain", "input"), ("comment", "tik:switch")])]));
        assert!(go(intent(), &mut f).unwrap_err().contains("already"));
        // too much to fit the revert job: refused before anything runs
        let mut f = hap_lite();
        let many = (0..60).map(|i| row(&[(".id", &format!("*{i}")), ("address", &format!("10.{i}.0.1/24")), ("interface", "ether1"), ("comment", "an address someone added by hand"), ("disabled", "false")])).collect::<Vec<_>>();
        let mut all = many;
        all.push(row(&[(".id", "*5"), ("address", "192.168.88.2/24"), ("interface", "bridge"), ("disabled", "false")]));
        f.answers.insert("/ip/address/print".into(), Ok(all));
        assert!(go(intent(), &mut f).unwrap_err().contains("too much"));
    }

    #[test]
    fn leave_switch_mode_restores_from_the_tags() {
        let mut f = Fake::default();
        assert!(go(json!({"kind":"leave-switch-mode"}), &mut f).unwrap_err().contains("not in switch mode"));
        f.answers.insert("/ip/firewall/nat/print".into(), Ok(vec![row(&[(".id", "*9"), ("chain", "srcnat"), ("action", "masquerade"), ("comment", "tik:switch-off|defconf: masquerade"), ("disabled", "true")])]));
        f.answers.insert("/ip/route/print".into(), Ok(vec![row(&[(".id", "*31"), ("dst-address", "0.0.0.0/0"), ("gateway", "192.168.88.1"), ("comment", "tik:switch-dns|9.9.9.9"), ("disabled", "false")])]));
        f.answers.insert("/ip/dns/print".into(), Ok(vec![row(&[("servers", "192.168.88.1")])]));
        f.answers.insert("/interface/bridge/port/print".into(), Ok(vec![
            row(&[(".id", "*2"), ("interface", "ether3"), ("bridge", "bridge"), ("comment", "tik:switch-moved:bridge2|defconf")]),
            row(&[(".id", "*7"), ("interface", "ether1"), ("bridge", "bridge"), ("comment", "tik:switch")]),
        ]));
        f.answers.insert("/ip/firewall/filter/print".into(), Ok(vec![
            row(&[(".id", "*50"), ("chain", "input"), ("action", "accept"), ("connection-state", "established,related,untracked"), ("comment", "tik:switch")]),
            row(&[(".id", "*51"), ("chain", "input"), ("action", "drop"), ("comment", "tik:switch")]),
            row(&[(".id", "*21"), ("chain", "input"), ("action", "accept"), ("comment", "defconf")]),
        ]));
        let p = go(json!({"kind":"leave-switch-mode"}), &mut f).unwrap();
        let cli = p.cli();
        assert_eq!(cli[0], r#"/ip/firewall/nat/set [find .id="*9"] disabled=no comment="defconf: masquerade""#);
        assert!(cli.contains(&"/ip/dns/set servers=9.9.9.9".to_string()));
        assert!(cli.contains(&r#"/interface/bridge/port/set [find .id="*2"] bridge=bridge2 comment=defconf"#.to_string()));
        assert_eq!(cli.last().unwrap(), r#"/ip/firewall/filter/remove [find comment="tik:switch"]"#);
        let undo = p.undo_cli();
        // its own revert puts our rules back first, in order, before defconf's
        assert_eq!(undo[0], "/ip/firewall/filter/add chain=input action=accept connection-state=established,related,untracked place-before=*21 comment=tik:switch");
        assert_eq!(undo[1], "/ip/firewall/filter/add chain=input action=drop place-before=*21 comment=tik:switch");
        assert!(undo.contains(&r#"/ip/firewall/nat/set [find .id="*9"] disabled=yes comment="tik:switch-off|defconf: masquerade""#.to_string()));
        assert!(undo.contains(&"/ip/route/add dst-address=0.0.0.0/0 gateway=192.168.88.1 comment=\"tik:switch-dns|9.9.9.9\"".to_string()));
        assert!(undo.contains(&"/ip/dns/set servers=192.168.88.1".to_string()));
    }

    #[test]
    fn every_kind_is_registered() {
        for k in KINDS {
            if let Err(e) = crate::apply::compile_value(&json!({"kind": k}), &mut Fake::default()) {
                assert!(!e.contains("unknown variant") && !e.contains("unknown kind"), "{k}: {e}");
            }
        }
    }
}
