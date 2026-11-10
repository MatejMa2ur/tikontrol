//! Control plane / System intents: site settings, SNMP, remote syslog, traffic-flow export, update
//! channel, LEDs, and the one-shot device actions (reboot, RouterBOOT upgrade, locate, config export).
//!
//! Settings read their current values first so the inverse restores exactly what was there.
//! One-shot actions have nothing to restore: they are `revertible: false` and say so in a warning.
//! SNMP community strings are secrets: tikontrol never reads or sets them.

use crate::apply::{dns_name, ipv4, plan, ros_name, yn, Op, Plan, Risk};
use crate::ros::cmd::{valid_value, Cmd};
use crate::ros::conn::{Router, Row};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SystemIntent {
    SiteSettings {
        #[serde(default)]
        identity: Option<String>,
        #[serde(default)]
        time_zone: Option<String>,
        #[serde(default)]
        ntp_enabled: Option<bool>,
        #[serde(default)]
        ntp_servers: Option<Vec<String>>,
    },
    SetSnmp {
        enabled: bool,
        #[serde(default)]
        contact: Option<String>,
        #[serde(default)]
        location: Option<String>,
    },
    RemoteSyslog {
        address: String,
        #[serde(default = "syslog_port")]
        port: u16,
        topics: Vec<String>,
    },
    RemoveRemoteSyslog {},
    SetTrafficFlow {
        enabled: bool,
        #[serde(default)]
        address: Option<String>,
        #[serde(default = "flow_port")]
        port: u16,
        #[serde(default = "flow_version")]
        version: String,
    },
    SetUpdateChannel { channel: String },
    SetLeds { all_leds_off: String },
    Reboot {},
    RouterbootUpgrade {},
    Locate { interface: String },
    ExportConfig { name: String },
    RemoveBackupFile { name: String },
    CheckUpdates {},
    InstallUpdate {},
    BackupSchedule { enabled: bool, #[serde(default = "week")] interval: String },
    /// A maintenance window: check (and optionally install) RouterOS updates daily or weekly.
    AutoUpdate {
        enabled: bool,
        #[serde(default = "daily")]
        day: String,
        #[serde(default = "three")]
        hour: u8,
        #[serde(default)]
        install: bool,
    },
    SaveBackup { name: String },
    RestoreBackup { name: String },
}

fn week() -> String {
    "7d".into()
}
fn daily() -> String {
    "daily".into()
}
fn three() -> u8 {
    3
}

pub const UPDATE_JOB: &str = "tik-auto-update";
const DAYS: &[&str] = &["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/// Weekday of a RouterOS date (both formats), 0 = Monday.
fn weekday(date: &str) -> Option<i64> {
    // `later` validates and normalises the date; 2000-01-03 was a Monday
    let (iso, _) = crate::apply::later(date, "00:00:00", 0)?;
    let to_days = |d: &str| -> Option<i64> {
        let (y, m, dd) = if let Some((y, rest)) = d.split_once('-') {
            let (m, dd) = rest.split_once('-')?;
            (y.parse::<i64>().ok()?, m.parse::<i64>().ok()?, dd.parse::<i64>().ok()?)
        } else {
            const MON: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
            let mut it = d.split('/');
            let mon = it.next()?.to_ascii_lowercase();
            let m = MON.iter().position(|x| *x == mon)? as i64 + 1;
            let dd = it.next()?.parse().ok()?;
            (it.next()?.parse().ok()?, m, dd)
        };
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + dd - 1;
        Some(era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy)
    };
    Some((to_days(&iso)? - to_days("2000-01-03")?).rem_euclid(7))
}

/// The first start (date, time) at `hour`:00 on `day` ("daily" or mon..sun) after the router's clock.
pub fn next_window(date: &str, time: &str, day: &str, hour: u8) -> Option<(String, String)> {
    let mut t = time.split(':').map(|x| x.parse::<i64>().ok());
    let now = t.next()?? * 3600 + t.next()?? * 60 + t.next()??;
    let today = weekday(date)?;
    let want = if day == "daily" { None } else { Some(DAYS.iter().position(|d| *d == day)? as i64) };
    (0..8).map(|k| (k, k * 86_400 + i64::from(hour) * 3600)).find(|(k, at)| *at > now && want.is_none_or(|w| (today + k) % 7 == w)).and_then(|(_, at)| crate::apply::later(date, "00:00:00", at as u64))
}

/// tikontrol's scheduled export: a readable script without secrets, overwritten each run.
pub const BACKUP_JOB: &str = "tik-backup";
const INTERVALS: &[&str] = &["1d", "7d", "30d"];

fn syslog_port() -> u16 {
    514
}
fn flow_port() -> u16 {
    2055
}
fn flow_version() -> String {
    "9".into()
}

pub const KINDS: &[&str] = &[
    "site-settings",
    "set-snmp",
    "remote-syslog",
    "remove-remote-syslog",
    "set-traffic-flow",
    "set-update-channel",
    "set-leds",
    "reboot",
    "routerboot-upgrade",
    "locate",
    "export-config",
    "remove-backup-file",
    "check-updates",
    "install-update",
    "backup-schedule",
    "auto-update",
    "save-backup",
    "restore-backup",
];

/// Logging action (and rule `action=`) that marks our remote syslog. Logging menus don't all take
/// comments, so the action name is the tag.
pub const SYSLOG_ACTION: &str = "tikremote"; // RouterOS: letters and digits only
const TOPICS: &[&str] = &["info", "warning", "error", "critical", "account", "system", "firewall", "dhcp", "wireless", "wifi", "interface", "script"];
const CHANNELS: &[&str] = &["stable", "long-term", "testing", "development"];
const LEDS: &[&str] = &["never", "immediate", "after-1min", "after-1h"];

pub fn compile(v: &serde_json::Value, r: &mut dyn Router) -> Option<Result<Plan, String>> {
    let kind = v.get("kind")?.as_str()?;
    if !KINDS.contains(&kind) {
        return None;
    }
    Some(serde_json::from_value::<SystemIntent>(v.clone()).map_err(|e| format!("invalid {kind} intent: {e}")).and_then(|i| build(&i, r)))
}

fn set(path: &'static str, args: Vec<(&'static str, String)>) -> Op {
    Op { path, verb: "set", find: None, args }
}

fn cmd(path: &'static str, verb: &'static str, args: Vec<(&'static str, String)>) -> Op {
    Op { path, verb, find: None, args }
}

fn first(r: &mut dyn Router, path: &str, keys: &[&str]) -> Result<Row, String> {
    Ok(r.run(&Cmd::new(&format!("{path}/print")).proplist(keys))?.into_iter().next().unwrap_or_default())
}

fn truthy(row: &Row, k: &str) -> bool {
    row.get(k).is_some_and(|v| v == "true" || v == "yes")
}

fn one_shot(title: String, risk: Risk, ops: Vec<Op>, warnings: Vec<String>) -> Plan {
    Plan { title, risk, warnings, ops, undo: vec![], revertible: false }
}

/// Free text for SNMP contact/location: printable, short, no quotes needed but allowed.
fn text(s: &str, what: &str) -> Result<String, String> {
    let s = s.trim();
    let ok = s.len() <= 128 && valid_value(s) && s.chars().all(|c| !c.is_control());
    ok.then(|| s.to_string()).ok_or_else(|| format!("invalid {what} (at most 128 printable characters)"))
}

fn time_zone(s: &str) -> Result<String, String> {
    let s = s.trim();
    let ok = !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b)) && !s.starts_with(['/', '-']);
    ok.then(|| s.to_string()).ok_or_else(|| "invalid time zone (e.g. Europe/Bratislava)".into())
}

