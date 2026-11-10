#!/usr/bin/env python3
"""End-to-end test of Safe apply intents against the disposable CHR (src/scripts/chr.sh + chr-prep.sh).

Starts the tikontrol binary with a fresh data dir, runs one-step setup as the CHR admin, switches to
read & write and drives every case through the same HTTP API the UI uses. Each case is checked with
RouterOS `/export terse`: the change must alter the config, and Revert (or the matching remove intent
after Keep) must give back exactly the config from before.

For a case with an inverse, the inverse's own revert must also give back the kept state (firewall rules in
the same place in their chain). SCENARIOS are multi-step flows (backups and download, the update window
firing, DHCPv6, input-rule order, ad-lists, the branded hotspot page, secrets out of the revert job, MCP
proposals); each cleans up after itself. A summary table (case, PASS/FAIL/SKIP, reason, FEATURES.md row)
ends the run. --list prints every case and scenario and the server intent kinds nothing exercises, without a CHR.

    python3 src/scripts/chr-e2e.py [--bin target/release/tikontrol] [--only kind,scenario,...] [--sites CHR_DIR:OFFSET]
    python3 src/scripts/chr-e2e.py --list

CHR_DIR / CHR_OFFSET select the CHR like chr.sh does (api-ssl on 8729+offset; tikontrol listens on 9443+offset).
--sites adds a second CHR as an extra router ("site") and checks that live data, credentials and Safe apply stay
per router; it also checks API keys, retention, viewers and the config drift inspector. --only none skips the cases.

Never point this at a real router: it writes.
"""
import argparse, functools, http.cookiejar, json, os, re, shutil, ssl, subprocess, sys, tempfile, time, urllib.error, urllib.parse, urllib.request

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
WGKEY4 = "ZTJlLXdpcmVndWFyZC1wZWVyLW5vdC1yZWFsLTAwMDA="  # a peer tikontrol adds

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
    ({"kind": "policy-route", "src": "192.168.88.70/32", "table": "tik-test"}, {"kind": "remove-policy-route", "comment": "tik:pbr:tik-test:192.168.88.70/32"}, "ok"),
    ({"kind": "remove-policy-route", "comment": "test: manual"}, None, "error"),
    # security
    ({"kind": "firewall-rule", "name": "no-smb", "action": "drop", "protocol": "tcp", "dst_port": "445"}, {"kind": "remove-firewall-rule", "name": "no-smb"}, "ok"),
    ({"kind": "input-rule", "name": "allow-wg", "action": "accept", "protocol": "udp", "dst_port": "13231"}, {"kind": "remove-input-rule", "name": "allow-wg"}, "ok"),
    ({"kind": "address-group", "name": "printers", "addresses": ["192.168.88.30", "192.168.88.31"]}, {"kind": "remove-address-group", "name": "printers"}, "ok"),
    ({"kind": "zone-policy", "from": "LAN", "to": "WAN", "policy": "block"}, {"kind": "zone-policy", "from": "LAN", "to": "WAN", "policy": "default"}, "ok"),
    ({"kind": "block-domain", "domain": "ads.example", "tls": True}, {"kind": "unblock-domain", "domain": "ads.example"}, "ok"),
    ({"kind": "add-adlist", "url": "https://example.com/list.txt"}, {"kind": "remove-adlist", "url": "https://example.com/list.txt"}, "ok"),
    ({"kind": "region-block", "country": "XX", "url": "https://example.com/xx.rsc"}, {"kind": "remove-region-block", "country": "XX"}, "ok"),
    ({"kind": "threat-feed", "name": "feed", "url": "https://example.com/feed.rsc"}, {"kind": "remove-threat-feed", "name": "feed"}, "ok"),
    ({"kind": "isolate-device", "mac": M2}, {"kind": "unisolate-device", "mac": M2}, "ok"),
    # vpn
    ({"kind": "wireguard-server", "name": "wg-home", "listen_port": 13231, "address": "10.9.0.1/24"}, None, "ok"),
    ({"kind": "vpn-server", "server": "l2tp", "enabled": True, "ipsec_secret": "not-a-real-secret-1"}, None, "ok"),
    ({"kind": "vpn-user", "name": "tik-vpn-test", "password": "not-a-real-secret-2", "service": "l2tp"}, {"kind": "remove-vpn-user", "name": "tik-vpn-test"}, "ok"),
    ({"kind": "vpn-user", "name": "tik-vpn-test2", "password": "not-a-real-secret-3", "service": "ovpn", "profile": "no-such-profile"}, None, "error"),
    ({"kind": "remove-vpn-user", "name": "tik-not-ours"}, None, "error"),
    ({"kind": "vpn-server", "server": "ovpn", "enabled": True, "name": "ovpn-test"}, None, "ok"),
    # /certificate and /file are not in the compared export: check by hand that the revert removed tik-ovpn-* and the .crt
    ({"kind": "ovpn-certificates", "common_name": "vpn.example.net"}, None, "noop"),
    ({"kind": "batch", "title": "OpenVPN certificates", "intents": [{"kind": "ovpn-certificates", "common_name": "vpn.example.net"}, {"kind": "ovpn-server-certificate", "name": "ovpn-test"}]}, None, "ok"),
    ({"kind": "export-ovpn-ca"}, None, "error"),  # reverted above: there is no tik-ovpn-ca
    ({"kind": "wireguard-client", "name": "wg-vpn", "address": "10.66.0.2/32", "public_key": WGKEY, "endpoint": "vpn.example.com", "endpoint_port": 51820, "sources": ["192.168.88.80"]}, None, "ok"),
    ({"kind": "site-to-site", "name": "wg-s2s", "listen_port": 13232, "address": "10.77.0.1/30", "public_key": WGKEY2, "endpoint": "office.example.com", "endpoint_port": 13232, "remote_subnets": ["10.200.0.0/24"]}, None, "ok"),
    ({"kind": "back-to-home", "enabled": True}, None, "error"),  # a CHR has no Back To Home: refused (7.12+, real hardware only)
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
    # remove-radius is one-shot (the inverse would need the secret): applied directly after Keep
    ({"kind": "radius-server", "address": "192.168.88.5", "secret": "not-a-real-secret", "services": ["hotspot"]}, {"kind": "remove-radius", "address": "192.168.88.5"}, "ok"),
    ({"kind": "remove-radius", "address": "192.168.88.6"}, None, "error"),  # SETUP's hand-made dot1x server
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

    # ---- added since 259c454 (multi-step flows for these are in SCENARIOS below) ----
    # system: update window (check only; an install window could fire during the run) and backups
    ({"kind": "auto-update", "enabled": True, "day": "daily", "hour": 3}, {"kind": "auto-update", "enabled": False}, "ok"),
    ({"kind": "auto-update", "enabled": True, "day": "sun", "hour": 4}, {"kind": "auto-update", "enabled": False}, "ok"),
    ({"kind": "auto-update", "enabled": False}, None, "error"),  # already off
    ({"kind": "auto-update", "enabled": True, "day": "someday", "hour": 3}, None, "error"),
    ({"kind": "auto-update", "enabled": True, "day": "daily", "hour": 24}, None, "error"),
    ({"kind": "check-updates"}, None, "action"),
    ({"kind": "export-config", "name": "bad name"}, None, "error"),
    ({"kind": "save-backup", "name": "../x"}, None, "error"),
    ({"kind": "restore-backup", "name": "tik-not-there.backup"}, None, "error"),
    ({"kind": "restore-backup", "name": "tik-e2e.rsc"}, None, "error"),
    ({"kind": "remove-backup-file", "name": "manual.backup"}, None, "error"),  # not tikontrol's own file
    ({"kind": "remove-backup-file", "name": "tik-not-there.rsc"}, None, "error"),
    # networks
    ({"kind": "batch", "title": "DHCP for the LAN", "intents": [{"kind": "set-dhcp-range", "pool": "default-dhcp", "range": "192.168.88.20-192.168.88.120"}, {"kind": "set-dhcp-lease", "server": "defconf", "lease_time": "2h"}]}, None, "ok"),
    ({"kind": "dhcp-option", "name": "tftp", "code": 66, "value_type": "text", "value": "boot.example", "network": "192.168.88.0/24"}, {"kind": "remove-dhcp-option", "name": "tftp"}, "ok"),
    ({"kind": "dhcp-option", "name": "hexopt", "code": 224, "value_type": "hex", "value": "c0a8"}, {"kind": "remove-dhcp-option", "name": "hexopt"}, "ok"),
    ({"kind": "remove-dhcp-option", "name": "not-ours"}, None, "error"),
    ({"kind": "set-mdns-repeat", "interfaces": []}, None, "noop"),
    ({"kind": "set-igmp-snooping", "bridge": "no-such-bridge", "enabled": True}, None, "error"),
    ({"kind": "remove-network", "name": "lan"}, None, "error"),  # defconf's LAN is not tikontrol's
    # no IPv6 from a pool on the bridge yet: refused in plain words (the success path is a scenario)
    ({"kind": "dhcpv6-server", "interface": "bridge"}, None, "error"),
    ({"kind": "remove-dhcpv6-server", "interface": "bridge"}, None, "error"),
    # PoE: CHR has no PoE-out ports, so these must be refused (real tests need a PoE board)
    ({"kind": "poe-power-cycle", "interface": "ether2"}, None, "error"),
    ({"kind": "poe-power-cycle", "interface": "ether2", "seconds": 0}, None, "error"),
    ({"kind": "set-poe", "interface": "ether2", "mode": "forced-on"}, None, "error"),
    # Wi-Fi intents not covered above: refused cleanly without a Wi-Fi package
    ({"kind": "wifi-radio-enabled", "interface": "wlan1", "enabled": False}, None, "error"),
    ({"kind": "wifi-radio-enabled", "package": "wifi", "interface": "wifi1", "enabled": False}, None, "error"),
    ({"kind": "wifi-guest", "master": "wlan1", "ssid": "guest", "passphrase": "not-a-real-secret-4"}, None, "error"),
    ({"kind": "remove-wifi-guest", "master": "wlan1", "ssid": "guest"}, None, "error"),
    ({"kind": "remove-wifi-access-list", "mac": M1}, None, "error"),
    ({"kind": "wifi-signal-range", "interface": "wlan2", "min": -75}, None, "error"),
    ({"kind": "remove-wifi-signal-range", "interface": "wlan2"}, None, "error"),
    ({"kind": "wifi-schedule", "interface": "wlan1", "on": "07:00", "off": "23:00"}, None, "error"),
    ({"kind": "wifi-schedule", "package": "wifi", "interface": "wifi1", "on": "07:00", "off": "23:00"}, None, "error"),
    ({"kind": "remove-wifi-schedule", "interface": "wlan1"}, None, "error"),
    # ports
    ({"kind": "remove-port-mirror"}, None, "error"),
    # security
    ({"kind": "firewall-rule", "name": "no-telnet", "action": "reject", "protocol": "tcp", "dst": "192.168.88.0/24", "dst_port": "23"}, {"kind": "remove-firewall-rule", "name": "no-telnet"}, "ok"),
    ({"kind": "remove-firewall-rule", "name": "not-ours"}, None, "error"),
    ({"kind": "remove-input-rule", "name": "not-ours"}, None, "error"),
    ({"kind": "input-rule", "name": "drop-all", "action": "drop"}, None, "error"),  # would lock everyone out
    ({"kind": "zone-policy", "from": "LAN", "to": "WAN", "policy": "allow"}, {"kind": "zone-policy", "from": "LAN", "to": "WAN", "policy": "default"}, "ok"),
    ({"kind": "zone-policy", "from": "LAN", "to": "NOPE", "policy": "block"}, None, "error"),
    ({"kind": "batch", "title": "Block an app", "intents": [{"kind": "block-domain", "domain": "app.example", "tls": True}, {"kind": "block-domain", "domain": "cdn-app.example", "tls": True}]},
     {"kind": "batch", "title": "Unblock an app", "intents": [{"kind": "unblock-domain", "domain": "app.example"}, {"kind": "unblock-domain", "domain": "cdn-app.example"}]}, "ok"),
    ({"kind": "unblock-domain", "domain": "never-blocked.example"}, None, "error"),
    ({"kind": "add-adlist", "url": "http://example.com/list.txt"}, None, "error"),
    ({"kind": "set-adlist", "url": "https://example.com/not-added.txt", "disabled": True}, None, "error"),
    ({"kind": "remove-adlist", "url": "https://example.com/not-added.txt"}, None, "error"),
    ({"kind": "remove-region-block", "country": "YY"}, None, "error"),
    ({"kind": "remove-threat-feed", "name": "not-ours"}, None, "error"),
    ({"kind": "unisolate-device", "mac": M1}, None, "error"),  # not isolated
    # routing removes: hand-made rows (SETUP, comment "test: manual") must be refused
    ({"kind": "remove-dns-record", "name": "manual.lan"}, None, "error"),
    ({"kind": "remove-port-forward", "protocol": "tcp", "port": 8098}, None, "error"),
    ({"kind": "remove-static-route", "dst": "198.51.100.128/25", "gateway": "10.0.2.2"}, None, "error"),
    # unblock / resume for a client that is not blocked: nothing to remove. Its revert must not block the client.
    ({"kind": "unblock-client", "mac": "02:00:00:AA:BB:09"}, None, "error"),
    ({"kind": "resume-client", "mac": "02:00:00:AA:BB:09"}, None, "error"),
    # vpn
    ({"kind": "vpn-user", "name": "tik-ovpn-test", "password": "not-a-real-secret-5", "service": "ovpn"}, {"kind": "remove-vpn-user", "name": "tik-ovpn-test"}, "ok"),
    ({"kind": "vpn-user", "name": "bad name", "password": "not-a-real-secret-6", "service": "l2tp"}, None, "error"),
    ({"kind": "vpn-server", "server": "l2tp", "enabled": True, "ipsec_secret": "short"}, None, "error"),
    ({"kind": "wireguard-peer", "interface": "wg-setup", "name": "phone", "public_key": WGKEY4, "allowed_address": "10.98.0.3/32"}, {"kind": "remove-wireguard-peer", "public_key": WGKEY4}, "ok"),
    ({"kind": "remove-wireguard-peer", "public_key": WGKEY3}, None, "ok"),  # SETUP's peer: revert puts it back
    ({"kind": "remove-wireguard-peer", "public_key": "bm90LWEtcGVlci1vbi10aGlzLXJvdXRlci0wMDAwMDA="}, None, "error"),
    # profiles / hotspot
    ({"kind": "remove-qos-rule", "name": "not-ours"}, None, "error"),
    ({"kind": "remove-hotspot-vouchers", "batch": "NOPE1234"}, None, "error"),
    ({"kind": "hotspot-page", "title": "Guests", "message": "Welcome", "color": "#0a7d4f"}, None, "error"),  # no guest hotspot yet
    ({"kind": "remove-hotspot-page"}, None, "error"),
]

