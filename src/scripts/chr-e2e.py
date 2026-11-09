#!/usr/bin/env python3
"""End-to-end test of Safe apply intents against the disposable CHR (src/scripts/chr.sh + chr-prep.sh).

Starts the tikontrol binary with a fresh data dir, runs one-step setup as the CHR admin, switches to
read & write and drives every case through the same HTTP API the UI uses. Each case is checked with
RouterOS `/export terse`: the change must alter the config, and Revert (or the matching remove intent
after Keep) must give back exactly the config from before.

    python3 src/scripts/chr-e2e.py [--bin target/release/tikontrol] [--only kind,kind] [--cases file.json]

Never point this at a real router: it writes.
"""
import argparse, functools, http.cookiejar, json, os, re, shutil, ssl, subprocess, sys, tempfile, time, urllib.error, urllib.request

DIR = os.path.expanduser(os.environ.get("CHR_DIR", "~/.cache/tikontrol-chr"))
HERE = os.path.dirname(os.path.abspath(__file__))
PORT = 9443
BASE = f"https://127.0.0.1:{PORT}"

M1, M2 = "02:00:00:AA:BB:01", "02:00:00:AA:BB:02"
WGKEY = "Y2hyLXRlc3QtcHVibGljLWtleS1ub3QtcmVhbC0xMjM="
WGKEY2 = "dGVzdC1wZWVyLWtleS1ub3QtcmVhbC0wMDAwMDAwMDA="
WGKEY3 = "c2V0dXAtcGVlci1rZXktbm90LXJlYWwtMDAwMDAwMDA="  # a peer that SETUP creates

