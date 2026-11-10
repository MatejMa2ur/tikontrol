//! MikroTik CMR, the fleet controller in RouterOS 7.26 (`cmr` package): managed devices, pairing,
//! alerts (Device Supervisor), upgrade rules, Wi-Fi networks by label, and app traffic. The CMR server
//! is the router itself when it runs the package, or an optional second "controller" connection with
//! its own pinned certificate and read-only user (same rules as the router connection, api.rs).
//! Writes are `cmr-*` intents (intents/cmr.rs); features.rs sends them to the controller.

use crate::api::{body, saved, valid_cred, valid_host, valid_pin, CredIn};
use crate::app::{lock, App, As};
use crate::auth::Session;
use crate::http::{Req, Resp};
use crate::policy;
use crate::ros::cmd::Cmd;
use crate::ros::conn::Router;
use crate::ros::queries::rows_json;
use crate::store::{Cred, Mode, RouterCfg, Role};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

pub const LABEL: &str = "controller-password";

pub fn controller(app: &App) -> Option<RouterCfg> {
    lock(&app.store).state.controller.clone()
}

/// A connection to the CMR server: the controller if one is set, otherwise the router.
// ponytail: a fresh controller login per request; pool it like the router reader if the page polls often
pub fn open(app: &App, who: As) -> Result<Box<dyn Router + Send>, String> {
    match controller(app) {
        Some(c) => app.open_cfg(&c, LABEL, who),
        None => app.open(who),
    }
}

/// True for `cmr-*` intents (and batches of them). Mixing them with router changes in one batch is
/// refused: the two may be different routers, each with its own revert job.
pub fn is_cmr(v: &Value) -> Result<bool, String> {
    let k = |v: &Value| v.get("kind").and_then(Value::as_str).is_some_and(|k| k.starts_with("cmr-"));
    if v.get("kind").and_then(Value::as_str) != Some("batch") {
        return Ok(k(v));
    }
    let parts: Vec<bool> = v.get("intents").and_then(Value::as_array).map(|a| a.iter().map(k).collect()).unwrap_or_default();
    match (parts.iter().any(|x| *x), parts.iter().all(|x| *x)) {
        (true, false) => Err("CMR changes and router changes can't be applied in one batch".into()),
        (any, _) => Ok(any),
    }
}

/// CMR label selector (manual: "Labels"): plain labels OR, `+label` AND, `-label` AND NOT, `all`
/// matches every device. No plain label means every device. Band labels (`+5ghz`) pick radios, not
/// devices, so they are ignored here.
pub fn selects(sel: &[String], labels: &[String]) -> bool {
    let has = |l: &str| l == "all" || labels.iter().any(|x| x == l);
    let band = |l: &str| matches!(l, "2ghz" | "5ghz" | "6ghz");
    let plain: Vec<&str> = sel.iter().map(String::as_str).filter(|l| !l.starts_with(['+', '-'])).collect();
    (plain.is_empty() || plain.iter().any(|l| has(l)))
        && sel.iter().filter_map(|l| l.strip_prefix('+')).filter(|l| !band(l)).all(has)
        && !sel.iter().filter_map(|l| l.strip_prefix('-')).any(has)
}

/// "62.5KiB" → bytes (CLI sizes are binary units).
fn size(s: &str) -> Option<u64> {
    let (n, mul) = [("TiB", 1u64 << 40), ("GiB", 1 << 30), ("MiB", 1 << 20), ("KiB", 1 << 10), ("B", 1)].iter().find_map(|(u, m)| s.strip_suffix(u).map(|n| (n, *m))).unwrap_or((s, 1));
    let f: f64 = n.parse().ok()?;
    (f >= 0.0).then(|| (f * mul as f64).round() as u64)
}

