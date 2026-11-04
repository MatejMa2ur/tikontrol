//! Networks and ports (docs/FEATURES.md "Networks", "Ports", "Profiles: Port profiles").
//!
//! `create-network` is PLAN §0.6 rule 1: one form (name, VLAN, subnet, DHCP, isolation) becomes the
//! VLAN interface, bridge VLAN entry, address, pool, DHCP server and network, interface-list member and
//! isolation rules, all tagged `tik:net:<name>` so the inverse removes exactly them, in reverse order.
//! Bridge VLAN filtering is never switched on as a side effect (docs/FEASIBILITY.md: lockout); it is its
//! own high-risk intent, `set-vlan-filtering`.

use crate::apply::{cidr, ipv4, plan, ros_name, yn, Op, Plan, Risk};
use crate::ros::cmd::Cmd;
use crate::ros::conn::{Router, Row};
use serde::Deserialize;
use std::net::Ipv4Addr;

const VLAN_IF: &str = "/interface/vlan";
const BVLAN: &str = "/interface/bridge/vlan";
const BRIDGE: &str = "/interface/bridge";
const BPORT: &str = "/interface/bridge/port";
const ADDR: &str = "/ip/address";
const LIST: &str = "/interface/list";
const MEMBER: &str = "/interface/list/member";
const POOL: &str = "/ip/pool";
const DHCPS: &str = "/ip/dhcp-server";
const DHCPN: &str = "/ip/dhcp-server/network";
const DHCPO: &str = "/ip/dhcp-server/option";
pub(crate) const FILTER: &str = "/ip/firewall/filter";

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum N {
    CreateNetwork {
        name: String,
        vlan: u16,
        subnet: String,
        #[serde(default = "yes")]
        dhcp: bool,
        /// "10.0.30.10-10.0.30.200"; empty = .10 to the last host
        #[serde(default)]
        range: String,
        #[serde(default)]
        lease_time: String,
        #[serde(default)]
        dns: Vec<String>,
        #[serde(default)]
        domain: String,
        #[serde(default)]
        isolated: bool,
        #[serde(default)]
        guest: bool,
        #[serde(default = "yes")]
        internet: bool,
        /// interface list for the new VLAN; empty = LAN
        #[serde(default)]
        list: String,
        #[serde(default)]
        bridge: String,
    },
    RemoveNetwork { name: String },
    SetVlanFiltering { bridge: String, enabled: bool },
    SetDhcpLease { server: String, lease_time: String },
    SetDhcpRange { pool: String, range: String },
    SetDhcpNetwork {
        address: String,
        #[serde(default)]
        gateway: String,
        #[serde(default)]
        dns: Vec<String>,
    },
    DhcpOption {
        name: String,
        code: u8,
        /// text | ip | hex
        value_type: String,
        value: String,
        /// optional DHCP network (cidr) to attach the option to
        #[serde(default)]
        network: String,
    },
    RemoveDhcpOption { name: String },
    SetMdnsRepeat { interfaces: Vec<String> },
    SetIgmpSnooping { bridge: String, enabled: bool },
    SetPortEnabled { interface: String, enabled: bool },
    SetBridgePort { interface: String, pvid: u16, frame_types: String },
    PortProfile {
        interface: String,
        #[serde(default = "pvid1")]
        pvid: u16,
        #[serde(default)]
        tagged: Vec<u16>,
    },
    SetPoe { interface: String, mode: String },
}

fn yes() -> bool {
    true
}
fn pvid1() -> u16 {
    1
}

const KINDS: &[&str] = &[
    "create-network",
    "remove-network",
    "set-vlan-filtering",
    "set-dhcp-lease",
    "set-dhcp-range",
    "set-dhcp-network",
    "dhcp-option",
    "remove-dhcp-option",
    "set-mdns-repeat",
    "set-igmp-snooping",
    "set-port-enabled",
    "set-bridge-port",
    "port-profile",
    "set-poe",
];

pub fn compile(v: &serde_json::Value, r: &mut dyn Router) -> Option<Result<Plan, String>> {
    let kind = v.get("kind")?.as_str()?;
    if !KINDS.contains(&kind) {
        return None;
    }
    Some(serde_json::from_value::<N>(v.clone()).map_err(|e| format!("invalid {kind} intent: {e}")).and_then(|i| build(&i, r)))
}

// ---- shared helpers (also used by `security`) ----

pub(crate) fn truthy(v: Option<&String>) -> bool {
    v.is_some_and(|s| s == "true" || s == "yes")
}

/// Short identifier used inside RouterOS names: letters, digits, - and _.
pub(crate) fn slug(s: &str) -> Result<String, String> {
    let s = s.trim();
    let ok = !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    ok.then(|| s.to_string()).ok_or_else(|| format!("invalid name {s:?} (letters, digits, - and _, at most 32)"))
}

/// Interface / list / bridge names as RouterOS shows them.
pub(crate) fn ifname(s: &str) -> Result<String, String> {
    let s = s.trim();
    let ok = !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b));
    ok.then(|| s.to_string()).ok_or_else(|| format!("invalid interface name {s:?}"))
}

