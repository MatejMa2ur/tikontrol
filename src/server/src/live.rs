//! Live numbers and history. A sampler reads WAN throughput, CPU and per-client byte counters every
//! few seconds over its own reader connection, streams them to browsers (Server-Sent Events), keeps
//! minute/hour history and derives events (client joined/left, WAN down/up).
//!
//! History is small on purpose: about 10k minute points + 2k hour points + 7 days of hourly bytes
//! per client, written to `history.json` every few minutes.

use crate::app::{lock, now, App, As};
use crate::ros::cmd::Cmd;
use crate::ros::conn::{Router, Row};
use crate::store::write_private;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// One chart line; `None` is a gap where tikontrol has no sample.
type Series = Vec<Option<f64>>;

pub const MINUTES: usize = 7 * 24 * 60;
pub const HOURS: usize = 90 * 24;
pub const CLIENT_HOURS: usize = 7 * 24;
pub const MAX_EVENTS: usize = 1000;
pub const MAX_CLIENTS: usize = 2000;
/// Longest history kept (hour points); the default when no retention is set.
pub const MAX_DAYS: u64 = 90;

/// Stored setting → days kept (0 = default).
pub fn retention_days(d: u64) -> u64 {
    if d == 0 { MAX_DAYS } else { d.min(MAX_DAYS) }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub t: u64,
    pub rx: f64,
    pub tx: f64,
    pub cpu: f64,
    pub clients: f64,
    /// worst latency in ms seen in this slot (0 = no sample), and whether the WAN was down
    #[serde(default)]
    pub ms: f64,
    #[serde(default)]
    pub down: bool,
    /// memory use in %, and the board temperature in °C when `/system/health` has a sensor
    #[serde(default)]
    pub mem: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Ev {
    pub t: u64,
    pub cat: String,
    pub level: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct History {
    pub minutes: VecDeque<Point>,
    pub hours: VecDeque<Point>,
    /// mac -> (hour start, bytes down, bytes up), newest last
    pub clients: BTreeMap<String, VecDeque<(u64, u64, u64)>>,
    pub events: VecDeque<Ev>,
    /// mac -> first seen, last seen, last interface (for "new client" and roaming)
    pub seen: BTreeMap<String, (u64, u64, String)>,
    #[serde(skip)]
    acc: Vec<Point>,
}

impl History {
    /// Adds one raw sample: averaged into the current minute, minutes rolled up into hours.
    pub fn add(&mut self, p: Point) {
        if let Some(last) = self.acc.last() {
            if last.t / 60 != p.t / 60 {
                let m = avg(&self.acc, last.t / 60 * 60);
                self.acc.clear();
                push(&mut self.minutes, m, MINUTES);
                if self.minutes.len() >= 2 && self.minutes[self.minutes.len() - 2].t / 3600 != m.t / 3600 {
                    let hour = self.minutes[self.minutes.len() - 2].t / 3600 * 3600;
                    let pts: Vec<Point> = self.minutes.iter().filter(|x| x.t / 3600 * 3600 == hour).copied().collect();
                    push(&mut self.hours, avg(&pts, hour), HOURS);
                }
            }
        }
        self.acc.push(p);
    }

    /// Data retention (Settings · System): drops everything older than `cutoff`.
    pub fn prune(&mut self, cutoff: u64) {
        self.minutes.retain(|p| p.t >= cutoff);
        self.hours.retain(|p| p.t >= cutoff);
        self.events.retain(|e| e.t >= cutoff);
        self.clients.retain(|_, q| {
            q.retain(|x| x.0 >= cutoff);
            !q.is_empty()
        });
        self.seen.retain(|_, x| x.1 >= cutoff);
    }

    pub fn event(&mut self, e: Ev) {
        push(&mut self.events, e, MAX_EVENTS);
    }

    pub fn client_bytes(&mut self, mac: &str, t: u64, down: u64, up: u64) {
        if !self.clients.contains_key(mac) && self.clients.len() >= MAX_CLIENTS {
            return;
        }
        let q = self.clients.entry(mac.into()).or_default();
        let hour = t / 3600 * 3600;
        match q.back_mut() {
            Some(last) if last.0 == hour => {
                last.1 += down;
                last.2 += up
            }
            _ => push(q, (hour, down, up), CLIENT_HOURS),
        }
    }

    /// Series for the dashboard's Internet Activity chart: 1H = 60×1 min, 1D = 96×15 min, 1W = 168×1 h.
    pub fn series(&self, range: &str) -> Value {
        let (n, step) = match range {
            "1D" => (96u64, 900u64),
            "1W" => (168, 3600),
            _ => (60, 60),
        };
        let end = now() / step * step;
        let src: Vec<Point> = if step == 3600 { self.hours.iter().chain(self.minutes.iter().filter(|m| m.t >= end)).copied().collect() } else { self.minutes.iter().copied().collect() };
        let mut t: Vec<u64> = vec![];
        let (mut cl, mut rx, mut tx): (Series, Series, Series) = (vec![], vec![], vec![]);
        let (mut cpu, mut mem, mut temp): (Series, Series, Series) = (vec![], vec![], vec![]);
        for i in 0..n {
            let start = end - (n - 1 - i) * step;
            let pts: Vec<Point> = src.iter().filter(|p| p.t >= start && p.t < start + step).copied().collect();
            t.push(start);
            // no samples (before tikontrol ran, or it was down): null, so the chart shows a gap, not a zero
            if pts.is_empty() {
                cl.push(None);
                rx.push(None);
                tx.push(None);
                cpu.push(None);
                mem.push(None);
                temp.push(None);
                continue;
            }
            let a = avg(&pts, start);
            cl.push(Some(a.clients.round()));
            rx.push(Some(a.rx.round()));
            tx.push(Some(a.tx.round()));
            cpu.push(Some(a.cpu.round()));
            mem.push(Some(a.mem.round()));
            temp.push(a.temp.map(|x| x.round()));
        }
        // peak minute average in this range and over all stored history (minutes + hours)
        // ponytail: minute averages, not raw 2 s samples; store a per-minute max in Point if short bursts matter
        let peak = |it: &mut dyn Iterator<Item = &Point>| it.fold((0.0f64, 0.0f64), |(r, x), p| (r.max(p.rx), x.max(p.tx)));
        let start = end - (n - 1) * step;
        let (prx, ptx) = peak(&mut src.iter().filter(|p| p.t >= start));
        let (arx, atx) = peak(&mut self.hours.iter().chain(self.minutes.iter()));
        json!({ "t": t, "clients": cl, "rx": rx, "tx": tx, "cpu": cpu, "mem": mem, "temp": temp, "samples": src.len(), "peak": { "rx": prx.round(), "tx": ptx.round() }, "peakAll": { "rx": arx.round(), "tx": atx.round() } })
    }

    /// Internet health bar: the last 12 h in 48 slots of 15 min. ok / warn (slow) / bad (down) / none.
    pub fn health(&self) -> Vec<&'static str> {
        let end = now() / 900 * 900;
        (0..48u64)
            .map(|i| {
                let start = end - (47 - i) * 900;
                let pts: Vec<&Point> = self.minutes.iter().filter(|p| p.t >= start && p.t < start + 900).collect();
                if pts.is_empty() {
                    "none"
                } else if pts.iter().any(|p| p.down) {
                    "bad"
                } else if pts.iter().any(|p| p.ms > 100.0) {
                    "warn"
                } else {
                    "ok"
                }
            })
            .collect()
    }
}

fn push<T>(q: &mut VecDeque<T>, v: T, cap: usize) {
    q.push_back(v);
    while q.len() > cap {
        q.pop_front();
    }
}

fn avg(pts: &[Point], t: u64) -> Point {
    if pts.is_empty() {
        return Point { t, ..Default::default() };
    }
    let n = pts.len() as f64;
    Point {
        t,
        rx: pts.iter().map(|p| p.rx).sum::<f64>() / n,
        tx: pts.iter().map(|p| p.tx).sum::<f64>() / n,
        cpu: pts.iter().map(|p| p.cpu).sum::<f64>() / n,
        clients: pts.iter().map(|p| p.clients).fold(0.0, f64::max),
        ms: pts.iter().map(|p| p.ms).fold(0.0, f64::max),
        down: pts.iter().any(|p| p.down),
        mem: pts.iter().map(|p| p.mem).sum::<f64>() / n,
        temp: {
            let ts: Vec<f64> = pts.iter().filter_map(|p| p.temp).collect();
            (!ts.is_empty()).then(|| ts.iter().sum::<f64>() / ts.len() as f64)
        },
    }
}

/// Board temperature from `/system/health/print` rows (name, value). Boards differ: prefer the CPU
/// sensor, then any "temperature" row. None when the board has no sensor (or no health menu).
pub fn temperature(rows: &[Row]) -> Option<f64> {
    let val = |want: &dyn Fn(&str) -> bool| rows.iter().find(|r| r.get("name").is_some_and(|n| want(n))).and_then(|r| r.get("value")?.parse::<f64>().ok());
    val(&|n| n == "cpu-temperature").or_else(|| val(&|n| n == "temperature")).or_else(|| val(&|n| n.contains("temperature")))
}

/// Parses the registration table's "rx,tx" byte counter.
pub fn pair(v: Option<&String>) -> (u64, u64) {
    let mut it = v.map(|s| s.as_str()).unwrap_or("").split(',').map(|x| x.trim().parse::<u64>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0))
}