# Cases that can't run on a CHR, listed so the summary shows them instead of hiding them.
SKIPPED = [
    ("install-update", "downloads RouterOS and restarts: needs a spare router"),
    ("auto-update install=true firing", "installs and restarts in the window: needs a spare router"),
    ("restore-backup (success)", "replaces the config and restarts the CHR mid-run"),
    ("reboot", "restarts the CHR mid-run"),
    ("routerboot-upgrade", "CHR has no RouterBOOT"),
    ("wifi-* success paths", "CHR has no Wi-Fi package: needs an AP (HARDWARE_TESTS.md)"),
    ("set-poe / poe-power-cycle success", "CHR has no PoE-out: needs a PoE board"),
]

# FEATURES.md row each kind (or scenario) gives evidence for, printed in the summary.
ROWS = {
    "auto-update": "Firmware/app updates (update window)", "check-updates": "Firmware/app updates (update window)", "install-update": "Firmware/app updates (update window)",
    "export-config": "Backups (scheduled, download, restore)", "save-backup": "Backups (scheduled, download, restore)", "restore-backup": "Backups (scheduled, download, restore)",
    "remove-backup-file": "Backups (scheduled, download, restore)", "backup-schedule": "Backups (scheduled, download, restore)",
    "dhcpv6-server": "IPv6 (prefix delegation, SLAAC, DHCPv6 server per network)", "remove-dhcpv6-server": "IPv6 (prefix delegation, SLAAC, DHCPv6 server per network)",
    "ipv6-lan": "IPv6 (prefix delegation, SLAAC, DHCPv6 server per network)",
    "set-mdns-repeat": "mDNS reflector across networks", "set-igmp-snooping": "IGMP snooping / multicast", "dhcp-option": "DHCP options (custom)", "remove-dhcp-option": "DHCP options (custom)",
    "set-dhcp-range": "DHCP range, lease time, DNS, gateway", "set-dhcp-lease": "DHCP range, lease time, DNS, gateway", "set-dhcp-network": "DHCP range, lease time, DNS, gateway",
    "create-network": "Create network / P14 Settings → Networks", "remove-network": "P14 Settings → Networks",
    "poe-power-cycle": "PoE status / power cycle", "set-poe": "PoE status / power cycle",
    "zone-policy": "Zone-based firewall (zone matrix)", "firewall-rule": "Firewall policies (custom rules)", "remove-firewall-rule": "Firewall policies (custom rules)",
    "input-rule": "Firewall policies (custom rules) / P17", "remove-input-rule": "Firewall policies (custom rules) / P17",
    "block-domain": "Simple app blocking", "unblock-domain": "Simple app blocking", "add-adlist": "Content filtering (ad-lists)", "set-adlist": "Content filtering (ad-lists)", "remove-adlist": "Content filtering (ad-lists)",
    "region-block": "Region (country) blocking", "remove-region-block": "Region (country) blocking", "threat-feed": "Threat intelligence feeds", "remove-threat-feed": "Threat intelligence feeds",
    "isolate-device": "Device isolation (per client)", "unisolate-device": "Device isolation (per client)", "address-group": "IP groups / port groups",
    "port-forward": "P18 Routing: port forwarding", "remove-port-forward": "P18 Routing: port forwarding", "static-route": "P18 Routing: static routes", "remove-static-route": "P18 Routing: static routes",
    "dns-record": "P18 Routing: local DNS records", "remove-dns-record": "P18 Routing: local DNS records", "policy-route": "Policy-based routes", "remove-policy-route": "Policy-based routes",
    "block-client": "Clients: block / unblock", "unblock-client": "Clients: block / unblock", "pause-client": "Clients: pause / resume", "resume-client": "Clients: pause / resume",
    "vpn-user": "L2TP/IPsec + OpenVPN server (VPN users)", "remove-vpn-user": "L2TP/IPsec + OpenVPN server (VPN users)", "vpn-server": "L2TP/IPsec + OpenVPN server",
    "wireguard-peer": "WireGuard server + peers", "wireguard-peer-enabled": "WireGuard server + peers", "remove-wireguard-peer": "WireGuard server + peers", "wireguard-server": "WireGuard server + peers",
    "radius-server": "RADIUS profiles / P19", "remove-radius": "RADIUS profiles / P19", "qos-rule": "QoS / DSCP rules / P19", "remove-qos-rule": "QoS / DSCP rules / P19",
    "group-limit": "Speed limit profiles / P19", "remove-group-limit": "Speed limit profiles / P19",
    "guest-hotspot": "P12 Guest portal", "remove-guest-hotspot": "P12 Guest portal", "hotspot-page": "Captive portal (branded page)", "remove-hotspot-page": "Captive portal (branded page)",
    "hotspot-vouchers": "Vouchers", "remove-hotspot-vouchers": "Vouchers",
    "remove-port-mirror": "Port mirroring", "set-port-mirror": "Port mirroring",
}
for k in ("wifi-radio-enabled", "wifi-guest", "remove-wifi-guest", "wifi-access-list", "remove-wifi-access-list", "wifi-signal-range", "remove-wifi-signal-range", "wifi-schedule", "remove-wifi-schedule"):
    ROWS[k] = "Settings → WiFi (CHR refusal only)"


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
    # hand-made rows the Remove intents must refuse (only tik: tagged objects are tikontrol's)
    ':if ([:len [/ip/dns/static find comment="test: manual"]] = 0) do={ /ip/dns/static add name=manual.lan address=192.168.88.65 comment="test: manual" }',
    ':if ([:len [/ip/firewall/nat find comment="test: manual"]] = 0) do={ /ip/firewall/nat add chain=dstnat action=dst-nat protocol=tcp dst-port=8098 in-interface-list=WAN to-addresses=192.168.88.65 to-ports=80 comment="test: manual" }',
    ':if ([:len [/ip/route find comment="test: manual"]] = 0) do={ /ip/route add dst-address=198.51.100.128/25 gateway=10.0.2.2 comment="test: manual" }',
    ':if ([:len [/routing/rule find comment="test: manual"]] = 0) do={ /routing/rule add src-address=192.168.88.71/32 action=lookup table=tik-test comment="test: manual" }',
]


