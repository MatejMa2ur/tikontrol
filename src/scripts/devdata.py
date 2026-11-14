#!/usr/bin/env python3
"""Build src/ui/src/lib/devdata.ts: public device lists, extracted at build time (nothing is fetched at runtime).

Sources (both permissive; licence texts in src/scripts/devdata/, listed in THIRD_PARTY_LICENSES.txt):
  - Home Assistant core, homeassistant/generated/{dhcp,zeroconf,ssdp}.py + integrations.json (Apache-2.0):
    the discovery matchers of its integrations, i.e. "this host name / mDNS service / UPnP description is brand X".
  - kyle-seongwoo-jun/apple-device-identifiers (MIT): Apple model identifiers -> product names.
Download them into one empty directory with these names, then:
  python3 -I src/scripts/devdata.py DIR HA_COMMIT APPLE_COMMIT > src/ui/src/lib/devdata.ts
    DIR/ha-dhcp.py ha-zeroconf.py ha-ssdp.py ha-integrations.json
    DIR/apple-{ios,mac,watchos,tvos,visionos}-device-identifiers*.json
The Python files are parsed with ast.literal_eval, never imported or run. Extracted and reformatted by this script
(Apache-2.0 section 4(b)).
"""
import ast
import json
import os
import re
import sys

d, ha_commit, apple_commit = sys.argv[1], sys.argv[2], sys.argv[3]


def literal(name, var):
    """The value assigned to `var` in a generated HA file, without executing it."""
    tree = ast.parse(open(os.path.join(d, name), encoding="utf-8").read())
    for node in tree.body:
        target = node.target if isinstance(node, ast.AnnAssign) else node.targets[0] if isinstance(node, ast.Assign) else None
        if isinstance(target, ast.Name) and target.id == var:
            return ast.literal_eval(node.value)
    raise SystemExit(f"{var} not found in {name}")


# integration domain -> (display name, brand, kind)
KIND = {  # HA category -> tikontrol kind; categories that say nothing about the box are left out
    "media_entertainment": "speaker", "climate": "iot", "energy": "iot", "security_safety": "iot", "hubs_protocols": "iot",
    "appliances_cleaning": "iot", "lighting": "iot", "plugs_switches": "iot", "sensors": "iot", "blinds_curtains": "iot",
    "water": "iot", "garden_plants": "iot", "buttons_remotes": "iot", "pets": "iot", "health_wellbeing": "iot",
}
info = {}


def walk(m, brand=None):
    for dom, v in m.items():
        if "integrations" in v:
            walk(v["integrations"], v.get("name"))
            continue
        kinds = [KIND[c.split(".")[0]] for c in v.get("categories", []) if c.split(".")[0] in KIND]
        info[dom] = (v.get("name") or dom, brand or "", kinds[0] if kinds else "")
        if v.get("supported_by"):  # a virtual integration: its matchers live under the integration that supports it
            info.setdefault(v["supported_by"], info[dom])


walk(json.load(open(os.path.join(d, "ha-integrations.json"), encoding="utf-8"))["integration"])

SAFE = re.compile(r"^[\w .:*?\[\]!/@+#&'()-]{1,120}$")


def glob(p):
    """fnmatch glob -> JS regex source, anchored, matched case-insensitively. None for anything unexpected."""
    if not isinstance(p, str) or not SAFE.match(p):
        return None
    out, i = "", 0
    while i < len(p):
        c = p[i]
        if c == "*":
            out += ".*"
        elif c == "?":
            out += "."
        elif c == "[" and "]" in p[i + 1:]:
            j = p.index("]", i + 1)
            body = p[i + 1:j]
            out += "[" + ("^" + body[1:] if body.startswith("!") else body).replace("\\", "") + "]"
            i = j
        else:
            out += re.escape(c).replace("\\ ", " ")
        i += 1
    return "^" + out + "$"


def who(dom):
    name, brand, kind = info.get(dom, (dom, "", ""))
    return [name, brand, kind]


