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

// ca.crt is the local CA's public certificate (never its key), needed before a device can trust the login page
const PUBLIC: &[(&str, &str)] = &[("GET", "/api/v1/health"), ("GET", "/api/v1/ca.crt"), ("POST", "/api/v1/setup/admin"), ("POST", "/api/v1/setup/start"), ("POST", "/api/v1/setup/probe"), ("POST", "/api/v1/setup/router"), ("POST", "/api/v1/login")];

pub fn handle(app: &Arc<App>, req: Req) -> Resp {
    if !req.path.starts_with("/api/") && req.path != "/mcp" {
        return asset(&req);
    }
    if req.path == "/mcp" {
        return crate::mcp::handle(app, &req);
    }
    let public = PUBLIC.iter().any(|(m, p)| *m == req.method && *p == req.path);
    let mut session = req.cookie(COOKIE).and_then(|t| lock(&app.sessions).get(t));
    // session gone (expired, or tikontrol restarted): a "keep me signed in" cookie opens a new one
    let mut renewed = None;
    if session.is_none() {
        if let Some((t, s)) = req.cookie(auth::REMEMBER_COOKIE).and_then(|t| restore(app, t)) {
            renewed = Some(auth::session_cookie(&t, app.tls));
            session = Some(s);
        }
    }
    let resp = handle_with(app, &req, public, session);
    match renewed {
        Some(c) => resp.with("set-cookie", c),
        None => resp,
    }
}

fn handle_with(app: &Arc<App>, req: &Req, public: bool, session: Option<Session>) -> Resp {
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
            ("GET", "/api/v1/ca.crt") => ca_cert(app),
            ("POST", "/api/v1/setup/admin") => setup_admin(app, req),
            ("POST", "/api/v1/setup/start") => setup_start(app, req),
            ("POST", "/api/v1/setup/probe") => setup_probe(app, req),
            ("POST", "/api/v1/setup/router") => setup_router(app, req),
            _ => login(app, req),
        };
    }
    if session.is_none() {
        if let Some(r) = crate::apikeys::authenticate(app, req) {
            return r.map_or_else(|e| e, |s| on_site(app, req, &s));
        }
    }
    let Some(s) = session else { return Resp::error(401, "login required") };
    on_site(app, req, &s)
}

/// Signed-in requests run against the router named by the `x-tik-site` header (or `site` query
/// parameter, for EventSource), else the main router. Sites are only resolved after authentication.
fn on_site(app: &Arc<App>, req: &Req, s: &Session) -> Resp {
    if req.path == "/api/v1/sites" {
        return crate::sites::route(app, req, s); // the site list lives on the main App
    }
    let id = req.header("x-tik-site").map(str::to_string).or_else(|| req.param("site")).unwrap_or_default();
    if id.is_empty() || id == app.site {
        return route(app, req, s);
    }
    match lock(&app.sites).get(&id).cloned() {
        Some(a) => route(&a, req, s),
        None => Resp::error(404, "no such site"),
    }
}