# (intent, inverse-after-keep or None for plain revert, expectation)
# expectation: "ok" (changes config, revert restores), "error" (preview or apply must refuse),
# "noop" (allowed to leave the config unchanged, e.g. setting a value it already has)
CASES = [
    # client actions (apply.rs)
    ({"kind": "block-client", "mac": M1}, {"kind": "unblock-client", "mac": M1}, "ok"),
    ({"kind": "pause-client", "mac": M1}, {"kind": "resume-client", "mac": M1}, "ok"),
    ({"kind": "limit-client", "mac": M1, "ip": "192.168.88.50", "down": "10M", "up": "2M"}, {"kind": "unlimit-client", "mac": M1}, "ok"),
    ({"kind": "dns-record", "name": "nas.lan", "address": "192.168.88.60"}, {"kind": "remove-dns-record", "name": "nas.lan"}, "ok"),
    ({"kind": "port-forward", "protocol": "tcp", "port": 8081, "to_ip": "192.168.88.60", "to_port": 80, "name": "web"}, {"kind": "remove-port-forward", "protocol": "tcp", "port": 8081}, "ok"),
    ({"kind": "static-route", "dst": "198.51.100.0/24", "gateway": "10.0.2.2"}, {"kind": "remove-static-route", "dst": "198.51.100.0/24", "gateway": "10.0.2.2"}, "ok"),
    ({"kind": "set-upnp", "enabled": True}, None, "ok"),
    ({"kind": "disable-services", "names": ["telnet", "ftp"]}, None, "ok"),
    ({"kind": "fixed-ip", "mac": M1}, None, "error"),
    ({"kind": "forget-client", "mac": M1}, None, "error"),
    ({"kind": "set-service", "name": "api-ssl", "enabled": False}, None, "error"),
    ({"kind": "set-service", "name": "ssh", "enabled": False}, None, "ok"),
    ({"kind": "batch", "title": "Two things", "intents": [{"kind": "dns-record", "name": "tv.lan", "address": "192.168.88.61"}, {"kind": "block-client", "mac": M2}]}, None, "ok"),
    ({"kind": "kick-wifi", "mac": M1}, None, "error"),
    # networks
    ({"kind": "create-network", "name": "iot", "vlan": 30, "subnet": "10.30.0.0/24", "isolated": True}, {"kind": "remove-network", "name": "iot"}, "ok"),
    ({"kind": "create-network", "name": "guest", "vlan": 40, "subnet": "10.40.0.0/24", "guest": True, "dns": ["1.1.1.3"], "domain": "guest.lan"}, {"kind": "remove-network", "name": "guest"}, "ok"),
    ({"kind": "set-dhcp-lease", "server": "defconf", "lease_time": "1h"}, None, "ok"),
    ({"kind": "set-dhcp-range", "pool": "default-dhcp", "range": "192.168.88.100-192.168.88.200"}, None, "ok"),
    ({"kind": "set-dhcp-network", "address": "192.168.88.0/24", "dns": ["192.168.88.1", "9.9.9.9"], "domain": "home.lan"}, None, "ok"),
    ({"kind": "dhcp-option", "name": "ntp", "code": 42, "value_type": "ip", "value": "192.168.88.1"}, {"kind": "remove-dhcp-option", "name": "ntp"}, "ok"),
    ({"kind": "set-mdns-repeat", "interfaces": ["bridge", "ether4"]}, None, "ok"),
    ({"kind": "set-igmp-snooping", "bridge": "bridge", "enabled": True}, None, "ok"),
    ({"kind": "set-port-enabled", "interface": "ether4", "enabled": False}, None, "ok"),
    ({"kind": "set-port-speed", "interface": "ether4", "speed": "100M-baseT-full"}, None, "ok"),
    ({"kind": "set-bridge-port", "interface": "ether3", "pvid": 30, "frame_types": "admit-only-untagged-and-priority-tagged"}, None, "ok"),
    ({"kind": "port-profile", "interface": "ether3", "pvid": 1, "tagged": [30, 40]}, None, "ok"),
    ({"kind": "set-vlan-filtering", "bridge": "bridge", "enabled": True}, None, "ok"),
    ({"kind": "ipv6-lan", "interface": "bridge", "pool": "tik-v6"}, None, "ok"),
    ({"kind": "set-poe", "interface": "ether2", "mode": "off"}, None, "error"),
    # internet
    ({"kind": "wan-dns", "servers": ["9.9.9.9", "149.112.112.112"], "doh": "https://dns.quad9.net/dns-query"}, None, "ok"),
    ({"kind": "ddns", "enabled": True}, None, "ok"),
    ({"kind": "wan-vlan", "interface": "ether1", "vlan_id": 35}, {"kind": "remove-wan-vlan", "vlan_id": 35}, "ok"),
    ({"kind": "wan-failover", "interface": "ether4", "gateway": "203.0.113.1"}, {"kind": "remove-wan-failover", "interface": "ether4"}, "ok"),
    ({"kind": "wan-balance", "interface": "ether4", "gateway": "203.0.113.1"}, None, "ok"),
    ({"kind": "smart-queue", "interface": "ether1", "down": "100M", "up": "20M"}, {"kind": "remove-smart-queue"}, "ok"),
    # never ether1: it is the management link (QEMU NAT), and a WAN change on it cuts tikontrol off
    ({"kind": "wan-type", "interface": "ether5", "type": "pppoe", "user": "test@isp", "password": "not-a-real-secret"}, None, "ok"),
    ({"kind": "wan-type", "interface": "ether5", "type": "static", "address": "203.0.113.10/24", "gateway": "203.0.113.1"}, None, "ok"),
    ({"kind": "ipv6-wan", "interface": "ether5", "lan": "bridge", "prefix_len": 60}, {"kind": "remove-ipv6-wan"}, "ok"),
    # routing
    ({"kind": "policy-route", "src": "192.168.88.70/32", "table": "main"}, None, "error"),
    ({"kind": "policy-route", "src": "192.168.88.70/32", "table": "tik-test"}, None, "ok"),
    # security
    ({"kind": "firewall-rule", "name": "no-smb", "action": "drop", "protocol": "tcp", "dst_port": "445"}, {"kind": "remove-firewall-rule", "name": "no-smb"}, "ok"),
    ({"kind": "input-rule", "name": "allow-wg", "action": "accept", "protocol": "udp", "dst_port": "13231"}, {"kind": "remove-input-rule", "name": "allow-wg"}, "ok"),
    ({"kind": "address-group", "name": "printers", "addresses": ["192.168.88.30", "192.168.88.31"]}, {"kind": "remove-address-group", "name": "printers"}, "ok"),
    ({"kind": "zone-policy", "from": "LAN", "to": "WAN", "policy": "block"}, None, "ok"),
    ({"kind": "block-domain", "domain": "ads.example", "tls": True}, {"kind": "unblock-domain", "domain": "ads.example"}, "ok"),
    ({"kind": "add-adlist", "url": "https://example.com/list.txt"}, {"kind": "remove-adlist", "url": "https://example.com/list.txt"}, "ok"),
    ({"kind": "region-block", "country": "XX", "url": "https://example.com/xx.rsc"}, {"kind": "remove-region-block", "country": "XX"}, "ok"),
    ({"kind": "threat-feed", "name": "feed", "url": "https://example.com/feed.rsc"}, {"kind": "remove-threat-feed", "name": "feed"}, "ok"),
    ({"kind": "isolate-device", "mac": M2}, {"kind": "unisolate-device", "mac": M2}, "ok"),
    # vpn
    ({"kind": "wireguard-server", "name": "wg-home", "listen_port": 13231, "address": "10.9.0.1/24"}, None, "ok"),
    ({"kind": "vpn-server", "server": "l2tp", "enabled": True}, None, "ok"),
    ({"kind": "vpn-server", "server": "ovpn", "enabled": True, "name": "ovpn-test"}, None, "ok"),
    ({"kind": "wireguard-client", "name": "wg-vpn", "address": "10.66.0.2/32", "public_key": WGKEY, "endpoint": "vpn.example.com", "endpoint_port": 51820, "sources": ["192.168.88.80"]}, None, "ok"),
    ({"kind": "site-to-site", "name": "wg-s2s", "listen_port": 13232, "address": "10.77.0.1/30", "public_key": WGKEY2, "endpoint": "office.example.com", "endpoint_port": 13232, "remote_subnets": ["10.200.0.0/24"]}, None, "ok"),
    ({"kind": "back-to-home", "enabled": True}, None, "ok"),
    ({"kind": "wireguard-peer-enabled", "public_key": WGKEY3, "enabled": False}, {"kind": "wireguard-peer-enabled", "public_key": WGKEY3, "enabled": True}, "ok"),
    # system
    ({"kind": "site-settings", "identity": "test-router", "time_zone": "Europe/Bratislava", "ntp_enabled": True, "ntp_servers": ["pool.ntp.org"]}, None, "ok"),
    ({"kind": "set-snmp", "enabled": True, "contact": "admin", "location": "rack"}, None, "ok"),
    ({"kind": "remote-syslog", "address": "192.168.88.5", "topics": ["info", "error"]}, {"kind": "remove-remote-syslog"}, "ok"),
    ({"kind": "set-traffic-flow", "enabled": True, "address": "192.168.88.5"}, None, "ok"),
    ({"kind": "set-update-channel", "channel": "long-term"}, None, "ok"),
    ({"kind": "backup-schedule", "enabled": True}, {"kind": "backup-schedule", "enabled": False}, "ok"),
    ({"kind": "set-leds", "all_leds_off": "immediate"}, None, "error"),
    ({"kind": "locate", "interface": "ether2"}, None, "action"),
    # profiles
    ({"kind": "client-group", "group": "kids", "members": [{"mac": M1, "ip": "192.168.88.50"}]}, None, "ok"),
    ({"kind": "group-limit", "group": "kids", "down": "20M", "up": "5M"}, {"kind": "remove-group-limit", "group": "kids"}, "ok"),
    ({"kind": "hotspot-vouchers", "codes": ["TEST1234", "TEST5678"], "uptime": "1d"}, {"kind": "remove-hotspot-vouchers", "batch": "TEST1234"}, "ok"),
    ({"kind": "qos-rule", "name": "voip", "protocol": "udp", "ports": "5060-5061", "dscp": 46, "priority": 1}, {"kind": "remove-qos-rule", "name": "voip"}, "ok"),
    ({"kind": "radius-server", "address": "192.168.88.5", "secret": "not-a-real-secret", "services": ["hotspot"]}, None, "ok"),
    # hotspot
    ({"kind": "guest-hotspot", "interface": "ether4", "address": "10.50.0.1/24", "down": "5M", "up": "1M"}, {"kind": "remove-guest-hotspot"}, "ok"),
    # ports
    ({"kind": "create-bond", "name": "bond1", "slaves": ["ether4", "ether5"]}, {"kind": "remove-bond", "name": "bond1"}, "ok"),
    ({"kind": "set-bridge-stp", "bridge": "bridge", "protocol_mode": "mstp"}, None, "error"),
    ({"kind": "set-bridge-stp", "bridge": "bridge", "protocol_mode": "stp"}, None, "ok"),
    ({"kind": "set-port-stp", "interface": "ether3", "edge": "yes", "bpdu_guard": True}, None, "ok"),
    ({"kind": "add-dot1x-port", "interface": "ether3"}, {"kind": "remove-dot1x-port", "interface": "ether3"}, "ok"),
    ({"kind": "set-discovery", "interface_list": "LAN", "protocols": ["lldp", "mndp"]}, None, "ok"),
    ({"kind": "set-port-mirror", "source": "ether2", "target": "ether3"}, None, "error"),
    ({"kind": "set-storm-rate", "interface": "ether2", "percent": 10}, None, "error"),
    # traffic
    ({"kind": "traffic-rule", "name": "bedtime", "target": "mac", "value": M1, "action": "block", "days": ["mon", "tue"], "from": "22:00", "to": "06:00"}, {"kind": "remove-traffic-rule", "name": "bedtime"}, "ok"),
    # a limit is a simple queue on the client's IP: refused for a MAC the router has no address for
    ({"kind": "traffic-rule", "name": "slow-video", "target": "mac", "value": M1, "action": "limit", "down": "5M", "up": "1M"}, None, "error"),
    ({"kind": "traffic-rule", "name": "slow-video", "target": "network", "value": "192.168.88.0/24", "action": "limit", "down": "5M", "up": "1M"}, {"kind": "remove-traffic-rule", "name": "slow-video"}, "ok"),
]


