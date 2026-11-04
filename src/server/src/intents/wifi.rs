//! Wi-Fi settings (docs/FEATURES.md "WiFi settings"). Works on the legacy `/interface/wireless` package
//! (with `security-profiles`) and on the new `/interface/wifi` package, picked per request by `package`.
//!
//! Passphrases: tikontrol never reads them (the reader has no `sensitive` policy), so an inverse can't
//! restore an old one. A passphrase change is still armed, but its inverse restores only the non-secret
//! fields, and the plan says so. Secrets never reach the inverse, the revert script or the CLI text.

use crate::apply::{norm_mac, plan, ros_name, yn, Op, Plan, Risk, SECRET_KEYS};
use crate::ros::cmd::{valid_value, Cmd};
use crate::ros::conn::{Router, Row};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Pkg {
    #[default]
    Wireless,
    Wifi,
}

impl Pkg {
    fn path(self) -> &'static str {
        match self {
            Pkg::Wireless => "/interface/wireless",
            Pkg::Wifi => "/interface/wifi",
        }
    }
    fn acl(self) -> &'static str {
        match self {
            Pkg::Wireless => "/interface/wireless/access-list",
            Pkg::Wifi => "/interface/wifi/access-list",
        }
    }
    fn key(self, legacy: &'static str, new: &'static str) -> &'static str {
        if self == Pkg::Wifi { new } else { legacy }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum W {
    /// SSID name, hide, client isolation, security type and passphrase for one SSID on its radios.
    WifiNetwork {
        #[serde(default)]
        package: Pkg,
        interfaces: Vec<String>,
        ssid: Option<String>,
        hidden: Option<bool>,
        isolated: Option<bool>,
        security: Option<String>,
        passphrase: Option<String>,
        /// radios that stop broadcasting this SSID (band selection): they are disabled
        #[serde(default)]
        off: Vec<String>,
        /// bridge port PVID for the SSID's interfaces (1 = default network)
        vlan: Option<u16>,
        /// per-client limits ("50M", "0" = unlimited), legacy wireless only
        limit_down: Option<String>,
        limit_up: Option<String>,
    },
    /// Band, channel (frequency in MHz or "auto"), width and tx power ("default" or dBm) per radio.
    WifiRadio {
        #[serde(default)]
        package: Pkg,
        interface: String,
        band: Option<String>,
        frequency: Option<String>,
        width: Option<String>,
        tx_power: Option<String>,
    },
    WifiRadioEnabled {
        #[serde(default)]
        package: Pkg,
        interface: String,
        enabled: bool,
    },
    /// Virtual AP with its own security, isolated clients.
    WifiGuest {
        #[serde(default)]
        package: Pkg,
        master: String,
        ssid: String,
        passphrase: String,
    },
    RemoveWifiGuest {
        #[serde(default)]
        package: Pkg,
        master: String,
        ssid: String,
    },
    WifiAccessList {
        #[serde(default)]
        package: Pkg,
        mac: String,
        allow: bool,
        interface: Option<String>,
    },
    RemoveWifiAccessList {
        #[serde(default)]
        package: Pkg,
        mac: String,
    },
    /// 5 GHz only accepts clients heard at `min` dBm or better: an accept rule with signal-range, then reject the rest.
    WifiSignalRange {
        #[serde(default)]
        package: Pkg,
        interface: String,
        min: i8,
    },
    RemoveWifiSignalRange {
        #[serde(default)]
        package: Pkg,
        interface: String,
    },
    /// Radio off at `off`, on at `on` (HH:MM, router clock), every day.
    WifiSchedule {
        #[serde(default)]
        package: Pkg,
        interface: String,
        on: String,
        off: String,
    },
    RemoveWifiSchedule {
        interface: String,
    },
}

const KINDS: &[&str] = &["wifi-network", "wifi-radio", "wifi-radio-enabled", "wifi-guest", "remove-wifi-guest", "wifi-access-list", "remove-wifi-access-list", "wifi-signal-range", "remove-wifi-signal-range", "wifi-schedule", "remove-wifi-schedule"];

pub fn compile(v: &Value, r: &mut dyn Router) -> Option<Result<Plan, String>> {
    let kind = v.get("kind")?.as_str()?;
    if !KINDS.contains(&kind) {
        return None;
    }
    Some(serde_json::from_value::<W>(v.clone()).map_err(|e| format!("invalid {kind} intent: {e}")).and_then(|w| build(w, r)))
}

// ---------- validation ----------

fn ifname(s: &str) -> Result<String, String> {
    let s = ros_name(s)?;
    if s.contains(' ') {
        return Err("invalid interface name".into());
    }
    Ok(s)
}

fn ssid(s: &str) -> Result<String, String> {
    let ok = !s.is_empty() && s.len() <= 32 && valid_value(s) && !s.starts_with(' ') && !s.ends_with(' ');
    ok.then(|| s.to_string()).ok_or_else(|| "SSID must be 1-32 bytes, no control characters or edge spaces".into())
}

fn passphrase(s: &str) -> Result<String, String> {
    let ok = (8..=63).contains(&s.len()) && s.bytes().all(|b| (0x20..0x7f).contains(&b));
    ok.then(|| s.to_string()).ok_or_else(|| "passphrase must be 8-63 printable ASCII characters".into())
}