fn route(app: &Arc<App>, req: &Req, s: &Session) -> Resp {
    let admin = s.role == Role::Admin;
    let need_admin = || Resp::error(403, "admin role required");
    match (req.method.as_str(), req.path.as_str()) {
        ("POST", "/api/v1/logout") => {
            if let Some(t) = req.cookie(COOKIE) {
                lock(&app.sessions).remove(t);
            }
            if let Some(t) = req.cookie(auth::REMEMBER_COOKIE) {
                let h = sha256_hex(t);
                let mut st = lock(&app.store);
                st.state.remembered.retain(|r| r.hash != h);
                let _ = st.save();
            }
            Resp::json(200, &json!({ "ok": true })).with("set-cookie", auth::clear_cookie(app.tls)).with("set-cookie", auth::remember_cookie("", app.tls))
        }
        ("GET", "/api/v1/me") => me(app, s),
        ("GET", "/api/v1/snapshot") => snapshot(app),
        ("GET", "/api/v1/prefs") => Resp::json(200, &serde_json::to_value(&lock(&app.store).state.prefs).unwrap_or_default()),
        ("PUT", "/api/v1/prefs") => put_prefs(app, req),
        ("GET", "/api/v1/router") => router_info(app),
        ("GET", "/api/v1/router/setup-commands") => {
            // ?host=<a newly found router>: only the API-SSL part, for tikontrol's address as that router sees it
            if let Some(h) = req.param("host") {
                let Some(h) = h.parse::<std::net::IpAddr>().ok().filter(|i| local_ip(*i)) else {
                    return Resp::error(400, "host must be a private IP address");
                };
                let me = own_ip_toward(&h.to_string(), 8729).map(|i| i.to_string()).unwrap_or_default();
                return Resp::json(200, &json!({ "commands": policy::api_ssl_commands(&me), "self": me }));
            }
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
            if let Some(slot) = app.cfg_slot(&mut st.state) {
                *slot = None;
            }
            let r = st.save();
            drop(st);
            app.drop_reader();
            *app.snap.write().unwrap_or_else(|e| e.into_inner()) = Default::default();
            saved(r)
        }
        ("PUT", "/api/v1/router/mode") if admin => set_mode(app, req, s),
        ("POST", "/api/v1/router/probe" | "/api/v1/router") | ("DELETE", "/api/v1/router") | ("PUT", "/api/v1/router/mode") => need_admin(),
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
    // after the router is connected, the next step is signing in (with a MikroTik account), not the token
    let needs = lock(&app.setup_token).is_some() && lock(&app.store).state.router.is_none() && app.demo.is_none();
    Resp::json(200, &json!({ "ok": true, "version": env!("CARGO_PKG_VERSION"), "needsSetup": needs, "routerHint": needs.then(router_hint).flatten() }))
}

fn ca_cert(app: &App) -> Resp {
    let dir = lock(&app.store).dir();
    match std::fs::read(dir.join("ca-cert.pem")) {
        Ok(b) => Resp::new(200, "application/x-x509-ca-cert", b).with("content-disposition", "attachment; filename=\"tikontrol-ca.crt\"".into()).with("cache-control", "no-store".into()),
        Err(_) => Resp::error(404, "no local CA: tikontrol uses the operator's certificate"),
    }
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
    let tok = lock(&app.setup_token);
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
    drop(st);
    drop(tok);
    if app.claim().is_err() {
        return Resp::error(500, "could not save state");
    }
    start_session(app, &b.name, Role::Admin)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetupStart {
    token: String,
}

/// The setup token opens a temporary admin session, only to connect the router. It ends, together
/// with the token, as soon as a real admin signs in (see App::claim).
fn setup_start(app: &App, req: &Req) -> Resp {
    let b: SetupStart = match body(req) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let ip = req.peer.unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));
    if let Err(wait) = lock(&app.limiter).check(ip) {
        return Resp::error(429, "too many attempts").with("retry-after", wait.to_string());
    }
    let ok = lock(&app.setup_token).as_ref().is_some_and(|want| ct_eq(want.as_bytes(), b.token.trim().as_bytes()));
    if !ok {
        lock(&app.limiter).fail(ip);
        return Resp::error(403, "wrong or used setup token (it is printed in the tikontrol log)");
    }
    lock(&app.limiter).success(ip);
    start_session(app, auth::SETUP_USER, Role::Admin)
}

/// One-step setup is open only while nobody has claimed tikontrol and no router is connected. With a known
/// router address (TIK_ROUTER or the default gateway) only that address is accepted, so a stranger on the
/// LAN can't claim tikontrol with their own router.
fn setup_open(app: &App, host: &str, port: u16) -> Result<(), Resp> {
    let done = { let st = lock(&app.store); st.state.router.is_some() || st.state.claimed };
    if app.demo.is_some() || done {
        return Err(Resp::error(409, "setup is already done"));
    }
    if !valid_host(host) || port == 8728 {
        return Err(Resp::error(400, "invalid router address (API-SSL only)"));
    }
    if router_hint().is_some_and(|h| hint_host(&h) != host) {
        return Err(Resp::error(403, "only the router tikontrol found can be used here; use the setup token for another one"));
    }
    Ok(())
}

/// Public while unclaimed: reads the router's certificate fingerprint. On failure it also returns the
/// commands that give API-SSL a certificate (fresh router).
fn setup_probe(app: &App, req: &Req) -> Resp {
    let b: Probe = match body(req) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let ip = req.peer.unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));
    if let Err(wait) = lock(&app.limiter).check(ip) {
        return Resp::error(429, "too many attempts").with("retry-after", wait.to_string());
    }
    if let Err(r) = setup_open(app, &b.host, b.port) {
        return r;
    }
    match conn::probe(&b.host, b.port) {
        Ok(fp) => Resp::json(200, &json!({ "pin": fp })),
        Err(e) => {
            lock(&app.limiter).fail(ip); // failures only: keeps this from becoming a port scanner
            let me = own_ip_toward(&b.host, b.port).map(|i| i.to_string()).unwrap_or_default();
            Resp::json(502, &json!({ "error": e, "commands": policy::api_ssl_commands(&me) }))
        }
    }
}

/// LAN, loopback or link-local: the setup guide never points tikontrol at the internet.
fn local_ip(i: std::net::IpAddr) -> bool {
    match i {
        std::net::IpAddr::V4(a) => a.is_private() || a.is_loopback() || a.is_link_local(),
        std::net::IpAddr::V6(a) => a.is_loopback() || a.is_unique_local() || a.is_unicast_link_local(),
    }
}

