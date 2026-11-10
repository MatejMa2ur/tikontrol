//! Shared app state and the background poller that keeps the router snapshot fresh.

use crate::auth::{Limiter, Sessions};
use crate::live::Live;
use crate::policy::{self, Policies};
use crate::ros::conn::{Conn, Router, Target};
use crate::ros::queries;
use crate::store::{Mode, RouterCfg, State, Store};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
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

pub type Connector = Arc<dyn Fn(&RouterCfg, &str, &str) -> Result<Box<dyn Router + Send>, String> + Send + Sync>;

/// One App per router ("site"). The main App serves the first router (`state.router`) and holds the
/// others in `sites`; they share the store, sessions and limiter, and each has its own snapshot, live
/// data, history file, pending changes and reader connection.
pub struct App {
    pub store: Arc<Mutex<Store>>,
    pub sessions: Arc<Mutex<Sessions>>,
    pub limiter: Arc<Mutex<Limiter>>,
    /// one-time token printed to the log at first start; needed to create the first admin
    pub setup_token: Arc<Mutex<Option<String>>>,
    /// "" for the main router, else the id of an entry in `state.sites`
    pub site: String,
    /// the other routers' Apps (only on the main App)
    pub sites: Mutex<BTreeMap<String, Arc<App>>>,
    /// set when a site is removed: its threads stop
    gone: AtomicBool,
    /// time between snapshots (TIK_POLL)
    pub every: Duration,
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
    Arc::new(|cfg, user, pw| {
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
            store: Arc::new(Mutex::new(store)),
            sessions: Arc::default(),
            limiter: Arc::default(),
            setup_token: Arc::new(Mutex::new(setup)),
            site: String::new(),
            sites: Mutex::default(),
            gone: AtomicBool::new(false),
            every: Duration::from_secs(30),
            snap: RwLock::default(),
            connector,
            tls,
            reader: Mutex::new(None),
        }
    }

    /// An App for the extra router `id`, sharing this (main) App's store, sessions and limiter.
    pub fn for_site(&self, id: &str) -> App {
        let path = lock(&self.store).dir().join(format!("history-{id}.json"));
        App {
            live: Live::load(&path),
            demo: None,
            mcp: Default::default(),
            store: self.store.clone(),
            sessions: self.sessions.clone(),
            limiter: self.limiter.clone(),
            setup_token: self.setup_token.clone(),
            site: id.into(),
            sites: Mutex::default(),
            gone: AtomicBool::new(false),
            every: self.every,
            snap: RwLock::default(),
            connector: self.connector.clone(),
            tls: self.tls,
            reader: Mutex::new(None),
        }
    }

    /// Where this router's history is kept.
    pub fn history_path(&self) -> std::path::PathBuf {
        let dir = lock(&self.store).dir();
        if self.site.is_empty() { dir.join("history.json") } else { dir.join(format!("history-{}.json", self.site)) }
    }

    /// Starts the background work for this router: snapshot poller, live sampler, alerts.
    pub fn spawn_workers(self: &Arc<Self>) {
        self.spawn_poller(self.every);
        crate::live::spawn(self.clone(), self.history_path());
        crate::alerts::spawn(self.clone(), Duration::from_secs(30));
    }

    /// Main App only: starts an App (and its threads) for every extra router in the store.
    pub fn start_sites(self: &Arc<Self>) {
        let ids: Vec<String> = lock(&self.store).state.sites.iter().map(|s| s.id.clone()).collect();
        for id in ids {
            let a = Arc::new(self.for_site(&id));
            a.spawn_workers();
            lock(&self.sites).insert(id, a);
        }
    }

    /// Main App only: stops a removed router's threads and deletes its history.
    pub fn drop_site_app(&self, id: &str) {
        if let Some(a) = lock(&self.sites).remove(id) {
            a.gone.store(true, Ordering::Relaxed);
            a.drop_reader();
            let _ = std::fs::remove_file(a.history_path());
            let _ = std::fs::remove_file(lock(&self.store).dir().join(format!("blueprint-{id}.json")));
        }
    }

    pub fn gone(&self) -> bool {
        self.gone.load(Ordering::Relaxed)
    }

