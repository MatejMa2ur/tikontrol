//! Alarm Manager: rules evaluated every 30 s over the router snapshot and live history, an alert
//! history with acknowledge, quiet hours, and opt-in notification channels (ntfy, webhook,
//! Telegram).
//!
//! An alert fires when its condition becomes true and re-arms only after the condition clears, so
//! a sustained problem notifies once. Channels are off by default: enabling one sends alert texts
//! (which can name clients and versions) to that third party. Channel secrets are sealed like router
//! passwords and never leave the server.

use crate::api::body;
use crate::app::{lock, now, App};
use crate::auth::Session;
use crate::http::{Req, Resp};
use crate::outbound::{self, parse_url};
use crate::store::{Role, Sealer};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const MAX_HISTORY: usize = 500;
/// at most this many "new client" alerts per tick, so a flood of MACs can't flood a channel
const MAX_NEW_CLIENTS: usize = 10;
const INSECURE: &[&str] = &["telnet", "ftp", "www"];

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    #[default]
    Warning,
    Critical,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub enabled: bool,
    pub severity: Severity,
    /// percent, for the CPU and memory rules only
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
}

fn rule(enabled: bool, severity: Severity, threshold: Option<f64>) -> Rule {
    Rule { enabled, severity, threshold }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Rules {
    pub wan_down: Rule,
    pub unreachable: Rule,
    pub new_client: Rule,
    pub cpu_high: Rule,
    pub memory_high: Rule,
    pub update_available: Rule,
    pub insecure_service: Rule,
}

impl Default for Rules {
    fn default() -> Rules {
        use Severity::*;
        Rules {
            wan_down: rule(true, Critical, None),
            unreachable: rule(true, Critical, None),
            new_client: rule(true, Info, None),
            cpu_high: rule(true, Warning, Some(90.0)),
            memory_high: rule(true, Warning, Some(90.0)),
            update_available: rule(true, Info, None),
            insecure_service: rule(true, Warning, None),
        }
    }
}

impl Rules {
    fn get(&self, id: &str) -> Option<&Rule> {
        Some(match id {
            "wanDown" => &self.wan_down,
            "unreachable" => &self.unreachable,
            "newClient" => &self.new_client,
            "cpuHigh" => &self.cpu_high,
            "memoryHigh" => &self.memory_high,
            "updateAvailable" => &self.update_available,
            "insecureService" => &self.insecure_service,
            _ => return None,
        })
    }

    fn validate(&self) -> Result<(), String> {
        for (id, r) in [("cpuHigh", &self.cpu_high), ("memoryHigh", &self.memory_high)] {
            if !r.threshold.is_some_and(|t| (1.0..=100.0).contains(&t)) {
                return Err(format!("{id}: threshold must be a percentage from 1 to 100"));
            }
        }
        for (id, r) in [("wanDown", &self.wan_down), ("unreachable", &self.unreachable), ("newClient", &self.new_client), ("updateAvailable", &self.update_available), ("insecureService", &self.insecure_service)] {
            if r.threshold.is_some() {
                return Err(format!("{id} has no threshold"));
            }
        }
        Ok(())
    }
}

/// Quiet hours in local time, given as the browser's UTC offset. Alerts are still recorded.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Quiet {
    pub enabled: bool,
    /// local hour 0-23; start == end means no quiet time
    pub start: u8,
    pub end: u8,
    /// minutes east of UTC, -720..=840
    pub offset_minutes: i32,
    /// critical alerts still notify during quiet hours
    pub critical_bypass: bool,
}

impl Default for Quiet {
    fn default() -> Quiet {
        Quiet { enabled: false, start: 23, end: 7, offset_minutes: 0, critical_bypass: true }
    }
}

