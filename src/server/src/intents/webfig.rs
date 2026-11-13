//! HTTPS for the router's own web interface (WebFig), in two Safe apply steps so the private key never leaves
//! the router (RouterOS logs `/file add` contents, so a key written that way would end up in the log):
//!
//! 1. `webfig-key`: the router makes a P-256 key and a certificate request (`create-certificate-request`). Its key
//!    file is protected by a passphrase derived from tikontrol's CA key (`tlscert::router_key_passphrase`).
//! 2. `webfig-https`: tikontrol reads the request, signs the router's public key with its local CA for the router's
//!    private IPv4 addresses and the local static DNS names pointing at them, and the router imports the
//!    certificate and its own key; www-ssl serves it and plain-HTTP www goes off.
//!
//! Each run imports a new `tik-webfig-<time>` certificate, so nothing is overwritten. Step 2's inverse puts www and
//! www-ssl back and removes the new certificate (with its key: after a revert, run both steps again).

use crate::apply::{plan, yn, Op, Plan, Risk};
use crate::ros::cmd::Cmd;
use crate::ros::conn::{Router, Row};
use serde_json::Value;
use std::net::Ipv4Addr;

const SVC: &str = "/ip/service";
const CERT: &str = "/certificate";
const FILE: &str = "/file";
/// What `create-certificate-request` writes (fixed names).
const REQ: &str = "certificate-request.pem";
const REQ_KEY: &str = "certificate-request_key.pem";
/// The certificate template the request is made from (holds no key).
const TEMPLATE: &str = "tik-webfig-request";

pub fn compile(v: &Value, r: &mut dyn Router) -> Option<Result<Plan, String>> {
    let kind = v.get("kind")?.as_str()?;
    if kind != "webfig-key" && kind != "webfig-https" {
        return None;
    }
    // no fields: everything comes from the router and tikontrol's CA, nothing from the request
    if v.as_object().is_some_and(|o| o.len() > 1) {
        return Some(Err(format!("invalid {kind} intent: it takes no fields")));
    }
    Some(if kind == "webfig-key" { key(r) } else { https(r) })
}

fn on(row: &Row, k: &str) -> bool {
    row.get(k).is_some_and(|v| v == "true" || v == "yes")
}

/// The router's private IPv4 addresses, then the static DNS names pointing at them that the CA may vouch for.
pub fn names(r: &mut dyn Router) -> Result<Vec<String>, String> {
    let mut ips: Vec<String> = vec![];
    for a in r.run(&Cmd::new("/ip/address/print").proplist(&["address", "disabled"]))? {
        let ip = a.get("address").and_then(|x| x.split('/').next()).and_then(|x| x.parse::<Ipv4Addr>().ok());
        if let Some(ip) = ip.filter(|ip| ip.is_private() && !on(&a, "disabled")) {
            ips.push(ip.to_string());
        }
    }
    let mut names = vec![];
    for d in r.run(&Cmd::new("/ip/dns/static/print").proplist(&["name", "address", "disabled"]))? {
        let n = d.get("name").map(|n| n.to_ascii_lowercase()).unwrap_or_default();
        if !on(&d, "disabled") && d.get("address").is_some_and(|a| ips.contains(a)) && crate::tlscert::allowed_name(&n) {
            names.push(n);
        }
    }
    ips.extend(names);
    ips.sort();
    ips.dedup();
    if ips.is_empty() {
        return Err("the router has no private IPv4 address to put in the certificate".into());
    }
    Ok(ips)
}

fn file(r: &mut dyn Router, name: &str, keys: &[&str]) -> Result<Option<Row>, String> {
    Ok(r.run(&Cmd::new("/file/print").proplist(keys).query("name", name))?.into_iter().next())
}

