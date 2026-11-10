#!/usr/bin/env python3
"""Read-feature checks against a running tikontrol that is set up (read-only) with a disposable CHR.

Signs in as the CHR admin (password from $CHR_DIR/admin.pw, never printed) and checks what the read pages and
tools return: router data, the router's own refusal of writes as the reader, debug tools, console allow-list,
API keys (GET only) and MCP (read tools, privacy mode, no secrets). tikontrol changes nothing on the router; this
script adds a throwaway PPP secret and hotspot user over SSH (to check their passwords never reach the snapshot)
and removes them again. It enables MCP and creates an API key in tikontrol and removes both again.

    CHR_DIR=~/.cache/tikontrol-chr-ui python3 src/scripts/chr-read.py [--prep] [https://127.0.0.1:9643]

--prep first adds that fictional activity to the CHR over SSH (idempotent; it stays for the next run).

Expects the fictional activity this repo's CHR notes describe: static leases "Living room TV"
(02:00:00:AA:BB:20 / 192.168.88.20), a netwatch, a WireGuard peer, a backup, a failed login among the last 40 log lines.
"""
import http.cookiejar, json, os, re, ssl, subprocess, sys, time, urllib.error, urllib.request

PREP = "--prep" in sys.argv
ARGS = [a for a in sys.argv[1:] if a != "--prep"]
BASE = ARGS[0] if ARGS else "https://127.0.0.1:9643"
DIR = os.path.expanduser(os.environ.get("CHR_DIR", "~/.cache/tikontrol-chr"))
PW = open(os.path.join(DIR, "admin.pw")).read().strip()
CTX = ssl._create_unverified_context()
fails = []


def check(ok, what):
    print(("ok   " if ok else "FAIL ") + what, flush=True)
    if not ok:
        fails.append(what)