    /// This App's router in the state (None: not connected yet, or the site was removed).
    pub fn cfg<'a>(&self, s: &'a State) -> Option<&'a RouterCfg> {
        if self.site.is_empty() {
            return s.router.as_ref();
        }
        s.sites.iter().find(|x| x.id == self.site)?.router.as_ref()
    }

    /// The slot this App's router lives in; None when the site was removed.
    pub fn cfg_slot<'a>(&self, s: &'a mut State) -> Option<&'a mut Option<RouterCfg>> {
        if self.site.is_empty() {
            return Some(&mut s.router);
        }
        s.sites.iter_mut().find(|x| x.id == self.site).map(|x| &mut x.router)
    }

    /// Sealing label for this router's `role` ("reader" / "writer") password.
    pub fn label(&self, role: &str) -> String {
        if self.site.is_empty() { format!("router-password:{role}") } else { format!("router-password:{}:{role}", self.site) }
    }

    /// The signed-in MikroTik account's password, used only against the main router: it is never
    /// sent to another router.
    pub fn ros<'a>(&self, s: &'a crate::auth::Session) -> Option<&'a crate::auth::Secret> {
        s.ros.as_ref().filter(|_| self.site.is_empty())
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
        let st = lock(&self.store);
        self.cfg(&st.state).cloned()
    }

    /// Opens a connection as the reader or writer. The password is decrypted only for the login.
    pub fn open(&self, who: As) -> Result<Box<dyn Router + Send>, String> {
        let cfg = self.router_cfg().ok_or("no router configured")?;
        let prefix = self.label("");
        self.open_cfg(&cfg, prefix.trim_end_matches(':'), who)
    }

    /// Same for any stored connection (the router, or the CMR controller with label "controller-password").
    pub fn open_cfg(&self, cfg: &RouterCfg, label: &str, who: As) -> Result<Box<dyn Router + Send>, String> {
        let (cred, role) = match who {
            As::Reader => (Some(cfg.reader.clone()), "reader"),
            As::Writer => (cfg.writer.clone(), "writer"),
        };
        let cred = cred.ok_or("read-only mode: no write credential is stored")?;
        let pw = lock(&self.store).sealer.open_str(&format!("{label}:{role}"), &cred.password).ok_or("stored router password cannot be decrypted (wrong key file?)")?;
        (self.connector)(cfg, &cred.user, &pw)
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
            Ok((p, mut data)) => {
                // latency is measured only every few polls: keep the last measurement in between
                if data.get("latency").is_none() {
                    if let (Some(lat), Some(o)) = (s.data.as_ref().and_then(|d| d.get("latency")), data.as_object_mut()) {
                        o.insert("latency".into(), lat.clone());
                    }
                }
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

    /// Poll less while the router is busy, so monitoring never adds load to a struggling router:
    /// x1 normally, x2 at 70% CPU or more, x4 at 90% or more (from the last snapshot).
    pub fn backoff(&self) -> u32 {
        let s = self.snap.read().unwrap_or_else(|e| e.into_inner());
        let cpu = s.data.as_deref().and_then(|d| d["resource"]["cpu-load"].as_str().and_then(|c| c.parse::<u32>().ok()).or_else(|| d["resource"]["cpu-load"].as_u64().map(|c| c as u32)));
        backoff_for(cpu)
    }

    pub fn spawn_poller(self: &Arc<Self>, every: Duration) {
        let app = self.clone();
        std::thread::spawn(move || {
            let mut n = 0u64;
            while !app.gone() {
                app.poll(n.is_multiple_of(4)); // latency every 4th poll: pings cost router CPU and take seconds
                n += 1;
                std::thread::sleep(every * app.backoff());
            }
        });
    }
}

pub fn backoff_for(cpu: Option<u32>) -> u32 {
    match cpu {
        Some(c) if c >= 90 => 4,
        Some(c) if c >= 70 => 2,
        _ => 1,
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::ros::queries::tests::{row, Fake};
    use crate::store::{tests::tmpdir, Cred};

    pub fn fake_router(user: &str, policy: &str) -> Fake {
        let mut f = Fake::default();
        f.answers.insert("/user/print".into(), Ok(vec![row(&[("name", user), ("group", "g")])]));
        f.answers.insert("/user/group/print".into(), Ok(vec![row(&[("name", "g"), ("policy", policy)])]));
        f.answers.insert("/user/active/print".into(), Ok(vec![row(&[("address", "192.0.2.50")])]));
        f.answers.insert("/system/identity/print".into(), Ok(vec![row(&[("name", "TestRouter")])]));
        f.answers.insert("/system/clock/print".into(), Ok(vec![row(&[("date", "2026-01-15"), ("time", "10:00:00")])]));
        f
    }

    pub fn fake_connector(policy: &'static str) -> Connector {
        Arc::new(move |_, user, pw| {
            if pw != "router-pass" {
                return Err("router rejected the login".into());
            }
            Ok(Box::new(fake_router(user, policy)) as Box<dyn Router + Send>)
        })
    }

    pub fn app_with_router(policy: &'static str, mode: Mode) -> App {
        let mut store = Store::open(&tmpdir("app")).unwrap();
        let pw = store.sealer.seal("router-password:reader", "router-pass");
        store.state.router = Some(RouterCfg { host: "192.0.2.1".into(), port: 8729, pin: "00".into(), mode, reader: Cred { user: "tik-read".into(), password: pw }, writer: None });
        App::new(store, fake_connector(policy), true)
    }

    #[test]
    fn polling_backs_off_when_the_router_is_busy() {
        assert_eq!([None, Some(10), Some(70), Some(95)].map(backoff_for), [1, 1, 2, 4]);
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
    fn polls_without_pings_keep_the_last_latency() {
        let app = app_with_router("read,api,test", Mode::ReadOnly);
        app.poll(true);
        assert!(app.snap.read().unwrap().data.as_ref().unwrap().get("latency").is_some());
        app.poll(false);
        assert!(app.snap.read().unwrap().data.as_ref().unwrap().get("latency").is_some());
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