/// `/cmr/device/apptraffic once` prints a table (the API version returns empty rows in 7.26beta5):
/// APPLICATION then down/up per period. The console is 80 columns wide, so periods that don't fit are
/// left out (week first, then day): the `Columns:` line says which ones are there. Missing periods are
/// null. Names may contain spaces, so the periods are taken from the right.
pub fn parse_apps(text: &str) -> Vec<Value> {
    const PERIODS: [(&str, &str); 4] = [("LAST-MINUTE", "minute"), ("LAST-HOUR", "hour"), ("LAST-DAY", "day"), ("LAST-WEEK", "week")];
    let Some(cols) = text.lines().find_map(|l| l.strip_prefix("Columns:")) else { return vec![] };
    let have: Vec<&str> = cols.split(',').filter_map(|c| PERIODS.iter().find(|p| p.0 == c.trim()).map(|p| p.1)).collect();
    let n = have.len();
    let mut v: Vec<Value> = text
        .lines()
        .filter_map(|l| {
            let t: Vec<&str> = l.split_whitespace().collect();
            if n == 0 || t.len() <= n || l.starts_with("Columns:") {
                return None;
            }
            let (name, periods) = t.split_at(t.len() - n);
            let p: Vec<[u64; 2]> = periods.iter().filter_map(|x| x.split_once('/').and_then(|(a, b)| Some([size(a)?, size(b)?]))).collect();
            if p.len() != n {
                return None;
            }
            let mut o = json!({ "name": name.join(" "), "minute": null, "hour": null, "day": null, "week": null });
            for (k, x) in have.iter().zip(p) {
                o[*k] = json!(x);
            }
            Some(o)
        })
        .collect();
    let total = |a: &Value| PERIODS.iter().rev().find_map(|p| a[p.1].as_array().map(|x| x.iter().filter_map(Value::as_u64).sum::<u64>())).unwrap_or(0);
    v.sort_by_key(|a| std::cmp::Reverse(total(a)));
    v
}

const DEVICE: &[&str] = &[".id", "identity", "ids", "board", "version", "address", "labels", "auto-labels", "pending", "remote-pending", "state", "connected", "controller", "stale", "upgrade-available", "upgrade-rule", "channel", "uptime", "serial", "comment"];
const ALERT: &[&str] = &[".id", "name", "labels", "severity", "category", "disabled", "devices", "devices-on", "fired", "comment", "cpu-above", "mem-above", "hdd-above", "disconnected-more-than", "upgrade-available", "connected", "rebooted", "upgrade-done", "unpaired-device-connected"];
const RULE: &[&str] = &[".id", "name", "labels", "channel", "schedule-time", "strategy", "fail-policy", "order", "disabled", "dynamic", "comment"];
const JOB: &[&str] = &[".id", "labels", "channel", "strategy", "schedule-time", "starts-in", "state", "success", "rule"];
// never security.passphrase: a reader without `sensitive` sees it masked, but don't even ask
const WIFI: &[&str] = &[".id", "ssid", "labels", "disabled", "mode", "vlan-id", "hide-ssid", "security.authentication-types", "comment"];
const LISTS: &[&str] = &["labels", "auto-labels", "category", "security.authentication-types"];

fn list(r: &mut dyn Router, path: &str, props: &[&str]) -> Result<Value, String> {
    Ok(rows_json(&r.run(&Cmd::new(path).proplist(props))?, LISTS))
}

fn strs(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}

