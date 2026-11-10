//! Feature routes beyond the core API: live stream, history, events. Each returns None when the
//! path isn't its own, so `api::route` falls through to 404.

use crate::app::{lock, now, App};
use crate::auth::Session;
use crate::http::{Body, Req, Resp};
use crate::live::Ev;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

pub fn extra(app: &Arc<App>, req: &Req, s: &Session) -> Option<Resp> {
    if let Some(r) = crate::mcp::session_route(app, req, s) {
        return Some(r);
    }
    if let Some(r) = crate::alerts::route(app, req, s) { return Some(r) }
    if let Some(r) = crate::cmr::route(app, req, s) { return Some(r) }
    if let Some(r) = crate::tools::route(app, req, s) { return Some(r) }
    if let Some(r) = crate::speedtest::route(app, req, s) { return Some(r) }
    if let Some(r) = crate::discovery::route(app, req, s) { return Some(r) }
    if let Some(r) = crate::admins::route(app, req, s) { return Some(r) }
    if let Some(r) = crate::apikeys::route(app, req, s) { return Some(r) }
    if let Some(r) = crate::drift::route(app, req, s) { return Some(r) }
    Some(match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/v1/live") => live(app),
        ("GET", "/api/v1/history") => {
            let range = req.param("range").unwrap_or_default();
            Resp::json(200, &lock(&app.live.history).series(&range))
        }
        ("GET", "/api/v1/health/internet") => Resp::json(200, &json!(lock(&app.live.history).health())),
        ("GET", "/api/v1/health/isp") => {
            let pts: Vec<crate::live::Point> = lock(&app.live.history).minutes.iter().copied().collect();
            Resp::json(200, &crate::live::isp_status(&pts, now()))
        }
        ("GET", "/api/v1/history/client") => {
            let mac = req.param("mac").unwrap_or_default().to_ascii_uppercase();
            let h = lock(&app.live.history);
            let hours = h.clients.get(&mac).map(|q| q.iter().map(|(t, d, u)| json!([t, d, u])).collect::<Vec<_>>()).unwrap_or_default();
            let seen = h.seen.get(&mac).map(|(f, l, i)| json!({ "first": f, "last": l, "interface": i }));
            let roams: Vec<&Ev> = h.events.iter().filter(|e| e.mac.as_deref() == Some(&mac)).collect();
            Resp::json(200, &json!({ "hours": hours, "seen": seen, "events": roams }))
        }
        ("GET", "/api/v1/history/clients") => {
            let Some(hours) = crate::live::client_window(&req.param("window").unwrap_or_else(|| "24h".into())) else {
                return Some(Resp::error(400, "window must be 24h or 7d"));
            };
            let top = match req.param("top").map(|t| t.parse::<usize>()) {
                None => 8,
                Some(Ok(n)) if (1..=crate::live::MAX_TOP).contains(&n) => n,
                _ => return Some(Resp::error(400, &format!("top must be 1-{}", crate::live::MAX_TOP))),
            };
            Resp::json(200, &crate::live::top_clients(&lock(&app.live.history).clients, now(), hours, top))
        }
        ("GET", "/api/v1/clients/offline") => {
            let h = lock(&app.live.history);
            let cutoff = now().saturating_sub(120);
            let v: Vec<Value> = h.seen.iter().filter(|(_, x)| x.1 < cutoff).map(|(m, x)| json!({ "mac": m, "first": x.0, "last": x.1, "interface": x.2 })).collect();
            Resp::json(200, &json!(v))
        }
        ("GET", "/api/v1/events") => events(app, req),
        ("GET", "/api/v1/floorplan") => match std::fs::read(floorplan_path(app)) {
            Ok(b) => Resp::new(200, "application/json", b).with("cache-control", "no-store".into()),
            Err(_) => Resp::json(200, &Value::Null),
        },
        ("PUT" | "DELETE", "/api/v1/floorplan") if s.role != crate::store::Role::Admin => Resp::error(403, "admin role required"),
        ("DELETE", "/api/v1/floorplan") => match std::fs::remove_file(floorplan_path(app)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Resp::error(500, "could not delete the floor plan"),
            _ => Resp::json(200, &json!({ "ok": true })),
        },
        ("PUT", "/api/v1/floorplan") => {
            let b: Floorplan = match crate::api::body(req) {
                Ok(b) => b,
                Err(r) => return Some(r),
            };
            if let Err(e) = b.check() {
                return Some(Resp::error(400, e));
            }
            crate::api::saved(crate::store::write_private(&floorplan_path(app), &serde_json::to_vec(&b).unwrap_or_default()))
        }
        ("GET", "/api/v1/retention") => Resp::json(200, &json!({ "days": crate::live::retention_days(lock(&app.store).state.history_days), "max": crate::live::MAX_DAYS })),
        ("PUT", "/api/v1/retention") if s.role != crate::store::Role::Admin => Resp::error(403, "admin role required"),
        ("PUT", "/api/v1/retention") => {
            #[derive(serde::Deserialize)]
            #[serde(deny_unknown_fields)]
            struct B {
                days: u64,
            }
            let b: B = match crate::api::body(req) {
                Ok(b) => b,
                Err(r) => return Some(r),
            };
            if b.days == 0 || b.days > crate::live::MAX_DAYS {
                return Some(Resp::error(400, &format!("days must be 1-{}", crate::live::MAX_DAYS)));
            }
            let mut st = lock(&app.store);
            st.state.history_days = b.days;
            let r = st.save();
            drop(st);
            lock(&app.live.history).prune(now().saturating_sub(b.days * 86400));
            crate::api::saved(r)
        }
        ("GET", "/api/v1/flows") => {
            let (mut v, countries) = {
                let a = lock(crate::flows::agg());
                (a.top(20), crate::flows::geo().get().map(|g| g.countries(&a)))
            };
            v["geoip"] = json!(countries.is_some());
            v["countries"] = countries.unwrap_or(json!([]));
            // name destinations from the router's DNS cache when we have it
            let snap = app.snap.read().unwrap_or_else(|e| e.into_inner());
            let names: std::collections::HashMap<&str, &str> = snap.data.as_deref().and_then(|d| d["dnsCache"].as_array()).map(|a| a.iter().filter_map(|c| Some((c["data"].as_str()?, c["name"].as_str()?))).collect()).unwrap_or_default();
            let name = |d: &mut Value| d["name"] = json!(d["ip"].as_str().and_then(|ip| names.get(ip)));
            v["destinations"].as_array_mut().into_iter().flatten().for_each(name);
            for t in v["talkers"].as_array_mut().into_iter().flatten() {
                t["destinations"].as_array_mut().into_iter().flatten().for_each(name);
            }
            Resp::json(200, &v)
        }
        _ => return crate::features::route(app, req, s),
    })
}

