#!/usr/bin/env python3
"""End-to-end test of Safe apply intents against the disposable CHR (src/scripts/chr.sh + chr-prep.sh).

Starts the tikontrol binary with a fresh data dir, runs one-step setup as the CHR admin, switches to
read & write and drives every case through the same HTTP API the UI uses. Each case is checked with
RouterOS `/export terse`: the change must alter the config, and Revert (or the matching remove intent
after Keep) must give back exactly the config from before.

    python3 src/scripts/chr-e2e.py [--bin target/release/tikontrol] [--only kind,kind] [--sites CHR_DIR:OFFSET]

CHR_DIR / CHR_OFFSET select the CHR like chr.sh does (api-ssl on 8729+offset; tikontrol listens on 9443+offset).
--sites adds a second CHR as an extra router ("site") and checks that live data, credentials and Safe apply stay
per router; it also checks API keys, retention, viewers and the config drift inspector. --only none skips the cases.

Never point this at a real router: it writes.
"""
import argparse, functools, http.cookiejar, json, os, re, shutil, ssl, subprocess, sys, tempfile, time, urllib.error, urllib.request

DIR = os.path.expanduser(os.environ.get("CHR_DIR", "~/.cache/tikontrol-chr"))
OFFSET = int(os.environ.get("CHR_OFFSET", "0"))
API = 8729 + OFFSET
HERE = os.path.dirname(os.path.abspath(__file__))
PORT = 9443 + OFFSET
BASE = f"https://127.0.0.1:{PORT}"
# throwaway test keys (public halves only)
SSH_ED = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBv30pa9ZGPrTRuTgYYp3OGtlBnqE/BtqvZvn9Z5daiJ e2e@example.com"
SSH_RSA2048 = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQC6rvtXcTXWl3ak9p5K3PfC0dHQ6QDmvDwcCbID6PSz3URJoMKxVZxiNgznkTTz1QR1P8FLUFaEQ2yMqxuAOqxxH999M/skpuwsbS2NQmAJMWDQptCoM2myWRLR41+O3OiSe1cz8R9MWR2FiI6h4ALdDrL1frrd0jbEUVw0OZVuVp2q/UVwOSz4bD7bZQqGcW3gK+Ql4F6UBueg25Pkar49p8QEr/hczAfnjbtUHmxXuCOgfLmtnbdhJ/lR0ugOUGWRq8dWKEi71P+OO5xjJLNvrm0gbRN9nAJynj24bLtkZ4PlGpmiKfRXcZ7VdmbXa0MaOQt2H+mODUwR8/22gHAx"

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
    # Wi-Fi: CHR has no Wi-Fi package, so every one must be refused cleanly (real tests need an AP)
    ({"kind": "wifi-client-vlan", "mac": M1, "vlan_id": 30}, None, "error"),
    ({"kind": "remove-wifi-client-vlan", "mac": M1}, None, "error"),
    ({"kind": "wifi-private-psk", "mac": M1, "passphrase": "own secret 123"}, None, "error"),
    ({"kind": "remove-wifi-private-psk", "mac": M1}, None, "error"),
    ({"kind": "wifi-roaming", "interfaces": ["wifi1"], "ft": True, "steering": True}, None, "error"),
    ({"kind": "wifi-mlo", "network": "home", "enabled": True}, None, "error"),
    ({"kind": "wifi-passpoint", "interface": "wifi1", "domain": "example.com", "realm_method": "eap-tls", "network_type": "private", "internet": True}, None, "error"),
    ({"kind": "remove-wifi-passpoint", "interface": "wifi1"}, None, "error"),
    ({"kind": "wifi-network", "interfaces": ["wlan1"], "hidden": True}, None, "error"),
    ({"kind": "wifi-network", "package": "wifi", "interfaces": ["wifi1"], "hidden": True}, None, "error"),
    ({"kind": "wifi-radio", "interface": "wlan1", "frequency": "auto"}, None, "error"),
    ({"kind": "wifi-access-list", "mac": M1, "allow": False}, None, "error"),
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
    # SSH keys go on a test user, never admin: a key turns off admin's SSH password login, which this script uses
    ({"kind": "ssh-key", "user": "tik-ssh-test", "label": "laptop", "key": SSH_ED}, {"kind": "remove-ssh-key", "user": "tik-ssh-test", "label": "laptop"}, "ok"),
    ({"kind": "ssh-key", "user": "tik-ssh-test", "label": "weak", "key": SSH_RSA2048}, None, "error"),
    ({"kind": "remove-ssh-key", "user": "tik-ssh-test", "label": "manual"}, None, "error"),  # a key tikontrol didn't add
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
    # a disabled user for the SSH key cases, with one key tikontrol must not touch
    f':if ([:len [/user find name=tik-ssh-test]] = 0) do={{ /user add name=tik-ssh-test group=read disabled=yes password=not-a-real-secret-{os.getpid()} }}',
    ':if ([:len [/user/ssh-keys find comment="test: manual"]] = 0) do={ /user/ssh-keys add user=tik-ssh-test key="ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBG+EfQGrvCIUUsHp6+RfCqflp6YkDA4/1rdFiJDISSBw0XD9M9U1AWHT9wAIFl0Y0Ep+xfClwaiwPWSeyPhHOt8=" comment="test: manual" }',
]


