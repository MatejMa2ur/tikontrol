//! The router snapshot, read over the binary API in the same JSON shape as
//! `src/scripts/snapshot.py`, so the UI (`src/ui/src/lib/data.ts`) reads either one.
//!
//! Every read names its fields (`.proplist`): keys, passphrases, script bodies and the
//! Back To Home private key are never requested, so they can't leak even by mistake.

use super::cmd::Cmd;
use super::conn::{Router, Row};
use serde_json::{json, Map, Value};

#[derive(Clone, Copy)]
enum Shape {
    List,
    One,
}

struct Q {
    key: &'static str,
    path: &'static str,
    props: &'static [&'static str],
    args: &'static [(&'static str, &'static str)],
    query: &'static [(&'static str, &'static str)],
    shape: Shape,
    /// fields RouterOS returns as comma-separated lists; `:serialize` gives them as arrays
    arrays: &'static [&'static str],
}

const fn q(key: &'static str, path: &'static str, props: &'static [&'static str]) -> Q {
    Q { key, path, props, args: &[], query: &[], shape: Shape::List, arrays: &[] }
}
const fn one(key: &'static str, path: &'static str, props: &'static [&'static str]) -> Q {
    Q { key, path, props, args: &[], query: &[], shape: Shape::One, arrays: &[] }
}

impl Q {
    const fn arrays(mut self, a: &'static [&'static str]) -> Q {
        self.arrays = a;
        self
    }
    const fn args(mut self, a: &'static [(&'static str, &'static str)]) -> Q {
        self.args = a;
        self
    }
    const fn query(mut self, a: &'static [(&'static str, &'static str)]) -> Q {
        self.query = a;
        self
    }
}

