//! Local API keys for scripts and Home Assistant. A key gives read-only (viewer) access to GET routes
//! under /api/v1 with `Authorization: Bearer tikapi_…`; it can never change anything. Only the
//! SHA-256 is stored and the key is shown once. Managing keys needs the admin role. Wrong keys count
//! towards the login rate limiter.

use crate::api::{body, saved, token_hash};
use crate::app::{lock, now, App};
use crate::auth::{self, Session};
use crate::http::{Req, Resp};
use crate::live::Ev;
use crate::store::{ct_eq, hex, random_bytes, Role};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

pub const MAX_KEYS: usize = 20;
pub const PREFIX: &str = "tikapi_";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ApiKey {
    pub name: String,
    /// SHA-256 of the key
    pub hash: String,
    pub created: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewKey {
    name: String,
}

/// A viewer session for a request that carries an API key; None when it carries none.
pub fn authenticate(app: &App, req: &Req) -> Option<Result<Session, Resp>> {
    let tok = req.header("authorization")?.strip_prefix("Bearer ")?.trim();
    if !tok.starts_with(PREFIX) {
        return None;
    }
    if req.method != "GET" {
        return Some(Err(Resp::error(403, "API keys are read-only")));
    }
    let ip = req.peer.unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));
    if let Err(wait) = lock(&app.limiter).check(ip) {
        return Some(Err(Resp::error(429, "too many failed attempts, try again later").with("retry-after", wait.to_string())));
    }
    let h = token_hash(tok);
    let name = lock(&app.store).state.api_keys.iter().find(|k| ct_eq(k.hash.as_bytes(), h.as_bytes())).map(|k| k.name.clone());
    Some(match name {
        Some(n) => Ok(auth::api_session(&n)),
        None => {
            lock(&app.limiter).fail(ip);
            Err(Resp::error(401, "unknown API key").with("www-authenticate", "Bearer".into()))
        }
    })
}

pub fn route(app: &Arc<App>, req: &Req, s: &Session) -> Option<Resp> {
    if req.path != "/api/v1/apikeys" {
        return None;
    }
    if s.role != Role::Admin {
        return Some(Resp::error(403, "admin role required"));
    }
    let audit = |text: String| lock(&app.live.history).event(Ev { t: now(), cat: "Admin".into(), level: "warning".into(), text: format!("{}: {text}", s.user), mac: None });
    Some(match req.method.as_str() {
        "GET" => {
            let st = lock(&app.store);
            Resp::json(200, &json!(st.state.api_keys.iter().map(|k| json!({ "name": k.name, "created": k.created })).collect::<Vec<_>>()))
        }
        "POST" => {
            let b: NewKey = match body(req) {
                Ok(b) => b,
                Err(r) => return Some(r),
            };
            if !auth::valid_username(&b.name) {
                return Some(Resp::error(400, "name: 1-32 letters, digits, '-', '_' or '.'"));
            }
            let mut st = lock(&app.store);
            if st.state.api_keys.iter().any(|k| k.name == b.name) {
                return Some(Resp::error(409, "a key with that name exists"));
            }
            if st.state.api_keys.len() >= MAX_KEYS {
                return Some(Resp::error(409, "too many API keys"));
            }
            let key = format!("{PREFIX}{}", hex(&random_bytes::<32>()));
            st.state.api_keys.push(ApiKey { name: b.name.clone(), hash: token_hash(&key), created: now() });
            if st.save().is_err() {
                st.state.api_keys.pop();
                return Some(Resp::error(500, "could not save state"));
            }
            drop(st);
            audit(format!("created API key {}", b.name));
            Resp::json(200, &json!({ "name": b.name, "key": key }))
        }
        "DELETE" => {
            let name = req.param("name").unwrap_or_default();
            let mut st = lock(&app.store);
            let n = st.state.api_keys.len();
            st.state.api_keys.retain(|k| k.name != name);
            if st.state.api_keys.len() == n {
                return Some(Resp::error(404, "no such key"));
            }
            let r = st.save();
            drop(st);
            audit(format!("revoked API key {name}"));
            saved(r)
        }
        _ => Resp::error(405, "use GET, POST or DELETE"),
    })
}

#[cfg(test)]
mod tests {
    use crate::api::handle;
    use crate::api::tests::{admin_session, new_app, req, with_session};
    use crate::app::lock;

    #[test]
    fn keys_are_hashed_shown_once_and_read_only() {
        let app = new_app();
        let (c, csrf) = admin_session(&app);
        let r = handle(&app, with_session(req("POST", "/api/v1/apikeys", r#"{"name":"ha"}"#), &c, &csrf));
        let v: serde_json::Value = match &r.body { crate::http::Body::Bytes(b) => serde_json::from_slice(b).unwrap(), _ => panic!() };
        let key = v["key"].as_str().unwrap().to_string();
        assert!(key.starts_with("tikapi_"));
        assert!(!serde_json::to_string(&lock(&app.store).state).unwrap().contains(&key));
        let bearer = |m: &str, p: &str, k: &str| {
            let mut r = req(m, p, "");
            r.headers.push(("authorization".into(), format!("Bearer {k}")));
            handle(&app, r).status
        };
        assert_eq!(bearer("GET", "/api/v1/me", &key), 200);
        assert_eq!(bearer("GET", "/api/v1/apikeys", &key), 403, "viewer can't list keys");
        assert_eq!(bearer("POST", "/api/v1/logout", &key), 401, "no writes with a key");
        assert_eq!(bearer("GET", "/api/v1/me", "tikapi_wrong"), 401);
        assert_eq!(handle(&app, with_session(req("DELETE", "/api/v1/apikeys?name=ha", ""), &c, &csrf)).status, 200);
        assert_eq!(bearer("GET", "/api/v1/me", &key), 401, "revoked");
    }
}
