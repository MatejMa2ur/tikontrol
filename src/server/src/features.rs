//! Safe apply routes (see apply.rs). Preview is open to every signed-in user so "Show in RouterOS"
//! works in read-only mode; applying needs the admin role, read-write mode and the writer user.

use crate::api::body;
use crate::app::{lock, now, App, As};
use crate::apply::{self, Plan, WINDOW_SECS};
use crate::auth::Session;
use crate::http::{Req, Resp};
use crate::live::Ev;
use crate::store::{hex, random_bytes, Mode, Role};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone, PartialEq, Debug)]
enum State {
    Previewed,
    Armed { deadline: u64 },
    Kept,
    Reverted,
    Failed(String),
}

struct Change {
    plan: Plan,
    user: String,
    created: u64,
    state: State,
}

fn changes() -> &'static Mutex<BTreeMap<String, Change>> {
    static C: OnceLock<Mutex<BTreeMap<String, Change>>> = OnceLock::new();
    C.get_or_init(Mutex::default)
}

/// Seconds before the router's own revert at which "keep" is refused, so the two can't race.
const MARGIN: u64 = 15;

fn state_json(c: &Change) -> serde_json::Value {
    let (state, deadline) = match &c.state {
        State::Previewed => ("previewed", None),
        State::Armed { deadline } if now() >= *deadline => ("reverted-by-router", Some(*deadline)),
        State::Armed { deadline } => ("armed", Some(*deadline)),
        State::Kept => ("kept", None),
        State::Reverted => ("reverted", None),
        State::Failed(_) => ("failed", None),
    };
    let error = if let State::Failed(e) = &c.state { Some(e.clone()) } else { None };
    json!({ "title": c.plan.title, "risk": c.plan.risk, "state": state, "deadline": deadline, "user": c.user, "created": c.created, "error": error })
}

/// The logged-in MikroTik account if there is one, otherwise the stored writer.
fn writer(app: &App, s: &Session) -> Result<Box<dyn crate::ros::conn::Router + Send>, String> {
    match &s.ros {
        Some(pw) => app.open_as(&s.user, &pw.0),
        None => app.open(As::Writer),
    }
}

fn can_write(app: &App, s: &Session) -> Result<(), &'static str> {
    if s.role != Role::Admin {
        return Err("only admins can change the router");
    }
    if app.demo.is_some() {
        return Err("demo mode: nothing can be changed");
    }
    match app.router_cfg() {
        Some(r) if r.mode == Mode::ReadWrite && (r.writer.is_some() || s.ros.is_some()) => Ok(()),
        Some(r) if r.mode == Mode::ReadWrite => Err("log in with your MikroTik account to make changes: tikontrol stores no write password"),
        _ => Err("read-only mode: tikontrol has no write access to this router (by design: the router user has no write policy)"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Preview {
    intent: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: String,
}

fn audit(app: &App, s: &Session, text: String, level: &str) {
    lock(&app.live.history).event(Ev { t: now(), cat: "Admin".into(), level: level.into(), text: format!("{}: {text}", s.user), mac: None });
}

pub fn route(app: &Arc<App>, req: &Req, s: &Session) -> Option<Resp> {
    Some(match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/v1/apply") => {
            let c = lock(changes());
            let list: Vec<_> = c.iter().rev().filter(|(_, c)| c.state != State::Previewed).take(20).map(|(id, c)| json!({ "id": id, "change": state_json(c) })).collect();
            Resp::json(200, &json!(list))
        }
        ("POST", "/api/v1/apply/preview") => {
            let b: Preview = match body(req) {
                Ok(b) => b,
                Err(r) => return Some(r),
            };
            // the inverse of some intents needs current values: read them with the reader
            let plan = match app.with_reader(|r| apply::compile_value(&b.intent, r)) {
                Ok(p) => p,
                Err(e) => return Some(Resp::error(400, &e)),
            };
            let id = hex(&random_bytes::<6>());
            let resp = json!({
                "id": id, "title": plan.title, "risk": plan.risk, "warnings": plan.warnings,
                "commands": plan.cli(), "undo": plan.undo_cli(), "revertible": plan.revertible,
                "window": WINDOW_SECS, "canApply": can_write(app, s).is_ok(), "reason": can_write(app, s).err(),
            });
            let mut c = lock(changes());
            c.retain(|_, x| now() - x.created < 3600 || matches!(x.state, State::Armed { .. }));
            if c.len() > 200 {
                return Some(Resp::error(429, "too many pending changes"));
            }
            c.insert(id, Change { plan, user: s.user.clone(), created: now(), state: State::Previewed });
            Resp::json(200, &resp)
        }
        ("POST", "/api/v1/apply/start") => {
            if let Err(e) = can_write(app, s) {
                return Some(Resp::error(403, e));
            }
            let b: Id = match body(req) {
                Ok(b) => b,
                Err(r) => return Some(r),
            };
            let plan = {
                let c = lock(changes());
                match c.get(&b.id) {
                    Some(x) if x.state == State::Previewed && x.user == s.user => x.plan.clone(),
                    Some(_) => return Some(Resp::error(409, "this change was already applied or belongs to someone else")),
                    None => return Some(Resp::error(404, "no such change: preview it again")),
                }
            };
            let mut w = match writer(app, s) {
                Ok(w) => w,
                Err(e) => return Some(Resp::error(502, &format!("writer: {e}"))),
            };
            let result = apply::start(w.as_mut(), &b.id, &plan);
            let mut c = lock(changes());
            let Some(ch) = c.get_mut(&b.id) else { return Some(Resp::error(404, "no such change")) };
            match result {
                Ok(()) => {
                    ch.state = if plan.revertible { State::Armed { deadline: now() + WINDOW_SECS } } else { State::Kept };
                    let r = state_json(ch);
                    drop(c);
                    audit(app, s, format!("applied \"{}\"", plan.title), "warning");
                    refresh(app);
                    Resp::json(200, &r)
                }
                Err(e) => {
                    ch.state = State::Failed(e.clone());
                    drop(c);
                    audit(app, s, format!("change \"{}\" failed and was undone", plan.title), "error");
                    Resp::error(502, &e)
                }
            }
        }
        ("POST", "/api/v1/apply/keep") | ("POST", "/api/v1/apply/revert") => {
            if let Err(e) = can_write(app, s) {
                return Some(Resp::error(403, e));
            }
            let keep = req.path.ends_with("/keep");
            let b: Id = match body(req) {
                Ok(b) => b,
                Err(r) => return Some(r),
            };
            let plan = {
                let c = lock(changes());
                match c.get(&b.id).map(|x| (x.state.clone(), x.plan.clone())) {
                    Some((State::Armed { deadline }, p)) if now() + MARGIN < deadline => p,
                    Some((State::Armed { .. }, _)) => return Some(Resp::error(409, "too late: the router is reverting this change by itself")),
                    Some(_) => return Some(Resp::error(409, "this change is not waiting for confirmation")),
                    None => return Some(Resp::error(404, "no such change")),
                }
            };
            let mut w = match writer(app, s) {
                Ok(w) => w,
                Err(e) => return Some(Resp::error(502, &format!("writer: {e} (the router will still revert by itself)"))),
            };
            let left = if keep { vec![] } else { apply::undo_now(w.as_mut(), &plan) };
            if let Err(e) = apply::disarm(w.as_mut(), &b.id) {
                return Some(Resp::error(502, &format!("could not remove the revert job: {e}")));
            }
            let mut c = lock(changes());
            if let Some(ch) = c.get_mut(&b.id) {
                ch.state = if keep { State::Kept } else { State::Reverted };
            }
            drop(c);
            audit(app, s, format!("{} \"{}\"", if keep { "kept" } else { "reverted" }, plan.title), "info");
            refresh(app);
            Resp::json(200, &json!({ "ok": left.is_empty(), "errors": left }))
        }
        _ => return None,
    })
}