const QUERIES: &[Q] = &[
    one("resource", "/system/resource/print", &["uptime", "version", "build-time", "free-memory", "total-memory", "cpu", "cpu-count", "cpu-frequency", "cpu-load", "free-hdd-space", "total-hdd-space", "architecture-name", "board-name", "platform"]),
    one("routerboard", "/system/routerboard/print", &["model", "current-firmware", "upgrade-firmware"]),
    one("update", "/system/package/update/print", &["channel", "installed-version", "latest-version"]),
    q("wan", "/ip/dhcp-client/print", &["interface", "address", "gateway", "status"]),
    // never without a proplist: /ip/cloud also holds the Back To Home private key
    one("cloud", "/ip/cloud/print", &["public-address", "dns-name", "back-to-home-vpn"]),
    q("interfaces", "/interface/print", &["name", "type", "running", "disabled", "rx-byte", "tx-byte", "comment", "mac-address"]),
    q("ethernet", "/interface/ethernet/print", &["name", "running", "disabled"]),
    q("wifi", "/interface/wireless/print", &["name", "ssid", "band", "frequency", "channel-width", "running", "disabled", "mode"]),
    q("stations", "/interface/wireless/registration-table/print", &["interface", "mac-address", "signal-strength", "tx-rate", "rx-rate", "uptime", "bytes", "last-ip"]),
    q("leases", "/ip/dhcp-server/lease/print", &["address", "mac-address", "host-name", "comment", "status", "dynamic", "last-seen"]),
    q("arp", "/ip/arp/print", &["address", "mac-address", "interface", "status"]),
    q("bridgeHosts", "/interface/bridge/host/print", &["mac-address", "interface", "on-interface", "local"]),
    one("dns", "/ip/dns/print", &["cache-size", "cache-used"]),
    q("adlist", "/ip/dns/adlist/print", &["url", "name-count", "match-count", "disabled"]),
    q("filter", "/ip/firewall/filter/print", &["chain", "action", "comment", "disabled", "in-interface-list", "connection-state"]).arrays(&["connection-state"]),
    q("nat", "/ip/firewall/nat/print", &["chain", "action", "protocol", "dst-port", "to-addresses", "to-ports", "comment", "disabled"]),
    q("addresses", "/ip/address/print", &["address", "interface", "comment"]),
    q("linkQuality", "/interface/wireless/registration-table/print", &["mac-address", "tx-ccq", "p-throughput"]),
    // ponytail: dotted proplist names are unverified over the API (docs/NEXT_STEPS.md)
    q("wifiNew", "/interface/wifi/print", &["name", "channel.band", "channel.width", "configuration.ssid", "running", "disabled"]),
    q("stationsNew", "/interface/wifi/registration-table/print", &["interface", "mac-address", "signal", "tx-rate", "rx-rate", "uptime", "bytes", "last-ip", "band", "ssid"]),
    q("wireguard", "/interface/wireguard/print", &["name", "listen-port", "mtu", "running", "disabled", "comment"]),
    q("wgPeers", "/interface/wireguard/peers/print", &["interface", "name", "allowed-address", "endpoint-address", "endpoint-port", "last-handshake", "rx", "tx", "comment", "disabled"]).arrays(&["allowed-address"]),
    q("routes", "/ip/route/print", &["dst-address", "gateway", "distance", "routing-table", "active", "dynamic", "static", "comment", "disabled"]),
    q("dnsStatic", "/ip/dns/static/print", &["name", "address", "type", "comment", "disabled"]),
    one("dnsCfg", "/ip/dns/print", &["servers", "use-doh-server", "allow-remote-requests", "mdns-repeat-ifaces"]).arrays(&["servers", "mdns-repeat-ifaces"]),
    q("dnsCache", "/ip/dns/cache/print", &["name", "type", "data", "ttl"]),
    q("queues", "/queue/simple/print", &["name", "target", "max-limit", "disabled", "comment"]).arrays(&["target"]),
    q("hotspot", "/ip/hotspot/print", &["name", "interface", "address-pool", "profile", "disabled"]),
    q("users", "/user/print", &["name", "group", "last-logged-in", "disabled", "comment"]),
    q("userGroups", "/user/group/print", &["name", "policy"]).arrays(&["policy"]),
    // no on-event: script bodies can hold secrets
    q("scheduler", "/system/scheduler/print", &["name", "start-time", "interval", "disabled", "next-run", "comment"]),
    one("clock", "/system/clock/print", &["time-zone-name", "date", "time"]),
    one("ntp", "/system/ntp/client/print", &["enabled", "servers"]).arrays(&["servers"]),
    one("snmp", "/snmp/print", &["enabled", "contact", "location"]),
    one("upnp", "/ip/upnp/print", &["enabled"]),
    q("pools", "/ip/pool/print", &["name", "ranges"]).arrays(&["ranges"]),
    q("dhcpServers", "/ip/dhcp-server/print", &["name", "interface", "address-pool", "lease-time", "disabled"]),
    q("dhcpNetworks", "/ip/dhcp-server/network/print", &["address", "gateway", "dns-server", "domain", "comment"]).arrays(&["gateway"]),
    q("bridges", "/interface/bridge/print", &["name", "vlan-filtering", "protocol-mode", "igmp-snooping", "comment"]),
    q("bridgePorts", "/interface/bridge/port/print", &["interface", "bridge", "pvid", "frame-types", "hw", "disabled", "comment"]),
    q("bridgeVlans", "/interface/bridge/vlan/print", &["bridge", "vlan-ids", "tagged", "untagged"]).arrays(&["vlan-ids", "tagged", "untagged"]),
    q("listMembers", "/interface/list/member/print", &["list", "interface", "comment"]),
    q("netwatch", "/tool/netwatch/print", &["host", "status", "interval", "type", "comment"]),
    q("loggingRules", "/system/logging/print", &["topics", "action", "disabled"]).arrays(&["topics"]),
    q("neighbors", "/ip/neighbor/print", &["interface", "address", "mac-address", "identity", "platform", "board", "version"]),
    // no wpa/wpa2 pre-shared keys
    q("secProfiles", "/interface/wireless/security-profiles/print", &["name", "mode", "authentication-types", "management-protection"]).arrays(&["authentication-types"]),
    q("accessList", "/interface/wireless/access-list/print", &["mac-address", "interface", "authentication", "forwarding", "comment"]),
    one("trafficFlow", "/ip/traffic-flow/print", &["enabled"]),
    q("ovpn", "/interface/ovpn-server/server/print", &["name", "disabled", "port", "protocol"]),
    one("l2tp", "/interface/l2tp-server/server/print", &["enabled"]),
    q("backups", "/file/print", &["name", "type", "size", "last-modified"]).query(&[("type", "backup")]),
    q("addressLists", "/ip/firewall/address-list/print", &["list", "address", "dynamic", "comment"]),
    q("etherStats", "/interface/print", &["name", "rx-byte", "tx-byte", "rx-error", "tx-error", "rx-drop", "tx-drop"]).args(&[("stats", "")]).query(&[("type", "ether")]),
    one("ipv6", "/ipv6/settings/print", &["disable-ipv6"]),
    q("kidControl", "/ip/kid-control/print", &["name", "rate-limit", "disabled"]),
    q("services", "/ip/service/print", &["name", "port", "disabled", "address", "certificate"]),
    q("log", "/log/print", &["time", "topics", "message"]).arrays(&["topics"]),
    // optional: MikroTik CMR (7.26+, `cmr` package). Routers without it just report an error for this key.
    // ponytail: field names from the CMR manual, unverified over the API
    q("cmrDevices", "/cmr/device/print", &["name", "identity", "model", "version", "state", "address", "labels"]).arrays(&["labels"]),
];

