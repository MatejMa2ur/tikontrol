//! `/api/v1/*` routes plus the embedded UI. Deny by default: only the routes in `PUBLIC` work
//! without a session, every state change needs the CSRF header and a same-origin request, and
//! router configuration needs the admin role. See docs/openapi.yaml.

use crate::app::{lock, App};
use crate::auth::{self, sha256_hex, Session, COOKIE};
use crate::http::{Body, Req, Resp};
use crate::policy;
use crate::ros::conn::{self, Target};
use crate::store::{ct_eq, Cred, Mode, Prefs, Role, RouterCfg};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

mod assets {
    include!(concat!(env!("OUT_DIR"), "/assets.rs"));
}

const PUBLIC: &[(&str, &str)] = &[("GET", "/api/v1/health"), ("POST", "/api/v1/setup/admin"), ("POST", "/api/v1/login")];

pub fn handle(app: &Arc<App>, req: Req) -> Resp {
    if !req.path.starts_with("/api/") && req.path != "/mcp" {
        return asset(&req);
    }
    if req.path == "/mcp" {
        return crate::mcp::handle(app, &req);
    }
    let public = PUBLIC.iter().any(|(m, p)| *m == req.method && *p == req.path);
    let session = req.cookie(COOKIE).and_then(|t| lock(&app.sessions).get(t));
    if req.method != "GET" {
        // CSRF: same-origin plus a per-session token header; public POSTs still need same-origin
        if !auth::same_origin(req.header("origin"), req.header("host"), req.header("sec-fetch-site")) {
            return Resp::error(403, "cross-origin request refused");
        }
        if !public {
            let ok = session.as_ref().is_some_and(|s| req.header("x-csrf-token").is_some_and(|t| ct_eq(t.as_bytes(), s.csrf.as_bytes())));
            if !ok {
                return Resp::error(if session.is_none() { 401 } else { 403 }, if session.is_none() { "login required" } else { "missing or wrong CSRF token" });
            }
        }
    }
    if public {
        return match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/api/v1/health") => health(app),
            ("POST", "/api/v1/setup/admin") => setup_admin(app, &req),
            _ => login(app, &req),
        };
    }
    let Some(s) = session else { return Resp::error(401, "login required") };
    route(app, &req, &s)
}

fn route(app: &Arc<App>, req: &Req, s: &Session) -> Resp {
    let admin = s.role == Role::Admin;
    let need_admin = || Resp::error(403, "admin role required");
    match (req.method.as_str(), req.path.as_str()) {
        ("POST", "/api/v1/logout") => {
            if let Some(t) = req.cookie(COOKIE) {
                lock(&app.sessions).remove(t);
            }
            Resp::json(200, &json!({ "ok": true })).with("set-cookie", auth::clear_cookie(app.tls))
        }
        ("GET", "/api/v1/me") => me(app, s),
        ("GET", "/api/v1/snapshot") => snapshot(app),
        ("GET", "/api/v1/prefs") => Resp::json(200, &serde_json::to_value(&lock(&app.store).state.prefs).unwrap_or_default()),
        ("PUT", "/api/v1/prefs") => put_prefs(app, req),
        ("GET", "/api/v1/router") => router_info(app),
        ("GET", "/api/v1/router/setup-commands") => {
            let ip = req.param("ip").unwrap_or_default();
            if !ip.is_empty() && ip.parse::<std::net::IpAddr>().is_err() {
                return Resp::error(400, "ip must be an IP address");
            }
            Resp::json(200, &json!({ "commands": policy::setup_commands(&ip, req.param("write").as_deref() == Some("1")) }))
        }
        ("POST", "/api/v1/router/probe") if admin => probe(req),
        ("POST", "/api/v1/router") if admin => set_router(app, req),
        ("DELETE", "/api/v1/router") if admin => {
            let mut st = lock(&app.store);
            st.state.router = None;
            let r = st.save();
            drop(st);
            app.drop_reader();
            *app.snap.write().unwrap_or_else(|e| e.into_inner()) = Default::default();
            saved(r)
        }
        ("POST", "/api/v1/router/probe" | "/api/v1/router") | ("DELETE", "/api/v1/router") => need_admin(),
        _ => crate::routes::extra(app, req, s).unwrap_or_else(|| Resp::error(404, "not found")),
    }
}

