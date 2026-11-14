# How clients get a name, type, maker, OS and icon (2026-11)

The owner asked how UniFi shows a name, device type, maker, model, OS and icon for each client, and whether a
web protocol does it. There is no single protocol. UniFi combines several weak signals, and the strongest one is a
fingerprint database matched on the gateway, with icons fetched from Ubiquiti's servers. This note covers what UniFi does, what tikontrol
already does, and what RouterOS 7 lets us add without outbound calls.

Sources: the stopped local `unifi-study` container (UniFi Network Application 10.6, notes kept locally). We listed
only file names and string constants of its jars. No code, data or icons were copied. The rest is public
docs and forums, linked inline.

## 1. What UniFi does

| Signal | How UniFi uses it | Evidence |
|---|---|---|
| **Fingerprint engines** | Each client record carries `dev_id`, `dev_cat`, `dev_family`, `dev_vendor`, `os_name`, `confidence`, `fingerprint_source` and `fingerprint_engine_version`. The engine values are `TDTS`, `FINGERBANK`, `ML` and `CUSTOM`, ranked by an engine-priority manager. A user's choice is stored as `dev_id_override` / `fingerprint_override` (`CUSTOM`) and wins. | String constants in the app's `internal-dependencies.jar` (`FROM_TDTS`, `FROM_FINGERBANK`, `FROM_ML`, `FROM_CUSTOM`, `foundByFingerbank`, `foundByTdts`) |
| **Gateway DPI (TDTS)** | Runs on the UniFi gateway as part of "Traffic and Device Identification" (see 1a). The controller pushes it as a DPI-fingerprint config (`ubntconfDpiFingerprintConfigFactory`, `fingerprintingEnabled`). *Our inference, not stated by Ubiquiti:* it reads the traffic the gateway already inspects, such as DHCP options, unencrypted headers and the hosts a client contacts. "TDTS" matches the name of the Trend Micro DPI kernel module found in several router firmwares. Ubiquiti does not name the vendor. | [UniFi help article](https://help.ui.com/hc/en-us/articles/12570783535383-UniFi-Gateway-Traffic-and-Device-Identification), jar strings |
| **Fingerbank** | Inverse's database matches DHCP option 55/60, DHCPv6, User-Agent and MAC prefix to a device. | `FINGERBANK` engine and `foundByFingerbank` counters. [Fingerbank licence](https://www.fingerbank.org/license) |
| **Device list and icons** | `dev_id` is resolved to names through `https://static.ubnt.com/fingerprint/<engine>/devicelist.json`. Icons come from `https://static.ui.com/fingerprint/ui/icons/`. The bundle also ships about 26 generic client photos. | URL constants in the jar, `images/clients/photos/dpi/*` in the web app |
| **Telemetry back** | The app has `WrongFingerprint` and `MissingFingerprintIcon` trace events and a `FingerprintReport` (counts per engine, unidentified Ubiquiti clients). They look like reports to Ubiquiti, but the help article says nothing about what is sent, so this is inferred from class names only. | `com/ubnt/service/trace/api/fingerprint/*` class names |
| **OUI** | `oui_table.json` (4.5 MB) is bundled. Its first entry ("Officially Xerox, but 0:0:0:0:0:0 is more common") is a line from Wireshark's `manuf`, so the table comes from Wireshark's file. | jar contents |
| **Hostname** | DHCP option 12/81. | [UniHosted guide](https://unihosted.com/blog/identifying-client-devices-on-your-unifi-network-a-detail-guide) |
| **LLDP / mDNS / UPnP** | LLDP (`wiredLldpProcessor`) for wired neighbours. mDNS appears as a proxy/reflector setting, not as an identification engine. | jar strings |

### 1a. What Ubiquiti states officially

The help article [UniFi Gateway - Traffic and Device Identification](https://help.ui.com/hc/en-us/articles/12570783535383-UniFi-Gateway-Traffic-and-Device-Identification)
(updated 2026-11-07) is short. It says:

- Both features are under Settings → System → Advanced, are "also referred to as Deep Packet Inspection or DPI", and
  **require a UniFi Gateway or Cloud Gateway**. Access points alone do not provide them.
- They are enabled on all networks. *Traffic Identification* finds the type of traffic passing through the gateway (Security
  Insights → Identified Traffic, which can feed Traffic Rules to block or limit an app). *Device Identification*
  finds the type of each device and lists **manufacturer, model and operating system** under Client Devices.
- **Device Identification requires Traffic Identification.** Device identity therefore comes mainly from inspecting the
  client's traffic on the gateway, not from a discovery protocol.

The article does not say which signals are used, what is sent to Ubiquiti, how often signatures update, or how
accurate the results are. Everything about engines, Fingerbank, icon URLs and reports in the table above comes from
the app's internals and is not documented. Third-party guides say the identification "isn't 100% accurate" and
that a manual override is the fix ([UniHosted](https://unihosted.com/blog/identifying-client-devices-on-your-unifi-network-a-detail-guide)).

DHCP fingerprinting works the same way in every product: the order of the option 55 parameter request list,
plus option 60, identifies the DHCP client stack and therefore the OS family
([Infoblox](https://infoblox-docs.atlassian.net/wiki/spaces/NAG8/pages/22252338/Configuring+DHCP+Fingerprints)).
Brand and model come from the other sources.

## 2. What tikontrol already does

- `src/ui/src/lib/oui.ts` (from `src/scripts/oui.py`): a subset of the IEEE MA-L registry, limited to about 60 brands, mapped to our own
  brand names. Private (locally administered) MACs show no maker. Hypervisor prefixes are recognised.
- `src/ui/src/lib/identify.ts`: guesses kind, type and OS from the hostname, `class-id` (option 60) and the maker.
  `/ip/neighbor` (MNDP/LLDP/CDP) wins for network gear.
- `src/server/src/discovery.rs` + `discovery.svelte.ts`: opt-in mDNS/DNS-SD and SSDP/UPnP queries on tikontrol's
  own LAN, run on demand or every 10 minutes. TXT and UPnP fields fill in maker and model. Values are sanitised and length-capped.
- Rename, custom icon and notes per client (`docs/FEATURES.md`).

Missing: option 55 fingerprints, learning from the user's corrections, mapping model identifiers to kinds, and
passive (listen-only) discovery.

**Update (2026-11):** steps 1–3 below are built behind Settings → System → Device identification (guesses on by
default; LAN queries and the DHCP listener opt-in). Learning is derived from `prefs.icons` and the current client
list (`discovery.svelte.ts`), the model table and the fingerprint key live in `identify.ts`, and the listener is in
`discovery.rs`. The port-67 bind inside the RouterOS container is still untested (see `DEPLOY.md`).

## 3. What RouterOS 7 exposes

| Source | Available? | Notes |
|---|---|---|
| `/ip/dhcp-server/lease`: `host-name`, `class-id` (60), `client-id` (61), `agent-circuit-id`/`agent-remote-id` (82), `active-mac-address` | Yes, read-only | Already read except `client-id`. Option 61 usually just repeats the MAC, so it adds little. |
| Option 55 | **No** | Not a lease field and not in the lease-script `lease-options` array, which holds only server-side options plus option 43 ([forum](https://forum.mikrotik.com/t/dhcp-server-lease-script-is-client-sent-option-55-parameter-request-list-accessible/269752)). |
| `/system/logging topics=dhcp,debug` | Write needed | The debug log prints a `Parameter-List` line per packet, but one packet spans several lines with no transaction ID ([forum](https://forum.mikrotik.com/t/future-request-better-dhcp-debug-logging/161180)). Turning it on is a config change and fills the log. |
| `/ip/dhcp-server/matcher` | Not useful | Matches an option (exact or substring) to choose a pool or option set. It cannot record what it saw ([manual](https://help.mikrotik.com/docs/spaces/ROS/pages/24805500/DHCP)). |
| `/tool/sniffer` streaming (TZSP) | Write needed | Works, but needs an extra policy, and the stream carries all matching traffic. Too much power for a reader. |
| `/ip/neighbor` (MNDP/LLDP/CDP) | Yes | Already used. |
| Wi-Fi probe/association IEs, HTTP User-Agent, DPI | No | RouterOS has no hook for these. TLS hides User-Agents anyway. CMR has per-device app classes on some architectures (`RESEARCH.md`). It does not identify devices. |

**Traffic as a device signal.** This is the RouterOS counterpart of UniFi's "Device Identification needs Traffic
Identification". The domains a client talks to are a strong signal: a device that only reaches one vendor's cloud is
that vendor's device.

| Source | Write needed? | Notes |
|---|---|---|
| `/ip/dns/cache` + `/ip/firewall/connection` | No, read-only | The DNS cache maps names to IPs. The connection table maps each client's src-address to dst IPs. Joining them gives "client → domains". Not reliable: CDN IPs are shared, cache entries expire, and clients using DoH/DoT or their own resolver bypass the router. |
| `/ip/kid-control/device` `activity` | Read-only, but needs kid-control in use | A read-only string that forums report holds the site a device is visiting. Its meaning is undocumented and it was reported blank before a 7.20 fix ([manual](https://manual.mikrotik.com/docs/cli-reference/ip/kid-control/device/), [forum](https://forum.mikrotik.com/t/how-to-allow-to-see-kid-control-actiity/160964)). Use it if present, never rely on it. |
| Firewall `tls-host` | Yes (mangle rules) | Matches the TLS ClientHello SNI with globs. TCP only, misses fragmented handshakes, QUIC and ECH ([forum](https://forum.mikrotik.com/t/tls-host-no-document/115012), [MUM slides](https://mum.mikrotik.com/presentations/CR18/presentation_5701_1532535774.pdf)). It cannot log the SNI. It only counts matches or adds the src to an address list per pattern, so each vendor domain needs its own rule. Too costly for identification. |
| `/ip/firewall/layer7-protocol` | Yes | Regex on the first bytes of unencrypted traffic. Useless with TLS, and CPU-heavy. ➖ |
| Traffic Flow (IPFIX) → tikontrol collector | Yes, once (`/ip/traffic-flow` + target) | Flow records (src, dst, ports, bytes) go to the container. They carry no names, so join them with the DNS cache as above. Adds router CPU. It belongs to traffic stats (`RESEARCH.md` "top domains"), not to identification alone. |

**Key point:** tikontrol already runs on the LAN bridge (RouterOS App / container veth, `docs/INSTALL.md`), so it
receives **broadcast** DHCPDISCOVER/REQUEST packets itself. Listening on UDP 67, without ever replying, captures option 55
and its order for every client that joins or reconnects. The router needs no change. Renewals are unicast to the router,
so they are missed, but the fingerprint does not change between them.

## 4. Recommended steps (ranked)

| # | Step | Effort | Benefit | Notes |
|---|---|---|---|---|
| 1 | **Learn from overrides.** When the user sets a client's type/icon, store it keyed by `(option-60 class, option-55 list or hostname pattern, maker)`, and suggest it for other clients with the same key ("Looks like your other X"). | S | High | This is UniFi's `CUSTOM` engine without the upload. Stored locally only. |
| 2 | **Model identifiers to kind.** Map mDNS TXT `model`/`md`/`rpMd`/`am` and UPnP `modelName` prefixes to a kind (e.g. Apple `iPhone*`/`iPad*`/`Mac*`/`AppleTV*`/`AudioAccessory*`/`Watch*`, Google Cast `md`, printers' `ty`). | S | Medium-high | A small table in `identify.ts` with a test. These identifiers are facts, not copyrighted data. |
| 3 | **Passive DHCP listener (option 55/60/12/81).** In `discovery.rs`: bind UDP 67 receive-only on the LAN interface, parse BOOTREQUEST options, and store `mac → {prl, class, hostname, fqdn}`. Opt-in under the same toggle as discovery. | M | High | Never send. Validate lengths at the parser and add a parser test with made-up packets. A non-root bind to port 67 needs `CAP_NET_BIND_SERVICE` (file capability in our image) or a raw socket. Check on CHR that the RouterOS container allows it. Docker deploys need host networking. Only the container's own L2 segment/VLAN is seen. |
| 4 | **Own option-55 signature table.** Map a few dozen well-known request-list prefixes to an OS family (Windows, macOS/iOS, Android, Linux dhcpcd/dhclient, udhcpc embedded, ESP-IDF/lwIP). Grow it from the owner's own clients, recorded anonymised. | S–M | Medium | Write it ourselves from RFC option numbers and observed packets. Do not import Fingerbank's data (licence below). |
| 5 | **Passive mDNS.** Keep the multicast socket open and record unsolicited announcements, not only scheduled queries. | S | Medium | Same sanitising and caps as today. It costs less than querying. |
| 5b | **Vendor-domain hint (read-only).** Join `/ip/dns/cache` with `/ip/firewall/connection` and match against a small table of our own vendor cloud domains (e.g. a TV maker's, a speaker maker's, a camera cloud's) to suggest a maker or kind. Label it "guess from traffic". | M | Medium | This is the closest RouterOS analogue to UniFi's DPI-based identification, without router changes or outbound calls. Domains are untrusted text: match them, show them as text only, never forward them. Can be reused when the "top domains" traffic feature is built. Use kid-control `activity` as an extra input only if it is filled. |
| 6 | **DHCPv6 DUID** from `/ipv6/dhcp-server/binding`. DUID-LL/LLT carry a MAC. | S | Low | Only helps where DHCPv6 is used. |
| 7 | Widen `oui.py` brands when real clients show "no maker". | S | Low | Random MACs make this less useful every year. |
| — | Fingerbank Cloud API | — | — | **Not by default.** Outbound third-party call, free tier 300 req/h. If ever added: opt-in, labelled, sending only option 55/60 and OUI, never hostnames. |
| — | DHCP debug logging, sniffer, DPI/User-Agent, Wi-Fi IE taxonomy | — | — | ➖ Needs router writes or broad sniffing, or RouterOS doesn't expose it. |
| — | `tls-host` rules, layer7 | — | — | ➖ Router writes per pattern, weak against TLS 1.3/QUIC/ECH. |
| — | Traffic Flow export | — | — | Later, together with per-client traffic stats. Not worth a router change for identification alone. |

The UniFi article does not change the order. Its main lesson is that UniFi's device identity mostly comes from traffic
inspection on the gateway. On RouterOS the cheap, read-only part of that is step 5b. Steps 1–3 remain the best value
because they need no router change and work for devices that are quiet on the internet.

Icons stay ours: draw one per `Kind` in `Icon.svelte`. Never fetch UniFi's icon CDN or copy its photos.

## 5. Licence notes

- **IEEE MA-L registry** (`oui.csv`): IEEE publishes it for public lookup but states no licence or terms for the
  listing ([IEEE RA FAQ](https://standards.ieee.org/faqs/regauth/), [registry page](https://standards.ieee.org/develop/regauth/oui/public.html)).
  A prefix-to-company assignment is a fact. Our approach (a small extract under our own brand labels, built offline by
  `oui.py`, no runtime fetch) is the low-risk option. Keep the source note in `oui.ts`.
- **Wireshark `manuf`**: generated by `tshark -G manuf`. The file has no licence header, and Wireshark itself is GPL-2.0-or-later. Prefer
  IEEE's `oui.csv` as input so the GPL question never comes up. `oui.py` accepts both, so document the IEEE one as the default.
- **Fingerbank**: the current database and API are commercial ([licence](https://www.fingerbank.org/license)).
  Old 2012 copies were published under ODbL/DbCL, which is share-alike and not a permissive licence. Under our licence rule we
  would need the owner's agreement, and the data is stale anyway. Do not bundle it.
- **UniFi**: nothing from the app (OUI JSON, device list, icons, photos) may be used. This note records only behaviour,
  read from names.