/// Fields that are text even when they look like numbers (a host named "1234" stays a string).
const TEXT: &[&str] = &["name", "comment", "host-name", "ssid", "identity", "message", "address", "version", "board", "url", "list", "contact", "location", "configuration.ssid", "target", "max-limit", "rate-limit", "dst-port", "to-ports", "vlan-ids"];

/// RouterOS API values are all strings; convert them the way `:serialize to=json` does.
pub fn typed(key: &str, v: &str, arrays: &[&str]) -> Value {
    if arrays.contains(&key) {
        return Value::Array(v.split(',').filter(|s| !s.is_empty()).map(|s| Value::String(s.into())).collect());
    }
    if TEXT.contains(&key) {
        return Value::String(v.into());
    }
    match v {
        "true" | "yes" => return Value::Bool(true),
        "false" | "no" => return Value::Bool(false),
        _ => {}
    }
    let digits = v.strip_prefix('-').unwrap_or(v);
    if !digits.is_empty() && digits.len() < 18 && digits.bytes().all(|b| b.is_ascii_digit()) && (digits == "0" || !digits.starts_with('0')) {
        if let Ok(n) = v.parse::<i64>() {
            return json!(n);
        }
    }
    Value::String(v.into())
}

fn row_json(r: &Row, arrays: &[&str]) -> Value {
    Value::Object(r.iter().map(|(k, v)| (k.clone(), typed(k, v, arrays))).collect())
}

fn rows_json(rows: &[Row], arrays: &[&str]) -> Value {
    Value::Array(rows.iter().map(|r| row_json(r, arrays)).collect())
}

fn build(q: &Q) -> Cmd {
    let mut c = Cmd::new(q.path);
    for (k, v) in q.args {
        c = c.arg(k, v);
    }
    c = c.proplist(q.props);
    for (k, v) in q.query {
        c = c.query(k, v);
    }
    c
}

/// Parses `/ping` timings like "17ms800us" into milliseconds.
pub fn rtt_ms(v: &str) -> Option<f64> {
    let mut ms = 0.0;
    let mut num = String::new();
    let mut any = false;
    let mut chars = v.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
            continue;
        }
        let n: f64 = num.parse().ok()?;
        num.clear();
        any = true;
        match c {
            'm' if chars.peek() == Some(&'s') => {
                chars.next();
                ms += n
            }
            'u' if chars.peek() == Some(&'s') => {
                chars.next();
                ms += n / 1000.0
            }
            's' => ms += n * 1000.0,
            _ => return None,
        }
    }
    if !num.is_empty() {
        return None;
    }
    any.then_some((ms * 10.0).round() / 10.0)
}

pub const RESOLVERS: &[(&str, &str)] = &[("Cloudflare", "1.1.1.1"), ("Google", "8.8.8.8"), ("Quad9", "9.9.9.9")];