fn auth_types(sec: &str) -> Result<&'static str, String> {
    match sec {
        "wpa2" => Ok("wpa2-psk"),
        "wpa2-wpa3" => Ok("wpa2-psk,wpa3-psk"),
        "wpa3" => Ok("wpa3-psk"),
        _ => Err("security must be wpa2, wpa2-wpa3 or wpa3".into()),
    }
}

fn hhmm(s: &str) -> Result<String, String> {
    let (h, m) = s.trim().split_once(':').ok_or("time must be HH:MM")?;
    let (h, m): (u8, u8) = (h.parse().map_err(|_| "time must be HH:MM")?, m.parse().map_err(|_| "time must be HH:MM")?);
    if h > 23 || m > 59 {
        return Err("time must be HH:MM".into());
    }
    Ok(format!("{h:02}:{m:02}:00"))
}

/// "50M", "1.5G", "800k" or plain bits per second -> bps (0 = unlimited).
fn bps(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let bad = || format!("invalid speed {s:?} (e.g. 50M, 0 = unlimited)");
    let (n, mul) = match s.as_bytes().last() {
        Some(b'k' | b'K') => (&s[..s.len() - 1], 1e3),
        Some(b'M') => (&s[..s.len() - 1], 1e6),
        Some(b'G') => (&s[..s.len() - 1], 1e9),
        _ => (s, 1.0),
    };
    let v: f64 = n.parse().map_err(|_| bad())?;
    let v = v * mul;
    (v.is_finite() && (0.0..=4_294_967_295.0).contains(&v)).then_some(v as u64).ok_or_else(bad)
}

fn slug(s: &str) -> String {
    let s: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).take(20).collect::<String>().to_ascii_lowercase();
    if s.is_empty() { "guest".into() } else { s }
}

const LEGACY_BANDS: &[&str] = &["2ghz-b", "2ghz-onlyg", "2ghz-b/g", "2ghz-onlyn", "2ghz-b/g/n", "2ghz-g/n", "5ghz-a", "5ghz-onlyn", "5ghz-a/n", "5ghz-onlyac", "5ghz-a/n/ac", "5ghz-n/ac"];
const WIFI_BANDS: &[&str] = &["2ghz-g", "2ghz-n", "2ghz-ax", "2ghz-be", "5ghz-a", "5ghz-n", "5ghz-ac", "5ghz-ax", "5ghz-be", "6ghz-ax", "6ghz-be"];
const LEGACY_WIDTHS: &[&str] = &["5mhz", "10mhz", "20mhz", "20/40mhz-Ce", "20/40mhz-eC", "20/40mhz-XX", "20/40/80mhz-Ceee", "20/40/80mhz-eCee", "20/40/80mhz-eeCe", "20/40/80mhz-eeeC", "20/40/80mhz-XXXX", "20/40/80/160mhz-XXXXXXXX"];
const WIFI_WIDTHS: &[&str] = &["20mhz", "20/40mhz", "20/40mhz-Ce", "20/40mhz-eC", "20/40/80mhz", "20/40/80+80mhz", "20/40/80/160mhz", "20/40/80/160/320mhz"];

// ---------- helpers ----------

fn is_true(v: &str) -> bool {
    v == "true" || v == "yes"
}

/// Booleans come back as true/false over the API; write them back as yes/no.
const BOOL_KEYS: &[&str] = &["hide-ssid", "configuration.hide-ssid", "default-forwarding", "datapath.client-isolation", "disabled"];

/// `set` (Some) / `unset` (None) for fields of the row named `name`, with the inverse read from the router.
/// Secret fields are written but never read or restored.
fn set_fields(r: &mut dyn Router, path: &'static str, name: &str, fields: Vec<(&'static str, Option<String>)>) -> Result<(Vec<Op>, Vec<Op>), String> {
    let keys: Vec<&str> = fields.iter().map(|(k, _)| *k).filter(|k| !SECRET_KEYS.contains(k)).collect();
    let cur: Row = if keys.is_empty() {
        Row::new()
    } else {
        let rows = r.run(&Cmd::new(&format!("{path}/print")).proplist(&keys).query("name", name))?;
        rows.into_iter().next().ok_or_else(|| format!("no {name} in {path}"))?
    };
    let (mut set, mut ops) = (vec![], vec![]);
    for (k, v) in fields {
        match v {
            Some(v) => set.push((k, v)),
            None => ops.push(Op::on(path, "unset", "name", name, vec![("value-name", k.to_string())])),
        }
    }
    if !set.is_empty() {
        ops.insert(0, Op::on(path, "set", "name", name, set));
    }
    let (mut back, mut undo) = (vec![], vec![]);
    for k in keys {
        match cur.get(k).filter(|v| !v.is_empty()) {
            Some(v) if BOOL_KEYS.contains(&k) => back.push((k, yn(is_true(v)))),
            Some(v) => back.push((k, v.clone())),
            None => undo.push(Op::on(path, "unset", "name", name, vec![("value-name", k.to_string())])),
        }
    }
    if !back.is_empty() {
        undo.insert(0, Op::on(path, "set", "name", name, back));
    }
    Ok((ops, undo))
}

/// Re-adds rows that an op is about to remove (no secrets: they are never read).
fn readd(path: &'static str, keys: &[&'static str], rows: &[Row]) -> Vec<Op> {
    rows.iter().map(|row| Op::add(path, keys.iter().filter_map(|k| row.get(*k).filter(|v| !v.is_empty()).map(|v| (*k, if BOOL_KEYS.contains(k) { yn(is_true(v)) } else { v.clone() }))).collect())).collect()
}