impl Quiet {
    pub fn active(&self, t: u64) -> bool {
        if !self.enabled || self.start == self.end {
            return false;
        }
        let h = ((t as i64 + self.offset_minutes as i64 * 60).rem_euclid(86_400) / 3600) as u8;
        if self.start < self.end {
            (self.start..self.end).contains(&h)
        } else {
            h >= self.start || h < self.end
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.start > 23 || self.end > 23 {
            return Err("quiet hours: start and end are hours from 0 to 23".into());
        }
        if !(-720..=840).contains(&self.offset_minutes) {
            return Err("quiet hours: offsetMinutes must be between -720 and 840".into());
        }
        Ok(())
    }
}

/// Stored channel settings. `token` and `url` fields hold sealed values ("alert-secret:<channel>").
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Ntfy {
    pub enabled: bool,
    pub min_severity: Severity,
    pub server: String,
    pub topic: String,
    pub token: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Webhook {
    pub enabled: bool,
    pub min_severity: Severity,
    /// sealed: webhook URLs often carry their secret in the path (Home Assistant) or query
    pub url: Option<String>,
    /// host of the URL, safe to show
    pub host: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Telegram {
    pub enabled: bool,
    pub min_severity: Severity,
    pub chat_id: String,
    pub token: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Channels {
    pub ntfy: Ntfy,
    pub webhook: Webhook,
    pub telegram: Telegram,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    pub id: u64,
    pub t: u64,
    pub rule: String,
    pub severity: Severity,
    pub text: String,
    /// local-only detail (never sent to a channel)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub ack: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ack_by: Option<String>,
    /// when the condition went away (set right away for one-shot alerts like a new client)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleared: Option<u64>,
    /// held back by quiet hours
    #[serde(default)]
    pub quiet: bool,
    #[serde(default)]
    pub sent: Vec<String>,
    #[serde(default)]
    pub failed: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Alerts {
    pub rules: Rules,
    pub quiet: Quiet,
    pub channels: Channels,
    /// newest last, capped at MAX_HISTORY
    pub history: VecDeque<Alert>,
    /// condition key -> alert id, for conditions that are currently true
    pub active: BTreeMap<String, u64>,
    pub next_id: u64,
    /// newest first-seen time already checked for the "new client" rule (0 = not started)
    pub seen_until: u64,
}

/// Older state files have `"alerts": null`; read that (or anything unreadable) as the defaults.
pub fn lenient<'de, D: Deserializer<'de>>(d: D) -> Result<Alerts, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

/// A condition that is true right now.
#[derive(Debug, Clone, PartialEq)]
pub struct Cond {
    pub key: String,
    pub rule: &'static str,
    pub text: String,
    pub detail: Option<String>,
}

/// What one tick sees.
#[derive(Default)]
pub struct Inputs<'a> {
    pub snap: Option<&'a Value>,
    pub error: Option<&'a str>,
    pub wan_up: Option<bool>,
    /// CPU load averaged over the last 5 minutes of live history, if there is any
    pub cpu_avg: Option<f64>,
    /// (mac, name, first seen) of clients first seen after `seen_until`
    pub new_clients: Vec<(String, String, u64)>,
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().trim_end_matches('%').parse().ok()))
}

fn truthy(v: &Value) -> bool {
    v.as_bool().unwrap_or_else(|| v.as_str() == Some("true"))
}

/// Names and hostnames come from other devices: strip control characters and cap the length.
pub fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

/// Evaluates every enabled rule. Returns the true conditions and the rules that could be evaluated
/// (a rule that can't be evaluated keeps its current state instead of clearing).
pub fn conditions(r: &Rules, i: &Inputs) -> (Vec<Cond>, BTreeSet<&'static str>) {
    let mut out = Vec::new();
    let mut evaluated = BTreeSet::from(["unreachable"]);
    if r.unreachable.enabled {
        if let Some(e) = i.error {
            out.push(Cond { key: "unreachable".into(), rule: "unreachable", text: "tikontrol cannot read the router".into(), detail: Some(clean(e, 300)) });
        }
    }
    let mut c = |key: String, rule: &'static str, text: String| out.push(Cond { key, rule, text, detail: None });
    let healthy = i.error.is_none() && i.snap.is_some();
    if healthy {
        let s = i.snap.unwrap_or(&Value::Null);
        evaluated.extend(["wanDown", "cpuHigh", "memoryHigh", "updateAvailable", "insecureService"]);
        if r.wan_down.enabled && i.wan_up == Some(false) {
            c("wanDown".into(), "wanDown", "Internet connection is down (WAN link lost)".into());
        }
        let res = &s["resource"];
        if r.cpu_high.enabled {
            let t = r.cpu_high.threshold.unwrap_or(90.0);
            if let Some(cpu) = i.cpu_avg.or_else(|| num(&res["cpu-load"])) {
                if cpu >= t {
                    c("cpuHigh".into(), "cpuHigh", format!("CPU load is {cpu:.0}% (threshold {t:.0}%)"));
                }
            }
        }
        if r.memory_high.enabled {
            let t = r.memory_high.threshold.unwrap_or(90.0);
            if let (Some(total), Some(free)) = (num(&res["total-memory"]), num(&res["free-memory"])) {
                let used = if total > 0.0 { (total - free) / total * 100.0 } else { 0.0 };
                if used >= t {
                    c("memoryHigh".into(), "memoryHigh", format!("Memory use is {used:.0}% (threshold {t:.0}%)"));
                }
            }
        }
        if r.update_available.enabled {
            let u = &s["update"];
            let (inst, latest) = (u["installed-version"].as_str().unwrap_or(""), u["latest-version"].as_str().unwrap_or(""));
            if !latest.is_empty() && !inst.is_empty() && inst != latest {
                let (inst, latest) = (clean(inst, 32), clean(latest, 32));
                c(format!("updateAvailable:{latest}"), "updateAvailable", format!("RouterOS {latest} is available (installed {inst})"));
            }
        }
        if r.insecure_service.enabled {
            let mut names: BTreeSet<&str> = BTreeSet::new();
            for sv in s["services"].as_array().into_iter().flatten() {
                if let Some(n) = sv["name"].as_str().filter(|n| INSECURE.contains(n)) {
                    if !truthy(&sv["disabled"]) {
                        names.insert(n);
                    }
                }
            }
            for n in names {
                c(format!("insecureService:{n}"), "insecureService", format!("Insecure service {n} is enabled on the router"));
            }
        }
    }
    (out, evaluated)
}

impl Alerts {
    fn record(&mut self, t: u64, rule: &str, text: String, detail: Option<String>, cleared: Option<u64>) -> u64 {
        self.next_id += 1;
        let severity = self.rules.get(rule).map_or(Severity::Info, |r| r.severity);
        self.history.push_back(Alert { id: self.next_id, t, rule: rule.into(), severity, text, detail, ack: false, ack_by: None, cleared, quiet: false, sent: vec![], failed: vec![] });
        while self.history.len() > MAX_HISTORY {
            self.history.pop_front();
        }
        self.next_id
    }

    /// Applies one evaluation: fires conditions that just became true, clears the ones that went
    /// away, records new clients. Returns the ids of new alerts (to notify) and whether anything changed.
    pub fn step(&mut self, t: u64, conds: Vec<Cond>, evaluated: &BTreeSet<&'static str>, new_clients: &[(String, String, u64)]) -> (Vec<u64>, bool) {
        let mut fired = Vec::new();
        let mut changed = false;
        let now_true: BTreeSet<String> = conds.iter().map(|c| c.key.clone()).collect();
        for c in conds {
            if !self.active.contains_key(&c.key) {
                let id = self.record(t, c.rule, c.text, c.detail, None);
                self.active.insert(c.key, id);
                fired.push(id);
            }
        }
        let gone: Vec<(String, u64)> = self.active.iter().filter(|(k, _)| !now_true.contains(*k) && evaluated.contains(k.split(':').next().unwrap_or(""))).map(|(k, v)| (k.clone(), *v)).collect();
        for (k, id) in gone {
            self.active.remove(&k);
            if let Some(a) = self.history.iter_mut().find(|a| a.id == id) {
                a.cleared = Some(t);
            }
            changed = true;
        }
        if self.seen_until == 0 {
            // first run: everything already known is not "new"
            self.seen_until = new_clients.iter().map(|c| c.2).max().unwrap_or(t).max(1);
            return (fired, true);
        }
        if let Some(max) = new_clients.iter().map(|c| c.2).max() {
            self.seen_until = self.seen_until.max(max);
            changed = true;
            if self.rules.new_client.enabled {
                for (mac, name, first) in new_clients.iter().take(MAX_NEW_CLIENTS) {
                    let text = if name.is_empty() || name == mac { format!("New client joined: {mac}") } else { format!("New client joined: {name} ({mac})") };
                    fired.push(self.record(*first, "newClient", text, None, Some(*first)));
                }
            }
        }
        changed |= !fired.is_empty();
        (fired, changed)
    }
}

/// Plain, unsealed channel settings for one send. Lives only for the duration of a tick.
struct Out {
    name: &'static str,
    min: Severity,
    url: outbound::Url,
    ctype: &'static str,
    auth: Option<String>,
    kind: Kind,
}

#[derive(Clone, Copy)]
enum Kind {
    Ntfy,
    Webhook,
    Telegram,
}

fn telegram_url(token: &str) -> Option<outbound::Url> {
    parse_url(&format!("https://api.telegram.org/bot{token}/sendMessage")).ok()
}

/// Opens the configured channels. `only` picks one for a test, ignoring its enabled switch.
fn outboxes(c: &Channels, sealer: &Sealer, only: Option<&str>) -> Vec<Out> {
    let pick = |name: &str, enabled: bool| only.map_or(enabled, |o| o == name);
    let mut v = Vec::new();
    if pick("ntfy", c.ntfy.enabled) && !c.ntfy.topic.is_empty() {
        if let Ok(url) = parse_url(&c.ntfy.server) {
            let token = c.ntfy.token.as_deref().and_then(|t| sealer.open_str("alert-secret:ntfy", t));
            let url = outbound::Url { path: "/".into(), ..url };
            v.push(Out { name: "ntfy", min: c.ntfy.min_severity, url, ctype: "application/json", auth: token.map(|t| format!("Bearer {t}")), kind: Kind::Ntfy });
        }
    }
    if pick("webhook", c.webhook.enabled) {
        if let Some(url) = c.webhook.url.as_deref().and_then(|u| sealer.open_str("alert-secret:webhook", u)).and_then(|u| parse_url(&u).ok()) {
            v.push(Out { name: "webhook", min: c.webhook.min_severity, url, ctype: "application/json", auth: None, kind: Kind::Webhook });
        }
    }
    if pick("telegram", c.telegram.enabled) && !c.telegram.chat_id.is_empty() {
        if let Some(url) = c.telegram.token.as_deref().and_then(|t| sealer.open_str("alert-secret:telegram", t)).and_then(|t| telegram_url(&t)) {
            v.push(Out { name: "telegram", min: c.telegram.min_severity, url, ctype: "application/json", auth: None, kind: Kind::Telegram });
        }
    }
    v
}

fn payload(o: &Out, c: &Channels, a: &Alert) -> Vec<u8> {
    let title = format!("tikontrol: {}", match a.severity {
        Severity::Critical => "critical alert",
        Severity::Warning => "warning",
        Severity::Info => "notice",
    });
    let v = match o.kind {
        Kind::Ntfy => json!({ "topic": c.ntfy.topic, "title": title, "message": a.text, "priority": match a.severity { Severity::Critical => 5, Severity::Warning => 4, Severity::Info => 3 }, "tags": ["tikontrol", a.rule] }),
        Kind::Webhook => json!({ "source": "tikontrol", "id": a.id, "time": a.t, "rule": a.rule, "severity": a.severity, "text": a.text }),
        Kind::Telegram => json!({ "chat_id": c.telegram.chat_id, "text": format!("{title}\n{}", a.text), "disable_web_page_preview": true }),
    };
    serde_json::to_vec(&v).unwrap_or_default()
}

fn send(o: &Out, c: &Channels, a: &Alert) -> Result<u16, String> {
    let auth = o.auth.as_deref().map(|v| ("authorization", v));
    outbound::post(&o.url, o.ctype, auth.as_slice(), &payload(o, c, a))
}

/// Sends the given new alerts to every channel whose minimum severity they meet, unless quiet
/// hours hold them back. Network I/O happens without holding the store lock.
fn notify(app: &App, ids: &[u64]) {
    if ids.is_empty() {
        return;
    }
    let t = now();
    let (outs, chans, quiet, alerts) = {
        let st = lock(&app.store);
        let al = &st.state.alerts;
        let list: Vec<Alert> = al.history.iter().filter(|a| ids.contains(&a.id)).cloned().collect();
        (outboxes(&al.channels, &st.sealer, None), al.channels.clone(), al.quiet.clone(), list)
    };
    let mut results: Vec<(u64, bool, Vec<String>, Vec<String>)> = Vec::new();
    for a in &alerts {
        let held = quiet.active(t) && !(quiet.critical_bypass && a.severity == Severity::Critical);
        let (mut sent, mut failed) = (vec![], vec![]);
        if !held {
            for o in outs.iter().filter(|o| a.severity >= o.min) {
                match send(o, &chans, a) {
                    Ok(_) => sent.push(o.name.to_string()),
                    Err(e) => failed.push(format!("{}: {}", o.name, clean(&e, 120))),
                }
            }
        }
        results.push((a.id, held, sent, failed));
    }
    let mut st = lock(&app.store);
    for (id, held, sent, failed) in results {
        if let Some(a) = st.state.alerts.history.iter_mut().find(|a| a.id == id) {
            a.quiet = held;
            a.sent = sent;
            a.failed = failed;
        }
    }
    let _ = st.save();
}

/// One evaluation over the current snapshot and live history.
pub fn tick(app: &App) {
    let t = now();
    let (data, error, configured) = {
        let s = app.snap.read().unwrap_or_else(|e| e.into_inner());
        (s.data.clone(), s.error.clone(), s.at > 0 || s.error.is_some())
    };
    if !configured || (app.router_cfg().is_none() && app.demo.is_none()) {
        return; // nothing polled yet
    }
    let wan_up = *lock(&app.live.wan_up);
    let seen_until = lock(&app.store).state.alerts.seen_until;
    let names: BTreeMap<String, String> = {
        let mut m: BTreeMap<String, String> = data
            .as_deref()
            .and_then(|d| d["leases"].as_array())
            .map(|a| a.iter().filter_map(|l| Some((l["mac-address"].as_str()?.to_ascii_uppercase(), l["comment"].as_str().filter(|c| !c.is_empty()).or(l["host-name"].as_str())?.to_string()))).collect())
            .unwrap_or_default();
        m.extend(lock(&app.store).state.prefs.aliases.clone());
        m
    };
    let (cpu_avg, new_clients) = {
        let h = lock(&app.live.history);
        let recent: Vec<f64> = h.minutes.iter().rev().take_while(|p| p.t + 360 >= t).map(|p| p.cpu).collect();
        let cpu = (!recent.is_empty()).then(|| recent.iter().sum::<f64>() / recent.len() as f64);
        let mut newc: Vec<(String, String, u64)> = h.seen.iter().filter(|(_, v)| v.0 > seen_until).map(|(m, v)| (clean(m, 32), clean(names.get(m).map_or("", |s| s), 64), v.0)).collect();
        newc.sort_by_key(|c| c.2);
        (cpu, newc)
    };
    let inputs = Inputs { snap: data.as_deref(), error: error.as_deref(), wan_up, cpu_avg, new_clients };
    let mut st = lock(&app.store);
    let (conds, evaluated) = conditions(&st.state.alerts.rules, &inputs);
    let (fired, changed) = st.state.alerts.step(t, conds, &evaluated, &inputs.new_clients);
    if changed {
        let _ = st.save();
    }
    drop(st);
    notify(app, &fired);
}

pub fn spawn(app: Arc<App>, every: Duration) {
    std::thread::spawn(move || loop {
        std::thread::sleep(every);
        tick(&app);
    });
}

// ---- API ----

fn secret_set(v: &Option<String>) -> Value {
    json!({ "set": v.is_some() })
}

/// Channels as the API shows them: never a secret. Viewers also don't see topic / chat id.
pub fn channels_view(c: &Channels, admin: bool) -> Value {
    let hide = |s: &str| if admin { json!(s) } else { json!(if s.is_empty() { "" } else { "•••" }) };
    json!({
        "ntfy": { "enabled": c.ntfy.enabled, "minSeverity": c.ntfy.min_severity, "server": c.ntfy.server, "topic": hide(&c.ntfy.topic), "token": secret_set(&c.ntfy.token) },
        "webhook": { "enabled": c.webhook.enabled, "minSeverity": c.webhook.min_severity, "url": { "set": c.webhook.url.is_some(), "host": c.webhook.host } },
        "telegram": { "enabled": c.telegram.enabled, "minSeverity": c.telegram.min_severity, "chatId": hide(&c.telegram.chat_id), "token": secret_set(&c.telegram.token) },
        "thirdParty": "Enabled channels send alert texts (which can include client names, MAC addresses and RouterOS versions) to that service.",
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RulesIn {
    rules: Rules,
    quiet: Quiet,
}

/// Secrets: absent or null keeps the stored one, "" clears it, anything else replaces it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct NtfyIn {
    enabled: bool,
    min_severity: Severity,
    server: String,
    topic: String,
    #[serde(default)]
    token: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct WebhookIn {
    enabled: bool,
    min_severity: Severity,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct TelegramIn {
    enabled: bool,
    min_severity: Severity,
    chat_id: String,
    #[serde(default)]
    token: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelsIn {
    ntfy: NtfyIn,
    webhook: WebhookIn,
    telegram: TelegramIn,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AckIn {
    id: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestIn {
    channel: String,
}

fn token_ok(t: &str, max: usize) -> bool {
    !t.is_empty() && t.len() <= max && t.bytes().all(|b| b.is_ascii_graphic())
}

fn is_ntfy_topic(t: &str) -> bool {
    !t.is_empty() && t.len() <= 64 && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn is_telegram_token(t: &str) -> bool {
    t.len() <= 100 && t.split_once(':').is_some_and(|(id, k)| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
}

fn is_chat_id(c: &str) -> bool {
    let n = c.strip_prefix('-').unwrap_or(c);
    c.len() <= 64 && ((!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) || c.strip_prefix('@').is_some_and(|u| u.len() >= 4 && u.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')))
}

/// Merges a channel update into the stored settings, validating everything and sealing secrets.
fn apply_channels(cur: &Channels, b: ChannelsIn, sealer: &Sealer) -> Result<Channels, String> {
    let mut c = cur.clone();
    let secret = |old: &Option<String>, new: Option<String>, label: &str, check: &dyn Fn(&str) -> Result<(), String>| -> Result<Option<String>, String> {
        match new {
            None => Ok(old.clone()),
            Some(s) if s.is_empty() => Ok(None),
            Some(s) => {
                check(&s)?;
                Ok(Some(sealer.seal(label, &s)))
            }
        }
    };
    // ntfy
    let server = b.ntfy.server.trim().trim_end_matches('/').to_string();
    if !server.is_empty() {
        let u = parse_url(&server).map_err(|e| format!("ntfy server: {e}"))?;
        if u.path != "/" {
            return Err("ntfy server: give the server address only (the topic goes in its own field)".into());
        }
    }
    if !b.ntfy.topic.is_empty() && !is_ntfy_topic(&b.ntfy.topic) {
        return Err("ntfy topic: 1-64 letters, digits, - or _".into());
    }
    c.ntfy.token = secret(&cur.ntfy.token, b.ntfy.token, "alert-secret:ntfy", &|t| if token_ok(t, 256) { Ok(()) } else { Err("ntfy token: printable characters, at most 256".into()) })?;
    if b.ntfy.enabled && (server.is_empty() || b.ntfy.topic.is_empty()) {
        return Err("ntfy needs a server and a topic before it can be enabled".into());
    }
    c.ntfy = Ntfy { enabled: b.ntfy.enabled, min_severity: b.ntfy.min_severity, server, topic: b.ntfy.topic, token: c.ntfy.token };
    // webhook
    match b.webhook.url {
        None => {}
        Some(u) if u.is_empty() => {
            c.webhook.url = None;
            c.webhook.host.clear();
        }
        Some(u) => {
            let p = parse_url(u.trim()).map_err(|e| format!("webhook URL: {e}"))?;
            c.webhook.url = Some(sealer.seal("alert-secret:webhook", u.trim()));
            c.webhook.host = p.host;
        }
    }
    if b.webhook.enabled && c.webhook.url.is_none() {
        return Err("the webhook needs a URL before it can be enabled".into());
    }
    c.webhook.enabled = b.webhook.enabled;
    c.webhook.min_severity = b.webhook.min_severity;
    // telegram
    if !b.telegram.chat_id.is_empty() && !is_chat_id(&b.telegram.chat_id) {
        return Err("Telegram chat id: a number or @channel".into());
    }
    c.telegram.token = secret(&cur.telegram.token, b.telegram.token, "alert-secret:telegram", &|t| if is_telegram_token(t) { Ok(()) } else { Err("Telegram bot token looks wrong (expected 123456:ABC…)".into()) })?;
    if b.telegram.enabled && (c.telegram.token.is_none() || b.telegram.chat_id.is_empty()) {
        return Err("Telegram needs a bot token and a chat id before it can be enabled".into());
    }
    c.telegram.enabled = b.telegram.enabled;
    c.telegram.min_severity = b.telegram.min_severity;
    c.telegram.chat_id = b.telegram.chat_id;
    Ok(c)
}

fn view(app: &App, admin: bool) -> Value {
    let st = lock(&app.store);
    let a = &st.state.alerts;
    let history: Vec<&Alert> = a.history.iter().rev().collect();
    json!({
        "rules": a.rules,
        "quiet": a.quiet,
        "channels": channels_view(&a.channels, admin),
        "history": history,
        "unacked": a.history.iter().filter(|h| !h.ack).count(),
    })
}

static LAST_TEST: AtomicU64 = AtomicU64::new(0);

pub fn route(app: &Arc<App>, req: &Req, s: &Session) -> Option<Resp> {
    if !req.path.starts_with("/api/v1/alerts") {
        return None;
    }
    let admin = s.role == Role::Admin;
    let need_admin = || Resp::error(403, "admin role required");
    Some(match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/v1/alerts") => Resp::json(200, &view(app, admin)),
        ("PUT", "/api/v1/alerts/rules" | "/api/v1/alerts/channels") | ("POST", "/api/v1/alerts/test") if !admin => need_admin(),
        ("PUT", "/api/v1/alerts/rules") => {
            let b: RulesIn = match body(req) {
                Ok(b) => b,
                Err(r) => return Some(r),
            };
            if let Err(e) = b.rules.validate().and_then(|_| b.quiet.validate()) {
                return Some(Resp::error(400, &e));
            }
            let mut st = lock(&app.store);
            st.state.alerts.rules = b.rules;
            st.state.alerts.quiet = b.quiet;
            crate::api::saved(st.save())
        }
        ("PUT", "/api/v1/alerts/channels") => {
            let b: ChannelsIn = match body(req) {
                Ok(b) => b,
                Err(r) => return Some(r),
            };
            let mut st = lock(&app.store);
            match apply_channels(&st.state.alerts.channels, b, &st.sealer) {
                Ok(c) => {
                    st.state.alerts.channels = c;
                    if st.save().is_err() {
                        return Some(Resp::error(500, "could not save state"));
                    }
                    Resp::json(200, &channels_view(&st.state.alerts.channels, true))
                }
                Err(e) => Resp::error(400, &e),
            }
        }
        ("POST", "/api/v1/alerts/ack") => {
            let b: AckIn = match body(req) {
                Ok(b) => b,
                Err(r) => return Some(r),
            };
            let mut st = lock(&app.store);
            let Some(a) = st.state.alerts.history.iter_mut().find(|a| a.id == b.id) else { return Some(Resp::error(404, "no such alert")) };
            a.ack = true;
            a.ack_by = Some(s.user.clone());
            crate::api::saved(st.save())
        }
        ("POST", "/api/v1/alerts/test") => {
            let b: TestIn = match body(req) {
                Ok(b) => b,
                Err(r) => return Some(r),
            };
            if !["ntfy", "webhook", "telegram"].contains(&b.channel.as_str()) {
                return Some(Resp::error(400, "channel must be ntfy, webhook or telegram"));
            }
            let t = now();
            if LAST_TEST.swap(t, Ordering::SeqCst) + 3 > t {
                return Some(Resp::error(429, "wait a few seconds between tests"));
            }
            let (outs, chans) = {
                let st = lock(&app.store);
                (outboxes(&st.state.alerts.channels, &st.sealer, Some(&b.channel)), st.state.alerts.channels.clone())
            };
            let Some(o) = outs.first() else { return Some(Resp::error(409, "this channel is not configured yet")) };
            let a = Alert { id: 0, t, rule: "test".into(), severity: Severity::Info, text: "Test notification from tikontrol".into(), detail: None, ack: true, ack_by: None, cleared: Some(t), quiet: false, sent: vec![], failed: vec![] };
            match send(o, &chans, &a) {
                Ok(status) => Resp::json(200, &json!({ "ok": true, "status": status })),
                Err(e) => Resp::error(502, &clean(&e, 200)),
            }
        }
        _ => Resp::error(404, "not found"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::handle;
    use crate::api::tests::{admin_session, new_app, req, with_session};
    use crate::auth;
    use crate::http::Body;

    fn sample() -> Value {
        json!({
            "resource": { "cpu-load": 12, "free-memory": 50_000_000u64, "total-memory": 100_000_000u64 },
            "update": { "installed-version": "7.20", "latest-version": "7.20" },
            "services": [ { "name": "telnet", "disabled": true }, { "name": "ssh", "disabled": false }, { "name": "www", "disabled": true } ],
        })
    }

    fn keys(c: &[Cond]) -> Vec<&str> {
        c.iter().map(|c| c.key.as_str()).collect()
    }

    fn json_of(r: &Resp) -> Value {
        match &r.body {
            Body::Bytes(b) => serde_json::from_slice(b).unwrap_or(Value::Null),
            _ => Value::Null,
        }
    }

    #[test]
    fn threshold_rules_on_a_sample_snapshot() {
        let r = Rules::default();
        let mut s = sample();
        let (c, _) = conditions(&r, &Inputs { snap: Some(&s), wan_up: Some(true), ..Default::default() });
        assert!(c.is_empty(), "{c:?}");
        s["resource"]["cpu-load"] = json!("95");
        s["resource"]["free-memory"] = json!(5_000_000);
        s["update"]["latest-version"] = json!("7.21");
        s["services"][0]["disabled"] = json!("false");
        let (c, ev) = conditions(&r, &Inputs { snap: Some(&s), wan_up: Some(false), ..Default::default() });
        assert_eq!(keys(&c), ["wanDown", "cpuHigh", "memoryHigh", "updateAvailable:7.21", "insecureService:telnet"]);
        assert!(c[1].text.contains("95%"));
        assert!(c[2].text.contains("95%"));
        assert!(ev.contains("cpuHigh"));
        // the 5-minute average wins over the instantaneous reading
        let (c, _) = conditions(&r, &Inputs { snap: Some(&s), cpu_avg: Some(40.0), ..Default::default() });
        assert!(!keys(&c).contains(&"cpuHigh"));
        // a higher threshold or a disabled rule silences it
        let mut r2 = r.clone();
        r2.memory_high.threshold = Some(99.0);
        r2.insecure_service.enabled = false;
        let (c, _) = conditions(&r2, &Inputs { snap: Some(&s), ..Default::default() });
        assert!(!keys(&c).contains(&"memoryHigh") && !keys(&c).contains(&"insecureService:telnet"));
        // poll error: only "unreachable" is evaluated, other rules keep their state
        let (c, ev) = conditions(&r, &Inputs { snap: Some(&s), error: Some("cannot reach router API-SSL"), ..Default::default() });
        assert_eq!(keys(&c), ["unreachable"]);
        assert_eq!(c[0].detail.as_deref(), Some("cannot reach router API-SSL"));
        assert!(!c[0].text.contains("API-SSL"), "error details stay local");
        assert!(!ev.contains("cpuHigh"));
    }

    #[test]
    fn the_sample_json_from_the_ui_raises_nothing_unexpected() {
        let s: Value = serde_json::from_str(include_str!("../../ui/src/lib/sample.json")).unwrap();
        let (c, _) = conditions(&Rules::default(), &Inputs { snap: Some(&s), wan_up: Some(true), ..Default::default() });
        for k in keys(&c) {
            assert!(k.starts_with("memoryHigh") || k.starts_with("insecureService") || k.starts_with("updateAvailable") || k == "cpuHigh", "{k}");
        }
    }

    #[test]
    fn fires_on_transition_and_rearms_after_clearing() {
        let mut a = Alerts { seen_until: 1, ..Default::default() };
        let all: BTreeSet<&str> = ["wanDown", "cpuHigh", "unreachable"].into();
        let wan = || vec![Cond { key: "wanDown".into(), rule: "wanDown", text: "down".into(), detail: None }];
        let (f, _) = a.step(100, wan(), &all, &[]);
        assert_eq!(f.len(), 1);
        assert_eq!(a.history[0].severity, Severity::Critical);
        for t in [130, 160, 190] {
            assert!(a.step(t, wan(), &all, &[]).0.is_empty(), "no repeat while it stays down");
        }
        assert_eq!(a.history.len(), 1);
        // not evaluated (router unreachable): stays active
        a.step(200, vec![], &BTreeSet::from(["unreachable"]), &[]);
        assert!(a.active.contains_key("wanDown"));
        // clears, then re-arms
        a.step(220, vec![], &all, &[]);
        assert_eq!(a.history[0].cleared, Some(220));
        assert!(a.active.is_empty());
        assert_eq!(a.step(250, wan(), &all, &[]).0.len(), 1);
        assert_eq!(a.history.len(), 2);
    }

    #[test]
    fn new_clients_fire_once_and_not_on_first_run() {
        let mut a = Alerts::default();
        let known = vec![("AA:AA:AA:AA:AA:01".to_string(), "Laptop".to_string(), 50)];
        let (f, _) = a.step(100, vec![], &BTreeSet::new(), &known);
        assert!(f.is_empty(), "clients known at the first run are not new");
        assert_eq!(a.seen_until, 50);
        let newc = vec![("AA:AA:AA:AA:AA:02".to_string(), "Phone".to_string(), 120)];
        let (f, _) = a.step(130, vec![], &BTreeSet::new(), &newc);
        assert_eq!(f.len(), 1);
        assert_eq!(a.history[0].text, "New client joined: Phone (AA:AA:AA:AA:AA:02)");
        assert_eq!(a.history[0].cleared, Some(120));
        assert_eq!(a.seen_until, 120);
        // a flood is capped
        let flood: Vec<_> = (0..50).map(|i| (format!("M{i}"), String::new(), 200 + i)).collect();
        assert_eq!(a.step(300, vec![], &BTreeSet::new(), &flood).0.len(), MAX_NEW_CLIENTS);
    }

    #[test]
    fn history_is_capped() {
        let mut a = Alerts { seen_until: 1, ..Default::default() };
        for i in 0..(MAX_HISTORY as u64 + 20) {
            a.record(i, "cpuHigh", "x".into(), None, None);
        }
        assert_eq!(a.history.len(), MAX_HISTORY);
        assert_eq!(a.history.back().unwrap().id, MAX_HISTORY as u64 + 20);
    }

    #[test]
    fn quiet_hours() {
        let q = Quiet { enabled: true, start: 23, end: 7, offset_minutes: 60, critical_bypass: true };
        // 22:30 UTC = 23:30 local (+01:00): quiet. 06:30 UTC = 07:30 local: not quiet.
        assert!(q.active(22 * 3600 + 1800));
        assert!(!q.active(6 * 3600 + 1800));
        assert!(q.active(3 * 3600));
        assert!(!q.active(12 * 3600));
        let day = Quiet { start: 9, end: 17, offset_minutes: -300, ..q.clone() };
        assert!(day.active(15 * 3600)); // 10:00 local
        assert!(!day.active(23 * 3600)); // 18:00 local
        assert!(!Quiet { enabled: false, ..q.clone() }.active(23 * 3600));
        assert!(!Quiet { start: 5, end: 5, ..q.clone() }.active(5 * 3600));
        assert!(Quiet { start: 24, ..q.clone() }.validate().is_err());
        assert!(Quiet { offset_minutes: 900, ..q }.validate().is_err());
    }

    /// A local "ntfy" that counts the POSTs it gets.
    fn counting_server() -> (u16, Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let n2 = n.clone();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let mut s = s;
                s.set_read_timeout(Some(Duration::from_millis(300))).ok();
                let mut b = [0u8; 4096];
                let _ = s.read(&mut b);
                n2.fetch_add(1, Ordering::SeqCst);
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
            }
        });
        (port, n)
    }

    #[test]
    fn quiet_hours_suppress_notifications_but_still_record() {
        let app = new_app();
        let (port, hits) = counting_server();
        {
            let mut st = lock(&app.store);
            let al = &mut st.state.alerts;
            al.channels.ntfy = Ntfy { enabled: true, min_severity: Severity::Info, server: format!("http://127.0.0.1:{port}"), topic: "home".into(), token: None };
            al.quiet = Quiet { enabled: true, start: 0, end: 0, ..Default::default() };
            al.seen_until = 1;
        }
        let fire = |key: &str, rule: &'static str| {
            let mut st = lock(&app.store);
            let all: BTreeSet<&str> = ["cpuHigh", "wanDown"].into();
            let (ids, _) = st.state.alerts.step(now(), vec![Cond { key: key.into(), rule, text: "t".into(), detail: None }], &all, &[]);
            drop(st);
            notify(&app, &ids);
            ids[0]
        };
        // not quiet (start == end): sent
        let id = fire("cpuHigh", "cpuHigh");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(lock(&app.store).state.alerts.history.iter().find(|a| a.id == id).unwrap().sent, ["ntfy"]);
        // quiet all day except the bypass: a warning is recorded but held, a critical goes out
        let h = (now() % 86_400 / 3600) as u8;
        lock(&app.store).state.alerts.quiet = Quiet { enabled: true, start: h, end: (h + 2) % 24, offset_minutes: 0, critical_bypass: true };
        lock(&app.store).state.alerts.active.clear();
        let id = fire("cpuHigh", "cpuHigh");
        let st = lock(&app.store);
        let a = st.state.alerts.history.iter().find(|a| a.id == id).unwrap();
        assert!(a.quiet && a.sent.is_empty());
        drop(st);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        let id = fire("wanDown", "wanDown");
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        assert!(!lock(&app.store).state.alerts.history.iter().find(|a| a.id == id).unwrap().quiet);
        // min severity per channel: a warning below "critical" is not sent
        lock(&app.store).state.alerts.quiet.enabled = false;
        lock(&app.store).state.alerts.channels.ntfy.min_severity = Severity::Critical;
        lock(&app.store).state.alerts.active.clear();
        fire("cpuHigh", "cpuHigh");
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn secrets_are_sealed_and_never_returned() {
        let app = new_app();
        let (c, csrf) = admin_session(&app);
        let body = r#"{"ntfy":{"enabled":true,"minSeverity":"warning","server":"https://ntfy.example.com","topic":"home-alerts","token":"tk_supersecret1"},
            "webhook":{"enabled":false,"minSeverity":"info","url":"https://hooks.example.com/api/webhook/hook-secret-id"},
            "telegram":{"enabled":true,"minSeverity":"critical","chatId":"-1001234","token":"123456:AAbbCC_secret-token"}}"#;
        let r = handle(&app, with_session(req("PUT", "/api/v1/alerts/channels", body), &c, &csrf));
        assert_eq!(r.status, 200, "{:?}", json_of(&r));
        let got = handle(&app, with_session(req("GET", "/api/v1/alerts", ""), &c, &csrf));
        let txt = serde_json::to_string(&json_of(&got)).unwrap();
        for secret in ["tk_supersecret1", "hook-secret-id", "AAbbCC_secret-token"] {
            assert!(!txt.contains(secret), "{secret} leaked in GET");
            assert!(!serde_json::to_string(&json_of(&r)).unwrap().contains(secret));
        }
        let v = json_of(&got);
        assert_eq!(v["channels"]["ntfy"]["token"]["set"], true);
        assert_eq!(v["channels"]["telegram"]["token"]["set"], true);
        assert_eq!(v["channels"]["webhook"]["url"]["host"], "hooks.example.com");
        // the state file holds them sealed
        let st = lock(&app.store);
        let raw = serde_json::to_string(&st.state).unwrap();
        assert!(!raw.contains("tk_supersecret1") && !raw.contains("hook-secret-id") && !raw.contains("AAbbCC_secret-token"));
        assert_eq!(st.sealer.open_str("alert-secret:ntfy", st.state.alerts.channels.ntfy.token.as_ref().unwrap()).as_deref(), Some("tk_supersecret1"));
        drop(st);
        // omitting a secret keeps it, "" clears it
        let keep = r#"{"ntfy":{"enabled":true,"minSeverity":"info","server":"https://ntfy.example.com","topic":"home-alerts"},
            "webhook":{"enabled":false,"minSeverity":"info","url":""},
            "telegram":{"enabled":false,"minSeverity":"critical","chatId":"-1001234","token":null}}"#;
        assert_eq!(handle(&app, with_session(req("PUT", "/api/v1/alerts/channels", keep), &c, &csrf)).status, 200);
        let st = lock(&app.store);
        assert!(st.state.alerts.channels.ntfy.token.is_some() && st.state.alerts.channels.telegram.token.is_some());
        assert!(st.state.alerts.channels.webhook.url.is_none());
    }

    #[test]
    fn channel_validation_and_admin_only() {
        let app = new_app();
        let (c, csrf) = admin_session(&app);
        let put = |b: &str| handle(&app, with_session(req("PUT", "/api/v1/alerts/channels", b), &c, &csrf)).status;
        let mk = |ntfy_server: &str, hook: &str| format!(r#"{{"ntfy":{{"enabled":false,"minSeverity":"info","server":"{ntfy_server}","topic":""}},"webhook":{{"enabled":false,"minSeverity":"info","url":"{hook}"}},"telegram":{{"enabled":false,"minSeverity":"info","chatId":""}}}}"#);
        assert_eq!(put(&mk("file:///etc/passwd", "")), 400);
        assert_eq!(put(&mk("http://ntfy.sh", "")), 400, "plain http to a public host");
        assert_eq!(put(&mk("", "http://example.com/hook")), 400);
        assert_eq!(put(&mk("", &format!("https://example.com/{}", "a".repeat(600)))), 400);
        assert_eq!(put(&mk("http://192.168.88.5:2586", "http://127.0.0.1:8123/api/webhook/x")), 200);
        assert_eq!(put(r#"{"ntfy":{"enabled":true,"minSeverity":"info","server":"","topic":""},"webhook":{"enabled":false,"minSeverity":"info"},"telegram":{"enabled":false,"minSeverity":"info","chatId":""}}"#), 400, "enabled without a server");
        assert_eq!(put(r#"{"ntfy":{"enabled":false,"minSeverity":"info","server":"","topic":"","evil":1},"webhook":{"enabled":false,"minSeverity":"info"},"telegram":{"enabled":false,"minSeverity":"info","chatId":""}}"#), 400, "unknown field");
        assert_eq!(put(r#"{"ntfy":{"enabled":false,"minSeverity":"info","server":"","topic":""},"webhook":{"enabled":false,"minSeverity":"info"},"telegram":{"enabled":false,"minSeverity":"info","chatId":"x","token":"nope"}}"#), 400);
        // rules validation
        let rules = |cpu: &str| format!(r#"{{"rules":{{"cpuHigh":{{"enabled":true,"severity":"warning","threshold":{cpu}}}}},"quiet":{{"enabled":false,"start":23,"end":7,"offsetMinutes":60,"criticalBypass":true}}}}"#);
        assert_eq!(handle(&app, with_session(req("PUT", "/api/v1/alerts/rules", &rules("150")), &c, &csrf)).status, 400);
        assert_eq!(handle(&app, with_session(req("PUT", "/api/v1/alerts/rules", &rules("80")), &c, &csrf)).status, 200);
        assert_eq!(lock(&app.store).state.alerts.rules.cpu_high.threshold, Some(80.0));
        assert_eq!(lock(&app.store).state.alerts.quiet.offset_minutes, 60);
        // test needs a configured channel
        assert_eq!(handle(&app, with_session(req("POST", "/api/v1/alerts/test", r#"{"channel":"telegram"}"#), &c, &csrf)).status, 409);
        assert_eq!(handle(&app, with_session(req("POST", "/api/v1/alerts/test", r#"{"channel":"email"}"#), &c, &csrf)).status, 400);
        // viewers can read and acknowledge, not configure
        lock(&app.store).state.admins.push(auth::new_admin("viewer", "a long viewer password", Role::Viewer).unwrap());
        let r = handle(&app, req("POST", "/api/v1/login", r#"{"name":"viewer","password":"a long viewer password"}"#));
        let vc = r.headers.iter().find(|(k, _)| *k == "set-cookie").unwrap().1.split(';').next().unwrap().split_once('=').unwrap().1.to_string();
        let vcsrf = json_of(&r)["csrf"].as_str().unwrap().to_string();
        assert_eq!(handle(&app, with_session(req("PUT", "/api/v1/alerts/channels", &mk("", "")), &vc, &vcsrf)).status, 403);
        assert_eq!(handle(&app, with_session(req("PUT", "/api/v1/alerts/rules", &rules("80")), &vc, &vcsrf)).status, 403);
        assert_eq!(handle(&app, with_session(req("POST", "/api/v1/alerts/test", r#"{"channel":"ntfy"}"#), &vc, &vcsrf)).status, 403);
        let v = json_of(&handle(&app, with_session(req("GET", "/api/v1/alerts", ""), &vc, &vcsrf)));
        assert_eq!(v["rules"]["cpuHigh"]["threshold"], 80.0);
    }

    #[test]
    fn acknowledge() {
        let app = new_app();
        let (c, csrf) = admin_session(&app);
        let id = lock(&app.store).state.alerts.record(10, "wanDown", "down".into(), None, None);
        let v = json_of(&handle(&app, with_session(req("GET", "/api/v1/alerts", ""), &c, &csrf)));
        assert_eq!(v["unacked"], 1);
        assert_eq!(handle(&app, with_session(req("POST", "/api/v1/alerts/ack", &format!(r#"{{"id":{id}}}"#)), &c, &csrf)).status, 200);
        assert_eq!(handle(&app, with_session(req("POST", "/api/v1/alerts/ack", r#"{"id":999}"#), &c, &csrf)).status, 404);
        let v = json_of(&handle(&app, with_session(req("GET", "/api/v1/alerts", ""), &c, &csrf)));
        assert_eq!(v["unacked"], 0);
        assert_eq!(v["history"][0]["ack"], true);
        assert_eq!(v["history"][0]["ackBy"], "admin");
        // persisted with the state
        let dir = lock(&app.store).dir();
        let again = crate::store::Store::open(&dir).unwrap();
        assert!(again.state.alerts.history[0].ack);
    }

    #[test]
    fn old_state_with_null_alerts_still_loads() {
        let s: crate::store::State = serde_json::from_str(r#"{"admins":[],"alerts":null,"site":null}"#).unwrap();
        assert_eq!(s.alerts.rules, Rules::default());
        let s: crate::store::State = serde_json::from_str(r#"{"admins":[]}"#).unwrap();
        assert!(s.alerts.history.is_empty());
    }

    #[test]
    fn tick_over_live_history_and_snapshot() {
        let app = new_app();
        {
            let mut s = app.snap.write().unwrap();
            s.at = now();
            s.data = Some(Arc::new(sample()));
        }
        // no router configured and no demo: nothing happens
        tick(&app);
        assert!(lock(&app.store).state.alerts.history.is_empty());
        let mut a = Arc::try_unwrap(app).ok().unwrap();
        a.demo = Some(Arc::new(sample()));
        let app = Arc::new(a);
        {
            let mut s = app.snap.write().unwrap();
            s.at = now();
            s.data = Some(Arc::new(sample()));
        }
        lock(&app.live.history).seen.insert("AA:00:00:00:00:01".into(), (now() - 100, now(), "wlan1".into()));
        tick(&app); // first run: baseline only
        assert!(lock(&app.store).state.alerts.history.is_empty());
        lock(&app.live.history).seen.insert("AA:00:00:00:00:02".into(), (now() + 1, now() + 1, "wlan1".into()));
        *lock(&app.live.wan_up) = Some(false);
        tick(&app);
        let st = lock(&app.store);
        let rules: Vec<&str> = st.state.alerts.history.iter().map(|a| a.rule.as_str()).collect();
        assert_eq!(rules, ["wanDown", "newClient"]);
    }
}
