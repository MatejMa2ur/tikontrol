//! Diagnostics that change nothing: ping, traceroute and a DNS cache lookup. They run as the reader
//! (its `test` policy allows ping and traceroute), so they work in read-only mode too.
//!
//! Only typed commands with a validated host are run: there is no way to pass arbitrary RouterOS
//! commands. Each session may start one tool every two seconds. Packet capture (`/tool/sniffer`) is
//! not offered: it needs the `sniff` policy, which the reader must not have.

use crate::api::body;
use crate::app::{lock, App, As};
use crate::auth::Session;
use crate::http::{Req, Resp};
use crate::intents::system::host;
use crate::ros::cmd::Cmd;
use crate::ros::conn::Row;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const MIN_GAP: Duration = Duration::from_secs(2);
pub const MAX_COUNT: u8 = 10;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ping {
    host: String,
    #[serde(default = "four")]
    count: u8,
}
fn four() -> u8 {
    4
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Host {
    host: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Name {
    name: String,
}

fn last_run() -> &'static Mutex<HashMap<String, Instant>> {
    static M: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    M.get_or_init(Mutex::default)
}

/// Ok, or the seconds to wait. Keyed by the session's CSRF token, which is unique per session.
fn limit(key: &str, now: Instant) -> Result<(), u64> {
    let mut m = lock(last_run());
    if m.len() > 1000 {
        m.retain(|_, t| now.duration_since(*t) < MIN_GAP);
    }
    if let Some(t) = m.get(key) {
        let gap = now.duration_since(*t);
        if gap < MIN_GAP {
            return Err((MIN_GAP - gap).as_secs().max(1));
        }
    }
    m.insert(key.to_string(), now);
    Ok(())
}

/// Keeps only the named fields, so nothing unexpected reaches the browser.
fn pick(rows: Vec<Row>, keys: &[&str]) -> Value {
    Value::Array(rows.into_iter().map(|r| Value::Object(keys.iter().filter_map(|k| r.get(*k).map(|v| (k.to_string(), json!(v)))).collect::<Map<_, _>>())).collect())
}

const PING_KEYS: &[&str] = &["seq", "host", "size", "ttl", "time", "status", "sent", "received", "packet-loss", "min-rtt", "avg-rtt", "max-rtt"];
const TRACE_KEYS: &[&str] = &["address", "loss", "sent", "last", "avg", "best", "worst", "status"];
const DNS_KEYS: &[&str] = &["name", "type", "data", "ttl"];

pub fn route(app: &Arc<App>, req: &Req, s: &Session) -> Option<Resp> {
    let tool = req.path.strip_prefix("/api/v1/tools/")?;
    if !matches!(tool, "ping" | "traceroute" | "dns") {
        return None;
    }
    if req.method != "POST" {
        return Some(Resp::error(405, "use POST"));
    }
    Some(run(app, req, s, tool))
}

fn run(app: &Arc<App>, req: &Req, s: &Session, tool: &str) -> Resp {
    let (cmd, keys, note) = match tool {
        "ping" => {
            let b: Ping = match body(req) {
                Ok(b) => b,
                Err(r) => return r,
            };
            if b.count == 0 || b.count > MAX_COUNT {
                return Resp::error(400, &format!("count must be 1-{MAX_COUNT}"));
            }
            let h = match host(&b.host) {
                Ok(h) => h,
                Err(e) => return Resp::error(400, &e),
            };
            (Cmd::new("/ping").arg("address", &h).arg("count", &b.count.to_string()), PING_KEYS, None)
        }
        "traceroute" => {
            let b: Host = match body(req) {
                Ok(b) => b,
                Err(r) => return r,
            };
            let h = match host(&b.host) {
                Ok(h) => h,
                Err(e) => return Resp::error(400, &e),
            };
            (Cmd::new("/tool/traceroute").arg("address", &h).arg("count", "1").arg("max-hops", "20"), TRACE_KEYS, None)
        }
        _ => {
            let b: Name = match body(req) {
                Ok(b) => b,
                Err(r) => return r,
            };
            let n = match crate::apply::dns_name(&b.name) {
                Ok(n) => n,
                Err(e) => return Resp::error(400, &e),
            };
            // `:resolve` is script-only and needs more than the reader may have: look in the cache instead
            (Cmd::new("/ip/dns/cache/print").proplist(DNS_KEYS).query("name", &n), DNS_KEYS, Some("From the router's DNS cache: empty means nobody looked this name up recently."))
        }
    };
    if app.demo.is_some() {
        return Resp::error(409, "demo mode: diagnostics need a real router");
    }
    if let Err(wait) = limit(&s.csrf, Instant::now()) {
        return Resp::error(429, "one diagnostic at a time: wait a moment").with("retry-after", wait.to_string());
    }
    // own connection, so a 10-second ping doesn't hold up the poller
    let rows = app.open(As::Reader).and_then(|mut r| r.run(&cmd));
    match rows {
        Ok(rows) => Resp::json(200, &json!({ "tool": tool, "command": cmd.to_cli(), "rows": pick(rows, keys), "note": note })),
        Err(e) => Resp::error(502, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::handle;
    use crate::api::tests::{admin_session, req, with_session};
    use crate::app::tests::app_with_router;
    use crate::store::Mode;

    #[test]
    fn rate_limit_is_per_session() {
        let now = Instant::now();
        assert!(limit("rl-a", now).is_ok());
        assert!(limit("rl-a", now + Duration::from_millis(500)).is_err());
        assert!(limit("rl-b", now).is_ok());
        assert!(limit("rl-a", now + MIN_GAP).is_ok());
    }

    #[test]
    fn tools_validate_and_run_as_the_reader() {
        let app = Arc::new(app_with_router("read,api,test", Mode::ReadOnly));
        let (c, csrf) = admin_session(&app);
        let call = |p: &str, b: &str| handle(&app, with_session(req("POST", p, b), &c, &csrf)).status;
        assert_eq!(call("/api/v1/tools/ping", r#"{"host":"192.0.2.1; /system/reboot"}"#), 400);
        assert_eq!(call("/api/v1/tools/ping", r#"{"host":"192.0.2.1","count":50}"#), 400);
        assert_eq!(call("/api/v1/tools/ping", r#"{"host":"x","extra":1}"#), 400);
        assert_eq!(call("/api/v1/tools/dns", r#"{"name":"a b"}"#), 400);
        assert_eq!(call("/api/v1/tools/ping", r#"{"host":"192.0.2.1","count":2}"#), 200);
        assert_eq!(call("/api/v1/tools/traceroute", r#"{"host":"example.com"}"#), 429);
        assert_eq!(handle(&app, with_session(req("GET", "/api/v1/tools/ping", ""), &c, &csrf)).status, 405);
        assert_eq!(handle(&app, req("POST", "/api/v1/tools/ping", r#"{"host":"192.0.2.1"}"#)).status, 401);
    }
}
