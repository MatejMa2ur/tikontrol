#!/usr/bin/env python3
"""snapshot.json -> sample.json: a fictional network with the same shape as the real one.

Usage: scripts/anonymize.py src/ui/src/lib/snapshot.json > src/ui/src/lib/sample.json

Only the hardware model and its specs survive, because the UI is built around them. Names, addresses,
counters, signal levels, uptimes, timestamps, versions and service state are all replaced.
Check the result before committing: anything new in a snapshot is kept until a rule here covers it.
"""
import json, random, re, sys

rng = random.Random(2026)
s = open(sys.argv[1], encoding="utf-8").read()

# MACs: keep the vendor prefix for real vendors, renumber everything else. Randomised ones stay flagged as private.
for i, m in enumerate(sorted(set(re.findall(r"(?:[0-9A-F]{2}:){5}[0-9A-F]{2}", s)))):
    if m != "00:00:00:00:00:00":
        private = int(m[:2], 16) & 2
        s = s.replace(m, ("02:AA:BB:" if private else m[:9]) + "00:00:%02X" % (i + 1))

# Private IPv4: each /24 becomes a made-up 10.x network and hosts are renumbered. .0, .1, .254 and .255 stay put.
PRIVATE = r"\b(?:192\.168\.\d+|172\.(?:1[6-9]|2\d|3[01])\.\d+|10\.\d+\.\d+)\.\d+\b"
ips = sorted(set(re.findall(PRIVATE, s)))
nets = sorted({ip.rsplit(".", 1)[0] for ip in ips})
ipmap = {}
for n, net in enumerate(nets):
    hosts = [ip for ip in ips if ip.rsplit(".", 1)[0] == net]
    for h, ip in enumerate(hosts):
        last = ip.rsplit(".", 1)[1]
        ipmap[ip] = f"10.{10 + n}.0.{last if last in ('0', '1', '254', '255') else 100 + h}"
s = re.sub(PRIVATE, lambda m: ipmap[m[0]], s)
s = re.sub(r"fc00:0:0:\d+::", "fc00:0:0:99::", s)

d = json.loads(s)