print = functools.partial(print, flush=True)  # results show up as they happen

# one-time prerequisites some cases need (idempotent): a delegated IPv6 pool as an ISP would hand out,
# a RADIUS server for 802.1X, and an OpenVPN server entry with a certificate
SETUP = [
    ':if ([:len [/ipv6/pool find name=tik-v6]] = 0) do={ /ipv6/pool add name=tik-v6 prefix=2001:db8:100::/48 prefix-length=64 comment="test: stands in for a delegated prefix" }',
    ':if ([:len [/radius find comment="test: dot1x"]] = 0) do={ /radius add address=192.168.88.6 secret=not-a-real-secret service=dot1x comment="test: dot1x" }',
    ':if ([:len [/interface/wireguard find name=wg-setup]] = 0) do={ /interface/wireguard add name=wg-setup listen-port=13299; /interface/wireguard/peers add interface=wg-setup public-key="c2V0dXAtcGVlci1rZXktbm90LXJlYWwtMDAwMDAwMDA=" allowed-address=10.98.0.2/32 comment="test: peer" }',
    ':if ([:len [/routing/table find name=tik-test]] = 0) do={ /routing/table add name=tik-test fib }',
    ':if ([:len [/interface/ovpn-server/server find name=ovpn-test]] = 0) do={ /interface/ovpn-server/server add name=ovpn-test certificate=api disabled=yes }',
]