/// RouterOS duration: `30m`, `1d`, `1d2h`, `00:10:00`.
pub(crate) fn duration(s: &str) -> Result<String, String> {
    let s = s.trim();
    let ok = !s.is_empty() && s.len() <= 16 && s.as_bytes()[0].is_ascii_digit() && s.bytes().all(|b| b.is_ascii_digit() || b"wdhms:".contains(&b));
    ok.then(|| s.to_string()).ok_or_else(|| format!("invalid duration {s:?} (e.g. 1d, 30m)"))
}

pub(crate) fn print(r: &mut dyn Router, path: &str, props: &[&str], q: Option<(&str, &str)>) -> Result<Vec<Row>, String> {
    let mut c = Cmd::new(&format!("{path}/print")).proplist(props);
    if let Some((k, v)) = q {
        c = c.query(k, v);
    }
    r.run(&c)
}

/// `.id` of the first rule of a chain, to place ours before FastTrack and "accept established".
pub(crate) fn first_rule(r: &mut dyn Router, path: &str, chain: &str) -> Result<Option<String>, String> {
    Ok(print(r, path, &[".id"], Some(("chain", chain)))?.first().and_then(|x| x.get(".id")).cloned())
}

pub(crate) fn remove(path: &'static str, comment: &str) -> Op {
    Op::on(path, "remove", "comment", comment, vec![])
}

/// Re-creates the rows tagged `comment` on `path` (the inverse of removing them).
pub(crate) fn rebuild(r: &mut dyn Router, path: &'static str, fields: &[&'static str], comment: &str) -> Result<Vec<Op>, String> {
    let rows = print(r, path, fields, Some(("comment", comment)))?;
    Ok(rows
        .iter()
        .map(|row| Op::add(path, fields.iter().filter_map(|k| row.get(*k).filter(|v| !v.is_empty()).map(|v| (*k, v.clone()))).chain([("comment", comment.to_string())]).collect()))
        .collect())
}

/// Undo for a list of adds: remove each tagged path once, in reverse order.
pub(crate) fn undo_adds(ops: &[Op], comment: &str) -> Vec<Op> {
    let mut seen: Vec<&'static str> = vec![];
    for op in ops.iter().rev() {
        if op.verb == "add" && !seen.contains(&op.path) {
            seen.push(op.path);
        }
    }
    seen.into_iter().map(|p| remove(p, comment)).collect()
}

fn ip(a: u32) -> String {
    Ipv4Addr::from(a).to_string()
}

/// Private network address with prefix /16../29 → (network, prefix).
fn subnet(s: &str) -> Result<(u32, u8), String> {
    let c = cidr(s)?;
    let (a, n) = c.split_once('/').ok_or("invalid subnet")?;
    let n: u8 = n.parse().map_err(|_| "invalid subnet")?;
    if !(16..=29).contains(&n) {
        return Err("subnet prefix must be /16 to /29".into());
    }
    let a = u32::from(ipv4(a)?);
    let mask = u32::MAX << (32 - n);
    if a & !mask != 0 {
        return Err(format!("use the network address, e.g. {}/{n}", ip(a & mask)));
    }
    if !Ipv4Addr::from(a).is_private() {
        return Err("networks must use a private range (10/8, 172.16/12, 192.168/16)".into());
    }
    Ok((a, n))
}

fn hosts(net: u32, n: u8) -> (u32, u32) {
    let bcast = net | !(u32::MAX << (32 - n));
    (net + 1, bcast - 1)
}

fn range_in(s: &str, net: u32, n: u8, gw: u32) -> Result<String, String> {
    let (lo, hi) = hosts(net, n);
    let (a, b) = s.split_once('-').ok_or("range must look like 10.0.30.10-10.0.30.200")?;
    let (a, b) = (u32::from(ipv4(a)?), u32::from(ipv4(b)?));
    if a > b || a < lo || b > hi || (a..=b).contains(&gw) {
        return Err("range must be inside the subnet and must not include the gateway".into());
    }
    Ok(format!("{}-{}", ip(a), ip(b)))
}

fn dns_list(v: &[String]) -> Result<String, String> {
    if v.len() > 3 {
        return Err("at most 3 DNS servers".into());
    }
    Ok(v.iter().map(|s| ipv4(s).map(|a| a.to_string())).collect::<Result<Vec<_>, _>>()?.join(","))
}

fn overlaps(a: (u32, u8), b: (u32, u8)) -> bool {
    let n = a.1.min(b.1);
    let m = if n == 0 { 0 } else { u32::MAX << (32 - n) };
    a.0 & m == b.0 & m
}

const NET_PARTS: &[(&str, &[&str])] = &[
    (VLAN_IF, &["name", "interface", "vlan-id"]),
    (BVLAN, &["bridge", "vlan-ids", "tagged"]),
    (ADDR, &["address", "interface"]),
    (LIST, &["name"]),
    (POOL, &["name", "ranges"]),
    (DHCPS, &["name", "interface", "address-pool", "lease-time"]),
    (DHCPN, &["address", "gateway", "dns-server", "domain"]),
    (MEMBER, &["list", "interface"]),
    (FILTER, &["chain", "in-interface", "out-interface", "in-interface-list", "out-interface-list", "action"]),
];