/// Everything the Devices, Insights and Wi-Fi pages need from the CMR server, in one read.
pub fn read(r: &mut dyn Router) -> Result<Value, String> {
    let cmr = r.run(&Cmd::new("/cmr/print").proplist(&["enabled", "apptraffic-devices"])).map_err(|e| format!("no CMR server here ({e})"))?;
    let mut devices = list(r, "/cmr/device/print", DEVICE)?;
    let mut alerts = list(r, "/cmr/alert/print", ALERT)?;
    // Device Supervisor: which devices each active state alert is firing for
    for a in alerts.as_array_mut().into_iter().flatten() {
        if a["devices-on"].as_i64().unwrap_or(0) > 0 {
            let id = a[".id"].as_str().unwrap_or_default().to_string();
            let on = r.run(&Cmd::new("/cmr/alert/show-devices").arg("number", &id).proplist(&["identity"]))?;
            a["active"] = json!(on.iter().filter_map(|x| x.get("identity")).collect::<Vec<_>>());
        }
    }
    for d in devices.as_array_mut().into_iter().flatten() {
        let on = d["controller"] == json!(true) || d["connected"] == json!(true);
        d["online"] = json!(on);
        d["paired"] = json!(d["pending"] != json!(true) && d["remote-pending"] != json!(true));
    }
    // which paired devices each CMR Wi-Fi network selects (they broadcast it if they have a radio)
    let mut wifi = list(r, "/cmr/wifi/print", WIFI)?;
    for w in wifi.as_array_mut().into_iter().flatten() {
        let sel = strs(&w["labels"]);
        let aps: Vec<&Value> = devices.as_array().into_iter().flatten().filter(|d| d["paired"] == json!(true) && selects(&sel, &[strs(&d["labels"]), strs(&d["auto-labels"])].concat())).map(|d| &d["identity"]).collect();
        w["aps"] = json!(aps);
    }
    // app traffic: the CLI table, run read-only through /execute (the reader's policies still apply)
    let text = r.run(&Cmd::new("/execute").arg("script", "/cmr/device/apptraffic [find] once").arg("as-string", ""));
    let mut apps = text.as_ref().ok().and_then(|rows| rows.first()).and_then(|x| x.get("ret")).map(|t| parse_apps(t)).unwrap_or_default();
    // categories as the server's own classifier names them (only apps it has seen itself)
    let cats: HashMap<String, String> = r.run(&Cmd::new("/tool/apptraffic/stats/print").proplist(&["application", "category"])).unwrap_or_default().into_iter().filter_map(|x| Some((x.get("application")?.clone(), x.get("category")?.clone()))).collect();
    for a in &mut apps {
        a["category"] = json!(a["name"].as_str().and_then(|n| cats.get(n)));
    }
    Ok(json!({
        "enabled": cmr.first().and_then(|c| c.get("enabled")).is_some_and(|e| e == "true" || e == "yes"),
        "apptrafficDevices": cmr.first().and_then(|c| c.get("apptraffic-devices")),
        "devices": devices, "alerts": alerts, "wifi": wifi, "apps": apps,
        "appsError": text.err(),
        "upgrades": list(r, "/cmr/upgrade/print", RULE)?,
        "jobs": list(r, "/cmr/upgrade/job/print", JOB)?,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetController {
    host: String,
    #[serde(default = "api_ssl")]
    port: u16,
    pin: String,
    reader: CredIn,
    writer: Option<CredIn>,
    #[serde(default)]
    confirm_write: bool,
}
fn api_ssl() -> u16 {
    8729
}

pub fn route(app: &Arc<App>, req: &Req, s: &Session) -> Option<Resp> {
    let admin = s.role == Role::Admin;
    Some(match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/v1/cmr") => {
            let c = controller(app);
            let info = c.as_ref().map(|c| json!({ "host": c.host, "port": c.port, "reader": c.reader.user, "writer": c.writer.as_ref().map(|w| &w.user), "mode": c.mode }));
            let mut v = match if app.demo.is_some() { Err("demo mode".to_string()) } else { open(app, As::Reader).and_then(|mut r| read(r.as_mut())) } {
                Ok(v) => v,
                Err(e) => json!({ "available": false, "error": e }),
            };
            v["source"] = json!(if c.is_some() { "controller" } else { "router" });
            v["controller"] = info.unwrap_or(Value::Null);
            Resp::json(200, &v)
        }
        ("POST", "/api/v1/controller") if admin => set(app, req),
        ("DELETE", "/api/v1/controller") if admin => {
            let mut st = lock(&app.store);
            st.state.controller = None;
            saved(st.save())
        }
        ("POST" | "DELETE", "/api/v1/controller") => Resp::error(403, "admin role required"),
        _ => return None,
    })
}

/// Stores the controller connection after proving it: pinned TLS, a reader that can't write, a
/// writer (optional) that can, and a CMR server behind it.
fn set(app: &App, req: &Req) -> Resp {
    let b: SetController = match body(req) {
        Ok(b) => b,
        Err(r) => return r,
    };
    if !valid_host(&b.host) || b.port == 8728 {
        return Resp::error(400, "invalid controller address (API-SSL only)");
    }
    if !valid_pin(&b.pin) {
        return Resp::error(400, "pin must be the 64-character certificate fingerprint");
    }
    if !std::iter::once(&b.reader).chain(b.writer.as_ref()).all(valid_cred) || b.writer.as_ref().is_some_and(|w| w.user == b.reader.user) {
        return Resp::error(400, "invalid controller users (separate reader and writer)");
    }
    if b.writer.is_some() && !b.confirm_write {
        return Resp::error(409, "a writer needs confirm_write: it can change every managed device");
    }
    let tmp = RouterCfg { host: b.host.clone(), port: b.port, pin: b.pin.to_ascii_lowercase(), mode: if b.writer.is_some() { Mode::ReadWrite } else { Mode::ReadOnly }, reader: Cred { user: b.reader.user.clone(), password: String::new() }, writer: None };
    let check = |c: &CredIn| -> Result<policy::Policies, String> {
        let mut r = (app.connector)(&tmp, &c.user, &c.password)?;
        r.run(&Cmd::new("/cmr/print").proplist(&["enabled"])).map_err(|e| format!("no CMR server there ({e})"))?;
        policy::detect(r.as_mut(), &c.user)
    };
    match check(&b.reader) {
        Ok(p) if !p.can_read() => return Resp::error(400, "the reader user needs the read and api policies"),
        Ok(p) if !p.read_only_violations().is_empty() => return Resp::error(400, &format!("the reader user must not have: {}", p.read_only_violations().join(", "))),
        Ok(_) => {}
        Err(e) => return Resp::error(502, &format!("reader: {e}")),
    }
    if let Some(w) = &b.writer {
        match check(w) {
            // RouterOS 7.26 refuses /cmr/device/pair (and reboot/upgrade) without `reboot`
            Ok(p) if p.has("write") && p.has("reboot") => {}
            Ok(_) => return Resp::error(400, "the writer user needs the write and reboot policies (CMR pairing and upgrades need reboot)"),
            Err(e) => return Resp::error(502, &format!("writer: {e}")),
        }
    }
    let mut st = lock(&app.store);
    let reader = Cred { user: b.reader.user.clone(), password: st.sealer.seal(&format!("{LABEL}:reader"), &b.reader.password) };
    let writer = b.writer.as_ref().map(|w| Cred { user: w.user.clone(), password: st.sealer.seal(&format!("{LABEL}:writer"), &w.password) });
    st.state.controller = Some(RouterCfg { reader, writer, ..tmp });
    saved(st.save())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ros::queries::tests::{row, Fake};

    fn l(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn label_selectors_follow_the_manual() {
        let ap = l(&["office", "ap", "hAP ax2"]);
        let sw = l(&["office", "switch"]);
        assert!(selects(&l(&["office", "lab"]), &sw), "plain labels are OR");
        assert!(selects(&l(&["office", "+ap"]), &ap) && !selects(&l(&["office", "+ap"]), &sw), "+ is AND");
        assert!(!selects(&l(&["office", "-ap"]), &ap) && selects(&l(&["office", "-ap"]), &sw), "- is AND NOT");
        assert!(selects(&l(&["all"]), &sw) && selects(&[], &sw), "all, and no labels, select everything");
        assert!(!selects(&l(&["office+ap"]), &ap), "without a comma it is one label name");
        assert!(selects(&l(&["ap", "+5ghz"]), &ap), "band labels pick radios, not devices");
    }

    #[test]
    fn apptraffic_table_is_parsed_from_the_right() {
        // captured from 7.26beta5 `/execute script="/cmr/device/apptraffic [find] once" as-string`
        let t = "Columns: APPLICATION, LAST-MINUTE, LAST-HOUR, LAST-DAY, LAST-WEEK\r\nAPPLI  LAST-MINUTE      LAST-HOUR          LAST-DAY           LAST-WEEK        \r\nDNS    0/0              942/380            942/380            942/380          \r\nHTTPS  0/0              62.5KiB/3006       62.5KiB/3006       62.5KiB/3006     \r\nMicrosoft Teams  1.5MiB/2KiB  1.5MiB/2KiB  1.5MiB/2KiB  1.5MiB/2KiB\r\nbroken line here";
        let a = parse_apps(t);
        assert_eq!(a.len(), 3);
        assert_eq!(a[0]["name"], "Microsoft Teams");
        assert_eq!(a[0]["day"], json!([1572864, 2048]));
        assert_eq!(a[1]["name"], "HTTPS");
        assert_eq!(a[1]["hour"], json!([64000, 3006]));
        assert_eq!(a[2]["minute"], json!([0, 0]));
        assert!(parse_apps("").is_empty());
        // wider numbers: the console drops LAST-WEEK
        let t = "Columns: APPLICATION, LAST-MINUTE, LAST-HOUR, LAST-DAY\r\nAPPLICATION  LAST-MINUTE        LAST-HOUR            LAST-DAY           \r\nOther        202.8KiB/102.2KiB  3422.7KiB/1958.3KiB  3422.7KiB/1958.3KiB\r\nSSH          2812/4110          672.8KiB/796.5KiB    672.8KiB/796.5KiB  ";
        let a = parse_apps(t);
        assert_eq!((a.len(), a[1]["name"].as_str(), a[1]["minute"].clone(), a[1]["week"].clone()), (2, Some("SSH"), json!([2812, 4110]), Value::Null));
    }

    #[test]
    fn batches_cannot_mix_controller_and_router_changes() {
        assert!(is_cmr(&json!({"kind": "cmr-pair"})).unwrap());
        assert!(!is_cmr(&json!({"kind": "block-client"})).unwrap());
        assert!(is_cmr(&json!({"kind": "batch", "intents": [{"kind": "cmr-pair"}, {"kind": "cmr-device-labels"}]})).unwrap());
        assert!(is_cmr(&json!({"kind": "batch", "intents": [{"kind": "cmr-pair"}, {"kind": "dns-record"}]})).is_err());
    }

    #[test]
    fn read_marks_supervisor_alerts_and_broadcasting_aps() {
        let mut f = Fake::default();
        f.answers.insert("/cmr/print".into(), Ok(vec![row(&[("enabled", "true"), ("apptraffic-devices", "gateway")])]));
        f.answers.insert("/cmr/device/print".into(), Ok(vec![
            row(&[(".id", "*1"), ("identity", "gw"), ("controller", "true"), ("auto-labels", "arm64,gw")]),
            row(&[(".id", "*2"), ("identity", "ap-1"), ("connected", "true"), ("labels", "ap"), ("auto-labels", "ap-1,192.168.88.2")]),
            row(&[(".id", "*3"), ("identity", "ap-2"), ("connected", "true"), ("pending", "true"), ("labels", "ap")]),
        ]));
        f.answers.insert("/cmr/alert/print".into(), Ok(vec![row(&[(".id", "*9"), ("name", "cpu busy"), ("devices-on", "1")]), row(&[(".id", "*A"), ("name", "idle"), ("devices-on", "0")])]));
        f.answers.insert("/cmr/alert/show-devices".into(), Ok(vec![row(&[("identity", "ap-1")])]));
        f.answers.insert("/cmr/wifi/print".into(), Ok(vec![row(&[(".id", "*1"), ("ssid", "Example"), ("labels", "ap")])]));
        f.answers.insert("/execute".into(), Ok(vec![row(&[("ret", "Columns: APPLICATION, LAST-MINUTE\nHTTPS  1KiB/1")])]));
        f.answers.insert("/tool/apptraffic/stats/print".into(), Ok(vec![row(&[("application", "HTTPS"), ("category", "Web")])]));
        let v = read(&mut f).unwrap();
        assert_eq!(v["devices"][0]["online"], true);
        assert_eq!(v["devices"][2]["paired"], false);
        assert_eq!(v["alerts"][0]["active"], json!(["ap-1"]));
        assert!(v["alerts"][1].get("active").is_none());
        assert_eq!(v["wifi"][0]["aps"], json!(["ap-1"]), "pending devices don't broadcast");
        assert_eq!(v["apps"][0]["category"], "Web");
        let wifi = f.seen.iter().find(|w| w[0] == "/cmr/wifi/print").unwrap();
        assert!(!wifi.iter().any(|w| w.contains("passphrase")), "never asks for the Wi-Fi key");
        let mut none = Fake::default();
        none.answers.insert("/cmr/print".into(), Err("no such command prefix".into()));
        assert!(read(&mut none).unwrap_err().contains("no CMR server"));
    }
}