class Client:
    def __init__(self):
        self.op = urllib.request.build_opener(urllib.request.HTTPSHandler(context=CTX), urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        self.csrf = None

    def call(self, m, p, body=None, headers=None):
        h = {"content-type": "application/json", "origin": BASE}
        if self.csrf:
            h["x-csrf-token"] = self.csrf
        h.update(headers or {})
        h = {k: v for k, v in h.items() if v is not None}  # None drops a header (MCP clients send no Origin)
        r = urllib.request.Request(BASE + p, data=None if body is None else json.dumps(body).encode(), method=m, headers=h)
        try:
            with self.op.open(r, timeout=180) as x:
                s, t = x.status, x.read()
        except urllib.error.HTTPError as e:
            s, t = e.code, e.read()
        try:
            d = json.loads(t or b"null")
        except ValueError:
            d = t.decode(errors="replace")
        if isinstance(d, dict) and d.get("csrf"):
            self.csrf = d["csrf"]
        return s, d


def ros(cmd):
    return subprocess.run([os.path.join(DIR, "ssh-chr.sh"), cmd], capture_output=True, text=True, timeout=60).stdout


# the fictional activity the checks below expect (documentation addresses and a made-up key only)
FIXTURES = [
    ':if ([:len [/ip/dhcp-server/lease/find mac-address=02:00:00:AA:BB:20]] = 0) do={ /ip/dhcp-server/lease/add mac-address=02:00:00:AA:BB:20 address=192.168.88.20 comment="Living room TV" }',
    ':if ([:len [/tool/netwatch/find comment="test: gateway"]] = 0) do={ /tool/netwatch/add host=192.168.88.1 comment="test: gateway" }',
    ':if ([:len [/tool/netwatch/find comment="test: dns"]] = 0) do={ /tool/netwatch/add host=192.0.2.53 comment="test: dns" }',
    ':if ([:len [/interface/wireguard/find name=wg-test]] = 0) do={ /interface/wireguard/add name=wg-test listen-port=51820 }',
    ':if ([:len [/interface/wireguard/peers/find comment="test: phone"]] = 0) do={ /interface/wireguard/peers/add interface=wg-test public-key="dGlrY29udHJvbC10ZXN0LXBlZXIta2V5LTAwMDAwMDA=" allowed-address=10.66.0.2/32 comment="test: phone" }',
    ':if ([:len [/file/find name~"^test-backup"]] = 0) do={ /system/backup/save name=test-backup dont-encrypt=yes }',
]


def prep():
    for cmd in FIXTURES:
        ros(cmd)
    # a failed login for the Security log: SSH as a user that doesn't exist, with an empty password
    env = dict(os.environ, SSH_ASKPASS=os.path.join(DIR, "askpass-empty.sh"), SSH_ASKPASS_REQUIRE="force")
    subprocess.run(["ssh", "-q", "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null", "-o", "NumberOfPasswordPrompts=1",
                    "-o", "PreferredAuthentications=password", "-p", "2222", "tik-nobody@127.0.0.1", "quit"], capture_output=True, timeout=30, env=env)
    for _ in range(30):  # /system/backup/save takes a while on a CHR
        if ros(':put [:len [/file/find name~"^test-backup"]]').strip() not in ("", "0"):
            break
        time.sleep(2)
    time.sleep(35)  # the next poll picks the new rows up


if PREP:
    prep()

c = Client()
s, me = c.call("POST", "/api/v1/login", {"name": "admin", "password": PW})
check(s == 200, "sign in with the MikroTik admin")
s, me = c.call("GET", "/api/v1/me")
pol = (me.get("policies") or {}).get("policy", [])
check(me.get("readOnly") is True and pol and not {"write", "sensitive", "password", "policy", "reboot"} & set(pol), f"read-only badge: detected reader policies {pol}")

s, d = c.call("GET", "/api/v1/snapshot")
check(s == 200 and d.get("identity") == "chr-test", "snapshot from the CHR")
check(any(l.get("comment") == "Living room TV" for l in d.get("leases", [])), "static DHCP lease")
check(any(b.get("name", "").startswith("test-backup") for b in d.get("backups", [])), "backups list")
check(len(d.get("netwatch", [])) >= 2 and len(d.get("wgPeers", [])) >= 1, "netwatch + WireGuard peers")
check(any("login failure" in l.get("message", "") for l in d.get("log", [])), "failed login kept in the log (Security)")
check(d.get("update", {}).get("installed-version") and d.get("resource", {}).get("version"), "updates: installed version + channel")
check(isinstance(d.get("latency"), dict), "latency kept between pings")
check(not any(k in json.dumps(d) for k in ("private-key", PW)), "snapshot carries no private key or admin password")



# newer read queries: all answer on a CHR (lists, maybe empty) and never carry a password or secret field
errs = {e.get("key") for e in d.get("_errors", []) if isinstance(e, dict)}
for k in ("pppActive", "pppSecrets", "hotspotUsers", "queueTree", "bridgeFilter", "dhcpOptions", "ipv6DhcpServers", "radius"):
    check(isinstance(d.get(k), list) and k not in errs, f"snapshot has {k} ({len(d.get(k) or [])} rows)")
check(not d.get("poe") and not d.get("poeMon"), f"no PoE rows on CHR (no PoE-out ports){' (poe logged in _errors)' if 'poe' in errs else ''}")
# a PPP secret and a hotspot user with passwords, added over SSH: the rows show up, the passwords don't
ppw, hpw = os.urandom(9).hex(), os.urandom(9).hex()
ros(f'/ppp/secret/add name=tik-read-ppp password="{ppw}" service=l2tp comment="test: read"; /ip/hotspot/user/add name=tik-read-hs password="{hpw}" comment="test: read"')
try:
    seen = None
    for _ in range(25):  # the snapshot is refreshed every TIK_POLL seconds (30 by default)
        s, snap = c.call("GET", "/api/v1/snapshot")
        if s == 200 and any(r.get("name") == "tik-read-ppp" for r in snap.get("pppSecrets", [])) and any(r.get("name") == "tik-read-hs" for r in snap.get("hotspotUsers", [])):
            seen = snap
            break
        time.sleep(5)
    check(seen is not None, "pppSecrets and hotspotUsers list the new rows")
    if seen:
        blob = json.dumps(seen)
        check(ppw not in blob and hpw not in blob, "PPP secret and hotspot user passwords never reach the snapshot")
        for k in ("pppSecrets", "hotspotUsers"):
            check(not any("password" in r for r in seen.get(k, [])), f"{k} rows have no password field")
finally:
    ros('/ppp/secret/remove [find comment="test: read"]; /ip/hotspot/user/remove [find comment="test: read"]')

for p in ["/api/v1/history?range=1h", "/api/v1/health/internet", "/api/v1/events", "/api/v1/alerts", "/api/v1/clients/offline", "/api/v1/flows", "/api/v1/admins", "/api/v1/retention"]:
    s, _ = c.call("GET", p)
    check(s == 200, f"GET {p}")
s, t = c.call("GET", "/api/v1/events?format=csv")
check(s == 200 and str(t).startswith("time,category,level,text"), "events CSV export")
s, a = c.call("GET", "/api/v1/alerts")
open_ = [h for h in a.get("history", []) if not h.get("ack")]
check(s == 200 and open_, f"alarm history: {len(open_)} open (a fresh CHR has insecure services on)")
if open_:
    s, _ = c.call("POST", "/api/v1/alerts/ack", {"id": open_[0]["id"]})
    check(s == 200 and c.call("GET", "/api/v1/alerts")[1].get("unacked") == len(open_) - 1, "acknowledge an alarm")

s, r = c.call("POST", "/api/v1/tools/ping", {"host": "10.0.2.2", "count": 2})
check(s == 200, f"debug ping ({s})")
time.sleep(2.2)  # the debug tools allow one run per 2 s per session
s, r = c.call("POST", "/api/v1/tools/traceroute", {"host": "10.0.2.2"})
check(s == 200, f"debug traceroute ({s})")
time.sleep(2.2)
s, r = c.call("POST", "/api/v1/tools/dns", {"name": "example.com"})
check(s == 200, f"debug DNS lookup ({s})")
time.sleep(2.2)
s, r = c.call("POST", "/api/v1/tools/console", {"command": "/ip/route/print"})
check(s == 200 and "10.0.2.2" in json.dumps(r), "console: /ip/route/print")
time.sleep(2.2)
s, r = c.call("POST", "/api/v1/tools/console", {"command": "/system/reboot"})
check(s == 400, "console refuses /system/reboot")
s, r = c.call("POST", "/api/v1/apply/preview", {"intent": {"kind": "dns-record", "name": "x.lan", "address": "192.168.88.9"}})
if s == 200:
    s, r = c.call("POST", "/api/v1/apply/start", {"id": r["id"]})
check(s >= 400, f"read-only: applying a change is refused ({s} {r.get('error') if isinstance(r, dict) else ''})")

# API keys: read-only bearer access, never writes
s, k = c.call("POST", "/api/v1/apikeys", {"name": "chr-read-check"})
check(s == 200 and k.get("key"), "create API key")
bare = Client()
s, _ = bare.call("GET", "/api/v1/snapshot", headers={"authorization": f"Bearer {k.get('key')}"})
check(s == 200, "API key reads the snapshot")
s, _ = bare.call("POST", "/api/v1/tools/ping", {"host": "10.0.2.2"}, headers={"authorization": f"Bearer {k.get('key')}"})
check(s in (401, 403), f"API key cannot POST ({s})")
s, _ = c.call("DELETE", "/api/v1/apikeys?name=chr-read-check")
check(s == 200, "delete API key")

# MCP
s, m = c.call("POST", "/api/v1/mcp/enable", {})
tok = m.get("token") if isinstance(m, dict) else None
check(s == 200 and tok and m.get("privacy") is True, "enable MCP (privacy on by default)")
n = [0]


def rpc(method, params=None):
    n[0] += 1
    s, r = bare.call("POST", "/mcp", {"jsonrpc": "2.0", "id": n[0], "method": method, "params": params or {}}, headers={"authorization": f"Bearer {tok}", "origin": None})
    return r.get("result") if isinstance(r, dict) else None


init = rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "chr-read", "version": "1"}})
check(bool(init), "MCP initialize")
tools = [t["name"] for t in (rpc("tools/list") or {}).get("tools", [])]
check(len(tools) > 3, f"MCP tools: {', '.join(tools)}")
check(not any(t.startswith("propose") for t in tools), "read-only: MCP offers no proposed changes")