pub fn saved(r: std::io::Result<()>) -> Resp {
    match r {
        Ok(()) => Resp::json(200, &json!({ "ok": true })),
        Err(_) => Resp::error(500, "could not save state"),
    }
}

pub fn body<T: for<'de> Deserialize<'de>>(req: &Req) -> Result<T, Resp> {
    if !req.header("content-type").is_some_and(|c| c.starts_with("application/json")) {
        return Err(Resp::error(415, "expected application/json"));
    }
    serde_json::from_slice(&req.body).map_err(|e| Resp::error(400, &format!("invalid request body: {}", e.to_string().split(" at line").next().unwrap_or(""))))
}

fn health(app: &App) -> Resp {
    Resp::json(200, &json!({ "ok": true, "version": env!("CARGO_PKG_VERSION"), "needsSetup": lock(&app.setup_token).is_some() }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetupAdmin {
    token: String,
    name: String,
    password: String,
}

fn setup_admin(app: &App, req: &Req) -> Resp {
    let b: SetupAdmin = match body(req) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let ip = req.peer.unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));
    if let Err(wait) = lock(&app.limiter).check(ip) {
        return Resp::error(429, "too many attempts").with("retry-after", wait.to_string());
    }
    let mut tok = lock(&app.setup_token);
    let Some(want) = tok.as_ref() else { return Resp::error(409, "setup is already done") };
    if !ct_eq(want.as_bytes(), b.token.trim().as_bytes()) {
        lock(&app.limiter).fail(ip);
        return Resp::error(403, "wrong setup token (it is printed in the tikontrol log)");
    }
    let a = match auth::new_admin(&b.name, &b.password, Role::Admin) {
        Ok(a) => a,
        Err(e) => return Resp::error(400, &e),
    };
    let mut st = lock(&app.store);
    st.state.admins.push(a);
    if st.save().is_err() {
        st.state.admins.clear();
        return Resp::error(500, "could not save state");
    }
    *tok = None;
    drop(st);
    start_session(app, &b.name, Role::Admin)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    name: String,
    password: String,
}

fn login(app: &App, req: &Req) -> Resp {
    let b: Login = match body(req) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let ip = req.peer.unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));
    if let Err(wait) = lock(&app.limiter).check(ip) {
        return Resp::error(429, "too many failed logins, try again later").with("retry-after", wait.to_string());
    }
    let admins = lock(&app.store).state.admins.clone();
    match auth::verify(&admins, &b.name, &b.password) {
        Some(a) => {
            lock(&app.limiter).success(ip);
            start_session(app, &a.name, a.role.clone())
        }
        None => {
            lock(&app.limiter).fail(ip);
            Resp::error(401, "wrong username or password")
        }
    }
}

fn start_session(app: &App, user: &str, role: Role) -> Resp {
    let (token, s) = lock(&app.sessions).create(user, role);
    Resp::json(200, &json!({ "user": s.user, "role": s.role, "csrf": s.csrf })).with("set-cookie", auth::session_cookie(&token, app.tls))
}

fn me(app: &App, s: &Session) -> Resp {
    let st = lock(&app.store);
    let r = st.state.router.as_ref();
    let snap = app.snap.read().unwrap_or_else(|e| e.into_inner());
    Resp::json(
        200,
        &json!({
            "user": s.user, "role": s.role, "csrf": s.csrf,
            "router": if app.demo.is_some() { Some(json!({ "host": "demo", "mode": Mode::ReadOnly })) } else { r.map(|r| json!({ "host": r.host, "mode": r.mode })) },
            "readOnly": r.is_none_or(|r| r.mode == Mode::ReadOnly) || s.role != Role::Admin,
            "policies": snap.policies,
            "error": snap.error,
        }),
    )
}

fn snapshot(app: &App) -> Resp {
    let s = app.snap.read().unwrap_or_else(|e| e.into_inner());
    match &s.data {
        Some(d) => {
            let mut r = Resp::new(200, "application/json", serde_json::to_vec(d.as_ref()).unwrap_or_default());
            r.headers.push(("cache-control", "no-store".into()));
            r.with("x-snapshot-age", crate::app::now().saturating_sub(s.at).to_string())
        }
        None => Resp::error(503, s.error.as_deref().unwrap_or(if app.router_cfg().is_some() || app.demo.is_some() { "first poll still running" } else { "no router configured" })),
    }
}