/// mac -> (interface, bytes down, bytes up)
pub type Stations = BTreeMap<String, (String, u64, u64)>;

/// What one sample reads from the router.
pub struct Reading {
    pub rx: f64,
    pub tx: f64,
    pub cpu: f64,
    pub mem: f64,
    pub temp: Option<f64>,
    pub wan_running: bool,
    /// mac -> (interface, bytes counter as seen by the AP: rx = client upload, tx = client download)
    pub stations: Stations,
}

pub fn read(r: &mut dyn Router, wan_if: &str) -> Result<Reading, String> {
    let m = r.run(&Cmd::new("/interface/monitor-traffic").arg("interface", wan_if).arg("once", "").proplist(&["rx-bits-per-second", "tx-bits-per-second"]))?;
    let m = m.first().cloned().unwrap_or_default();
    let num = |row: &Row, k: &str| row.get(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    let res = r.run(&Cmd::new("/system/resource/print").proplist(&["cpu-load", "free-memory", "total-memory"]))?;
    // feature-detect: not every board has /system/health or a temperature sensor
    let temp = r.run(&Cmd::new("/system/health/print").proplist(&["name", "value"])).ok().and_then(|rows| temperature(&rows));
    let running = r.run(&Cmd::new("/interface/print").proplist(&["running"]).query("name", wan_if))?;
    let mut stations = BTreeMap::new();
    for path in ["/interface/wireless/registration-table/print", "/interface/wifi/registration-table/print"] {
        // a router has one of the two wifi packages; the other path errors and is skipped
        if let Ok(rows) = r.run(&Cmd::new(path).proplist(&["mac-address", "interface", "bytes"])) {
            for row in rows {
                let (up, down) = pair(row.get("bytes"));
                if let Some(mac) = row.get("mac-address") {
                    stations.insert(mac.clone(), (row.get("interface").cloned().unwrap_or_default(), down, up));
                }
            }
        }
    }
    Ok(Reading {
        rx: num(&m, "rx-bits-per-second"),
        tx: num(&m, "tx-bits-per-second"),
        cpu: res.first().map_or(0.0, |r| num(r, "cpu-load")),
        mem: res.first().map_or(0.0, |r| {
            let total = num(r, "total-memory");
            if total > 0.0 { (1.0 - num(r, "free-memory") / total) * 100.0 } else { 0.0 }
        }),
        temp,
        wan_running: running.first().and_then(|r| r.get("running")).is_none_or(|v| v == "true"),
        stations,
    })
}

#[derive(Default)]
pub struct Live {
    pub history: Mutex<History>,
    /// latest SSE frame and a counter so waiting streams know a new one is there
    frame: Mutex<(u64, String)>,
    cv: Condvar,
    pub subscribers: Mutex<usize>,
    prev: Mutex<Option<(u64, Stations)>>,
    /// last WAN link state the sampler saw (None until the first reading); read by the alert rules
    pub wan_up: Mutex<Option<bool>>,
    pub latency_ms: Mutex<f64>,
}

impl Live {
    /// Folds one reading into the live frame, history and derived events.
    pub fn ingest(&self, t: u64, rd: Reading, names: &BTreeMap<String, String>) {
        let mut rates = serde_json::Map::new();
        let mut h = lock(&self.history);
        {
            let mut prev = lock(&self.prev);
            if let Some((pt, old)) = prev.as_ref() {
                let dt = t.saturating_sub(*pt).max(1) as f64;
                for (mac, (_, down, up)) in &rd.stations {
                    if let Some((_, od, ou)) = old.get(mac) {
                        // counters reset when a client reconnects: ignore negative deltas
                        let (dd, du) = (down.saturating_sub(*od), up.saturating_sub(*ou));
                        rates.insert(mac.clone(), json!({ "rx": dd as f64 * 8.0 / dt, "tx": du as f64 * 8.0 / dt }));
                        h.client_bytes(mac, t, dd, du);
                    }
                }
                let name = |m: &str| names.get(m).cloned().unwrap_or_else(|| m.to_string());
                for (mac, (ifn, ..)) in &rd.stations {
                    match old.get(mac) {
                        None => {
                            let new = !h.seen.contains_key(mac);
                            h.event(Ev { t, cat: "Clients".into(), level: "info".into(), text: format!("{} {} ({ifn})", name(mac), if new { "joined for the first time" } else { "connected" }), mac: Some(mac.clone()) });
                        }
                        Some((oi, ..)) if oi != ifn => h.event(Ev { t, cat: "WiFi".into(), level: "info".into(), text: format!("{} roamed from {oi} to {ifn}", name(mac)), mac: Some(mac.clone()) }),
                        _ => {}
                    }
                }
                for mac in old.keys().filter(|m| !rd.stations.contains_key(*m)) {
                    h.event(Ev { t, cat: "Clients".into(), level: "info".into(), text: format!("{} disconnected", name(mac)), mac: Some(mac.clone()) });
                }
            }
            for (mac, (ifn, ..)) in &rd.stations {
                let e = h.seen.entry(mac.clone()).or_insert((t, t, ifn.clone()));
                e.1 = t;
                e.2 = ifn.clone();
            }
            if h.seen.len() > MAX_CLIENTS * 2 {
                let mut by_age: Vec<(String, u64)> = h.seen.iter().map(|(k, v)| (k.clone(), v.1)).collect();
                by_age.sort_by_key(|x| x.1);
                for (k, _) in by_age.iter().take(h.seen.len() - MAX_CLIENTS * 2) {
                    h.seen.remove(k);
                }
            }
            *prev = Some((t, rd.stations.clone()));
        }
        let mut up = lock(&self.wan_up);
        if *up != Some(rd.wan_running) {
            if up.is_some() {
                let (level, text) = if rd.wan_running { ("info", "Internet connection is back") } else { ("error", "Internet connection is down (WAN link lost)") };
                h.event(Ev { t, cat: "System".into(), level: level.into(), text: text.into(), mac: None });
            }
            *up = Some(rd.wan_running);
        }
        let ms = *lock(&self.latency_ms);
        h.add(Point { t, rx: rd.rx, tx: rd.tx, cpu: rd.cpu, clients: rd.stations.len() as f64, ms, down: !rd.wan_running, mem: rd.mem, temp: rd.temp });
        drop(h);
        let frame = json!({ "t": t, "rx": rd.rx, "tx": rd.tx, "cpu": rd.cpu, "mem": rd.mem, "temp": rd.temp, "wanUp": rd.wan_running, "rate": rates });
        let mut f = lock(&self.frame);
        f.0 += 1;
        f.1 = frame.to_string();
        self.cv.notify_all();
    }

    /// Blocks until a frame newer than `after` exists (or the timeout passes).
    pub fn next_frame(&self, after: u64, timeout: Duration) -> Option<(u64, String)> {
        let g = lock(&self.frame);
        let (g, _) = self.cv.wait_timeout_while(g, timeout, |f| f.0 <= after).unwrap_or_else(|e| e.into_inner());
        (g.0 > after).then(|| g.clone())
    }

    pub fn load(path: &Path) -> Live {
        let h = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Live { history: Mutex::new(h), ..Default::default() }
    }

    pub fn save(&self, path: &Path) {
        let h = lock(&self.history);
        if let Ok(b) = serde_json::to_vec(&*h) {
            let _ = write_private(path, &b);
        }
    }
}

/// Sampler thread: 2 s while someone watches, 15 s otherwise (history only). Own connection, so a
/// slow snapshot never stalls the live chart.
pub fn spawn(app: Arc<App>, path: PathBuf) {
    std::thread::spawn(move || {
        let mut conn: Option<Box<dyn Router + Send>> = None;
        let mut last_save = now();
        let mut last_err: Option<String> = None;
        loop {
            let watching = *lock(&app.live.subscribers) > 0;
            if app.router_cfg().is_some() && app.snap.read().map(|s| s.data.is_some()).unwrap_or(false) {
                if conn.is_none() {
                    conn = app.open(As::Reader).ok();
                }
                let (wan_if, names) = {
                    let s = app.snap.read().unwrap_or_else(|e| e.into_inner());
                    let d = s.data.as_deref();
                    let wan = d.and_then(|d| d["wan"].get(0)).and_then(|w| w["interface"].as_str()).unwrap_or("ether1").to_string();
                    let mut names: BTreeMap<String, String> = d
                        .and_then(|d| d["leases"].as_array())
                        .map(|a| a.iter().filter_map(|l| Some((l["mac-address"].as_str()?.to_string(), l["comment"].as_str().filter(|c| !c.is_empty()).or(l["host-name"].as_str())?.to_string()))).collect())
                        .unwrap_or_default();
                    names.extend(lock(&app.store).state.prefs.aliases.clone());
                    (wan, names)
                };
                if let Some(c) = conn.as_mut() {
                    match read(c.as_mut(), &wan_if) {
                        Ok(rd) => app.live.ingest(now(), rd, &names),
                        Err(e) => {
                            // say why once per distinct error, so a reconnect loop is visible without flooding the log
                            if last_err.as_deref() != Some(e.as_str()) {
                                eprintln!("tikontrol: live read failed, reconnecting: {e}");
                                last_err = Some(e);
                            }
                            conn = None
                        }
                    }
                }
            }
            if now() - last_save >= 300 {
                let days = retention_days(lock(&app.store).state.history_days);
                lock(&app.live.history).prune(now().saturating_sub(days * 86400));
                app.live.save(&path);
                last_save = now();
            }
            std::thread::sleep(Duration::from_secs(if watching { 2 } else { 15 }) * app.backoff());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rd(st: &[(&str, &str, u64, u64)], up: bool) -> Reading {
        Reading { rx: 1000.0, tx: 500.0, cpu: 5.0, mem: 40.0, temp: None, wan_running: up, stations: st.iter().map(|(m, i, d, u)| (m.to_string(), (i.to_string(), *d, *u))).collect() }
    }

    #[test]
    fn rates_events_and_bytes() {
        let l = Live::default();
        let names: BTreeMap<String, String> = [("AA".to_string(), "Phone".to_string())].into();
        l.ingest(1000, rd(&[("AA", "wlan1", 0, 0)], true), &names);
        l.ingest(1002, rd(&[("AA", "wlan2", 2000, 1000), ("BB", "wlan1", 5, 5)], false), &names);
        l.ingest(1004, rd(&[("BB", "wlan1", 5, 5)], true), &names);
        let h = lock(&l.history);
        let texts: Vec<&str> = h.events.iter().map(|e| e.text.as_str()).collect();
        assert!(texts.contains(&"Phone roamed from wlan1 to wlan2"));
        assert!(texts.contains(&"BB joined for the first time (wlan1)"));
        assert!(texts.contains(&"Phone disconnected"));
        assert!(texts.contains(&"Internet connection is down (WAN link lost)"));
        assert!(texts.contains(&"Internet connection is back"));
        assert_eq!(h.clients["AA"].back().unwrap().1, 2000);
        drop(h);
        let (_, f) = l.next_frame(0, Duration::from_millis(1)).unwrap();
        let v: Value = serde_json::from_str(&f).unwrap();
        assert_eq!(v["rate"]["BB"]["rx"], 0.0);
    }

    #[test]
    fn counter_reset_is_not_negative() {
        let l = Live::default();
        l.ingest(10, rd(&[("AA", "w", 9000, 9000)], true), &BTreeMap::new());
        l.ingest(12, rd(&[("AA", "w", 10, 10)], true), &BTreeMap::new());
        assert_eq!(lock(&l.history).clients["AA"].back().unwrap().1, 0);
    }

    #[test]
    fn minutes_roll_up_and_rings_are_capped() {
        let mut h = History::default();
        for i in 0..(3 * 3600 / 30) {
            h.add(Point { t: 1_000_000 + i * 30, rx: 10.0, clients: 2.0, ..Default::default() });
        }
        assert!(h.minutes.len() >= 179);
        assert!(h.hours.len() >= 2);
        assert_eq!(h.minutes[0].rx, 10.0);
        let mut q = VecDeque::new();
        for i in 0..10 {
            push(&mut q, i, 3);
        }
        assert_eq!(q, [7, 8, 9]);
    }

    #[test]
    fn series_has_the_ui_shape() {
        let h = History::default();
        let s = h.series("1D");
        assert_eq!(s["t"].as_array().unwrap().len(), 96);
        assert!(s["rx"].as_array().unwrap().iter().all(Value::is_null), "no data is a gap, not a zero");
        assert_eq!(h.health().len(), 48);
        assert!(h.health().iter().all(|x| *x == "none"));
    }

    #[test]
    fn temperature_prefers_cpu_sensor() {
        use crate::ros::queries::tests::row;
        assert_eq!(temperature(&[row(&[("name", "voltage"), ("value", "24.1")]), row(&[("name", "board-temperature1"), ("value", "41")]), row(&[("name", "cpu-temperature"), ("value", "52")])]), Some(52.0));
        assert_eq!(temperature(&[row(&[("name", "board-temperature1"), ("value", "41")])]), Some(41.0));
        assert_eq!(temperature(&[row(&[("name", "fan1-speed"), ("value", "x")])]), None);
    }

    #[test]
    fn pair_parses_counters() {
        assert_eq!(pair(Some(&"12,34".to_string())), (12, 34));
        assert_eq!(pair(None), (0, 0));
        assert_eq!(pair(Some(&"x".to_string())), (0, 0));
    }
}
