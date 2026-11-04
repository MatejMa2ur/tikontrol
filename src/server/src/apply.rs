//! Safe apply (docs/PLAN.md §3). Every write is a typed intent, validated here at the trust boundary,
//! compiled to RouterOS operations plus their exact inverse, and applied like this:
//!
//! 1. preview: the browser shows the commands and warnings;
//! 2. arm: a one-shot `/system/scheduler` job on the router runs the inverse in 5 minutes;
//! 3. apply the operations over the writer connection (any failure undoes at once);
//! 4. keep: the browser must confirm within the window, which removes the job. No confirmation,
//!    no keep: the router reverts by itself, even if tikontrol is down or cut off.
//!
//! Objects we create carry `comment=tik:<kind>:<key>` so the inverse can find exactly them.

use crate::ros::cmd::{cli_quote, valid_value, Cmd};
use crate::ros::conn::Router;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

pub const WINDOW_SECS: u64 = 300;

/// One RouterOS operation. `find` selects existing rows (resolved to `.id`s over the API,
/// rendered as `[find k="v"]` in CLI and in the revert script).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Op {
    pub path: &'static str,
    pub verb: &'static str,
    pub find: Option<(&'static str, String)>,
    pub args: Vec<(&'static str, String)>,
}

impl Op {
    pub fn add(path: &'static str, args: Vec<(&'static str, String)>) -> Op {
        Op { path, verb: "add", find: None, args }
    }
    pub fn on(path: &'static str, verb: &'static str, k: &'static str, v: &str, args: Vec<(&'static str, String)>) -> Op {
        Op { path, verb, find: Some((k, v.to_string())), args }
    }

    pub fn to_cli(&self) -> String {
        let mut s = format!("{}/{}", self.path, self.verb);
        if let Some((k, v)) = &self.find {
            s.push_str(&format!(" [find {k}={}]", quoted(v)));
        }
        for (k, v) in &self.args {
            // secrets go to the router only: never into the preview, error messages or the revert job
            let v = if SECRET_KEYS.contains(k) { "\"********\"".to_string() } else { cli_quote(v) };
            s.push_str(&format!(" {k}={v}"));
        }
        s
    }

    fn valid(&self) -> bool {
        self.args.iter().all(|(_, v)| valid_value(v)) && self.find.as_ref().is_none_or(|(_, v)| valid_value(v))
    }

    /// Runs the operation over the API. Returns how many rows it touched.
    pub fn run(&self, r: &mut dyn Router) -> Result<usize, String> {
        let full = format!("{}/{}", self.path, self.verb);
        let with_args = |mut c: Cmd| {
            for (k, v) in &self.args {
                c = c.arg(k, v);
            }
            c
        };
        match &self.find {
            None => r.run(&with_args(Cmd::new(&full))).map(|_| 1),
            Some((k, v)) => {
                let rows = r.run(&Cmd::new(&format!("{}/print", self.path)).proplist(&[".id"]).query(k, v))?;
                let ids: Vec<&String> = rows.iter().filter_map(|row| row.get(".id")).collect();
                if ids.is_empty() {
                    return Ok(0);
                }
                let list = ids.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",");
                r.run(&with_args(Cmd::new(&full).arg("numbers", &list))).map(|_| ids.len())
            }
        }
    }
}

/// Arguments whose values are masked in every CLI rendering of an op.
pub const SECRET_KEYS: &[&str] = &["password", "secret", "private-key", "preshared-key"];

/// Always-quoted string for `[find k="v"]`, escaping like `cli_quote`.
fn quoted(v: &str) -> String {
    let q = cli_quote(v);
    if q.starts_with('"') { q } else { format!("\"{q}\"") }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub title: String,
    pub risk: Risk,
    pub warnings: Vec<String>,
    pub ops: Vec<Op>,
    pub undo: Vec<Op>,
    /// false for actions with nothing to restore (kicking a Wi-Fi client)
    pub revertible: bool,
}

impl Plan {
    pub fn cli(&self) -> Vec<String> {
        self.ops.iter().map(Op::to_cli).collect()
    }
    pub fn undo_cli(&self) -> Vec<String> {
        self.undo.iter().map(Op::to_cli).collect()
    }
}

