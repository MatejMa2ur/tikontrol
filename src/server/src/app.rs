//! Shared app state and the background poller that keeps the router snapshot fresh.

use crate::auth::{Limiter, Sessions};
use crate::live::Live;
use crate::policy::{self, Policies};
use crate::ros::conn::{Conn, Router, Target};
use crate::ros::queries;
use crate::store::{Mode, RouterCfg, Store};
use serde_json::Value;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Poisoned locks only mean another thread panicked mid-update; the data is still usable.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Default, Clone)]
pub struct Snap {
    pub data: Option<Arc<Value>>,
    pub at: u64,
    pub error: Option<String>,
    pub policies: Option<Policies>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum As {
    Reader,
    Writer,
}

pub type Connector = Box<dyn Fn(&RouterCfg, &str, &str) -> Result<Box<dyn Router + Send>, String> + Send + Sync>;

pub struct App {
    pub store: Mutex<Store>,
    pub sessions: Mutex<Sessions>,
    pub limiter: Mutex<Limiter>,
    /// one-time token printed to the log at first start; needed to create the first admin
    pub setup_token: Mutex<Option<String>>,
    pub snap: RwLock<Snap>,
    pub connector: Connector,
    pub tls: bool,
    pub live: Live,
    /// TIK_DEMO: serve this snapshot instead of a router (trying tikontrol out, UI tests)
    pub demo: Option<Arc<Value>>,
    /// MCP proposals and token rate limit (src/server/src/mcp.rs)
    pub mcp: crate::mcp::Mcp,
    reader: Mutex<Option<Box<dyn Router + Send>>>,
}

pub fn real_connector() -> Connector {
    Box::new(|cfg, user, pw| {
        let t = Target { host: cfg.host.clone(), port: cfg.port, pin: cfg.pin.clone() };
        Ok(Box::new(Conn::connect(&t, user, pw)?) as Box<dyn Router + Send>)
    })
}

impl App {
    pub fn new(store: Store, connector: Connector, tls: bool) -> App {
        let setup = (store.state.admins.is_empty() && !store.state.claimed).then(|| crate::store::hex(&crate::store::random_bytes::<16>()));
        let live = Live::load(&store.dir().join("history.json"));
        App {
            live,
            demo: None,
            mcp: Default::default(),
            store: Mutex::new(store),
            sessions: Mutex::default(),
            limiter: Mutex::default(),
            setup_token: Mutex::new(setup),
            snap: RwLock::default(),
            connector,
            tls,
            reader: Mutex::new(None),
        }
    }

    /// First admin signed in: the setup token and any setup sessions stop working, for good.
    pub fn claim(&self) -> std::io::Result<()> {
        *lock(&self.setup_token) = None;
        lock(&self.sessions).remove_user(crate::auth::SETUP_USER);
        let mut st = lock(&self.store);
        if st.state.claimed {
            return Ok(());
        }
        st.state.claimed = true;
        st.save()
    }

    pub fn router_cfg(&self) -> Option<RouterCfg> {
        lock(&self.store).state.router.clone()
    }

    /// Opens a connection as the reader or writer. The password is decrypted only for the login.
    pub fn open(&self, who: As) -> Result<Box<dyn Router + Send>, String> {
        let st = lock(&self.store);
        let cfg = st.state.router.clone().ok_or("no router configured")?;
        let (cred, label) = match who {
            As::Reader => (Some(cfg.reader.clone()), "router-password:reader"),
            As::Writer => (cfg.writer.clone(), "router-password:writer"),
        };
        let cred = cred.ok_or("read-only mode: no write credential is stored")?;
        let pw = st.sealer.open_str(label, &cred.password).ok_or("stored router password cannot be decrypted (wrong key file?)")?;
        drop(st);
        (self.connector)(&cfg, &cred.user, &pw)
    }

    /// Opens a connection with credentials the user just typed (MikroTik-account login, their own writes).
    pub fn open_as(&self, user: &str, pw: &str) -> Result<Box<dyn Router + Send>, String> {
        let cfg = self.router_cfg().ok_or("no router configured")?;
        (self.connector)(&cfg, user, pw)
    }

    /// Runs `f` on the shared reader connection, reconnecting once if it dropped.
    pub fn with_reader<T>(&self, f: impl Fn(&mut dyn Router) -> Result<T, String>) -> Result<T, String> {
        let mut g = lock(&self.reader);
        if g.is_none() {
            *g = Some(self.open(As::Reader)?);
        }
        match f(g.as_mut().unwrap().as_mut()) {
            Ok(v) => Ok(v),
            Err(e) => {
                eprintln!("tikontrol: reader connection failed, reconnecting: {e}");
                *g = Some(self.open(As::Reader)?);
                f(g.as_mut().unwrap().as_mut())
            }
        }
    }