fn live(app: &Arc<App>) -> Resp {
    let app = app.clone();
    let stream = move |w: &mut dyn std::io::Write| {
        *lock(&app.live.subscribers) += 1;
        let started = now();
        let mut seen = 0;
        let _ = w.write_all(b"retry: 3000\n\n");
        // one hour per stream; EventSource reconnects by itself
        while now() - started < 3600 {
            let out = match app.live.next_frame(seen, Duration::from_secs(15)) {
                Some((n, f)) => {
                    seen = n;
                    format!("data: {f}\n\n")
                }
                None => ": ping\n\n".into(),
            };
            if w.write_all(out.as_bytes()).and_then(|_| w.flush()).is_err() {
                break;
            }
        }
        let mut n = lock(&app.live.subscribers);
        *n = n.saturating_sub(1);
    };
    Resp {
        status: 200,
        headers: vec![("content-type", "text/event-stream".into()), ("cache-control", "no-store".into())],
        body: Body::Stream(Box::new(stream)),
    }
}

/// Coverage map (Radios page): one floor plan image, where the APs sit on it (percent of width/height), and
/// what the browser's RF prediction (src/ui/src/lib/coverage.ts) needs: scale, walls, per-band AP settings,
/// measured points and the model. The server only stores and validates it; nothing here reaches the router.
/// Kept in its own file next to state.json so the state stays small. Plans saved before the RF fields load as is.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Floorplan {
    /// data:image/{png,jpeg,webp};base64,... (the browser scales it down first)
    image: String,
    aps: Vec<Pin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scale: Option<Scale>,
    #[serde(default)]
    walls: Vec<Wall>,
    #[serde(default)]
    measurements: Vec<Measurement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model: Option<Model>,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    label: String,
    x: f64,
    y: f64,
    /// radios of this AP; empty = placed only (plans saved before the RF model)
    #[serde(default)]
    bands: Vec<BandCfg>,
    /// antenna gain, dBi
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gain: Option<f64>,
}