/// This host's address as the router sees it: the source address the OS picks for that route
/// (a UDP connect sends nothing).
fn own_ip_toward(host: &str, port: u16) -> Option<std::net::IpAddr> {
    let s = std::net::UdpSocket::bind(if host.contains(':') { "[::]:0" } else { "0.0.0.0:0" }).ok()?;
    s.connect((host, port)).ok()?;
    Some(s.local_addr().ok()?.ip()).filter(|i| !i.is_unspecified())
}

/// Where the reader may log in from: the address the router sees our admin API session coming from
/// (right behind NAT, e.g. Docker bridge or a VM), else tikontrol's own address, else the router's LAN.
fn reader_address(r: &mut dyn conn::Router, user: &str, host: &str, port: u16) -> Option<String> {
    let seen = r.run(&crate::ros::cmd::Cmd::new("/user/active/print").proplist(&["address"]).query("name", user).query("via", "api")).unwrap_or_default();
    let mut ips = seen.iter().filter_map(|a| a.get("address")?.parse::<std::net::IpAddr>().ok());
    // ponytail: several API sessions of this admin from different places is ambiguous, so fall back
    if let Some(ip) = ips.next().filter(|first| ips.all(|i| i == *first)) {
        return Some(format!("{ip}/{}", if ip.is_ipv4() { 32 } else { 128 }));
    }
    if let Some(ip) = own_ip_toward(host, port) {
        return Some(format!("{ip}/{}", if ip.is_ipv4() { 32 } else { 128 }));
    }
    let rows = r.run(&crate::ros::cmd::Cmd::new("/ip/address/print").proplist(&["address", "network"])).ok()?;
    rows.iter().find_map(|a| {
        let (ip, len) = a.get("address")?.split_once('/')?;
        (ip == host).then(|| Some(format!("{}/{len}", a.get("network")?))).flatten()
    })
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetupRouter {
    host: String,
    #[serde(default = "api_ssl")]
    port: u16,
    pin: String,
    user: String,
    password: String,
    #[serde(default = "yes")]
    remember: bool,
}

/// One-step setup: a full router admin proves ownership (instead of the setup token). As that admin,
/// tikontrol creates its read-only group and user, stores the reader encrypted, claims itself and signs
/// the admin in. The admin's password lives only in this request and the session (MikroTik-login rules).
fn setup_router(app: &Arc<App>, req: &Req) -> Resp {
    let b: SetupRouter = match body(req) {
        Ok(b) => b,
        Err(r) => return r,
    };
    let ip = req.peer.unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));
    if let Err(wait) = lock(&app.limiter).check(ip) {
        return Resp::error(429, "too many failed sign-ins, try again later").with("retry-after", wait.to_string());
    }
    // held until the router is stored: one setup at a time, and never after the claim
    let tok = lock(&app.setup_token);
    if tok.is_none() {
        return Resp::error(409, "setup is already done");
    }
    if let Err(r) = setup_open(app, &b.host, b.port) {
        return r;
    }
    if !valid_pin(&b.pin) {
        return Resp::error(400, "pin must be the 64-character certificate fingerprint");
    }
    if b.user.is_empty() || b.user.len() > 64 || b.user.chars().any(char::is_control) || b.password.is_empty() || b.password.len() > auth::MAX_PASSWORD {
        return Resp::error(400, "invalid router user or password");
    }
    let cfg = RouterCfg {
        host: b.host.clone(),
        port: b.port,
        pin: b.pin.to_ascii_lowercase(),
        mode: Mode::ReadOnly,
        reader: Cred { user: policy::reader_name(), password: String::new() },
        writer: None,
    };
    let mut r = match (app.connector)(&cfg, &b.user, &b.password) {
        Ok(r) => r,
        Err(e) => {
            lock(&app.limiter).fail(ip);
            return Resp::error(if e.contains("rejected") { 401 } else { 502 }, &e);
        }
    };
    let p = match policy::detect(r.as_mut(), &b.user) {
        Ok(p) => p,
        Err(e) => return Resp::error(502, &e),
    };
    let missing: Vec<&str> = policy::ADMIN.iter().copied().filter(|x| !p.has(x)).collect();
    if !missing.is_empty() {
        lock(&app.limiter).fail(ip);
        return Resp::error(403, &format!("'{}' is not a full admin (missing {}). Use an admin account, or the setup token", b.user, missing.join(", ")));
    }
    let Some(addr) = reader_address(r.as_mut(), &b.user, &b.host, b.port) else { return Resp::error(502, "cannot tell tikontrol's own address") };
    let pw = crate::store::hex(&crate::store::random_bytes::<16>()); // 32 characters, never shown or logged
    let name = policy::reader_name();
    if let Err(e) = policy::ensure_reader(r.as_mut(), &name, &addr, &pw) {
        return Resp::error(502, &format!("creating {name} on the router failed: {e}"));
    }
    drop(r);
    let mut st = lock(&app.store);
    let reader = Cred { user: name.clone(), password: st.sealer.seal("router-password:reader", &pw) };
    st.state.router = Some(RouterCfg { reader, ..cfg });
    let saved = st.save();
    drop(st);
    drop(tok);
    if saved.is_err() || app.claim().is_err() {
        return Resp::error(500, "could not save state");
    }
    app.drop_reader();
    *app.snap.write().unwrap_or_else(|e| e.into_inner()) = Default::default();
    lock(&app.limiter).success(ip);
    eprintln!("tikontrol: set up by router admin {:?}; created or reset {name} on the router", b.user);
    // first read right away instead of at the next poll (up to TIK_POLL seconds), so the dashboard opens quickly
    let a = app.clone();
    std::thread::spawn(move || a.poll(false));
    let ros = Some(auth::Secret(b.password));
    let (token, s) = lock(&app.sessions).create_with(&b.user, Role::Admin, ros.clone());
    let resp = Resp::json(200, &json!({ "user": s.user, "role": s.role, "csrf": s.csrf, "mikrotik": true })).with("set-cookie", auth::session_cookie(&token, app.tls));
    if b.remember { remember(app, resp, &b.user, Role::Admin, ros) } else { resp }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    name: String,
    password: String,
    /// keep this browser signed in for 30 days, across restarts
    #[serde(default)]
    remember: bool,
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
    if let Some(a) = auth::verify(&admins, &b.name, &b.password) {
        lock(&app.limiter).success(ip);
        if a.role == Role::Admin && app.claim().is_err() {
            return Resp::error(500, "could not save state");
        }
        let resp = start_session(app, &a.name, a.role.clone());
        return if b.remember { remember(app, resp, &a.name, a.role.clone(), None) } else { resp };
    }
    // not a local account: try it as a MikroTik account (local names always win, so they can't be shadowed)
    if !admins.iter().any(|a| a.name == b.name) {
        if let Some((role, ros)) = router_login(app, &b.name, &b.password) {
            lock(&app.limiter).success(ip);
            if role == Role::Admin && app.claim().is_err() {
                return Resp::error(500, "could not save state");
            }
            let (token, s) = lock(&app.sessions).create_with(&b.name, role.clone(), ros.clone());
            let resp = Resp::json(200, &json!({ "user": s.user, "role": s.role, "csrf": s.csrf, "mikrotik": true })).with("set-cookie", auth::session_cookie(&token, app.tls));
            return if b.remember { remember(app, resp, &b.name, role, ros) } else { resp };
        }
    }
    lock(&app.limiter).fail(ip);
    Resp::error(401, "wrong username or password")
}

