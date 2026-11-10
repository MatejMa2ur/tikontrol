//! Hotspot / guest portal: a RouterOS hotspot on a guest interface (address, pool, DHCP, hotspot
//! profile, user profile, server), terms-only login (trial) or voucher login, and voucher batches as
//! `/ip/hotspot/user` entries, and an optional branded login page (title, message, colour) generated
//! here and written to the router as a file. Untested on a real router.

use super::internet::{duration, fresh, parse, readd, rm, rows, with};
use crate::apply::{ipv4, plan, rate, ros_name, Op, Plan, Risk};
use crate::ros::cmd::Cmd;
use crate::ros::conn::Router;
use serde::Deserialize;
use serde_json::Value;
use std::net::Ipv4Addr;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum I {
    GuestHotspot {
        /// an existing guest interface or VLAN; empty when `guest_ssid` names the guest Wi-Fi instead
        #[serde(default)]
        interface: String,
        /// the guest SSID `wifi-guest` creates (same master and SSID): the portal goes on its virtual AP interface.
        /// Sent in one batch with that `wifi-guest`, before it.
        #[serde(default)]
        guest_ssid: Option<GuestSsid>,
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
    HotspotPage { title: String, message: String, color: String },
    RemoveHotspotPage {},
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GuestSsid {
    master: String,
    ssid: String,
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

const KINDS: &[&str] = &["guest-hotspot", "remove-guest-hotspot", "hotspot-vouchers", "remove-hotspot-vouchers", "hotspot-page", "remove-hotspot-page"];

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
// RouterOS adds these disabled placeholders (chain unused-hs-chain) a moment after a hotspot is enabled, too late to
// remove in the same apply, and leaves them behind when the hotspot goes
const PLACEHOLDER: &str = "place hotspot rules here";
const PH_PATHS: [&str; 2] = ["/ip/firewall/filter", "/ip/firewall/nat"];

fn placeholders(r: &mut dyn Router) -> Result<bool, String> {
    let mut any = false;
    for p in PH_PATHS {
        any |= !rows(r, p, &[".id"], "comment", PLACEHOLDER)?.is_empty();
    }
    Ok(any)
}

fn rm_placeholders() -> [Op; 2] {
    PH_PATHS.map(|p| rm(p, "comment", PLACEHOLDER))
}

const FILE: &str = "/file";
/// The branded page lives in its own folder, so RouterOS's own hotspot folder is never overwritten.
const DIR: &str = "tik-guest-html";
const PAGE: &str = "tik-guest-html/login.html";
/// Below the API value limit (4096); RouterOS's `/file` contents limit is about the same.
const PAGE_MAX: usize = 4000;

/// Escapes text for HTML element content and attribute values. `$` is escaped too: RouterOS expands
/// `$(name)` in hotspot pages, so a title of `$(mac)` must stay literal.
pub fn html_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            '$' => o.push_str("&#36;"),
            _ => o.push(c),
        }
    }
    o
}

/// Strict `#rrggbb`, lower-cased.
fn colour(s: &str) -> Result<String, String> {
    let s = s.trim();
    let ok = s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit());
    ok.then(|| s.to_ascii_lowercase()).ok_or_else(|| "colour must be #rrggbb".into())
}

/// Text on the brand colour: near-black or white, whichever has more contrast (WCAG relative luminance).
fn on_colour(c: &str) -> &'static str {
    let ch = |i: usize| {
        let v = u8::from_str_radix(&c[i..i + 2], 16).unwrap_or(0) as f64 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    let l = 0.2126 * ch(1) + 0.7152 * ch(3) + 0.0722 * ch(5);
    if (l + 0.05) / 0.05 > 1.05 / (l + 0.05) { "#111111" } else { "#ffffff" }
}

fn text(s: &str, max: usize, what: &str) -> Result<String, String> {
    let s = s.trim();
    let ok = s.chars().count() <= max && !s.chars().any(char::is_control);
    ok.then(|| s.to_string()).ok_or_else(|| format!("{what} is at most {max} characters, on one line"))
}