def ros(cmd, d=None):
    return subprocess.run([os.path.join(d or DIR, "ssh-chr.sh"), cmd], capture_output=True, text=True, timeout=120).stdout


VOLATILE = re.compile(r"^#|^/system scheduler add .*name=tik-revert|^/certificate|^/file|^/system keymat-provider")


def export(d=None):
    out = ros("/export terse", d)
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
        self.site = ""  # x-tik-site: which router the requests are for
        self.bearer = None

    def call(self, m, p, body=None):
        h = {"content-type": "application/json", "origin": BASE}
        if self.csrf:
            h["x-csrf-token"] = self.csrf
        if self.site:
            h["x-tik-site"] = self.site
        if self.bearer:
            h["authorization"] = f"Bearer {self.bearer}"
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


def check(name, ok, detail=""):
    print(f"{'PASS' if ok else 'FAIL'} {name:46} {'' if ok else detail}")
    return 0 if ok else 1


def wait(c, path, cond, secs=120):
    end = time.time() + secs
    while time.time() < end:
        s, d = c.call("GET", path)
        if s == 200 and cond(d):
            return d
        time.sleep(3)
    return None


def sites(c, d2, off2, data):
    """A second router as an extra site: own credentials, data, history and Safe apply. Plus API keys,
    retention, a viewer account and the drift inspector against real router data."""
    f = 0
    rpw, wpw = os.urandom(12).hex(), os.urandom(12).hex()
    clean = '/user/remove [find name~"^tik-e2e-"]; /user/group/remove [find name~"^tik-e2e-"]; /user/remove [find name=tik-ssh-test]'
    ros(clean, d2)
    # least privilege as the setup commands describe: a read-only reader, and a writer without `policy`
    ros(f'/user/group/add name=tik-e2e-read policy=read,api,test; /user/add name=tik-e2e-read group=tik-e2e-read password="{rpw}"; '
        f'/user/group/add name=tik-e2e-write policy=read,write,api,test; /user/add name=tik-e2e-write group=tik-e2e-write password="{wpw}"; '
        f'/user/add name=tik-ssh-test group=read disabled=yes password="{rpw}x"', d2)
    ident_a = (wait(c, "/api/v1/snapshot", lambda d: True) or {}).get("identity")
    ident_b = ros(":put [/system/identity/get name]", d2).strip()

    s, r = c.call("POST", "/api/v1/sites", {"name": "Site B"})
    f += check("sites: add a router", s == 200, r)
    sid = r["id"]
    c.site = sid
    s, p = c.call("POST", "/api/v1/router/probe", {"host": "127.0.0.1", "port": 8729 + off2})
    s, r = c.call("POST", "/api/v1/router", {"host": "127.0.0.1", "port": 8729 + off2, "pin": p.get("pin", ""), "mode": "read-write",
                                             "reader": {"user": "tik-e2e-read", "password": rpw}, "writer": {"user": "tik-e2e-write", "password": wpw}, "confirm_write": True})
    f += check("sites: connect it (reader + writer)", s == 200, r)
    snap = wait(c, "/api/v1/snapshot", lambda d: True)
    f += check("sites: live data comes from that router", bool(snap) and snap.get("identity") == ident_b != ident_a, f"{ident_a} / {ident_b} / {snap and snap.get('identity')}")
    c.site = ""
    f += check("sites: main router unchanged", c.call("GET", "/api/v1/snapshot")[1].get("identity") == ident_a)
    c.site = sid
    leaked = [x for x in (json.dumps(c.call("GET", "/api/v1/router")[1]), json.dumps(c.call("GET", "/api/v1/sites")[1]), json.dumps(c.call("GET", "/api/v1/me")[1]), open(os.path.join(data, "state.json")).read()) if rpw in x or wpw in x]
    f += check("sites: no password in responses or state.json", not leaked)

    # Safe apply is per router, and a change can't hop routers
    s, _ = c.call("POST", "/api/v1/drift/baseline", {})
    f += check("drift: save blueprint", s == 200)
    before_a, before_b = export(), export(d2)
    s, p = c.call("POST", "/api/v1/apply/preview", {"intent": {"kind": "dns-record", "name": "siteb.lan", "address": "192.168.88.62"}})
    c.site = ""
    s2, _ = c.call("POST", "/api/v1/apply/start", {"id": p["id"]})
    f += check("sites: change can't run on another router", s2 == 404, s2)
    c.site = sid
    s, r = c.call("POST", "/api/v1/apply/start", {"id": p["id"]})
    f += check("sites: change lands on its router only", s == 200 and export(d2) != before_b and export() == before_a, r)
    d = wait(c, "/api/v1/drift", lambda d: any(x["menu"] == "dnsStatic" and any(y.get("name") == "siteb.lan" for y in x["added"]) for x in d["changes"]), 90)
    f += check("drift: the new DNS record shows as drift", d is not None)
    s, r = c.call("POST", "/api/v1/apply/revert", {"id": p["id"]})
    f += check("sites: revert restores that router", s == 200 and export(d2) == before_b, r)
    d = wait(c, "/api/v1/drift", lambda d: d["changes"] == [], 90)
    f += check("drift: none after the revert", d is not None, c.call("GET", "/api/v1/drift")[1])

    # /user/ssh-keys needs the `policy` policy: this writer has none, so the router refuses
    s, p = c.call("POST", "/api/v1/apply/preview", {"intent": {"kind": "ssh-key", "user": "tik-ssh-test", "label": "laptop", "key": SSH_ED}})
    s, r = c.call("POST", "/api/v1/apply/start", {"id": p.get("id", "")})
    f += check("ssh-key: refused without the policy policy", s != 200 and "permission" in json.dumps(r) and export(d2) == before_b, r)

    # API keys: read-only, real data, per router
    c.site = ""
    s, r = c.call("POST", "/api/v1/apikeys", {"name": "e2e"})
    k = Client()
    k.bearer = r.get("key")
    s, snap = k.call("GET", "/api/v1/snapshot")
    f += check("api key: reads the router", s == 200 and snap.get("identity") == ident_a)
    k.site = sid
    s, snap = k.call("GET", "/api/v1/snapshot")
    f += check("api key: reads the second router too", s == 200 and snap.get("identity") == ident_b)
    f += check("api key: can't write", k.call("POST", "/api/v1/apply/preview", {"intent": {"kind": "dns-record", "name": "x.lan", "address": "192.168.88.63"}})[0] in (401, 403))

    # retention and history
    s, _ = c.call("PUT", "/api/v1/retention", {"days": 7})
    f += check("retention: set and read back", s == 200 and c.call("GET", "/api/v1/retention")[1].get("days") == 7)
    f += check("retention: history served", c.call("GET", "/api/v1/history?range=day")[0] == 200)

    # a viewer sees router data (incl. router users) and can't change anything
    c.call("POST", "/api/v1/admins", {"name": "e2e-viewer", "password": "a long viewer password", "role": "viewer"})
    v = Client()
    v.call("POST", "/api/v1/login", {"name": "e2e-viewer", "password": "a long viewer password"})
    s, snap = v.call("GET", "/api/v1/snapshot")
    f += check("viewer: sees router users", s == 200 and any(u.get("name") == "admin" for u in snap.get("users", [])))
    s, p = v.call("POST", "/api/v1/apply/preview", {"intent": {"kind": "dns-record", "name": "x.lan", "address": "192.168.88.63"}})
    f += check("viewer: can't apply", s != 200 or v.call("POST", "/api/v1/apply/start", {"id": p["id"]})[0] == 403)
    f += check("viewer: can't add routers", v.call("POST", "/api/v1/sites", {"name": "x"})[0] == 403)

    # removing the site forgets its credentials
    s, _ = c.call("DELETE", f"/api/v1/sites?id={sid}")
    c.site = sid
    f += check("sites: removed router is gone", s == 200 and c.call("GET", "/api/v1/me")[0] == 404 and sid not in open(os.path.join(data, "state.json")).read())
    c.site = ""
    ros(clean, d2)
    return f


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", default=os.path.join(HERE, "..", "server", "target", "release", "tikontrol"))
    ap.add_argument("--only", default="")
    ap.add_argument("--sites", default="", help="second CHR as DIR:OFFSET, e.g. ~/.cache/tikontrol-chr-b:600")
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
        s, p = c.call("POST", "/api/v1/setup/probe", {"host": "127.0.0.1", "port": API})
        assert s == 200, p
        s, r = c.call("POST", "/api/v1/setup/router", {"host": "127.0.0.1", "port": API, "pin": p["pin"], "user": "admin", "password": pw})
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
        if a.sites:
            d2, off2 = a.sites.rsplit(":", 1)
            fails += sites(c, os.path.expanduser(d2), int(off2), data)
    finally:
        srv.terminate()
        shutil.rmtree(data, ignore_errors=True)
    print(f"\n{fails} failure(s)")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