/// Checks a MikroTik account by logging in to the router. The password is never stored: it stays in the
/// in-memory session only when the account can write, so that user's changes run as them.
/// Role: `write` policy → admin, otherwise viewer.
fn router_login(app: &App, user: &str, pw: &str) -> Option<(Role, Option<auth::Secret>)> {
    if app.demo.is_some() || user.is_empty() || user.len() > 64 || user.chars().any(char::is_control) || pw.is_empty() || pw.len() > auth::MAX_PASSWORD {
        return None;
    }
    let mut r = app.open_as(user, pw).ok()?;
    let p = policy::detect(r.as_mut(), user).ok()?;
    if !p.can_read() {
        return None;
    }
    if p.has("write") { Some((Role::Admin, Some(auth::Secret(pw.into())))) } else { Some((Role::Viewer, None)) }
}

/// Adds a "keep me signed in" cookie to a successful login. The stored entry holds only the token's hash.
fn remember(app: &App, resp: Resp, user: &str, role: Role, ros: Option<auth::Secret>) -> Resp {
    let token = crate::store::hex(&crate::store::random_bytes::<32>());
    let mut st = lock(&app.store);
    let now = crate::app::now();
    let ros = ros.map(|r| st.sealer.seal("remember", &r.0));
    let list = &mut st.state.remembered;
    list.retain(|r| r.expires > now);
    if list.len() >= auth::MAX_REMEMBERED {
        list.remove(0); // oldest first
    }
    list.push(crate::store::Remembered { hash: sha256_hex(&token), user: user.into(), role, ros, expires: now + auth::REMEMBER.as_secs() });
    if st.save().is_err() {
        return resp; // still signed in, just not remembered
    }
    resp.with("set-cookie", auth::remember_cookie(&token, app.tls))
}