fn first_id(r: &mut dyn Router, path: &str) -> Result<Option<String>, String> {
    Ok(r.run(&Cmd::new(&format!("{path}/print")).proplist(&[".id"]))?.first().and_then(|x| x.get(".id").cloned()))
}

const PASS_WARNING: &str = "High risk: every client on this network disconnects and must join with the new password, including this browser if it uses this Wi-Fi. The old password can't be restored automatically: tikontrol never reads Wi-Fi passwords, so an automatic revert restores the other settings only and the new password stays.";
const SCHED: &str = "/system/scheduler";

fn build(w: W, r: &mut dyn Router) -> Result<Plan, String> {
    match w {
        W::WifiNetwork { package: pkg, interfaces, ssid: name, hidden, isolated, security, passphrase: pass, off, vlan, limit_down, limit_up } => {
            if interfaces.is_empty() || interfaces.len() > 8 || off.len() > 8 {
                return Err("pick 1-8 Wi-Fi interfaces".into());
            }
            let ifs = interfaces.iter().map(|i| ifname(i)).collect::<Result<Vec<_>, _>>()?;
            let off = off.iter().map(|i| ifname(i)).collect::<Result<Vec<_>, _>>()?;
            if off.iter().any(|o| ifs.contains(o)) {
                return Err("a radio can't both broadcast and stop broadcasting the SSID".into());
            }
            let name = name.as_deref().map(ssid).transpose()?;
            let auth = security.as_deref().map(auth_types).transpose()?;
            let pass = pass.as_deref().filter(|p| !p.is_empty()).map(passphrase).transpose()?;
            if vlan.is_some_and(|v| !(1..=4094).contains(&v)) {
                return Err("VLAN must be 1-4094".into());
            }
            let limits = match (limit_down.as_deref(), limit_up.as_deref()) {
                (None, None) => None,
                _ if pkg == Pkg::Wifi => return Err("per-client speed limits are only supported on the legacy wireless package".into()),
                (d, u) => Some((bps(d.unwrap_or("0"))?, bps(u.unwrap_or("0"))?)),
            };
            if name.is_none() && hidden.is_none() && isolated.is_none() && auth.is_none() && pass.is_none() && off.is_empty() && vlan.is_none() && limits.is_none() {
                return Err("nothing to change".into());
            }
            let (mut ops, mut undo) = (vec![], vec![]);
            for i in &ifs {
                let mut f: Vec<(&'static str, Option<String>)> = vec![];
                if let Some((d, u)) = &limits {
                    f.push(("default-ap-tx-limit", Some(d.to_string())));
                    f.push(("default-client-tx-limit", Some(u.to_string())));
                }
                if let Some(s) = &name {
                    f.push((pkg.key("ssid", "configuration.ssid"), Some(s.clone())));
                }
                if let Some(h) = hidden {
                    f.push((pkg.key("hide-ssid", "configuration.hide-ssid"), Some(yn(h))));
                }
                if let Some(iso) = isolated {
                    match pkg {
                        Pkg::Wireless => f.push(("default-forwarding", Some(yn(!iso)))),
                        Pkg::Wifi => f.push(("datapath.client-isolation", Some(yn(iso)))),
                    }
                }
                if pkg == Pkg::Wifi {
                    if let Some(a) = auth {
                        f.push(("security.authentication-types", Some(a.into())));
                    }
                    if let Some(p) = &pass {
                        f.push(("security.passphrase", Some(p.clone())));
                    }
                }
                if !f.is_empty() {
                    let (o, u) = set_fields(r, pkg.path(), i, f)?;
                    ops.extend(o);
                    undo.splice(0..0, u);
                }
                if let Some(v) = vlan {
                    let rows = r.run(&Cmd::new("/interface/bridge/port/print").proplist(&["pvid"]).query("interface", i))?;
                    let was = rows.first().and_then(|x| x.get("pvid")).cloned().ok_or_else(|| format!("{i} is not a bridge port"))?;
                    ops.push(Op::on("/interface/bridge/port", "set", "interface", i, vec![("pvid", v.to_string())]));
                    undo.insert(0, Op::on("/interface/bridge/port", "set", "interface", i, vec![("pvid", was)]));
                }
            }
            for i in &off {
                let (o, u) = set_fields(r, pkg.path(), i, vec![("disabled", Some("yes".into()))])?;
                ops.extend(o);
                undo.splice(0..0, u);
            }
            let mut warnings = vec![];
            if !off.is_empty() {
                warnings.push(format!("{} turns off: every client on it disconnects, and any other SSID on that radio goes off too.", off.join(", ")));
            }
            if vlan.is_some() {
                warnings.push("The SSID's clients move to that network and get a new address. The VLAN only takes effect with bridge VLAN filtering on, and the bridge must be tagged in that VLAN (networks made by tikontrol are).".into());
            }
            if limits.is_some() {
                warnings.push("The limit applies to each client on this SSID, not to the SSID as a whole.".into());
            }
            if pkg == Pkg::Wireless && (auth.is_some() || pass.is_some()) {
                // legacy: security lives in the interface's security profile
                let mut profiles: Vec<String> = vec![];
                for i in &ifs {
                    let rows = r.run(&Cmd::new("/interface/wireless/print").proplist(&["security-profile"]).query("name", i))?;
                    let p = rows.first().and_then(|x| x.get("security-profile")).cloned().ok_or_else(|| format!("no Wi-Fi interface {i}"))?;
                    if !profiles.contains(&p) {
                        profiles.push(p);
                    }
                }
                for p in &profiles {
                    let p = ifname(p)?;
                    let mut f: Vec<(&'static str, Option<String>)> = vec![("mode", Some("dynamic-keys".into()))];
                    if let Some(a) = auth {
                        f.push(("authentication-types", Some(a.into())));
                    }
                    if let Some(k) = &pass {
                        f.push(("wpa2-pre-shared-key", Some(k.clone())));
                        f.push(("wpa3-pre-shared-key", Some(k.clone())));
                    }
                    let (o, u) = set_fields(r, "/interface/wireless/security-profiles", &p, f)?;
                    ops.extend(o);
                    undo.splice(0..0, u);
                    if p == "default" {
                        warnings.push("The security profile \"default\" may be shared with other interfaces: they change too.".into());
                    }
                }
            }
            let risk = if pass.is_some() || auth.is_some() { Risk::High } else if name.is_some() || vlan.is_some() || !off.is_empty() { Risk::Medium } else { Risk::Low };
            if pass.is_some() {
                warnings.insert(0, PASS_WARNING.into());
            } else if auth.is_some() {
                warnings.push("High risk: changing the security type disconnects every client; older devices may not support WPA3.".into());
            }
            if name.is_some() {
                warnings.push("Clients disconnect; devices that saved the old name must join again.".into());
            }
            if hidden == Some(true) {
                warnings.push("Hiding the SSID is not a security feature: devices must be told the name to join.".into());
            }
            let title = format!("Wi-Fi settings on {}", ifs.join(", "));
            Ok(Plan { warnings, ..plan(title, risk, ops, undo) })
        }
        W::WifiRadio { package: pkg, interface, band, frequency, width, tx_power } => {
            let i = ifname(&interface)?;
            let mut f: Vec<(&'static str, Option<String>)> = vec![];
            if let Some(b) = band {
                let ok = if pkg == Pkg::Wifi { WIFI_BANDS } else { LEGACY_BANDS };
                if !ok.contains(&b.as_str()) {
                    return Err(format!("band must be one of {}", ok.join(", ")));
                }
                f.push((pkg.key("band", "channel.band"), Some(b)));
            }
            if let Some(fr) = frequency {
                let v = if fr == "auto" {
                    None
                } else {
                    let mhz: u16 = fr.parse().map_err(|_| "frequency must be auto or MHz")?;
                    if !(2400..=7125).contains(&mhz) {
                        return Err("frequency must be 2400-7125 MHz".into());
                    }
                    Some(mhz.to_string())
                };
                // legacy takes frequency=auto; the new package unsets it for auto
                f.push((pkg.key("frequency", "channel.frequency"), if pkg == Pkg::Wireless { Some(v.unwrap_or_else(|| "auto".into())) } else { v }));
            }
            if let Some(wd) = width {
                let ok = if pkg == Pkg::Wifi { WIFI_WIDTHS } else { LEGACY_WIDTHS };
                if !ok.contains(&wd.as_str()) {
                    return Err(format!("width must be one of {}", ok.join(", ")));
                }
                f.push((pkg.key("channel-width", "channel.width"), Some(wd)));
            }
            if let Some(p) = tx_power {
                let dbm = if p == "default" {
                    None
                } else {
                    let n: u8 = p.parse().map_err(|_| "tx power must be default or 1-30 dBm")?;
                    if !(1..=30).contains(&n) {
                        return Err("tx power must be default or 1-30 dBm".into());
                    }
                    Some(n.to_string())
                };
                match pkg {
                    Pkg::Wireless => {
                        f.push(("tx-power-mode", Some(if dbm.is_some() { "all-rates-fixed" } else { "default" }.into())));
                        if dbm.is_some() {
                            f.push(("tx-power", dbm));
                        }
                    }
                    Pkg::Wifi => f.push(("configuration.tx-power", dbm)),
                }
            }
            if f.is_empty() {
                return Err("nothing to change".into());
            }
            let low_unii1 = f.iter().any(|(k, v)| k.ends_with("frequency") && v.as_deref().and_then(|v| v.parse::<u16>().ok()).is_some_and(|m| (5150..=5250).contains(&m)));
            let (ops, undo) = set_fields(r, pkg.path(), &i, f)?;
            let mut p = plan(format!("Radio settings for {i}"), Risk::Medium, ops, undo);
            p.warnings.push("Clients on this radio disconnect for a few seconds. A wrong channel or power can break coverage until it reverts. Stay within your country's limits.".into());
            if low_unii1 {
                p.warnings.push(EU_POWER.into());
            }
            Ok(p)
        }
        W::WifiRadioEnabled { package: pkg, interface, enabled } => {
            let i = ifname(&interface)?;
            let (ops, undo) = set_fields(r, pkg.path(), &i, vec![("disabled", Some(yn(!enabled)))])?;
            let mut p = plan(format!("{} {i}", if enabled { "Enable" } else { "Disable" }), if enabled { Risk::Low } else { Risk::Medium }, ops, undo);
            if !enabled {
                p.warnings.push("Every client on this radio disconnects. If this browser uses it, you can't confirm the change and it reverts after 5 minutes.".into());
            }
            Ok(p)
        }
        W::WifiGuest { package: pkg, master, ssid: name, passphrase: pass } => {
            let (m, name, pass) = (ifname(&master)?, ssid(&name)?, passphrase(&pass)?);
            let s = slug(&name);
            let c = format!("tik:guest:{s}:{m}");
            let obj = format!("tik-guest-{s}-{m}");
            let mut ops = vec![];
            let mut undo = vec![Op::on(pkg.path(), "remove", "comment", &c, vec![])];
            match pkg {
                Pkg::Wireless => {
                    let prof = "/interface/wireless/security-profiles";
                    ops.push(Op::add(prof, vec![("name", obj.clone()), ("mode", "dynamic-keys".into()), ("authentication-types", "wpa2-psk".into()), ("wpa2-pre-shared-key", pass), ("comment", c.clone())]));
                    ops.push(Op::add(pkg.path(), vec![("master-interface", m.clone()), ("name", obj.clone()), ("ssid", name.clone()), ("security-profile", obj), ("default-forwarding", "no".into()), ("disabled", "no".into()), ("comment", c.clone())]));
                    undo.push(Op::on(prof, "remove", "comment", &c, vec![]));
                }
                Pkg::Wifi => ops.push(Op::add(pkg.path(), vec![("master-interface", m.clone()), ("name", obj), ("configuration.ssid", name.clone()), ("security.authentication-types", "wpa2-psk,wpa3-psk".into()), ("security.passphrase", pass), ("datapath.client-isolation", "yes".into()), ("disabled", "no".into()), ("comment", c.clone())])),
            }
            let mut p = plan(format!("Add guest network {name} on {m}"), Risk::Medium, ops, undo);
            p.warnings.push("Guest clients can't see each other, but they can still reach your LAN: give the guest network its own VLAN or bridge and firewall it off (Networks).".into());
            p.warnings.push("The radio restarts briefly; its clients reconnect.".into());
            Ok(p)
        }
        W::RemoveWifiGuest { package: pkg, master, ssid: name } => {
            let (m, name) = (ifname(&master)?, ssid(&name)?);
            let c = format!("tik:guest:{}:{m}", slug(&name));
            if r.run(&Cmd::new(&format!("{}/print", pkg.path())).proplist(&["name"]).query("comment", &c))?.is_empty() {
                return Err("only guest networks created by tikontrol can be removed here".into());
            }
            let mut ops = vec![Op::on(pkg.path(), "remove", "comment", &c, vec![])];
            if pkg == Pkg::Wireless {
                ops.push(Op::on("/interface/wireless/security-profiles", "remove", "comment", &c, vec![]));
            }
            Ok(Plan {
                title: format!("Remove guest network {name} on {m}"),
                risk: Risk::Medium,
                warnings: vec!["This can't be undone automatically: tikontrol never reads the guest password. Create the network again to bring it back.".into()],
                ops,
                undo: vec![],
                revertible: false,
            })
        }
        W::WifiAccessList { package: pkg, mac, allow, interface } => {
            let mac = norm_mac(&mac)?;
            let ifn = interface.as_deref().filter(|i| !i.is_empty()).map(ifname).transpose()?;
            let c = format!("tik:acl:{mac}");
            let keys = acl_keys(pkg);
            let old = r.run(&Cmd::new(&format!("{}/print", pkg.acl())).proplist(&keys).query("comment", &c))?;
            let mut args = vec![("mac-address", mac.clone())];
            if let Some(i) = ifn {
                args.push(("interface", i));
            }
            match pkg {
                Pkg::Wireless => args.extend([("authentication", yn(allow)), ("forwarding", yn(allow))]),
                Pkg::Wifi => args.push(("action", if allow { "accept" } else { "reject" }.into())),
            }
            args.push(("comment", c.clone()));
            // the list is ordered: first match wins, so put ours on top
            if let Some(id) = first_id(r, pkg.acl())? {
                args.push(("place-before", id));
            }
            let ops = vec![Op::on(pkg.acl(), "remove", "comment", &c, vec![]), Op::add(pkg.acl(), args)];
            let mut undo = vec![Op::on(pkg.acl(), "remove", "comment", &c, vec![])];
            undo.extend(readd(pkg.acl(), &keys, &old));
            let mut p = plan(format!("{} {mac} on Wi-Fi", if allow { "Allow" } else { "Deny" }), Risk::Low, ops, undo);
            if allow {
                p.warnings.push("Allow entries only restrict who can join once the interface stops accepting unknown devices (default-authentication=no, or a reject-all entry at the end).".into());
            }
            Ok(p)
        }
        W::RemoveWifiAccessList { package: pkg, mac } => {
            let mac = norm_mac(&mac)?;
            let c = format!("tik:acl:{mac}");
            let keys = acl_keys(pkg);
            let old = r.run(&Cmd::new(&format!("{}/print", pkg.acl())).proplist(&keys).query("comment", &c))?;
            if old.is_empty() {
                return Err("only access-list entries created by tikontrol can be removed here".into());
            }
            Ok(plan(format!("Remove Wi-Fi access rule for {mac}"), Risk::Low, vec![Op::on(pkg.acl(), "remove", "comment", &c, vec![])], readd(pkg.acl(), &keys, &old)))
        }
        W::WifiSignalRange { package: pkg, interface, min } => {
            let i = ifname(&interface)?;
            if !(-90..=-50).contains(&min) {
                return Err("minimum signal must be -90 to -50 dBm".into());
            }
            let c = format!("tik:sigrange:{i}");
            let keys = range_keys(pkg);
            let old = r.run(&Cmd::new(&format!("{}/print", pkg.acl())).proplist(&keys).query("comment", &c))?;
            let verdict = |ok: bool| match pkg {
                Pkg::Wireless => vec![("authentication", yn(ok)), ("forwarding", yn(ok))],
                Pkg::Wifi => vec![("action", if ok { "accept" } else { "reject" }.into())],
            };
            // appended after the user's own rules, so their MAC allow/deny entries still win
            let mut accept = vec![("interface", i.clone()), ("signal-range", format!("{min}..120"))];
            accept.extend(verdict(true));
            accept.push(("comment", c.clone()));
            let mut reject = vec![("interface", i.clone())];
            reject.extend(verdict(false));
            reject.push(("comment", c.clone()));
            let ops = vec![Op::on(pkg.acl(), "remove", "comment", &c, vec![]), Op::add(pkg.acl(), accept), Op::add(pkg.acl(), reject)];
            let mut undo = vec![Op::on(pkg.acl(), "remove", "comment", &c, vec![])];
            undo.extend(readd(pkg.acl(), &keys, &old));
            let mut p = plan(format!("{i} only accepts clients at {min} dBm or better"), Risk::Medium, ops, undo);
            p.warnings.push(format!("Wi-Fi clients on {i} weaker than {min} dBm are disconnected and must use another band; clients reconnect when this applies or reverts."));
            p.warnings.push("Pick a threshold a few dB away from where a device usually sits, or it keeps bouncing at the edge. Devices that only have this band can't join from weak spots.".into());
            Ok(p)
        }
        W::RemoveWifiSignalRange { package: pkg, interface } => {
            let i = ifname(&interface)?;
            let c = format!("tik:sigrange:{i}");
            let keys = range_keys(pkg);
            let old = r.run(&Cmd::new(&format!("{}/print", pkg.acl())).proplist(&keys).query("comment", &c))?;
            if old.is_empty() {
                return Err("only signal limits created by tikontrol can be removed here".into());
            }
            Ok(plan(format!("Remove the signal limit on {i}"), Risk::Low, vec![Op::on(pkg.acl(), "remove", "comment", &c, vec![])], readd(pkg.acl(), &keys, &old)))
        }
        W::WifiSchedule { package: pkg, interface, on, off } => {
            let (i, on, off) = (ifname(&interface)?, hhmm(&on)?, hhmm(&off)?);
            if on == off {
                return Err("on and off times must differ".into());
            }
            let c = format!("tik:wifisched:{i}");
            let old = r.run(&Cmd::new(&format!("{SCHED}/print")).proplist(&SCHED_KEYS).query("comment", &c))?;
            let ev = |verb: &str| format!("{}/{verb} [find name=\"{i}\"]", pkg.path());
            let add = |n: String, t: &str, e: String| Op::add(SCHED, vec![("name", n), ("start-time", t.to_string()), ("interval", "1d".into()), ("policy", "read,write,test".into()), ("on-event", e), ("comment", c.clone())]);
            let ops = vec![Op::on(SCHED, "remove", "comment", &c, vec![]), add(format!("tik-wifi-off-{i}"), &off, ev("disable")), add(format!("tik-wifi-on-{i}"), &on, ev("enable"))];
            let mut undo = vec![Op::on(SCHED, "remove", "comment", &c, vec![])];
            undo.extend(readd(SCHED, &SCHED_KEYS, &old));
            let mut p = plan(format!("Wi-Fi {i} on {} to {}", &on[..5], &off[..5]), Risk::Low, ops, undo);
            p.warnings.push(format!("The radio turns off at {} by the router clock, and its clients disconnect. Check the router's time zone.", &off[..5]));
            Ok(p)
        }
        W::RemoveWifiSchedule { interface } => {
            let i = ifname(&interface)?;
            let c = format!("tik:wifisched:{i}");
            let old = r.run(&Cmd::new(&format!("{SCHED}/print")).proplist(&SCHED_KEYS).query("comment", &c))?;
            if old.is_empty() {
                return Err("only Wi-Fi schedules created by tikontrol can be removed here".into());
            }
            Ok(plan(format!("Remove the Wi-Fi schedule for {i}"), Risk::Low, vec![Op::on(SCHED, "remove", "comment", &c, vec![])], readd(SCHED, &SCHED_KEYS, &old)))
        }
    }
}

const EU_POWER: &str = "Range trade-off (EU): channels 36-48 allow 200 mW EIRP, channels 100-140 (DFS) allow 1 W, so leaving DFS can shorten 5 GHz range. Check your country's rules.";

fn range_keys(pkg: Pkg) -> Vec<&'static str> {
    match pkg {
        Pkg::Wireless => vec!["interface", "signal-range", "authentication", "forwarding", "comment"],
        Pkg::Wifi => vec!["interface", "signal-range", "action", "comment"],
    }
}

const SCHED_KEYS: [&str; 6] = ["name", "start-time", "interval", "policy", "on-event", "comment"];

fn acl_keys(pkg: Pkg) -> Vec<&'static str> {
    match pkg {
        Pkg::Wireless => vec!["mac-address", "interface", "authentication", "forwarding", "comment"],
        Pkg::Wifi => vec!["mac-address", "interface", "action", "comment"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply::{compile_value, revert_script};
    use crate::ros::queries::tests::{row, Fake};
    use serde_json::json;

    fn go(v: Value, f: &mut Fake) -> Result<Plan, String> {
        compile_value(&v, f)
    }

    #[test]
    fn other_kinds_are_not_ours() {
        assert!(compile(&json!({"kind": "block-client", "mac": "AA:BB:CC:DD:EE:FF"}), &mut Fake::default()).is_none());
    }

    #[test]
    fn ssid_hide_isolation_restore_previous_values() {
        let mut f = Fake::default();
        f.answers.insert("/interface/wireless/print".into(), Ok(vec![row(&[("ssid", "Old"), ("hide-ssid", "false"), ("default-forwarding", "true")])]));
        let p = go(json!({"kind": "wifi-network", "interfaces": ["wlan1"], "ssid": "New net", "hidden": true, "isolated": true}), &mut f).unwrap();
        assert_eq!(p.cli(), [r#"/interface/wireless/set [find name="wlan1"] ssid="New net" hide-ssid=yes default-forwarding=no"#]);
        assert_eq!(p.undo_cli(), [r#"/interface/wireless/set [find name="wlan1"] ssid=Old hide-ssid=no default-forwarding=yes"#]);
        assert_eq!(p.risk, Risk::Medium);
    }

    #[test]
    fn new_package_uses_dotted_keys_and_unsets_missing_values() {
        let mut f = Fake::default();
        f.answers.insert("/interface/wifi/print".into(), Ok(vec![row(&[("channel.band", "5ghz-ax")])]));
        let p = go(json!({"kind": "wifi-radio", "package": "wifi", "interface": "wifi1", "band": "5ghz-ac", "frequency": "5180", "tx_power": "default"}), &mut f).unwrap();
        assert_eq!(p.cli(), [r#"/interface/wifi/set [find name="wifi1"] channel.band=5ghz-ac channel.frequency=5180"#, r#"/interface/wifi/unset [find name="wifi1"] value-name=configuration.tx-power"#]);
        assert!(p.undo_cli().contains(&r#"/interface/wifi/set [find name="wifi1"] channel.band=5ghz-ax"#.to_string()));
        assert!(p.undo_cli().contains(&r#"/interface/wifi/unset [find name="wifi1"] value-name=channel.frequency"#.to_string()));
    }

    #[test]
    fn passphrase_is_never_shown_or_restored() {
        let mut f = Fake::default();
        f.answers.insert("/interface/wireless/print".into(), Ok(vec![row(&[("security-profile", "default")])]));
        f.answers.insert("/interface/wireless/security-profiles/print".into(), Ok(vec![row(&[("mode", "dynamic-keys"), ("authentication-types", "wpa2-psk")])]));
        let p = go(json!({"kind": "wifi-network", "interfaces": ["wlan1", "wlan2"], "security": "wpa2-wpa3", "passphrase": "correct horse battery"}), &mut f).unwrap();
        assert_eq!(p.risk, Risk::High);
        assert!(p.warnings[0].contains("can't be restored"));
        assert!(p.revertible);
        // read only non-secret fields
        assert!(f.seen.iter().all(|w| !w.iter().any(|x| x.contains("pre-shared-key"))));
        let s = revert_script("t1", &p).unwrap();
        assert!(!s.contains("correct horse") && !s.contains("pre-shared-key"), "{s}");
        assert!(p.cli().iter().all(|c| !c.contains("correct horse")));
        assert!(s.contains("authentication-types=wpa2-psk"));
        // the op itself still carries the real value for the router
        assert!(p.ops.iter().any(|o| o.args.iter().any(|(_, v)| v == "correct horse battery")));
        // one shared profile is set once
        assert_eq!(p.ops.iter().filter(|o| o.path.ends_with("security-profiles")).count(), 1);
    }

    #[test]
    fn guest_adds_tagged_objects_and_removes_them_on_undo() {
        let p = go(json!({"kind": "wifi-guest", "master": "wlan2", "ssid": "Guests!", "passphrase": "guestpass1"}), &mut Fake::default()).unwrap();
        assert!(p.cli()[1].contains("master-interface=wlan2") && p.cli()[1].contains("default-forwarding=no"));
        assert!(p.cli()[0].contains(r#"wpa2-pre-shared-key="********""#));
        assert_eq!(p.undo_cli(), [r#"/interface/wireless/remove [find comment="tik:guest:guests:wlan2"]"#, r#"/interface/wireless/security-profiles/remove [find comment="tik:guest:guests:wlan2"]"#]);
        assert!(!revert_script("g", &p).unwrap().contains("guestpass1"));
        let w = go(json!({"kind": "wifi-guest", "package": "wifi", "master": "wifi1", "ssid": "G", "passphrase": "guestpass1"}), &mut Fake::default()).unwrap();
        assert!(w.cli()[0].contains("datapath.client-isolation=yes") && w.cli()[0].contains(r#"security.passphrase="********""#));
    }

    #[test]
    fn access_list_and_schedule() {
        let mut f = Fake::default();
        f.answers.insert("/interface/wireless/access-list/print".into(), Ok(vec![row(&[(".id", "*1")])]));
        let p = go(json!({"kind": "wifi-access-list", "mac": "aa:bb:cc:dd:ee:01", "allow": false}), &mut f).unwrap();
        assert!(p.cli()[1].contains("authentication=no") && p.cli()[1].ends_with("place-before=*1"));
        let p = go(json!({"kind": "wifi-schedule", "interface": "wlan1", "on": "7:00", "off": "23:30"}), &mut Fake::default()).unwrap();
        assert_eq!(p.cli()[1], r#"/system/scheduler/add name=tik-wifi-off-wlan1 start-time=23:30:00 interval=1d policy=read,write,test on-event="/interface/wireless/disable [find name=\"wlan1\"]" comment=tik:wifisched:wlan1"#);
        assert_eq!(p.undo_cli(), [r#"/system/scheduler/remove [find comment="tik:wifisched:wlan1"]"#]);
        assert!(go(json!({"kind": "remove-wifi-schedule", "interface": "wlan1"}), &mut Fake::default()).is_err());
    }

    #[test]
    fn signal_range_adds_accept_then_reject_and_undo_removes_them() {
        let p = go(json!({"kind": "wifi-signal-range", "interface": "wlan2", "min": -72}), &mut Fake::default()).unwrap();
        assert_eq!(p.cli()[1], "/interface/wireless/access-list/add interface=wlan2 signal-range=-72..120 authentication=yes forwarding=yes comment=tik:sigrange:wlan2");
        assert_eq!(p.cli()[2], "/interface/wireless/access-list/add interface=wlan2 authentication=no forwarding=no comment=tik:sigrange:wlan2");
        assert_eq!(p.undo_cli(), [r#"/interface/wireless/access-list/remove [find comment="tik:sigrange:wlan2"]"#]);
        assert!(go(json!({"kind": "wifi-signal-range", "interface": "wlan2", "min": -20}), &mut Fake::default()).is_err());
        let mut f = Fake::default();
        f.answers.insert("/interface/wireless/print".into(), Ok(vec![row(&[("frequency", "5640")])]));
        assert!(go(json!({"kind": "wifi-radio", "interface": "wlan2", "frequency": "5180"}), &mut f).unwrap().warnings.iter().any(|w| w.contains("200 mW")));
    }

    #[test]
    fn bands_vlan_and_limit_compile_with_undo() {
        let mut f = Fake::default();
        f.answers.insert("/interface/wireless/print".into(), Ok(vec![row(&[("default-ap-tx-limit", "0"), ("default-client-tx-limit", "0"), ("disabled", "false")])]));
        f.answers.insert("/interface/bridge/port/print".into(), Ok(vec![row(&[("pvid", "1")])]));
        let p = go(json!({"kind": "wifi-network", "interfaces": ["wlan2"], "off": ["wlan1"], "vlan": 30, "limit_down": "50M", "limit_up": "10M"}), &mut f).unwrap();
        assert_eq!(p.cli(), [
            r#"/interface/wireless/set [find name="wlan2"] default-ap-tx-limit=50000000 default-client-tx-limit=10000000"#,
            r#"/interface/bridge/port/set [find interface="wlan2"] pvid=30"#,
            r#"/interface/wireless/set [find name="wlan1"] disabled=yes"#,
        ]);
        assert_eq!(p.undo_cli(), [
            r#"/interface/wireless/set [find name="wlan1"] disabled=no"#,
            r#"/interface/bridge/port/set [find interface="wlan2"] pvid=1"#,
            r#"/interface/wireless/set [find name="wlan2"] default-ap-tx-limit=0 default-client-tx-limit=0"#,
        ]);
        for b in [
            json!({"kind": "wifi-network", "interfaces": ["wlan2"], "off": ["wlan2"]}),
            json!({"kind": "wifi-network", "interfaces": ["wlan2"], "vlan": 5000}),
            json!({"kind": "wifi-network", "interfaces": ["wlan2"], "limit_down": "50M; /system/reboot"}),
            json!({"kind": "wifi-network", "package": "wifi", "interfaces": ["wifi1"], "limit_down": "50M"}),
            json!({"kind": "wifi-network", "interfaces": ["wlan2"], "off": ["wlan1\"]"]}),
        ] {
            assert!(go(b.clone(), &mut f).is_err(), "{b}");
        }
    }

    #[test]
    fn bad_input_is_rejected() {
        let bad = [
            json!({"kind": "wifi-network", "interfaces": ["wlan1"]}),
            json!({"kind": "wifi-network", "interfaces": ["wlan1\"; /system/reboot"], "hidden": true}),
            json!({"kind": "wifi-network", "interfaces": ["wlan1"], "ssid": "x".repeat(33)}),
            json!({"kind": "wifi-network", "interfaces": ["wlan1"], "passphrase": "short"}),
            json!({"kind": "wifi-network", "interfaces": ["wlan1"], "security": "wep"}),
            json!({"kind": "wifi-network", "interfaces": ["wlan1"], "hidden": true, "extra": 1}),
            json!({"kind": "wifi-radio", "interface": "wlan1", "band": "9ghz"}),
            json!({"kind": "wifi-radio", "interface": "wlan1", "frequency": "100"}),
            json!({"kind": "wifi-radio", "interface": "wlan1", "tx_power": "99"}),
            json!({"kind": "wifi-radio", "package": "other", "interface": "wlan1", "band": "2ghz-n"}),
            json!({"kind": "wifi-guest", "master": "wlan1", "ssid": "G", "passphrase": "pässwörd123"}),
            json!({"kind": "wifi-access-list", "mac": "nope", "allow": true}),
            json!({"kind": "wifi-schedule", "interface": "wlan1", "on": "25:00", "off": "01:00"}),
        ];
        let mut f = Fake::default();
        f.answers.insert("/interface/wireless/print".into(), Ok(vec![row(&[("ssid", "x")])]));
        for b in bad {
            assert!(go(b.clone(), &mut f).is_err(), "{b}");
        }
        assert!(go(json!({"kind": "remove-wifi-guest", "master": "wlan1", "ssid": "G"}), &mut Fake::default()).is_err());
    }
}