    pub fn drop_reader(&self) {
        *lock(&self.reader) = None;
    }

    /// One poll: check the reader's real policies, then read the snapshot.
    pub fn poll(&self, with_latency: bool) {
        if let Some(d) = &self.demo {
            let p = Policies { user: "demo".into(), group: "demo".into(), policy: policy::READER.iter().map(|s| s.to_string()).collect() };
            *self.snap.write().unwrap_or_else(|e| e.into_inner()) = Snap { data: Some(d.clone()), at: now(), error: None, policies: Some(p) };
            return;
        }
        let Some(cfg) = self.router_cfg() else { return };
        let result = self.with_reader(|r| {
            let p = policy::detect(r, &cfg.reader.user)?;
            if cfg.mode == Mode::ReadOnly && !p.read_only_violations().is_empty() {
                return Err(format!(
                    "read-only mode refused: the router user '{}' has {} — use a group without them (see setup commands)",
                    p.user,
                    p.read_only_violations().join(", ")
                ));
            }
            Ok((p, queries::snapshot(r, with_latency)))
        });
        let mut s = self.snap.write().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok((p, data)) => {
                if let Some(lat) = data.get("latency").and_then(Value::as_object) {
                    let worst = lat.values().filter_map(|v| v["ms"].as_f64()).fold(0.0, f64::max);
                    *lock(&self.live.latency_ms) = worst;
                }
                *s = Snap { data: Some(Arc::new(data)), at: now(), error: None, policies: Some(p) };
            }
            Err(e) => {
                s.error = Some(e);
                if s.error.as_deref().is_some_and(|e| e.starts_with("read-only mode refused")) {
                    s.data = None; // never show data read through an over-privileged user
                }
                drop(s);
                self.drop_reader();
            }
        }
    }

    pub fn spawn_poller(self: &Arc<Self>, every: Duration) {
        let app = self.clone();
        std::thread::spawn(move || {
            let mut n = 0u64;
            loop {
                app.poll(n.is_multiple_of(4)); // latency every 4th poll: pings cost router CPU and take seconds
                n += 1;
                std::thread::sleep(every);
            }
        });
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::ros::queries::tests::{row, Fake};
    use crate::store::{tests::tmpdir, Cred};

    pub fn fake_connector(policy: &'static str) -> Connector {
        Box::new(move |_, user, pw| {
            if pw != "router-pass" {
                return Err("router rejected the login".into());
            }
            let mut f = Fake::default();
            f.answers.insert("/user/print".into(), Ok(vec![row(&[("name", user), ("group", "g")])]));
            f.answers.insert("/user/group/print".into(), Ok(vec![row(&[("name", "g"), ("policy", policy)])]));
            f.answers.insert("/system/identity/print".into(), Ok(vec![row(&[("name", "TestRouter")])]));
            f.answers.insert("/system/clock/print".into(), Ok(vec![row(&[("date", "2026-01-15"), ("time", "10:00:00")])]));
            Ok(Box::new(f) as Box<dyn Router + Send>)
        })
    }

    pub fn app_with_router(policy: &'static str, mode: Mode) -> App {
        let mut store = Store::open(&tmpdir("app")).unwrap();
        let pw = store.sealer.seal("router-password:reader", "router-pass");
        store.state.router = Some(RouterCfg { host: "192.0.2.1".into(), port: 8729, pin: "00".into(), mode, reader: Cred { user: "tik-read".into(), password: pw }, writer: None });
        App::new(store, fake_connector(policy), true)
    }

    #[test]
    fn poll_reads_the_snapshot_with_a_safe_reader() {
        let app = app_with_router("read,api,test", Mode::ReadOnly);
        app.poll(false);
        let s = app.snap.read().unwrap();
        assert_eq!(s.error, None);
        assert_eq!(s.data.as_ref().unwrap()["identity"], "TestRouter");
    }

    #[test]
    fn poll_refuses_an_over_privileged_reader_in_read_only_mode() {
        let app = app_with_router("read,write,api", Mode::ReadOnly);
        app.poll(false);
        let s = app.snap.read().unwrap();
        assert!(s.error.as_ref().unwrap().contains("read-only mode refused"));
        assert!(s.data.is_none());
    }

    #[test]
    fn read_only_mode_has_no_writer() {
        let app = app_with_router("read,api", Mode::ReadOnly);
        assert!(app.open(As::Writer).err().unwrap().contains("read-only mode"));
    }
}