/// Reads the full snapshot. One failing query (missing package, older RouterOS) only drops its key
/// and adds a note to `_errors`, like `snapshot.py` warns and carries on.
pub fn snapshot(r: &mut dyn Router, with_latency: bool) -> Value {
    let mut out = Map::new();
    let mut errors = Vec::new();
    let mut run = |out: &mut Map<String, Value>, key: &str, cmd: Cmd, shape: Shape, arrays: &[&str]| match r.run(&cmd) {
        Ok(rows) => {
            let v = match shape {
                Shape::List => rows_json(&rows, arrays),
                Shape::One => rows.first().map_or(Value::Object(Map::new()), |row| row_json(row, arrays)),
            };
            out.insert(key.into(), v);
        }
        Err(e) => errors.push(json!({ "key": key, "error": e })),
    };

    run(&mut out, "identityRow", Cmd::new("/system/identity/print").proplist(&["name"]), Shape::One, &[]);
    let id = out.remove("identityRow").and_then(|v| v.get("name").cloned()).unwrap_or(Value::String("router".into()));
    out.insert("identity".into(), id);
    for q in QUERIES {
        run(&mut out, q.key, build(q), q.shape, q.arrays);
    }

    // queries that depend on what was read above
    let wan_if = out["wan"].get(0).and_then(|w| w["interface"].as_str()).unwrap_or("ether1").to_string();
    run(&mut out, "wanTraffic", Cmd::new("/interface/monitor-traffic").arg("interface", &wan_if).arg("once", "").proplist(&["name", "rx-bits-per-second", "tx-bits-per-second"]), Shape::One, &[]);
    run(&mut out, "wanStats", Cmd::new("/interface/print").arg("stats", "").proplist(&["name", "rx-byte", "tx-byte"]).query("name", &wan_if), Shape::List, &[]);
    let names = |key: &str| -> Vec<String> {
        out.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x["name"].as_str().map(String::from)).collect()).unwrap_or_default()
    };
    let (eth, wl) = (names("ethernet"), names("wifi"));
    if !eth.is_empty() {
        run(&mut out, "etherMon", Cmd::new("/interface/ethernet/monitor").arg("numbers", &eth.join(",")).arg("once", "").proplist(&["name", "status", "rate", "full-duplex", "auto-negotiation"]), Shape::List, &[]);
    }
    if !wl.is_empty() {
        run(&mut out, "radios", Cmd::new("/interface/wireless/monitor").arg("numbers", &wl.join(",")).arg("once", "").proplist(&["channel", "noise-floor", "overall-tx-ccq", "registered-clients", "status"]), Shape::List, &[]);
    }

    trim(&mut out);
    if with_latency {
        let mut lat = Map::new();
        for (name, ip) in RESOLVERS {
            let v = match r.run(&Cmd::new("/ping").arg("address", ip).arg("count", "3")) {
                Ok(rows) => {
                    let last = rows.last();
                    let ms = last.and_then(|x| x.get("avg-rtt")).and_then(|v| rtt_ms(v));
                    let loss = last.and_then(|x| x.get("packet-loss")).and_then(|v| v.parse::<i64>().ok());
                    json!({ "ms": ms, "loss": loss })
                }
                Err(_) => json!({ "ms": null, "loss": null }),
            };
            lat.insert((*name).into(), v);
        }
        out.insert("latency".into(), Value::Object(lat));
    }
    if !errors.is_empty() {
        out.insert("_errors".into(), Value::Array(errors));
    }
    Value::Object(out)
}