def ros(cmd, d=None):
    # a busy CHR sometimes refuses or drops the ssh login: try again rather than read an empty answer
    for wait in (2, 4, 8, 0):
        r = subprocess.run([os.path.join(d or DIR, "ssh-chr.sh"), cmd], capture_output=True, text=True, timeout=120)
        if r.returncode != 255 or not wait:
            return r.stdout
        time.sleep(wait)


VOLATILE = re.compile(r"^#|^/system scheduler add .*name=tik-revert|^/certificate|^/file|^/system keymat-provider")


def export(d=None):
    """/export terse, read until two reads agree: under load the CHR now and then returns one with whole
    sections missing, which looked like a failed revert."""
    def once():
        out = ros("/export terse", d)
        lines = [l.rstrip() for l in out.replace("\r", "").split("\n") if l.strip()]
        return [l for l in lines if not VOLATILE.search(l)]
    last = once()
    for _ in range(4):
        now = once()
        if now == last:
            return now
        last = now
    return last


def diff(a, b):
    sa, sb = set(a), set(b)
    d = [f"- {l}" for l in a if l not in sb] + [f"+ {l}" for l in b if l not in sa]
    if d:
        return d
    i = next((i for i, (x, y) in enumerate(zip(a, b)) if x != y), 0)
    return ["(same lines, different order) from line %d:" % i] + [f"  was {x}" for x in a[i:i + 4]] + [f"  now {y}" for y in b[i:i + 4]]