#[derive(serde::Deserialize, serde::Serialize, PartialEq, Clone, Copy)]
enum Band {
    #[serde(rename = "2.4")]
    B24,
    #[serde(rename = "5")]
    B5,
    #[serde(rename = "6")]
    B6,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct BandCfg {
    band: Band,
    /// transmit power, dBm
    tx: f64,
    /// name of the router radio it was filled from (display only; never sent to the router)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    radio: Option<String>,
}

/// Reference line in percent and its real length; mpu = metres per plan unit (1 % of the image width).
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Scale {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    meters: f64,
    mpu: f64,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum Material {
    Drywall,
    Glass,
    Wood,
    Brick,
    Concrete,
    Metal,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Wall {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    m: Material,
}

/// A signal a client reported at a spot, to compare with the prediction.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Measurement {
    x: f64,
    y: f64,
    band: Band,
    dbm: f64,
    #[serde(default)]
    label: String,
}

/// Path-loss exponent n and grid cell size in metres.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Model {
    n: f64,
    cell: f64,
}

pub const MAX_PLAN_IMAGE: usize = 200_000;
pub const MAX_PINS: usize = 32;
pub const MAX_WALLS: usize = 500;
pub const MAX_MEASUREMENTS: usize = 200;

fn floorplan_path(app: &App) -> std::path::PathBuf {
    lock(&app.store).dir().join("floorplan.json")
}

/// A short label of printable characters: no control or invisible format characters (bidi overrides etc.).
fn label_ok(s: &str, min: usize, max: usize) -> bool {
    let n = s.chars().count();
    (min..=max).contains(&n) && !s.chars().any(|c| c.is_control() || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}'))
}

/// Finite and inside the range.
fn within(v: f64, lo: f64, hi: f64) -> bool {
    v.is_finite() && (lo..=hi).contains(&v)
}

fn pct(v: f64) -> bool {
    within(v, 0.0, 100.0)
}

impl Floorplan {
    fn check(&self) -> Result<(), &'static str> {
        // no SVG: it can carry script; the bytes must really be the image type they claim
        let magic: [(&str, &[u8]); 3] = [("png", b"\x89PNG"), ("jpeg", b"\xff\xd8\xff"), ("webp", b"RIFF")];
        let (kind, data) = self.image.strip_prefix("data:image/").and_then(|r| r.split_once(";base64,")).ok_or("image must be a PNG, JPEG or WebP data URL")?;
        let want = magic.iter().find(|(k, _)| *k == kind).ok_or("image must be PNG, JPEG or WebP")?.1;
        if self.image.len() > MAX_PLAN_IMAGE {
            return Err("image too large: scale it down (about 150 KB)");
        }
        if data.len() < 16 || !data.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b)) || !b64_head(&data[..16]).starts_with(want) {
            return Err("image data is not valid");
        }
        if self.aps.len() > MAX_PINS {
            return Err("at most 32 access points");
        }
        let ok = |p: &Pin| pct(p.x) && pct(p.y) && label_ok(&p.label, 1, 32);
        if !self.aps.iter().all(ok) {
            return Err("each access point needs a label (1-32 characters) and a position of 0-100 %");
        }
        for p in &self.aps {
            if p.bands.len() > 3 || p.bands.iter().enumerate().any(|(i, b)| p.bands[..i].iter().any(|o| o.band == b.band)) {
                return Err("each access point has at most one entry per band (2.4, 5, 6 GHz)");
            }
            if !p.bands.iter().all(|b| within(b.tx, 0.0, 36.0) && b.radio.as_deref().is_none_or(|r| label_ok(r, 1, 32))) {
                return Err("transmit power must be 0-36 dBm and a radio name 1-32 characters");
            }
            if p.gain.is_some_and(|g| !within(g, -5.0, 20.0)) {
                return Err("antenna gain must be -5 to 20 dBi");
            }
        }
        if let Some(s) = &self.scale {
            let len = (s.x2 - s.x1).hypot(s.y2 - s.y1);
            if ![s.x1, s.y1, s.x2, s.y2].into_iter().all(pct) || len < 0.5 || !within(s.meters, 0.1, 2000.0) || !within(s.mpu, 0.001, 50.0) {
                return Err("scale: the reference line must lie inside the plan, measure 0.1-2000 m, and give 0.001-50 m per unit");
            }
        }
        if self.walls.len() > MAX_WALLS {
            return Err("at most 500 walls");
        }
        if !self.walls.iter().all(|w| [w.x1, w.y1, w.x2, w.y2].into_iter().all(pct) && (w.x1, w.y1) != (w.x2, w.y2)) {
            return Err("each wall needs two different ends inside the plan (0-100 %)");
        }
        if self.measurements.len() > MAX_MEASUREMENTS {
            return Err("at most 200 measurements");
        }
        if !self.measurements.iter().all(|m| pct(m.x) && pct(m.y) && within(m.dbm, -120.0, 0.0) && label_ok(&m.label, 0, 32)) {
            return Err("each measurement needs a position of 0-100 %, a signal of -120 to 0 dBm and a label of up to 32 characters");
        }
        if let Some(m) = &self.model {
            if !within(m.n, 1.5, 4.0) || !within(m.cell, 0.25, 2.0) {
                return Err("model: path-loss exponent 1.5-4.0 and cell size 0.25-2 m");
            }
        }
        Ok(())
    }
}

