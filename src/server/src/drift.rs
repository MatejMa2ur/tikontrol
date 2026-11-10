//! Blueprints / config drift. A blueprint is the router's configuration as tikontrol's reader sees it,
//! saved by an admin; drift is what changed since, menu by menu (rows added / removed).
//!
//! Why not `/export`: over the API with only the `read` policy, `/export` returns an empty `!done` on
//! RouterOS 7.25 (CHR), and an export with `sensitive` would put secrets in the file. So the blueprint
//! is built from the snapshot's structured prints, which already use vetted proplists (no keys or
//! passwords), minus counters and other fields that change on their own. One file per router.

use crate::app::{lock, now, App};
use crate::auth::Session;
use crate::http::{Req, Resp};
use crate::store::{write_private, Role};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Snapshot keys that are configuration (not state like leases, ARP or logs).
const MENUS: &[&str] = &[
    "addresses", "adlist", "bonds", "bridgeFilter", "bridgePorts", "bridgeVlans", "bridges", "bthUsers", "dhcpNetworks", "dhcpOptions", "dhcpServers", "discovery", "dnsCfg", "dnsStatic", "filter", "hotspot", "ipv6DhcpServers", "kidControl", "listMembers",
    "loggingRules", "nat", "ntp", "ovpn", "pools", "queues", "radius", "routes", "routingRules", "routingTables", "scheduler", "services", "snmp", "sshKeys", "trafficFlow", "upnp", "userGroups", "users",
    "vlans", "wgPeers", "wifi", "wifiNew", "wireguard",
];
/// Fields that change without anyone changing the configuration.
const VOLATILE: &[&str] = &["last-logged-in", "next-run", "running", "last-handshake", "rx", "tx", "name-count", "match-count", "status", "active", "last-seen", "bytes", "packets", "uptime"];

/// The configuration part of a snapshot: per menu, rows without volatile fields; dynamic rows dropped.
pub fn blueprint(snap: &Value) -> BTreeMap<String, Vec<Value>> {
    let clean = |r: &Value| -> Option<Value> {
        let o = r.as_object()?;
        if o.get("dynamic").is_some_and(|d| d == true || d == "true") {
            return None;
        }
        Some(Value::Object(o.iter().filter(|(k, _)| !VOLATILE.contains(&k.as_str()) && *k != "dynamic").map(|(k, v)| (k.clone(), v.clone())).collect::<Map<_, _>>()))
    };
    MENUS
        .iter()
        .filter_map(|m| {
            let rows = match snap.get(*m)? {
                Value::Array(a) => a.iter().filter_map(clean).collect(),
                v @ Value::Object(_) => clean(v).into_iter().collect(),
                _ => return None,
            };
            Some((m.to_string(), rows))
        })
        .collect()
}