def same(a, b, ordered=True):
    """Exports match. Order matters for firewall rules (a rule re-added at the end of its chain may never match)."""
    return a == b if ordered else sorted(a) == sorted(b)


# Every result, for the summary table at the end: (name, PASS/FAIL/SKIP, reason, FEATURES row)
RESULTS = []


def record(name, status, reason="", row=""):
    """Prints one result as it happens and keeps it for the summary."""
    if status is True or status is False:
        status = "PASS" if status else "FAIL"
    reason = str(reason or "")
    print(f"{status} {name:46} {reason}".rstrip())
    RESULTS.append((name, status, reason.split("\n")[0][:110], row))
    return 1 if status == "FAIL" else 0


def row_of(intent):
    k = intent["kind"]
    if k == "batch":
        k = intent["intents"][0]["kind"]
    return ROWS.get(k, "")


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

    def raw(self, m, p):
        """For non-JSON answers (file downloads): (status, body bytes, headers)."""
        h = {"origin": BASE}
        if self.csrf:
            h["x-csrf-token"] = self.csrf
        r = urllib.request.Request(BASE + p, method=m, headers=h)
        try:
            with self.op.open(r, timeout=180) as x:
                return x.status, x.read(), dict(x.headers)
        except urllib.error.HTTPError as e:
            return e.code, e.read(), dict(e.headers)

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


def check(name, ok, detail="", row=""):
    return record(name, bool(ok), "" if ok else detail, row)


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
        f'/user/group/add name=tik-e2e-write policy=read,write,api,test,ftp; /user/add name=tik-e2e-write group=tik-e2e-write password="{wpw}"; '
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




# ---------- table cases ----------

# Removes whose revert must put firewall rules back in place, not at the end of the chain
IN_PLACE = {"remove-firewall-rule", "remove-input-rule", "zone-policy", "unblock-domain"}
LABEL_KEYS = ("name", "interface", "server", "service", "mac", "country", "domain", "url", "day", "package", "policy", "address", "user", "label", "batch", "comment")


def kinds_of(intent):
    return [i["kind"] for i in (intent.get("intents") or [intent])]


def label(intent, inverse, want):
    """A readable, stable case name: kind(distinguishing value)[ + inverse][ refused]."""
    if intent["kind"] == "batch":
        base = "batch(" + "+".join(dict.fromkeys(kinds_of(intent))) + ")"
    else:
        arg = next((str(intent[k]) for k in LABEL_KEYS if isinstance(intent.get(k), (str, int)) and str(intent.get(k))), "")
        if not arg and "public_key" in intent:
            arg = intent["public_key"][:6] + "..."
        base = intent["kind"] + (f"({arg[:24]})" if arg else "")
    if inverse:
        base += " + " + (inverse["kind"] if inverse["kind"] != "batch" else "batch(" + "+".join(dict.fromkeys(kinds_of(inverse))) + ")")
    if want == "error":
        base += " refused"
    n = sum(1 for r in RESULTS if r[0] == base or r[0].startswith(base + " #"))
    return base if n == 0 else f"{base} #{n + 1}"


