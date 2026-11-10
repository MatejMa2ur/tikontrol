//! Security / CyberSecure (docs/FEATURES.md): custom forward rules, zone policy over interface lists,
//! IP groups, app/domain blocking, ad and content lists, region and threat feeds, per-device isolation.
//!
//! Custom rules live in the forward chain. The input chain (traffic to the router itself, so also
//! management access) is only touched by the explicit high-risk `input-rule` kind.

use super::networks::{first_rule, print, rebuild, remove, slug, undo_adds, FILTER};
use crate::apply::{cidr, dns_name, ipv4, norm_mac, plan, ros_name, Op, Plan, Risk};
use crate::ros::conn::Router;
use serde::Deserialize;

const ALIST: &str = "/ip/firewall/address-list";
const RAW: &str = "/ip/firewall/raw";
const STATIC: &str = "/ip/dns/static";
const ADLIST: &str = "/ip/dns/adlist";
const SCRIPT: &str = "/system/script";
const SCHED: &str = "/system/scheduler";
const BFILTER: &str = "/interface/bridge/filter";

const RULE_FIELDS: &[&str] = &["chain", "action", "protocol", "src-address", "src-address-list", "dst-address", "dst-address-list", "dst-port", "in-interface-list", "out-interface-list", "connection-state"];

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum S {
    FirewallRule {
        name: String,
        action: String,
        #[serde(default)]
        protocol: String,
        #[serde(default)]
        src: String,
        #[serde(default)]
        src_list: String,
        #[serde(default)]
        dst: String,
        #[serde(default)]
        dst_list: String,
        #[serde(default)]
        dst_port: String,
    },
    RemoveFirewallRule { name: String },
    InputRule {
        name: String,
        action: String,
        #[serde(default)]
        protocol: String,
        #[serde(default)]
        src: String,
        #[serde(default)]
        src_list: String,
        #[serde(default)]
        dst_port: String,
    },
    RemoveInputRule { name: String },
    /// allow | block | default (= remove our rule)
    ZonePolicy { from: String, to: String, policy: String },
    AddressGroup { name: String, addresses: Vec<String> },
    RemoveAddressGroup { name: String },
    BlockDomain {
        domain: String,
        #[serde(default)]
        tls: bool,
    },
    UnblockDomain { domain: String },
    AddAdlist { url: String },
    RemoveAdlist { url: String },
    RegionBlock { country: String, url: String },
    RemoveRegionBlock { country: String },
    ThreatFeed { name: String, url: String },
    RemoveThreatFeed { name: String },
    IsolateDevice { mac: String },
    UnisolateDevice { mac: String },
}

const KINDS: &[&str] = &[
    "firewall-rule",
    "remove-firewall-rule",
    "input-rule",
    "remove-input-rule",
    "zone-policy",
    "address-group",
    "remove-address-group",
    "block-domain",
    "unblock-domain",
    "add-adlist",
    "remove-adlist",
    "region-block",
    "remove-region-block",
    "threat-feed",
    "remove-threat-feed",
    "isolate-device",
    "unisolate-device",
];

pub fn compile(v: &serde_json::Value, r: &mut dyn Router) -> Option<Result<Plan, String>> {
    let kind = v.get("kind")?.as_str()?;
    if !KINDS.contains(&kind) {
        return None;
    }
    Some(serde_json::from_value::<S>(v.clone()).map_err(|e| format!("invalid {kind} intent: {e}")).and_then(|i| build(&i, r)))
}

/// IPv4 address, `a/n` or `a-b`.
fn addr(s: &str) -> Result<String, String> {
    let s = s.trim();
    if let Some((a, b)) = s.split_once('-') {
        let (a, b) = (ipv4(a)?, ipv4(b)?);
        if u32::from(a) > u32::from(b) {
            return Err("address range must be ordered".into());
        }
        return Ok(format!("{a}-{b}"));
    }
    if s.contains('/') { cidr(s) } else { ipv4(s).map(|a| a.to_string()) }
}