/// Rows added and removed per menu (as multisets, so reordering alone is not drift).
pub fn diff(old: &BTreeMap<String, Vec<Value>>, new: &BTreeMap<String, Vec<Value>>) -> Vec<Value> {
    let mut out = vec![];
    for m in old.keys().chain(new.keys()).collect::<std::collections::BTreeSet<_>>() {
        let (a, b) = (old.get(m).cloned().unwrap_or_default(), new.get(m).cloned().unwrap_or_default());
        let take = |from: &[Value], minus: &[Value]| {
            let mut left: Vec<&Value> = minus.iter().collect();
            from.iter()
                .filter(|r| match left.iter().position(|x| x == r) {
                    Some(i) => {
                        left.swap_remove(i);
                        false
                    }
                    None => true,
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        let (added, removed) = (take(&b, &a), take(&a, &b));
        if !added.is_empty() || !removed.is_empty() {
            out.push(json!({ "menu": m, "added": added, "removed": removed }));
        }
    }
    out
}

fn path(app: &App) -> std::path::PathBuf {
    let dir = lock(&app.store).dir();
    if app.site.is_empty() { dir.join("blueprint.json") } else { dir.join(format!("blueprint-{}.json", app.site)) }
}

pub fn route(app: &Arc<App>, req: &Req, s: &Session) -> Option<Resp> {
    let current = || app.snap.read().unwrap_or_else(|e| e.into_inner()).data.clone();
    Some(match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/v1/drift") => {
            let saved: Option<Value> = std::fs::read(path(app)).ok().and_then(|b| serde_json::from_slice(&b).ok());
            let Some(saved) = saved else { return Some(Resp::json(200, &json!({ "saved": null, "changes": [] }))) };
            let Some(cur) = current() else { return Some(Resp::error(503, "no router data yet")) };
            let old: BTreeMap<String, Vec<Value>> = serde_json::from_value(saved["config"].clone()).unwrap_or_default();
            Resp::json(200, &json!({ "saved": saved["saved"], "changes": diff(&old, &blueprint(&cur)) }))
        }
        ("POST", "/api/v1/drift/baseline") if s.role != Role::Admin => Resp::error(403, "admin role required"),
        ("POST", "/api/v1/drift/baseline") => {
            let Some(cur) = current() else { return Some(Resp::error(503, "no router data yet")) };
            let b = json!({ "saved": now(), "config": blueprint(&cur) });
            crate::api::saved(write_private(&path(app), &serde_json::to_vec(&b).unwrap_or_default()))
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drift_ignores_counters_dynamic_rows_and_order() {
        let a = json!({
            "filter": [{"chain": "input", "action": "accept", "bytes": 10}, {"chain": "forward", "action": "drop"}],
            "routes": [{"dst-address": "0.0.0.0/0", "dynamic": true, "active": true}],
            "users": [{"name": "admin", "last-logged-in": "x"}],
            "leases": [{"address": "192.168.88.10"}],
            "snmp": {"enabled": false},
        });
        let b = json!({
            "filter": [{"chain": "forward", "action": "drop"}, {"chain": "input", "action": "accept", "bytes": 999}],
            "routes": [{"dst-address": "0.0.0.0/0", "dynamic": true, "active": false}],
            "users": [{"name": "admin", "last-logged-in": "y"}],
            "leases": [{"address": "192.168.88.11"}],
            "snmp": {"enabled": false},
        });
        assert!(diff(&blueprint(&a), &blueprint(&b)).is_empty());
        assert!(!blueprint(&a).contains_key("leases"), "state, not config");

        let c = json!({ "filter": [{"chain": "input", "action": "accept"}], "dnsStatic": [{"name": "nas.lan", "address": "192.168.88.4"}], "snmp": {"enabled": true} });
        let d = diff(&blueprint(&a), &blueprint(&c));
        let by = |m: &str| d.iter().find(|x| x["menu"] == m).cloned().unwrap();
        assert_eq!(by("filter")["removed"], json!([{"chain": "forward", "action": "drop"}]));
        assert_eq!(by("dnsStatic")["added"][0]["name"], "nas.lan");
        assert_eq!(by("snmp")["added"], json!([{"enabled": true}]));
        assert_eq!(by("users")["removed"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn baseline_needs_admin_and_data() {
        use crate::api::handle;
        use crate::api::tests::{admin_session, new_app, req, with_session};
        let app = new_app();
        let (c, csrf) = admin_session(&app);
        let call = |m: &str, p: &str| handle(&app, with_session(req(m, p, if m == "POST" { "{}" } else { "" }), &c, &csrf)).status;
        assert_eq!(call("GET", "/api/v1/drift"), 200);
        assert_eq!(call("POST", "/api/v1/drift/baseline"), 503);
        app.snap.write().unwrap().data = Some(Arc::new(json!({ "dnsStatic": [] })));
        assert_eq!(call("POST", "/api/v1/drift/baseline"), 200);
        app.snap.write().unwrap().data = Some(Arc::new(json!({ "dnsStatic": [{"name": "tv.lan"}] })));
        let r = handle(&app, with_session(req("GET", "/api/v1/drift", ""), &c, &csrf));
        let v: Value = match &r.body { crate::http::Body::Bytes(b) => serde_json::from_slice(b).unwrap(), _ => Value::Null };
        assert_eq!(v["changes"][0]["added"][0]["name"], "tv.lan");
    }
}