def run_case(c, intent, inverse, want):
    """One table case. Returns 1 on failure."""
    name, row = label(intent, inverse, want), row_of(intent)
    before = export()
    if want == "error":
        st, msg = c.change(intent, "revert")
        ok = st in ("preview", "apply") and export() == before
        return record(name, ok, f"refused at {st}: {msg}" if ok else f"expected a refusal, got {st} {msg}", row)
    if want == "action":  # one-shot (blink, check for updates, export): applied directly, config untouched
        st, msg = c.change(intent, None)
        ok = st == "applied" and export() == before
        return record(name, ok, "" if ok else f"{st} {msg}", row)
    # 1) apply + revert must restore exactly
    st, msg = c.change(intent, None)
    if st != "ok":
        return record(name, False, f"{st}: {msg}", row)
    during = export()
    s, r = c.call("POST", "/api/v1/apply/revert", {"id": msg["id"]})
    after = export()
    problems, notes = [], []
    if during == before and want != "noop":
        problems.append("applied but the config did not change")
    if s != 200 or not r.get("ok", True):
        problems.append(f"revert said {s} {r}")
    if after != before:
        problems.append("revert did not restore the config:\n      " + "\n      ".join(diff(before, after)[:12]))
    # 2) apply + keep; the inverse's own revert must give back the kept state (in place for firewall rules);
    #    then the inverse + keep must restore exactly. One-shot inverses (remove-radius) apply directly.
    if inverse and not problems:
        st, msg = c.change(intent, "keep")
        if st != "ok":
            problems.append(f"keep: {st} {msg}")
        else:
            kept = export()
            st2, msg2 = c.change(inverse, None)
            if st2 == "ok":
                c.call("POST", "/api/v1/apply/revert", {"id": msg2["id"]})
                back = export()
                strict = bool(IN_PLACE & set(kinds_of(inverse)))
                if not same(kept, back, strict):
                    problems.append(f"revert of {inverse['kind']} did not put things back{' in place' if strict else ''}:\n      " + "\n      ".join(diff(kept, back)[:12]))
                elif back != kept:
                    notes.append(f"revert of {inverse['kind']} changed the order")
                st2, msg2 = c.change(inverse, "keep")
            if st2 not in ("ok", "applied"):
                problems.append(f"keep+{inverse['kind']}: {st2} {msg2}")
            else:
                final = export()
                if final != before:
                    problems.append(f"{inverse['kind']} after keep left:\n      " + "\n      ".join(diff(before, final)[:12]))
    return record(name, not problems, "; ".join(problems or notes), row)


# ---------- multi-step scenarios ----------

class Flow:
    """A scenario: several changes in a row, checked as they go, cleaned up at the end whatever happened."""

    def __init__(self, c, name, row):
        self.c, self.name, self.row, self.problems = c, name, row, []
        self.before = export()

    def need(self, ok, what):
        if not ok:
            self.problems.append(what)
        return ok

    def keep(self, intent):
        """Apply and keep (or a one-shot action). True when it went through."""
        st, m = self.c.change(intent, "keep")
        return self.need(st in ("ok", "applied"), f"{intent['kind']}: {st} {m}")

    def refuse(self, intent, why):
        snap = export()
        st, m = self.c.change(intent, "revert")
        return self.need(st in ("preview", "apply") and export() == snap, f"{why}: expected a refusal, got {st} {m}")

    def apply_revert(self, intent, while_applied=None, ordered=True, in_export=True):
        """Apply, run the checks while it is live, revert: the config must be exactly as before the apply.
        in_export=False for changes that only touch files (not in /export): while_applied checks those."""
        snap = export()
        st, m = self.c.change(intent, None)
        if not self.need(st == "ok", f"{intent['kind']}: {st} {m}"):
            return
        if in_export:
            self.need(export() != snap, f"{intent['kind']} changed nothing")
        if while_applied:
            while_applied()
        s, r = self.c.call("POST", "/api/v1/apply/revert", {"id": m["id"]})
        after = export()
        self.need(s == 200 and same(snap, after, ordered), f"revert of {intent['kind']} left: " + "; ".join(diff(snap, after)[:6]))

    def ros_is(self, cmd, want, what):
        got = ros(cmd).strip()
        return self.need(got == want, f"{what}: got {got!r}, want {want!r}")

    def done(self, cleanup=()):
        for i in cleanup:  # best effort; refused when there is nothing left to clean
            self.c.change(i, "keep")
        after = export()
        self.need(after == self.before, "not back to the starting config: " + "; ".join(diff(self.before, after)[:6]))
        return record(self.name, not self.problems, "; ".join(self.problems), self.row)


def ros_lines(cmd):
    return [l.strip() for l in ros(cmd).replace("\r", "").split("\n") if l.strip()]


def file_count(prefix):
    return ros(f':put [:len [/file/find name~"^{prefix}"]]').strip()


def sc_backups(c, data):
    """export-config, download it through /tools/export (as the writer: /file/read needs ftp), .backup and other names refused,
    save-backup, remove-backup-file both. Files are not in /export, so the files themselves are checked."""
    f = Flow(c, "scenario backups: export, download, delete", ROWS["export-config"])
    pw = open(os.path.join(DIR, "admin.pw")).read().strip()
    ros('/file/remove [find name~"^tik-e2e"]')
    if f.keep({"kind": "export-config", "name": "e2e"}):
        for _ in range(10):
            if file_count("tik-e2e.rsc") != "0":
                break
            time.sleep(2)
        f.need(file_count("tik-e2e.rsc") == "1", "tik-e2e.rsc was not written")
        time.sleep(2.2)  # one download per 2 s per session
        s, body, h = c.raw("GET", "/api/v1/tools/export?name=tik-e2e.rsc")
        hint = " (the writer needs the ftp policy for /file/read)" if s == 404 else ""
        f.need(s == 200 and b"/ip" in body, f"download: {s} {body[:120]!r}{hint}")
        cd = {k.lower(): v for k, v in h.items()}.get("content-disposition", "")
        f.need("tik-e2e.rsc" in cd, f"download has no attachment file name: {cd!r}")
        f.need(pw.encode() not in body and b"private-key=" not in body and b"password=" not in body, "the export carries a secret")
    for bad in ("tik-e2e.backup", "../tik-e2e.rsc", "defconf.rsc", "tik-e2e.rsc;x", "tik-.rsc"):
        s, _, _ = c.raw("GET", "/api/v1/tools/export?name=" + urllib.parse.quote(bad))
        f.need(s == 400, f"download of {bad!r} not refused ({s})")
    s, _, _ = c.raw("POST", "/api/v1/tools/export?name=tik-e2e.rsc")
    f.need(s == 405, f"POST download not refused ({s})")
    if f.keep({"kind": "save-backup", "name": "e2e"}):
        time.sleep(3)
        f.need(file_count("tik-e2e.backup") == "1", "tik-e2e.backup was not written")
        time.sleep(2.2)
        s, _, _ = c.raw("GET", "/api/v1/tools/export?name=tik-e2e.backup")
        f.need(s == 400, f"a .backup download was not refused ({s})")
    for n in ("tik-e2e.backup", "tik-e2e.rsc"):
        if f.keep({"kind": "remove-backup-file", "name": n}):
            time.sleep(1)
            f.need(file_count(n) == "0", f"{n} still there")
    f.refuse({"kind": "remove-backup-file", "name": "tik-e2e.rsc"}, "removing a file that is gone")
    ros('/file/remove [find name~"^tik-e2e"]')
    return f.done()


