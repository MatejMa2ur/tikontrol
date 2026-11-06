//! tikontrol's own users (not RouterOS users). Listing shows names and roles only, never hashes.
//! Adding and removing users needs the admin role; the last admin and your own account can't be
//! removed. Changing your password needs the current one, ends all your sessions and starts a new one.

use crate::api::{body, saved};
use crate::app::{lock, App};
use crate::auth::{self, Session};
use crate::http::{Req, Resp};
use crate::live::Ev;
use crate::store::Role;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

pub const MAX_ADMINS: usize = 50;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewAdmin {
    name: String,
    password: String,
    role: Role,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewPassword {
    current: String,
    new: String,
}

fn audit(app: &App, s: &Session, text: String) {
    lock(&app.live.history).event(Ev { t: crate::app::now(), cat: "Admin".into(), level: "warning".into(), text: format!("{}: {text}", s.user), mac: None });
}

pub fn route(app: &Arc<App>, req: &Req, s: &Session) -> Option<Resp> {
    let admin = s.role == Role::Admin;
    Some(match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/v1/admins") => {
            let st = lock(&app.store);
            let list: Vec<_> = st.state.admins.iter().map(|a| json!({ "name": a.name, "role": a.role, "you": a.name == s.user })).collect();
            Resp::json(200, &json!(list))
        }
        ("POST", "/api/v1/admins") | ("DELETE", "/api/v1/admins") if !admin => Resp::error(403, "admin role required"),
        ("POST", "/api/v1/admins") => {
            let b: NewAdmin = match body(req) {
                Ok(b) => b,
                Err(r) => return Some(r),
            };
            let a = match auth::new_admin(&b.name, &b.password, b.role) {
                Ok(a) => a,
                Err(e) => return Some(Resp::error(400, &e)),
            };
            let mut st = lock(&app.store);
            if st.state.admins.iter().any(|x| x.name == a.name) {
                return Some(Resp::error(409, "that username is taken"));
            }
            if st.state.admins.len() >= MAX_ADMINS {
                return Some(Resp::error(409, "too many users"));
            }
            let text = format!("added user {} ({:?})", a.name, a.role).to_lowercase();
            st.state.admins.push(a);
            let r = st.save();
            if r.is_err() {
                st.state.admins.pop();
            }
            drop(st);
            audit(app, s, text);
            saved(r)
        }
        ("DELETE", "/api/v1/admins") => {
            let name = req.param("name").unwrap_or_default();
            if name == s.user {
                return Some(Resp::error(409, "you can't remove yourself"));
            }
            let mut st = lock(&app.store);
            let Some(pos) = st.state.admins.iter().position(|a| a.name == name) else { return Some(Resp::error(404, "no such user")) };
            let admins = st.state.admins.iter().filter(|a| a.role == Role::Admin).count();
            if st.state.admins[pos].role == Role::Admin && admins <= 1 {
                return Some(Resp::error(409, "can't remove the last admin"));
            }
            let gone = st.state.admins.remove(pos);
            st.state.remembered.retain(|r| r.user != name);
            let r = st.save();
            if r.is_err() {
                st.state.admins.insert(pos, gone);
                return Some(saved(r));
            }
            drop(st);
            lock(&app.sessions).remove_user(&name);
            audit(app, s, format!("removed user {name}"));
            saved(r)
        }
        ("POST", "/api/v1/admins/password") => change_password(app, req, s),
        _ => return None,
    })
}

fn change_password(app: &App, req: &Req, s: &Session) -> Resp {
    let b: NewPassword = match body(req) {
        Ok(b) => b,
        Err(r) => return r,
    };
    // the current password is a login: same brute-force limit
    let ip = req.peer.unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));
    if let Err(wait) = lock(&app.limiter).check(ip) {
        return Resp::error(429, "too many failed attempts, try again later").with("retry-after", wait.to_string());
    }
    let admins = lock(&app.store).state.admins.clone();
    let Some(me) = auth::verify(&admins, &s.user, &b.current) else {
        lock(&app.limiter).fail(ip);
        return Resp::error(403, "current password is wrong");
    };
    if b.new == b.current {
        return Resp::error(400, "the new password must be different");
    }
    let fresh = match auth::new_admin(&me.name, &b.new, me.role.clone()) {
        Ok(a) => a,
        Err(e) => return Resp::error(400, &e),
    };
    let mut st = lock(&app.store);
    let Some(a) = st.state.admins.iter_mut().find(|a| a.name == s.user) else { return Resp::error(404, "no such user") };
    let old = std::mem::replace(a, fresh);
    if st.save().is_err() {
        if let Some(a) = st.state.admins.iter_mut().find(|a| a.name == s.user) {
            *a = old;
        }
        return Resp::error(500, "could not save state");
    }
    drop(st);
    lock(&app.limiter).success(ip);
    // every session of this user ends (a stolen cookie stops working); this browser gets a new one
    let mut sessions = lock(&app.sessions);
    sessions.remove_user(&s.user);
    let (token, ns) = sessions.create(&s.user, s.role.clone());
    drop(sessions);
    lock(&app.live.history).event(Ev { t: crate::app::now(), cat: "Admin".into(), level: "info".into(), text: format!("{}: changed their password", s.user), mac: None });
    Resp::json(200, &json!({ "user": ns.user, "role": ns.role, "csrf": ns.csrf })).with("set-cookie", auth::session_cookie(&token, app.tls))
}

