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
    one("cloud", "/ip/cloud/print", &["public-address", "dns-name", "back-to-home-vpn", "ddns-enabled"]),
    q("interfaces", "/interface/print", &["name", "type", "running", "disabled", "rx-byte", "tx-byte", "comment", "mac-address"]),
    q("ethernet", "/interface/ethernet/print", &["name", "running", "disabled"]),
    q("wifi", "/interface/wireless/print", &["name", "ssid", "band", "frequency", "channel-width", "running", "disabled", "mode", "master-interface", "security-profile", "hide-ssid", "default-forwarding", "default-ap-tx-limit", "default-client-tx-limit", "comment"]),
    q("stations", "/interface/wireless/registration-table/print", &["interface", "mac-address", "signal-strength", "tx-rate", "rx-rate", "uptime", "bytes", "last-ip"]),
    q("leases", "/ip/dhcp-server/lease/print", &["address", "mac-address", "host-name", "comment", "status", "dynamic", "last-seen", "class-id"]),
    q("arp", "/ip/arp/print", &["address", "mac-address", "interface", "status"]),
    q("bridgeHosts", "/interface/bridge/host/print", &["mac-address", "interface", "on-interface", "local"]),
    one("dns", "/ip/dns/print", &["cache-size", "cache-used"]),
    q("adlist", "/ip/dns/adlist/print", &["url", "name-count", "match-count", "disabled"]),
    q("filter", "/ip/firewall/filter/print", &["chain", "action", "comment", "disabled", "in-interface-list", "connection-state", "src-mac-address", "src-address", "src-address-list", "protocol", "tls-host", "time", "out-interface-list", "dst-address", "dst-address-list", "dst-port"]).arrays(&["connection-state"]),
    q("nat", "/ip/firewall/nat/print", &["chain", "action", "protocol", "dst-port", "to-addresses", "to-ports", "comment", "disabled"]),
    q("addresses", "/ip/address/print", &["address", "interface", "comment"]),
    q("linkQuality", "/interface/wireless/registration-table/print", &["mac-address", "tx-ccq", "p-throughput"]),
    // ponytail: dotted proplist names are unverified over the API (docs/NEXT_STEPS.md)
    q("wifiNew", "/interface/wifi/print", &["name", "channel.band", "channel.width", "configuration.ssid", "running", "disabled", "master-interface", "configuration.hide-ssid", "datapath.client-isolation", "security.authentication-types", "security.ft", "steering.rrm", "steering.wnm", "interworking", "comment"]).arrays(&["security.authentication-types"]),
    // Wi-Fi 7: MLO lives on /interface/wifi/network; a radio that can do it reports an ml-group
    q("wifiNetworks", "/interface/wifi/network/print", &["name", "ssid", "mlo", "disabled"]),
    q("wifiRadios", "/interface/wifi/radio/print", &["name", "ml-group"]),
    // no passphrase: private PSKs are never read
    q("accessListNew", "/interface/wifi/access-list/print", &["mac-address", "interface", "signal-range", "action", "vlan-id", "comment"]),
    q("stationsNew", "/interface/wifi/registration-table/print", &["interface", "mac-address", "signal", "tx-rate", "rx-rate", "uptime", "bytes", "last-ip", "band", "ssid"]),
    q("wireguard", "/interface/wireguard/print", &["name", "listen-port", "mtu", "running", "disabled", "comment", "public-key"]),
    q("wgPeers", "/interface/wireguard/peers/print", &["interface", "name", "allowed-address", "endpoint-address", "endpoint-port", "last-handshake", "rx", "tx", "comment", "disabled", "public-key"]).arrays(&["allowed-address"]),
    // no private-key: Back To Home users carry their WireGuard keys
    q("bthUsers", "/ip/cloud/back-to-home-users/print", &["name", "allow-lan", "disabled", "comment", "expires"]),
    q("routingRules", "/routing/rule/print", &["src-address", "dst-address", "action", "table", "interface", "routing-mark", "disabled", "comment"]),
    q("routingTables", "/routing/table/print", &["name", "fib", "comment"]),
    // read-only views; no BGP tcp-md5-key or OSPF auth keys
    q("ospfInstances", "/routing/ospf/instance/print", &["name", "version", "router-id", "disabled"]),
    q("ospfNeighbors", "/routing/ospf/neighbor/print", &["instance", "address", "router-id", "state", "adjacency"]),
    q("bgpSessions", "/routing/bgp/session/print", &["name", "remote.address", "remote.as", "local.as", "established", "uptime", "prefix-count"]),
    q("routes", "/ip/route/print", &["dst-address", "gateway", "distance", "routing-table", "active", "dynamic", "static", "comment", "disabled"]),
    q("dnsStatic", "/ip/dns/static/print", &["name", "address", "type", "comment", "disabled"]),
    one("dnsCfg", "/ip/dns/print", &["servers", "use-doh-server", "allow-remote-requests", "mdns-repeat-ifaces"]).arrays(&["servers", "mdns-repeat-ifaces"]),
    q("dnsCache", "/ip/dns/cache/print", &["name", "type", "data", "ttl"]),
    q("queues", "/queue/simple/print", &["name", "target", "max-limit", "disabled", "comment", "time"]).arrays(&["target"]),
    // speed profiles and QoS rules are queue tree entries (tik:profile:<group>, tik:qos:<name>)
    q("queueTree", "/queue/tree/print", &["name", "parent", "packet-mark", "queue", "priority", "max-limit", "comment", "disabled"]),
    q("queueTypes", "/queue/type/print", &["name", "kind", "pcq-rate"]).query(&[("kind", "pcq")]),
    q("hotspot", "/ip/hotspot/print", &["name", "interface", "address-pool", "profile", "disabled"]),
    q("hotspotActive", "/ip/hotspot/active/print", &["server", "user", "address", "mac-address", "login-by", "uptime", "session-time-left", "bytes-in", "bytes-out"]),
    q("hotspotProfiles", "/ip/hotspot/profile/print", &["name", "hotspot-address", "login-by", "html-directory", "trial-uptime-limit"]).arrays(&["login-by"]),
    q("hotspotUserProfiles", "/ip/hotspot/user/profile/print", &["name", "session-timeout", "rate-limit", "shared-users"]),
    // vouchers: never the password field
    q("hotspotUsers", "/ip/hotspot/user/print", &["name", "profile", "limit-uptime", "uptime", "comment", "disabled"]),
    q("users", "/user/print", &["name", "group", "last-logged-in", "disabled", "comment"]),
    // fingerprints only, never the key itself (public, but not needed)
    q("sshKeys", "/user/ssh-keys/print", &["user", "key-type", "bits", "info", "fingerprint", "comment"]),
    q("userGroups","/user/group/print", &["name", "policy"]).arrays(&["policy"]),
    // no on-event: script bodies can hold secrets
    q("scheduler", "/system/scheduler/print", &["name", "start-time", "interval", "disabled", "next-run", "comment"]),
    one("clock", "/system/clock/print", &["time-zone-name", "date", "time"]),
    one("ntp", "/system/ntp/client/print", &["enabled", "servers"]).arrays(&["servers"]),
    one("snmp", "/snmp/print", &["enabled", "contact", "location"]),
    one("upnp", "/ip/upnp/print", &["enabled"]),
    q("pools", "/ip/pool/print", &["name", "ranges"]).arrays(&["ranges"]),
    q("dhcpServers", "/ip/dhcp-server/print", &["name", "interface", "address-pool", "lease-time", "disabled"]),
    q("dhcpNetworks", "/ip/dhcp-server/network/print", &["address", "gateway", "dns-server", "domain", "dhcp-option", "comment"]).arrays(&["gateway", "dhcp-option"]),
    q("dhcpOptions", "/ip/dhcp-server/option/print", &["name", "code", "value", "comment"]),
    q("vlans", "/interface/vlan/print", &["name", "vlan-id", "interface", "comment"]),
    q("bridges", "/interface/bridge/print", &["name", "vlan-filtering", "protocol-mode", "igmp-snooping", "comment"]),
    // device isolation (tik:isolate:<mac>) lives here
    q("bridgeFilter", "/interface/bridge/filter/print", &["chain", "action", "src-mac-address", "dst-mac-address", "disabled", "comment"]),
    q("bridgePorts", "/interface/bridge/port/print", &["interface", "bridge", "pvid", "frame-types", "hw", "disabled", "comment", "edge", "bpdu-guard"]),
    // Ports advanced: each menu only answers when the router / switch chip has it (feature detection)
    q("bonds", "/interface/bonding/print", &["name", "slaves", "mode", "running", "disabled", "comment"]).arrays(&["slaves"]),
    q("switches", "/interface/ethernet/switch/print", &["name", "type", "mirror-source", "mirror-target"]),
    q("switchPorts", "/interface/ethernet/switch/port/print", &["name", "storm-rate"]),
    // optional: only boards with PoE-out have this menu; the others report an error (or nothing) for this key
    q("poe", "/interface/ethernet/poe/print", &["name", "poe-out", "poe-priority", "poe-voltage"]),
    q("dot1x", "/interface/dot1x/server/print", &["interface", "auth-types", "disabled", "comment"]).arrays(&["auth-types"]),
    // never the RADIUS secret
    q("radius", "/radius/print", &["address", "service", "comment", "disabled"]).arrays(&["service"]),
    one("discovery", "/ip/neighbor/discovery-settings/print", &["discover-interface-list", "protocol", "lldp-med-net-policy-vlan"]).arrays(&["protocol"]),
    q("bridgeVlans", "/interface/bridge/vlan/print", &["bridge", "vlan-ids", "tagged", "untagged"]).arrays(&["vlan-ids", "tagged", "untagged"]),
    q("listMembers", "/interface/list/member/print", &["list", "interface", "comment"]),
    q("netwatch", "/tool/netwatch/print", &["host", "status", "interval", "type", "comment"]),
    q("loggingRules", "/system/logging/print", &["topics", "action", "disabled"]).arrays(&["topics"]),
    q("neighbors", "/ip/neighbor/print", &["interface", "address", "mac-address", "identity", "platform", "board", "version"]),
    // no wpa/wpa2 pre-shared keys
    q("secProfiles", "/interface/wireless/security-profiles/print", &["name", "mode", "authentication-types", "management-protection"]).arrays(&["authentication-types"]),
    q("accessList", "/interface/wireless/access-list/print", &["mac-address", "interface", "signal-range", "authentication", "forwarding", "vlan-id", "comment"]),
    // optional: boards without sensors return nothing (or an error) for this key
    q("health", "/system/health/print", &["name", "value", "type"]),
    one("trafficFlow", "/ip/traffic-flow/print", &["enabled"]),
    // 7.17+ lists several servers with `disabled`; older versions have one with `enabled`
    q("ovpn", "/interface/ovpn-server/server/print", &["name", "disabled", "enabled", "port", "protocol", "certificate"]),
    // never ipsec-secret
    one("l2tp", "/interface/l2tp-server/server/print", &["enabled", "use-ipsec"]),
    // connected L2TP/OpenVPN users; names and caller-id come from remote devices: plain text only
    q("pppActive", "/ppp/active/print", &["name", "service", "caller-id", "address", "uptime", "encoding"]),
    // never password
    q("pppSecrets", "/ppp/secret/print", &["name", "service", "profile", "disabled", "comment"]),
    q("pppProfiles", "/ppp/profile/print", &["name"]),
    q("backups", "/file/print", &["name", "type", "size", "last-modified"]).query(&[("type", "backup")]),
    // tikontrol's text exports (tik-*.rsc); RouterOS lists .rsc files as type "script"
    q("exports", "/file/print", &["name", "type", "size", "last-modified"]).query(&[("type", "script")]),
    q("addressLists", "/ip/firewall/address-list/print", &["list", "address", "dynamic", "comment"]),
    q("etherStats", "/interface/print", &["name", "rx-byte", "tx-byte", "rx-error", "tx-error", "rx-drop", "tx-drop"]).args(&[("stats", "")]).query(&[("type", "ether")]),
    one("ipv6", "/ipv6/settings/print", &["disable-ipv6"]),
    q("ipv6Pools", "/ipv6/pool/print", &["name", "prefix", "prefix-length"]),
    q("ipv6Addresses", "/ipv6/address/print", &["address", "interface", "from-pool", "advertise", "dynamic", "comment"]),
    q("ipv6DhcpServers", "/ipv6/dhcp-server/print", &["name", "interface", "address-pool", "disabled", "comment"]),
    q("kidControl", "/ip/kid-control/print", &["name", "rate-limit", "disabled"]),
    q("services", "/ip/service/print", &["name", "port", "disabled", "address", "certificate", "dynamic"]),
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

