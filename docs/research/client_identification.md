# How clients get a name, type, maker, OS and icon (2026-11)

The owner asked how UniFi shows a name, device type, maker, model, OS and icon for each client, and whether a
web protocol does it. There is no single protocol. UniFi combines several weak signals, and the strongest one is a
fingerprint database matched on the gateway, with icons fetched from Ubiquiti's servers. This note covers what UniFi does, what tikontrol
already does, and what RouterOS 7 lets us add without outbound calls.

Sources: the stopped local `unifi-study` container (UniFi Network Application 10.6, see `unifi_study.md`). We listed
only file names and string constants of its jars. No code, data or icons were copied. The rest is public
docs and forums, linked inline.

## 1. What UniFi does

| Signal | How UniFi uses it | Evidence |
|---|---|---|
| **Fingerprint engines** | Each client record carries `dev_id`, `dev_cat`, `dev_family`, `dev_vendor`, `os_name`, `confidence`, `fingerprint_source` and `fingerprint_engine_version`. The engine values are `TDTS`, `FINGERBANK`, `ML` and `CUSTOM`, ranked by an engine-priority manager. A user's choice is stored as `dev_id_override` / `fingerprint_override` (`CUSTOM`) and wins. | String constants in the app's `internal-dependencies.jar` (`FROM_TDTS`, `FROM_FINGERBANK`, `FROM_ML`, `FROM_CUSTOM`, `foundByFingerbank`, `foundByTdts`) |
| **Gateway DPI (TDTS)** | The gateway's "Traffic and Device Identification" (DPI) engine classifies the device from its traffic: DHCP options, HTTP User-Agent, the hosts it contacts and similar signals. The controller pushes the setting as a DPI-fingerprint config (`ubntconfDpiFingerprintConfigFactory`, `fingerprintingEnabled`). "TDTS" matches the name of the Trend Micro DPI kernel module found in several router firmwares. Ubiquiti does not document the vendor. | [UniFi help: Traffic and Device Identification](https://help.ui.com/hc/en-us/articles/12570783535383), jar strings |
| **Fingerbank** | Inverse's database matches DHCP option 55/60, DHCPv6, User-Agent and MAC prefix to a device. | `FINGERBANK` engine and `foundByFingerbank` counters. [Fingerbank licence](https://www.fingerbank.org/license) |
| **Device list and icons** | `dev_id` is resolved to names through `https://static.ubnt.com/fingerprint/<engine>/devicelist.json`. Icons come from `https://static.ui.com/fingerprint/ui/icons/`. The bundle also ships about 26 generic client photos. | URL constants in the jar, `images/clients/photos/dpi/*` in the web app |
| **Telemetry back** | `WrongFingerprint` and `MissingFingerprintIcon` trace events, plus a `FingerprintReport` (counts per engine, unidentified Ubiquiti clients), are sent to Ubiquiti. User overrides train their database. | `com/ubnt/service/trace/api/fingerprint/*` class names |
| **OUI** | `oui_table.json` (4.5 MB) is bundled. Its first entry ("Officially Xerox, but 0:0:0:0:0:0 is more common") is a line from Wireshark's `manuf`, so the table comes from Wireshark's file. | jar contents |
| **Hostname** | DHCP option 12/81. | [UniHosted guide](https://unihosted.com/blog/identifying-client-devices-on-your-unifi-network-a-detail-guide) |
| **LLDP / mDNS / UPnP** | LLDP (`wiredLldpProcessor`) for wired neighbours. mDNS appears as a proxy/reflector setting, not as an identification engine. | jar strings |

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
| 6 | **DHCPv6 DUID** from `/ipv6/dhcp-server/binding`. DUID-LL/LLT carry a MAC. | S | Low | Only helps where DHCPv6 is used. |
| 7 | Widen `oui.py` brands when real clients show "no maker". | S | Low | Random MACs make this less useful every year. |
| — | Fingerbank Cloud API | — | — | **Not by default.** Outbound third-party call, free tier 300 req/h. If ever added: opt-in, labelled, sending only option 55/60 and OUI, never hostnames. |
| — | DHCP debug logging, sniffer, DPI/User-Agent, Wi-Fi IE taxonomy | — | — | ➖ Needs router writes or broad sniffing, or RouterOS doesn't expose it. |

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
