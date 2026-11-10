//! Demo mode (TIK_DEMO): a read-only stand-in for the router, answered from the demo snapshot so
//! Review & apply can show the commands an intent would run. Only `print` is answered, using the
//! snapshot's query table (path -> key); every other command is refused, so nothing can be applied.

use super::cmd::Cmd;
use super::conn::{Router, Row};
use super::queries::keys_for;
use serde_json::Value;
use std::sync::Arc;

pub struct Demo {
    snap: Arc<Value>,
}

impl Demo {
    pub fn new(snap: Arc<Value>) -> Demo {
        Demo { snap }
    }
}

/// RouterOS API values are strings: undo what `queries::typed` did.
fn text(v: &Value) -> Option<String> {
    Some(match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(a) => a.iter().filter_map(text).collect::<Vec<_>>().join(","),
        Value::Null | Value::Object(_) => return None,
    })
}

fn row(v: &Value) -> Option<Row> {
    Some(v.as_object()?.iter().filter_map(|(k, v)| Some((k.clone(), text(v)?))).collect())
}

impl Router for Demo {
    fn run(&mut self, cmd: &Cmd) -> Result<Vec<Row>, String> {
        let words = cmd.words()?;
        let path = cmd.path();
        let Some(menu) = path.strip_suffix("/print") else {
            return Err(format!("demo mode: {path} is not a read; nothing is applied in demo mode"));
        };
        let mut props: Option<Vec<&str>> = None;
        let mut filters = vec![];
        for w in &words[1..] {
            if let Some(p) = w.strip_prefix("=.proplist=") {
                props = Some(p.split(',').collect());
            } else if let Some(q) = w.strip_prefix('?') {
                filters.push(q.split_once('=').unwrap_or((q, "")));
            }
            // other arguments (stats, .tag) don't change which rows exist
        }
        let keys = keys_for(path);
        let present = |filtered: bool| keys.iter().filter(move |(_, q)| q.is_empty() != filtered).filter_map(|(k, _)| self.snap.get(*k));
        let mut rows: Vec<Row> = vec![];
        // the whole menu: one list, or one settings object read in parts (/ip/dns: dns + dnsCfg)
        for v in present(false) {
            match v {
                Value::Array(a) if rows.is_empty() => rows.extend(a.iter().filter_map(row)),
                Value::Object(_) => match rows.first_mut() {
                    Some(r) => r.extend(row(v).unwrap_or_default()),
                    None => rows.extend(row(v)),
                },
                _ => {}
            }
        }
        // no unfiltered read: the union of the filtered ones (/file: backups + exports)
        if keys.iter().all(|(_, q)| !q.is_empty()) {
            for v in present(true) {
                rows.extend(v.as_array().into_iter().flatten().filter_map(row));
            }
        }
        if keys.iter().all(|(k, _)| self.snap.get(*k).is_none()) {
            return Err(format!("demo mode: the sample router has no {menu}"));
        }
        let mut out = vec![];
        for (n, mut r) in rows.into_iter().enumerate() {
            // the real router gives every list row an .id; older snapshots may not have kept it
            r.entry(".id".into()).or_insert_with(|| format!("*D{n:X}"));
            if !filters.iter().all(|(k, v)| r.get(*k).map_or("", String::as_str) == *v) {
                continue;
            }
            if let Some(p) = &props {
                r.retain(|k, _| p.contains(&k.as_str()));
            }
            out.push(r);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn demo() -> Demo {
        Demo::new(Arc::new(json!({
            "upnp": { "enabled": false },
            "dns": { "cache-size": 2048 },
            "dnsCfg": { "servers": ["192.0.2.53", "198.51.100.53"], "allow-remote-requests": true },
            "leases": [
                { ".id": "*1A", "address": "10.12.0.105", "mac-address": "02:00:00:00:00:01", "dynamic": true, "host-name": "speaker" },
                { "address": "10.12.0.106", "mac-address": "02:00:00:00:00:02", "dynamic": false, "comment": "tik:fixed:02:00:00:00:00:02" },
            ],
            "backups": [{ "name": "a.backup", "type": "backup" }],
            "exports": [{ "name": "tik-a.rsc", "type": "script" }],
            "interfaces": [{ "name": "ether1", "type": "ether" }],
            "etherStats": [{ "name": "stats-only", "rx-byte": 1 }],
        })))
    }

    #[test]
    fn paths_map_to_snapshot_keys() {
        let mut d = demo();
        let r = d.run(&Cmd::new("/ip/upnp/print").proplist(&["enabled"])).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].get("enabled").map(String::as_str), Some("false"));
        // one settings menu read into two keys: both parts answer
        let r = d.run(&Cmd::new("/ip/dns/print").proplist(&["servers", "cache-size"])).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].get("servers").map(String::as_str), Some("192.0.2.53,198.51.100.53"));
        assert_eq!(r[0].get("cache-size").map(String::as_str), Some("2048"));
        // the full menu, not the `stats` read of the same path
        let r = d.run(&Cmd::new("/interface/print").proplist(&["name"])).unwrap();
        assert_eq!(r.iter().map(|r| r["name"].as_str()).collect::<Vec<_>>(), ["ether1"]);
        // a menu read in pieces by type is answered with all of them
        let r = d.run(&Cmd::new("/file/print").proplist(&["name"])).unwrap();
        assert_eq!(r.len(), 2);
        // not in the table, or not in this snapshot: an error, like a router without the package
        assert!(d.run(&Cmd::new("/system/script/print").proplist(&["name"])).is_err());
        assert!(d.run(&Cmd::new("/interface/wifi/radio/print").proplist(&["name"])).is_err());
    }

    #[test]
    fn filters_and_proplists_apply() {
        let mut d = demo();
        let r = d.run(&Cmd::new("/ip/dhcp-server/lease/print").proplist(&["dynamic"]).query("mac-address", "02:00:00:00:00:01")).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].len(), 1, "only the asked fields: {:?}", r[0]);
        assert_eq!(r[0]["dynamic"], "true");
        let r = d.run(&Cmd::new("/ip/dhcp-server/lease/print").proplist(&[".id", "address"]).query("comment", "tik:fixed:02:00:00:00:00:02")).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0]["address"], "10.12.0.106");
        assert!(r[0][".id"].starts_with('*'), "a missing .id is made up");
        let r = d.run(&Cmd::new("/ip/dhcp-server/lease/print").proplist(&[".id"]).query("dynamic", "true")).unwrap();
        assert_eq!(r[0][".id"], "*1A", "a kept .id is used");
        assert!(d.run(&Cmd::new("/ip/dhcp-server/lease/print").proplist(&["address"]).query("mac-address", "02:00:00:00:00:09")).unwrap().is_empty());
    }

    #[test]
    fn anything_but_print_is_refused() {
        let mut d = demo();
        for c in [
            Cmd::new("/ip/upnp/set").arg("enabled", "yes"),
            Cmd::new("/ip/dns/static/add").arg("name", "a.lan").arg("address", "10.0.0.1"),
            Cmd::new("/ip/firewall/filter/remove").arg("numbers", "*1"),
            Cmd::new("/system/reboot"),
            Cmd::new("/system/scheduler/add").arg("name", "x"),
            Cmd::new("/ping").arg("address", "192.0.2.1"),
            Cmd::new("/interface/monitor-traffic").arg("interface", "ether1").arg("once", ""),
        ] {
            let e = d.run(&c).unwrap_err();
            assert!(e.starts_with("demo mode"), "{e}");
        }
        // an invalid command is refused before anything is looked up
        assert!(d.run(&Cmd::new("/ip/upnp/print").proplist(&[])).is_err());
    }
}