pub fn rows_json(rows: &[Row], arrays: &[&str]) -> Value {
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

/// Demo mode (ros/demo.rs): the snapshot keys a plain `print` of `path` was read into, with the
/// fixed filter each one was read with. Reads with extra arguments (`stats`) are left out.
pub fn keys_for(path: &str) -> Vec<(&'static str, &'static [(&'static str, &'static str)])> {
    QUERIES.iter().filter(|q| q.path == path && q.args.is_empty()).map(|q| (q.key, q.query)).collect()
}

/// Console (read-only): a RouterOS command is accepted only if it is the `print` of a menu the snapshot
/// already reads, or a one-shot monitor of a named interface. It runs with the snapshot's vetted
/// proplist, so no field outside it (keys, secrets, script bodies) can be asked for. Returns the command
/// and the fields to show.
pub fn console(line: &str) -> Result<(Cmd, &'static [&'static str]), String> {
    const MON: &[&str] = &["name", "rx-bits-per-second", "tx-bits-per-second"];
    const ETH: &[&str] = &["name", "status", "rate", "full-duplex", "auto-negotiation"];
    let w: Vec<&str> = line.split_whitespace().collect();
    let path = match w.as_slice() {
        [p] => p.to_string(),
        [p, "print"] => format!("{}/print", p.trim_end_matches('/')),
        [p @ ("/interface/monitor-traffic" | "/interface/ethernet/monitor"), name, "once"] => {
            let n = crate::apply::ros_name(name)?;
            let props = if p.ends_with("monitor-traffic") { MON } else { ETH };
            let key = if props == MON { "interface" } else { "numbers" };
            return Ok((Cmd::new(p).arg(key, &n).arg("once", "").proplist(props), props));
        }
        _ => return Err("one command, no arguments: a menu's print (e.g. /ip/route/print) or /interface/monitor-traffic <name> once".into()),
    };
    let q = QUERIES.iter().find(|q| q.path == path && q.args.is_empty()).ok_or_else(|| format!("{path} isn't on the console's read-only list"))?;
    Ok((build(q), q.props))
}