/// What the browser (or an approved MCP proposal) asks for. Unknown fields are rejected.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Intent {
    BlockClient { mac: String },
    UnblockClient { mac: String },
    PauseClient { mac: String },
    ResumeClient { mac: String },
    LimitClient { mac: String, ip: String, down: String, up: String },
    UnlimitClient { mac: String },
    FixedIp { mac: String },
    KickWifi { mac: String },
    DnsRecord { name: String, address: String },
    RemoveDnsRecord { name: String },
    PortForward { protocol: String, port: u16, to_ip: String, to_port: u16, #[serde(default)] name: String },
    RemovePortForward { protocol: String, port: u16 },
    StaticRoute { dst: String, gateway: String, #[serde(default = "one")] distance: u8 },
    RemoveStaticRoute { dst: String, gateway: String },
    SetUpnp { enabled: bool },
    SetAdlist { url: String, disabled: bool },
    DisableServices { names: Vec<String> },
    WireguardPeer { interface: String, name: String, public_key: String, allowed_address: String },
}

fn one() -> u8 {
    1
}

pub fn norm_mac(m: &str) -> Result<String, String> {
    let m = m.trim().to_ascii_uppercase().replace('-', ":");
    let ok = m.len() == 17 && m.split(':').all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_hexdigit()));
    ok.then_some(m).ok_or_else(|| "invalid MAC address".into())
}

pub fn ipv4(s: &str) -> Result<Ipv4Addr, String> {
    s.trim().parse::<Ipv4Addr>().map_err(|_| format!("invalid IPv4 address {s:?}"))
}

pub fn cidr(s: &str) -> Result<String, String> {
    let (a, n) = s.trim().split_once('/').unwrap_or((s.trim(), "32"));
    let n: u8 = n.parse().map_err(|_| "invalid prefix length")?;
    if n > 32 {
        return Err("invalid prefix length".into());
    }
    Ok(format!("{}/{n}", ipv4(a)?))
}

/// "10M", "500k", "1G", or a plain number of bits per second.
pub fn rate(s: &str) -> Result<String, String> {
    let s = s.trim();
    let digits = s.trim_end_matches(['k', 'M', 'G']);
    let ok = !digits.is_empty() && digits.len() <= 6 && digits.bytes().all(|b| b.is_ascii_digit()) && s.len() - digits.len() <= 1 && digits != "0";
    ok.then(|| s.to_string()).ok_or_else(|| format!("invalid rate {s:?} (e.g. 10M)"))
}