fn set1(path: &'static str, args: Vec<(&'static str, String)>) -> Op {
    Op { path, verb: "set", find: None, args }
}

fn build(i: &N, r: &mut dyn Router) -> Result<Plan, String> {
    use N::*;
    Ok(match i {
        CreateNetwork { name, vlan, subnet: sn, dhcp, range, lease_time, dns, domain, isolated, guest, internet, list, bridge } => {
            let name = slug(name)?;
            if !(2..=4094).contains(vlan) {
                return Err("VLAN ID must be 2-4094".into());
            }
            let (net, n) = subnet(sn)?;
            let gw = net + 1;
            let range = if range.trim().is_empty() {
                let (_, hi) = hosts(net, n);
                if hi < net + 10 {
                    format!("{}-{}", ip(net + 2), ip(hi))
                } else {
                    format!("{}-{}", ip(net + 10), ip(hi))
                }
            } else {
                range_in(range.trim(), net, n, gw)?
            };
            let lease = if lease_time.is_empty() { None } else { Some(duration(lease_time)?) };
            let dns = if dns.is_empty() { ip(gw) } else { dns_list(dns)? };
            let domain = if domain.is_empty() { None } else { Some(crate::apply::dns_name(domain)?) };
            let list = if list.is_empty() { "LAN".to_string() } else { ifname(list)? };
            let isolated = *isolated || *guest;
            let c = format!("tik:net:{name}");

            let bridges = print(r, BRIDGE, &["name", "vlan-filtering"], None)?;
            let br = if bridge.is_empty() { bridges.first() } else { bridges.iter().find(|b| b.get("name") == Some(bridge)) };
            let br = br.ok_or("no such bridge: networks are VLANs on a bridge")?;
            let bname = ifname(br.get("name").map_or("", |s| s))?;
            let filtering = truthy(br.get("vlan-filtering"));

            for v in print(r, VLAN_IF, &["name", "vlan-id", "comment"], None)? {
                if v.get("vlan-id").and_then(|x| x.parse::<u16>().ok()) == Some(*vlan) {
                    return Err(format!("VLAN {vlan} is already used by {}", v.get("name").map_or("an interface", |s| s)));
                }
                if v.get("comment") == Some(&c) {
                    return Err(format!("a network named {name} already exists"));
                }
            }
            for a in print(r, ADDR, &["address"], None)? {
                if let Some(Ok(other)) = a.get("address").map(|s| cidr(s)) {
                    let (oa, on) = other.split_once('/').map(|(x, y)| (u32::from(x.parse::<Ipv4Addr>().unwrap_or(Ipv4Addr::UNSPECIFIED)), y.parse::<u8>().unwrap_or(32))).unwrap_or((0, 32));
                    if overlaps((net, n), (oa, on)) {
                        return Err(format!("{sn} overlaps an existing address ({other})"));
                    }
                }
            }
            let list_exists = list == "LAN" || !print(r, LIST, &["name"], Some(("name", &list)))?.is_empty();

            let ifn = format!("vlan{vlan}");
            let pool = format!("tik-net-{name}");
            let tag = |mut a: Vec<(&'static str, String)>| {
                a.push(("comment", c.clone()));
                a
            };
            let mut ops = vec![
                Op::add(VLAN_IF, tag(vec![("name", ifn.clone()), ("interface", bname.clone()), ("vlan-id", vlan.to_string())])),
                Op::add(BVLAN, tag(vec![("bridge", bname.clone()), ("vlan-ids", vlan.to_string()), ("tagged", bname.clone())])),
                Op::add(ADDR, tag(vec![("address", format!("{}/{n}", ip(gw))), ("interface", ifn.clone())])),
            ];
            if !list_exists {
                ops.push(Op::add(LIST, tag(vec![("name", list.clone())])));
            }
            if *dhcp {
                ops.push(Op::add(POOL, tag(vec![("name", pool.clone()), ("ranges", range.clone())])));
                let mut s = vec![("name", pool.clone()), ("interface", ifn.clone()), ("address-pool", pool.clone())];
                if let Some(l) = &lease {
                    s.push(("lease-time", l.clone()));
                }
                ops.push(Op::add(DHCPS, tag(s)));
                let mut nw = vec![("address", format!("{}/{n}", ip(net))), ("gateway", ip(gw)), ("dns-server", dns.clone())];
                if let Some(d) = &domain {
                    nw.push(("domain", d.clone()));
                }
                ops.push(Op::add(DHCPN, tag(nw)));
            }
            ops.push(Op::add(MEMBER, tag(vec![("list", list.clone()), ("interface", ifn.clone())])));
            let first = if isolated || !*internet { first_rule(r, FILTER, "forward")? } else { None };
            let place = |mut a: Vec<(&'static str, String)>| {
                if let Some(id) = &first {
                    a.push(("place-before", id.clone()));
                }
                tag(a)
            };
            if isolated {
                ops.push(Op::add(FILTER, place(vec![("chain", "forward".into()), ("in-interface", ifn.clone()), ("out-interface-list", "!WAN".into()), ("action", "drop".into())])));
                ops.push(Op::add(FILTER, place(vec![("chain", "forward".into()), ("out-interface", ifn.clone()), ("in-interface-list", "!WAN".into()), ("action", "drop".into())])));
            }
            if !*internet {
                ops.push(Op::add(FILTER, place(vec![("chain", "forward".into()), ("in-interface", ifn.clone()), ("out-interface-list", "WAN".into()), ("action", "drop".into())])));
            }
            let undo = undo_adds(&ops, &c);
            let mut p = plan(format!("Create network {name} (VLAN {vlan}, {}/{n})", ip(net)), Risk::Medium, ops, undo);
            if !filtering {
                p.warnings.push(format!("High risk if changed: bridge {bname} has VLAN filtering off. The router can use VLAN {vlan} now, but switch ports will only carry it tagged once bridge VLAN filtering is on. tikontrol does not switch that on here; use \"VLAN filtering\" (a separate, high-risk change) after checking every port's VLAN."));
            }
            if isolated {
                p.warnings.push("Isolation blocks traffic between this network and your other networks. Devices on it still reach the internet and the router (DNS, DHCP).".into());
            }
            if *guest {
                p.warnings.push("Guest networks can still reach the router's own services (WinBox, SSH, WebFig) because the default input rules trust the LAN list. Limit /ip/service addresses or add an input rule if guests must not see them.".into());
            }
            if list != "LAN" {
                p.warnings.push(format!("The VLAN joins list {list}, not LAN: the default input rules may then block DNS from it."));
            }
            p
        }
        RemoveNetwork { name } => {
            let name = slug(name)?;
            let c = format!("tik:net:{name}");
            let mut undo = vec![];
            for (path, fields) in NET_PARTS {
                undo.extend(rebuild(r, path, fields, &c)?);
            }
            if undo.is_empty() {
                return Err("only networks created by tikontrol can be removed here".into());
            }
            let ops = NET_PARTS.iter().rev().map(|(path, _)| remove(path, &c)).collect();
            let mut p = plan(format!("Remove network {name}"), Risk::Medium, ops, undo);
            p.warnings.push("Devices on this network lose their address and connection.".into());
            p
        }
        SetVlanFiltering { bridge, enabled } => {
            let b = ifname(bridge)?;
            let rows = print(r, BRIDGE, &["vlan-filtering"], Some(("name", &b)))?;
            let was = truthy(rows.first().ok_or("no such bridge")?.get("vlan-filtering"));
            let mut p = plan(
                format!("{} VLAN filtering on {b}", if *enabled { "Turn on" } else { "Turn off" }),
                Risk::High,
                vec![Op::on(BRIDGE, "set", "name", &b, vec![("vlan-filtering", yn(*enabled))])],
                vec![Op::on(BRIDGE, "set", "name", &b, vec![("vlan-filtering", yn(was))])],
            );
            p.warnings.push("High risk: VLAN filtering changes what every bridge port carries. A port whose PVID or tagged VLANs are wrong stops passing traffic, and that can cut off this browser and tikontrol.".into());
            if *enabled {
                let ports = print(r, BPORT, &["interface", "pvid", "frame-types"], Some(("bridge", &b)))?;
                let vlans = print(r, BVLAN, &["vlan-ids", "tagged", "untagged"], Some(("bridge", &b)))?;
                p.warnings.push(format!("Check first: {} ports and {} bridge VLAN entries. The port you manage the router from must be untagged in its PVID, and the bridge itself must be tagged in every VLAN the router serves.", ports.len(), vlans.len()));
                p.warnings.push("If you are cut off, do nothing: the router turns filtering back off by itself in 5 minutes.".into());
            }
            p
        }
        SetDhcpLease { server, lease_time } => {
            let (s, l) = (ros_name(server)?, duration(lease_time)?);
            let rows = print(r, DHCPS, &["lease-time"], Some(("name", &s)))?;
            let was = rows.first().ok_or("no such DHCP server")?.get("lease-time").cloned().unwrap_or_else(|| "30m".into());
            plan(format!("DHCP lease time {l} on {s}"), Risk::Low, vec![Op::on(DHCPS, "set", "name", &s, vec![("lease-time", l)])], vec![Op::on(DHCPS, "set", "name", &s, vec![("lease-time", was)])])
        }
        SetDhcpRange { pool, range } => {
            let p_ = ros_name(pool)?;
            let rows = print(r, POOL, &["ranges"], Some(("name", &p_)))?;
            let was = rows.first().ok_or("no such address pool")?.get("ranges").cloned().unwrap_or_default();
            // validate against the old range's /16 at least: both ends must be private and ordered
            let (a, b) = range.trim().split_once('-').ok_or("range must look like 10.0.30.10-10.0.30.200")?;
            let (a, b) = (ipv4(a)?, ipv4(b)?);
            if u32::from(a) > u32::from(b) || !a.is_private() || !b.is_private() || (u32::from(b) - u32::from(a)) > 65535 {
                return Err("range must be private, ordered and at most 65536 addresses".into());
            }
            let mut pl = plan(format!("DHCP range {a}-{b} for {p_}"), Risk::Low, vec![Op::on(POOL, "set", "name", &p_, vec![("ranges", format!("{a}-{b}"))])], vec![Op::on(POOL, "set", "name", &p_, vec![("ranges", was)])]);
            pl.warnings.push("Clients keep their current lease until it expires.".into());
            pl
        }
        SetDhcpNetwork { address, gateway, dns } => {
            let (net, n) = subnet(address)?;
            let a = format!("{}/{n}", ip(net));
            let rows = print(r, DHCPN, &["gateway", "dns-server"], Some(("address", &a)))?;
            let old = rows.first().ok_or("no DHCP network with that address")?;
            let mut args = vec![];
            let mut back = vec![];
            if !gateway.is_empty() {
                let g = u32::from(ipv4(gateway)?);
                let (lo, hi) = hosts(net, n);
                if g < lo || g > hi {
                    return Err("the gateway must be inside the network".into());
                }
                args.push(("gateway", ip(g)));
                back.push(("gateway", old.get("gateway").cloned().unwrap_or_default()));
            }
            if !dns.is_empty() {
                args.push(("dns-server", dns_list(dns)?));
                back.push(("dns-server", old.get("dns-server").cloned().unwrap_or_default()));
            }
            if args.is_empty() {
                return Err("nothing to change".into());
            }
            plan(format!("DHCP settings for {a}"), Risk::Low, vec![Op::on(DHCPN, "set", "address", &a, args)], vec![Op::on(DHCPN, "set", "address", &a, back)])
        }
        DhcpOption { name, code, value_type, value, network } => {
            let name = slug(name)?;
            if *code == 0 || *code == 255 {
                return Err("option code must be 1-254".into());
            }
            let v = value.trim();
            let val = match value_type.as_str() {
                "text" if !v.is_empty() && v.len() <= 200 && v.bytes().all(|b| (0x20..0x7f).contains(&b) && b != b'\'' && b != b'\\' && b != b'"' && b != b'$') => format!("s'{v}'"),
                "ip" => format!("'{}'", ipv4(v)?),
                "hex" if v.len() >= 2 && v.len() <= 200 && v.len() % 2 == 0 && v.bytes().all(|b| b.is_ascii_hexdigit()) => format!("0x{v}"),
                _ => return Err("value_type must be text, ip or hex (even number of hex digits), with a plain value".into()),
            };
            let c = format!("tik:dhcpopt:{name}");
            if !print(r, DHCPO, &["name"], Some(("name", &name)))?.is_empty() {
                return Err(format!("a DHCP option named {name} already exists"));
            }
            let mut ops = vec![Op::add(DHCPO, vec![("name", name.clone()), ("code", code.to_string()), ("value", val), ("comment", c.clone())])];
            let mut undo = vec![remove(DHCPO, &c)];
            if !network.is_empty() {
                let (net, n) = subnet(network)?;
                let a = format!("{}/{n}", ip(net));
                let rows = print(r, DHCPN, &["dhcp-option"], Some(("address", &a)))?;
                let old = rows.first().ok_or("no DHCP network with that address")?.get("dhcp-option").cloned().unwrap_or_default();
                let new = if old.is_empty() { name.clone() } else { format!("{old},{name}") };
                ops.push(Op::on(DHCPN, "set", "address", &a, vec![("dhcp-option", new)]));
                undo.insert(0, Op::on(DHCPN, "set", "address", &a, vec![("dhcp-option", old)]));
            }
            plan(format!("DHCP option {name} (code {code})"), Risk::Low, ops, undo)
        }
        RemoveDhcpOption { name } => {
            let name = slug(name)?;
            let c = format!("tik:dhcpopt:{name}");
            let undo = rebuild(r, DHCPO, &["name", "code", "value"], &c)?;
            if undo.is_empty() {
                return Err("only DHCP options created by tikontrol can be removed here".into());
            }
            let mut p = plan(format!("Remove DHCP option {name}"), Risk::Low, vec![remove(DHCPO, &c)], undo);
            p.warnings.push("If a DHCP network still lists this option, remove it there too.".into());
            p
        }
        SetMdnsRepeat { interfaces } => {
            if interfaces.len() > 16 {
                return Err("at most 16 interfaces".into());
            }
            let list = interfaces.iter().map(|s| ifname(s)).collect::<Result<Vec<_>, _>>()?.join(",");
            let rows = print(r, "/ip/dns", &["mdns-repeat-ifaces"], None)?;
            let was = rows.first().ok_or("cannot read DNS settings")?;
            if !was.contains_key("mdns-repeat-ifaces") {
                return Err("this RouterOS version has no mDNS repeater (needs 7.16 or later)".into());
            }
            let was = was.get("mdns-repeat-ifaces").cloned().unwrap_or_default();
            let title = if list.is_empty() { "Turn off mDNS across networks".to_string() } else { format!("Repeat mDNS between {list}") };
            plan(title, Risk::Low, vec![set1("/ip/dns", vec![("mdns-repeat-ifaces", list)])], vec![set1("/ip/dns", vec![("mdns-repeat-ifaces", was)])])
        }
        SetIgmpSnooping { bridge, enabled } => {
            let b = ifname(bridge)?;
            let rows = print(r, BRIDGE, &["igmp-snooping"], Some(("name", &b)))?;
            let was = truthy(rows.first().ok_or("no such bridge")?.get("igmp-snooping"));
            let mut p = plan(format!("{} IGMP snooping on {b}", if *enabled { "Turn on" } else { "Turn off" }), Risk::Medium, vec![Op::on(BRIDGE, "set", "name", &b, vec![("igmp-snooping", yn(*enabled))])], vec![Op::on(BRIDGE, "set", "name", &b, vec![("igmp-snooping", yn(was))])]);
            p.warnings.push("Changing bridge settings can briefly interrupt traffic on every port.".into());
            p
        }
        SetPortEnabled { interface, enabled } => {
            let ifn = ifname(interface)?;
            let rows = print(r, "/interface/ethernet", &["disabled"], Some(("name", &ifn)))?;
            let was = truthy(rows.first().ok_or("no such ethernet port")?.get("disabled"));
            let mut p = plan(format!("{} port {ifn}", if *enabled { "Enable" } else { "Disable" }), if *enabled { Risk::Low } else { Risk::Medium }, vec![Op::on("/interface/ethernet", "set", "name", &ifn, vec![("disabled", yn(!enabled))])], vec![Op::on("/interface/ethernet", "set", "name", &ifn, vec![("disabled", yn(was))])]);
            if !enabled {
                p.warnings.push(format!("Everything on {ifn} goes offline. If this browser reaches the router through {ifn}, you will be cut off until it reverts in 5 minutes."));
            }
            p
        }
        SetBridgePort { interface, pvid, frame_types } => {
            let ifn = ifname(interface)?;
            bridge_port(r, &ifn, *pvid, frame_types, &[])?
        }
        PortProfile { interface, pvid, tagged } => {
            let ifn = ifname(interface)?;
            let ft = if tagged.is_empty() { "admit-only-untagged-and-priority-tagged" } else { "admit-all" };
            bridge_port(r, &ifn, *pvid, ft, tagged)?
        }
        SetPoe { interface, mode } => {
            let ifn = ifname(interface)?;
            if !matches!(mode.as_str(), "auto-on" | "off" | "forced-on") {
                return Err("PoE mode must be auto-on, off or forced-on".into());
            }
            let path = "/interface/ethernet/poe";
            let rows = print(r, path, &["poe-out"], Some(("name", &ifn)))?;
            let was = rows.first().and_then(|x| x.get("poe-out")).cloned().ok_or("this port has no PoE output")?;
            let mut p = plan(format!("PoE {mode} on {ifn}"), Risk::Medium, vec![Op::on(path, "set", "name", &ifn, vec![("poe-out", mode.clone())])], vec![Op::on(path, "set", "name", &ifn, vec![("poe-out", was)])]);
            if mode == "forced-on" {
                p.warnings.push("forced-on powers the port even without a PoE device detected: it can damage equipment that does not expect power.".into());
            }
            if mode == "off" {
                p.warnings.push(format!("A device powered from {ifn} will turn off."));
            }
            p
        }
    })
}