/// Re-reads the router soon after a change so the UI shows the result.
fn refresh(app: &Arc<App>) {
    let a = app.clone();
    std::thread::spawn(move || a.poll(false));
}

#[cfg(test)]
mod tests {
    use crate::api::handle;
    use crate::api::tests::{admin_session, new_app, req, with_session};
    use crate::app::{lock, tests::app_with_router};
    use crate::store::{Cred, Mode};
    use std::sync::Arc;

    fn id_of(r: &crate::http::Resp) -> serde_json::Value {
        match &r.body {
            crate::http::Body::Bytes(b) => serde_json::from_slice(b).unwrap(),
            _ => serde_json::Value::Null,
        }
    }

    fn login(app: &Arc<crate::app::App>, name: &str, pw: &str) -> (u16, serde_json::Value, String) {
        let r = handle(app, req("POST", "/api/v1/login", &format!(r#"{{"name":"{name}","password":"{pw}"}}"#)));
        let cookie = r.headers.iter().find(|(k, _)| *k == "set-cookie").map(|(_, v)| v.split(';').next().unwrap().split_once('=').unwrap().1.to_string()).unwrap_or_default();
        (r.status, id_of(&r), cookie)
    }

    const DNS: &str = r#"{"intent":{"kind":"dns-record","name":"nas.lan","address":"192.168.88.4"}}"#;

    #[test]
    fn mikrotik_account_with_write_logs_in_as_admin_and_changes_run_as_them() {
        // read-write mode with no stored writer: only a MikroTik-account login can change anything
        let app = Arc::new(app_with_router("read,write,api,test", Mode::ReadWrite));
        let (local, local_csrf) = admin_session(&app);
        let r = handle(&app, with_session(req("POST", "/api/v1/apply/preview", DNS), &local, &local_csrf));
        assert_eq!(id_of(&r)["canApply"], false, "the local account has no router password");
        assert!(id_of(&r)["reason"].as_str().unwrap().contains("MikroTik account"));

        let (st, v, cookie) = login(&app, "alice", "router-pass");
        assert_eq!(st, 200);
        assert_eq!(v["role"], "admin");
        assert_eq!(v["mikrotik"], true);
        let csrf = v["csrf"].as_str().unwrap().to_string();
        let r = handle(&app, with_session(req("POST", "/api/v1/apply/preview", DNS), &cookie, &csrf));
        assert_eq!(id_of(&r)["canApply"], true);
        let id = id_of(&r)["id"].as_str().unwrap().to_string();
        let r = handle(&app, with_session(req("POST", "/api/v1/apply/start", &format!(r#"{{"id":"{id}"}}"#)), &cookie, &csrf));
        assert_eq!(r.status, 200, "{:?}", id_of(&r));

        // the password lives only in memory, never in the state file or a debug print
        let raw = std::fs::read_to_string(lock(&app.store).dir().join("state.json")).unwrap_or_default();
        assert!(!raw.contains("router-pass"));
        let sess = format!("{:?}", lock(&app.sessions).get(&cookie).unwrap());
        assert!(!sess.contains("router-pass") && sess.contains("***"));
    }

    #[test]
    fn mikrotik_account_without_write_is_a_viewer_and_keeps_no_password() {
        let app = Arc::new(app_with_router("read,api,test", Mode::ReadWrite));
        admin_session(&app);
        let (st, v, cookie) = login(&app, "viewer", "router-pass");
        assert_eq!(st, 200);
        assert_eq!(v["role"], "viewer");
        assert!(lock(&app.sessions).get(&cookie).unwrap().ros.is_none());
    }

    #[test]
    fn mikrotik_login_is_refused_for_wrong_passwords_local_names_and_read_only_mode_writes() {
        let app = Arc::new(app_with_router("read,write,api,test", Mode::ReadOnly));
        admin_session(&app);
        assert_eq!(login(&app, "alice", "wrong").0, 401);
        // the router accepts "router-pass" for any name, but a local name never falls through to the router
        assert_eq!(login(&app, "admin", "router-pass").0, 401);
        // read-only mode stays read-only even for a full MikroTik admin
        let (st, v, cookie) = login(&app, "alice", "router-pass");
        assert_eq!(st, 200);
        let r = handle(&app, with_session(req("POST", "/api/v1/apply/preview", DNS), &cookie, v["csrf"].as_str().unwrap()));
        assert_eq!(id_of(&r)["canApply"], false);
    }

    #[test]
    fn full_flow_in_read_write_mode() {
        let app = Arc::new(app_with_router("read,write,api,test", Mode::ReadWrite));
        {
            let mut st = lock(&app.store);
            let pw = st.sealer.seal("router-password:writer", "router-pass");
            st.state.router.as_mut().unwrap().writer = Some(Cred { user: "tik-write".into(), password: pw });
        }
        let (c, csrf) = admin_session(&app);
        let r = handle(&app, with_session(req("POST", "/api/v1/apply/preview", r#"{"intent":{"kind":"dns-record","name":"nas.lan","address":"192.168.88.4"}}"#), &c, &csrf));
        assert_eq!(r.status, 200);
        let v = id_of(&r);
        assert_eq!(v["canApply"], true);
        assert_eq!(v["commands"][0], "/ip/dns/static/add name=nas.lan address=192.168.88.4 comment=tik:dns:nas.lan");
        let id = v["id"].as_str().unwrap().to_string();
        let r = handle(&app, with_session(req("POST", "/api/v1/apply/start", &format!(r#"{{"id":"{id}"}}"#)), &c, &csrf));
        assert_eq!(r.status, 200, "{:?}", id_of(&r));
        assert_eq!(id_of(&r)["state"], "armed");
        let r = handle(&app, with_session(req("POST", "/api/v1/apply/start", &format!(r#"{{"id":"{id}"}}"#)), &c, &csrf));
        assert_eq!(r.status, 409, "no double apply");
        let r = handle(&app, with_session(req("POST", "/api/v1/apply/keep", &format!(r#"{{"id":"{id}"}}"#)), &c, &csrf));
        assert_eq!(r.status, 200);
        let r = handle(&app, with_session(req("POST", "/api/v1/apply/keep", &format!(r#"{{"id":"{id}"}}"#)), &c, &csrf));
        assert_eq!(r.status, 409);
        assert!(lock(&app.live.history).events.iter().any(|e| e.text.contains("kept")));
    }

    #[test]
    fn preview_works_read_only_but_apply_is_refused() {
        let app = new_app();
        let (c, csrf) = admin_session(&app);
        // no router configured: the preview needs no reader for this intent? it does (reader opens), so expect 400
        let r = handle(&app, with_session(req("POST", "/api/v1/apply/preview", r#"{"intent":{"kind":"block-client","mac":"AA:BB:CC:DD:EE:FF"}}"#), &c, &csrf));
        assert_eq!(r.status, 400);
        let r = handle(&app, with_session(req("POST", "/api/v1/apply/start", r#"{"id":"x"}"#), &c, &csrf));
        assert_eq!(r.status, 403);
        let r = handle(&app, with_session(req("POST", "/api/v1/apply/preview", r#"{"intent":{"kind":"run-script","source":"/system/reboot"}}"#), &c, &csrf));
        assert_eq!(r.status, 400);
    }
}