/// Same trimming as snapshot.py: keep the UI payload small.
fn trim(out: &mut Map<String, Value>) {
    if let Some(Value::Array(log)) = out.get_mut("log") {
        log.retain(|l| l["message"].as_str().is_some_and(|m| !m.contains("via ssh") && !m.contains("publickey accepted") && !m.contains("via api")));
        let n = log.len().saturating_sub(40);
        log.drain(..n);
    }
    if let Some(Value::Array(c)) = out.get_mut("dnsCache") {
        c.retain(|x| matches!(x["type"].as_str(), Some("A" | "AAAA")));
        c.truncate(300);
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Fake router: answers by command path, records every command it saw.
    #[derive(Default)]
    pub struct Fake {
        pub answers: HashMap<String, Result<Vec<Row>, String>>,
        pub seen: Vec<Vec<String>>,
    }
    pub fn row(kv: &[(&str, &str)]) -> Row {
        kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }
    impl Router for Fake {
        fn run(&mut self, cmd: &Cmd) -> Result<Vec<Row>, String> {
            self.seen.push(cmd.words()?.to_vec());
            self.answers.get(cmd.path()).cloned().unwrap_or(Ok(vec![]))
        }
    }

    #[test]
    fn typed_matches_serialize() {
        assert_eq!(typed("running", "true", &[]), json!(true));
        assert_eq!(typed("cpu-load", "7", &[]), json!(7));
        assert_eq!(typed("signal", "-61", &[]), json!(-61));
        assert_eq!(typed("version", "7.20", &[]), json!("7.20"));
        assert_eq!(typed("mac-address", "00:11:22:33:44:55", &[]), json!("00:11:22:33:44:55"));
        assert_eq!(typed("name", "1234", &[]), json!("1234"));
        assert_eq!(typed("comment", "true", &[]), json!("true"));
        assert_eq!(typed("pvid", "007", &[]), json!("007"));
        assert_eq!(typed("topics", "wireless,info", &["topics"]), json!(["wireless", "info"]));
        assert_eq!(typed("servers", "", &["servers"]), json!([]));
    }

    #[test]
    fn rtt_parses_routeros_durations() {
        assert_eq!(rtt_ms("17ms800us"), Some(17.8));
        assert_eq!(rtt_ms("8ms"), Some(8.0));
        assert_eq!(rtt_ms("950us"), Some(1.0));
        assert_eq!(rtt_ms("1s5ms"), Some(1005.0));
        assert_eq!(rtt_ms(""), None);
        assert_eq!(rtt_ms("12"), None);
        assert_eq!(rtt_ms("x"), None);
    }

    #[test]
    fn every_read_has_an_explicit_proplist() {
        let mut f = Fake::default();
        f.answers.insert("/interface/ethernet/print".into(), Ok(vec![row(&[("name", "ether1")])]));
        f.answers.insert("/interface/wireless/print".into(), Ok(vec![row(&[("name", "wlan1")])]));
        snapshot(&mut f, true);
        for w in &f.seen {
            if w[0] == "/ping" {
                continue;
            }
            assert!(w.iter().any(|x| x.starts_with("=.proplist=")), "{w:?}");
        }
        let cloud = f.seen.iter().find(|w| w[0] == "/ip/cloud/print").unwrap();
        assert_eq!(cloud[1], "=.proplist=public-address,dns-name,back-to-home-vpn");
        assert!(!f.seen.iter().flatten().any(|w| w.contains("private-key") || w.contains("on-event") || w.contains("passphrase")));
    }

    #[test]
    fn snapshot_has_the_ui_shape() {
        let mut f = Fake::default();
        f.answers.insert("/system/identity/print".into(), Ok(vec![row(&[("name", "Router")])]));
        f.answers.insert("/system/resource/print".into(), Ok(vec![row(&[("cpu-load", "3"), ("board-name", "hAP ac^2")])]));
        f.answers.insert("/ip/dhcp-client/print".into(), Ok(vec![row(&[("interface", "ether1"), ("address", "10.0.0.2/24")])]));
        f.answers.insert("/log/print".into(), Ok(vec![row(&[("message", "user admin logged in via ssh"), ("topics", "system,info")]), row(&[("message", "x"), ("topics", "dhcp,info")])]));
        f.answers.insert("/interface/wireless/print".into(), Err("no such command prefix".into()));
        f.answers.insert("/ping".into(), Ok(vec![row(&[("avg-rtt", "9ms"), ("packet-loss", "0")])]));
        let s = snapshot(&mut f, true);
        assert_eq!(s["identity"], "Router");
        assert_eq!(s["resource"]["cpu-load"], 3);
        assert_eq!(s["wan"][0]["interface"], "ether1");
        assert_eq!(s["log"].as_array().unwrap().len(), 1);
        assert_eq!(s["log"][0]["topics"], json!(["dhcp", "info"]));
        assert_eq!(s["latency"]["Quad9"]["ms"], 9.0);
        assert!(s.get("wifi").is_none());
        assert_eq!(s["_errors"][0]["key"], "wifi");
        assert!(s["routerboard"].is_object());
    }
}