fn key(r: &mut dyn Router) -> Result<Plan, String> {
    names(r)?;
    if file(r, REQ, &["name"])?.is_some() || file(r, REQ_KEY, &["name"])?.is_some() {
        return Err(format!("{REQ} or {REQ_KEY} is already on the router: run step 2 (Turn on HTTPS), or remove both files in RouterOS (Files) to start again"));
    }
    let pass = crate::tlscert::router_key_passphrase()?;
    let mut ops = vec![];
    let mut undo = vec![Op::on(FILE, "remove", "name", REQ, vec![]), Op::on(FILE, "remove", "name", REQ_KEY, vec![])];
    if r.run(&Cmd::new(&format!("{CERT}/print")).proplist(&["name"]).query("name", TEMPLATE))?.is_empty() {
        ops.push(Op::add(CERT, vec![("name", TEMPLATE.into()), ("common-name", "router".into()), ("key-size", "prime256v1".into())]));
        undo.push(Op::on(CERT, "remove", "name", TEMPLATE, vec![]));
    }
    ops.push(Op { path: CERT, verb: "create-certificate-request", find: None, args: vec![("template", TEMPLATE.into()), ("key-passphrase", pass)] });
    let mut p = plan("HTTPS for WebFig, step 1: the router makes its key".into(), Risk::Low, ops, undo);
    p.warnings.push(format!("The router creates a private key and a certificate request ({REQ}). The key stays on the router, in {REQ_KEY}, protected by a passphrase only tikontrol knows. Keep this change, then run step 2."));
    Ok(p)
}

fn https(r: &mut dyn Router) -> Result<Plan, String> {
    let names = names(r)?;
    let req = file(r, REQ, &["name", "contents"])?.and_then(|f| f.get("contents").cloned()).filter(|c| !c.is_empty());
    let (Some(req), true) = (req, file(r, REQ_KEY, &["name"])?.is_some()) else {
        return Err(format!("no certificate request on the router: run step 1 first ({REQ} and {REQ_KEY} are needed)"));
    };
    let crt = crate::tlscert::sign_router_request(&req, &names)?;
    let svc = r.run(&Cmd::new(&format!("{SVC}/print")).proplist(&["name", "disabled", "certificate"]))?;
    let get = |n: &str| svc.iter().find(|s| s.get("name").map(String::as_str) == Some(n)).cloned().ok_or(format!("no {n} service on this router"));
    let (www, ssl) = (get("www")?, get("www-ssl")?);
    let old_cert = ssl.get("certificate").filter(|c| !c.is_empty()).cloned().unwrap_or_else(|| "none".into());

    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs();
    let name = format!("tik-webfig-{secs}");
    let crt_file = format!("{name}.crt");
    let import = |file: &str, pass: String| Op { path: CERT, verb: "import", find: None, args: vec![("file-name", file.to_string()), ("name", name.clone()), ("passphrase", pass)] };
    // RouterOS deletes the imported files; the request itself is spent
    let ops = vec![
        Op::add(FILE, vec![("name", crt_file.clone()), ("contents", crt)]),
        import(&crt_file, String::new()),
        import(REQ_KEY, crate::tlscert::router_key_passphrase()?),
        Op::on(FILE, "remove", "name", REQ, vec![]),
        Op::on(SVC, "set", "name", "www-ssl", vec![("certificate", name.clone()), ("disabled", "no".into())]),
        Op::on(SVC, "set", "name", "www", vec![("disabled", "yes".into())]),
    ];
    let undo = vec![
        Op::on(SVC, "set", "name", "www", vec![("disabled", yn(on(&www, "disabled")))]),
        Op::on(SVC, "set", "name", "www-ssl", vec![("certificate", old_cert), ("disabled", yn(on(&ssl, "disabled")))]),
        Op::on(CERT, "remove", "name", &name, vec![]),
        // only there if an import failed half way
        Op::on(FILE, "remove", "name", &crt_file, vec![]),
    ];
    let mut p = plan("HTTPS for WebFig".into(), Risk::High, ops, undo);
    p.warnings.push("High risk: changes management access (/ip/service). Plain-HTTP WebFig goes off; keep WinBox or SSH at hand until you have opened the HTTPS address.".into());
    p.warnings.push(format!("The certificate covers {} and is valid for 397 days; run both steps again before it runs out.", names.join(", ")));
    p.warnings.push("Browsers show a padlock only on devices that trust tikontrol's local CA. Others see a certificate warning instead of plain HTTP.".into());
    p.warnings.push("If this change is reverted, the new certificate and the router's key are removed: run both steps again.".into());
    Ok(p)
}

#[cfg(test)]
mod tests {
    use crate::apply::{compile_value, revert_script};
    use crate::ros::queries::tests::{row, Fake};
    use serde_json::json;