/// `80`, `8000-8100`, `80,443` (at most 15 parts).
fn ports(s: &str) -> Result<String, String> {
    let bad = || format!("invalid port list {s:?}");
    let parts: Vec<&str> = s.split(',').map(str::trim).collect();
    if parts.len() > 15 {
        return Err(bad());
    }
    let one = |p: &str| p.parse::<u16>().ok().filter(|n| *n > 0);
    for p in &parts {
        let ok = match p.split_once('-') {
            Some((a, b)) => matches!((one(a), one(b)), (Some(a), Some(b)) if a <= b),
            None => one(p).is_some(),
        };
        if !ok {
            return Err(bad());
        }
    }
    Ok(parts.join(","))
}

fn https_url(u: &str) -> Result<String, String> {
    let u = u.trim();
    let host_ok = u.strip_prefix("https://").is_some_and(|rest| !rest.is_empty() && !rest.split('/').next().unwrap_or("").contains('@'));
    let ok = host_ok && u.len() <= 512 && u.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~/:%&=+,".contains(&b));
    // no '?', '$', quotes or backslashes: the URL goes into a RouterOS script; no user:pass@ either
    ok.then(|| u.to_string()).ok_or_else(|| "the URL must be https://, without credentials or a query string (letters, digits and - . _ ~ / : % & = + , only)".into())
}

struct Match<'a> {
    protocol: &'a str,
    src: &'a str,
    src_list: &'a str,
    dst: &'a str,
    dst_list: &'a str,
    dst_port: &'a str,
}

fn rule_args(chain: &str, action: &str, m: &Match) -> Result<Vec<(&'static str, String)>, String> {
    if !matches!(action, "accept" | "drop" | "reject") {
        return Err("action must be accept, drop or reject".into());
    }
    let mut a = vec![("chain", chain.to_string()), ("action", action.to_string())];
    match m.protocol {
        "" => {}
        "tcp" | "udp" | "icmp" => a.push(("protocol", m.protocol.into())),
        _ => return Err("protocol must be tcp, udp or icmp".into()),
    }
    for (addr_v, list_v, ka, kl) in [(m.src, m.src_list, "src-address", "src-address-list"), (m.dst, m.dst_list, "dst-address", "dst-address-list")] {
        match (addr_v.trim().is_empty(), list_v.trim().is_empty()) {
            (false, false) => return Err("give an address or a group, not both".into()),
            (false, true) => a.push((ka, addr(addr_v)?)),
            (true, false) => a.push((kl, super::networks::ifname(list_v)?)),
            _ => {}
        }
    }
    if !m.dst_port.trim().is_empty() {
        if !matches!(m.protocol, "tcp" | "udp") {
            return Err("a port needs protocol tcp or udp".into());
        }
        a.push(("dst-port", ports(m.dst_port)?));
    }
    if action == "reject" {
        a.push(("reject-with", if m.protocol == "tcp" { "tcp-reset" } else { "icmp-network-unreachable" }.into()));
    }
    Ok(a)
}