fn put_prefs(app: &App, req: &Req) -> Resp {
    let p: Prefs = match body(req) {
        Ok(p) => p,
        Err(r) => return r,
    };
    // names are shown in the UI and MCP output: cap their size, the UI escapes them
    let small = |m: &std::collections::BTreeMap<String, String>| m.len() <= 2000 && m.iter().all(|(k, v)| k.len() <= 64 && v.len() <= 500);
    if !small(&p.aliases) || !small(&p.icons) || !small(&p.notes) || p.groups.len() > 100 || p.hidden.len() > 2000 || req.body.len() > 200_000 {
        return Resp::error(413, "prefs too large");
    }
    let mut st = lock(&app.store);
    st.state.prefs = p;
    saved(st.save())
}

fn router_info(app: &App) -> Resp {
    let st = lock(&app.store);
    let snap = app.snap.read().unwrap_or_else(|e| e.into_inner());
    match &st.state.router {
        None => Resp::json(200, &json!({ "configured": false })),
        Some(r) => Resp::json(
            200,
            &json!({
                "configured": true, "host": r.host, "port": r.port, "pin": r.pin, "mode": r.mode,
                "reader": r.reader.user, "writer": r.writer.as_ref().map(|w| &w.user),
                "policies": snap.policies, "error": snap.error, "lastPoll": snap.at,
                "readOnlyViolations": snap.policies.as_ref().map(|p| p.read_only_violations()),
                "unneeded": snap.policies.as_ref().map(|p| p.extra()),
            }),
        ),
    }
}