def sc_update_window(c, data):
    """auto-update (check only): scheduler policy and script, then the window really fires (moved 30 s ahead
    on the CHR) and logs its check; turning it off restores the config."""
    f = Flow(c, "scenario update window fires (check only)", ROWS["auto-update"])
    if f.keep({"kind": "auto-update", "enabled": True, "day": "daily", "hour": 3}):
        job = "[/system/scheduler/find name=tik-auto-update]"
        f.ros_is(f":put [/system/scheduler/get {job} policy]", "read;write;test", "check-only window policy")  # :put joins the list with ;
        ev = ros(f":put [/system/scheduler/get {job} on-event]")
        f.need("check-for-updates once" in ev and "install" not in ev, f"check-only window script: {ev.strip()[:120]!r}")
        # either line proves the window ran: a CHR has no internet, and a job made by a writer without the
        # policy permission can't check, so it logs the warning instead of stopping silently
        msg = "tikontrol: checked for RouterOS updates|tikontrol: the RouterOS update check failed"
        n0 = ros(f':put [:len [/log/find message~"{msg}"]]').strip()
        ros(f"/system/scheduler/set {job} start-date=[/system/clock/get date] start-time=([:totime [/system/clock/get time]] + 30s)")
        fired = False
        for _ in range(30):
            time.sleep(5)
            if ros(f':put [:len [/log/find message~"{msg}"]]').strip() not in ("", n0):
                fired = True
                break
        f.need(fired, "the window logged nothing within 150 s (near midnight the start time wraps)")
    return f.done([{"kind": "auto-update", "enabled": False}])


def sc_dhcpv6_bridge(c, data):
    """ipv6-lan (keep), dhcpv6-server apply/revert/keep, a second one refused, remove-dhcpv6-server
    revert/keep, ipv6-lan off: back to the start."""
    f = Flow(c, "scenario dhcpv6-server on the bridge", ROWS["dhcpv6-server"])
    srv, rm = {"kind": "dhcpv6-server", "interface": "bridge"}, {"kind": "remove-dhcpv6-server", "interface": "bridge"}
    if f.keep({"kind": "ipv6-lan", "interface": "bridge", "pool": "tik-v6"}):
        f.apply_revert(srv, lambda: f.ros_is(':put [/ipv6/dhcp-server/get [find comment="tik:dhcpv6:bridge"] address-pool]', "tik-v6", "DHCPv6 address-pool"))
        if f.keep(srv):
            f.refuse(srv, "a second DHCPv6 server on bridge")
            f.apply_revert(rm, lambda: f.ros_is(':put [:len [/ipv6/dhcp-server/find comment="tik:dhcpv6:bridge"]]', "0", "DHCPv6 server gone while removed"))
            f.keep(rm)
    return f.done([rm, {"kind": "ipv6-lan", "interface": "bridge", "pool": ""}])


def sc_dhcpv6_network(c, data):
    """create-network with IPv6, dhcpv6-server on its VLAN, remove-network also removes the DHCPv6 server
    (revert brings both back, keep leaves nothing)."""
    f = Flow(c, "scenario remove-network takes its DHCPv6 server", ROWS["remove-network"])
    if f.keep({"kind": "create-network", "name": "v6net", "vlan": 60, "subnet": "10.60.0.0/24", "ipv6_pool": "tik-v6"}):
        if f.keep({"kind": "dhcpv6-server", "interface": "vlan60"}):
            f.apply_revert({"kind": "remove-network", "name": "v6net"}, lambda: f.ros_is(':put [:len [/ipv6/dhcp-server/find comment="tik:dhcpv6:vlan60"]]', "0", "DHCPv6 server gone with the network"))
            f.keep({"kind": "remove-network", "name": "v6net"})
    return f.done([{"kind": "remove-dhcpv6-server", "interface": "vlan60"}, {"kind": "remove-network", "name": "v6net"}])


def input_order():
    return ros_lines(":foreach x in=[/ip/firewall/filter/find chain=input] do={ :put [/ip/firewall/filter/get $x comment] }")


def sc_input_rule_order(c, data):
    """input-rule goes before defconf's "drop all not coming from LAN"; remove-input-rule's revert puts it
    back at the same place (an accept re-added after that drop would never match)."""
    f = Flow(c, "scenario input-rule revert keeps its place", ROWS["input-rule"])
    tag, drop = "tik:input:allow-wg", "defconf: drop all not coming from LAN"

    def before_drop(what):
        o = input_order()
        f.need(tag in o and drop in o and o.index(tag) < o.index(drop), f"{what}: input chain order {o}")

    if f.keep({"kind": "input-rule", "name": "allow-wg", "action": "accept", "protocol": "udp", "dst_port": "13231"}):
        before_drop("after add")
        f.apply_revert({"kind": "remove-input-rule", "name": "allow-wg"}, lambda: f.need(tag not in input_order(), "rule still there while removed"))
        before_drop("after the remove was reverted")
        f.keep({"kind": "remove-input-rule", "name": "allow-wg"})
    return f.done([{"kind": "remove-input-rule", "name": "allow-wg"}])


def sc_adlist(c, data):
    """add-adlist, set-adlist off/on (apply/revert each), remove-adlist revert/keep."""
    url = "https://example.com/e2e-hosts.txt"
    f = Flow(c, "scenario adlist add / disable / remove", ROWS["set-adlist"])
    dis = f':put [/ip/dns/adlist/get [find url="{url}"] disabled]'
    if f.keep({"kind": "add-adlist", "url": url}):
        f.refuse({"kind": "add-adlist", "url": url}, "adding the same list twice")
        f.apply_revert({"kind": "set-adlist", "url": url, "disabled": True}, lambda: f.ros_is(dis, "true", "disabled while applied"))
        if f.keep({"kind": "set-adlist", "url": url, "disabled": True}):
            f.apply_revert({"kind": "set-adlist", "url": url, "disabled": False}, lambda: f.ros_is(dis, "false", "enabled while applied"))
        f.apply_revert({"kind": "remove-adlist", "url": url})
        f.keep({"kind": "remove-adlist", "url": url})
    return f.done([{"kind": "remove-adlist", "url": url}])


PAGE_FILE = "tik-guest-html/login.html"


def page_contents():
    return ros(f':put [/file/get [find name="{PAGE_FILE}"] contents]').replace("\r", "").strip()