/// Opens a session from a "keep me signed in" token, if it is known and not expired.
/// ponytail: the token isn't rotated on use; rotate it if cookie theft becomes a concern.
fn restore(app: &App, token: &str) -> Option<(String, Session)> {
    let h = sha256_hex(token);
    let st = lock(&app.store);
    let r = st.state.remembered.iter().find(|r| ct_eq(r.hash.as_bytes(), h.as_bytes()) && r.expires > crate::app::now())?.clone();
    let ros = r.ros.as_deref().and_then(|s| st.sealer.open_str("remember", s)).map(auth::Secret);
    drop(st);
    Some(lock(&app.sessions).create_with(&r.user, r.role, ros))
}

fn start_session(app: &App, user: &str, role: Role) -> Resp {
    let (token, s) = lock(&app.sessions).create(user, role);
    Resp::json(200, &json!({ "user": s.user, "role": s.role, "csrf": s.csrf })).with("set-cookie", auth::session_cookie(&token, app.tls))
}

fn me(app: &App, s: &Session) -> Resp {
    let st = lock(&app.store);
    let r = app.cfg(&st.state);
    let snap = app.snap.read().unwrap_or_else(|e| e.into_inner());
    Resp::json(
        200,
        &json!({
            "user": s.user, "role": s.role, "csrf": s.csrf, "setup": s.user == auth::SETUP_USER,
            "router": if app.demo.is_some() { Some(json!({ "host": "demo", "mode": Mode::ReadOnly })) } else { r.map(|r| json!({ "host": r.host, "mode": r.mode })) },
            "readOnly": r.is_none_or(|r| r.mode == Mode::ReadOnly || (r.writer.is_none() && app.ros(s).is_none())) || s.role != Role::Admin,
            "site": app.site,
            "mikrotik": s.ros.is_some(),
            "policies": snap.policies,
            "error": snap.error,
            "routerHint": r.is_none().then(router_hint).flatten(),
        }),
    )
}

/// Router address to pre-fill on first setup: `TIK_ROUTER`, else this host's default gateway (which is
/// the router itself when tikontrol runs as a RouterOS container).
fn router_hint() -> Option<String> {
    std::env::var("TIK_ROUTER").ok().filter(|h| !h.is_empty()).or_else(|| default_gateway(&std::fs::read_to_string("/proc/net/route").ok()?))
}

/// The host part of a `TIK_ROUTER` written as `host:port` (IPv4 or name; a bare IPv6 address stays whole).
fn hint_host(h: &str) -> &str {
    match h.split_once(':') {
        Some((host, port)) if port.parse::<u16>().is_ok() => host,
        _ => h,
    }
}

/// Default gateway from Linux's /proc/net/route (addresses are little-endian hex).
fn default_gateway(route: &str) -> Option<String> {
    route.lines().skip(1).find_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        let gw = u32::from_str_radix(f.get(2)?, 16).ok()?;
        (f.get(1) == Some(&"00000000") && gw != 0).then(|| std::net::Ipv4Addr::from(gw.swap_bytes()).to_string())
    })
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
    if req.body.len() > 200_000 || !prefs_ok(&p) {
        return Resp::error(413, "prefs too large or invalid");
    }
    let mut st = lock(&app.store);
    st.state.prefs = p;
    saved(st.save())
}

