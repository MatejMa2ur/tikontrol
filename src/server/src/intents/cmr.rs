//! MikroTik CMR fleet changes (cmr.rs reads): adopt (pair) a device, set its labels, upgrade rules,
//! Device Supervisor alerts, and upgrading one device now. These run on the CMR server, which may be
//! a separate controller: features.rs routes every `cmr-*` kind there, revert job included.
//! Devices are selected by `ids` (the key pair CMR shows), never by identity, which another device sets.

use crate::apply::{plan, ros_name, yn, Op, Plan, Risk};
use crate::ros::cmd::Cmd;
use crate::ros::conn::{Router, Row};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
#[allow(clippy::enum_variant_names)] // the kinds are cmr-*
enum C {
    CmrPair { device: String, #[serde(default)] username: String, #[serde(default)] password: String },
    CmrDeviceLabels { device: String, labels: Vec<String> },
    CmrUpgradeRule { name: String, #[serde(default)] labels: Vec<String>, channel: String, #[serde(default)] time: String, #[serde(default)] days: Vec<String>, strategy: String, #[serde(default = "stop")] fail_policy: String },
    CmrRemoveUpgradeRule { name: String },
    CmrAlert { name: String, #[serde(default)] labels: Vec<String>, metric: String, #[serde(default)] threshold: u32, #[serde(default = "medium")] severity: String },
    CmrRemoveAlert { name: String },
    CmrAlertEnabled { name: String, enabled: bool },
    CmrUpgradeDevice { device: String },
}

fn stop() -> String {
    "stop".into()
}
fn medium() -> String {
    "medium".into()
}

const KINDS: &[&str] = &["cmr-pair", "cmr-device-labels", "cmr-upgrade-rule", "cmr-remove-upgrade-rule", "cmr-alert", "cmr-remove-alert", "cmr-alert-enabled", "cmr-upgrade-device"];
const DEV: &str = "/cmr/device";
const UPG: &str = "/cmr/upgrade";
const ALERT: &str = "/cmr/alert";
/// Settable alert fields, enough to re-create one tikontrol made.
const ALERT_KEYS: [&str; 10] = ["name", "labels", "severity", "cpu-above", "mem-above", "hdd-above", "disconnected-more-than", "upgrade-available", "action.log", "disabled"];
const RULE_KEYS: [&str; 6] = ["name", "labels", "channel", "schedule-time", "strategy", "fail-policy"];

pub fn compile(v: &Value, r: &mut dyn Router) -> Option<Result<Plan, String>> {
    let kind = v.get("kind")?.as_str()?;
    if !KINDS.contains(&kind) {
        return None;
    }
    Some(serde_json::from_value::<C>(v.clone()).map_err(|e| format!("invalid {kind} intent: {e}")).and_then(|x| build(x, r)))
}

/// `<64 hex>/<64+ hex>` as `/cmr/device print` shows it.
fn ids(s: &str) -> Result<String, String> {
    let s = s.trim();
    let ok = s.len() <= 300 && s.split_once('/').is_some_and(|(a, b)| !a.is_empty() && !b.is_empty() && (a.to_owned() + b).bytes().all(|c| c.is_ascii_hexdigit()));
    ok.then(|| s.to_string()).ok_or_else(|| "invalid device ids".into())
}

/// Labels are joined with commas and `+`/`-` are selector operators, so a label is a plain word.
fn labels(v: &[String], selector: bool) -> Result<String, String> {
    if v.len() > 16 {
        return Err("at most 16 labels".into());
    }
    let ok = |l: &str| {
        let w = if selector { l.strip_prefix(['+', '-']).unwrap_or(l) } else { l };
        !w.is_empty() && w.len() <= 32 && w.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    };
    let l: Vec<&str> = v.iter().map(|s| s.trim()).collect();
    l.iter().all(|x| ok(x)).then(|| l.join(",")).ok_or_else(|| "invalid label (letters, digits, - _ .)".into())
}

fn device(r: &mut dyn Router, id: &str) -> Result<Row, String> {
    let rows = r.run(&Cmd::new("/cmr/device/print").proplist(&["identity", "labels", "pending", "remote-pending", "upgrade-available", "version"]).query("ids", id))?;
    rows.into_iter().next().ok_or_else(|| "no such device on the CMR server".into())
}

fn is(row: &Row, k: &str) -> bool {
    row.get(k).is_some_and(|v| v == "true" || v == "yes")
}

/// Re-create args from a printed row: only non-empty settable keys, in a fixed order.
fn back(row: &Row, keys: &[&'static str], comment: &str) -> Vec<(&'static str, String)> {
    keys.iter().filter_map(|k| row.get(*k).filter(|v| !v.is_empty()).map(|v| (*k, v.clone()))).chain([("comment", comment.to_string())]).collect()
}

fn build(x: C, r: &mut dyn Router) -> Result<Plan, String> {
    match x {
        C::CmrPair { device: d, username, password } => {
            let id = ids(&d)?;
            let dev = device(r, &id)?;
            let name = dev.get("identity").cloned().unwrap_or_default();
            if !is(&dev, "pending") && !is(&dev, "remote-pending") {
                return Err(format!("{name} is already adopted"));
            }
            let mut args = vec![];
            if !username.is_empty() {
                args.push(("username", ros_name(&username)?));
            }
            if !password.is_empty() {
                if password.len() > 256 {
                    return Err("password too long".into());
                }
                args.push(("password", password));
            }
            // unpairing (remove) brings the device back as pending: that is the exact inverse
            let mut p = plan(format!("Adopt {name}"), Risk::Medium, vec![Op::on(DEV, "pair", "ids", &id, args)], vec![Op::on(DEV, "remove", "ids", &id, vec![])]);
            p.warnings.push("Once adopted, the CMR server pushes its Wi-Fi, VLAN and upgrade rules to this device by label.".into());
            p.warnings.push("The device login (if its pairing needs one) is passed to it once and never stored.".into());
            Ok(p)
        }
        C::CmrDeviceLabels { device: d, labels: l } => {
            let id = ids(&d)?;
            let new = labels(&l, false)?;
            let dev = device(r, &id)?;
            let old = dev.get("labels").cloned().unwrap_or_default();
            // an empty value isn't "no labels" in export: unset restores the exact config
            let set = |v: &str| if v.is_empty() { Op::on(DEV, "unset", "ids", &id, vec![("value-name", "labels".into())]) } else { Op::on(DEV, "set", "ids", &id, vec![("labels", v.to_string())]) };
            let mut p = plan(format!("Labels of {}", dev.get("identity").map_or("", |s| s)), Risk::Medium, vec![set(&new)], vec![set(&old)]);
            p.warnings.push("Labels decide which Wi-Fi networks, VLANs and upgrade rules apply to the device.".into());
            Ok(p)
        }
        C::CmrUpgradeRule { name, labels: l, channel, time, days, strategy, fail_policy } => {
            let name = ros_name(&name)?;
            let ver = |c: &str| !c.is_empty() && c.len() <= 16 && c.bytes().all(|b| b.is_ascii_digit() || b == b'.') && c.contains('.');
            if !matches!(channel.as_str(), "long-term" | "stable" | "testing" | "development") && !ver(&channel) {
                return Err("channel must be long-term, stable, testing, development or a version like 7.24.4".into());
            }
            if !matches!(strategy.as_str(), "sequential" | "parallel") || !matches!(fail_policy.as_str(), "stop" | "continue") {
                return Err("strategy is sequential or parallel; fail policy is stop or continue".into());
            }
            let c = format!("tik:upgrade:{name}");
            let mut args = vec![("name", name.clone()), ("channel", channel.clone()), ("strategy", strategy), ("fail-policy", fail_policy), ("comment", c.clone())];
            if !l.is_empty() {
                args.insert(1, ("labels", labels(&l, true)?));
            }
            if !days.is_empty() {
                let t = time.split_once(':').and_then(|(h, m)| Some((h.parse::<u8>().ok()?, m.parse::<u8>().ok()?))).filter(|(h, m)| *h < 24 && *m < 60).ok_or("time must be HH:MM")?;
                const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
                if days.len() > 7 || days.iter().any(|d| !DAYS.contains(&d.as_str())) {
                    return Err("days are mon, tue, wed, thu, fri, sat, sun".into());
                }
                args.push(("schedule-time", days.iter().map(|d| format!("{:02}:{:02}:00@{d}", t.0, t.1)).collect::<Vec<_>>().join(",")));
            }
            if !r.run(&Cmd::new("/cmr/upgrade/print").proplist(&["name"]).query("name", &name))?.is_empty() {
                return Err(format!("an upgrade rule named {name} already exists"));
            }
            let mut p = plan(format!("Upgrade rule {name} ({channel})"), Risk::Medium, vec![Op::add(UPG, args)], vec![Op::on(UPG, "remove", "comment", &c, vec![])]);
            p.warnings.push("Matching devices upgrade (and reboot) at the scheduled time, without asking again. The first matching rule claims a device.".into());
            if ver(&channel) {
                p.warnings.push("A pinned version is installed even if it is older than what the device runs.".into());
            }
            Ok(p)
        }
        C::CmrRemoveUpgradeRule { name } => {
            let c = format!("tik:upgrade:{}", ros_name(&name)?);
            let rows = r.run(&Cmd::new("/cmr/upgrade/print").proplist(&RULE_KEYS).query("comment", &c))?;
            let old = rows.first().ok_or("only upgrade rules created by tikontrol can be removed here")?;
            Ok(plan(format!("Remove upgrade rule {name}"), Risk::Low, vec![Op::on(UPG, "remove", "comment", &c, vec![])], vec![Op::add(UPG, back(old, &RULE_KEYS, &c))]))
        }
        C::CmrAlert { name, labels: l, metric, threshold, severity } => {
            let name = ros_name(&name)?;
            if !matches!(severity.as_str(), "critical" | "high" | "medium" | "low") {
                return Err("severity is critical, high, medium or low".into());
            }
            let pct = |k: &'static str| (1..=100).contains(&threshold).then(|| (k, threshold.to_string())).ok_or("threshold must be 1-100 %");
            let (cond, text) = match metric.as_str() {
                "cpu" => (pct("cpu-above")?, "[device] CPU [cpu-usage]%"),
                "memory" => (pct("mem-above")?, "[device] memory [mem-usage]%"),
                "disk" => (pct("hdd-above")?, "[device] disk [hdd-usage]%"),
                "offline" if (1..=1440).contains(&threshold) => (("disconnected-more-than", format!("{threshold}m")), "[device] offline"),
                "offline" => return Err("offline threshold is 1-1440 minutes".into()),
                "update" => (("upgrade-available", "yes".into()), "[device] can upgrade to [available-version]"),
                _ => return Err("metric is cpu, memory, disk, offline or update".into()),
            };
            if !r.run(&Cmd::new("/cmr/alert/print").proplist(&["name"]).query("name", &name))?.is_empty() {
                return Err(format!("an alert named {name} already exists"));
            }
            let c = format!("tik:alert:{name}");
            let mut args = vec![("name", name.clone()), cond, ("severity", severity), ("action.log", format!("{name}: {text}")), ("comment", c.clone())];
            if !l.is_empty() {
                args.insert(1, ("labels", labels(&l, true)?));
            }
            Ok(plan(format!("Device Supervisor alert {name}"), Risk::Low, vec![Op::add(ALERT, args)], vec![Op::on(ALERT, "remove", "comment", &c, vec![])]))
        }
        C::CmrRemoveAlert { name } => {
            let c = format!("tik:alert:{}", ros_name(&name)?);
            let rows = r.run(&Cmd::new("/cmr/alert/print").proplist(&ALERT_KEYS).query("comment", &c))?;
            let old = rows.first().ok_or("only alerts created by tikontrol can be removed here")?;
            Ok(plan(format!("Remove alert {name}"), Risk::Low, vec![Op::on(ALERT, "remove", "comment", &c, vec![])], vec![Op::add(ALERT, back(old, &ALERT_KEYS, &c))]))
        }
        C::CmrAlertEnabled { name, enabled } => {
            let name = ros_name(&name)?;
            let rows = r.run(&Cmd::new("/cmr/alert/print").proplist(&["disabled"]).query("name", &name))?;
            let was_off = is(rows.first().ok_or("no such alert")?, "disabled");
            let set = |off: bool| Op::on(ALERT, "set", "name", &name, vec![("disabled", yn(off))]);
            Ok(plan(format!("{} alert {name}", if enabled { "Enable" } else { "Disable" }), Risk::Low, vec![set(!enabled)], vec![set(was_off)]))
        }
        C::CmrUpgradeDevice { device: d } => {
            let id = ids(&d)?;
            let dev = device(r, &id)?;
            let name = dev.get("identity").cloned().unwrap_or_default();
            if !is(&dev, "upgrade-available") {
                return Err(format!("{name} has no upgrade available (check for updates on the CMR server first)"));
            }
            Ok(Plan { title: format!("Upgrade {name} now"), risk: Risk::High, warnings: vec![format!("{name} downloads the new RouterOS and reboots: its clients lose the network for a few minutes. This can't be undone from here.")], ops: vec![Op::on(DEV, "upgrade", "ids", &id, vec![])], undo: vec![], revertible: false })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apply::compile_value;
    use crate::ros::queries::tests::{row, Fake};
    use serde_json::json;

    const ID: &str = "8B50AA/0583BB";

    fn pending() -> Fake {
        let mut f = Fake::default();
        f.answers.insert("/cmr/device/print".into(), Ok(vec![row(&[("identity", "ap-office"), ("pending", "true"), ("labels", "ap")])]));
        f
    }

    #[test]
    fn every_kind_is_registered() {
        assert!(compile(&json!({"kind": "pair"}), &mut Fake::default()).is_none());
        for k in KINDS {
            if let Err(e) = compile(&json!({"kind": k}), &mut Fake::default()).unwrap() {
                assert!(!e.contains("unknown variant"), "{k}: {e}");
            }
        }
    }

    #[test]
    fn adopt_pairs_by_ids_and_unpairs_on_revert_without_the_password() {
        let p = compile_value(&json!({"kind": "cmr-pair", "device": ID, "username": "admin", "password": "s3cret"}), &mut pending()).unwrap();
        assert_eq!(p.cli(), [r#"/cmr/device/pair [find ids="8B50AA/0583BB"] username=admin password="********""#]);
        assert_eq!(p.undo_cli(), [r#"/cmr/device/remove [find ids="8B50AA/0583BB"]"#]);
        assert!(p.revertible && p.title == "Adopt ap-office");
        let mut f = pending();
        f.answers.insert("/cmr/device/print".into(), Ok(vec![row(&[("identity", "ap-office")])]));
        assert!(compile_value(&json!({"kind": "cmr-pair", "device": ID}), &mut f).unwrap_err().contains("already adopted"));
        for bad in ["ap-office", "x/y", "AA/BB; /system/reboot"] {
            assert!(compile_value(&json!({"kind": "cmr-pair", "device": bad}), &mut pending()).is_err(), "{bad}");
        }
        assert!(compile_value(&json!({"kind": "cmr-pair", "device": "AA/BB"}), &mut Fake::default()).unwrap_err().contains("no such device"));
    }

    #[test]
    fn labels_restore_the_old_ones() {
        let p = compile_value(&json!({"kind": "cmr-device-labels", "device": ID, "labels": ["ap", "office"]}), &mut pending()).unwrap();
        assert_eq!(p.cli(), [r#"/cmr/device/set [find ids="8B50AA/0583BB"] labels=ap,office"#]);
        assert_eq!(p.undo_cli(), [r#"/cmr/device/set [find ids="8B50AA/0583BB"] labels=ap"#]);
        let mut f = pending();
        f.answers.insert("/cmr/device/print".into(), Ok(vec![row(&[("identity", "ap-office")])]));
        let p = compile_value(&json!({"kind": "cmr-device-labels", "device": ID, "labels": ["ap"]}), &mut f).unwrap();
        assert_eq!(p.undo_cli(), [r#"/cmr/device/unset [find ids="8B50AA/0583BB"] value-name=labels"#]);
        for bad in [json!(["a,b"]), json!(["+ap"]), json!([""])] {
            assert!(compile_value(&json!({"kind": "cmr-device-labels", "device": ID, "labels": bad}), &mut pending()).is_err());
        }
    }

    #[test]
    fn upgrade_rules_are_validated_scheduled_and_undone() {
        let v = json!({"kind": "cmr-upgrade-rule", "name": "nightly-aps", "labels": ["ap", "-core"], "channel": "stable", "time": "3:00", "days": ["sat", "sun"], "strategy": "sequential"});
        let p = compile_value(&v, &mut Fake::default()).unwrap();
        assert_eq!(p.cli(), ["/cmr/upgrade/add name=nightly-aps labels=ap,-core channel=stable strategy=sequential fail-policy=stop comment=tik:upgrade:nightly-aps schedule-time=\"03:00:00@sat,03:00:00@sun\""]);
        assert_eq!(p.undo_cli(), [r#"/cmr/upgrade/remove [find comment="tik:upgrade:nightly-aps"]"#]);
        let pinned = compile_value(&json!({"kind": "cmr-upgrade-rule", "name": "pin", "channel": "7.24.4", "strategy": "parallel"}), &mut Fake::default()).unwrap();
        assert!(pinned.warnings.iter().any(|w| w.contains("older")) && !pinned.cli()[0].contains("schedule-time"));
        for bad in [json!({"channel": "beta"}), json!({"strategy": "random"}), json!({"days": ["someday"], "time": "03:00"}), json!({"days": ["sat"], "time": "25:00"})] {
            let mut x = v.clone();
            x.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
            assert!(compile_value(&x, &mut Fake::default()).is_err(), "{x}");
        }
        let mut f = Fake::default();
        f.answers.insert("/cmr/upgrade/print".into(), Ok(vec![row(&[("name", "nightly-aps"), ("labels", "ap"), ("channel", "stable"), ("schedule-time", "03:00:00@sun"), ("strategy", "sequential"), ("fail-policy", "stop")])]));
        assert!(compile_value(&v, &mut f).unwrap_err().contains("already exists"));
        let p = compile_value(&json!({"kind": "cmr-remove-upgrade-rule", "name": "nightly-aps"}), &mut f).unwrap();
        assert_eq!(p.undo_cli(), ["/cmr/upgrade/add name=nightly-aps labels=ap channel=stable schedule-time=\"03:00:00@sun\" strategy=sequential fail-policy=stop comment=tik:upgrade:nightly-aps"]);
        assert!(compile_value(&json!({"kind": "cmr-remove-upgrade-rule", "name": "default"}), &mut Fake::default()).is_err());
    }

    #[test]
    fn supervisor_alerts() {
        let p = compile_value(&json!({"kind": "cmr-alert", "name": "cpu busy", "metric": "cpu", "threshold": 90, "severity": "high"}), &mut Fake::default()).unwrap();
        assert_eq!(p.cli(), [r#"/cmr/alert/add name="cpu busy" cpu-above=90 severity=high action.log="cpu busy: [device] CPU [cpu-usage]%" comment="tik:alert:cpu busy""#]);
        assert_eq!(p.undo_cli(), [r#"/cmr/alert/remove [find comment="tik:alert:cpu busy"]"#]);
        let off = compile_value(&json!({"kind": "cmr-alert", "name": "gone", "metric": "offline", "threshold": 60, "labels": ["ap"]}), &mut Fake::default()).unwrap();
        assert!(off.cli()[0].contains("labels=ap disconnected-more-than=60m"));
        for bad in [json!({"metric": "cpu", "threshold": 0}), json!({"metric": "cpu", "threshold": 101}), json!({"metric": "temperature", "threshold": 5}), json!({"metric": "cpu", "threshold": 5, "severity": "urgent"})] {
            let mut x = json!({"kind": "cmr-alert", "name": "x"});
            x.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
            assert!(compile_value(&x, &mut Fake::default()).is_err(), "{x}");
        }
        let mut f = Fake::default();
        f.answers.insert("/cmr/alert/print".into(), Ok(vec![row(&[("name", "cpu busy"), ("labels", "all"), ("cpu-above", "90"), ("severity", "high"), ("disabled", "true")])]));
        let p = compile_value(&json!({"kind": "cmr-remove-alert", "name": "cpu busy"}), &mut f).unwrap();
        assert_eq!(p.undo_cli(), [r#"/cmr/alert/add name="cpu busy" labels=all severity=high cpu-above=90 disabled=true comment="tik:alert:cpu busy""#]);
        let p = compile_value(&json!({"kind": "cmr-alert-enabled", "name": "cpu busy", "enabled": true}), &mut f).unwrap();
        assert_eq!((p.cli()[0].as_str(), p.undo_cli()[0].as_str()), (r#"/cmr/alert/set [find name="cpu busy"] disabled=no"#, r#"/cmr/alert/set [find name="cpu busy"] disabled=yes"#));
    }

    #[test]
    fn upgrading_a_device_needs_an_available_upgrade_and_is_not_revertible() {
        assert!(compile_value(&json!({"kind": "cmr-upgrade-device", "device": ID}), &mut pending()).unwrap_err().contains("no upgrade available"));
        let mut f = Fake::default();
        f.answers.insert("/cmr/device/print".into(), Ok(vec![row(&[("identity", "ap-office"), ("upgrade-available", "true")])]));
        let p = compile_value(&json!({"kind": "cmr-upgrade-device", "device": ID}), &mut f).unwrap();
        assert_eq!((p.risk, p.revertible, p.cli()[0].as_str()), (Risk::High, false, r#"/cmr/device/upgrade [find ids="8B50AA/0583BB"]"#));
    }
}