    fn router(req: bool) -> Fake {
        let mut f = Fake::default();
        f.answers.insert("/ip/address/print".into(), Ok(vec![row(&[("address", "10.20.0.1/24"), ("disabled", "false")]), row(&[("address", "203.0.113.5/24"), ("disabled", "false")])]));
        f.answers.insert(
            "/ip/dns/static/print".into(),
            Ok(vec![row(&[("name", "router.lan"), ("address", "10.20.0.1")]), row(&[("name", "evil.example.com"), ("address", "10.20.0.1")]), row(&[("name", "nas.lan"), ("address", "10.20.0.9")])]),
        );
        f.answers.insert("/ip/service/print".into(), Ok(vec![row(&[("name", "www"), ("disabled", "false"), ("certificate", "")]), row(&[("name", "www-ssl"), ("disabled", "true"), ("certificate", "")])]));
        if req {
            // ponytail: the Fake answers by path, so both files come back for either query
            f.answers.insert("/file/print".into(), Ok(vec![row(&[("name", super::REQ), ("contents", crate::tlscert::tests::TEST_CSR)])]));
        }
        f
    }

    #[test]
    fn step_one_makes_the_key_on_the_router() {
        crate::tlscert::test_ca();
        let p = compile_value(&json!({"kind": "webfig-key"}), &mut router(false)).unwrap();
        assert_eq!(p.cli()[0], "/certificate/add name=tik-webfig-request common-name=router key-size=prime256v1");
        assert_eq!(p.cli()[1], r#"/certificate/create-certificate-request template=tik-webfig-request key-passphrase="********""#);
        assert!(p.undo_cli().iter().any(|c| c.contains("certificate-request_key.pem")));
        let e = compile_value(&json!({"kind": "webfig-key"}), &mut router(true)).unwrap_err();
        assert!(e.contains("step 2"), "{e}");
    }

    #[test]
    fn step_two_signs_the_request_and_switches_www_off_with_an_exact_inverse() {
        crate::tlscert::test_ca();
        let p = compile_value(&json!({"kind": "webfig-https"}), &mut router(true)).unwrap();
        let cli = p.cli();
        assert!(cli[0].starts_with("/file/add name=tik-webfig-") && cli[0].contains("-----BEGIN CERTIFICATE-----\\n"), "{}", cli[0]);
        assert!(!cli.join(" ").contains("PRIVATE KEY"));
        assert!(cli[1].starts_with("/certificate/import file-name=tik-webfig-") && cli[2].contains("file-name=certificate-request_key.pem") && cli[2].ends_with(r#"passphrase="********""#));
        assert!(cli[4].starts_with(r#"/ip/service/set [find name="www-ssl"] certificate=tik-webfig-"#) && cli[4].ends_with("disabled=no"));
        assert_eq!(cli[5], r#"/ip/service/set [find name="www"] disabled=yes"#);
        let undo = p.undo_cli();
        assert_eq!(undo[0], r#"/ip/service/set [find name="www"] disabled=no"#);
        assert_eq!(undo[1], r#"/ip/service/set [find name="www-ssl"] certificate=none disabled=yes"#);
        assert!(undo[2].starts_with(r#"/certificate/remove [find name="tik-webfig-"#));
        assert!(revert_script("abc", &p).is_ok_and(|s| !s.contains('\n')));
        // only the router's private address and the local name pointing at it
        assert!(p.warnings[1].contains("covers 10.20.0.1, router.lan and"), "{}", p.warnings[1]);
        assert!(!p.warnings[1].contains("203.0.113.5") && !p.warnings[1].contains("evil") && !p.warnings[1].contains("nas.lan"));
        assert!(compile_value(&json!({"kind": "webfig-https"}), &mut router(false)).unwrap_err().contains("step 1"));
    }

    #[test]
    fn refuses_fields_and_routers_without_a_private_address() {
        crate::tlscert::test_ca();
        assert!(compile_value(&json!({"kind": "webfig-https", "names": ["example.com"]}), &mut router(true)).is_err());
        let mut f = Fake::default();
        f.answers.insert("/ip/address/print".into(), Ok(vec![row(&[("address", "203.0.113.5/24")])]));
        assert!(compile_value(&json!({"kind": "webfig-key"}), &mut f).unwrap_err().contains("private IPv4"));
    }
}