fn prefs_ok(p: &Prefs) -> bool {
    // names are shown in the UI and MCP output: cap their size, the UI escapes them
    let small = |m: &std::collections::BTreeMap<String, String>| m.len() <= 2000 && m.iter().all(|(k, v)| k.len() <= 64 && v.len() <= 500);
    // card ids are short slugs; the UI ignores ids it doesn't know
    let ids = |v: &[String]| v.len() <= 32 && v.iter().all(|i| !i.is_empty() && i.len() <= 32 && i.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'));
    small(&p.aliases) && small(&p.icons) && small(&p.notes) && p.groups.len() <= 100 && p.port_profiles.len() <= 50 && serde_json::to_string(&p.port_profiles).is_ok_and(|j| j.len() <= 20_000) && p.hidden.len() <= 2000 && ids(&p.card_order) && ids(&p.hidden_cards) && ids(&p.wide_cards)
}

fn router_info(app: &App) -> Resp {
    let st = lock(&app.store);
    let snap = app.snap.read().unwrap_or_else(|e| e.into_inner());
    match app.cfg(&st.state) {
        None => Resp::json(200, &json!({ "configured": false })),
        Some(r) => Resp::json(
            200,
            &json!({
                "configured": true, "host": r.host, "port": r.port, "pin": r.pin, "mode": r.mode,
                "reader": r.reader.user, "writer": r.writer.as_ref().map(|w| &w.user),
                "policies": snap.policies, "error": snap.error, "lastPoll": snap.at,
                "readOnlyViolations": snap.policies.as_ref().map(|p| p.read_only_violations()),
                "unneeded": snap.policies.as_ref().map(|p| p.extra()),
                // where the router can send flows/syslog to this tikontrol (behind NAT: the mapped host address instead)
                "selfAddress": own_ip_toward(&r.host, r.port).map(|i| i.to_string()),
            }),
        ),
    }
}

pub fn valid_pin(p: &str) -> bool {
    p.len() == 64 && p.bytes().all(|c| c.is_ascii_hexdigit())
}

pub fn valid_cred(c: &CredIn) -> bool {
    !(c.user.is_empty() || c.user.len() > 64 || c.password.is_empty() || c.password.len() > 256 || c.user.chars().any(char::is_control))
}

pub fn valid_host(h: &str) -> bool {
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
pub struct CredIn {
    pub user: String,
    pub password: String,
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

fn set_router(app: &Arc<App>, req: &Req) -> Resp {
    let b: SetRouter = match body(req) {
        Ok(b) => b,
        Err(r) => return r,
    };
    if !valid_host(&b.host) || b.port == 8728 {
        return Resp::error(400, "invalid router address (API-SSL only)");
    }
    if !valid_pin(&b.pin) {
        return Resp::error(400, "pin must be the 64-character certificate fingerprint");
    }
    if !std::iter::once(&b.reader).chain(b.writer.as_ref()).all(valid_cred) {
        return Resp::error(400, "invalid router user or password");
    }
    let write = b.mode == Mode::ReadWrite;
    // read-write without a stored writer is fine: changes run as the admin logged in with their MikroTik account
    if b.writer.is_some() && !write {
        return Resp::error(400, "read-only mode stores no write credential");
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
    let reader = Cred { user: b.reader.user.clone(), password: st.sealer.seal(&app.label("reader"), &b.reader.password) };
    let writer = b.writer.as_ref().map(|w| Cred { user: w.user.clone(), password: st.sealer.seal(&app.label("writer"), &w.password) });
    let Some(slot) = app.cfg_slot(&mut st.state) else { return Resp::error(404, "no such site") };
    *slot = Some(RouterCfg { reader, writer, ..tmp });
    let r = st.save();
    drop(st);
    app.drop_reader();
    *app.snap.write().unwrap_or_else(|e| e.into_inner()) = crate::app::Snap { policies: Some(rp), ..Default::default() };
    if r.is_ok() {
        let a = app.clone();
        std::thread::spawn(move || a.poll(false)); // first read now, not at the next poll
    }
    saved(r)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetMode {
    mode: Mode,
    /// read-write is a high-risk change: the browser must confirm it explicitly
    #[serde(default)]
    confirm_write: bool,
}

/// Switches tikontrol between read-only and read-write without re-entering the router credentials.
/// Read-write stores nothing new: changes run as the signed-in MikroTik admin (or a writer set up earlier).
/// Back to read-only drops any stored write credential.
fn set_mode(app: &App, req: &Req, s: &Session) -> Resp {
    let b: SetMode = match body(req) {
        Ok(b) => b,
        Err(r) => return r,
    };
    if b.mode == Mode::ReadWrite && !b.confirm_write {
        return Resp::error(400, "confirm_write is required to allow changes");
    }
    let mut st = lock(&app.store);
    let Some(r) = app.cfg_slot(&mut st.state).and_then(|s| s.as_mut()) else { return Resp::error(409, "no router connected") };
    r.mode = b.mode.clone();
    if b.mode == Mode::ReadOnly {
        r.writer = None;
    }
    let res = st.save();
    drop(st);
    let text = format!("{}: tikontrol switched to {}", s.user, if b.mode == Mode::ReadWrite { "read & write" } else { "read-only" });
    lock(&app.live.history).event(crate::live::Ev { t: crate::app::now(), cat: "Admin".into(), level: "warning".into(), text, mac: None });
    saved(res)
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
    #[test]
    fn default_gateway_from_proc_net_route() {
        let r = "Iface\tDestination\tGateway \tFlags\nveth0\t0058A8C0\t00000000\t0001\nveth0\t00000000\t0158A8C0\t0003\n";
        assert_eq!(super::default_gateway(r).as_deref(), Some("192.168.88.1"));
        assert_eq!(super::default_gateway("Iface\tDestination\tGateway\n"), None);
    }

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
        assert_eq!(handle(&app, req("GET", "/api/v1/ca.crt", "")).status, 404, "public, but only the CA certificate");
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
    fn mode_switch_needs_admin_confirmation_and_a_router() {
        let app = new_app();
        let (c, csrf) = admin_session(&app);
        let put = |b: &str| handle(&app, with_session(req("PUT", "/api/v1/router/mode", b), &c, &csrf)).status;
        assert_eq!(put(r#"{"mode":"read-write"}"#), 400, "must be confirmed");
        assert_eq!(put(r#"{"mode":"read-write","confirm_write":true}"#), 409, "no router yet");
        lock(&app.store).state.router = Some(RouterCfg { host: "192.0.2.1".into(), port: 8729, pin: "00".into(), mode: Mode::ReadOnly, reader: Cred { user: "r".into(), password: "x".into() }, writer: None });
        assert_eq!(put(r#"{"mode":"read-write","confirm_write":true}"#), 200);
        assert_eq!(lock(&app.store).state.router.as_ref().unwrap().mode, Mode::ReadWrite);
        assert_eq!(put(r#"{"mode":"read-only"}"#), 200);
        assert_eq!(lock(&app.store).state.router.as_ref().unwrap().mode, Mode::ReadOnly);
    }

    #[test]
    fn keep_me_signed_in_survives_a_restart_until_logout() {
        let app = new_app();
        admin_session(&app);
        let ok = handle(&app, req("POST", "/api/v1/login", r#"{"name":"admin","password":"a long admin password","remember":true}"#));
        let r = ok.headers.iter().find(|(k, v)| *k == "set-cookie" && v.starts_with("tik_r=")).unwrap().1.split(';').next().unwrap().to_string();
        assert!(!lock(&app.store).state.remembered[0].hash.contains(&r[6..]), "only the hash is stored");
        lock(&app.sessions).clear(); // restart
        let with_r = |path: &str, method: &str| { let mut q = req(method, path, "{}"); q.headers.push(("cookie".into(), r.clone())); q };
        let me = handle(&app, with_r("/api/v1/me", "GET"));
        assert_eq!(me.status, 200);
        let (c, csrf) = (cookie_of(&me), json_of(&me)["csrf"].as_str().unwrap().to_string());
        let mut out = req("POST", "/api/v1/logout", "{}");
        out.headers.push(("cookie".into(), format!("{COOKIE}={c}; {r}")));
        out.headers.push(("x-csrf-token".into(), csrf));
        assert_eq!(handle(&app, out).status, 200);
        lock(&app.sessions).clear();
        assert_eq!(handle(&app, with_r("/api/v1/me", "GET")).status, 401, "logout forgets the browser");
    }

    #[test]
    fn setup_guide_commands_only_for_local_hosts() {
        let app = new_app();
        let (c, csrf) = admin_session(&app);
        let get = |q: &str| handle(&app, with_session(req("GET", &format!("/api/v1/router/setup-commands?host={q}"), ""), &c, &csrf));
        for bad in ["8.8.8.8", "example.com", "", "2001:db8::1"] {
            assert_eq!(get(bad).status, 400, "{bad}");
        }
        let ok = json_of(&get("127.0.0.1"));
        assert_eq!(ok["self"], "127.0.0.1");
        let cmds: Vec<String> = serde_json::from_value(ok["commands"].clone()).unwrap();
        assert!(cmds.iter().any(|c| c.contains("api-ssl") && c.contains("127.0.0.1/32")));
        assert!(!cmds.iter().any(|c| c.contains("/user/add")), "only the API-SSL part");
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

    /// A fake router that keeps every command it saw, across connections.
    type Recorded = Arc<std::sync::Mutex<Vec<Vec<String>>>>;

    fn recording_app(policy: &'static str) -> (Arc<App>, Recorded) {
        use crate::ros::queries::tests::Fake;
        struct Rec(Fake, Arc<std::sync::Mutex<Vec<Vec<String>>>>);
        impl conn::Router for Rec {
            fn run(&mut self, c: &crate::ros::cmd::Cmd) -> Result<Vec<crate::ros::conn::Row>, String> {
                self.1.lock().unwrap().push(c.words()?.to_vec());
                self.0.run(c)
            }
        }
        let log = Arc::new(std::sync::Mutex::new(vec![]));
        let l = log.clone();
        let conn: crate::app::Connector = Arc::new(move |_, u, p| {
            if p != "router-pass" {
                return Err("router rejected the login".into());
            }
            Ok(Box::new(Rec(crate::app::tests::fake_router(u, policy), l.clone())) as Box<dyn conn::Router + Send>)
        });
        (Arc::new(App::new(Store::open(&tmpdir("onestep")).unwrap(), conn, true)), log)
    }

    fn one_step(app: &Arc<App>, user: &str, pw: &str) -> (Resp, String) {
        let host = router_hint().map(|h| hint_host(&h).to_string()).unwrap_or("127.0.0.1".into());
        let pin = "a".repeat(64);
        (handle(app, req("POST", "/api/v1/setup/router", &format!(r#"{{"host":"{host}","pin":"{pin}","user":"{user}","password":"{pw}"}}"#))), host)
    }

    #[test]
    fn router_hint_may_carry_a_port() {
        assert_eq!(hint_host("127.0.0.1:8929"), "127.0.0.1");
        assert_eq!(hint_host("192.168.88.1"), "192.168.88.1");
        assert_eq!(hint_host("fd00::1"), "fd00::1");
    }

    #[test]
    fn one_step_setup_creates_the_reader_and_claims() {
        let (app, log) = recording_app("read,write,policy,api,test,ssh");
        let r = one_step(&app, "admin", "router-pass").0;
        assert_eq!(r.status, 200, "{:?}", json_of(&r));
        assert_eq!(json_of(&r)["mikrotik"], true);
        let addr = "192.0.2.50/32"; // where the fake router sees the admin's API session (NAT), not our own IP
        let st = lock(&app.store).state.clone();
        assert!(st.claimed && lock(&app.setup_token).is_none());
        let cfg = st.router.unwrap();
        assert_eq!((cfg.mode, cfg.reader.user.as_str(), cfg.writer.is_none()), (Mode::ReadOnly, "tikontrol-read", true));
        let pw = lock(&app.store).sealer.open_str("router-password:reader", &cfg.reader.password).unwrap();
        assert_eq!(pw.len(), 32);
        let writes: Vec<Vec<String>> = log.lock().unwrap().iter().filter(|c| !c[0].ends_with("/print")).cloned().collect();
        let w = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(writes, [
            w(&["/user/group/add", "=name=tikontrol-read", "=policy=read,api,test", "=comment=tik: tikontrol"]),
            w(&["/user/add", "=name=tikontrol-read", "=group=tikontrol-read", &format!("=address={addr}"), &format!("=password={pw}"), "=comment=tik: tikontrol"]),
        ]);
        let body = String::from_utf8_lossy(match &r.body { Body::Bytes(b) => b, _ => &[] }).to_string();
        assert!(!body.contains(&pw) && !body.contains("router-pass"), "no password in the answer");
        assert!(!std::fs::read_to_string(lock(&app.store).dir().join("state.json")).unwrap().contains(&pw), "stored encrypted");
        assert_eq!(one_step(&app, "admin", "router-pass").0.status, 409, "only once");
        assert_eq!(handle(&app, req("POST", "/api/v1/setup/probe", r#"{"host":"127.0.0.1"}"#)).status, 409);
    }

    #[test]
    fn one_step_setup_refuses_non_admins_and_is_rate_limited() {
        let (app, log) = recording_app("read,api,test");
        let r = one_step(&app, "viewer", "router-pass").0;
        assert_eq!(r.status, 403);
        assert!(json_of(&r)["error"].as_str().unwrap().contains("write, policy"));
        assert!(log.lock().unwrap().iter().all(|c| c[0].ends_with("/print")), "nothing written");
        assert!(lock(&app.store).state.router.is_none() && !lock(&app.store).state.claimed);
        for _ in 0..4 {
            assert_eq!(one_step(&app, "admin", "wrong").0.status, 401);
        }
        assert_eq!(one_step(&app, "admin", "router-pass").0.status, 429);
    }

    #[test]
    fn one_step_setup_needs_an_unclaimed_tikontrol() {
        let (app, log) = recording_app("read,write,policy,api,test");
        admin_session(&app);
        assert_eq!(one_step(&app, "admin", "router-pass").0.status, 409);
        assert!(log.lock().unwrap().is_empty(), "never even connects");
    }

    #[test]
    fn spa_fallback_serves_index() {
        let app = new_app();
        let r = handle(&app, req("GET", "/clients/abc", ""));
        assert_eq!(r.status, 200);
        assert!(r.headers.iter().any(|(k, v)| *k == "content-type" && v.starts_with("text/html")));
    }
}

#[cfg(test)]
mod prefs_tests {
    use super::*;

    #[test]
    fn prefs_validation() {
        // old state/clients without the layout fields still load
        let p: Prefs = serde_json::from_str(r#"{"aliases":{},"hidden":[]}"#).unwrap();
        assert!(prefs_ok(&p) && p.card_order.is_empty());
        let mut p: Prefs = serde_json::from_str(r#"{"cardOrder":["activity","traffic"],"hiddenCards":["aps"]}"#).unwrap();
        assert!(prefs_ok(&p) && p.card_order[0] == "activity");
        p.hidden_cards = vec!["<script>".into()];
        assert!(!prefs_ok(&p));
        p.hidden_cards = vec!["a".into(); 33];
        assert!(!prefs_ok(&p));
        p.hidden_cards = vec![];
        p.wide_cards = vec!["traffic".into()];
        assert!(prefs_ok(&p));
        p.wide_cards = vec!["x\"y".into()];
        assert!(!prefs_ok(&p));
    }
}
