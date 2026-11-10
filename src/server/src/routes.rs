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

/// Coverage map (Radios page): one floor plan image and where the APs sit on it, in percent.
/// No RF model. Kept in its own file next to state.json so the state stays small.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Floorplan {
    /// data:image/{png,jpeg,webp};base64,... (the browser scales it down first)
    image: String,
    aps: Vec<Pin>,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    label: String,
    x: f64,
    y: f64,
}

pub const MAX_PLAN_IMAGE: usize = 200_000;
pub const MAX_PINS: usize = 32;

fn floorplan_path(app: &App) -> std::path::PathBuf {
    lock(&app.store).dir().join("floorplan.json")
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
        let ok = |p: &Pin| (0.0..=100.0).contains(&p.x) && (0.0..=100.0).contains(&p.y) && !p.label.is_empty() && p.label.chars().count() <= 32 && !p.label.chars().any(char::is_control);
        if !self.aps.iter().all(ok) {
            return Err("each access point needs a label (1-32 characters) and a position of 0-100 %");
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
        let fp = |image: &str, aps: Value| Floorplan { image: image.into(), aps: serde_json::from_value(aps).unwrap() }.check();
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
}