/// The commands the console accepts, for the UI's suggestions.
pub fn console_commands() -> Vec<&'static str> {
    let mut v: Vec<&str> = QUERIES.iter().filter(|q| q.args.is_empty()).map(|q| q.path).collect();
    v.dedup();
    v
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
    let (eth, wl, wn, poe) = (names("ethernet"), names("wifi"), names("wifiNew"), names("poe"));
    if !eth.is_empty() {
        run(&mut out, "etherMon", Cmd::new("/interface/ethernet/monitor").arg("numbers", &eth.join(",")).arg("once", "").proplist(&["name", "status", "rate", "full-duplex", "auto-negotiation"]), Shape::List, &[]);
    }
    if !poe.is_empty() {
        // ponytail: field names from the PoE-out manual, unverified over the API; missing ones are just absent
        run(&mut out, "poeMon", Cmd::new("/interface/ethernet/poe/monitor").arg("numbers", &poe.join(",")).arg("once", "").proplist(&["name", "poe-out-status", "poe-out-voltage", "poe-out-current", "poe-out-power"]), Shape::List, &[]);
    }
    if !wl.is_empty() {
        run(&mut out, "radios", Cmd::new("/interface/wireless/monitor").arg("numbers", &wl.join(",")).arg("once", "").proplist(&["channel", "noise-floor", "overall-tx-ccq", "registered-clients", "status"]), Shape::List, &[]);
    }
    if !wn.is_empty() {
        // ponytail: field names from the wifi manual, unverified over the API; missing ones are just absent
        run(&mut out, "radiosNew", Cmd::new("/interface/wifi/monitor").arg("numbers", &wn.join(",")).arg("once", "").proplist(&["channel", "state", "registered-peers", "tx-power"]), Shape::List, &[]);
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
        // drop the API session noise (tikontrol's own polling logs in and out every few seconds), keep every login
        // failure and other logins: they are admin activity and security events
        log.retain(|l| l["message"].as_str().is_some_and(|m| !m.contains("publickey accepted") && !(m.contains("via api") && (m.contains(" logged in ") || m.contains(" logged out ")))));
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
    fn console_only_runs_vetted_reads() {
        let (c, _) = console("/ip/route print").unwrap();
        assert_eq!(c.words().unwrap()[0], "/ip/route/print");
        let (c, _) = console("/ip/cloud/print").unwrap();
        assert_eq!(c.words().unwrap()[1], "=.proplist=public-address,dns-name,back-to-home-vpn,ddns-enabled");
        assert!(console("/interface/monitor-traffic ether1 once").is_ok());
        for bad in ["/system/reboot", "/ip/route/print; /system/reboot", "/ip/route/print detail", "/system/script/print", "/interface/wireguard/peers/print where name=x", "/interface/monitor-traffic a;b once", "/interface/monitor-traffic ether1", "/ip/address/add address=1.2.3.4", ""] {
            assert!(console(bad).is_err(), "{bad}");
        }
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
        assert_eq!(cloud[1], "=.proplist=public-address,dns-name,back-to-home-vpn,ddns-enabled");
        assert!(!f.seen.iter().flatten().any(|w| w.contains("private-key") || w.contains("on-event") || w.contains("passphrase")));
        // PPP passwords and the L2TP IPsec secret are never asked for
        let props = |p: &str| f.seen.iter().find(|w| w[0] == p).unwrap()[1].clone();
        assert_eq!(props("/ppp/secret/print"), "=.proplist=name,service,profile,disabled,comment");
        assert_eq!(props("/interface/l2tp-server/server/print"), "=.proplist=enabled,use-ipsec");
        assert_eq!(props("/ppp/active/print"), "=.proplist=name,service,caller-id,address,uptime,encoding");
    }

    #[test]
    fn poe_is_optional_and_monitored_only_where_present() {
        let mut f = Fake::default();
        f.answers.insert("/interface/ethernet/poe/print".into(), Err("no such command prefix".into()));
        let s = snapshot(&mut f, false);
        assert!(s.get("poe").is_none() && s.get("poeMon").is_none());
        assert!(s["_errors"].as_array().unwrap().iter().any(|e| e["key"] == "poe"));
        assert!(s.get("resource").is_some(), "a board without PoE still gets the rest of the snapshot");
        assert!(!f.seen.iter().any(|w| w[0] == "/interface/ethernet/poe/monitor"));

        let mut f = Fake::default();
        f.answers.insert("/interface/ethernet/poe/print".into(), Ok(vec![row(&[("name", "ether2"), ("poe-out", "auto-on")]), row(&[("name", "ether3"), ("poe-out", "off")])]));
        f.answers.insert("/interface/ethernet/poe/monitor".into(), Ok(vec![row(&[("name", "ether2"), ("poe-out-status", "powered-on"), ("poe-out-power", "3.2")])]));
        let s = snapshot(&mut f, false);
        assert_eq!(s["poe"][1]["poe-out"], "off");
        assert_eq!(s["poeMon"][0]["poe-out-status"], "powered-on");
        let mon = f.seen.iter().find(|w| w[0] == "/interface/ethernet/poe/monitor").unwrap();
        assert!(mon.contains(&"=numbers=ether2,ether3".to_string()), "{mon:?}");
        // RADIUS secret and hotspot user passwords are never asked for
        for path in ["/radius/print", "/ip/hotspot/user/print"] {
            let w = f.seen.iter().find(|w| w[0] == path).unwrap();
            assert!(!w[1].contains("secret") && !w[1].contains("password"), "{w:?}");
        }
    }

    #[test]
    fn snapshot_has_the_ui_shape() {
        let mut f = Fake::default();
        f.answers.insert("/system/identity/print".into(), Ok(vec![row(&[("name", "Router")])]));
        f.answers.insert("/system/resource/print".into(), Ok(vec![row(&[("cpu-load", "3"), ("board-name", "hAP ac^2")])]));
        f.answers.insert("/ip/dhcp-client/print".into(), Ok(vec![row(&[("interface", "ether1"), ("address", "10.0.0.2/24")])]));
        f.answers.insert("/log/print".into(), Ok(vec![row(&[("message", "user tikontrol-read logged in from 10.0.0.9 via api"), ("topics", "system,info,account")]), row(&[("message", "login failure for user admin from 10.0.0.9 via api"), ("topics", "system,error,critical")]), row(&[("message", "x"), ("topics", "dhcp,info")])]));
        f.answers.insert("/interface/wireless/print".into(), Err("no such command prefix".into()));
        f.answers.insert("/ping".into(), Ok(vec![row(&[("avg-rtt", "9ms"), ("packet-loss", "0")])]));
        let s = snapshot(&mut f, true);
        assert_eq!(s["identity"], "Router");
        assert_eq!(s["resource"]["cpu-load"], 3);
        assert_eq!(s["wan"][0]["interface"], "ether1");
        assert_eq!(s["log"].as_array().unwrap().len(), 2);
        assert_eq!(s["log"][1]["topics"], json!(["dhcp", "info"]));
        assert_eq!(s["latency"]["Quad9"]["ms"], 9.0);
        assert!(s.get("wifi").is_none());
        assert_eq!(s["_errors"][0]["key"], "wifi");
        assert!(s["routerboard"].is_object());
    }
}
