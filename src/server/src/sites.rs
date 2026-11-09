//! Several routers ("sites") in one tikontrol. The first router stays in `state.router` (so a
//! single-router state file needs no migration) and gets id ""; extra routers live in `state.sites`
//! with random ids. Each runs in its own App (app.rs): own snapshot, live data, history and pending
//! changes. Requests pick one with the `x-tik-site` header; the router itself is then connected with
//! the usual `POST /api/v1/router` (reader/writer credentials sealed with per-site labels).
//! Listing shows no secrets; adding and removing sites needs the admin role.

use crate::api::body;
use crate::app::{lock, now, App};
use crate::auth::Session;
use crate::http::{Req, Resp};
use crate::live::Ev;
use crate::store::{hex, random_bytes, Role, Site};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub const MAX_SITES: usize = 20;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewSite {
    name: String,
}

fn valid_name(n: &str) -> bool {
    let n = n.trim();
    !n.is_empty() && n.chars().count() <= 40 && !n.chars().any(char::is_control)
}

fn entry(a: &App, name: &str) -> Value {
    let cfg = a.router_cfg();
    let snap = a.snap.read().unwrap_or_else(|e| e.into_inner());
    json!({
        "id": a.site, "name": name,
        "identity": snap.data.as_deref().and_then(|d| d["identity"].as_str().map(str::to_string)),
        "host": cfg.as_ref().map(|r| r.host.clone()), "mode": cfg.as_ref().map(|r| r.mode.clone()),
        "error": snap.error,
    })
}

pub fn route(app: &Arc<App>, req: &Req, s: &Session) -> Resp {
    let audit = |text: String| lock(&app.live.history).event(Ev { t: now(), cat: "Admin".into(), level: "warning".into(), text: format!("{}: {text}", s.user), mac: None });
    match req.method.as_str() {
        "GET" => {
            let names: Vec<(String, String)> = lock(&app.store).state.sites.iter().map(|x| (x.id.clone(), x.name.clone())).collect();
            let mut list = vec![entry(app, "Main")];
            let apps = lock(&app.sites).clone();
            list.extend(names.iter().filter_map(|(id, n)| apps.get(id).map(|a| entry(a, n))));
            Resp::json(200, &json!(list))
        }
        _ if s.role != Role::Admin => Resp::error(403, "admin role required"),
        "POST" => {
            let b: NewSite = match body(req) {
                Ok(b) => b,
                Err(r) => return r,
            };
            if !valid_name(&b.name) {
                return Resp::error(400, "name: 1-40 characters");
            }
            let id = hex(&random_bytes::<6>());
            let mut st = lock(&app.store);
            if st.state.sites.len() >= MAX_SITES {
                return Resp::error(409, "too many sites");
            }
            st.state.sites.push(Site { id: id.clone(), name: b.name.trim().into(), router: None });
            if st.save().is_err() {
                st.state.sites.pop();
                return Resp::error(500, "could not save state");
            }
            drop(st);
            let a = Arc::new(app.for_site(&id));
            a.spawn_workers();
            lock(&app.sites).insert(id.clone(), a);
            audit(format!("added site {}", b.name.trim()));
            Resp::json(200, &json!({ "id": id }))
        }
        "DELETE" => {
            let id = req.param("id").unwrap_or_default();
            let mut st = lock(&app.store);
            let Some(pos) = st.state.sites.iter().position(|x| x.id == id) else { return Resp::error(404, "no such site (the main router can't be removed here)") };
            let gone = st.state.sites.remove(pos);
            if st.save().is_err() {
                st.state.sites.insert(pos, gone);
                return Resp::error(500, "could not save state");
            }
            drop(st);
            app.drop_site_app(&id);
            audit(format!("removed site {} and its stored credentials", gone.name));
            Resp::json(200, &json!({ "ok": true }))
        }
        _ => Resp::error(405, "use GET, POST or DELETE"),
    }
}

#[cfg(test)]
mod tests {
    use crate::api::handle;
    use crate::api::tests::{admin_session, new_app, req, with_session};
    use crate::app::lock;
    use crate::http::Body;
    use crate::store::{Cred, Mode, Store};
    use serde_json::Value;

    fn json_of(r: &crate::http::Resp) -> Value {
        match &r.body {
            Body::Bytes(b) => serde_json::from_slice(b).unwrap_or(Value::Null),
            _ => Value::Null,
        }
    }

    fn on(mut r: crate::http::Req, site: &str) -> crate::http::Req {
        r.headers.push(("x-tik-site".into(), site.into()));
        r
    }