/// Decodes 16 base64 characters (12 bytes): enough to check an image's magic number.
fn b64_head(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => 0,
    };
    s.as_bytes().chunks(4).flat_map(|q| {
        let n = q.iter().fold(0u32, |a, &c| a << 6 | val(c) as u32);
        [(n >> 16) as u8, (n >> 8) as u8, n as u8]
    }).collect()
}

/// Spreadsheet apps run cells starting with = + - @ as formulas: neutralise them on export.
pub fn csv_cell(s: &str) -> String {
    let s = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{s}") } else { s.to_string() };
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn events(app: &Arc<App>, req: &Req) -> Resp {
    let q = req.param("q").unwrap_or_default().to_lowercase();
    let cat = req.param("cat").unwrap_or_default();
    let h = lock(&app.live.history);
    let list: Vec<&Ev> = h
        .events
        .iter()
        .rev()
        .filter(|e| (cat.is_empty() || e.cat == cat) && (q.is_empty() || e.text.to_lowercase().contains(&q)))
        .take(500)
        .collect();
    if req.param("format").as_deref() == Some("csv") {
        let mut out = String::from("time,category,level,text\n");
        for e in &list {
            out.push_str(&format!("{},{},{},{}\n", e.t, csv_cell(&e.cat), csv_cell(&e.level), csv_cell(&e.text)));
        }
        return Resp::new(200, "text/csv; charset=utf-8", out.into_bytes()).with("content-disposition", "attachment; filename=\"tikontrol-events.csv\"".into()).with("cache-control", "no-store".into());
    }
    Resp::json(200, &json!(list))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_neutralises_formulas_and_quotes() {
        assert_eq!(csv_cell("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(csv_cell("plain"), "\"plain\"");
        assert_eq!(csv_cell("-1"), "\"'-1\"");
    }

    #[test]
    fn floorplan_is_validated_saved_and_admin_only() {
        use crate::api::handle;
        use crate::api::tests::{admin_session, req, with_session};
        use crate::app::tests::app_with_router;
        use crate::store::Mode;
        let text = |r: &Resp| match &r.body {
            Body::Bytes(b) => String::from_utf8_lossy(b).to_string(),
            _ => String::new(),
        };
        // a PNG header, base64
        let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let fp = |image: &str, aps: Value| serde_json::from_value::<Floorplan>(json!({ "image": image, "aps": aps })).unwrap().check();
        assert!(fp(png, json!([{ "label": "Living room", "x": 10.5, "y": 99 }])).is_ok());
        assert!(fp(&png.replace("png", "svg+xml"), json!([])).is_err());
        assert!(fp(&png.replace("image/png", "image/jpeg"), json!([])).is_err(), "magic bytes must match");
        assert!(fp(&format!("{png}<script>"), json!([])).is_err());
        assert!(fp(&format!("{png}{}", "A".repeat(MAX_PLAN_IMAGE)), json!([])).is_err());
        assert!(fp(png, json!([{ "label": "x", "x": 101, "y": 0 }])).is_err());
        assert!(fp(png, json!([{ "label": "", "x": 1, "y": 0 }])).is_err());
        assert!(fp(png, json!((0..33).map(|_| json!({ "label": "a", "x": 1, "y": 1 })).collect::<Vec<_>>())).is_err());

        let app = Arc::new(app_with_router("read,api,test", Mode::ReadOnly));
        let (c, csrf) = admin_session(&app);
        let call = |m: &str, b: &str| handle(&app, with_session(req(m, "/api/v1/floorplan", b), &c, &csrf));
        assert_eq!(text(&call("GET", "")), "null");
        let body = json!({ "image": png, "aps": [{ "label": "AP", "x": 50, "y": 50 }] }).to_string();
        assert_eq!(call("PUT", &body).status, 200);
        assert!(text(&call("GET", "")).contains("\"label\":\"AP\""));
        assert_eq!(call("PUT", r#"{"image":"data:image/svg+xml;base64,PHN2Zz4=","aps":[]}"#).status, 400);
        assert_eq!(call("DELETE", "").status, 200);
        assert_eq!(text(&call("GET", "")), "null");
        assert_eq!(handle(&app, req("GET", "/api/v1/floorplan", "")).status, 401);
    }

    #[test]
    fn floorplan_rf_fields_are_validated_and_old_plans_still_load() {
        let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let good = json!({
            "image": png,
            "aps": [{ "label": "Hall", "x": 20, "y": 30, "gain": 2, "bands": [{ "band": "2.4", "tx": 20, "radio": "wlan1" }, { "band": "5", "tx": 23 }] }],
            "scale": { "x1": 10, "y1": 50, "x2": 60, "y2": 50, "meters": 10, "mpu": 0.2 },
            "walls": [{ "x1": 0, "y1": 0, "x2": 100, "y2": 0, "m": "brick" }, { "x1": 50, "y1": 0, "x2": 50, "y2": 100, "m": "drywall" }],
            "measurements": [{ "x": 40, "y": 40, "band": "5", "dbm": -61, "label": "Phone" }],
            "model": { "n": 2.5, "cell": 0.5 }
        });
        // parse, check and round-trip (what PUT stores is what GET returns)
        let check = |v: &Value| serde_json::from_value::<Floorplan>(v.clone()).map_err(|e| e.to_string()).and_then(|f| f.check().map_err(String::from));
        assert_eq!(check(&good), Ok(()));
        let stored = serde_json::to_value(serde_json::from_value::<Floorplan>(good.clone()).unwrap()).unwrap();
        assert_eq!((stored["scale"]["mpu"].as_f64(), stored["model"]["n"].as_f64(), stored["walls"][0]["m"].as_str()), (Some(0.2), Some(2.5), Some("brick")));
        assert_eq!((stored["aps"][0]["bands"][0]["radio"].as_str(), stored["aps"][0]["bands"][1].get("radio"), stored["measurements"][0]["label"].as_str()), (Some("wlan1"), None, Some("Phone")));
        // a plan saved before the RF model loads and saves back unchanged
        let old = json!({ "image": png, "aps": [{ "label": "AP", "x": 50, "y": 50 }] });
        assert_eq!(check(&old), Ok(()));
        assert_eq!(serde_json::to_value(serde_json::from_value::<Floorplan>(old.clone()).unwrap()).unwrap(), json!({ "image": png, "aps": [{ "label": "AP", "x": 50.0, "y": 50.0, "bands": [] }], "walls": [], "measurements": [] }));

        let bad = |path: &[&str], v: Value| {
            let mut b = good.clone();
            let mut at = &mut b;
            for k in path {
                at = match k.parse::<usize>() { Ok(i) => &mut at[i], Err(_) => &mut at[*k] };
            }
            *at = v;
            assert!(check(&b).is_err(), "{path:?} should be rejected");
        };
        bad(&["extra"], json!(1));
        bad(&["walls", "0", "m"], json!("adamantium"));
        bad(&["walls", "0", "x2"], json!(100.1));
        bad(&["walls", "0", "thickness"], json!(3));
        bad(&["walls", "1"], json!({ "x1": 5, "y1": 5, "x2": 5, "y2": 5, "m": "wood" }));
        bad(&["walls"], json!((0..501).map(|i| json!({ "x1": 0, "y1": 0, "x2": 1 + i % 99, "y2": 1, "m": "glass" })).collect::<Vec<_>>()));
        bad(&["scale", "meters"], json!(0));
        bad(&["scale", "mpu"], json!(-1));
        bad(&["scale", "x2"], json!(10)); // zero-length reference line
        bad(&["scale", "unit"], json!("ft"));
        bad(&["model", "n"], json!(9));
        bad(&["model", "cell"], json!(0.01));
        bad(&["aps", "0", "gain"], json!(40));
        bad(&["aps", "0", "bands", "0", "tx"], json!(50));
        bad(&["aps", "0", "bands", "0", "band"], json!("60"));
        bad(&["aps", "0", "bands", "1", "band"], json!("2.4")); // same band twice
        bad(&["aps", "0", "bands", "0", "radio"], json!("wlan1\u{202e}"));
        bad(&["aps", "0", "bands", "0", "radio"], json!(""));
        bad(&["measurements", "0", "dbm"], json!(5));
        bad(&["measurements", "0", "dbm"], json!(-150));
        bad(&["measurements", "0", "label"], json!("x".repeat(33)));
        bad(&["measurements", "0", "label"], json!("a\nb"));
        bad(&["measurements", "0", "mac"], json!("02:00:00:00:00:01"));
        bad(&["measurements"], json!((0..201).map(|_| json!({ "x": 1, "y": 1, "band": "5", "dbm": -60 })).collect::<Vec<_>>()));
        // the largest valid plan still fits in one request body
        let mut max = good.clone();
        max["image"] = json!(format!("{png}{}", "A".repeat(MAX_PLAN_IMAGE - png.len())));
        max["walls"] = json!((0..MAX_WALLS).map(|i| json!({ "x1": 12.3, "y1": 45.6, "x2": 78.9, "y2": (i % 100) as f64 + 0.5, "m": "concrete" })).collect::<Vec<_>>());
        max["measurements"] = json!((0..MAX_MEASUREMENTS).map(|_| json!({ "x": 12.3, "y": 45.6, "band": "2.4", "dbm": -61, "label": "x".repeat(32) })).collect::<Vec<_>>());
        let bands: Vec<Value> = ["2.4", "5", "6"].iter().map(|b| json!({ "band": b, "tx": 23.5, "radio": "x".repeat(32) })).collect();
        let ap = json!({ "label": "x".repeat(32), "x": 12.3, "y": 45.6, "gain": 2.5, "bands": bands });
        max["aps"] = json!((0..MAX_PINS).map(|_| ap.clone()).collect::<Vec<_>>());
        assert_eq!(check(&max), Ok(()));
        assert!(max.to_string().len() < crate::http::MAX_BODY, "{} bytes", max.to_string().len());
    }
}