dhcp = []
for m in literal("ha-dhcp.py", "DHCP"):
    if m["domain"] not in info or m.get("registered_devices") or not ({"hostname", "macaddress"} & m.keys()) or set(m) - {"domain", "hostname", "macaddress"}:
        continue
    h, mac = glob(m.get("hostname", "*")), glob(m.get("macaddress", "*"))
    if h and mac and (h, mac) != ("^.*$", "^.*$"):
        dhcp.append([h, mac, *who(m["domain"])])

# Protocols many brands speak: a bare service match says nothing about the box (Macs and iPhones announce
# _companion-link too), so only rows with a name or TXT condition are kept for these.
GENERIC = {"_companion-link._tcp", "_hap._tcp", "_hap._udp", "_ipp._tcp", "_ipps._tcp", "_matter._tcp", "_matterc._udp",
           "_meshcop._udp", "_googlecast._tcp", "_airplay._tcp", "_raop._tcp", "_http._tcp", "_printer._tcp", "_ssh._tcp"}
mdns = []
for svc, ms in literal("ha-zeroconf.py", "ZEROCONF").items():
    svc = svc.removesuffix(".local.")
    for m in ms:
        if set(m) - {"domain", "name", "properties"} or (svc in GENERIC and len(m) == 1) or m["domain"] not in info:
            continue
        props = [[k, glob(v)] for k, v in (m.get("properties") or {}).items()]
        name = glob(m["name"]) if "name" in m else ""
        if None in [p[1] for p in props] or name is None:
            continue
        mdns.append([svc, name, props, *who(m["domain"])])

homekit = sorted(([md, *who(v["domain"])] for md, v in literal("ha-zeroconf.py", "HOMEKIT").items() if SAFE.match(md) and v["domain"] in info),
                 key=lambda r: -len(r[0]))  # longest prefix first

ssdp = []
UPNP = {"manufacturer", "deviceType", "modelName", "modelNumber", "friendlyName"}  # fields discovery.rs keeps
for dom, ms in literal("ha-ssdp.py", "SSDP").items():
    for m in ms:
        if not m or set(m) - UPNP or dom not in info:
            continue
        rules = [[k, glob(v)] for k, v in m.items()]
        if None not in [r[1] for r in rules]:
            ssdp.append([rules, *who(dom)])

apple = {}
for f in sorted(os.listdir(d)):
    if f.startswith("apple-") and f.endswith(".json") and "mac-device-identifiers.json" not in f:
        for k, v in json.load(open(os.path.join(d, f), encoding="utf-8")).items():
            name = v[0] if isinstance(v, list) else v
            if re.match(r"^[A-Za-z]+\d+,\d+$", k) and isinstance(name, str) and SAFE.match(name.replace(",", "")):
                apple.setdefault(k, name)

j = lambda v: json.dumps(v, ensure_ascii=False, separators=(",", ":"))
print(f"""// Generated by src/scripts/devdata.py; do not edit by hand. Extracted and reformatted from:
//  - Home Assistant core {ha_commit} (Apache-2.0): homeassistant/generated/{{dhcp,zeroconf,ssdp}}.py, integrations.json
//  - kyle-seongwoo-jun/apple-device-identifiers {apple_commit} (MIT)
// Licence texts: THIRD_PARTY_LICENSES.txt. Patterns are anchored regex sources, matched case-insensitively.
// Rows end with [display name, brand, kind]; kind '' = the list doesn't say.
type Who = [name: string, brand: string, kind: string]
export const DHCP: [host: string, mac: string, ...Who][] = {j(dhcp)}
export const MDNS: [service: string, name: string, props: [string, string][], ...Who][] = {j(mdns)}
export const HOMEKIT: [modelPrefix: string, ...Who][] = {j(homekit)}
export const SSDP: [rules: [string, string][], ...Who][] = {j(ssdp)}
export const APPLE: Record<string, string> = {j(apple)}""")
