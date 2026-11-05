//! Traffic rules (docs/FEATURES.md, Security): block internet, block domains, or limit speed for one
//! client (MAC), a client group (address list `tik-<group>`) or a network, optionally only on some
//! days and hours. Everything a rule creates carries `comment=tik:rule:<name>`; saving a rule with an
//! existing name replaces it (edit), and the undo restores the old one.

use super::networks::{first_rule, print, rebuild, remove, slug, FILTER};
use crate::apply::{cidr, dns_name, ipv4, norm_mac, plan, rate, Op, Plan, Risk};
use crate::ros::conn::Router;
use serde::Deserialize;

const QUEUE: &str = "/queue/simple";
const ALIST: &str = "/ip/firewall/address-list";
const DAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
const FILTER_FIELDS: &[&str] = &["chain", "action", "protocol", "dst-port", "src-mac-address", "src-address", "src-address-list", "out-interface-list", "tls-host", "reject-with", "time"];
const QUEUE_FIELDS: &[&str] = &["name", "target", "max-limit", "time"];

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
#[allow(clippy::large_enum_variant)] // parsed once per request, never stored
enum T {
    TrafficRule {
        name: String,
        /// mac | group | network
        target: String,
        value: String,
        /// block | domains | limit
        action: String,
        #[serde(default)]
        domains: Vec<String>,
        #[serde(default)]
        down: String,
        #[serde(default)]
        up: String,
        #[serde(default)]
        days: Vec<String>,
        #[serde(default)]
        from: String,
        #[serde(default)]
        to: String,
    },
    RemoveTrafficRule { name: String },
}

pub fn compile(v: &serde_json::Value, r: &mut dyn Router) -> Option<Result<Plan, String>> {
    let kind = v.get("kind")?.as_str()?;
    if !matches!(kind, "traffic-rule" | "remove-traffic-rule") {
        return None;
    }
    Some(serde_json::from_value::<T>(v.clone()).map_err(|e| format!("invalid {kind} intent: {e}")).and_then(|t| build(t, r)))
}

/// "HH:MM" → minutes since midnight.
fn hm(s: &str) -> Result<u32, String> {
    let bad = || format!("invalid time {s:?} (HH:MM)");
    let (h, m) = s.trim().split_once(':').ok_or_else(bad)?;
    let ok = |x: &str, max: u32| (x.len() == 2 && x.bytes().all(|b| b.is_ascii_digit())).then(|| x.parse::<u32>().unwrap()).filter(|n| *n <= max);
    Ok(ok(h, 23).ok_or_else(bad)? * 60 + ok(m, 59).ok_or_else(bad)?)
}

/// RouterOS `time=` values. An overnight window (22:00-07:00) becomes two, the second on the next day.
fn windows(days: &[String], from: &str, to: &str) -> Result<Vec<Option<String>>, String> {
    let mut idx = vec![];
    for d in days {
        let i = DAYS.iter().position(|x| *x == d.trim().to_ascii_lowercase()).ok_or_else(|| format!("invalid day {d:?}"))?;
        if !idx.contains(&i) {
            idx.push(i);
        }
    }
    idx.sort();
    if from.trim().is_empty() && to.trim().is_empty() {
        return Ok(vec![(!idx.is_empty()).then(|| render("00:00:00", "23:59:59", &idx))]);
    }
    let (a, b) = (hm(from)?, hm(to)?);
    let t = |m: u32| format!("{:02}:{:02}:00", m / 60, m % 60);
    Ok(match a.cmp(&b) {
        std::cmp::Ordering::Equal => return Err("start and end time must differ".into()),
        std::cmp::Ordering::Less => vec![Some(render(&t(a), &t(b), &idx))],
        std::cmp::Ordering::Greater => {
            let next: Vec<usize> = idx.iter().map(|i| (i + 1) % 7).collect();
            vec![Some(render(&t(a), "23:59:59", &idx)), Some(render("00:00:00", &t(b), &next))]
        }
    })
}

fn render(a: &str, b: &str, idx: &[usize]) -> String {
    // no days listed = every day
    let all: Vec<usize> = if idx.is_empty() { (0..7).collect() } else { idx.to_vec() };
    let mut s = format!("{a}-{b}");
    for i in DAYS.iter().enumerate().filter(|(i, _)| all.contains(i)).map(|(_, d)| d) {
        s.push(',');
        s.push_str(i);
    }
    s
}