/// PVID, frame types and (for trunks) tagged membership in each VLAN's bridge entry.
fn bridge_port(r: &mut dyn Router, ifn: &str, pvid: u16, frame_types: &str, tagged: &[u16]) -> Result<Plan, String> {
    if !(1..=4094).contains(&pvid) || tagged.iter().any(|v| !(1..=4094).contains(v)) || tagged.len() > 32 {
        return Err("VLAN IDs must be 1-4094 (at most 32 tagged)".into());
    }
    if !matches!(frame_types, "admit-all" | "admit-only-untagged-and-priority-tagged" | "admit-only-vlan-tagged") {
        return Err("frame types must be admit-all, admit-only-untagged-and-priority-tagged or admit-only-vlan-tagged".into());
    }
    let rows = print(r, BPORT, &["bridge", "pvid", "frame-types"], Some(("interface", ifn)))?;
    let old = rows.first().ok_or("this interface is not a bridge port")?;
    let bridge = old.get("bridge").cloned().unwrap_or_default();
    let mut ops = vec![Op::on(BPORT, "set", "interface", ifn, vec![("pvid", pvid.to_string()), ("frame-types", frame_types.into())])];
    let mut undo = vec![Op::on(BPORT, "set", "interface", ifn, vec![("pvid", old.get("pvid").cloned().unwrap_or_else(|| "1".into())), ("frame-types", old.get("frame-types").cloned().unwrap_or_else(|| "admit-all".into()))])];
    if !tagged.is_empty() {
        let entries = print(r, BVLAN, &[".id", "vlan-ids", "tagged"], Some(("bridge", &bridge)))?;
        for v in tagged {
            let c = format!("tik:portvlan:{ifn}:{v}");
            match entries.iter().find(|e| e.get("vlan-ids").is_some_and(|ids| ids.split(',').any(|x| x.trim() == v.to_string()))) {
                Some(e) => {
                    let old_t = e.get("tagged").cloned().unwrap_or_default();
                    if old_t.split(',').any(|x| x == ifn) {
                        continue;
                    }
                    let id = e.get(".id").cloned().ok_or("bridge VLAN entry without id")?;
                    let new_t = if old_t.is_empty() { ifn.to_string() } else { format!("{old_t},{ifn}") };
                    ops.push(Op::on(BVLAN, "set", ".id", &id, vec![("tagged", new_t)]));
                    undo.insert(0, Op::on(BVLAN, "set", ".id", &id, vec![("tagged", old_t)]));
                }
                None => {
                    ops.push(Op::add(BVLAN, vec![("bridge", bridge.clone()), ("vlan-ids", v.to_string()), ("tagged", format!("{bridge},{ifn}")), ("comment", c.clone())]));
                    undo.insert(0, remove(BVLAN, &c));
                }
            }
        }
    }
    let mut p = plan(format!("Port {ifn}: native VLAN {pvid}{}", if tagged.is_empty() { String::new() } else { format!(", tagged {}", tagged.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")) }), Risk::High, ops, undo);
    p.warnings.push(format!("High risk: changes which VLANs {ifn} carries. If you manage the router through {ifn}, a wrong VLAN cuts you off until it reverts in 5 minutes."));
    p.warnings.push("Only takes effect when VLAN filtering is on for the bridge.".into());
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ros::queries::tests::{row, Fake};
    use serde_json::json;

    fn fake() -> Fake {
        let mut f = Fake::default();
        f.answers.insert("/interface/bridge/print".into(), Ok(vec![row(&[("name", "bridge"), ("vlan-filtering", "false"), ("igmp-snooping", "false")])]));
        f.answers.insert("/ip/firewall/filter/print".into(), Ok(vec![row(&[(".id", "*7")])]));
        f
    }
    fn go(v: serde_json::Value, f: &mut Fake) -> Result<Plan, String> {
        compile(&v, f).expect("own kind")
    }

    #[test]
    fn create_network_compiles_all_objects_and_reverses() {
        let p = go(json!({"kind":"create-network","name":"IoT","vlan":30,"subnet":"10.0.30.0/24","isolated":true,"lease_time":"1d"}), &mut fake()).unwrap();
        let cli = p.cli();
        assert_eq!(cli[0], "/interface/vlan/add name=vlan30 interface=bridge vlan-id=30 comment=tik:net:IoT");
        assert!(cli.iter().any(|c| c.starts_with("/ip/address/add address=10.0.30.1/24 interface=vlan30")));
        assert!(cli.iter().any(|c| c.contains("/ip/pool/add name=tik-net-IoT ranges=10.0.30.10-10.0.30.254")));
        assert!(cli.iter().any(|c| c.contains("lease-time=1d")));
        assert!(cli.iter().any(|c| c.starts_with("/ip/dhcp-server/network/add address=10.0.30.0/24 gateway=10.0.30.1 dns-server=10.0.30.1")));
        assert_eq!(cli.iter().filter(|c| c.starts_with("/ip/firewall/filter/add") && c.contains("place-before=*7")).count(), 2);
        let undo = p.undo_cli();
        assert_eq!(undo.first().unwrap(), r#"/ip/firewall/filter/remove [find comment="tik:net:IoT"]"#);
        assert_eq!(undo.last().unwrap(), r#"/interface/vlan/remove [find comment="tik:net:IoT"]"#);
        let paths: Vec<&str> = p.undo.iter().map(|o| o.path).collect();
        assert_eq!(paths, [FILTER, MEMBER, DHCPN, DHCPS, POOL, ADDR, BVLAN, VLAN_IF]);
        assert!(p.warnings[0].contains("VLAN filtering off"));
        assert!(!cli.iter().any(|c| c.contains("vlan-filtering")), "never toggled as a side effect");
    }

    #[test]
    fn create_network_validates() {
        for bad in [
            json!({"kind":"create-network","name":"x","vlan":1,"subnet":"10.0.30.0/24"}),
            json!({"kind":"create-network","name":"x","vlan":30,"subnet":"10.0.30.5/24"}),
            json!({"kind":"create-network","name":"x","vlan":30,"subnet":"8.8.8.0/24"}),
            json!({"kind":"create-network","name":"a b","vlan":30,"subnet":"10.0.30.0/24"}),
            json!({"kind":"create-network","name":"x","vlan":30,"subnet":"10.0.30.0/24","range":"10.0.30.1-10.0.30.9"}),
            json!({"kind":"create-network","name":"x","vlan":30,"subnet":"10.0.30.0/24","extra":1}),
            json!({"kind":"set-poe","interface":"ether2","mode":"max"}),
            json!({"kind":"set-dhcp-lease","server":"s","lease_time":"1d; /system/reboot"}),
            json!({"kind":"dhcp-option","name":"o","code":66,"value_type":"text","value":"a'b"}),
            json!({"kind":"set-bridge-port","interface":"ether2","pvid":5000,"frame_types":"admit-all"}),
        ] {
            assert!(go(bad.clone(), &mut fake()).is_err(), "{bad}");
        }
        let mut f = fake();
        f.answers.insert("/ip/address/print".into(), Ok(vec![row(&[("address", "10.0.0.1/16")])]));
        assert!(go(json!({"kind":"create-network","name":"x","vlan":30,"subnet":"10.0.30.0/24"}), &mut f).unwrap_err().contains("overlaps"));
        assert!(compile(&json!({"kind":"block-client","mac":"x"}), &mut fake()).is_none());
    }

    #[test]
    fn remove_network_rebuilds_in_order() {
        let mut f = fake();
        f.answers.insert("/interface/vlan/print".into(), Ok(vec![row(&[("name", "vlan30"), ("interface", "bridge"), ("vlan-id", "30")])]));
        f.answers.insert("/ip/pool/print".into(), Ok(vec![row(&[("name", "tik-net-IoT"), ("ranges", "10.0.30.10-10.0.30.254")])]));
        let p = go(json!({"kind":"remove-network","name":"IoT"}), &mut f).unwrap();
        assert_eq!(p.ops.first().unwrap().path, FILTER);
        assert_eq!(p.ops.last().unwrap().path, VLAN_IF);
        assert_eq!(p.undo[0].path, VLAN_IF);
        assert!(go(json!({"kind":"remove-network","name":"none"}), &mut Fake::default()).is_err());
    }

    #[test]
    fn high_risk_changes_are_flagged() {
        let p = go(json!({"kind":"set-vlan-filtering","bridge":"bridge","enabled":true}), &mut fake()).unwrap();
        assert_eq!(p.risk, Risk::High);
        assert_eq!(p.undo_cli(), [r#"/interface/bridge/set [find name="bridge"] vlan-filtering=no"#]);
        let mut f = fake();
        f.answers.insert("/interface/bridge/port/print".into(), Ok(vec![row(&[("bridge", "bridge"), ("pvid", "1"), ("frame-types", "admit-all")])]));
        f.answers.insert("/interface/bridge/vlan/print".into(), Ok(vec![row(&[(".id", "*3"), ("vlan-ids", "30"), ("tagged", "bridge")])]));
        let p = go(json!({"kind":"port-profile","interface":"ether3","pvid":1,"tagged":[30,40]}), &mut f).unwrap();
        assert_eq!(p.risk, Risk::High);
        assert!(p.cli().contains(&r#"/interface/bridge/vlan/set [find .id="*3"] tagged=bridge,ether3"#.to_string()));
        assert!(p.cli().iter().any(|c| c.starts_with("/interface/bridge/vlan/add bridge=bridge vlan-ids=40 tagged=bridge,ether3")));
        assert_eq!(p.undo.last().unwrap().path, BPORT, "port settings restored last");
    }

    #[test]
    fn settings_restore_previous_values() {
        let mut f = fake();
        f.answers.insert("/ip/dns/print".into(), Ok(vec![row(&[("mdns-repeat-ifaces", "bridge")])]));
        let p = go(json!({"kind":"set-mdns-repeat","interfaces":["bridge","vlan30"]}), &mut f).unwrap();
        assert_eq!(p.cli(), ["/ip/dns/set mdns-repeat-ifaces=bridge,vlan30"]);
        assert_eq!(p.undo_cli(), ["/ip/dns/set mdns-repeat-ifaces=bridge"]);
        f.answers.insert("/interface/ethernet/print".into(), Ok(vec![row(&[("disabled", "false")])]));
        let p = go(json!({"kind":"set-port-enabled","interface":"ether4","enabled":false}), &mut f).unwrap();
        assert_eq!(p.cli(), [r#"/interface/ethernet/set [find name="ether4"] disabled=yes"#]);
        assert_eq!(p.undo_cli(), [r#"/interface/ethernet/set [find name="ether4"] disabled=no"#]);
        assert!(go(json!({"kind":"set-poe","interface":"ether2","mode":"auto-on"}), &mut f).unwrap_err().contains("no PoE"));
        f.answers.insert("/ip/dhcp-server/network/print".into(), Ok(vec![row(&[("gateway", "10.0.30.1"), ("dns-server", "10.0.30.1"), ("dhcp-option", "")])]));
        let p = go(json!({"kind":"set-dhcp-network","address":"10.0.30.0/24","dns":["9.9.9.9"]}), &mut f).unwrap();
        assert_eq!(p.undo_cli(), [r#"/ip/dhcp-server/network/set [find address="10.0.30.0/24"] dns-server=10.0.30.1"#]);
        let p = go(json!({"kind":"dhcp-option","name":"tftp","code":66,"value_type":"text","value":"boot.lan","network":"10.0.30.0/24"}), &mut f).unwrap();
        assert!(p.cli()[0].contains("value=\"s'boot.lan'\""), "{:?}", p.cli());
        assert_eq!(p.undo.last().unwrap().path, DHCPO);
    }

    #[test]
    fn every_kind_is_routed_here() {
        for k in KINDS {
            let e = go(json!({"kind": k}), &mut fake()).unwrap_err();
            assert!(!e.contains("unknown variant"), "{k}: {e}");
        }
    }
}
