//! Routing (docs/FEATURES.md "Routing"): policy-based routes, i.e. `/routing/rule` entries that send
//! traffic from a source subnet or device through another routing table. Only rules tagged
//! `comment=tik:pbr:<table>:<src>` can be removed here; the inverse re-adds exactly what was there.

use crate::apply::{cidr, plan, ros_name, yn, Op, Plan, Risk};
use crate::intents::vpn::network;
use crate::ros::cmd::Cmd;
use crate::ros::conn::Router;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum R {
    PolicyRoute { src: String, table: String },
    RemovePolicyRoute { comment: String },
}

const KINDS: &[&str] = &["policy-route", "remove-policy-route"];
const RULE: &str = "/routing/rule";
const KEYS: [&str; 5] = ["src-address", "dst-address", "action", "table", "disabled"];

pub fn compile(v: &Value, r: &mut dyn Router) -> Option<Result<Plan, String>> {
    let kind = v.get("kind")?.as_str()?;
    if !KINDS.contains(&kind) {
        return None;
    }
    Some(serde_json::from_value::<R>(v.clone()).map_err(|e| format!("invalid {kind} intent: {e}")).and_then(|x| build(x, r)))
}

fn build(x: R, r: &mut dyn Router) -> Result<Plan, String> {
    match x {
        R::PolicyRoute { src, table } => {
            let s = network(&cidr(&src)?);
            let t = ros_name(&table)?;
            if s == "0.0.0.0/0" || t == "main" || t.contains(' ') {
                return Err("name a source subnet or device and a routing table other than main".into());
            }
            if r.run(&Cmd::new("/routing/table/print").proplist(&["name"]).query("name", &t))?.is_empty() {
                return Err(format!("no routing table {t}"));
            }
            let c = format!("tik:pbr:{t}:{s}");
            let add = Op::add(RULE, vec![("src-address", s.clone()), ("action", "lookup".into()), ("table", t.clone()), ("comment", c.clone())]);
            let mut p = plan(format!("Route {s} through table {t}"), Risk::Medium, vec![add], vec![Op::on(RULE, "remove", "comment", &c, vec![])]);
            p.warnings.push(format!("Traffic from {s} uses table {t}; if that table has no matching route it falls back to main."));
            Ok(p)
        }
        R::RemovePolicyRoute { comment } => {
            if !comment.starts_with("tik:pbr:") {
                return Err("only policy routes created by tikontrol can be removed here".into());
            }
            let rows = r.run(&Cmd::new("/routing/rule/print").proplist(&KEYS).query("comment", &comment))?;
            let old = rows.first().ok_or("no such policy route")?;
            let mut back: Vec<(&'static str, String)> = KEYS.iter().filter_map(|k| old.get(*k).filter(|v| !v.is_empty()).map(|v| (*k, if *k == "disabled" { yn(v == "true" || v == "yes") } else { v.clone() }))).collect();
            back.push(("comment", comment.clone()));
            Ok(plan(format!("Remove policy route {}", old.get("src-address").cloned().unwrap_or_default()), Risk::Medium, vec![Op::on(RULE, "remove", "comment", &comment, vec![])], vec![Op::add(RULE, back)]))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::apply::compile_value;
    use crate::ros::queries::tests::{row, Fake};
    use serde_json::json;

    #[test]
    fn policy_route_is_validated_tagged_and_undone() {
        let mut f = Fake::default();
        assert!(compile_value(&json!({"kind": "policy-route", "src": "192.168.88.50", "table": "vpn"}), &mut f).is_err(), "table must exist");
        f.answers.insert("/routing/table/print".into(), Ok(vec![row(&[("name", "vpn")])]));
        let p = compile_value(&json!({"kind": "policy-route", "src": "192.168.88.7/24", "table": "vpn"}), &mut f).unwrap();
        assert_eq!(p.cli(), ["/routing/rule/add src-address=192.168.88.0/24 action=lookup table=vpn comment=tik:pbr:vpn:192.168.88.0/24"]);
        assert_eq!(p.undo_cli(), [r#"/routing/rule/remove [find comment="tik:pbr:vpn:192.168.88.0/24"]"#]);
        for bad in [json!({"kind": "policy-route", "src": "0.0.0.0/0", "table": "vpn"}), json!({"kind": "policy-route", "src": "10.0.0.1", "table": "main"}), json!({"kind": "remove-policy-route", "comment": "defconf"})] {
            assert!(compile_value(&bad, &mut f).is_err(), "{bad}");
        }
        f.answers.insert("/routing/rule/print".into(), Ok(vec![row(&[("src-address", "10.0.0.1/32"), ("action", "lookup"), ("table", "vpn"), ("disabled", "false")])]));
        let p = compile_value(&json!({"kind": "remove-policy-route", "comment": "tik:pbr:vpn:10.0.0.1/32"}), &mut f).unwrap();
        assert_eq!(p.undo_cli(), ["/routing/rule/add src-address=10.0.0.1/32 action=lookup table=vpn disabled=no comment=tik:pbr:vpn:10.0.0.1/32"]);
    }
}