def sc_hotspot_page(c, data):
    """guest-hotspot, hotspot-page: escaped login.html in its own folder (read back from /file), revert removes
    it, an edit restores the old page on revert, remove-hotspot-page revert/keep, then the hotspot goes."""
    f = Flow(c, "scenario branded hotspot page", ROWS["hotspot-page"])
    page = {"kind": "hotspot-page", "title": "Cafe <b>$(mac)</b>", "message": 'Welcome & "enjoy"', "color": "#0a7d4f"}
    page2 = {"kind": "hotspot-page", "title": "Guests", "message": "Second version", "color": "#203040"}

    def escaped():
        t = page_contents()
        f.need("Cafe &lt;b&gt;&#36;(mac)&lt;/b&gt;" in t and "<b>" not in t and "&quot;enjoy&quot;" in t and "&amp;" in t, f"login.html not escaped as expected: {t[:160]!r}")
        f.need(any("tik-guest-html" in l for l in export()), "the hotspot profile does not use tik-guest-html")

    if f.keep({"kind": "guest-hotspot", "interface": "ether4", "address": "10.50.0.1/24", "down": "5M", "up": "1M"}):
        f.apply_revert(page, escaped)
        f.need(file_count("tik-guest-html") == "0", "revert left tik-guest-html files")
        if f.keep(page):
            first = page_contents()
            f.apply_revert(page2, lambda: f.need("Second version" in page_contents(), "edited page not written"), in_export=False)
            f.need(page_contents() == first, "revert of an edit did not restore the old page")
            f.apply_revert({"kind": "remove-hotspot-page"}, lambda: f.need(file_count("tik-guest-html") == "0", "files still there while removed"))
            f.need(page_contents() == first, "revert of remove-hotspot-page did not bring the page back")
            f.keep({"kind": "remove-hotspot-page"})
            f.need(file_count("tik-guest-html") == "0", "remove-hotspot-page left files")
            f.refuse({"kind": "remove-hotspot-page"}, "removing the page twice")
    return f.done([{"kind": "remove-hotspot-page"}, {"kind": "remove-guest-hotspot"}])


def sc_secrets_out_of_revert(c, data):
    """While a change with a secret is live, the revert job (scheduler), the server log and state.json must not hold it."""
    f = Flow(c, "scenario secrets stay out of revert job and log", "Safe apply (auto-revert) / security rules")
    cases = [
        ({"kind": "vpn-server", "server": "l2tp", "enabled": True, "ipsec_secret": "e2e-ipsec-secret-0001"}, "e2e-ipsec-secret-0001"),
        ({"kind": "vpn-user", "name": "tik-sec-test", "password": "e2e-vpn-password-0002", "service": "l2tp"}, "e2e-vpn-password-0002"),
        ({"kind": "radius-server", "address": "192.168.88.7", "secret": "e2e-radius-secret-0003", "services": ["hotspot"]}, "e2e-radius-secret-0003"),
        ({"kind": "wan-type", "interface": "ether5", "type": "pppoe", "user": "e2e@isp", "password": "e2e-pppoe-password-0004"}, "e2e-pppoe-password-0004"),
    ]
    for intent, secret in cases:
        def look(secret=secret, intent=intent):
            jobs = ros(':foreach x in=[/system/scheduler/find name~"^tik-revert"] do={ :put [/system/scheduler/get $x on-event] }')
            f.need(jobs.strip() != "", f"{intent['kind']}: no revert job while applied")
            f.need(secret not in jobs, f"{intent['kind']}: the secret is in the revert job")
            if intent["kind"] == "vpn-server":
                f.ros_is(":put [/interface/l2tp-server/server/get use-ipsec]", "required", "L2TP use-ipsec while applied")
        f.apply_revert(intent, look)
        seen = open(os.path.join(DIR, "e2e-server.log"), errors="replace").read() + open(os.path.join(data, "state.json"), errors="replace").read()
        f.need(secret not in seen, f"{intent['kind']}: the secret is in the server log or state.json")
    return f.done()


def sc_mcp(c, data):
    """MCP propose_change with an intent: checked read-only, nothing applied, listed for the admin, then applied
    and reverted through Safe apply like the UI does; proposals with a password or secret are refused."""
    f = Flow(c, "scenario MCP propose_change -> Safe apply", "Proposed changes (MCP) through Safe apply")
    s, m = c.call("POST", "/api/v1/mcp/enable", {})
    tok = m.get("token") if isinstance(m, dict) else None
    if not f.need(s == 200 and tok, f"enable MCP: {s} {m}"):
        return f.done()
    k = Client()
    k.bearer = tok
    n = [0]

    def rpc(method, params=None):
        n[0] += 1
        _, r = k.call("POST", "/mcp", {"jsonrpc": "2.0", "id": n[0], "method": method, "params": params or {}})
        return r.get("result") if isinstance(r, dict) else None

    def propose(intent, title="e2e"):
        time.sleep(2.2)  # one proposal per 2 s
        r = rpc("tools/call", {"name": "propose_change", "arguments": {"title": title, "intent": intent}}) or {}
        return r.get("isError"), " ".join(x.get("text", "") for x in r.get("content", []))

    try:
        f.need(rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "chr-e2e", "version": "1"}}) is not None, "MCP initialize")
        tools = [t["name"] for t in (rpc("tools/list") or {}).get("tools", [])]
        f.need("propose_change" in tools, f"read-write mode offers propose_change: {tools}")
        snap = export()
        intent = {"kind": "dns-record", "name": "mcp.lan", "address": "192.168.88.66"}
        err, text = propose(intent, "NAS name from MCP")
        f.need(err is False and "checked against the router" in text, f"proposal refused: {text}")
        f.need(export() == snap, "proposing applied something")
        s, props = c.call("GET", "/api/v1/mcp/proposals")
        props = props if s == 200 and isinstance(props, list) else []
        last = props[-1] if props else {}
        f.need(last.get("intent") == intent and any("/ip/dns/static/add" in x for x in last.get("commands", [])), f"proposal list: {last}")
        if last.get("intent"):
            f.apply_revert(last["intent"], lambda: f.need(any("mcp.lan" in l for l in export()), "the proposed record is not on the router"))
        for bad, why in [
            ({"kind": "vpn-user", "name": "mcp-user", "password": "chosen-by-ai-123", "service": "l2tp"}, "password"),
            ({"kind": "batch", "title": "t", "intents": [{"kind": "vpn-server", "server": "l2tp", "enabled": True, "ipsec_secret": "chosen-by-ai-456"}]}, "nested secret"),
            ({"kind": "radius-server", "address": "192.168.88.9", "secret": "chosen-by-ai-789", "services": ["hotspot"]}, "secret"),
        ]:
            err, text = propose(bad)
            f.need(err is True and "secrets" in text, f"proposal with a {why} not refused: {text}")
        err, text = propose({"kind": "no-such-kind"})
        f.need(err is True, f"unknown intent not refused: {text}")
        s, after = c.call("GET", "/api/v1/mcp/proposals")
        f.need(isinstance(after, list) and len(after) == len(props), "a refused proposal was stored")
        if last.get("id") is not None:
            f.need(c.call("DELETE", f"/api/v1/mcp/proposals?id={last['id']}")[0] == 200, "delete the proposal")
    finally:
        c.call("POST", "/api/v1/mcp/disable", {})
    s, _ = k.call("POST", "/mcp", {"jsonrpc": "2.0", "id": 99, "method": "tools/list"})
    f.need(s == 404, f"MCP off: old token still answers ({s})")
    return f.done()