def all_tools():
    out = ""
    for t in sorted(tools, key=lambda t: t != "list_clients"):  # list_clients first: get_client takes a MAC from it
        if t.startswith("propose"):
            continue
        mac = re.search(r'\\"mac\\": ?\\"([^\\]+)\\"', out)
        r = rpc("tools/call", {"name": t, "arguments": {"mac": mac.group(1)} if t == "get_client" and mac else {}})
        check(r is not None and not r.get("isError"), f"MCP {t}")
        out += json.dumps(r)
    return out


private = all_tools()
for secret in (PW, "Living room TV", "02:00:00:AA:BB:20", "192.168.88.20", "private-key", "password\": \""):
    check(secret not in private, f"privacy mode hides {'the admin password' if secret == PW else secret!r}")
s, _ = c.call("POST", "/api/v1/mcp/privacy", {"privacy": False})
plain = all_tools()
check("Living room TV" in plain, "privacy off shows real names")
check(PW not in plain and "private-key" not in plain and tok not in plain, "MCP never returns credentials")
s, _ = bare.call("POST", "/mcp", {"jsonrpc": "2.0", "id": 1, "method": "tools/list"}, headers={"authorization": "Bearer wrong", "origin": None})
check(s == 401, "MCP refuses a wrong token")
c.call("POST", "/api/v1/mcp/disable", {})
s, _ = bare.call("POST", "/mcp", {"jsonrpc": "2.0", "id": 1, "method": "tools/list"}, headers={"authorization": f"Bearer {tok}", "origin": None})
check(s == 404, "MCP off again: old token dead")

print(f"\n{len(fails)} failed" if fails else "\nall read checks passed")
sys.exit(1 if fails else 0)