#[cfg(test)]
mod tests {
    use crate::api::handle;
    use crate::api::tests::{admin_session, new_app, req, with_session};
    use crate::http::{Body, Resp};
    use serde_json::Value;

    fn json_of(r: &Resp) -> Value {
        match &r.body {
            Body::Bytes(b) => serde_json::from_slice(b).unwrap_or(Value::Null),
            _ => Value::Null,
        }
    }

    fn login(app: &std::sync::Arc<crate::app::App>, name: &str, pw: &str) -> (String, String) {
        let r = handle(app, req("POST", "/api/v1/login", &format!(r#"{{"name":"{name}","password":"{pw}"}}"#)));
        assert_eq!(r.status, 200);
        let c = r.headers.iter().find(|(k, _)| *k == "set-cookie").unwrap().1.split(';').next().unwrap().split_once('=').unwrap().1.to_string();
        (c, json_of(&r)["csrf"].as_str().unwrap().to_string())
    }

    #[test]
    fn admins_list_add_remove() {
        let app = new_app();
        let (c, csrf) = admin_session(&app);
        let call = |m: &str, p: &str, b: &str| handle(&app, with_session(req(m, p, b), &c, &csrf));
        let list = json_of(&call("GET", "/api/v1/admins", ""));
        assert_eq!(list[0]["name"], "admin");
        assert!(!serde_json::to_string(&list).unwrap().contains("hash"));
        assert_eq!(call("POST", "/api/v1/admins", r#"{"name":"viewer","password":"short","role":"viewer"}"#).status, 400);
        assert_eq!(call("POST", "/api/v1/admins", r#"{"name":"viewer","password":"a long viewer password","role":"root"}"#).status, 400);
        assert_eq!(call("POST", "/api/v1/admins", r#"{"name":"viewer","password":"a long viewer password","role":"viewer"}"#).status, 200);
        assert_eq!(call("POST", "/api/v1/admins", r#"{"name":"viewer","password":"a long viewer password","role":"viewer"}"#).status, 409);
        assert_eq!(call("DELETE", "/api/v1/admins?name=admin", "").status, 409, "not yourself");

        // viewers can't manage users, and the last admin stays
        let (vc, vcsrf) = login(&app, "viewer", "a long viewer password");
        let v = |m: &str, p: &str, b: &str| handle(&app, with_session(req(m, p, b), &vc, &vcsrf)).status;
        assert_eq!(v("POST", "/api/v1/admins", r#"{"name":"evil","password":"a long evil password","role":"admin"}"#), 403);
        assert_eq!(v("DELETE", "/api/v1/admins?name=admin", ""), 403);
        assert_eq!(v("GET", "/api/v1/admins", ""), 200);

        assert_eq!(call("POST", "/api/v1/admins", r#"{"name":"second","password":"a long second password","role":"admin"}"#).status, 200);
        let (sc, scsrf) = login(&app, "second", "a long second password");
        assert_eq!(handle(&app, with_session(req("DELETE", "/api/v1/admins?name=admin", ""), &sc, &scsrf)).status, 200);
        assert_eq!(call("GET", "/api/v1/admins", "").status, 401, "removed user's sessions end");
        assert_eq!(handle(&app, with_session(req("DELETE", "/api/v1/admins?name=viewer", ""), &sc, &scsrf)).status, 200);
        assert_eq!(handle(&app, with_session(req("DELETE", "/api/v1/admins?name=nobody", ""), &sc, &scsrf)).status, 404);
    }

    #[test]
    fn last_admin_cannot_be_removed() {
        use crate::app::lock;
        use crate::store::Role;
        let app = new_app();
        admin_session(&app);
        lock(&app.store).state.admins.push(crate::auth::new_admin("second", "a long second password", Role::Admin).unwrap());
        let (sc, scsrf) = login(&app, "second", "a long second password");
        // "second" is demoted while its session is still open: "admin" is now the only admin
        lock(&app.store).state.admins.iter_mut().find(|a| a.name == "second").unwrap().role = Role::Viewer;
        assert_eq!(handle(&app, with_session(req("DELETE", "/api/v1/admins?name=admin", ""), &sc, &scsrf)).status, 409);
        assert_eq!(lock(&app.store).state.admins.len(), 2);
    }

    #[test]
    fn password_change_needs_the_current_one_and_ends_sessions() {
        let app = new_app();
        let (c, csrf) = admin_session(&app);
        let (c2, csrf2) = login(&app, "admin", "a long admin password");
        let r = handle(&app, with_session(req("POST", "/api/v1/admins/password", r#"{"current":"wrong password!!","new":"a brand new password"}"#), &c, &csrf));
        assert_eq!(r.status, 403);
        let r = handle(&app, with_session(req("POST", "/api/v1/admins/password", r#"{"current":"a long admin password","new":"short"}"#), &c, &csrf));
        assert_eq!(r.status, 400);
        let r = handle(&app, with_session(req("POST", "/api/v1/admins/password", r#"{"current":"a long admin password","new":"a brand new password"}"#), &c, &csrf));
        assert_eq!(r.status, 200);
        assert_eq!(handle(&app, with_session(req("GET", "/api/v1/me", ""), &c2, &csrf2)).status, 401);
        assert_eq!(handle(&app, with_session(req("GET", "/api/v1/me", ""), &c, &csrf)).status, 401);
        login(&app, "admin", "a brand new password");
    }
}