/// An IPv4 address or a DNS name (NTP servers, diagnostics targets).
pub fn host(s: &str) -> Result<String, String> {
    let s = s.trim();
    if let Ok(ip) = s.parse::<std::net::Ipv4Addr>() {
        return Ok(ip.to_string());
    }
    if s.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return Err(format!("invalid address {s:?}"));
    }
    dns_name(s).map_err(|_| format!("invalid host {s:?} (IPv4 address or DNS name)"))
}

fn remote_target(s: &str) -> Result<std::net::Ipv4Addr, String> {
    let ip = ipv4(s)?;
    if ip.is_unspecified() || ip.is_loopback() || ip.is_broadcast() || ip.is_multicast() {
        return Err("the target must be a normal unicast address".into());
    }
    Ok(ip)
}

/// Names for files tikontrol creates: `tik-` + letters, digits, '-' and '_'.
fn file_stem(s: &str) -> Result<String, String> {
    let s = s.trim();
    let ok = !s.is_empty() && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b));
    ok.then(|| s.to_string()).ok_or_else(|| "invalid file name (letters, digits, - and _, at most 40)".into())
}

fn build(i: &SystemIntent, r: &mut dyn Router) -> Result<Plan, String> {
    use SystemIntent::*;
    Ok(match i {
        SiteSettings { identity, time_zone: tz, ntp_enabled, ntp_servers } => {
            let (mut ops, mut undo, mut what) = (vec![], vec![], vec![]);
            if let Some(n) = identity {
                let n = ros_name(n)?;
                let was = first(r, "/system/identity", &["name"])?.get("name").cloned().unwrap_or_default();
                ops.push(set("/system/identity", vec![("name", n.clone())]));
                undo.push(set("/system/identity", vec![("name", was)]));
                what.push(format!("name {n}"));
            }
            if let Some(z) = tz {
                let z = time_zone(z)?;
                let was = first(r, "/system/clock", &["time-zone-name", "time-zone-autodetect"])?;
                ops.push(set("/system/clock", vec![("time-zone-autodetect", "no".into()), ("time-zone-name", z.clone())]));
                let mut back = vec![("time-zone-name", was.get("time-zone-name").cloned().unwrap_or_else(|| "manual".into()))];
                if was.contains_key("time-zone-autodetect") {
                    back.insert(0, ("time-zone-autodetect", yn(truthy(&was, "time-zone-autodetect"))));
                }
                undo.push(set("/system/clock", back));
                what.push(format!("time zone {z}"));
            }
            if ntp_enabled.is_some() || ntp_servers.is_some() {
                let was = first(r, "/system/ntp/client", &["enabled", "servers"])?;
                let mut args = vec![];
                let mut back = vec![];
                if let Some(e) = ntp_enabled {
                    args.push(("enabled", yn(*e)));
                    back.push(("enabled", yn(truthy(&was, "enabled"))));
                    what.push(format!("NTP {}", if *e { "on" } else { "off" }));
                }
                if let Some(list) = ntp_servers {
                    if list.len() > 4 {
                        return Err("at most 4 NTP servers".into());
                    }
                    let list = list.iter().map(|s| host(s)).collect::<Result<Vec<_>, _>>()?;
                    args.push(("servers", list.join(",")));
                    back.push(("servers", was.get("servers").cloned().unwrap_or_default()));
                    what.push("NTP servers".into());
                }
                ops.push(set("/system/ntp/client", args));
                undo.push(set("/system/ntp/client", back));
            }
            if ops.is_empty() {
                return Err("nothing to change".into());
            }
            plan(format!("Site settings: {}", what.join(", ")), Risk::Low, ops, undo)
        }
        SetSnmp { enabled, contact, location } => {
            let was = first(r, "/snmp", &["enabled", "contact", "location"])?;
            let mut args = vec![("enabled", yn(*enabled))];
            let mut back = vec![("enabled", yn(truthy(&was, "enabled")))];
            for (k, v) in [("contact", contact), ("location", location)] {
                if let Some(v) = v {
                    args.push((k, text(v, k)?));
                    back.push((k, was.get(k).cloned().unwrap_or_default()));
                }
            }
            let mut p = plan(format!("{} SNMP", if *enabled { "Enable" } else { "Disable" }), if *enabled { Risk::Medium } else { Risk::Low }, vec![set("/snmp", args)], vec![set("/snmp", back)]);
            if *enabled {
                p.warnings.push("SNMP v1/v2c communities travel in plain text. tikontrol never reads or sets community secrets: in WinBox, replace the default 'public' community and limit its addresses to your monitoring host.".into());
                p.warnings.push("Make sure the input firewall drops SNMP (udp 161) from the internet.".into());
            }
            p
        }
        RemoteSyslog { address, port, topics } => {
            let ip = remote_target(address)?;
            if *port == 0 {
                return Err("port must be 1-65535".into());
            }
            if topics.is_empty() || topics.len() > 8 || topics.iter().any(|t| !TOPICS.contains(&t.as_str())) {
                return Err(format!("topics: 1-8 of {}", TOPICS.join(", ")));
            }
            let exists = r.run(&Cmd::new("/system/logging/action/print").proplist(&["name"]).query("name", SYSLOG_ACTION))?;
            if !exists.is_empty() {
                return Err("remote syslog is already set up by tikontrol: remove it first".into());
            }
            let mut ops = vec![Op::add("/system/logging/action", vec![("name", SYSLOG_ACTION.into()), ("target", "remote".into()), ("remote", ip.to_string()), ("remote-port", port.to_string())])];
            ops.extend(topics.iter().map(|t| Op::add("/system/logging", vec![("topics", t.clone()), ("action", SYSLOG_ACTION.into())])));
            let undo = vec![Op::on("/system/logging", "remove", "action", SYSLOG_ACTION, vec![]), Op::on("/system/logging/action", "remove", "name", SYSLOG_ACTION, vec![])];
            let mut p = plan(format!("Send the router log ({}) to {ip}:{port}", topics.join(", ")), Risk::Low, ops, undo);
            p.warnings.push("Syslog is unencrypted UDP and the log contains device names and addresses: only send it to a host on your own network.".into());
            p
        }
        RemoveRemoteSyslog {} => {
            let a = r.run(&Cmd::new("/system/logging/action/print").proplist(&["remote", "remote-port"]).query("name", SYSLOG_ACTION))?;
            let a = a.first().ok_or("only remote syslog set up by tikontrol can be removed here")?;
            let rules = r.run(&Cmd::new("/system/logging/print").proplist(&["topics"]).query("action", SYSLOG_ACTION))?;
            let ops = vec![Op::on("/system/logging", "remove", "action", SYSLOG_ACTION, vec![]), Op::on("/system/logging/action", "remove", "name", SYSLOG_ACTION, vec![])];
            let mut undo = vec![Op::add("/system/logging/action", vec![("name", SYSLOG_ACTION.into()), ("target", "remote".into()), ("remote", a.get("remote").cloned().unwrap_or_default()), ("remote-port", a.get("remote-port").cloned().unwrap_or_else(|| "514".into()))])];
            undo.extend(rules.iter().map(|x| Op::add("/system/logging", vec![("topics", x.get("topics").cloned().unwrap_or_else(|| "info".into())), ("action", SYSLOG_ACTION.into())])));
            plan("Stop remote syslog".into(), Risk::Low, ops, undo)
        }
        SetTrafficFlow { enabled, address, port, version } => {
            let was = first(r, "/ip/traffic-flow", &["enabled"])?;
            let mut ops = vec![set("/ip/traffic-flow", vec![("enabled", yn(*enabled))])];
            let mut undo = vec![set("/ip/traffic-flow", vec![("enabled", yn(truthy(&was, "enabled")))])];
            let mut title = format!("{} traffic-flow (NetFlow/IPFIX)", if *enabled { "Enable" } else { "Disable" });
            if let Some(a) = address {
                if !enabled {
                    return Err("a collector can only be added while enabling".into());
                }
                let ip = remote_target(a)?;
                if *port == 0 {
                    return Err("port must be 1-65535".into());
                }
                if !matches!(version.as_str(), "5" | "9" | "ipfix") {
                    return Err("version must be 5, 9 or ipfix".into());
                }
                // targets take no comment: the collector address is the key, so one target per address
                if !r.run(&Cmd::new("/ip/traffic-flow/target/print").proplist(&["port"]).query("dst-address", &ip.to_string()))?.is_empty() {
                    return Err(format!("{ip} is already a traffic-flow collector"));
                }
                ops.push(Op::add("/ip/traffic-flow/target", vec![("dst-address", ip.to_string()), ("port", port.to_string()), ("version", version.clone())]));
                undo.insert(0, Op::on("/ip/traffic-flow/target", "remove", "dst-address", &ip.to_string(), vec![]));
                title.push_str(&format!(" to {ip}:{port}"));
            }
            let mut p = plan(title, Risk::Low, ops, undo);
            if *enabled {
                p.warnings.push("Flow records show who talked to whom: only export them to a collector you control. Traffic-flow costs some router CPU.".into());
            }
            p
        }
        SetUpdateChannel { channel } => {
            if !CHANNELS.contains(&channel.as_str()) {
                return Err(format!("channel must be one of {}", CHANNELS.join(", ")));
            }
            let was = first(r, "/system/package/update", &["channel"])?.get("channel").cloned().unwrap_or_else(|| "stable".into());
            let mut p = plan(format!("Update channel: {channel}"), Risk::Low, vec![set("/system/package/update", vec![("channel", channel.clone())])], vec![set("/system/package/update", vec![("channel", was)])]);
            if matches!(channel.as_str(), "testing" | "development") {
                p.warnings.push(format!("The {channel} channel gets pre-release RouterOS builds. Use stable or long-term on a router others depend on."));
            }
            p.warnings.push("This only picks the channel: nothing is downloaded or installed.".into());
            p
        }
        SetLeds { all_leds_off } => {
            if !LEDS.contains(&all_leds_off.as_str()) {
                return Err(format!("all_leds_off must be one of {}", LEDS.join(", ")));
            }
            let was = first(r, "/system/leds/settings", &["all-leds-off"])?.get("all-leds-off").cloned().unwrap_or_else(|| "never".into());
            let title = if all_leds_off == "never" { "Turn the LEDs on".to_string() } else { format!("Turn the LEDs off ({all_leds_off})") };
            plan(title, Risk::Low, vec![set("/system/leds/settings", vec![("all-leds-off", all_leds_off.clone())])], vec![set("/system/leds/settings", vec![("all-leds-off", was)])])
        }
        Reboot {} => one_shot(
            "Restart the router".into(),
            Risk::High,
            vec![cmd("/system", "reboot", vec![])],
            vec![
                "High risk: the router restarts now. Internet, Wi-Fi and tikontrol drop for one to three minutes.".into(),
                "This cannot be undone or reverted automatically. Unsaved state (DHCP leases not yet written, connection tracking) is lost.".into(),
                "The writer user needs the reboot policy for this; without it the router refuses.".into(),
            ],
        ),
        RouterbootUpgrade {} => one_shot(
            "Upgrade RouterBOOT firmware".into(),
            Risk::High,
            vec![cmd("/system/routerboard", "upgrade", vec![])],
            vec![
                "High risk: writes new boot firmware. It takes effect at the next restart; do not cut power during that restart.".into(),
                "This cannot be reverted automatically. Upgrade RouterOS first: RouterBOOT should match the installed RouterOS version.".into(),
            ],
        ),
        Locate { interface } => {
            let ifn = ros_name(interface)?;
            one_shot(format!("Blink the {ifn} port LED"), Risk::Low, vec![Op::on("/interface/ethernet", "blink", "name", &ifn, vec![])], vec!["Blinks the port LED for a few seconds; nothing is changed.".into()])
        }
        ExportConfig { name } => {
            let file = format!("tik-{}", file_stem(name)?);
            // no show-sensitive: passwords and keys stay out of the file
            one_shot(
                format!("Save the configuration as {file}.rsc"),
                Risk::Low,
                vec![cmd("", "export", vec![("file", file)])],
                vec!["Saved on the router's storage as a readable script, without passwords and keys: a restore needs them entered again. Download it with WinBox or WebFig Files to keep a copy off the router.".into()],
            )
        }
        CheckUpdates {} => one_shot(
            "Check for RouterOS updates".into(),
            Risk::Low,
            vec![cmd("/system/package/update", "check-for-updates", vec![])],
            vec!["Asks MikroTik's update server for the newest version on the selected channel. Nothing is installed.".into()],
        ),
        InstallUpdate {} => {
            let u = first(r, "/system/package/update", &["installed-version", "latest-version", "status"])?;
            let (have, new) = (u.get("installed-version").cloned().unwrap_or_default(), u.get("latest-version").cloned().unwrap_or_default());
            if new.is_empty() || new == have {
                return Err("no newer RouterOS version is known: check for updates first".into());
            }
            one_shot(
                format!("Install RouterOS {new}"),
                Risk::High,
                vec![cmd("/system/package/update", "install", vec![])],
                vec![
                    format!("High risk: downloads RouterOS {new} (now {have}) and restarts the router to install it. Internet, Wi-Fi and tikontrol drop for a few minutes."),
                    "This cannot be reverted automatically. Going back needs a manual downgrade.".into(),
                    "Save a backup first, and don't cut power while it installs.".into(),
                ],
            )
        }
        BackupSchedule { enabled, interval } => {
            if !INTERVALS.contains(&interval.as_str()) {
                return Err(format!("interval must be one of {}", INTERVALS.join(", ")));
            }
            let path = "/system/scheduler";
            let have = r.run(&Cmd::new("/system/scheduler/print").proplist(&["interval", "start-date", "start-time"]).query("name", BACKUP_JOB))?;
            let job = |iv: &str, start: Vec<(&'static str, String)>| {
                let mut args = vec![("name", BACKUP_JOB.into()), ("interval", iv.to_string()), ("on-event", "/export file=tik-auto".into()), ("policy", "read,write,test".into()), ("comment", "tik: scheduled backup".into())];
                args.extend(start);
                Op::add(path, args)
            };
            let remove = Op::on(path, "remove", "name", BACKUP_JOB, vec![]);
            let was = have.first().map(|row| row.get("interval").cloned().unwrap_or_else(|| "7d".into()));
            // putting a stopped job back keeps its start, or RouterOS would start it "now" and shift every run
            let start: Vec<(&'static str, String)> = have.first().map(|row| ["start-date", "start-time"].into_iter().filter_map(|k| row.get(k).filter(|v| !v.is_empty()).map(|v| (k, v.clone()))).collect()).unwrap_or_default();
            match (enabled, was) {
                (true, None) => plan(format!("Back up every {interval}"), Risk::Low, vec![job(interval, vec![])], vec![remove]),
                (true, Some(iv)) => plan(format!("Back up every {interval}"), Risk::Low, vec![Op::on(path, "set", "name", BACKUP_JOB, vec![("interval", interval.clone())])], vec![Op::on(path, "set", "name", BACKUP_JOB, vec![("interval", iv)])]),
                (false, Some(iv)) => plan("Stop scheduled backups".into(), Risk::Low, vec![remove], vec![job(&iv, start)]),
                (false, None) => return Err("scheduled backups are already off".into()),
            }
        }
        AutoUpdate { enabled, day, hour, install } => {
            if day != "daily" && !DAYS.contains(&day.as_str()) {
                return Err(format!("day must be daily or one of {}", DAYS.join(", ")));
            }
            if *hour > 23 {
                return Err("hour must be 0-23".into());
            }
            let path = "/system/scheduler";
            let have = r.run(&Cmd::new("/system/scheduler/print").proplist(&["start-date", "start-time", "interval", "on-event", "policy"]).query("name", UPDATE_JOB))?;
            let remove = Op::on(path, "remove", "name", UPDATE_JOB, vec![]);
            let restore = |row: &Row| {
                let g = |k: &str| row.get(k).cloned().unwrap_or_default();
                Op::add(path, vec![("name", UPDATE_JOB.into()), ("start-date", g("start-date")), ("start-time", g("start-time")), ("interval", g("interval")), ("on-event", g("on-event")), ("policy", g("policy")), ("comment", "tik: update window".into())])
            };
            if !enabled {
                return match have.first() {
                    Some(row) => Ok(plan("Stop the update window".into(), Risk::Low, vec![remove], vec![restore(row)])),
                    None => Err("the update window is already off".into()),
                };
            }
            let clock = first(r, "/system/clock", &["date", "time"])?;
            let (date, time) = next_window(clock.get("date").map_or("", |s| s), clock.get("time").map_or("", |s| s), day, *hour).ok_or("cannot read the router clock")?;
            let check = "/system/package/update/check-for-updates once; :delay 20s";
            let event = if *install {
                format!("{check}; :if ([/system/package/update/get installed-version] != [/system/package/update/get latest-version]) do={{ :log warning \"tikontrol: installing a RouterOS update in the update window\"; /system/package/update/install }}")
            } else {
                format!("{check}; :log info \"tikontrol: checked for RouterOS updates\"")
            };
            let policy = if *install { "read,write,policy,test,reboot" } else { "read,write,test" };
            let interval = if day == "daily" { "1d" } else { "7d" };
            let job = Op::add(path, vec![("name", UPDATE_JOB.into()), ("start-date", date), ("start-time", time), ("interval", interval.into()), ("on-event", event), ("policy", policy.into()), ("comment", "tik: update window".into())]);
            let when = if day == "daily" { format!("daily at {hour:02}:00") } else { format!("every {day} at {hour:02}:00") };
            let undo = match have.first() {
                Some(row) => vec![remove.clone(), restore(row)],
                None => vec![remove.clone()],
            };
            let mut ops = if have.is_empty() { vec![] } else { vec![remove] };
            ops.push(job);
            let mut p = plan(if *install { format!("Install RouterOS updates {when}") } else { format!("Check for RouterOS updates {when}") }, if *install { Risk::High } else { Risk::Low }, ops, undo);
            p.warnings.push("Router time is used: check the router's clock and time zone.".into());
            if *install {
                p.warnings.push("High risk: in the window the router installs a newer version on its channel by itself and restarts. Internet and Wi-Fi drop for a few minutes; there is no automatic way back.".into());
                p.warnings.push("Keep the channel on stable or long-term, and keep a recent backup.".into());
            } else {
                p.warnings.push("Only checks: tikontrol's \"RouterOS update available\" alert tells you when there is something to install.".into());
            }
            p
        }
        SaveBackup { name } => {
            let file = format!("tik-{}", file_stem(name)?);
            one_shot(
                format!("Save a full backup as {file}.backup"),
                Risk::Low,
                vec![cmd("/system/backup", "save", vec![("name", file), ("dont-encrypt", "yes".into())])],
                vec![
                    "A full binary backup, for restoring this same router. It includes passwords and keys, so it stays on the router's storage: download it only to a place you trust.".into(),
                    "For moving settings to another router, use Save configuration (a readable script) instead.".into(),
                ],
            )
        }
        RestoreBackup { name } => {
            let n = name.trim();
            if n.is_empty() || n.len() > 80 || !n.ends_with(".backup") || !valid_value(n) || n.contains(['"', '\\']) {
                return Err("pick a .backup file stored on the router".into());
            }
            if r.run(&Cmd::new("/file/print").proplist(&["name"]).query("name", n))?.is_empty() {
                return Err("no such file".into());
            }
            one_shot(
                format!("Restore {n}"),
                Risk::High,
                vec![cmd("/system/backup", "load", vec![("name", n.to_string()), ("password", String::new())])],
                vec![
                    "High risk: replaces the whole configuration with the backup and restarts the router. Everything changed since the backup is lost.".into(),
                    "This cannot be reverted automatically. A backup from another router model can leave this one unreachable.".into(),
                ],
            )
        }
        RemoveBackupFile { name } => {
            let n = name.trim();
            let stem = n.strip_prefix("tik-").and_then(|s| s.strip_suffix(".rsc").or_else(|| s.strip_suffix(".backup")));
            let Some(stem) = stem else { return Err("only tikontrol's own files (tik-*.rsc, tik-*.backup) can be removed here".into()) };
            file_stem(stem)?;
            let found = r.run(&Cmd::new("/file/print").proplist(&["name"]).query("name", n))?;
            if found.is_empty() {
                return Err("no such file".into());
            }
            one_shot(format!("Delete {n}"), Risk::Low, vec![Op::on("/file", "remove", "name", n, vec![])], vec!["The file is deleted for good; this cannot be undone.".into()])
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply::compile_value;
    use crate::ros::queries::tests::{row, Fake};
    use serde_json::json;

    fn go(v: serde_json::Value, f: &mut Fake) -> Result<Plan, String> {
        compile_value(&v, f)
    }

    #[test]
    fn site_settings_restore_current_values() {
        let mut f = Fake::default();
        f.answers.insert("/system/identity/print".into(), Ok(vec![row(&[("name", "OldName")])]));
        f.answers.insert("/system/clock/print".into(), Ok(vec![row(&[("time-zone-name", "Europe/Prague"), ("time-zone-autodetect", "true")])]));
        f.answers.insert("/system/ntp/client/print".into(), Ok(vec![row(&[("enabled", "false"), ("servers", "")])]));
        let p = go(json!({"kind":"site-settings","identity":"Home Lab","time_zone":"Europe/Bratislava","ntp_enabled":true,"ntp_servers":["pool.ntp.org","192.0.2.123"]}), &mut f).unwrap();
        assert_eq!(p.cli(), [r#"/system/identity/set name="Home Lab""#, "/system/clock/set time-zone-autodetect=no time-zone-name=Europe/Bratislava", "/system/ntp/client/set enabled=yes servers=pool.ntp.org,192.0.2.123"]);
        assert_eq!(p.undo_cli(), ["/system/identity/set name=OldName", "/system/clock/set time-zone-autodetect=yes time-zone-name=Europe/Prague", r#"/system/ntp/client/set enabled=no servers="""#]);
        assert!(p.revertible);
    }

    #[test]
    fn invalid_input_is_rejected() {
        let bad = [
            json!({"kind":"site-settings"}),
            json!({"kind":"site-settings","identity":"x\"; /system/reboot"}),
            json!({"kind":"site-settings","time_zone":"../etc"}),
            json!({"kind":"site-settings","ntp_servers":["a b"]}),
            json!({"kind":"site-settings","ntp_servers":["999.1.1.1"]}),
            json!({"kind":"site-settings","evil":1}),
            json!({"kind":"set-snmp","enabled":true,"community":"public"}),
            json!({"kind":"remote-syslog","address":"127.0.0.1","topics":["info"]}),
            json!({"kind":"remote-syslog","address":"192.0.2.9","topics":["debug"]}),
            json!({"kind":"remote-syslog","address":"192.0.2.9","topics":[]}),
            json!({"kind":"set-traffic-flow","enabled":false,"address":"192.0.2.9"}),
            json!({"kind":"set-traffic-flow","enabled":true,"address":"192.0.2.9","version":"7"}),
            json!({"kind":"set-update-channel","channel":"nightly"}),
            json!({"kind":"set-leds","all_leds_off":"sometimes"}),
            json!({"kind":"export-config","name":"../x"}),
            json!({"kind":"remove-backup-file","name":"flash/user.rsc"}),
            json!({"kind":"remove-backup-file","name":"tik-a/b.rsc"}),
            json!({"kind":"reboot","now":true}),
        ];
        for b in bad {
            assert!(go(b.clone(), &mut Fake::default()).is_err(), "{b}");
        }
    }

    #[test]
    fn snmp_never_touches_communities_and_warns() {
        let mut f = Fake::default();
        f.answers.insert("/snmp/print".into(), Ok(vec![row(&[("enabled", "false"), ("contact", ""), ("location", "")])]));
        let p = go(json!({"kind":"set-snmp","enabled":true,"location":"Attic"}), &mut f).unwrap();
        assert_eq!(p.cli(), ["/snmp/set enabled=yes location=Attic"]);
        assert_eq!(p.undo_cli(), [r#"/snmp/set enabled=no location="""#]);
        assert!(p.warnings[0].contains("community"));
        assert!(f.seen.iter().flatten().all(|w| !w.contains("community")));
    }

    #[test]
    fn remote_syslog_is_tagged_and_removable() {
        let p = go(json!({"kind":"remote-syslog","address":"192.0.2.50","topics":["info","error"]}), &mut Fake::default()).unwrap();
        assert_eq!(p.cli()[0], "/system/logging/action/add name=tikremote target=remote remote=192.0.2.50 remote-port=514");
        assert_eq!(p.cli()[2], "/system/logging/add topics=error action=tikremote");
        assert_eq!(p.undo_cli(), [r#"/system/logging/remove [find action="tikremote"]"#, r#"/system/logging/action/remove [find name="tikremote"]"#]);
        let mut f = Fake::default();
        f.answers.insert("/system/logging/action/print".into(), Ok(vec![row(&[("name", "tikremote")])]));
        assert!(go(json!({"kind":"remote-syslog","address":"192.0.2.50","topics":["info"]}), &mut f).unwrap_err().contains("already"));
        f.answers.insert("/system/logging/action/print".into(), Ok(vec![row(&[("remote", "192.0.2.50"), ("remote-port", "514")])]));
        f.answers.insert("/system/logging/print".into(), Ok(vec![row(&[("topics", "info")])]));
        let p = go(json!({"kind":"remove-remote-syslog"}), &mut f).unwrap();
        assert_eq!(p.undo_cli()[1], "/system/logging/add topics=info action=tikremote");
        assert!(go(json!({"kind":"remove-remote-syslog"}), &mut Fake::default()).is_err());
    }

    #[test]
    fn traffic_flow_target_has_an_inverse() {
        let p = go(json!({"kind":"set-traffic-flow","enabled":true,"address":"192.0.2.60","version":"ipfix"}), &mut Fake::default()).unwrap();
        assert_eq!(p.cli(), ["/ip/traffic-flow/set enabled=yes", "/ip/traffic-flow/target/add dst-address=192.0.2.60 port=2055 version=ipfix"]);
        assert_eq!(p.undo_cli(), [r#"/ip/traffic-flow/target/remove [find dst-address="192.0.2.60"]"#, "/ip/traffic-flow/set enabled=no"]);
    }

    #[test]
    fn channel_and_leds_restore_previous() {
        let mut f = Fake::default();
        f.answers.insert("/system/package/update/print".into(), Ok(vec![row(&[("channel", "long-term")])]));
        f.answers.insert("/system/leds/settings/print".into(), Ok(vec![row(&[("all-leds-off", "never")])]));
        let p = go(json!({"kind":"set-update-channel","channel":"testing"}), &mut f).unwrap();
        assert_eq!(p.undo_cli(), ["/system/package/update/set channel=long-term"]);
        assert!(p.warnings[0].contains("pre-release"));
        let p = go(json!({"kind":"set-leds","all_leds_off":"immediate"}), &mut f).unwrap();
        assert_eq!(p.cli(), ["/system/leds/settings/set all-leds-off=immediate"]);
        assert_eq!(p.undo_cli(), ["/system/leds/settings/set all-leds-off=never"]);
    }

    #[test]
    fn update_window_starts_at_the_next_matching_time() {
        assert_eq!(weekday("2026-11-10"), Some(1)); // a Tuesday
        assert_eq!(weekday("nov/10/2026"), Some(1));
        assert_eq!(next_window("2026-11-10", "10:00:00", "daily", 3), Some(("2026-11-11".into(), "03:00:00".into())));
        assert_eq!(next_window("2026-11-10", "02:00:00", "daily", 3), Some(("2026-11-10".into(), "03:00:00".into())));
        assert_eq!(next_window("2026-11-10", "10:00:00", "sun", 4), Some(("2026-11-15".into(), "04:00:00".into())));
        assert_eq!(next_window("2026-11-10", "10:00:00", "tue", 4), Some(("2026-11-17".into(), "04:00:00".into())));
        assert_eq!(next_window("dec/31/2026", "23:30:00", "daily", 1), Some(("jan/01/2027".into(), "01:00:00".into())));
        assert_eq!(next_window("garbage", "10:00:00", "daily", 1), None);
    }

    #[test]
    fn update_window_intent() {
        let c = |v: serde_json::Value, f: &mut Fake| compile_value(&v, f);
        let mut f = Fake::default();
        f.answers.insert("/system/clock/print".into(), Ok(vec![row(&[("date", "2026-11-10"), ("time", "10:00:00")])]));
        let p = c(json!({"kind":"auto-update","enabled":true,"day":"sun","hour":4}), &mut f).unwrap();
        let cli = p.cli().join("\n");
        assert!(cli.contains("start-date=2026-11-15") && cli.contains("interval=7d") && !cli.contains("/install"), "{cli}");
        assert_eq!(p.risk, Risk::Low);
        assert_eq!(p.undo_cli(), [r#"/system/scheduler/remove [find name="tik-auto-update"]"#]);
        let p = c(json!({"kind":"auto-update","enabled":true,"install":true}), &mut f).unwrap();
        assert_eq!(p.risk, Risk::High);
        assert!(p.cli().join(" ").contains("/system/package/update/install"));
        assert!(c(json!({"kind":"auto-update","enabled":true,"day":"someday"}), &mut f).is_err());
        assert!(c(json!({"kind":"auto-update","enabled":true,"hour":24}), &mut f).is_err());
        assert!(c(json!({"kind":"auto-update","enabled":false}), &mut f).unwrap_err().contains("already off"));
    }

    #[test]
    fn one_shot_actions_are_not_revertible() {
        for (v, risk, cli) in [
            (json!({"kind":"reboot"}), Risk::High, "/system/reboot"),
            (json!({"kind":"routerboot-upgrade"}), Risk::High, "/system/routerboard/upgrade"),
            (json!({"kind":"locate","interface":"ether1"}), Risk::Low, r#"/interface/ethernet/blink [find name="ether1"]"#),
            (json!({"kind":"export-config","name":"2026-01-15"}), Risk::Low, "/export file=tik-2026-01-15"),
        ] {
            let p = go(v.clone(), &mut Fake::default()).unwrap();
            assert!(!p.revertible && p.undo.is_empty(), "{v}");
            assert_eq!(p.risk, risk, "{v}");
            assert_eq!(p.cli(), [cli]);
            assert!(!p.warnings.is_empty());
            assert!(!p.cli()[0].contains("show-sensitive"));
        }
    }

    #[test]
    fn only_own_backup_files_can_be_removed() {
        let mut f = Fake::default();
        f.answers.insert("/file/print".into(), Ok(vec![row(&[("name", "tik-2026-01-15.rsc")])]));
        let p = go(json!({"kind":"remove-backup-file","name":"tik-2026-01-15.rsc"}), &mut f).unwrap();
        assert_eq!(p.cli(), [r#"/file/remove [find name="tik-2026-01-15.rsc"]"#]);
        assert!(go(json!({"kind":"remove-backup-file","name":"tik-x.rsc"}), &mut Fake::default()).unwrap_err().contains("no such"));
        assert!(go(json!({"kind":"remove-backup-file","name":"auto-before-reset.backup"}), &mut f).is_err());
    }

    #[test]
    fn other_kinds_fall_through() {
        assert!(compile(&json!({"kind":"block-client","mac":"AA:BB:CC:DD:EE:FF"}), &mut Fake::default()).is_none());
    }
}