/// Like `rebuild` for filter rules tagged `c` in `chain`, but the re-created rules go back where they
/// were (before the rule that followed them), not to the end of the chain. Order matters: an input
/// `accept` re-added after "drop all not coming from LAN" would never match.
fn rebuild_in_place(r: &mut dyn Router, chain: &str, fields: &[&'static str], c: &str) -> Result<Vec<Op>, String> {
    let mut undo = rebuild(r, FILTER, fields, c)?;
    let rows = print(r, FILTER, &[".id", "comment"], Some(("chain", chain)))?;
    let ours = |x: &crate::ros::conn::Row| x.get("comment").is_some_and(|v| v == c);
    let next = rows.iter().rposition(ours).and_then(|i| rows[i + 1..].iter().find(|x| !ours(x))).and_then(|x| x.get(".id")).cloned();
    if let Some(id) = next {
        for op in &mut undo {
            let at = op.args.len().saturating_sub(1); // keep `comment` last, like `tagged`
            op.args.insert(at, ("place-before", id.clone()));
        }
    }
    Ok(undo)
}

fn tagged(mut a: Vec<(&'static str, String)>, c: &str, before: &Option<String>) -> Vec<(&'static str, String)> {
    if let Some(id) = before {
        a.push(("place-before", id.clone()));
    }
    a.push(("comment", c.to_string()));
    a
}

fn exists(r: &mut dyn Router, path: &str, c: &str) -> Result<bool, String> {
    Ok(!print(r, path, &[".id"], Some(("comment", c)))?.is_empty())
}

/// Address list filled from an https feed by a daily script, plus raw drop rules both ways.
fn feed(list: &str, url: &str, c: &str) -> Vec<Op> {
    let src = format!(
        ":do {{ :local r [/tool/fetch url=\"{url}\" check-certificate=yes output=user as-value]; :if (($r->\"status\") = \"finished\") do={{ /ip/firewall/address-list/remove [find comment=\"{c}\"]; :local d ($r->\"data\"); :while ([:len $d] > 0) do={{ :local e [:find $d \"\\n\"]; :if ([:typeof $e] = \"nil\") do={{ :set e [:len $d] }}; :local l [:pick $d 0 $e]; :foreach x in={{\";\";\" \";\"\\r\";\"\\t\"}} do={{ :local s [:find $l $x]; :if ([:typeof $s] != \"nil\") do={{ :set l [:pick $l 0 $s] }} }}; :if ([:len $l] > 6 && [:pick $l 0 1] != \"#\") do={{ :do {{ /ip/firewall/address-list/add list=\"{list}\" address=$l comment=\"{c}\" }} on-error={{}} }}; :set d [:pick $d ($e + 1) [:len $d]] }} }} }} on-error={{ :log warning \"tikontrol: feed {list} could not be updated\" }}"
    );
    vec![
        Op::add(SCRIPT, vec![("name", list.to_string()), ("policy", "read,write,test".into()), ("source", src), ("comment", c.to_string())]),
        Op::add(SCHED, vec![("name", list.to_string()), ("start-time", "startup".into()), ("interval", "1d".into()), ("policy", "read,write,test".into()), ("on-event", format!("/system/script/run {list}")), ("comment", c.to_string())]),
        Op::add(RAW, vec![("chain", "prerouting".into()), ("src-address-list", list.to_string()), ("action", "drop".into()), ("comment", c.to_string())]),
        Op::add(RAW, vec![("chain", "prerouting".into()), ("dst-address-list", list.to_string()), ("action", "drop".into()), ("comment", c.to_string())]),
        Op::on(SCRIPT, "run", "name", list, vec![]),
    ]
}

fn feed_undo(c: &str) -> Vec<Op> {
    // entries the script added carry the same tag
    vec![remove(SCHED, c), remove(SCRIPT, c), remove(RAW, c), remove(ALIST, c)]
}

fn feed_plan(title: String, list: &str, url: &str, c: &str) -> Plan {
    let mut p = plan(title, Risk::Medium, feed(list, url, c), feed_undo(c));
    p.warnings.push(format!("The router downloads {url} now and once a day. Only use a feed you trust: every address in it gets blocked, in both directions."));
    p.warnings.push("Very large lists can exceed what /tool/fetch keeps in memory; the script then logs a warning and keeps the old list.".into());
    p
}

fn remove_feed(r: &mut dyn Router, title: String, list: &str, c: &str) -> Result<Plan, String> {
    let rows = print(r, SCRIPT, &["source"], Some(("comment", c)))?;
    let src = rows.first().and_then(|x| x.get("source")).ok_or("only feeds created by tikontrol can be removed here")?;
    let url = src.split_once("url=\"").and_then(|(_, rest)| rest.split_once('"')).map(|(u, _)| u).ok_or("cannot read the feed URL")?;
    let url = https_url(url)?;
    Ok(plan(title, Risk::Low, feed_undo(c), feed(list, &url, c)))
}

fn build(i: &S, r: &mut dyn Router) -> Result<Plan, String> {
    use S::*;
    Ok(match i {
        FirewallRule { name, action, protocol, src, src_list, dst, dst_list, dst_port } => {
            let name = ros_name(name)?;
            let c = format!("tik:fw:{name}");
            let args = rule_args("forward", action, &Match { protocol, src, src_list, dst, dst_list, dst_port })?;
            if exists(r, FILTER, &c)? {
                return Err(format!("a rule named {name} already exists"));
            }
            let before = first_rule(r, FILTER, "forward")?;
            let mut p = plan(format!("Firewall rule {name}: {action}"), Risk::Medium, vec![Op::add(FILTER, tagged(args, &c, &before))], vec![remove(FILTER, &c)]);
            p.warnings.push("The rule goes first in the forward chain, before FastTrack and \"accept established\", so it also matches open connections.".into());
            p
        }
        RemoveFirewallRule { name } | RemoveInputRule { name } => {
            let name = ros_name(name)?;
            let input = matches!(i, RemoveInputRule { .. });
            let c = format!("tik:{}:{name}", if input { "input" } else { "fw" });
            let mut fields = RULE_FIELDS.to_vec();
            fields.push("reject-with");
            let undo = rebuild_in_place(r, if input { "input" } else { "forward" }, &fields, &c)?;
            if undo.is_empty() {
                return Err("only rules created by tikontrol can be removed here".into());
            }
            let mut p = plan(format!("Remove {}rule {name}", if input { "input " } else { "firewall " }), if input { Risk::High } else { Risk::Low }, vec![remove(FILTER, &c)], undo);
            if input {
                p.warnings.push("High risk: changes access to the router itself (input chain). Make sure you can still reach it before keeping.".into());
            }
            p
        }
        InputRule { name, action, protocol, src, src_list, dst_port } => {
            let name = ros_name(name)?;
            let c = format!("tik:input:{name}");
            let args = rule_args("input", action, &Match { protocol, src, src_list, dst: "", dst_list: "", dst_port })?;
            if action != "accept" && src.trim().is_empty() && src_list.trim().is_empty() && dst_port.trim().is_empty() {
                return Err("an input drop rule needs a source or a port: dropping everything locks you out".into());
            }
            if exists(r, FILTER, &c)? {
                return Err(format!("a rule named {name} already exists"));
            }
            let before = first_rule(r, FILTER, "input")?;
            let mut p = plan(format!("Router access rule {name}: {action}"), Risk::High, vec![Op::add(FILTER, tagged(args, &c, &before))], vec![remove(FILTER, &c)]);
            p.warnings.push("High risk: input rules control access to the router itself, including WinBox, SSH, WebFig and the API tikontrol uses. A wrong rule locks you and tikontrol out.".into());
            p.warnings.push("If you are cut off, do nothing: the router removes the rule by itself in 5 minutes.".into());
            p
        }
        ZonePolicy { from, to, policy } => {
            let (a, b) = (super::networks::ifname(from)?, super::networks::ifname(to)?);
            if a == b {
                return Err("pick two different zones".into());
            }
            if !matches!(policy.as_str(), "allow" | "block" | "default") {
                return Err("policy must be allow, block or default".into());
            }
            for z in [&a, &b] {
                if print(r, "/interface/list", &["name"], Some(("name", z)))?.is_empty() {
                    return Err(format!("no interface list named {z}"));
                }
            }
            let c = format!("tik:zone:{a}:{b}");
            let mut undo = rebuild_in_place(r, "forward", RULE_FIELDS, &c)?;
            let mut ops = vec![remove(FILTER, &c)];
            if policy != "default" {
                let before = first_rule(r, FILTER, "forward")?;
                let mut args = vec![("chain", "forward".to_string()), ("in-interface-list", a.clone()), ("out-interface-list", b.clone())];
                if policy == "block" {
                    // replies to connections opened from `b` still pass
                    args.push(("connection-state", "new".into()));
                    args.push(("action", "drop".into()));
                } else {
                    args.push(("action", "accept".into()));
                }
                ops.push(Op::add(FILTER, tagged(args, &c, &before)));
                undo.insert(0, remove(FILTER, &c));
            }
            let mut p = plan(format!("Zone {a} → {b}: {policy}"), Risk::Medium, ops, undo);
            p.warnings.push("Zone rules go first in the forward chain. Your own hand-written rules further down still apply after them.".into());
            p
        }
        AddressGroup { name, addresses } => {
            let name = slug(name)?;
            if addresses.is_empty() || addresses.len() > 256 {
                return Err("a group needs 1-256 addresses".into());
            }
            let c = format!("tik:group:{name}");
            if exists(r, ALIST, &c)? {
                return Err(format!("group {name} already exists"));
            }
            let ops = addresses.iter().map(|a| addr(a).map(|a| Op::add(ALIST, vec![("list", name.clone()), ("address", a), ("comment", c.clone())]))).collect::<Result<Vec<_>, _>>()?;
            plan(format!("IP group {name} ({} addresses)", ops.len()), Risk::Low, ops, vec![remove(ALIST, &c)])
        }
        RemoveAddressGroup { name } => {
            let name = slug(name)?;
            let c = format!("tik:group:{name}");
            let undo = rebuild(r, ALIST, &["list", "address"], &c)?;
            if undo.is_empty() {
                return Err("only groups created by tikontrol can be removed here".into());
            }
            let mut p = plan(format!("Remove IP group {name}"), Risk::Low, vec![remove(ALIST, &c)], undo);
            p.warnings.push("Rules that use this group stop matching anything.".into());
            p
        }
        BlockDomain { domain, tls } => {
            let d = dns_name(domain)?;
            if !d.contains('.') {
                return Err("give a full domain, e.g. example.com".into());
            }
            let c = format!("tik:app:{d}");
            if exists(r, STATIC, &c)? {
                return Err(format!("{d} is already blocked"));
            }
            let mut ops = vec![Op::add(STATIC, vec![("name", d.clone()), ("type", "NXDOMAIN".into()), ("match-subdomain", "yes".into()), ("comment", c.clone())])];
            if *tls {
                let before = first_rule(r, FILTER, "forward")?;
                for host in [d.clone(), format!("*.{d}")] {
                    let args = vec![("chain", "forward".to_string()), ("protocol", "tcp".into()), ("dst-port", "443".into()), ("tls-host", host), ("action", "reject".into()), ("reject-with", "tcp-reset".into())];
                    ops.push(Op::add(FILTER, tagged(args, &c, &before)));
                }
            }
            let undo = undo_adds(&ops, &c);
            let mut p = plan(format!("Block {d}"), Risk::Low, ops, undo);
            p.warnings.push("Devices that use their own DNS (DNS over HTTPS, hard-coded resolvers) can get around DNS blocking.".into());
            if *tls {
                p.warnings.push("TLS-host matching is checked by the CPU on every new HTTPS connection and misses encrypted ClientHello (ECH).".into());
            }
            p
        }
        UnblockDomain { domain } => {
            let d = dns_name(domain)?;
            let c = format!("tik:app:{d}");
            let mut undo = rebuild(r, STATIC, &["name", "type", "match-subdomain"], &c)?;
            if undo.is_empty() {
                return Err("only domains blocked by tikontrol can be unblocked here".into());
            }
            undo.extend(rebuild_in_place(r, "forward", &["chain", "protocol", "dst-port", "tls-host", "action", "reject-with"], &c)?);
            plan(format!("Unblock {d}"), Risk::Low, vec![remove(FILTER, &c), remove(STATIC, &c)], undo)
        }
        AddAdlist { url } => {
            let u = https_url(url)?;
            if !print(r, ADLIST, &["url"], Some(("url", &u)))?.is_empty() {
                return Err("this list is already added".into());
            }
            let mut p = plan("Add content filter list".into(), Risk::Low, vec![Op::add(ADLIST, vec![("url", u.clone()), ("ssl-verify", "yes".into())])], vec![Op::on(ADLIST, "remove", "url", &u, vec![])]);
            p.warnings.push("Large lists need a bigger DNS cache (/ip/dns cache-size); names that don't fit are not blocked.".into());
            p
        }
        RemoveAdlist { url } => {
            let u = https_url(url)?;
            if print(r, ADLIST, &["url"], Some(("url", &u)))?.is_empty() {
                return Err("no such list".into());
            }
            plan("Remove content filter list".into(), Risk::Low, vec![Op::on(ADLIST, "remove", "url", &u, vec![])], vec![Op::add(ADLIST, vec![("url", u.clone()), ("ssl-verify", "yes".into())])])
        }
        RegionBlock { country, url } => {
            let cc = country.trim().to_ascii_uppercase();
            if cc.len() != 2 || !cc.bytes().all(|b| b.is_ascii_uppercase()) {
                return Err("country must be a two-letter ISO code".into());
            }
            let u = https_url(url)?;
            let c = format!("tik:geo:{cc}");
            if exists(r, SCRIPT, &c)? {
                return Err(format!("{cc} is already blocked"));
            }
            feed_plan(format!("Block region {cc}"), &format!("tik-geo-{cc}"), &u, &c)
        }
        RemoveRegionBlock { country } => {
            let cc = country.trim().to_ascii_uppercase();
            if cc.len() != 2 || !cc.bytes().all(|b| b.is_ascii_uppercase()) {
                return Err("country must be a two-letter ISO code".into());
            }
            remove_feed(r, format!("Unblock region {cc}"), &format!("tik-geo-{cc}"), &format!("tik:geo:{cc}"))?
        }
        ThreatFeed { name, url } => {
            let n = slug(name)?;
            let u = https_url(url)?;
            let c = format!("tik:threat:{n}");
            if exists(r, SCRIPT, &c)? {
                return Err(format!("feed {n} already exists"));
            }
            feed_plan(format!("Threat feed {n}"), &format!("tik-threat-{n}"), &u, &c)
        }
        RemoveThreatFeed { name } => {
            let n = slug(name)?;
            remove_feed(r, format!("Remove threat feed {n}"), &format!("tik-threat-{n}"), &format!("tik:threat:{n}"))?
        }
        IsolateDevice { mac } => {
            let mac = norm_mac(mac)?;
            let c = format!("tik:isolate:{mac}");
            let ops = vec![
                Op::add(BFILTER, vec![("chain", "forward".into()), ("src-mac-address", format!("{mac}/FF:FF:FF:FF:FF:FF")), ("action", "drop".into()), ("comment", c.clone())]),
                Op::add(BFILTER, vec![("chain", "forward".into()), ("dst-mac-address", format!("{mac}/FF:FF:FF:FF:FF:FF")), ("action", "drop".into()), ("comment", c.clone())]),
            ];
            let mut p = plan(format!("Isolate {mac} from the LAN"), Risk::Low, ops, vec![remove(BFILTER, &c)]);
            p.warnings.push("The device keeps internet access through the router but can't reach other devices on the same bridge.".into());
            p.warnings.push("Bridge filters only see traffic the CPU handles: on ports with hardware offloading, switched traffic between two local ports can bypass them.".into());
            p
        }
        UnisolateDevice { mac } => {
            let mac = norm_mac(mac)?;
            let c = format!("tik:isolate:{mac}");
            let undo = rebuild(r, BFILTER, &["chain", "src-mac-address", "dst-mac-address", "action"], &c)?;
            if undo.is_empty() {
                return Err("this device is not isolated by tikontrol".into());
            }
            plan(format!("End isolation for {mac}"), Risk::Low, vec![remove(BFILTER, &c)], undo)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ros::queries::tests::{row, Fake};
    use serde_json::json;

    fn go(v: serde_json::Value, f: &mut Fake) -> Result<Plan, String> {
        compile(&v, f).expect("own kind")
    }
    fn fake() -> Fake {
        let mut f = Fake::default();
        f.answers.insert("/ip/firewall/filter/print".into(), Ok(vec![]));
        f
    }

    #[test]
    fn firewall_rule_compiles_and_reverses() {
        let mut f = Fake::default();
        f.answers.insert("/ip/firewall/filter/print".into(), Ok(vec![]));
        let p = go(json!({"kind":"firewall-rule","name":"no-nas","action":"drop","protocol":"tcp","src_list":"kids","dst":"10.0.0.5","dst_port":"445,139"}), &mut f).unwrap();
        assert_eq!(p.cli(), ["/ip/firewall/filter/add chain=forward action=drop protocol=tcp src-address-list=kids dst-address=10.0.0.5 dst-port=445,139 comment=tik:fw:no-nas"]);
        assert_eq!(p.undo_cli(), [r#"/ip/firewall/filter/remove [find comment="tik:fw:no-nas"]"#]);
        assert_eq!(p.risk, Risk::Medium);
    }

    #[test]
    fn validation_rejects_bad_input() {
        for bad in [
            json!({"kind":"firewall-rule","name":"x","action":"jump"}),
            json!({"kind":"firewall-rule","name":"x","action":"drop","dst_port":"80"}),
            json!({"kind":"firewall-rule","name":"x","action":"drop","protocol":"tcp","dst_port":"0"}),
            json!({"kind":"firewall-rule","name":"x","action":"drop","src":"10.0.0.1","src_list":"g"}),
            json!({"kind":"firewall-rule","name":"x","action":"drop","src":"10.0.0.1; /system/reboot"}),
            json!({"kind":"firewall-rule","name":"x","action":"drop","chain":"input"}),
            json!({"kind":"input-rule","name":"x","action":"drop"}),
            json!({"kind":"region-block","country":"XYZ","url":"https://example.org/x.zone"}),
            json!({"kind":"region-block","country":"XX","url":"http://example.org/x.zone"}),
            json!({"kind":"threat-feed","name":"f","url":"https://user:pw@example.org/drop.txt"}),
            json!({"kind":"threat-feed","name":"f","url":"https://example.org/drop.txt?token=abc"}),
            json!({"kind":"threat-feed","name":"f","url":"https://example.org/\"; /system/reboot"}),
            json!({"kind":"add-adlist","url":"http://example.org/hosts"}),
            json!({"kind":"block-domain","domain":"com"}),
            json!({"kind":"address-group","name":"g","addresses":["10.0.0.300"]}),
            json!({"kind":"isolate-device","mac":"nope"}),
            json!({"kind":"zone-policy","from":"LAN","to":"LAN","policy":"block"}),
        ] {
            assert!(go(bad.clone(), &mut fake()).is_err(), "{bad}");
        }
        assert!(compile(&json!({"kind":"create-network"}), &mut fake()).is_none());
    }

    #[test]
    fn input_rules_are_high_risk() {
        let mut f = Fake::default();
        f.answers.insert("/ip/firewall/filter/print".into(), Ok(vec![row(&[(".id", "*1")])]));
        f.answers.insert("/ip/firewall/filter/print".into(), Ok(vec![]));
        let p = go(json!({"kind":"input-rule","name":"winbox-lan","action":"accept","protocol":"tcp","src":"10.0.0.0/24","dst_port":"8291"}), &mut f).unwrap();
        assert_eq!(p.risk, Risk::High);
        assert!(p.warnings[0].contains("locks you"));
        assert!(p.cli()[0].starts_with("/ip/firewall/filter/add chain=input"));
    }

    #[test]
    fn removed_rules_come_back_in_place() {
        // the fake answers every filter print with these rows: ours, then defconf's drop
        let mut f = Fake::default();
        f.answers.insert(
            "/ip/firewall/filter/print".into(),
            Ok(vec![
                row(&[(".id", "*7"), ("chain", "input"), ("action", "accept"), ("protocol", "tcp"), ("dst-port", "8291"), ("comment", "tik:input:winbox-wan")]),
                row(&[(".id", "*8"), ("chain", "input"), ("action", "drop"), ("in-interface-list", "!LAN"), ("comment", "defconf: drop all not coming from LAN")]),
            ]),
        );
        let p = go(json!({"kind":"remove-input-rule","name":"winbox-wan"}), &mut f).unwrap();
        assert_eq!(p.risk, Risk::High);
        assert_eq!(p.cli(), [r#"/ip/firewall/filter/remove [find comment="tik:input:winbox-wan"]"#]);
        assert_eq!(p.undo_cli()[0], "/ip/firewall/filter/add chain=input action=accept protocol=tcp dst-port=8291 place-before=*8 comment=tik:input:winbox-wan");
        assert!(crate::apply::revert_script("t3", &p).is_ok());

        // last in the chain: nothing to place before, the rule goes back to the end
        f.answers.insert("/ip/firewall/filter/print".into(), Ok(vec![row(&[(".id", "*9"), ("chain", "forward"), ("action", "drop"), ("comment", "tik:fw:no-nas")])]));
        let p = go(json!({"kind":"remove-firewall-rule","name":"no-nas"}), &mut f).unwrap();
        assert_eq!(p.undo_cli(), ["/ip/firewall/filter/add chain=forward action=drop comment=tik:fw:no-nas"]);

        // only tikontrol's rules
        let e = go(json!({"kind":"remove-firewall-rule","name":"defconf"}), &mut fake()).unwrap_err();
        assert!(e.contains("only rules created by tikontrol"), "{e}");
    }

    #[test]
    fn zone_policy_replaces_and_restores() {
        let mut f = Fake::default();
        f.answers.insert("/interface/list/print".into(), Ok(vec![row(&[("name", "x")])]));
        f.answers.insert("/ip/firewall/filter/print".into(), Ok(vec![row(&[(".id", "*4"), ("chain", "forward"), ("in-interface-list", "IoT"), ("out-interface-list", "LAN"), ("action", "accept")])]));
        let p = go(json!({"kind":"zone-policy","from":"IoT","to":"LAN","policy":"block"}), &mut f).unwrap();
        assert_eq!(p.cli()[0], r#"/ip/firewall/filter/remove [find comment="tik:zone:IoT:LAN"]"#);
        assert!(p.cli()[1].contains("connection-state=new action=drop place-before=*4 comment=tik:zone:IoT:LAN"), "{}", p.cli()[1]);
        assert_eq!(p.undo_cli()[0], r#"/ip/firewall/filter/remove [find comment="tik:zone:IoT:LAN"]"#);
        assert!(p.undo_cli()[1].starts_with("/ip/firewall/filter/add chain=forward action=accept"));
    }

    #[test]
    fn domain_block_and_feeds() {
        let p = go(json!({"kind":"block-domain","domain":"Example.COM","tls":true}), &mut fake()).unwrap();
        assert_eq!(p.cli()[0], "/ip/dns/static/add name=example.com type=NXDOMAIN match-subdomain=yes comment=tik:app:example.com");
        assert_eq!(p.cli().len(), 3);
        assert_eq!(p.undo.iter().map(|o| o.path).collect::<Vec<_>>(), [FILTER, STATIC]);

        let p = go(json!({"kind":"region-block","country":"xx","url":"https://example.org/xx.zone"}), &mut fake()).unwrap();
        assert_eq!(p.risk, Risk::Medium);
        assert!(p.ops[0].args.iter().any(|(k, v)| *k == "source" && v.contains("url=\"https://example.org/xx.zone\"")));
        assert_eq!(p.undo.iter().map(|o| o.path).collect::<Vec<_>>(), [SCHED, SCRIPT, RAW, ALIST]);
        let s = crate::apply::revert_script("t1", &p).unwrap();
        assert!(s.contains("tik:geo:XX"));

        let mut f = Fake::default();
        f.answers.insert("/system/script/print".into(), Ok(vec![row(&[("source", p.ops[0].args[2].1.as_str())])]));
        let r = go(json!({"kind":"remove-region-block","country":"XX"}), &mut f).unwrap();
        assert_eq!(r.undo.len(), 5);
        assert!(crate::apply::revert_script("t2", &r).is_ok(), "undo fits in a revert script");
    }

    #[test]
    fn device_isolation_and_groups() {
        let p = go(json!({"kind":"isolate-device","mac":"aa-bb-cc-dd-ee-ff"}), &mut fake()).unwrap();
        assert!(p.cli()[0].starts_with("/interface/bridge/filter/add chain=forward src-mac-address=AA:BB:CC:DD:EE:FF/FF:FF:FF:FF:FF:FF action=drop"));
        let p = go(json!({"kind":"address-group","name":"servers","addresses":["10.0.0.2","10.0.1.0/24","10.0.2.1-10.0.2.9"]}), &mut fake()).unwrap();
        assert_eq!(p.ops.len(), 3);
        assert_eq!(p.undo_cli(), [r#"/ip/firewall/address-list/remove [find comment="tik:group:servers"]"#]);
    }

    #[test]
    fn every_kind_is_routed_here() {
        for k in KINDS {
            let e = go(json!({"kind": k}), &mut fake()).unwrap_err();
            assert!(!e.contains("unknown variant"), "{k}: {e}");
        }
    }
}