def ros(cmd):
    return subprocess.run([os.path.join(DIR, "ssh-chr.sh"), cmd], capture_output=True, text=True, timeout=120).stdout


VOLATILE = re.compile(r"^#|^/system scheduler add .*name=tik-revert|^/certificate|^/file|^/system keymat-provider")


def export():
    out = ros("/export terse")
    lines = [l.rstrip() for l in out.replace("\r", "").split("\n") if l.strip()]
    return [l for l in lines if not VOLATILE.search(l)]


def diff(a, b):
    sa, sb = set(a), set(b)
    return [f"- {l}" for l in a if l not in sb] + [f"+ {l}" for l in b if l not in sa]


class Client:
    def __init__(self):
        self.jar = http.cookiejar.CookieJar()
        self.op = urllib.request.build_opener(urllib.request.HTTPSHandler(context=ssl._create_unverified_context()), urllib.request.HTTPCookieProcessor(self.jar))
        self.csrf = None

    def call(self, m, p, body=None):
        h = {"content-type": "application/json", "origin": BASE}
        if self.csrf:
            h["x-csrf-token"] = self.csrf
        r = urllib.request.Request(BASE + p, data=None if body is None else json.dumps(body).encode(), method=m, headers=h)
        try:
            with self.op.open(r, timeout=180) as x:
                d = json.loads(x.read() or b"null")
                s = x.status
        except urllib.error.HTTPError as e:
            t = e.read()
            try:
                d = json.loads(t)
            except ValueError:
                d = {"error": t.decode(errors="replace")[:300]}
            s = e.code
        if isinstance(d, dict) and d.get("csrf"):
            self.csrf = d["csrf"]
        return s, d

    def change(self, intent, finish):
        """preview + start + keep/revert. Returns (status, message)."""
        s, p = self.call("POST", "/api/v1/apply/preview", {"intent": intent})
        if s != 200:
            return "preview", p.get("error")
        s, r = self.call("POST", "/api/v1/apply/start", {"id": p["id"]})
        if s != 200:
            return "apply", r.get("error")
        if not p.get("revertible", True):
            return "applied", None
        if finish:
            s, r = self.call("POST", f"/api/v1/apply/{finish}", {"id": p["id"]})
            if s != 200 or not r.get("ok", True):
                return finish, r
        return "ok", p


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", default=os.path.join(HERE, "..", "server", "target", "release", "tikontrol"))
    ap.add_argument("--only", default="")
    a = ap.parse_args()
    only = set(filter(None, a.only.split(",")))
    data = tempfile.mkdtemp(prefix="tik-e2e-")
    env = dict(os.environ, TIK_DATA=data, TIK_LISTEN=f"127.0.0.1:{PORT}", TIK_ROUTER="127.0.0.1", TIK_POLL="30")
    log = open(os.path.join(DIR, "e2e-server.log"), "w")
    srv = subprocess.Popen([a.bin], env=env, stdout=log, stderr=subprocess.STDOUT)
    fails = 0
    try:
        time.sleep(1.5)
        if srv.poll() is not None:
            sys.exit(f"tikontrol exited at start, see {DIR}/e2e-server.log")
        for cmd in SETUP:
            ros(cmd)
        c = Client()
        pw = open(os.path.join(DIR, "admin.pw")).read().strip()
        s, p = c.call("POST", "/api/v1/setup/probe", {"host": "127.0.0.1", "port": 8729})
        assert s == 200, p
        s, r = c.call("POST", "/api/v1/setup/router", {"host": "127.0.0.1", "port": 8729, "pin": p["pin"], "user": "admin", "password": pw})
        assert s == 200, r
        s, r = c.call("PUT", "/api/v1/router/mode", {"mode": "read-write", "confirm_write": True})
        assert s == 200, r
        c.call("GET", "/api/v1/me")
        for intent, inverse, want in CASES:
            if only and intent["kind"] not in only:
                continue
            name = intent["kind"] + ("" if not inverse else " + " + inverse["kind"])
            before = export()
            if want == "error":
                st, msg = c.change(intent, "revert")
                ok = st in ("preview", "apply") and export() == before
                print(f"{'PASS' if ok else 'FAIL'} {name:46} refused at {st}: {msg}" if ok else f"FAIL {name:46} expected a refusal, got {st} {msg}")
                fails += not ok
                continue
            if want == "action":  # one-shot (blink, reboot-free actions): applied directly, config untouched
                st, msg = c.change(intent, None)
                ok = st == "applied" and export() == before
                print(f"{'PASS' if ok else 'FAIL'} {name:46} {st} {'' if ok else msg}")
                fails += not ok
                continue
            # 1) apply + revert must restore exactly
            st, msg = c.change(intent, None)
            if st != "ok":
                print(f"FAIL {name:46} {st}: {msg}")
                fails += 1
                continue
            during = export()
            changed = during != before
            pid = msg["id"]
            s, r = c.call("POST", "/api/v1/apply/revert", {"id": pid})
            after = export()
            problems = []
            if not changed and want != "noop":
                problems.append("applied but the config did not change")
            if s != 200 or not r.get("ok", True):
                problems.append(f"revert said {s} {r}")
            if after != before:
                problems.append("revert did not restore the config:\n      " + "\n      ".join(diff(before, after)[:12]))
            # 2) apply + keep, then the remove intent must restore exactly
            if inverse and not problems:
                st, msg = c.change(intent, "keep")
                st2, msg2 = c.change(inverse, "keep") if st == "ok" else (st, msg)
                final = export()
                if st2 != "ok":
                    problems.append(f"keep+{inverse['kind']}: {st2} {msg2}")
                elif final != before:
                    problems.append(f"{inverse['kind']} after keep left:\n      " + "\n      ".join(diff(before, final)[:12]))
            ok = not problems
            fails += not ok
            print(f"{'PASS' if ok else 'FAIL'} {name:46} {'' if ok else '; '.join(problems)}")
    finally:
        srv.terminate()
        shutil.rmtree(data, ignore_errors=True)
    print(f"\n{fails} failure(s)")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
