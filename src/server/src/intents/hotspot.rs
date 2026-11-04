//! Hotspot / guest portal: a RouterOS hotspot on a guest interface (address, pool, DHCP, hotspot
//! profile, user profile, server), terms-only login (trial) or voucher login, and voucher batches as
//! `/ip/hotspot/user` entries. The login page stays RouterOS's default HTML (no custom upload yet).
//! Untested on a real router.

use super::internet::{duration, fresh, parse, readd, rm, rows, with};
use crate::apply::{ipv4, plan, rate, ros_name, Op, Plan, Risk};
use crate::ros::conn::Router;
use serde::Deserialize;
use serde_json::Value;
use std::net::Ipv4Addr;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum I {
    GuestHotspot {
        interface: String,
        address: String,
        #[serde(default)]
        down: String,
        #[serde(default)]
        up: String,
        #[serde(default = "day")]
        session: String,
        #[serde(default = "terms")]
        login: String,
    },
    RemoveGuestHotspot {},
    HotspotVouchers { codes: Vec<String>, uptime: String, #[serde(default = "one")] shared_users: u8 },
    RemoveHotspotVouchers { batch: String },
}

fn day() -> String {
    "1d".into()
}
fn terms() -> String {
    "terms".into()
}
fn one() -> u8 {
    1
}

const KINDS: &[&str] = &["guest-hotspot", "remove-guest-hotspot", "hotspot-vouchers", "remove-hotspot-vouchers"];

pub fn compile(v: &Value, r: &mut dyn Router) -> Option<Result<Plan, String>> {
    parse::<I>(v, KINDS).map(|i| i.and_then(|i| build(i, r)))
}

const NAME: &str = "tik-guest";
const C: &str = "tik:hotspot";
const ADDR: &str = "/ip/address";
const POOL: &str = "/ip/pool";
const NET: &str = "/ip/dhcp-server/network";
const DHCPS: &str = "/ip/dhcp-server";
const UPROF: &str = "/ip/hotspot/user/profile";
const HPROF: &str = "/ip/hotspot/profile";
const HS: &str = "/ip/hotspot";
const USER: &str = "/ip/hotspot/user";

fn code(s: &str) -> Result<String, String> {
    let ok = (4..=16).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    ok.then(|| s.to_string()).ok_or_else(|| "voucher codes are 4-16 capital letters and digits".into())
}

fn build(i: I, r: &mut dyn Router) -> Result<Plan, String> {
    use I::*;
    Ok(match i {
        GuestHotspot { interface, address, down, up, session, login } => {
            let ifn = ros_name(&interface)?;
            let (a, n) = address.trim().split_once('/').ok_or("address needs a prefix, e.g. 10.10.10.1/24")?;
            let n: u32 = n.parse().map_err(|_| "invalid prefix length")?;
            if !(16..=29).contains(&n) {
                return Err("guest network prefix must be /16 to /29".into());
            }
            let gw = ipv4(a)?;
            if !gw.is_private() {
                return Err("the guest network must use a private address range".into());
            }
            let mask = u32::MAX << (32 - n);
            let (g, net) = (u32::from(gw), u32::from(gw) & mask);
            let bcast = net | !mask;
            let (start, end) = (g + 1, bcast - 1);
            if g == net || g >= bcast || start >= end {
                return Err("the router's address must be a host address with room after it for guests (e.g. 10.10.10.1/24)".into());
            }
            let (start, end, netaddr) = (Ipv4Addr::from(start), Ipv4Addr::from(end), Ipv4Addr::from(net));
            let session = duration(&session)?;
            let login_by = match login.as_str() {
                "terms" => "trial",
                "voucher" => "http-chap,http-pap",
                _ => return Err("login must be terms or voucher".into()),
            };
            for (path, k, v) in [(HS, "name", NAME), (HPROF, "name", NAME), (POOL, "name", NAME), (DHCPS, "name", NAME), (UPROF, "name", NAME)] {
                fresh(r, path, k, v, &format!("{path} {NAME}"))?;
            }
            let mut uprof = vec![("name", NAME.to_string()), ("session-timeout", session.clone())];
            if !down.is_empty() || !up.is_empty() {
                // rate-limit is rx/tx as the router sees it: guest upload / guest download
                uprof.push(("rate-limit", format!("{}/{}", rate(&up)?, rate(&down)?)));
            }
            let mut hprof = vec![("name", NAME.to_string()), ("hotspot-address", gw.to_string()), ("login-by", login_by.to_string())];
            if login == "terms" {
                hprof.push(("trial-uptime-limit", session.clone()));
                hprof.push(("trial-user-profile", NAME.to_string()));
            }
            let ops = vec![
                Op::add(ADDR, vec![("address", format!("{gw}/{n}")), ("interface", ifn.clone()), ("comment", C.into())]),
                Op::add(POOL, vec![("name", NAME.into()), ("ranges", format!("{start}-{end}")), ("comment", C.into())]),
                Op::add(NET, vec![("address", format!("{netaddr}/{n}")), ("gateway", gw.to_string()), ("dns-server", gw.to_string()), ("comment", C.into())]),
                Op::add(DHCPS, vec![("name", NAME.into()), ("interface", ifn.clone()), ("address-pool", NAME.into()), ("lease-time", "1h".into()), ("comment", C.into())]),
                Op::add(UPROF, uprof),
                Op::add(HPROF, hprof),
                Op::add(HS, vec![("name", NAME.into()), ("interface", ifn.clone()), ("address-pool", NAME.into()), ("profile", NAME.into()), ("disabled", "no".into())]),
            ];
            let undo = vec![rm(HS, "name", NAME), rm(HPROF, "name", NAME), rm(UPROF, "name", NAME), rm(DHCPS, "name", NAME), rm(NET, "comment", C), rm(POOL, "name", NAME), rm(ADDR, "comment", C)];
            with(plan(format!("Guest hotspot on {ifn} ({netaddr}/{n}, {})", if login == "terms" { "accept terms" } else { "vouchers" }), Risk::High, ops, undo), &[
                "High risk: choose an interface only guests use (a guest VLAN or guest Wi-Fi). On your main LAN every device, including this browser, lands on the login page.",
                "Guests can still reach your LAN unless a firewall rule blocks it (Security). The walled garden is empty.",
                "The login page is RouterOS's default; a branded page (title, message, colour) isn't uploaded yet.",
                "Not yet tested on a real router.",
            ])
        }
        RemoveGuestHotspot {} => {
            if rows(r, HS, &[".id"], "name", NAME)?.is_empty() {
                return Err("only the guest hotspot set up by tikontrol can be removed here".into());
            }
            let mut undo = readd(r, ADDR, "comment", C, &["address", "interface"])?;
            undo.extend(readd(r, POOL, "name", NAME, &["name", "ranges", "comment"])?);
            undo.extend(readd(r, NET, "comment", C, &["address", "gateway", "dns-server"])?);
            undo.extend(readd(r, DHCPS, "name", NAME, &["name", "interface", "address-pool", "lease-time", "comment"])?);
            undo.extend(readd(r, UPROF, "name", NAME, &["name", "session-timeout", "rate-limit"])?);
            undo.extend(readd(r, HPROF, "name", NAME, &["name", "hotspot-address", "login-by", "trial-uptime-limit", "trial-user-profile"])?);
            undo.extend(readd(r, HS, "name", NAME, &["name", "interface", "address-pool", "profile"])?);
            let ops = vec![rm(HS, "name", NAME), rm(HPROF, "name", NAME), rm(UPROF, "name", NAME), rm(DHCPS, "name", NAME), rm(NET, "comment", C), rm(POOL, "name", NAME), rm(ADDR, "comment", C)];
            with(plan("Remove the guest hotspot".into(), Risk::Medium, ops, undo), &["Guests are disconnected. Voucher users stay until you remove them."])
        }
        HotspotVouchers { codes, uptime, shared_users } => {
            if codes.is_empty() || codes.len() > 100 {
                return Err("create 1 to 100 vouchers at a time".into());
            }
            let codes = codes.iter().map(|c| code(c)).collect::<Result<Vec<_>, _>>()?;
            if codes.iter().enumerate().any(|(i, c)| codes[..i].contains(c)) {
                return Err("voucher codes must be unique".into());
            }
            let uptime = duration(&uptime)?;
            if !(1..=10).contains(&shared_users) {
                return Err("a voucher can be shared by 1-10 devices".into());
            }
            let c = format!("tik:voucher:{}", codes[0]);
            let has_profile = !rows(r, UPROF, &[".id"], "name", NAME)?.is_empty();
            let ops = codes
                .iter()
                .map(|code| {
                    let mut a = vec![("name", code.clone()), ("limit-uptime", uptime.clone()), ("shared-users", shared_users.to_string())];
                    if has_profile {
                        a.push(("profile", NAME.into()));
                    }
                    a.push(("comment", c.clone()));
                    Op::add(USER, a)
                })
                .collect();
            with(plan(format!("Create {} voucher{} ({uptime} each)", codes.len(), if codes.len() == 1 { "" } else { "s" }), Risk::Low, ops, vec![rm(USER, "comment", &c)]), &[
                "Each code is the guest's login (no password): anyone holding one gets online. Print or hand them out, don't post them.",
                "The hotspot profile must allow voucher login (http-chap/http-pap). Not yet tested on a real router.",
            ])
        }
        RemoveHotspotVouchers { batch } => {
            let c = format!("tik:voucher:{}", code(&batch)?);
            let undo = readd(r, USER, "comment", &c, &["name", "limit-uptime", "shared-users", "profile"])?;
            if undo.is_empty() {
                return Err("no vouchers from that batch".into());
            }
            plan(format!("Remove {} voucher{}", undo.len(), if undo.len() == 1 { "" } else { "s" }), Risk::Low, vec![rm(USER, "comment", &c)], undo)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ros::queries::tests::{row, Fake};
    use serde_json::json;

    fn ok(v: Value, f: &mut Fake) -> Plan {
        compile(&v, f).expect("own kind").expect("compiles")
    }

    #[test]
    fn every_kind_is_registered() {
        assert!(compile(&json!({"kind": "block-domain"}), &mut Fake::default()).is_none());
        for k in KINDS {
            if let Err(e) = compile(&json!({"kind": k}), &mut Fake::default()).unwrap() {
                assert!(!e.contains("unknown variant"), "{k}: {e}");
            }
        }
    }

    #[test]
    fn guest_hotspot_builds_network_and_inverse() {
        let p = ok(json!({"kind": "guest-hotspot", "interface": "guest-vlan", "address": "10.10.10.1/24", "down": "20M", "up": "5M"}), &mut Fake::default());
        assert_eq!(p.risk, Risk::High);
        let cli = p.cli().join("\n");
        assert!(cli.contains("ranges=10.10.10.2-10.10.10.254"), "{cli}");
        assert!(cli.contains("/ip/dhcp-server/network/add address=10.10.10.0/24 gateway=10.10.10.1"));
        assert!(cli.contains("rate-limit=5M/20M"));
        assert!(cli.contains("login-by=trial trial-uptime-limit=1d"));
        assert_eq!(p.undo.len(), p.ops.len());
        for bad in [json!({"kind": "guest-hotspot", "interface": "x", "address": "10.10.10.1"}), json!({"kind": "guest-hotspot", "interface": "x", "address": "8.8.8.1/24"}), json!({"kind": "guest-hotspot", "interface": "x", "address": "10.10.10.0/24"}), json!({"kind": "guest-hotspot", "interface": "x", "address": "10.10.10.1/24", "login": "radius"})] {
            assert!(compile(&bad, &mut Fake::default()).unwrap().is_err(), "{bad}");
        }
        let mut f = Fake::default();
        f.answers.insert("/ip/hotspot/print".into(), Ok(vec![row(&[(".id", "*1")])]));
        assert!(compile(&json!({"kind": "guest-hotspot", "interface": "x", "address": "10.10.10.1/24"}), &mut f).unwrap().is_err());
    }

    #[test]
    fn vouchers_are_validated_and_batched() {
        let p = ok(json!({"kind": "hotspot-vouchers", "codes": ["ABCDE", "FGHJK"], "uptime": "1d"}), &mut Fake::default());
        assert_eq!(p.ops.len(), 2);
        assert_eq!(p.undo_cli(), [r#"/ip/hotspot/user/remove [find comment="tik:voucher:ABCDE"]"#]);
        assert!(compile(&json!({"kind": "hotspot-vouchers", "codes": ["ab"], "uptime": "1d"}), &mut Fake::default()).unwrap().is_err());
        assert!(compile(&json!({"kind": "hotspot-vouchers", "codes": ["ABCDE", "ABCDE"], "uptime": "1d"}), &mut Fake::default()).unwrap().is_err());
        assert!(compile(&json!({"kind": "hotspot-vouchers", "codes": ["ABCDE"], "uptime": "forever"}), &mut Fake::default()).unwrap().is_err());
    }
}