    #[test]
    fn single_router_state_files_load_unchanged() {
        // a state.json from before sites existed: no "sites" key
        let d = crate::store::tests::tmpdir("migrate");
        let old = r#"{"claimed":true,"admins":[],"router":{"host":"192.0.2.1","port":8729,"pin":"00","mode":"read-only","reader":{"user":"tik-read","password":"x"},"writer":null}}"#;
        std::fs::write(d.join("state.json"), old).unwrap();
        let st = Store::open(&d).unwrap();
        assert!(st.state.sites.is_empty());
        assert_eq!(st.state.router.as_ref().unwrap().host, "192.0.2.1");
        st.save().unwrap();
        let again = Store::open(&d).unwrap();
        assert_eq!(again.state.router.unwrap().reader.user, "tik-read");
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn sites_are_scoped_sealed_and_admin_only() {
        let app = new_app();
        let (c, csrf) = admin_session(&app);
        let call = |r| handle(&app, with_session(r, &c, &csrf));
        assert_eq!(call(req("POST", "/api/v1/sites", r#"{"name":""}"#)).status, 400);
        let r = call(req("POST", "/api/v1/sites", r#"{"name":"Office"}"#));
        assert_eq!(r.status, 200);
        let id = json_of(&r)["id"].as_str().unwrap().to_string();
        let list = json_of(&call(req("GET", "/api/v1/sites", "")));
        assert_eq!(list.as_array().unwrap().len(), 2);
        assert_eq!(list[1]["name"], "Office");

        // unknown site: 404; without a session: 401 (sites resolve only after auth)
        assert_eq!(call(on(req("GET", "/api/v1/me", ""), "nope")).status, 404);
        assert_eq!(handle(&app, on(req("GET", "/api/v1/me", ""), &id)).status, 401);

        // connect the site's router: credentials are sealed with the site's own label
        let pin = "a".repeat(64);
        let body = format!(r#"{{"host":"192.0.2.2","pin":"{pin}","reader":{{"user":"tik-read","password":"router-pass"}}}}"#);
        assert_eq!(call(on(req("POST", "/api/v1/router", &body), &id)).status, 200);
        {
            let st = lock(&app.store);
            assert!(st.state.router.is_none(), "the main router is untouched");
            let cfg = st.state.sites[0].router.as_ref().unwrap();
            assert_eq!(cfg.host, "192.0.2.2");
            assert!(!serde_json::to_string(&st.state).unwrap().contains("router-pass"));
            assert_eq!(st.sealer.open_str(&format!("router-password:{id}:reader"), &cfg.reader.password).as_deref(), Some("router-pass"));
            assert_eq!(st.sealer.open_str("router-password:reader", &cfg.reader.password), None, "can't be moved to the main router");
        }
        let me = json_of(&call(on(req("GET", "/api/v1/me", ""), &id)));
        assert_eq!(me["site"], id.as_str());
        assert_eq!(me["router"]["host"], "192.0.2.2");
        assert_eq!(json_of(&call(req("GET", "/api/v1/me", "")))["router"], Value::Null);

        // live data and history are per site
        let site = lock(&app.sites).get(&id).cloned().unwrap();
        site.poll(false);
        assert_eq!(site.snap.read().unwrap().data.as_ref().unwrap()["identity"], "TestRouter");
        assert!(app.snap.read().unwrap().data.is_none());
        assert_eq!(call(on(req("GET", "/api/v1/snapshot", ""), &id)).status, 200);
        assert_eq!(call(req("GET", "/api/v1/snapshot", "")).status, 503);
        assert!(site.history_path().ends_with(format!("history-{id}.json")));

        // a change previewed on one router can't be applied on another
        let dns = r#"{"intent":{"kind":"dns-record","name":"nas.lan","address":"192.168.88.4"}}"#;
        lock(&app.store).state.sites[0].router.as_mut().unwrap().mode = Mode::ReadWrite;
        assert_eq!(json_of(&call(on(req("POST", "/api/v1/apply/preview", dns), &id)))["canApply"], false, "no stored writer on this site");
        {
            let mut st = lock(&app.store);
            let w = Cred { user: "tik-write".into(), password: st.sealer.seal(&format!("router-password:{id}:writer"), "router-pass") };
            st.state.sites[0].router.as_mut().unwrap().writer = Some(w.clone());
            let mut main = st.state.sites[0].router.clone().unwrap();
            main.writer = Some(Cred { password: st.sealer.seal("router-password:writer", "router-pass"), ..w });
            main.reader.password = st.sealer.seal("router-password:reader", "router-pass");
            st.state.router = Some(main);
        }
        let p = call(on(req("POST", "/api/v1/apply/preview", dns), &id));
        assert_eq!(json_of(&p)["canApply"], true);
        let start = format!(r#"{{"id":"{}"}}"#, json_of(&p)["id"].as_str().unwrap());
        assert_eq!(call(req("POST", "/api/v1/apply/start", &start)).status, 404, "not on the main router");
        assert_ne!(call(on(req("POST", "/api/v1/apply/start", &start), &id)).status, 404);

        // removing the site drops its credentials and its App
        assert_eq!(call(req("DELETE", &format!("/api/v1/sites?id={id}"), "")).status, 200);
        assert!(lock(&app.store).state.sites.is_empty());
        assert!(site.gone());
        assert_eq!(call(on(req("GET", "/api/v1/me", ""), &id)).status, 404);
        assert_eq!(call(req("DELETE", "/api/v1/sites?id=", "")).status, 404);
    }

    #[test]
    fn viewers_can_list_but_not_change_sites() {
        let app = new_app();
        let (c, csrf) = admin_session(&app);
        handle(&app, with_session(req("POST", "/api/v1/admins", r#"{"name":"viewer","password":"a long viewer password","role":"viewer"}"#), &c, &csrf));
        let r = handle(&app, req("POST", "/api/v1/login", r#"{"name":"viewer","password":"a long viewer password"}"#));
        let vc = r.headers.iter().find(|(k, _)| *k == "set-cookie").unwrap().1.split(';').next().unwrap().split_once('=').unwrap().1.to_string();
        let vcsrf = json_of(&r)["csrf"].as_str().unwrap().to_string();
        assert_eq!(handle(&app, with_session(req("GET", "/api/v1/sites", ""), &vc, &vcsrf)).status, 200);
        assert_eq!(handle(&app, with_session(req("POST", "/api/v1/sites", r#"{"name":"x"}"#), &vc, &vcsrf)).status, 403);
    }
}