fn build(t: T, r: &mut dyn Router) -> Result<Plan, String> {
    let (name, c) = match &t {
        T::TrafficRule { name, .. } | T::RemoveTrafficRule { name } => {
            let n = slug(name)?;
            (n.clone(), format!("tik:rule:{n}"))
        }
    };
    // inverse of whatever this rule name owns now (empty for a new rule)
    let mut old = rebuild(r, FILTER, FILTER_FIELDS, &c)?;
    old.extend(rebuild(r, QUEUE, QUEUE_FIELDS, &c)?);
    let clear = vec![remove(FILTER, &c), remove(QUEUE, &c)];
    let T::TrafficRule { target, value, action, domains, down, up, days, from, to, .. } = t else {
        if old.is_empty() {
            return Err("only traffic rules created by tikontrol can be removed here".into());
        }
        return Ok(plan(format!("Remove traffic rule {name}"), Risk::Low, clear, old));
    };

    // who: a firewall matcher and, for queues, the target addresses
    let (matcher, targets): ((&'static str, String), Vec<String>) = match target.as_str() {
        "mac" => {
            let mac = norm_mac(&value)?;
            let ip = if action == "limit" { client_ip(r, &mac)? } else { String::new() };
            (("src-mac-address", mac), vec![ip])
        }
        "group" => {
            let list = format!("tik-{}", slug(&value)?);
            let members = print(r, ALIST, &["address"], Some(("list", &list)))?;
            let ips = members.iter().filter_map(|m| m.get("address")).map(|a| ipv4(a).map(|a| a.to_string())).collect::<Result<Vec<_>, _>>()?;
            if ips.is_empty() {
                return Err(format!("client group {value} has no members"));
            }
            (("src-address-list", list), ips)
        }
        "network" => {
            let net = cidr(&value)?;
            if !ipv4(net.split('/').next().unwrap_or(""))?.is_private() {
                return Err("the network must be a private LAN range".into());
            }
            (("src-address", net.clone()), vec![net])
        }
        _ => return Err("target must be mac, group or network".into()),
    };
    let times = windows(&days, &from, &to)?;
    let when = |a: &mut Vec<(&'static str, String)>, w: &Option<String>| {
        if let Some(w) = w {
            a.push(("time", w.clone()));
        }
    };

    let before = first_rule(r, FILTER, "forward")?;
    let fw = |mut a: Vec<(&'static str, String)>, w: &Option<String>| {
        a.insert(1, matcher.clone());
        when(&mut a, w);
        if let Some(id) = &before {
            a.push(("place-before", id.clone()));
        }
        a.push(("comment", c.clone()));
        Op::add(FILTER, a)
    };
    let chain = || ("chain", "forward".to_string());
    let mut ops = clear;
    let mut warnings = vec![];
    let what = match action.as_str() {
        "block" => {
            if print(r, "/interface/list", &["name"], Some(("name", "WAN")))?.is_empty() {
                return Err("no WAN interface list: blocking internet needs it (it is in the default config)".into());
            }
            for w in &times {
                ops.push(fw(vec![chain(), ("out-interface-list", "WAN".into()), ("action", "drop".into())], w));
            }
            "block internet".to_string()
        }
        "domains" => {
            if domains.is_empty() || domains.len() > 16 {
                return Err("give 1-16 domains".into());
            }
            let mut ds: Vec<String> = vec![];
            for d in &domains {
                let d = dns_name(d)?;
                if !d.contains('.') {
                    return Err("give full domains, e.g. example.com".into());
                }
                if !ds.contains(&d) {
                    ds.push(d);
                }
            }
            for w in &times {
                // QUIC (UDP 443) carries no TLS-host we can match: drop it so apps fall back to TCP
                ops.push(fw(vec![chain(), ("protocol", "udp".into()), ("dst-port", "443".into()), ("action", "drop".into())], w));
                for d in &ds {
                    for host in [d.clone(), format!("*.{d}")] {
                        ops.push(fw(vec![chain(), ("protocol", "tcp".into()), ("dst-port", "443".into()), ("tls-host", host), ("action", "reject".into()), ("reject-with", "tcp-reset".into())], w));
                    }
                }
            }
            warnings.push("Blocks HTTPS by TLS host name for these devices only, and all QUIC (UDP 443) from them so apps fall back to TCP. Plain HTTP, encrypted ClientHello (ECH) and VPNs get around it.".to_string());
            format!("block {}", ds.join(", "))
        }
        "limit" => {
            let (down, up) = (rate(&down)?, rate(if up.trim().is_empty() { &down } else { &up })?);
            for (n, w) in times.iter().enumerate() {
                let qname = if n == 0 { format!("tik-rule-{name}") } else { format!("tik-rule-{name}-{}", n + 1) };
                let mut a = vec![("name", qname), ("target", targets.join(",")), ("max-limit", format!("{up}/{down}"))];
                when(&mut a, w);
                a.push(("comment", c.clone()));
                ops.push(Op::add(QUEUE, a));
            }
            warnings.push(super::internet::FASTTRACK.to_string());
            if target == "group" {
                warnings.push("Group members share one limit, by their IP address at the time you save.".into());
            }
            format!("limit to {down} down / {up} up")
        }
        _ => return Err("action must be block, domains or limit".into()),
    };
    if times[0].is_some() {
        warnings.push("The schedule uses the router's clock and time zone (System → Time).".into());
    }
    let mut undo = vec![remove(FILTER, &c), remove(QUEUE, &c)];
    let edit = !old.is_empty();
    // restored rules go to the end of the chain: an `.id` from now may be gone when the revert runs
    undo.extend(old);
    let mut p = plan(format!("{} traffic rule {name}: {what}", if edit { "Change" } else { "Add" }), Risk::Low, ops, undo);
    p.warnings = warnings;
    Ok(p)
}

/// The client's IP from its DHCP lease, else ARP (queues match addresses, not MACs).
fn client_ip(r: &mut dyn Router, mac: &str) -> Result<String, String> {
    for path in ["/ip/dhcp-server/lease", "/ip/arp"] {
        if let Some(a) = print(r, path, &["address"], Some(("mac-address", mac)))?.iter().find_map(|x| x.get("address")) {
            return Ok(format!("{}/32", ipv4(a)?));
        }
    }
    Err(format!("no IP address known for {mac}: a speed limit needs one (give it a fixed IP)"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ros::queries::tests::{row, Fake};
    use serde_json::json;

    #[test]
    fn compiles_reverses_and_rejects_hostile_input() {
        let fake = || {
            let mut f = Fake::default();
            f.answers.insert("/interface/list/print".into(), Ok(vec![row(&[("name", "WAN")])]));
            f
        };
        let mut f = fake();
        let go = |v: serde_json::Value, f: &mut Fake| compile(&v, f).expect("own kind");

        // overnight block, Mon and Fri: two windows, the second on the next day
        let p = go(json!({"kind":"traffic-rule","name":"bedtime","target":"mac","value":"aa-bb-cc-dd-ee-ff","action":"block","days":["fri","mon"],"from":"22:00","to":"07:30"}), &mut f).unwrap();
        assert_eq!(p.cli()[2], "/ip/firewall/filter/add chain=forward src-mac-address=AA:BB:CC:DD:EE:FF out-interface-list=WAN action=drop time=22:00:00-23:59:59,mon,fri comment=tik:rule:bedtime");
        assert!(p.cli()[3].contains("time=00:00:00-07:30:00,tue,sat"));
        assert_eq!(p.undo_cli(), [r#"/ip/firewall/filter/remove [find comment="tik:rule:bedtime"]"#, r#"/queue/simple/remove [find comment="tik:rule:bedtime"]"#]);

        // edit of an existing limit: the undo removes the new one and restores the old queue
        f.answers.insert("/queue/simple/print".into(), Ok(vec![row(&[("name", "tik-rule-net"), ("target", "10.0.5.0/24"), ("max-limit", "1M/2M")])]));
        let p = go(json!({"kind":"traffic-rule","name":"net","target":"network","value":"10.0.5.0/24","action":"limit","down":"20M","up":"5M"}), &mut f).unwrap();
        assert_eq!(p.cli()[2], "/queue/simple/add name=tik-rule-net target=10.0.5.0/24 max-limit=5M/20M comment=tik:rule:net");
        assert_eq!(p.undo_cli()[2], "/queue/simple/add name=tik-rule-net target=10.0.5.0/24 max-limit=1M/2M comment=tik:rule:net");
        assert!(p.title.starts_with("Change"));

        for bad in [
            json!({"kind":"traffic-rule","name":"x\"; /system/reboot","target":"mac","value":"AA:BB:CC:DD:EE:FF","action":"block"}),
            json!({"kind":"traffic-rule","name":"x","target":"mac","value":"AA:BB:CC:DD:EE:FF; /system/reboot","action":"block"}),
            json!({"kind":"traffic-rule","name":"x","target":"network","value":"8.8.8.0/24","action":"block"}),
            json!({"kind":"traffic-rule","name":"x","target":"interface","value":"ether1","action":"block"}),
            json!({"kind":"traffic-rule","name":"x","target":"mac","value":"AA:BB:CC:DD:EE:FF","action":"domains","domains":["evil.com\" action=accept"]}),
            json!({"kind":"traffic-rule","name":"x","target":"mac","value":"AA:BB:CC:DD:EE:FF","action":"block","days":["funday"]}),
            json!({"kind":"traffic-rule","name":"x","target":"mac","value":"AA:BB:CC:DD:EE:FF","action":"block","from":"25:00","to":"07:00"}),
            json!({"kind":"traffic-rule","name":"x","target":"network","value":"10.0.0.0/24","action":"limit","down":"1M;x"}),
            json!({"kind":"traffic-rule","name":"x","target":"mac","value":"AA:BB:CC:DD:EE:FF","action":"block","chain":"input"}),
            json!({"kind":"remove-traffic-rule","name":"none"}),
        ] {
            assert!(compile(&bad, &mut fake()).unwrap().is_err(), "{bad}");
        }
    }
}