pub fn dns_name(s: &str) -> Result<String, String> {
    let s = s.trim().trim_end_matches('.').to_ascii_lowercase();
    let ok = !s.is_empty() && s.len() <= 253 && s.split('.').all(|l| !l.is_empty() && l.len() <= 63 && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') && !l.starts_with('-'));
    ok.then_some(s).ok_or_else(|| "invalid DNS name".into())
}

pub fn ros_name(s: &str) -> Result<String, String> {
    let s = s.trim();
    let ok = !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_. ".contains(&b));
    ok.then(|| s.to_string()).ok_or_else(|| "invalid name (letters, digits, - _ . and spaces)".into())
}

pub fn wg_key(s: &str) -> Result<String, String> {
    let s = s.trim();
    let ok = s.len() == 44 && s.ends_with('=') && s[..43].bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/');
    ok.then(|| s.to_string()).ok_or_else(|| "invalid WireGuard public key".into())
}

pub fn yn(b: bool) -> String {
    if b { "yes" } else { "no" }.into()
}

const FILTER: &str = "/ip/firewall/filter";

/// Drop rule for a MAC, placed before the first forward rule so FastTrack and "accept established"
/// can't let its traffic through. Over the API `place-before` takes an `.id`, not a position.
fn drop_mac(current: &mut dyn Router, mac: &str, comment: &str) -> Result<Op, String> {
    let first = current.run(&Cmd::new("/ip/firewall/filter/print").proplist(&[".id"]).query("chain", "forward"))?;
    let mut args = vec![("chain", "forward".into()), ("src-mac-address", mac.to_string()), ("action", "drop".into()), ("comment", comment.to_string())];
    if let Some(id) = first.first().and_then(|r| r.get(".id")) {
        args.push(("place-before", id.clone()));
    }
    Ok(Op::add(FILTER, args))
}
const NAT: &str = "/ip/firewall/nat";

/// Builds the plan. `current` reads present values for intents whose inverse needs them.
pub fn compile(i: &Intent, current: &mut dyn Router) -> Result<Plan, String> {
    use Intent::*;
    let p = |title: String, risk, ops: Vec<Op>, undo: Vec<Op>| Plan { title, risk, warnings: vec![], ops, undo, revertible: true };
    let plan = match i {
        BlockClient { mac } | PauseClient { mac } => {
            let mac = norm_mac(mac)?;
            let (kind, verb) = if matches!(i, BlockClient { .. }) { ("block", "Block") } else { ("pause", "Pause internet for") };
            let c = format!("tik:{kind}:{mac}");
            let ops = vec![drop_mac(current, &mac, &c)?];
            p(format!("{verb} {mac}"), Risk::Low, ops, vec![Op::on(FILTER, "remove", "comment", &c, vec![])])
        }
        UnblockClient { mac } | ResumeClient { mac } => {
            let mac = norm_mac(mac)?;
            let kind = if matches!(i, UnblockClient { .. }) { "block" } else { "pause" };
            let c = format!("tik:{kind}:{mac}");
            let mut undo = vec![drop_mac(current, &mac, &c)?];
            // in the revert script an `.id` from now may be gone; append instead
            undo[0].args.retain(|(k, _)| *k != "place-before");
            p(format!("{} {mac}", if kind == "block" { "Unblock" } else { "Resume internet for" }), Risk::Low, vec![Op::on(FILTER, "remove", "comment", &c, vec![])], undo)
        }
        LimitClient { mac, ip, down, up } => {
            let (mac, ip, down, up) = (norm_mac(mac)?, ipv4(ip)?, rate(down)?, rate(up)?);
            let c = format!("tik:limit:{mac}");
            let name = format!("tik-limit-{}", mac.replace(':', ""));
            // simple queue max-limit is upload/download from the client's point of view
            let ops = vec![
                Op::on("/queue/simple", "remove", "comment", &c, vec![]),
                Op::add("/queue/simple", vec![("name", name), ("target", format!("{ip}/32")), ("max-limit", format!("{up}/{down}")), ("comment", c.clone())]),
            ];
            let mut pl = p(format!("Limit {mac} to {down} down / {up} up"), Risk::Low, ops, vec![Op::on("/queue/simple", "remove", "comment", &c, vec![])]);
            pl.warnings.push("Queues don't see FastTracked traffic: the limit may not apply to established connections while FastTrack is on.".into());
            pl
        }
        UnlimitClient { mac } => {
            let mac = norm_mac(mac)?;
            let c = format!("tik:limit:{mac}");
            let rows = current.run(&Cmd::new("/queue/simple/print").proplist(&["name", "target", "max-limit"]).query("comment", &c))?;
            let undo = rows.iter().map(|r| Op::add("/queue/simple", vec![("name", r.get("name").cloned().unwrap_or_default()), ("target", r.get("target").cloned().unwrap_or_default()), ("max-limit", r.get("max-limit").cloned().unwrap_or_default()), ("comment", c.clone())])).collect();
            p(format!("Remove the speed limit for {mac}"), Risk::Low, vec![Op::on("/queue/simple", "remove", "comment", &c, vec![])], undo)
        }
        FixedIp { mac } => {
            let mac = norm_mac(mac)?;
            let lease = "/ip/dhcp-server/lease";
            let c = format!("tik:fixed:{mac}");
            let ops = vec![Op::on(lease, "make-static", "mac-address", &mac, vec![]), Op::on(lease, "set", "mac-address", &mac, vec![("comment", c.clone())])];
            // removing the static lease lets the client get a dynamic one again
            p(format!("Give {mac} a fixed IP"), Risk::Low, ops, vec![Op::on(lease, "remove", "comment", &c, vec![])])
        }
        KickWifi { mac } => {
            let mac = norm_mac(mac)?;
            let ops = vec![Op::on("/interface/wireless/registration-table", "remove", "mac-address", &mac, vec![])];
            Plan { title: format!("Reconnect {mac}"), risk: Risk::Low, warnings: vec![], ops, undo: vec![], revertible: false }
        }
        DnsRecord { name, address } => {
            let (name, address) = (dns_name(name)?, ipv4(address)?);
            let c = format!("tik:dns:{name}");
            let ops = vec![Op::add("/ip/dns/static", vec![("name", name.clone()), ("address", address.to_string()), ("comment", c.clone())])];
            p(format!("Add DNS record {name} → {address}"), Risk::Low, ops, vec![Op::on("/ip/dns/static", "remove", "comment", &c, vec![])])
        }
        RemoveDnsRecord { name } => {
            let name = dns_name(name)?;
            let c = format!("tik:dns:{name}");
            let rows = current.run(&Cmd::new("/ip/dns/static/print").proplist(&["name", "address"]).query("comment", &c))?;
            if rows.is_empty() {
                return Err("only records created by tikontrol can be removed here".into());
            }
            let undo = rows.iter().map(|r| Op::add("/ip/dns/static", vec![("name", r.get("name").cloned().unwrap_or_default()), ("address", r.get("address").cloned().unwrap_or_default()), ("comment", c.clone())])).collect();
            p(format!("Remove DNS record {name}"), Risk::Low, vec![Op::on("/ip/dns/static", "remove", "comment", &c, vec![])], undo)
        }
        PortForward { protocol, port, to_ip, to_port, name } => {
            let proto = match protocol.as_str() {
                "tcp" | "udp" => protocol.clone(),
                _ => return Err("protocol must be tcp or udp".into()),
            };
            if *port == 0 || *to_port == 0 {
                return Err("ports must be 1-65535".into());
            }
            let to_ip = ipv4(to_ip)?;
            if !to_ip.is_private() {
                return Err("port forwards must point at a private LAN address".into());
            }
            let label = if name.is_empty() { String::new() } else { ros_name(name)? };
            let c = format!("tik:pf:{proto}:{port}");
            let ops = vec![Op::add(NAT, vec![("chain", "dstnat".into()), ("action", "dst-nat".into()), ("protocol", proto.clone()), ("dst-port", port.to_string()), ("in-interface-list", "WAN".into()), ("to-addresses", to_ip.to_string()), ("to-ports", to_port.to_string()), ("comment", c.clone())])];
            let mut pl = p(format!("Forward {proto} {port} to {to_ip}:{to_port}{}", if label.is_empty() { String::new() } else { format!(" ({label})") }), Risk::Medium, ops, vec![Op::on(NAT, "remove", "comment", &c, vec![])]);
            pl.warnings.push(format!("This exposes {to_ip}:{to_port} to the whole internet. Only forward to devices that are kept up to date."));
            pl.warnings.push("Uses the WAN interface list: your WAN interface must be in it (it is in the default config).".into());
            pl
        }
        RemovePortForward { protocol, port } => {
            let c = format!("tik:pf:{protocol}:{port}");
            if !matches!(protocol.as_str(), "tcp" | "udp") {
                return Err("protocol must be tcp or udp".into());
            }
            let keys = ["chain", "action", "protocol", "dst-port", "in-interface-list", "to-addresses", "to-ports"];
            let rows = current.run(&Cmd::new("/ip/firewall/nat/print").proplist(&keys).query("comment", &c))?;
            if rows.is_empty() {
                return Err("only port forwards created by tikontrol can be removed here".into());
            }
            let undo = rows.iter().map(|r| Op::add(NAT, keys.iter().map(|k| (*k, r.get(*k).cloned().unwrap_or_default())).filter(|(_, v)| !v.is_empty()).chain([("comment", c.clone())]).collect())).collect();
            p(format!("Remove port forward {protocol} {port}"), Risk::Low, vec![Op::on(NAT, "remove", "comment", &c, vec![])], undo)
        }
        StaticRoute { dst, gateway, distance } => {
            let (dst, gw) = (cidr(dst)?, ipv4(gateway)?);
            if dst == "0.0.0.0/0" {
                return Err("changing the default route is not supported here: it can cut off the internet and tikontrol".into());
            }
            let c = format!("tik:route:{dst}:{gw}");
            let ops = vec![Op::add("/ip/route", vec![("dst-address", dst.clone()), ("gateway", gw.to_string()), ("distance", distance.to_string()), ("comment", c.clone())])];
            let mut pl = p(format!("Route {dst} via {gw}"), Risk::Medium, ops, vec![Op::on("/ip/route", "remove", "comment", &c, vec![])]);
            pl.warnings.push("A wrong route can make parts of the network unreachable until it reverts.".into());
            pl
        }
        RemoveStaticRoute { dst, gateway } => {
            let (dst, gw) = (cidr(dst)?, ipv4(gateway)?);
            let c = format!("tik:route:{dst}:{gw}");
            let rows = current.run(&Cmd::new("/ip/route/print").proplist(&["distance"]).query("comment", &c))?;
            let d = rows.first().ok_or("only routes created by tikontrol can be removed here")?.get("distance").cloned().unwrap_or_else(|| "1".into());
            let undo = vec![Op::add("/ip/route", vec![("dst-address", dst.clone()), ("gateway", gw.to_string()), ("distance", d), ("comment", c.clone())])];
            p(format!("Remove route {dst} via {gw}"), Risk::Medium, vec![Op::on("/ip/route", "remove", "comment", &c, vec![])], undo)
        }
        SetUpnp { enabled } => {
            let before = current.run(&Cmd::new("/ip/upnp/print").proplist(&["enabled"]))?;
            let was = before.first().and_then(|r| r.get("enabled")).is_some_and(|v| v == "true" || v == "yes");
            let mut pl = p(format!("{} UPnP", if *enabled { "Enable" } else { "Disable" }), Risk::Low, vec![Op { path: "/ip/upnp", verb: "set", find: None, args: vec![("enabled", yn(*enabled))] }], vec![Op { path: "/ip/upnp", verb: "set", find: None, args: vec![("enabled", yn(was))] }]);
            if *enabled {
                pl.warnings.push("UPnP lets any device on the LAN open ports to the internet. It also needs UPnP interfaces set (external/internal) to work.".into());
            }
            pl
        }
        SetAdlist { url, disabled } => {
            if url.len() > 512 || !url.starts_with("https://") || !valid_value(url) {
                return Err("adlist URL must be https:// and at most 512 characters".into());
            }
            let before = current.run(&Cmd::new("/ip/dns/adlist/print").proplist(&["disabled"]).query("url", url))?;
            let Some(was) = before.first().and_then(|r| r.get("disabled")) else { return Err("no such adlist".into()) };
            let was = was == "true" || was == "yes";
            let path = "/ip/dns/adlist";
            p(format!("{} ad-block list", if *disabled { "Disable" } else { "Enable" }), Risk::Low, vec![Op::on(path, "set", "url", url, vec![("disabled", yn(*disabled))])], vec![Op::on(path, "set", "url", url, vec![("disabled", yn(was))])])
        }
        DisableServices { names } => {
            const ALLOWED: &[&str] = &["telnet", "ftp", "www", "api"];
            if names.is_empty() || names.iter().any(|n| !ALLOWED.contains(&n.as_str())) {
                return Err("only telnet, ftp, www and api can be disabled here".into());
            }
            let mut ops = vec![];
            let mut undo = vec![];
            for n in names {
                let rows = current.run(&Cmd::new("/ip/service/print").proplist(&["disabled"]).query("name", n))?;
                let was = rows.first().and_then(|r| r.get("disabled")).is_some_and(|v| v == "true" || v == "yes");
                ops.push(Op::on("/ip/service", "set", "name", n, vec![("disabled", "yes".into())]));
                undo.push(Op::on("/ip/service", "set", "name", n, vec![("disabled", yn(was))]));
            }
            let mut pl = p(format!("Disable {}", names.join(", ")), Risk::High, ops, undo);
            pl.warnings.push("High risk: changes management access (/ip/service). Make sure you can still reach the router with WinBox, SSH or WebFig over HTTPS before keeping it.".into());
            if names.iter().any(|n| n == "www") {
                pl.warnings.push("Disabling www turns off plain-HTTP WebFig; use www-ssl instead.".into());
            }
            pl
        }
        WireguardPeer { interface, name, public_key, allowed_address } => {
            let (ifn, name, key, allowed) = (ros_name(interface)?, ros_name(name)?, wg_key(public_key)?, cidr(allowed_address)?);
            let c = format!("tik:wg:{key}");
            let ops = vec![Op::add("/interface/wireguard/peers", vec![("interface", ifn.clone()), ("name", name.clone()), ("public-key", key), ("allowed-address", allowed.clone()), ("comment", c.clone())])];
            p(format!("Add WireGuard peer {name} ({allowed}) on {ifn}"), Risk::Medium, ops, vec![Op::on("/interface/wireguard/peers", "remove", "comment", &c, vec![])])
        }
    };
    if !plan.ops.iter().chain(&plan.undo).all(Op::valid) {
        return Err("a value contains characters RouterOS commands can't carry".into());
    }
    Ok(plan)
}

/// Intents from feature areas live in `crate::intents::*`. Each area parses the JSON intent (its own
/// `kind`s) and returns None when the kind isn't its own. Add an area with one line here.
pub type Area = fn(&serde_json::Value, &mut dyn Router) -> Option<Result<Plan, String>>;
pub const AREAS: &[Area] = &[
    crate::intents::system::compile,
    crate::intents::networks::compile,
    crate::intents::security::compile,
    crate::intents::internet::compile,
    crate::intents::profiles::compile,
    crate::intents::hotspot::compile,
];

/// Entry point for the API: core intents first, then the areas. Unknown kinds are rejected.
pub fn compile_value(v: &serde_json::Value, r: &mut dyn Router) -> Result<Plan, String> {
    let kind = v.get("kind").and_then(|k| k.as_str()).ok_or("intent needs a kind")?;
    for area in AREAS {
        if let Some(p) = area(v, r) {
            let p = p?;
            if !p.ops.iter().chain(&p.undo).all(Op::valid) {
                return Err("a value contains characters RouterOS commands can't carry".into());
            }
            if p.undo.iter().any(|o| o.args.iter().any(|(k, _)| SECRET_KEYS.contains(k))) {
                return Err("a revert can't carry a secret".into());
            }
            return Ok(p);
        }
    }
    let i: Intent = serde_json::from_value(v.clone()).map_err(|e| format!("invalid {kind} intent: {e}"))?;
    compile(&i, r)
}

/// Shorthand for areas.
pub fn plan(title: String, risk: Risk, ops: Vec<Op>, undo: Vec<Op>) -> Plan {
    Plan { title, risk, warnings: vec![], ops, undo, revertible: true }
}

/// RouterOS clock → start-date/start-time `secs` later, in the same date format the router used.
/// Handles both `2026-01-15` (7.10+) and `jan/15/2026`.
pub fn later(date: &str, time: &str, secs: u64) -> Option<(String, String)> {
    const MON: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let (y, m, d, iso) = if let Some((y, rest)) = date.split_once('-') {
        let (m, d) = rest.split_once('-')?;
        (y.parse::<i64>().ok()?, m.parse::<i64>().ok()?, d.parse::<i64>().ok()?, true)
    } else {
        let mut it = date.split('/');
        let mon = it.next()?.to_ascii_lowercase();
        let m = MON.iter().position(|x| *x == mon)? as i64 + 1;
        let d: i64 = it.next()?.parse().ok()?;
        (it.next()?.parse().ok()?, m, d, false)
    };
    let mut t = time.split(':').map(|x| x.parse::<i64>().ok());
    let (hh, mm, ss) = (t.next()??, t.next()??, t.next()??);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 59 {
        return None;
    }
    // days from civil (Howard Hinnant), so month and year rollovers are exact
    let days = |y: i64, m: i64, d: i64| {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
        era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy
    };
    let total = days(y, m, d) * 86400 + hh * 3600 + mm * 60 + ss + secs as i64;
    let (z, rem) = (total.div_euclid(86400), total.rem_euclid(86400));
    // `days` counts from 0000-03-01, which is what the inverse below expects
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d2 = doy - (153 * mp + 2) / 5 + 1;
    let m2 = if mp < 10 { mp + 3 } else { mp - 9 };
    let y2 = yoe + era * 400 + if m2 <= 2 { 1 } else { 0 };
    let date = if iso { format!("{y2:04}-{m2:02}-{d2:02}") } else { format!("{}/{d2:02}/{y2:04}", MON[(m2 - 1) as usize]) };
    Some((date, format!("{:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)))
}

/// The revert job: removes itself first, then runs the inverse, then logs.
pub fn revert_script(id: &str, plan: &Plan) -> Result<String, String> {
    let mut parts = vec![format!("/system/scheduler/remove [find name=\"tik-revert-{id}\"]")];
    // each inverse in its own :do so one failure doesn't stop the rest
    parts.extend(plan.undo_cli().into_iter().map(|c| format!(":do {{ {c} }} on-error={{}}")));
    parts.push(format!(":log warning \"tikontrol: change {id} was not confirmed and has been reverted\""));
    let s = parts.join("; ");
    if !valid_value(&s) {
        return Err("revert script too long: split the change into smaller ones".into());
    }
    Ok(s)
}

pub fn arm_cmd(id: &str, plan: &Plan, clock: (&str, &str)) -> Result<Cmd, String> {
    let (date, time) = later(clock.0, clock.1, WINDOW_SECS).ok_or("cannot read the router clock")?;
    Ok(Cmd::new("/system/scheduler/add")
        .arg("name", &format!("tik-revert-{id}"))
        .arg("start-date", &date)
        .arg("start-time", &time)
        .arg("interval", "0s")
        .arg("policy", "read,write,test")
        .arg("comment", "tikontrol Safe apply: reverts this change unless it is confirmed")
        .arg("on-event", &revert_script(id, plan)?))
}

pub fn disarm(r: &mut dyn Router, id: &str) -> Result<usize, String> {
    Op::on("/system/scheduler", "remove", "name", &format!("tik-revert-{id}"), vec![]).run(r)
}

pub fn undo_now(r: &mut dyn Router, plan: &Plan) -> Vec<String> {
    plan.undo.iter().filter_map(|op| op.run(r).err().map(|e| format!("{}: {e}", op.to_cli()))).collect()
}

/// Arms the revert, then applies. On any failure it undoes what ran and removes the job.
pub fn start(r: &mut dyn Router, id: &str, plan: &Plan) -> Result<(), String> {
    if plan.revertible {
        let clock = r.run(&Cmd::new("/system/clock/print").proplist(&["date", "time"]))?;
        let c = clock.first().ok_or("cannot read the router clock")?;
        let arm = arm_cmd(id, plan, (c.get("date").map_or("", |s| s), c.get("time").map_or("", |s| s)))?;
        r.run(&arm).map_err(|e| format!("could not arm the automatic revert, nothing was changed: {e} (the scheduler is unavailable in home device-mode)"))?;
    }
    for op in &plan.ops {
        if let Err(e) = op.run(r) {
            let left = undo_now(r, plan);
            let _ = disarm(r, id);
            return Err(format!("{} failed: {e}. Undone{}", op.to_cli(), if left.is_empty() { String::new() } else { format!(" except: {}", left.join("; ")) }));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ros::queries::tests::{row, Fake};

    fn plan(i: Intent) -> Plan {
        compile(&i, &mut Fake::default()).unwrap()
    }

    #[test]
    fn block_compiles_with_its_inverse() {
        let p = plan(Intent::BlockClient { mac: "aa-bb-cc-dd-ee-ff".into() });
        assert_eq!(p.cli(), [r#"/ip/firewall/filter/add chain=forward src-mac-address=AA:BB:CC:DD:EE:FF action=drop comment=tik:block:AA:BB:CC:DD:EE:FF"#]);
        let mut f = Fake::default();
        f.answers.insert("/ip/firewall/filter/print".into(), Ok(vec![row(&[(".id", "*2A")])]));
        let p = compile(&Intent::BlockClient { mac: "AA:BB:CC:DD:EE:FF".into() }, &mut f).unwrap();
        assert!(p.cli()[0].ends_with("place-before=*2A"));
        assert_eq!(p.undo_cli(), [r#"/ip/firewall/filter/remove [find comment="tik:block:AA:BB:CC:DD:EE:FF"]"#]);
    }

    #[test]
    fn input_is_validated_at_the_boundary() {
        let bad = [
            Intent::BlockClient { mac: "nope".into() },
            Intent::DnsRecord { name: "a b".into(), address: "10.0.0.1".into() },
            Intent::DnsRecord { name: "ok.lan".into(), address: "10.0.0.1; /system/reboot".into() },
            Intent::PortForward { protocol: "icmp".into(), port: 80, to_ip: "10.0.0.2".into(), to_port: 80, name: String::new() },
            Intent::PortForward { protocol: "tcp".into(), port: 80, to_ip: "8.8.8.8".into(), to_port: 80, name: String::new() },
            Intent::StaticRoute { dst: "0.0.0.0/0".into(), gateway: "10.0.0.1".into(), distance: 1 },
            Intent::LimitClient { mac: "AA:BB:CC:DD:EE:FF".into(), ip: "10.0.0.5".into(), down: "10M; x".into(), up: "1M".into() },
            Intent::DisableServices { names: vec!["ssh".into()] },
            Intent::WireguardPeer { interface: "wg0".into(), name: "x".into(), public_key: "short".into(), allowed_address: "10.1.0.2/32".into() },
            Intent::SetAdlist { url: "http://x".into(), disabled: true },
        ];
        for b in bad {
            assert!(compile(&b, &mut Fake::default()).is_err(), "{b:?}");
        }
    }

    #[test]
    fn untrusted_values_are_quoted_in_cli_and_scripts() {
        let mut f = Fake::default();
        f.answers.insert("/ip/dns/static/print".into(), Ok(vec![row(&[("name", "x.lan"), ("address", "10.0.0.9\"; /system/reboot; \"")])]));
        let p = compile(&Intent::RemoveDnsRecord { name: "x.lan".into() }, &mut f).unwrap();
        let s = revert_script("ab12", &p).unwrap();
        assert!(s.contains(r#"address="10.0.0.9\"; /system/reboot; \"""#), "{s}");
    }

    #[test]
    fn port_forward_warns() {
        let p = plan(Intent::PortForward { protocol: "tcp".into(), port: 443, to_ip: "192.168.88.10".into(), to_port: 8443, name: "NAS".into() });
        assert_eq!(p.risk, Risk::Medium);
        assert!(p.warnings[0].contains("whole internet"));
        assert!(p.cli()[0].contains("in-interface-list=WAN"));
    }

    #[test]
    fn disabling_services_is_high_risk_and_restores_previous_state() {
        let mut f = Fake::default();
        f.answers.insert("/ip/service/print".into(), Ok(vec![row(&[("disabled", "false")])]));
        let p = compile(&Intent::DisableServices { names: vec!["telnet".into()] }, &mut f).unwrap();
        assert_eq!(p.risk, Risk::High);
        assert_eq!(p.undo_cli(), [r#"/ip/service/set [find name="telnet"] disabled=no"#]);
    }

    #[test]
    fn later_rolls_over_days_months_and_years() {
        assert_eq!(later("2026-01-15", "12:00:00", 300), Some(("2026-01-15".into(), "12:05:00".into())));
        assert_eq!(later("2026-12-31", "23:58:00", 300), Some(("2027-01-01".into(), "00:03:00".into())));
        assert_eq!(later("2028-02-28", "23:59:00", 300), Some(("2028-02-29".into(), "00:04:00".into())));
        assert_eq!(later("jan/31/2026", "23:59:59", 300), Some(("feb/01/2026".into(), "00:04:59".into())));
        assert_eq!(later("garbage", "12:00:00", 1), None);
        assert_eq!(later("2026-13-01", "12:00:00", 1), None);
    }

    #[test]
    fn start_arms_before_applying_and_undoes_on_failure() {
        let mut f = Fake::default();
        f.answers.insert("/system/clock/print".into(), Ok(vec![row(&[("date", "2026-01-15"), ("time", "10:00:00")])]));
        f.answers.insert("/ip/dns/static/add".into(), Err("failure: entry already exists".into()));
        f.answers.insert("/ip/dns/static/print".into(), Ok(vec![row(&[(".id", "*9")])]));
        f.answers.insert("/system/scheduler/print".into(), Ok(vec![row(&[(".id", "*A")])]));
        let p = plan(Intent::DnsRecord { name: "nas.lan".into(), address: "10.0.0.4".into() });
        let e = start(&mut f, "id1", &p).unwrap_err();
        assert!(e.contains("Undone"));
        let paths: Vec<&str> = f.seen.iter().map(|w| w[0].as_str()).collect();
        let arm = paths.iter().position(|p| *p == "/system/scheduler/add").unwrap();
        let add = paths.iter().position(|p| *p == "/ip/dns/static/add").unwrap();
        assert!(arm < add, "revert is armed first: {paths:?}");
        assert!(paths.contains(&"/ip/dns/static/remove"));
        assert!(paths.contains(&"/system/scheduler/remove"));
        let sched = &f.seen[arm];
        assert!(sched.contains(&"=start-time=10:05:00".to_string()));
        assert!(sched.iter().any(|w| w.starts_with("=on-event=/system/scheduler/remove [find name=\"tik-revert-id1\"]")));
    }

    #[test]
    fn no_scheduler_means_no_change() {
        let mut f = Fake::default();
        f.answers.insert("/system/clock/print".into(), Ok(vec![row(&[("date", "2026-01-15"), ("time", "10:00:00")])]));
        f.answers.insert("/system/scheduler/add".into(), Err("not allowed by device-mode".into()));
        let p = plan(Intent::BlockClient { mac: "AA:BB:CC:DD:EE:FF".into() });
        assert!(start(&mut f, "x", &p).unwrap_err().contains("nothing was changed"));
        assert!(!f.seen.iter().any(|w| w[0] == "/ip/firewall/filter/add"));
    }

    #[test]
    fn secrets_are_masked_in_cli() {
        let op = Op::add("/radius", vec![("address", "10.0.0.1".into()), ("secret", "topsecret".into())]);
        assert_eq!(op.to_cli(), r#"/radius/add address=10.0.0.1 secret="********""#);
    }

    #[test]
    fn areas_are_reached_through_compile_value() {
        let v = serde_json::json!({"kind": "block-domain", "name": "example.com"});
        assert!(compile_value(&v, &mut Fake::default()).unwrap().title.contains("example.com"));
        assert!(compile_value(&serde_json::json!({"kind": "nope"}), &mut Fake::default()).is_err());
    }

    #[test]
    fn intents_reject_unknown_fields() {
        assert!(serde_json::from_str::<Intent>(r#"{"kind":"block-client","mac":"AA:BB:CC:DD:EE:FF","extra":1}"#).is_err());
        assert!(serde_json::from_str::<Intent>(r#"{"kind":"run-script","source":"x"}"#).is_err());
        assert!(serde_json::from_str::<Intent>(r#"{"kind":"block-client","mac":"AA:BB:CC:DD:EE:FF"}"#).is_ok());
    }
}