fn valid_host(h: &str) -> bool {
    !h.is_empty() && h.len() <= 253 && h.bytes().all(|b| b.is_ascii_alphanumeric() || b".-:".contains(&b))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Probe {
    host: String,
    #[serde(default = "api_ssl")]
    port: u16,
}
fn api_ssl() -> u16 {
    8729
}

fn probe(req: &Req) -> Resp {
    let b: Probe = match body(req) {
        Ok(b) => b,
        Err(r) => return r,
    };
    if !valid_host(&b.host) {
        return Resp::error(400, "invalid router address");
    }
    if b.port == 8728 {
        return Resp::error(400, "8728 is the plain-text API; tikontrol only uses API-SSL (8729)");
    }
    match conn::probe(&b.host, b.port) {
        Ok(fp) => Resp::json(200, &json!({ "pin": fp })),
        Err(e) => Resp::error(502, &e),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CredIn {
    user: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetRouter {
    host: String,
    #[serde(default = "api_ssl")]
    port: u16,
    pin: String,
    #[serde(default)]
    mode: Mode,
    reader: CredIn,
    writer: Option<CredIn>,
    /// switching an existing read-only setup to read-write needs this set
    #[serde(default)]
    confirm_write: bool,
}

fn set_router(app: &App, req: &Req) -> Resp {
    let b: SetRouter = match body(req) {
        Ok(b) => b,
        Err(r) => return r,
    };
    if !valid_host(&b.host) || b.port == 8728 {
        return Resp::error(400, "invalid router address (API-SSL only)");
    }
    if b.pin.len() != 64 || !b.pin.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Resp::error(400, "pin must be the 64-character certificate fingerprint");
    }
    for c in std::iter::once(&b.reader).chain(b.writer.as_ref()) {
        if c.user.is_empty() || c.user.len() > 64 || c.password.is_empty() || c.password.len() > 256 || c.user.chars().any(char::is_control) {
            return Resp::error(400, "invalid router user or password");
        }
    }
    let write = b.mode == Mode::ReadWrite;
    match (&b.writer, write) {
        (Some(_), false) => return Resp::error(400, "read-only mode stores no write credential"),
        (None, true) => return Resp::error(400, "read-write mode needs a separate writer user"),
        _ => {}
    }
    if write && b.writer.as_ref().is_some_and(|w| w.user == b.reader.user) {
        return Resp::error(400, "use separate reader and writer users");
    }
    let was_ro = app.router_cfg().is_none_or(|r| r.mode == Mode::ReadOnly);
    if write && was_ro && !b.confirm_write {
        return Resp::error(409, "switching to read and write needs confirm_write: the writer credential can change the whole router");
    }
    let target = Target { host: b.host.clone(), port: b.port, pin: b.pin.to_ascii_lowercase() };
    let tmp = RouterCfg {
        host: target.host.clone(),
        port: target.port,
        pin: target.pin.clone(),
        mode: b.mode.clone(),
        reader: Cred { user: b.reader.user.clone(), password: String::new() },
        writer: None,
    };
    // prove both credentials work and check what the router really allows before storing anything
    let check = |c: &CredIn| -> Result<policy::Policies, String> {
        let mut r = (app.connector)(&tmp, &c.user, &c.password)?;
        policy::detect(r.as_mut(), &c.user)
    };
    let rp = match check(&b.reader) {
        Ok(p) => p,
        Err(e) => return Resp::error(502, &format!("reader: {e}")),
    };
    if !rp.can_read() {
        return Resp::error(400, "the reader user needs the read and api policies");
    }
    if !rp.read_only_violations().is_empty() {
        return Resp::error(400, &format!("the reader user must not have: {} (create a custom group, see setup commands)", rp.read_only_violations().join(", ")));
    }
    if let Some(w) = &b.writer {
        match check(w) {
            Ok(p) if p.has("write") => {}
            Ok(_) => return Resp::error(400, "the writer user has no write policy"),
            Err(e) => return Resp::error(502, &format!("writer: {e}")),
        }
    }
    let mut st = lock(&app.store);
    let reader = Cred { user: b.reader.user.clone(), password: st.sealer.seal("router-password:reader", &b.reader.password) };
    let writer = b.writer.as_ref().map(|w| Cred { user: w.user.clone(), password: st.sealer.seal("router-password:writer", &w.password) });
    st.state.router = Some(RouterCfg { reader, writer, ..tmp });
    let r = st.save();
    drop(st);
    app.drop_reader();
    *app.snap.write().unwrap_or_else(|e| e.into_inner()) = crate::app::Snap { policies: Some(rp), ..Default::default() };
    saved(r)
}

fn asset(req: &Req) -> Resp {
    if req.method != "GET" && req.method != "HEAD" {
        return Resp::error(405, "method not allowed");
    }
    let path = if req.path == "/" { "/index.html" } else { req.path.as_str() };
    // SPA: unknown paths get index.html; hashed asset names are cached for a year
    let found = assets::ASSETS.iter().find(|(p, ..)| *p == path).or_else(|| assets::ASSETS.iter().find(|(p, ..)| *p == "/index.html"));
    let Some((p, mime, gz, bytes)) = found else { return Resp::error(404, "not found") };
    let mut r = Resp { status: 200, headers: vec![("content-type", (*mime).into())], body: Body::Static(bytes) };
    if *gz {
        r.headers.push(("content-encoding", "gzip".into()));
        r.headers.push(("vary", "accept-encoding".into()));
    }
    let cache = if p.starts_with("/assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    r.headers.push(("cache-control", cache.into()));
    r
}

/// Hash of an MCP/API bearer token, as stored.
pub fn token_hash(t: &str) -> String {
    sha256_hex(t)
}

pub fn json_ok(v: Value) -> Resp {
    Resp::json(200, &v)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::app::tests::fake_connector;
    use crate::store::{tests::tmpdir, Store};

    pub fn req(method: &str, path: &str, body: &str) -> Req {
        let mut r = Req { method: method.into(), path: path.into(), body: body.as_bytes().to_vec(), peer: Some("192.0.2.50".parse().unwrap()), tls: true, ..Default::default() };
        r.headers.push(("host".into(), "tik.test".into()));
        r.headers.push(("origin".into(), "https://tik.test".into()));
        r.headers.push(("content-type".into(), "application/json".into()));
        if let Some((p, q)) = path.split_once('?') {
            r.path = p.into();
            r.query = q.into();
        }
        r
    }

    pub fn with_session(mut r: Req, cookie: &str, csrf: &str) -> Req {
        r.headers.push(("cookie".into(), format!("{COOKIE}={cookie}")));
        r.headers.push(("x-csrf-token".into(), csrf.into()));
        r
    }

    fn json_of(r: &Resp) -> Value {
        match &r.body {
            Body::Bytes(b) => serde_json::from_slice(b).unwrap_or(Value::Null),
            _ => Value::Null,
        }
    }

    fn cookie_of(r: &Resp) -> String {
        let c = &r.headers.iter().find(|(k, _)| *k == "set-cookie").unwrap().1;
        c.split(';').next().unwrap().split_once('=').unwrap().1.to_string()
    }

    pub fn new_app() -> Arc<App> {
        Arc::new(App::new(Store::open(&tmpdir("api")).unwrap(), fake_connector("read,api,test"), true))
    }

    /// Runs first-run setup and returns (cookie, csrf).
    pub fn admin_session(app: &Arc<App>) -> (String, String) {
        let tok = lock(&app.setup_token).clone().unwrap();
        let r = handle(app, req("POST", "/api/v1/setup/admin", &format!(r#"{{"token":"{tok}","name":"admin","password":"a long admin password"}}"#)));
        assert_eq!(r.status, 200);
        (cookie_of(&r), json_of(&r)["csrf"].as_str().unwrap().to_string())
    }

    #[test]
    fn everything_needs_a_session_by_default() {
        let app = new_app();
        for p in ["/api/v1/me", "/api/v1/snapshot", "/api/v1/prefs", "/api/v1/router", "/api/v1/anything"] {
            assert_eq!(handle(&app, req("GET", p, "")).status, 401, "{p}");
        }
        assert_eq!(handle(&app, req("GET", "/api/v1/health", "")).status, 200);
    }

    #[test]
    fn setup_needs_the_token_and_runs_once() {
        let app = new_app();
        let bad = handle(&app, req("POST", "/api/v1/setup/admin", r#"{"token":"x","name":"admin","password":"a long admin password"}"#));
        assert_eq!(bad.status, 403);
        admin_session(&app);
        let again = handle(&app, req("POST", "/api/v1/setup/admin", r#"{"token":"x","name":"evil","password":"a long admin password"}"#));
        assert_eq!(again.status, 409);
        assert_eq!(json_of(&handle(&app, req("GET", "/api/v1/health", "")))["needsSetup"], false);
    }

    #[test]
    fn login_logout_and_rate_limit() {
        let app = new_app();
        admin_session(&app);
        let ok = handle(&app, req("POST", "/api/v1/login", r#"{"name":"admin","password":"a long admin password"}"#));
        assert_eq!(ok.status, 200);
        let (c, csrf) = (cookie_of(&ok), json_of(&ok)["csrf"].as_str().unwrap().to_string());
        assert_eq!(handle(&app, with_session(req("GET", "/api/v1/me", ""), &c, &csrf)).status, 200);
        assert_eq!(handle(&app, with_session(req("POST", "/api/v1/logout", "{}"), &c, &csrf)).status, 200);
        assert_eq!(handle(&app, with_session(req("GET", "/api/v1/me", ""), &c, &csrf)).status, 401);
        for _ in 0..5 {
            assert_eq!(handle(&app, req("POST", "/api/v1/login", r#"{"name":"admin","password":"wrong wrong wrong"}"#)).status, 401);
        }
        assert_eq!(handle(&app, req("POST", "/api/v1/login", r#"{"name":"admin","password":"a long admin password"}"#)).status, 429);
    }

    #[test]
    fn csrf_and_origin_are_enforced() {
        let app = new_app();
        let (c, csrf) = admin_session(&app);
        let body = r#"{"aliases":{},"icons":{},"notes":{},"groups":[],"hidden":[]}"#;
        assert_eq!(handle(&app, with_session(req("PUT", "/api/v1/prefs", body), &c, "wrong")).status, 403);
        let mut cross = with_session(req("PUT", "/api/v1/prefs", body), &c, &csrf);
        cross.headers.retain(|(k, _)| k != "origin");
        cross.headers.push(("origin".into(), "https://evil.example".into()));
        assert_eq!(handle(&app, cross).status, 403);
        assert_eq!(handle(&app, with_session(req("PUT", "/api/v1/prefs", body), &c, &csrf)).status, 200);
        let mut cross_login = req("POST", "/api/v1/login", r#"{"name":"admin","password":"x"}"#);
        cross_login.headers.push(("sec-fetch-site".into(), "cross-site".into()));
        assert_eq!(handle(&app, cross_login).status, 403);
    }

    #[test]
    fn unknown_fields_and_wrong_content_type_are_rejected() {
        let app = new_app();
        let (c, csrf) = admin_session(&app);
        let r = handle(&app, with_session(req("POST", "/api/v1/router/probe", r#"{"host":"x","evil":1}"#), &c, &csrf));
        assert_eq!(r.status, 400);
        let mut r = with_session(req("POST", "/api/v1/router/probe", r#"{"host":"x"}"#), &c, &csrf);
        r.headers.retain(|(k, _)| k != "content-type");
        assert_eq!(handle(&app, r).status, 415);
    }

    #[test]
    fn router_setup_rejects_writers_in_read_only_and_plain_api() {
        let app = new_app();
        let (c, csrf) = admin_session(&app);
        let pin = "a".repeat(64);
        let mk = |b: String| with_session(req("POST", "/api/v1/router", &b), &c, &csrf);
        let ro_with_writer = format!(r#"{{"host":"192.0.2.1","pin":"{pin}","reader":{{"user":"r","password":"router-pass"}},"writer":{{"user":"w","password":"router-pass"}}}}"#);
        assert_eq!(handle(&app, mk(ro_with_writer)).status, 400);
        let plain = format!(r#"{{"host":"192.0.2.1","port":8728,"pin":"{pin}","reader":{{"user":"r","password":"router-pass"}}}}"#);
        assert_eq!(handle(&app, mk(plain)).status, 400);
        let rw_unconfirmed = format!(r#"{{"host":"192.0.2.1","pin":"{pin}","mode":"read-write","reader":{{"user":"r","password":"router-pass"}},"writer":{{"user":"w","password":"router-pass"}}}}"#);
        assert_eq!(handle(&app, mk(rw_unconfirmed)).status, 409);
        let wrong_pw = format!(r#"{{"host":"192.0.2.1","pin":"{pin}","reader":{{"user":"r","password":"nope"}}}}"#);
        assert_eq!(handle(&app, mk(wrong_pw)).status, 502);
        let ok = format!(r#"{{"host":"192.0.2.1","pin":"{pin}","reader":{{"user":"r","password":"router-pass"}}}}"#);
        assert_eq!(handle(&app, mk(ok)).status, 200);
        let info = json_of(&handle(&app, with_session(req("GET", "/api/v1/router", ""), &c, &csrf)));
        assert_eq!(info["mode"], "read-only");
        assert!(info["writer"].is_null());
        assert!(!serde_json::to_string(&info).unwrap().contains("router-pass"));
        app.poll(false);
        assert_eq!(handle(&app, with_session(req("GET", "/api/v1/snapshot", ""), &c, &csrf)).status, 200);
    }

    #[test]
    fn viewers_cannot_configure_the_router() {
        let app = new_app();
        admin_session(&app);
        lock(&app.store).state.admins.push(auth::new_admin("viewer", "a long viewer password", Role::Viewer).unwrap());
        let r = handle(&app, req("POST", "/api/v1/login", r#"{"name":"viewer","password":"a long viewer password"}"#));
        let (c, csrf) = (cookie_of(&r), json_of(&r)["csrf"].as_str().unwrap().to_string());
        assert_eq!(handle(&app, with_session(req("POST", "/api/v1/router/probe", r#"{"host":"192.0.2.1"}"#), &c, &csrf)).status, 403);
        assert_eq!(json_of(&handle(&app, with_session(req("GET", "/api/v1/me", ""), &c, &csrf)))["readOnly"], true);
    }

    #[test]
    fn spa_fallback_serves_index() {
        let app = new_app();
        let r = handle(&app, req("GET", "/clients/abc", ""));
        assert_eq!(r.status, 200);
        assert!(r.headers.iter().any(|(k, v)| *k == "content-type" && v.starts_with("text/html")));
    }
}
