//! tikontrol: one small binary that serves the web UI and talks to RouterOS over the binary API.
//!
//! Settings come from the environment (see docs/DEPLOY.md):
//! TIK_DATA (default ./data), TIK_LISTEN (HTTPS, default 0.0.0.0:8443), TIK_HTTP (plain HTTP, loopback only),
//! TIK_TLS_CERT + TIK_TLS_KEY (own certificate), TIK_POLL (seconds between snapshots, default 30).

use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tikontrol::{api, app, http, store, tlscert};

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

fn die(msg: &str) -> ! {
    eprintln!("tikontrol: {msg}");
    std::process::exit(1)
}

fn main() {
    if std::env::args().any(|a| a == "--version") {
        println!("tikontrol {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let data = PathBuf::from(env("TIK_DATA").unwrap_or_else(|| "./data".into()));
    let st = store::Store::open(&data).unwrap_or_else(|e| die(&format!("cannot open {}: {e}", data.display())));
    let poll = env("TIK_POLL").and_then(|s| s.parse().ok()).unwrap_or(30u64).clamp(5, 3600);

    let mut listeners: Vec<(TcpListener, Option<Arc<rustls::ServerConfig>>)> = Vec::new();
    if let Some(plain) = env("TIK_HTTP") {
        let a: SocketAddr = plain.parse().unwrap_or_else(|_| die("TIK_HTTP must be ip:port"));
        // plain HTTP would put the session cookie on the wire: only for a local reverse proxy or development
        if !a.ip().is_loopback() {
            die("TIK_HTTP only accepts a loopback address; use TIK_LISTEN (HTTPS) for the network");
        }
        listeners.push((TcpListener::bind(a).unwrap_or_else(|e| die(&format!("bind {a}: {e}"))), None));
    }
    let tls_on = env("TIK_LISTEN").as_deref() != Some("off");
    if tls_on {
        let addr = env("TIK_LISTEN").unwrap_or_else(|| "0.0.0.0:8443".into());
        let (cfg, fp) = tlscert::server_config(&data, env("TIK_TLS_CERT").as_deref(), env("TIK_TLS_KEY").as_deref()).unwrap_or_else(|e| die(&e));
        println!("tikontrol: HTTPS on {addr}, certificate SHA-256 {fp}");
        listeners.push((TcpListener::bind(&addr).unwrap_or_else(|e| die(&format!("bind {addr}: {e}"))), Some(cfg)));
    }
    if listeners.is_empty() {
        die("nothing to listen on: set TIK_LISTEN or TIK_HTTP");
    }

    let app = Arc::new(app::App::new(st, app::real_connector(), tls_on));
    if let Some(t) = app::lock(&app.setup_token).as_ref() {
        println!("tikontrol: first start. Create the admin account in the browser with setup token {t}");
    }
    app.spawn_poller(Duration::from_secs(poll));
    let handler: http::Handler = {
        let app = app.clone();
        Arc::new(move |req| api::handle(&app, req))
    };
    let mut threads = Vec::new();
    for (l, tls) in listeners {
        let h = handler.clone();
        threads.push(std::thread::spawn(move || http::serve(l, tls, h)));
    }
    for t in threads {
        let _ = t.join();
    }
}