/// The login page, on one line (API values can't hold newlines). `terms` logs in as a trial user
/// (`T-<mac>`); otherwise the guest types a voucher code, sent as the user name with an empty password.
pub fn login_page(title: &str, message: &str, color: &str, terms: bool) -> Result<String, String> {
    let (t, m, c) = (html_escape(&text(title, 60, "the title")?), html_escape(&text(message, 240, "the message")?), colour(color)?);
    let fg = on_colour(&c);
    let fields = if terms {
        r#"<input type="hidden" name="username" value="T-$(mac)"><input type="hidden" name="password" value=""><label class="t"><input type="checkbox" required> I accept the terms of use</label>"#
    } else {
        r#"<label for="u">Voucher code</label><input id="u" type="text" name="username" autocomplete="off" autocapitalize="characters" required><input type="hidden" name="password" value="">"#
    };
    let page = format!(
        concat!(
            r#"<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{t}</title>"#,
            r#"<style>body{{margin:0;font-family:system-ui,sans-serif;background:#f4f5f7;color:#222;display:grid;place-items:center;min-height:100vh}}"#,
            r#"main{{background:#fff;width:min(340px,90vw);box-sizing:border-box;padding:28px;border-radius:16px;text-align:center;box-shadow:0 4px 24px #0002;border-top:6px solid {c}}}"#,
            r#"h1{{font-size:22px;margin:0 0 8px}}p{{color:#444;font-size:15px}}label{{display:block;font-size:14px;margin:8px 0 4px}}.t{{display:flex;gap:6px;justify-content:center}}"#,
            r#"input[type=text]{{width:100%;box-sizing:border-box;padding:10px;font-size:16px;border:1px solid #767676;border-radius:8px}}"#,
            r#"button{{width:100%;margin-top:14px;padding:12px;border:0;border-radius:10px;background:{c};color:{fg};font-size:16px;font-weight:600}}"#,
            r#"button:focus-visible,input:focus-visible{{outline:3px solid #222;outline-offset:2px}}.e{{color:#b00020}}</style></head>"#,
            r#"<body><main><h1>{t}</h1><p>{m}</p>$(if error)<p class="e" role="alert">$(error)</p>$(endif)"#,
            r#"<form name="login" action="$(link-login-only)" method="post"><input type="hidden" name="dst" value="$(link-orig)"><input type="hidden" name="popup" value="false">{fields}"#,
            r#"<button type="submit">Connect</button></form></main></body></html>"#
        ),
        t = t,
        m = m,
        c = c,
        fg = fg,
        fields = fields
    );
    if page.len() > PAGE_MAX {
        return Err(format!("the page is too long for the router ({} bytes, at most {PAGE_MAX}): shorten the title or message", page.len()));
    }
    Ok(page)
}

/// The branded page on the router: None if absent, Some(None) if its contents can't be read back.
fn page_now(r: &mut dyn Router) -> Result<Option<Option<String>>, String> {
    Ok(rows(r, FILE, &["name", "contents"], "name", PAGE)?.first().map(|row| row.get("contents").filter(|c| !c.is_empty()).cloned()))
}

fn code(s: &str) -> Result<String, String> {
    let ok = (4..=16).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    ok.then(|| s.to_string()).ok_or_else(|| "voucher codes are 4-16 capital letters and digits".into())
}

