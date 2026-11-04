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
    Some(match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/v1/live") => live(app),
        ("GET", "/api/v1/history") => {
            let range = req.param("range").unwrap_or_default();
            Resp::json(200, &lock(&app.live.history).series(&range))
        }
        ("GET", "/api/v1/health/internet") => Resp::json(200, &json!(lock(&app.live.history).health())),
        ("GET", "/api/v1/history/client") => {
            let mac = req.param("mac").unwrap_or_default().to_ascii_uppercase();
            let h = lock(&app.live.history);
            let hours = h.clients.get(&mac).map(|q| q.iter().map(|(t, d, u)| json!([t, d, u])).collect::<Vec<_>>()).unwrap_or_default();
            let seen = h.seen.get(&mac).map(|(f, l, i)| json!({ "first": f, "last": l, "interface": i }));
            let roams: Vec<&Ev> = h.events.iter().filter(|e| e.mac.as_deref() == Some(&mac)).collect();
            Resp::json(200, &json!({ "hours": hours, "seen": seen, "events": roams }))
        }
        ("GET", "/api/v1/clients/offline") => {
            let h = lock(&app.live.history);
            let cutoff = now().saturating_sub(120);
            let v: Vec<Value> = h.seen.iter().filter(|(_, x)| x.1 < cutoff).map(|(m, x)| json!({ "mac": m, "first": x.0, "last": x.1, "interface": x.2 })).collect();
            Resp::json(200, &json!(v))
        }
        ("GET", "/api/v1/events") => events(app, req),
        ("GET", "/api/v1/flows") => {
            let mut v = lock(crate::flows::agg()).top(20);
            // name destinations from the router's DNS cache when we have it
            let snap = app.snap.read().unwrap_or_else(|e| e.into_inner());
            let names: std::collections::HashMap<&str, &str> = snap.data.as_deref().and_then(|d| d["dnsCache"].as_array()).map(|a| a.iter().filter_map(|c| Some((c["data"].as_str()?, c["name"].as_str()?))).collect()).unwrap_or_default();
            if let Some(ds) = v["destinations"].as_array_mut() {
                for d in ds {
                    let n = d["ip"].as_str().and_then(|ip| names.get(ip)).map(|s| s.to_string());
                    d["name"] = json!(n);
                }
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
}