# name for --only, function, kinds it covers (for the coverage list)
SCENARIOS = [
    ("backups", sc_backups, ["export-config", "save-backup", "remove-backup-file"]),
    ("update-window", sc_update_window, ["auto-update"]),
    ("dhcpv6-bridge", sc_dhcpv6_bridge, ["ipv6-lan", "dhcpv6-server", "remove-dhcpv6-server"]),
    ("dhcpv6-network", sc_dhcpv6_network, ["create-network", "dhcpv6-server", "remove-network"]),
    ("input-order", sc_input_rule_order, ["input-rule", "remove-input-rule"]),
    ("adlist", sc_adlist, ["add-adlist", "set-adlist", "remove-adlist"]),
    ("hotspot-page", sc_hotspot_page, ["guest-hotspot", "hotspot-page", "remove-hotspot-page", "remove-guest-hotspot"]),
    ("secrets", sc_secrets_out_of_revert, ["vpn-server", "vpn-user", "radius-server", "wan-type"]),
    ("mcp", sc_mcp, ["dns-record"]),
]


# ---------- offline: case list and coverage ----------

def server_kinds():
    """Every intent kind the server knows, read from src/server/src (KINDS lists, matches! lists, the Intent enum)."""
    src = os.path.join(HERE, "..", "server", "src")
    kinds = set()
    for d, _, files in os.walk(src):
        for fn in files:
            if not fn.endswith(".rs"):
                continue
            t = open(os.path.join(d, fn)).read()
            for m in re.finditer(r"KINDS: &\[&str\] = &\[(.*?)\];", t, re.S):
                kinds |= set(re.findall(r'"([a-z0-9-]+)"', m.group(1)))
            for m in re.finditer(r'if !matches!\(kind, ((?:"[a-z0-9-]+"\s*\|?\s*)+)\)', t):
                kinds |= set(re.findall(r'"([a-z0-9-]+)"', m.group(1)))
            if fn == "apply.rs":
                body = re.search(r"pub enum Intent \{(.*?)\n\}", t, re.S)
                for v in re.findall(r"^    ([A-Z][A-Za-z]*)", body.group(1) if body else "", re.M):
                    kinds.add(re.sub(r"(?<!^)([A-Z])", r"-\1", v).lower())
    return kinds


def covered_kinds():
    out = set()
    for intent, inverse, _ in CASES:
        for i in (intent, inverse):
            if i:
                out |= set(kinds_of(i))
    for _, _, ks in SCENARIOS:
        out |= set(ks)
    return out


def list_cases():
    """--list: what a run would do and which server kinds nothing exercises. Needs no CHR."""
    for intent, inverse, want in CASES:
        name = label(intent, inverse, want)
        RESULTS.append((name, "", "", ""))  # keeps the #n numbering as in a run
        print(f"case     {name:62} {want:6} {row_of(intent)}")
    for key, fn, _ in SCENARIOS:
        print(f"scenario {key:62} {'':6} {(fn.__doc__ or '').split(chr(10))[0].strip()}")
    for name, why in SKIPPED:
        print(f"skip     {name:62} {'':6} {why}")
    kinds = server_kinds()
    cmr = set(re.findall(r'"kind": "([a-z0-9-]+)"', open(os.path.join(HERE, "cmr-e2e.py")).read()))
    mine = covered_kinds()
    unknown = sorted(mine - kinds - {"batch"})
    missing = sorted(kinds - mine - cmr)
    names = [r[0] for r in RESULTS]
    dupes = sorted({n for n in names if names.count(n) > 1})
    print(f"\n{len(CASES)} cases, {len(SCENARIOS)} scenarios, {len(SKIPPED)} listed skips; server kinds: {len(kinds)}, "
          f"exercised here: {len(kinds & mine)}, by cmr-e2e.py: {len(kinds & cmr)}")
    print("not exercised on CHR:", ", ".join(missing) or "none")
    if unknown:
        print("cases name kinds the server doesn't know:", ", ".join(unknown))
    if dupes:
        print("duplicate case names:", ", ".join(dupes))
    return 1 if unknown or dupes else 0


def summary():
    print("\n" + "=" * 110)
    print(f"{'case':60} {'result':6} reason | FEATURES row")
    print("-" * 110)
    for name, st, reason, row in RESULTS:
        why = reason if st != "PASS" else ""
        print(f"{name[:60]:60} {st:6} {why}{' | ' if why and row else ''}{row}")
    n = {s: sum(1 for r in RESULTS if r[1] == s) for s in ("PASS", "FAIL", "SKIP")}
    print("-" * 110)
    print(f"{n['PASS']} passed, {n['FAIL']} failed, {n['SKIP']} skipped")
    return n["FAIL"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", default=os.path.join(HERE, "..", "server", "target", "release", "tikontrol"))
    ap.add_argument("--only", default="", help="intent kinds and/or scenario names (see --list); 'none' runs no cases")
    ap.add_argument("--sites", default="", help="second CHR as DIR:OFFSET, e.g. ~/.cache/tikontrol-chr-b:600")
    ap.add_argument("--list", action="store_true", help="print the cases, scenarios and uncovered kinds, then exit (no CHR needed)")
    a = ap.parse_args()
    if a.list:
        sys.exit(list_cases())
    only = set(filter(None, a.only.split(",")))
    data = tempfile.mkdtemp(prefix="tik-e2e-")
    env = dict(os.environ, TIK_DATA=data, TIK_LISTEN=f"127.0.0.1:{PORT}", TIK_ROUTER="127.0.0.1", TIK_POLL="30")
    log = open(os.path.join(DIR, "e2e-server.log"), "w")
    srv = subprocess.Popen([a.bin], env=env, stdout=log, stderr=subprocess.STDOUT)
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
            if only and not (set(kinds_of(intent)) & only):
                continue
            try:
                run_case(c, intent, inverse, want)
            except Exception as e:  # one broken case must not hide the others
                record(label(intent, inverse, want), False, f"harness error: {e!r}", row_of(intent))
        for key, fn, ks in SCENARIOS:
            if only and key not in only and not (set(ks) & only):
                continue
            try:
                fn(c, data)
            except Exception as e:
                record(f"scenario {key}", False, f"harness error: {e!r}")
        if not only:
            for name, why in SKIPPED:
                record(name, "SKIP", why)
        if a.sites:
            d2, off2 = a.sites.rsplit(":", 1)
            sites(c, os.path.expanduser(d2), int(off2), data)
    finally:
        srv.terminate()
        shutil.rmtree(data, ignore_errors=True)
    sys.exit(1 if summary() else 0)


if __name__ == "__main__":
    main()