fn build(i: I, r: &mut dyn Router) -> Result<Plan, String> {
    use I::*;
    Ok(match i {
        GuestHotspot { interface, guest_ssid, address, down, up, session, login } => {
            let ifn = match &guest_ssid {
                Some(_) if !interface.trim().is_empty() => return Err("give either an interface or a guest SSID, not both".into()),
                Some(g) => super::wifi::guest_iface(&g.master, &g.ssid)?,
                None => ros_name(&interface)?,
            };
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
            let mut undo = vec![rm(HS, "name", NAME), rm(HPROF, "name", NAME), rm(UPROF, "name", NAME), rm(DHCPS, "name", NAME), rm(NET, "comment", C), rm(POOL, "name", NAME), rm(ADDR, "comment", C)];
            if !placeholders(r)? {
                undo.extend(rm_placeholders()); // a revert leaves nothing behind
            }
            let first = if guest_ssid.is_some() {
                "High risk: the portal goes on the new guest Wi-Fi's own interface. If that interface ends up in your LAN bridge, the bridge's devices don't see the portal and guests get no address: check it isn't a bridge port after applying."
            } else {
                "High risk: choose an interface only guests use (a guest VLAN or guest Wi-Fi). On your main LAN every device, including this browser, lands on the login page."
            };
            with(plan(format!("Guest hotspot on {ifn} ({netaddr}/{n}, {})", if login == "terms" { "accept terms" } else { "vouchers" }), Risk::High, ops, undo), &[
                first,
                "Guests can still reach your LAN unless a firewall rule blocks it (Security). The walled garden is empty.",
                "The login page is RouterOS's default; a branded page (title, message, colour) isn't uploaded yet.",
                "Tested on CHR (RouterOS in a VM); the guest login itself is not yet tested with a real device.",
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
            undo.extend(readd(r, HPROF, "name", NAME, &["name", "hotspot-address", "login-by", "trial-uptime-limit", "trial-user-profile", "html-directory"])?);
            undo.extend(readd(r, HS, "name", NAME, &["name", "interface", "address-pool", "profile"])?);
            let mut ops = vec![rm(HS, "name", NAME), rm(HPROF, "name", NAME), rm(UPROF, "name", NAME), rm(DHCPS, "name", NAME), rm(NET, "comment", C), rm(POOL, "name", NAME), rm(ADDR, "comment", C)];
            // with the last hotspot gone they are meaningless; re-adding the hotspot on undo brings them back
            if r.run(&Cmd::new("/ip/hotspot/print").proplist(&["name"]))?.len() == 1 && placeholders(r)? {
                ops.extend(rm_placeholders());
            }
            with(plan("Remove the guest hotspot".into(), Risk::Medium, ops, undo), &["Guests are disconnected. Voucher users and the branded login page file stay until you remove them."])
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
            // shared-users is a user-profile setting on RouterOS, not a per-user one: a voucher for several devices
            // uses profile tik-guest-xN (made on first use, copying the guest profile's limits).
            // ponytail: profile is left after its vouchers are removed; clean up with the guest hotspot if it matters
            let guest = rows(r, UPROF, &["session-timeout", "rate-limit"], "name", NAME)?;
            let (mut ops, mut undo, profile) = (vec![], vec![], if shared_users > 1 { Some(format!("{NAME}-x{shared_users}")) } else { guest.first().map(|_| NAME.to_string()) });
            if let Some(p) = profile.as_deref().filter(|p| *p != NAME) {
                if rows(r, UPROF, &[".id"], "name", p)?.is_empty() {
                    let mut a = vec![("name", p.to_string()), ("shared-users", shared_users.to_string())];
                    if let Some(g) = guest.first() {
                        a.extend(["session-timeout", "rate-limit"].into_iter().filter_map(|k| g.get(k).filter(|v| !v.is_empty()).map(|v| (k, v.clone()))));
                    }
                    ops.push(Op::add(UPROF, a));
                    undo.push(rm(UPROF, "name", p));
                }
            }
            for code in &codes {
                let mut a = vec![("name", code.clone()), ("limit-uptime", uptime.clone())];
                if let Some(p) = &profile {
                    a.push(("profile", p.clone()));
                }
                a.push(("comment", c.clone()));
                ops.push(Op::add(USER, a));
            }
            undo.insert(0, rm(USER, "comment", &c));
            with(plan(format!("Create {} voucher{} ({uptime} each)", codes.len(), if codes.len() == 1 { "" } else { "s" }), Risk::Low, ops, undo), &[
                "Each code is the guest's login (no password): anyone holding one gets online. Print or hand them out, don't post them.",
                "The hotspot profile must allow voucher login (http-chap/http-pap).",
            ])
        }
        RemoveHotspotVouchers { batch } => {
            let c = format!("tik:voucher:{}", code(&batch)?);
            let undo = readd(r, USER, "comment", &c, &["name", "limit-uptime", "profile"])?;
            if undo.is_empty() {
                return Err("no vouchers from that batch".into());
            }
            plan(format!("Remove {} voucher{}", undo.len(), if undo.len() == 1 { "" } else { "s" }), Risk::Low, vec![rm(USER, "comment", &c)], undo)
        }
        HotspotPage { title, message, color } => {
            let prof = rows(r, HPROF, &["login-by", "html-directory"], "name", NAME)?;
            let prof = prof.first().ok_or("set up the guest hotspot first")?;
            let terms = prof.get("login-by").is_some_and(|l| l.split(',').any(|x| x == "trial"));
            let page = login_page(&title, &message, &color, terms)?;
            let (mut ops, mut undo) = (vec![], vec![]);
            if rows(r, FILE, &[".id"], "name", DIR)?.is_empty() {
                ops.push(Op::add(FILE, vec![("name", DIR.into()), ("type", "directory".into())]));
                undo.push(rm(FILE, "name", DIR));
            }
            match page_now(r)? {
                None => {
                    ops.push(Op::add(FILE, vec![("name", PAGE.into()), ("contents", page)]));
                    undo.insert(0, rm(FILE, "name", PAGE));
                }
                Some(Some(old)) => {
                    ops.push(Op::on(FILE, "set", "name", PAGE, vec![("contents", page)]));
                    undo.insert(0, Op::on(FILE, "set", "name", PAGE, vec![("contents", old)]));
                }
                Some(None) => return Err("the router has a branded page tikontrol can't read back: switch to the default page first".into()),
            }
            let dir = prof.get("html-directory").cloned().unwrap_or_default();
            if dir != DIR {
                ops.push(Op::on(HPROF, "set", "name", NAME, vec![("html-directory", DIR.into())]));
                undo.insert(0, Op::on(HPROF, "set", "name", NAME, vec![("html-directory", dir)]));
            }
            with(plan(format!("Branded login page ({})", if terms { "accept terms" } else { "voucher" }), Risk::Medium, ops, undo), &[
                "The guest hotspot serves this page from its own folder (tik-guest-html); RouterOS's own hotspot files are left alone.",
                "Untested on a real router: check the login with a guest device before relying on it.",
            ])
        }
        RemoveHotspotPage {} => {
            let prof = rows(r, HPROF, &["html-directory"], "name", NAME)?;
            if prof.first().and_then(|p| p.get("html-directory")).map(String::as_str) != Some(DIR) {
                return Err("the guest hotspot doesn't use a branded page".into());
            }
            // back to the folder the router's default profile uses (hotspot, or flash/hotspot)
            let def = rows(r, HPROF, &["html-directory"], "default", "true")?.first().and_then(|p| p.get("html-directory").cloned()).filter(|d| d != DIR).unwrap_or_else(|| "hotspot".into());
            let mut ops = vec![Op::on(HPROF, "set", "name", NAME, vec![("html-directory", def)])];
            let mut undo = vec![];
            // the files go too when their contents can be put back on undo; otherwise they stay, unused
            if let Some(Some(old)) = page_now(r)? {
                ops.push(rm(FILE, "name", DIR));
                undo.push(Op::add(FILE, vec![("name", DIR.into()), ("type", "directory".into())]));
                undo.push(Op::add(FILE, vec![("name", PAGE.into()), ("contents", old)]));
            }
            undo.push(Op::on(HPROF, "set", "name", NAME, vec![("html-directory", DIR.into())]));
            with(plan("Use RouterOS's default login page".into(), Risk::Low, ops, undo), &["Guests see RouterOS's default login page again."])
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
        // RouterOS adds placeholder rules with the first hotspot: a revert removes them when the router had none
        assert_eq!(p.undo.len(), p.ops.len() + 2);
        assert_eq!(p.undo_cli().last().unwrap(), r#"/ip/firewall/nat/remove [find comment="place hotspot rules here"]"#);
        for bad in [json!({"kind": "guest-hotspot", "interface": "x", "address": "10.10.10.1"}), json!({"kind": "guest-hotspot", "interface": "x", "address": "8.8.8.1/24"}), json!({"kind": "guest-hotspot", "interface": "x", "address": "10.10.10.0/24"}), json!({"kind": "guest-hotspot", "interface": "x", "address": "10.10.10.1/24", "login": "radius"})] {
            assert!(compile(&bad, &mut Fake::default()).unwrap().is_err(), "{bad}");
        }
        let mut f = Fake::default();
        f.answers.insert("/ip/hotspot/print".into(), Ok(vec![row(&[(".id", "*1")])]));
        assert!(compile(&json!({"kind": "guest-hotspot", "interface": "x", "address": "10.10.10.1/24"}), &mut f).unwrap().is_err());
    }

    #[test]
    fn guest_hotspot_on_the_new_guest_ssid() {
        let v = json!({"kind": "guest-hotspot", "guest_ssid": {"master": "wlan1", "ssid": "Café Guests"}, "address": "10.10.10.1/24"});
        let p = ok(v, &mut Fake::default());
        assert!(p.cli()[0].contains("interface=tik-guest-cafguests-wlan1"), "{:?}", p.cli());
        assert!(p.cli().last().unwrap().contains("/ip/hotspot/add name=tik-guest interface=tik-guest-cafguests-wlan1"));
        for bad in [
            json!({"kind": "guest-hotspot", "address": "10.10.10.1/24"}),
            json!({"kind": "guest-hotspot", "interface": "ether5", "guest_ssid": {"master": "wlan1", "ssid": "G"}, "address": "10.10.10.1/24"}),
            json!({"kind": "guest-hotspot", "guest_ssid": {"master": "wlan1"}, "address": "10.10.10.1/24"}),
            json!({"kind": "guest-hotspot", "guest_ssid": {"master": "wlan1", "ssid": ""}, "address": "10.10.10.1/24"}),
            json!({"kind": "guest-hotspot", "guest_ssid": {"master": "wlan1;x", "ssid": "G"}, "address": "10.10.10.1/24"}),
            json!({"kind": "guest-hotspot", "guest_ssid": {"master": "wlan1", "ssid": "G", "vlan": 5}, "address": "10.10.10.1/24"}),
        ] {
            assert!(compile(&bad, &mut Fake::default()).unwrap().is_err(), "{bad}");
        }
    }

    /// Guest Wi-Fi and portal in one change: the SSID's interface is the hotspot's, and a revert removes both.
    #[test]
    fn guest_wifi_and_portal_in_one_batch() {
        for (pkg, master) in [("wireless", "wlan1"), ("wifi", "wifi1")] {
            let v = json!({"kind": "batch", "title": "Guest Wi-Fi with portal", "intents": [
                {"kind": "wifi-guest", "package": pkg, "master": master, "ssid": "Guests", "passphrase": "guestpass1"},
                {"kind": "guest-hotspot", "guest_ssid": {"master": master, "ssid": "Guests"}, "address": "10.10.10.1/24", "login": "voucher"},
            ]});
            let p = crate::apply::compile_value(&v, &mut Fake::default()).unwrap();
            let cli = p.cli();
            let ifn = format!("tik-guest-guests-{master}");
            assert!(cli.iter().any(|c| c.starts_with(&format!("/interface/{pkg}/add master-interface={master} name={ifn} "))), "{cli:?}");
            assert!(cli.iter().any(|c| c == &format!("/ip/hotspot/add name=tik-guest interface={ifn} address-pool=tik-guest profile=tik-guest disabled=no")), "{cli:?}");
            let wifi_at = cli.iter().position(|c| c.starts_with(&format!("/interface/{pkg}/add"))).unwrap();
            assert!(wifi_at < cli.iter().position(|c| c.starts_with("/ip/address/add")).unwrap(), "the SSID exists before the portal uses it");
            assert!(p.revertible);
            let undo = p.undo_cli();
            assert_eq!(undo[0], r#"/ip/hotspot/remove [find name="tik-guest"]"#, "the portal goes first");
            assert!(undo.iter().any(|c| c == &format!(r#"/interface/{pkg}/remove [find comment="tik:guest:guests:{master}"]"#)), "{undo:?}");
            let rs = crate::apply::revert_script("g", &p).unwrap();
            assert!(!rs.contains("guestpass1"));
        }
    }

    #[test]
    fn removing_the_last_hotspot_removes_the_placeholders() {
        let mut f = Fake::default();
        f.answers.insert("/ip/hotspot/print".into(), Ok(vec![row(&[(".id", "*1"), ("name", "tik-guest")])]));
        f.answers.insert("/ip/firewall/filter/print".into(), Ok(vec![row(&[(".id", "*9")])]));
        let p = ok(json!({"kind": "remove-guest-hotspot"}), &mut f);
        assert!(p.cli().last().unwrap().contains("place hotspot rules here"), "{:?}", p.cli());
        f.answers.insert("/ip/hotspot/print".into(), Ok(vec![row(&[("name", "tik-guest")]), row(&[("name", "other")])]));
        let p = ok(json!({"kind": "remove-guest-hotspot"}), &mut f);
        assert!(!p.cli().iter().any(|c| c.contains("place hotspot rules here")), "another hotspot still uses them");
    }

    #[test]
    fn vouchers_are_validated_and_batched() {
        let p = ok(json!({"kind": "hotspot-vouchers", "codes": ["ABCDE", "FGHJK"], "uptime": "1d"}), &mut Fake::default());
        assert_eq!(p.ops.len(), 2);
        assert_eq!(p.undo_cli(), [r#"/ip/hotspot/user/remove [find comment="tik:voucher:ABCDE"]"#]);
        let mut f = Fake::default();
        let p = ok(json!({"kind": "hotspot-vouchers", "codes": ["ABCDE"], "uptime": "1d", "shared_users": 3}), &mut f);
        assert_eq!(p.cli()[0], "/ip/hotspot/user/profile/add name=tik-guest-x3 shared-users=3", "shared-users lives on a profile");
        assert!(p.cli()[1].contains("profile=tik-guest-x3") && !p.cli()[1].contains("shared-users"));
        assert!(compile(&json!({"kind": "hotspot-vouchers", "codes": ["ab"], "uptime": "1d"}), &mut Fake::default()).unwrap().is_err());
        assert!(compile(&json!({"kind": "hotspot-vouchers", "codes": ["ABCDE", "ABCDE"], "uptime": "1d"}), &mut Fake::default()).unwrap().is_err());
        assert!(compile(&json!({"kind": "hotspot-vouchers", "codes": ["ABCDE"], "uptime": "forever"}), &mut Fake::default()).unwrap().is_err());
    }

    #[test]
    fn remove_vouchers_restores_the_batch() {
        assert!(compile(&json!({"kind": "remove-hotspot-vouchers", "batch": "ABCDE"}), &mut Fake::default()).unwrap().is_err(), "nothing to remove");
        assert!(compile(&json!({"kind": "remove-hotspot-vouchers", "batch": "ab\"]"}), &mut Fake::default()).unwrap().is_err());
        let mut f = Fake::default();
        f.answers.insert("/ip/hotspot/user/print".into(), Ok(vec![row(&[("name", "ABCDE"), ("limit-uptime", "1d"), ("profile", "tik-guest")])]));
        let p = ok(json!({"kind": "remove-hotspot-vouchers", "batch": "ABCDE"}), &mut f);
        assert_eq!(p.cli(), [r#"/ip/hotspot/user/remove [find comment="tik:voucher:ABCDE"]"#]);
        assert_eq!(p.undo_cli(), ["/ip/hotspot/user/add name=ABCDE limit-uptime=1d profile=tik-guest comment=tik:voucher:ABCDE"]);
        assert!(f.seen.iter().flatten().all(|w| !w.contains("password")), "never reads voucher passwords");
    }

    #[test]
    fn html_is_escaped_including_routeros_variables() {
        assert_eq!(html_escape(r#"<script>alert("x")</script> & 'y' $(mac)"#), "&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt; &amp; &#39;y&#39; &#36;(mac)");
        let p = login_page("<b>Café</b>", "\"><img src=x onerror=alert(1)> $(link-login)", "#2672F5", true).unwrap();
        assert!(!p.contains("<b>") && !p.contains("<img") && !p.contains("$(link-login)"), "{p}");
        assert!(p.contains("&lt;b&gt;Café&lt;/b&gt;") && p.contains("&#36;(link-login)"));
        assert!(p.contains("background:#2672f5"));
        // the page's own RouterOS variables stay
        assert!(p.contains(r#"action="$(link-login-only)""#) && p.contains(r#"value="T-$(mac)""#));
        assert!(!p.contains('\n'), "one line: API values can't hold newlines");
        let v = login_page("Hi", "", "#000000", false).unwrap();
        assert!(v.contains(r#"name="username""#) && v.contains("Voucher code") && !v.contains("T-$(mac)"));
    }

    #[test]
    fn page_inputs_are_strict() {
        for c in ["red", "#fff", "#12345g", "#1234567", "url(x)", "#123456;x", "", "#ééééé"] {
            assert!(login_page("t", "m", c, true).is_err(), "{c}");
        }
        assert!(login_page(&"x".repeat(61), "m", "#000000", true).is_err());
        assert!(login_page("t", &"x".repeat(241), "#000000", true).is_err());
        assert!(login_page("a\nb", "m", "#000000", true).is_err());
        // worst-case escaping still fits the router's limit
        assert!(login_page(&"$".repeat(60), &"\"".repeat(240), "#000000", true).is_ok());
        assert_eq!(on_colour("#ffff00"), "#111111");
        assert_eq!(on_colour("#1a3d8f"), "#ffffff");
    }

    #[test]
    fn page_needs_the_guest_hotspot_and_has_an_undo() {
        let v = json!({"kind": "hotspot-page", "title": "Welcome", "message": "Free Wi-Fi", "color": "#2672f5"});
        assert!(compile(&v, &mut Fake::default()).unwrap().is_err(), "no guest hotspot");
        assert!(compile(&json!({"kind": "hotspot-page", "title": "x", "message": "y", "color": "#2672f5", "html": "<x>"}), &mut Fake::default()).unwrap().is_err(), "no raw html");
        let mut f = Fake::default();
        f.answers.insert("/ip/hotspot/profile/print".into(), Ok(vec![row(&[("login-by", "http-chap,http-pap"), ("html-directory", "flash/hotspot")])]));
        let p = ok(v.clone(), &mut f);
        assert_eq!(p.cli()[0], "/file/add name=tik-guest-html type=directory");
        assert!(p.cli()[1].starts_with("/file/add name=tik-guest-html/login.html contents=") && p.cli()[1].contains("Voucher code"));
        assert_eq!(p.cli()[2], r#"/ip/hotspot/profile/set [find name="tik-guest"] html-directory=tik-guest-html"#);
        assert_eq!(p.undo_cli(), [r#"/ip/hotspot/profile/set [find name="tik-guest"] html-directory=flash/hotspot"#, r#"/file/remove [find name="tik-guest-html/login.html"]"#, r#"/file/remove [find name="tik-guest-html"]"#]);
        assert!(crate::apply::revert_script("x", &p).is_ok());
        // re-branding replaces the contents and puts the old ones back on undo
        f.answers.insert("/ip/hotspot/profile/print".into(), Ok(vec![row(&[("login-by", "trial"), ("html-directory", "tik-guest-html")])]));
        f.answers.insert("/file/print".into(), Ok(vec![row(&[(".id", "*1"), ("name", PAGE), ("contents", "<p>old</p>")])]));
        let p = ok(v.clone(), &mut f);
        assert_eq!(p.ops.len(), 1);
        assert!(p.cli()[0].starts_with(r#"/file/set [find name="tik-guest-html/login.html"] contents="#) && p.cli()[0].contains("T-\\$(mac)"), "{}", p.cli()[0]);
        assert_eq!(p.undo_cli(), [r#"/file/set [find name="tik-guest-html/login.html"] contents="<p>old</p>""#]);
        f.answers.insert("/file/print".into(), Ok(vec![row(&[(".id", "*1"), ("name", PAGE)])]));
        assert!(compile(&v, &mut f).unwrap().is_err(), "unreadable page: no undo");
    }

    #[test]
    fn removing_the_page_restores_the_default_folder() {
        assert!(compile(&json!({"kind": "remove-hotspot-page"}), &mut Fake::default()).unwrap().is_err());
        let mut f = Fake::default();
        f.answers.insert("/ip/hotspot/profile/print".into(), Ok(vec![row(&[("html-directory", "tik-guest-html")])]));
        f.answers.insert("/file/print".into(), Ok(vec![row(&[("name", PAGE), ("contents", "<p>x</p>")])]));
        let p = ok(json!({"kind": "remove-hotspot-page"}), &mut f);
        assert_eq!(p.cli(), [r#"/ip/hotspot/profile/set [find name="tik-guest"] html-directory=hotspot"#, r#"/file/remove [find name="tik-guest-html"]"#]);
        assert_eq!(p.undo_cli().last().unwrap(), r#"/ip/hotspot/profile/set [find name="tik-guest"] html-directory=tik-guest-html"#);
        assert_eq!(p.undo.len(), 3);
    }
}