# Timestamps: same order, invented dates
stamp = r"\d{4}-\d\d-\d\d \d\d:\d\d:\d\d"
times = sorted({t for t in re.findall(stamp, s) if not t.startswith("1970")})
minute = 0
newtime = {}
for t in times:
    minute += rng.randint(1, 45)
    newtime[t] = "2026-01-%02d %02d:%02d:%02d" % (10 + minute // 1440, minute // 60 % 24, minute % 60, rng.randint(0, 59))

KEEP = {"cpu-count", "cpu-frequency", "total-memory", "total-hdd-space", "port", "mtu", "pvid", "distance", "endpoint-port", "registered-clients", "loss"}
RATES = ["144Mbps-20MHz/2S", "300Mbps-40MHz/2S", "433Mbps-80MHz/1S", "866Mbps-80MHz/2S"]
NAMES = [("sonos|speaker", "Speaker"), ("arch|qemu|vm", "dev-vm"), ("zima|nas|server", "home-server"), ("pc|desktop|laptop", "desktop-pc"), ("gw|hub|bridge", "smart-hub")]


def alias(name):
    """A generic name by device type. Anything we can't classify loses its name."""
    return next((new for pat, new in NAMES if name and re.search(pat, name, re.I)), None)


def fuzz(v):
    return type(v)(round(v * rng.uniform(0.6, 1.4), 1)) if v else v


ids = iter(range(1, 100000))


def scrub(x, key=""):
    if isinstance(x, dict):
        return {k: scrub(v, k) for k, v in x.items()}
    if isinstance(x, list):
        return [scrub(v, key) for v in x]
    if isinstance(x, bool) or x is None:
        return x
    if isinstance(x, (int, float)):
        if key == "noise-floor":
            return -rng.randint(95, 106)
        if key in ("tx-ccq", "overall-tx-ccq"):
            return min(100, int(fuzz(x)))
        return x if key in KEEP else fuzz(x)
    if key == ".id":
        return "*%X" % next(ids)
    if key == "uptime" or (key == "last-modified" and x.startswith("1970")):
        return "1970-01-%02d %02d:%02d:%02d" % (rng.randint(2, 12), rng.randint(0, 23), rng.randint(0, 59), rng.randint(0, 59))
    if key == "last-seen":
        return f"{rng.randint(1, 20)}m{rng.randint(0, 59)}s"
    if key == "bytes":
        return ",".join(str(int(fuzz(int(n)))) for n in x.split(","))
    if key == "signal-strength":
        return re.sub(r"^-\d+", lambda m: str(max(-85, min(-30, int(m[0]) + rng.randint(-7, 7)))), x)
    if key in ("tx-rate", "rx-rate"):
        return rng.choice(RATES)
    return re.sub(stamp, lambda m: newtime.get(m[0], m[0]), x)


d = scrub(d)
d["identity"] = "Router"
d["cloud"] = {**d.get("cloud", {}), "public-address": None, "dns-name": None}
d["clock"] = {"time-zone-name": "Etc/UTC", "date": "2026-01-15", "time": "12:00:00"}
d["resource"].update({"version": "7.20", "build-time": "2026-01-01 12:00:00", "minimum-version": "7.0"})
d["routerboard"].update({"current-firmware": "7.19", "upgrade-firmware": "7.20"})
d["update"] = {**d.get("update", {}), "channel": "stable", "installed-version": "7.20", "latest-version": "7.20"}
for w in d.get("wifi") or []:
    w["ssid"] = "HomeNet"
for r, ch in zip(d.get("radios") or [], ["2437/20-Ce/gn(17dBm)", "5180/20-Ceee/ac(23dBm)"]):
    r["channel"] = ch
for l in d.get("leases") or []:
    name = alias(l.get("comment") or l.get("host-name"))
    l.pop("comment", None)
    l.pop("host-name", None)
    if name:
        l["host-name"] = name
for w in d.get("wireguard") or []:
    w["listen-port"] = 51820
for a in d.get("adlist") or []:
    a["url"] = "https://example.com/blocklist/hosts"
for i, svc in enumerate(d.get("services") or []):
    if svc.get("name") in ("telnet", "ftp", "www"):
        svc["disabled"] = True
    if str(svc.get("name")).startswith("wireguard"):
        svc["port"] = 51820
    elif svc.get("port", 0) >= 10000:  # randomly assigned on the router
        svc["port"] = 40000 + i * 37
for i, b in enumerate(d.get("backups") or []):
    b.update({".id": f"*{i + 1}", "name": f"flash/backup-{i + 1}.backup"})
# Log lines can quote anything, so only the two shapes the UI parses are kept
d["log"] = [
    {**l, "message": re.sub(r"signal strength -\d+", lambda m: f"signal strength -{rng.randint(45, 80)}", l["message"])}
    for l in d.get("log") or []
    if re.match(r"(?:[0-9A-F]{2}:){5}[0-9A-F]{2}@|.*assigned [\d.]+ for ", str(l.get("message")))
]
# DNS cache = browsing history and VPN peer comments = personal devices: replace, never publish
GENERIC = ["www.example.com", "www.example.org", "www.example.net", "pool.ntp.org"]
if isinstance(d.get("dnsCache"), list):
    d["dnsCache"] = [{"name": n, "type": "A", "ttl": "00:10:00"} for n in GENERIC]
for i, p in enumerate(d.get("wgPeers") or []):
    p["comment"] = f"back-to-home-vpn device {i + 1}"
    p["name"] = f"peer{i + 1}"

sys.stdout.reconfigure(encoding="utf-8")
sys.stdout.write(json.dumps(d, indent=1, ensure_ascii=False))
