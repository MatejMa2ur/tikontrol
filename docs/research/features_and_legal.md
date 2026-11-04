# UniFi Network features on MikroTik RouterOS v7: feasibility map and legal risk of a UniFi-inspired UI

Research date: 2026-11-04. Public product facts only. The legal sections are not legal advice.

**Evidence labels used on every finding**
- **[P]** Primary source, page opened and read (MikroTik manual, ui.com, legislation).
- **[P*]** Primary source, but seen only through a search-tool summary or snippet. The full page was not opened.
- **[S]** Secondary source (law firm, press, third-party project, independent review).
- **[U]** User report (forum post). Not confirmed by the vendor.
- Inferences are kept in the Inferences subsections. Unsourced recollections are kept in Gaps and marked "unverified".

**Read this first: three limits on the evidence**
1. RouterOS 7.26 was still in beta at the research date. The newest build found is 7.26beta5, dated 2026-10-30. Everything about CMR and `/tool/apptraffic` describes beta behaviour and can change.
2. help.mikrotik.com pages are now marked as frozen, and manual.mikrotik.com is the current manual. A few findings still rest on frozen pages and are marked.
3. Court judgments could not be opened. EUR-Lex returned empty pages, and Justia, IPPT and WIPO Lex returned 403. Every case holding below therefore rests on secondary summaries, even where a judgment is named. Check the originals before relying on them.

## What is the overall map of possible, partly possible and impossible?

### Takeaway
RouterOS 7.26 closes much more of the gap than earlier versions did, because MikroTik has just shipped its own controller (CMR) and an application classifier (`/tool/apptraffic`). Fleet management, alerts, topology and basic app statistics are now feasible on supported hardware. Per-client application breakdown, app-based firewall policy, UniFi-grade device fingerprinting and non-disruptive RF scanning are the real blockers.

### Cited Findings

| UniFi feature | Verdict | What RouterOS offers | Main limit | Evidence |
|---|---|---|---|---|
| App statistics, whole site | Partly | `/tool/apptraffic`: per-application and category totals | Beta. Not on mmips, smips, powerpc. Routed traffic only | [P] |
| App statistics per client | **Not native** | `apptraffic/stats` has no client, IP or MAC field | Needs an external flow, DNS and SNI pipeline, with lower accuracy | [P] for the absence of the field |
| Block or limit by app | **Not found** | Layer-7 regex only | L7 cannot read encrypted traffic | [P] |
| Per-client bytes and rate | Partly | Kid Control, connection table, Wi-Fi registration table, Traffic Flow | Each has blind spots with FastTrack or offload | [P], [U] |
| Long-term history | Partly | Apptraffic windows reach one week. Traffic Flow exports to a collector | Needs an external database | [P] |
| Client list | Possible | DHCP leases, ARP, Wi-Fi registration table, neighbours | Merging sources is the app's job | [P], [P*] |
| Device fingerprint (make, model, OS) | **Mostly not** | Host name, option 60, MAC vendor | Option 55 reported unavailable. Random MACs defeat vendor lookup | [P*], [U] |
| Topology map | Partly to possible | `/cmr/layout`, `/ip/neighbor` | RouterOS devices best. Unmanaged segments are guesswork | [P] |
| Adoption and pairing | Possible, with catches | CMR pairing | CMR does not adopt existing config | [P] |
| Fleet firmware | Possible | CMR upgrade rules, CAPsMAN upgrade policy | Beta. One job at a time | [P] |
| Central Wi-Fi | Possible | CAPsMAN, or CMR Wi-Fi | The two conflict. CMR takes over radios | [P] |
| Central switch VLANs | Possible, risky | `/cmr/vlan` | Broad rules can cut off the fleet | [P], [S] |
| Guest portal | Possible | Hotspot | IPv4 only. HTTPS redirect gives certificate errors | [P] |
| Vouchers | Possible | User Manager | Separate package. Sessions capped by licence. PayPal only | [P] |
| Zone firewall | Partly (as a UI abstraction) | Interface lists, address lists | No native zone object. No app or region matcher found | [U], [P] |
| Band steering | Partly | `2g-probe-delay`, 802.11k/v, neighbour groups | Soft only. Clients may ignore it | [P] |
| Fast roaming | Possible | 802.11r/k/v | Same RouterOS instance needed. No OWE | [P] |
| RF scan | Partly | `frequency-scan`, `scan`, spectral scan | Frequency scan disconnects clients. Spectral is `wifi-qcom` only | [P] |
| Speed test | Partly | `/tool/speed-test` (router to router) | No built-in Ookla test | [P*], [U] |
| Remote access without a cloud | Possible | WireGuard, Back To Home | BTH needs ARM, ARM64 or TILE. No hosted portal | [P*] |
| Notifications | Possible | CMR alerts: log, script, HTTP webhook | Phone push needs your own relay | [P] |
| Mobile app | Possible, must be built | REST API | Nothing native for third parties | [S] (ha-cmr uses REST) |

**Hard blockers, ranked**
1. No per-client application data from the classifier. `tool/apptraffic/stats` has 26 read-only fields keyed by `application` and `category`, and none by client, host, IP or MAC — [P: apptraffic stats](https://manual.mikrotik.com/docs/cli-reference/tool/apptraffic/stats/).
2. The classifier is unavailable on mmips, smips and powerpc — [P: CMR guide](https://manual.mikrotik.com/docs/management-tools/cmr/).
3. No application-aware firewall matcher was found. The firewall feature list offers only "Custom Layer7 matcher" — [P: Software Specifications](https://manual.mikrotik.com/docs/getting-started/software-specifications/).
4. Monitoring can damage performance. A running sniffer or torch, or an active mac-scan or ip-scan, is listed as incompatible with FastTrack — [P, frozen page: MikroTik help](https://help.mikrotik.com/docs/spaces/ROS/pages/130220087/Firewall+and+QoS+Case+Studies).
5. CMR "does not adopt existing configuration" and manages RouterOS devices only — [P: CMR guide](https://manual.mikrotik.com/docs/management-tools/cmr/); [S: routeros-cmr skill](https://www.skills.sh/tikoci/routeros-skills/routeros-cmr) for "does not provide general ping/SNMP/HTTP monitoring of non-RouterOS equipment".

### Inferences
- MikroTik is now a direct competitor to any third-party controller. CMR's dashboard, map, upgrades and alerts overlap the core of a UniFi-like product. A third-party UI has to win on the areas CMR explicitly leaves out: IP addressing, DHCP, routing, firewall and users.
- A UniFi-like product cannot run purely on the router. History, per-client analytics and fingerprinting all point to an external server or a container with a time-series store.

### Gaps
- Whether 7.26 stable will match the beta. No stable release was found.

## Does RouterOS have real application classification, and what does it cost?

### Takeaway
Yes, since 7.26beta1, but it is undocumented, aggregate and reporting-only as far as public sources show. Layer-7 regex is not a substitute, because it cannot read encrypted traffic. Traffic Flow plus DNS and SNI correlation is the only route to per-client app views, and it needs an external collector.

### Cited Findings
**`/tool/apptraffic` and CMR app traffic**
- CMR's server "enables the RouterOS application traffic classifier (`/tool/apptraffic`)" on devices selected by `apptraffic-devices`, which defaults to the `gateway` label — [P: CMR guide](https://manual.mikrotik.com/docs/management-tools/cmr/); [P: cmr CLI reference](https://manual.mikrotik.com/docs/cli-reference/cmr/).
- Data is per-application traffic in and out over the last minute, hour, day and week, also grouped by category, in the GUI and through `/cmr/device/apptraffic` — [P: CMR guide](https://manual.mikrotik.com/docs/management-tools/cmr/).
- The classifier sees only traffic the device routes. Bridged fast-path traffic on an access point is not visible — [P: CMR guide](https://manual.mikrotik.com/docs/management-tools/cmr/).
- The CLI reference is a stub. It lists only `enabled` and `stats`, with the condition `!mmips, !powerpc, !smips` — [P: apptraffic reference](https://manual.mikrotik.com/docs/cli-reference/tool/apptraffic/).
- `stats` also offers a ten-minute window, which the CMR guide does not mention — [P: apptraffic stats](https://manual.mikrotik.com/docs/cli-reference/tool/apptraffic/stats/).
- 7.26beta3 changelog: "fixed connection tracking not reverting to auto when apptraffic is disabled", and restored missing application counters in the CMR view — [P: 7.26beta forum thread](https://forum.mikrotik.com/t/7-26beta-development-is-released/273582).
- A hands-on reviewer found app reports depend on the observation point, called application reporting limited, and wants more flexible identification. This was a few days of beta testing — [S: Link Technologies](https://shop.linktechs.net/cmr_first_look).
- Ubiquiti's comparison point: Traffic Identification covers traffic "passing through the gateway", and rules can block or speed-limit an app for one or more devices — [P*: UniFi help](https://help.ui.com/hc/en-us/articles/12570783535383-UniFi-Gateway-Traffic-and-Device-Identification).

**Layer-7 matcher**
- It "collects the first 10 packets of a connection or the first 2KB of a connection". If there is no match, inspection stops and the protocol is marked unknown — [P: Layer7](https://manual.mikrotik.com/docs/firewall-and-quality-of-service/firewall/layer7/).
- MikroTik calls it "very resource-intensive", warns that a device "will exhaust its resources", and says "Only unencrypted HTTP can be matched" — [P: Layer7](https://manual.mikrotik.com/docs/firewall-and-quality-of-service/firewall/layer7/).
- Forum users call it useless for HTTPS and QUIC — [U: Block TikTok thread](https://forum.mikrotik.com/t/block-tiktok-at-layer-7/267633).

**Traffic Flow (NetFlow and IPFIX)**
- It supports v1, v5, v9 and IPFIX. IPFIX fields include bytes, packets, addresses, ports, MAC addresses, interfaces and NAT translations. Packet sampling exists in v7 — [P, frozen page: Traffic Flow](https://help.mikrotik.com/docs/spaces/ROS/pages/21102653/Traffic+Flow).
- It sees only traffic the CPU processes, so hardware-offloaded bridged traffic does not appear. `cache-entries` defaults to 4k, and a very small inactive timeout can overflow the buffer — [P, frozen page: Traffic Flow](https://help.mikrotik.com/docs/spaces/ROS/pages/21102653/Traffic+Flow).
- 7.26beta3 adds post-NAT IPv6 addresses to NetFlow records — [P: 7.26beta forum thread](https://forum.mikrotik.com/t/7-26beta-development-is-released/273582).

**DNS and SNI inference**
- A third-party project, ros-dpi, builds per-client app statistics from IPFIX plus DNS and TLS SNI sniffing, and says RouterOS lacks DPI. Seen as a snippet only, and the claim may predate apptraffic — [S: ros-dpi](https://github.com/yxzhoubing/ros-dpi).

**Cost on a small router**
- No published CPU or RAM figure for apptraffic was found. The CMR guide does not discuss resource cost, and no staff comment on it appears in the beta thread — [P: CMR guide](https://manual.mikrotik.com/docs/management-tools/cmr/); [P: 7.26beta forum thread](https://forum.mikrotik.com/t/7-26beta-development-is-released/273582).
- No published L7 benchmark per model was found. MikroTik's warnings are qualitative — [P: Layer7](https://manual.mikrotik.com/docs/firewall-and-quality-of-service/firewall/layer7/).

### Inferences
- The beta3 changelog line suggests enabling apptraffic forces connection tracking on. That would rule out a "connection tracking off" fast router, and it hints at a conntrack-based design.
- Apptraffic appears to be reporting-only, because the reference shows no action or matcher. UniFi-style "block this app" would need domain lists and address lists instead, which is a different and weaker mechanism.
- Accuracy of DNS and SNI inference should be expected to fall as encrypted DNS and encrypted client hello spread, and shared CDN addresses blur attribution. This is general reasoning, not a measured result.
- UniFi's app view is also gateway-based, so "routed traffic only" is not by itself a gap against UniFi. Same-subnet traffic is invisible to both gateways.

### Gaps
- **The classification method is undocumented.** It is unknown whether apptraffic uses signatures, DNS, SNI or a licensed engine, how many apps it knows, and how signatures are updated.
- **Unknown whether FastTracked traffic is classified.** This is the most important test to run on hardware, along with CPU and RAM use on a small ARM device.
- Unknown whether stats are retained across reboots.
- RouterOS firewall `tls-host` matching and FQDN address lists were not verified in this research.

## What per-client statistics and history can RouterOS provide, and what breaks them?

### Takeaway
There are several partial sources and no single complete one. Kid Control is the easiest, the connection table is the most FastTrack-aware, and the Wi-Fi registration table is the best for wireless. History must be built outside the router.

### Cited Findings
**Sources**
- Kid Control `device print stats` gives rate-down, rate-up, bytes-down and bytes-up, plus idle time. Devices are identified by "the MAC address that is retrieved from the ARP table" — [P: Kid Control](https://manual.mikrotik.com/docs/firewall-and-quality-of-service/kid-control/).
- It enforces limits by "adding dynamic firewall filter rules or simple queue rules" — [P: Kid Control](https://manual.mikrotik.com/docs/firewall-and-quality-of-service/kid-control/).
- The connection list has `orig-bytes`, `repl-bytes` and rate fields, and separate `orig-fasttrack-bytes`, `repl-fasttrack-bytes` and packet counters — [P, frozen page: MikroTik help](https://help.mikrotik.com/docs/spaces/ROS/pages/130220087/Firewall+and+QoS+Case+Studies).
- The Wi-Fi registration table gives `signal`, `tx-rate`, `rx-rate`, `tx-bits-per-second`, `rx-bits-per-second`, `bytes`, `packets`, `uptime`, `band`, `vlan-id` and `ssid` — [P: Wi-Fi](https://manual.mikrotik.com/docs/wireless/wifi/).
- "CAPs don't send traffic usage information to CAPsMAN" — [P: WiFi CAPsMAN](https://manual.mikrotik.com/docs/wireless/wifi/capsman/).
- IP accounting appears to be gone from v7. Forum users point to Kid Control, Traffic Flow or scripts. No official removal notice was found — [U: replacement thread](https://forum.mikrotik.com/t/routeros-7-replacement-for-ip-accounting/270287); [U: Traffic Accounting](https://forum.mikrotik.com/t/traffic-accounting/180760).

**FastTrack and offload conflicts**
- "FastTrack packets bypass firewall, connection tracking, simple queues, queue tree with parent=global, ip traffic-flow, IP accounting, IPSec, hotspot universal client, VRF assignment" — [P: Packet Flow](https://manual.mikrotik.com/docs/firewall-and-quality-of-service/packet-flow-in-routeros/). **Conflict:** the frozen help page's list, as read, omits traffic-flow — [P, frozen page: MikroTik help](https://help.mikrotik.com/docs/spaces/ROS/pages/130220087/Firewall+and+QoS+Case+Studies).
- FastTrack needs no running sniffer, torch or traffic generator, and no active mac-scan or ip-scan — [P, frozen page: MikroTik help](https://help.mikrotik.com/docs/spaces/ROS/pages/130220087/Firewall+and+QoS+Case+Studies).
- Offloading moves forwarding and filtering to the switch chip without CPU involvement — [P: Packet Flow](https://manual.mikrotik.com/docs/firewall-and-quality-of-service/packet-flow-in-routeros/).
- A forum user says their Kid Control tracking is "fully compatible with fasttrack". This is one user's claim about their own setup — [U: Per-IP WAN statistics](https://forum.mikrotik.com/t/per-ip-wan-traffic-statistics/157975/55).
- Kid Control counts all routed traffic, including inter-VLAN, not only WAN — [U: Per-IP WAN statistics](https://forum.mikrotik.com/viewtopic.php?t=185910).
- A user on 7.10 saw about 30 Mbps reported during a 100 Mbps test, suggesting live rates are averaged while byte counts look right — [U: Kid control + SNMP](https://forum.mikrotik.com/t/kid-control-snmp/165838).
- IPv6 reports conflict. One says counters use IPv4 only, another says IPv6 bytes are counted and rate queues may be IPv4 only. The manual does not mention IPv6 — [U: Kid control + SNMP](https://forum.mikrotik.com/t/kid-control-snmp/165838); [P: Kid Control](https://manual.mikrotik.com/docs/firewall-and-quality-of-service/kid-control/).
- Disabling FastTrack raises CPU load. One secondary guide estimates a 30 to 70 percent CPU saving from FastPath and FastTrack. This figure is not from MikroTik — [S: RouterOS Performance Tuning](https://mikrotikdocs.fyi/system/performance-tuning/).

### Inferences
- A simple queue per client gives exact counters but needs FastTrack off for that traffic. On a small router that trades throughput for visibility.
- The `*-fasttrack-bytes` counters suggest periodic polling of the connection table can estimate per-client use with FastTrack on. Short connections that open and close between polls would be missed.
- Wireless per-client data must be polled from each CAP directly, because CAPs do not report usage to CAPsMAN.
- The app must never use torch, the sniffer or ip-scan as a routine data source, because they conflict with FastTrack.
- Traffic between two clients on the same bridge with hardware offload will not appear in any router-side counter. Only switch port counters will show it.

### Gaps
- **Kid Control and FastTrack: no authoritative statement.** Test by comparing its counters against a known transfer.
- **FastTrack and Traffic Flow: sources conflict.** My recollection is that this restriction was lifted in RouterOS 6.33 (unverified). The current manual still lists it, so test it.
- Whether Kid Control counters survive a reboot was not established.
- A cost benchmark for FastTrack off on small routers was not found.

## How well can clients be listed and identified?

### Takeaway
A good client list is achievable by merging sources. UniFi-style identification of make, model and OS is not, unless packets are captured off the router.

### Cited Findings
- DHCP leases expose `host-name` (option 12), `class-id` (option 60) and the client identifier (option 61) — [P*: lease reference](https://manual.mikrotik.com/docs/cli-reference/ip/dhcp-server/lease/).
- A forum user building a fingerprinter reports that client-sent options such as option 55 are absent from lease-script data, which holds only server-sent options — [U: option 55 thread](https://forum.mikrotik.com/t/dhcp-server-lease-script-is-client-sent-option-55-parameter-request-list-accessible/269752). The official reference matches this, saying the script receives options sent to the client — [P*: DHCP](https://manual.mikrotik.com/docs/network-management/dhcp/).
- By default the server recognises clients by `client-mac,client-id`, so a client changing its identifier gets a new lease — [P*: dhcp-server reference](https://manual.mikrotik.com/docs/cli-reference/ip/dhcp-server/).
- Users detect random MACs through the locally administered bit in a lease script. The thread does not confirm the script works — [U: random MAC thread](https://forum.mikrotik.com/t/how-to-conquer-random-mac-address/146346).
- Kid Control users say rotating MACs make accurate device tracking impossible — [U: Collecting usage stats](https://forum.mikrotik.com/viewtopic.php?t=159684).
- UniFi lists manufacturer, model and OS per client, and Device Identification requires Traffic Identification — [P*: UniFi help](https://help.ui.com/hc/en-us/articles/12570783535383-UniFi-Gateway-Traffic-and-Device-Identification).

### Inferences
- Stable identity under MAC randomisation needs heuristics, such as host name plus client-id plus SSID, and will sometimes be wrong. The UI should allow manual merge and naming of clients.
- MAC vendor lookup needs a bundled OUI table in the app and returns nothing useful for random MACs.
- Fingerprinting by option 55 would need a capture path. Running it on the router conflicts with FastTrack (see above), so it would need a mirror port to an external box.

### Gaps
- The bridge host table was not returned by searches, so its fields were not confirmed from the manual.
- Whether option 55 is still unavailable in 7.26 was not confirmed. The forum post is from 2026 but is a single user.
- UniFi's fingerprint database size and accuracy were not researched.

## What topology can really be drawn?

### Takeaway
Router-to-router and switch links between RouterOS devices can be drawn well, and CMR already does it. Client attachment and non-MikroTik segments are best-effort.

### Cited Findings
- `/ip/neighbor` lists devices found by MNDP, CDP and LLDP in the Layer 2 broadcast domain, with `discovered-by` from 7.7 — [P*: Neighbor Discovery](https://manual.mikrotik.com/docs/system-information-and-utilities/neighbor-discovery/).
- Fields include interface, mac-address, identity, platform, version, system-caps, board and address4/6 — [P*: ip/neighbor reference](https://manual.mikrotik.com/docs/cli-reference/ip/neighbor/).
- Discovery depends on `discover-interface-list`. Protocols default to `cdp,lldp,mndp` — [P*: discovery-settings](https://manual.mikrotik.com/docs/cli-reference/ip/neighbor/discovery-settings/).
- CMR `track-topology` collects "routes, WiFi registrations, ARP, neighbors, DHCP state, and interface states" — [P: cmr CLI reference](https://manual.mikrotik.com/docs/cli-reference/cmr/).
- `rebuild-links` creates links between directly connected devices, and links show port traffic and PoE state. "The topology itself is displayed only in the GUI" — [P: CMR guide](https://manual.mikrotik.com/docs/management-tools/cmr/).
- A third-party integration reads `/cmr/layout`, `/cmr/layout/node` and `/cmr/layout/link` over REST. REST omits a link's detected ports, which need `/execute`, and `print as-value` on links returns nothing — [S: ha-cmr](https://github.com/Anrijs/ha-cmr).
- Reviewer: topology was reasonably accurate on a small network. It showed correctly in WebFig but not in WinBox in that test — [S: Link Technologies](https://shop.linktechs.net/cmr_first_look). Staff suggest WebFig for now because "it has more features, like maps" — [P: 7.26beta forum thread](https://forum.mikrotik.com/t/7-26beta-development-is-released/273582).

### Inferences
- Neighbour data is per broadcast domain. An unmanaged switch that does not speak LLDP stays invisible, and several devices will appear to sit on one port.
- Wired client-to-port placement can be derived from bridge hosts, but it degrades behind unmanaged switches.

### Gaps
- Whether neighbour entries expose the remote port name for every protocol was not confirmed.
- Behaviour with non-MikroTik LLDP devices was not tested by any source found.

## How far can device adoption and firmware management go?

### Takeaway
CMR is a genuine fleet controller, but it is beta and has sharp edges. Third-party tools can sit on top of its REST menus today. Replacing it means reverse engineering, and per-device API access remains necessary for everything CMR declines to manage.

### Cited Findings
**CMR**
- CMR means Centralized Monitoring and Remote management. One device runs the `cmr` package, and clients are built into RouterOS — [P: CMR guide](https://manual.mikrotik.com/docs/management-tools/cmr/).
- The server runs only on arm, arm64 and x86. Every device can be a client except smips and powerpc — [P: CMR guide](https://manual.mikrotik.com/docs/management-tools/cmr/).
- Clients find the server by MNDP, DNS or `controller-addresses`, over TCP 54321, and can cross routed networks, NAT or VPN — [P: CMR guide](https://manual.mikrotik.com/docs/management-tools/cmr/).
- Pairing requirements are `none`, `password` or `confirm`, and the reset button can pair too — [P: CMR guide](https://manual.mikrotik.com/docs/management-tools/cmr/).
- Upgrade rules set schedule, channel, strategy and fail policy. "Only one upgrade job runs at a time." A pinned version can downgrade devices — [P: CMR guide](https://manual.mikrotik.com/docs/management-tools/cmr/).
- CMR does not configure IP addressing, DHCP, routing, firewall or users. It does not adopt existing configuration, coordinate roaming, or act as a central authentication server — [P: CMR guide](https://manual.mikrotik.com/docs/management-tools/cmr/).
- Managed objects cannot be edited on the client. CMR Wi-Fi networks replace existing radio configuration, including CAPsMAN-managed radios, and removing them does not restore it — [P: CMR guide](https://manual.mikrotik.com/docs/management-tools/cmr/).
- `run-script` is limited to `read`, `write`, `test` and `reboot` — [P: CMR guide](https://manual.mikrotik.com/docs/management-tools/cmr/).
- A VLAN rule caused about three hours of downtime in a test, because it cut the path to the controller and needed local console recovery. The reviewer also notes no fleet backup workflow, and that scheduled upgrades were unproven — [S: Link Technologies](https://shop.linktechs.net/cmr_first_look).
- First release date differs by source: 2026-10-01 in the changelog, 2026-10-05 in the review — [P: 7.26beta forum thread](https://forum.mikrotik.com/t/7-26beta-development-is-released/273582); [S: Link Technologies](https://shop.linktechs.net/cmr_first_look).

**Third-party access**
- The CMR guide does not document an API, but REST works for devices, alerts, upgrades, layouts and Wi-Fi. "Live Wi-Fi client counts" are "not available from the controller's API yet" — [S: ha-cmr](https://github.com/Anrijs/ha-cmr).
- `go-mikrotik-cmr` is a controller "reverse-engineered from the RouterOS 7.26 binaries". It needs open pairing, password (PKEX) pairing is not implemented, and VLAN is the only config type worked out end to end. Its README calls CMR a Wi-Fi and mesh controller, which is narrower than the manual — [S: go-mikrotik-cmr](https://github.com/AthenaNetworks/go-mikrotik-cmr).

**CAPsMAN**
- Any device with the WiFi package can run it. Unlimited CAPs are supported, and CAPs need at least a Level 4 licence — [P: WiFi CAPsMAN](https://manual.mikrotik.com/docs/wireless/wifi/capsman/).
- `upgrade-policy` is `none`, `require-same-version` or a suggest value. The page's value name and description do not match. With an empty `package-path`, only CAPs of the same architecture as the manager are upgraded — [P: WiFi CAPsMAN](https://manual.mikrotik.com/docs/wireless/wifi/capsman/).
- "WiFi CAPsMAN can only control WiFi interfaces, and WiFi CAPs can join only WiFi CAPsMAN." Bridge and port setup stay local — [P: WiFi CAPsMAN](https://manual.mikrotik.com/docs/wireless/wifi/capsman/).

### Inferences
- A sensible architecture uses CMR over REST where available and the per-device REST API for DHCP, firewall and routing. It should offer read-only onboarding, since neither system adopts existing configuration cleanly.
- UniFi-style "one click adopt and everything is configured" is not achievable on an already configured MikroTik device without risk. A safety step such as a config export before any change is warranted.
- Depending on a reverse-engineered protocol is fragile during a beta, and it carries the licence questions noted in the legal section.

### Gaps
- MikroTik has published no CMR API or protocol specification that I could find, and stability of `/cmr` REST paths is unknown.
- Whether a device can be managed by CMR and CAPsMAN at once in a clean way was not established beyond the takeover warning.

## What can Hotspot and User Manager deliver for a guest portal and vouchers?

### Takeaway
A working captive portal with vouchers is possible on RouterOS, and the feature set is broad. The weak spots are HTTPS behaviour, IPv4-only operation, licence-capped sessions and limited payment options.

### Cited Findings
- Hotspot login methods are cookie, http-chap, http-pap, https, mac, mac-cookie and trial, with a local database or RADIUS — [P: HotSpot](https://manual.mikrotik.com/docs/authentication-authorization-accounting/hotspot-captive-portal/).
- Login pages are editable HTML. The walled garden supports hosts, paths, wildcards and regex — [P: HotSpot](https://manual.mikrotik.com/docs/authentication-authorization-accounting/hotspot-captive-portal/).
- Per-user rate limits create a simple queue. Byte limits, uptime limits and session timeouts exist — [P: HotSpot](https://manual.mikrotik.com/docs/authentication-authorization-accounting/hotspot-captive-portal/).
- It runs on Ethernet, wireless, VLAN and bridge interfaces, one server per interface. RFC 7710 captive portal detection is supported — [P: HotSpot](https://manual.mikrotik.com/docs/authentication-authorization-accounting/hotspot-captive-portal/).
- Limits: redirecting HTTPS can cause certificate errors, reliable operation needs IPv4, only the default routing table is used, and device-mode can block Hotspot — [P: HotSpot](https://manual.mikrotik.com/docs/authentication-authorization-accounting/hotspot-captive-portal/).
- FastTrack bypasses the Hotspot universal client — [P: Packet Flow](https://manual.mikrotik.com/docs/firewall-and-quality-of-service/packet-flow-in-routeros/).
- User Manager is a separate package on all architectures except SMIPS. Active sessions are limited by licence level, and multiple connections by one user count against that limit — [P, frozen page: User Manager](https://help.mikrotik.com/docs/spaces/ROS/pages/2555940/User%20Manager).
- `add-batch-users` creates accounts, and `generate-voucher` prints from an HTML template. Profiles set validity and price, and limitations cap transfer, uptime and rate — [P, frozen page: User Manager](https://help.mikrotik.com/docs/spaces/ROS/pages/2555940/User%20Manager).
- "Only PayPal is supported" for payment. A self-service portal lives at `/um/` — [P, frozen page: User Manager](https://help.mikrotik.com/docs/spaces/ROS/pages/2555940/User%20Manager).
- A forum reply claims level 4 allows 200 active Hotspot users, level 5 allows 500 and level 6 is unlimited. Unverified — [U: Hotspot and User Manager thread](https://forum.mikrotik.com/t/help-with-hotspot-radius-hotspot-user-management/74828).
- With the `wifi` package, access lists can query RADIUS by MAC and assign `vlan-id`, and rules can be limited by time — [P: Wi-Fi](https://manual.mikrotik.com/docs/wireless/wifi/).
- With `on-cap` forwarding, traffic stays on the CAP, and `wifi-qcom-ac` supports only local forwarding — [P: WiFi CAPsMAN](https://manual.mikrotik.com/docs/wireless/wifi/capsman/).
- UniFi has an External Hotspot API for authorising clients — [P*: UniFi help](https://help.ui.com/hc/en-us/articles/31228198640023-External-Hotspot-API-for-Authorization-Clients).

### Inferences
- The clean design with `wifi` CAPs is a guest VLAN carried back to the gateway, with Hotspot on that VLAN interface. The Hotspot runs at the gateway rather than at the AP.
- A third-party app can run its own voucher store and act as, or feed, RADIUS. That avoids User Manager's PayPal limit and its package requirement.

### Gaps
- No primary source was found for licence-level session numbers. The RouterOS Licensing page was not read.
- No source specifically confirmed Hotspot behaviour with `wifi` package CAPs. The design above is inference.
- UniFi's guest portal feature list (payments, social login) was not retrieved, so parity is not assessed.

## How do the other UniFi features map?

### Takeaway
Fast roaming and remote access are solid. Zone firewall can be delivered as a UI abstraction over interface lists. Band steering, RF scanning and speed test are weaker versions of the UniFi feature. Notifications work, but phone push needs your own service.

### Cited Findings
**Zone-based firewall**
- UniFi groups interfaces into zones, with policies matching Device, Network, IP or MAC, Port, and App, Domain or Region. It arrived in UniFi Network 9.0 — [P*: UniFi ZBF](https://help.ui.com/hc/en-us/articles/115003173168-Zone-Based-Firewalls-in-UniFi); [P*: Migrating](https://help.ui.com/hc/en-us/articles/28223082254743-Migrating-to-Zone-Based-Firewalls-in-UniFi).
- The RouterOS firewall page does not mention zones — [P: Firewall](https://manual.mikrotik.com/docs/firewall-and-quality-of-service/firewall/).
- Forum users treat interface lists as zones, and the default config has used LAN and WAN lists since 6.40. These posts are old — [U: Zone Based Firewall](https://forum.mikrotik.com/viewtopic.php?t=133830); [U: Interface / List](https://forum.mikrotik.com/viewtopic.php?t=129680).

**Band steering and fast roaming**
- `2g-probe-delay` (default no) skips some early 2.4 GHz probe responses for strong clients when a 5 GHz AP is in the neighbour group. This is the nearest thing to band steering on the page — [P: Wi-Fi](https://manual.mikrotik.com/docs/wireless/wifi/).
- `rrm` (802.11k) and `wnm` (802.11v) default to yes. Unsolicited 802.11v requests arrived in 7.21 and trigger below `transition-threshold`. Clients may ignore them — [P: Wi-Fi](https://manual.mikrotik.com/docs/wireless/wifi/).
- `ft` (802.11r) defaults to no. Both APs must be managed by the same RouterOS instance, and it does not work over OWE — [P: Wi-Fi](https://manual.mikrotik.com/docs/wireless/wifi/).
- Legacy `wireless` conflicts with `wifi-qcom` and `wifi-qcom-ac` — [P*: Wi-Fi 5](https://manual.mikrotik.com/docs/wireless/wifi-ac/).
- Android may struggle to roam under WPA3 in one report — [U: 802.11r/k/v thread](https://forum.mikrotik.com/t/finally-success-802-11r-k-v-fast-roaming-works-reliably-with-wifiwave2/169733).

**RF scans**
- `frequency-scan` reports per-channel networks, load, noise floor and signal, but running it disconnects clients — [P: Wi-Fi](https://manual.mikrotik.com/docs/wireless/wifi/).
- `scan` lists nearby APs. Spectral scan is `wifi-qcom` only and classifies interference such as microwave ovens and Bluetooth — [P: Wi-Fi](https://manual.mikrotik.com/docs/wireless/wifi/).

**Speed test**
- `/tool/speed-test` measures between two MikroTik routers. Device mode must allow bandwidth-test on both, and most routers delivered with 7.17 or later have it off — [P*: Speed Test](https://manual.mikrotik.com/docs/diagnostics-monitoring-and-troubleshooting/speed-test/).
- CPU load can limit results, and the tool warns when this happens — [P*: Speed Test](https://manual.mikrotik.com/docs/diagnostics-monitoring-and-troubleshooting/speed-test/).
- Users run the Ookla CLI in a RouterOS container as a workaround — [U: Ookla container](https://forum.mikrotik.com/t/ookla-speedtest-container/163953).

**Remote access**
- Back To Home needs 7.12 or later on ARM, ARM64 or TILE. The relay is used without a public address and cannot decrypt the tunnel — [P*: Back To Home](https://manual.mikrotik.com/docs/network-management/cloud/back-to-home/).
- MIPS routers such as RB750Gr3 are reported unsupported — [U: BTH supported router](https://forum.mikrotik.com/t/back-to-home-supported-router/171813).
- A 2023 beta announcement said there were no relay limits at that time, which may have changed — [U: BTH beta thread](https://forum.mikrotik.com/viewtopic.php?t=197745).

**Notifications and mobile**
- CMR alert rules run log, script or HTTP webhook actions, and cover state and event alerts — [P: CMR guide](https://manual.mikrotik.com/docs/management-tools/cmr/).
- MikroTik has its own mobile app and a Back To Home app. Their feature sets were not reviewed — [P*: MikroTik app listing](https://apps.apple.com/app/id1323064830); [P*: mikrotik.com/bth](https://mikrotik.com/bth).

### Inferences
- A zone editor can compile to interface lists and ordered filter rules. It works only if the app owns the rule set, so hand-edited firewalls need care. App and Region criteria will not carry over directly.
- RF scans must be scheduled and warned about in the UI, since they drop clients.
- WireGuard gives a cloud-free path to a self-hosted UI, but the operator supplies the reachable endpoint. Back To Home can cover the no-public-IP case on supported CPUs.
- Phone push needs a relay service using Apple and Google push, so a truly cloud-free app cannot deliver it. A progressive web app or self-hosted ntfy-style service would be alternatives.

### Gaps
- UniFi's RF scan and band steering behaviour was not retrieved from help.ui.com, so the comparison is against features as named.
- RouterOS IDS/IPS was not specifically searched. No such feature appeared in the pages read.
- My recollection is that changing device-mode needs physical confirmation (unverified). If true, the app cannot enable Hotspot or bandwidth test remotely.

## Is it lawful to call a product "UniFi-inspired" (trademark, US and EU)?

### Takeaway
Not legal advice. In the US a truthful, plain-text reference is likely defensible if it passes the nominative fair use factors. The EU is riskier than the brief suggests: the sharper problem is not Article 14 but the comparative advertising ban on presenting goods as "imitations or replicas". The safest position is to keep "UniFi" out of the name, logo and tagline, and to make only objective factual comparisons.

### Cited Findings
**US**
- The *New Kids* test asks whether the product is identifiable without the mark, whether more of the mark was used than needed, and whether sponsorship was suggested — [S: Klemchuk](https://klemchuk.com/ideate/nominative-fair-use); [S: The Fashion Law](https://www.thefashionlaw.com/new-kids-on-the-block-v-new-america-publishing-inc/).
- *Toyota v. Tabari*, 610 F.3d 1171 (9th Cir. 2010), applied the test to domain names. A failing use may be modified, but nominative use may not be barred altogether — [S: Justia entry](https://law.justia.com/cases/federal/appellate-courts/ca9/07-55344/07-55344-2011-02-25.html); [S: Eric Goldman](https://blog.ericgoldman.org/archives/2010/07/funky_ninth_cir.htm).
- Circuits differ on the test and on who bears the burden — [S: Arnold & Porter](https://www.arnoldporter.com/en/perspectives/publications/2016/11/2016_11_16_second_circuit_expands_split__13324).

**EU**
- Article 14(1)(c) of Regulation 2017/1001 covers use "for the purpose of identifying or referring to" goods as those of the proprietor. The 2015 reform removed "where it is necessary" — [S: Maastricht University](https://www.maastrichtuniversity.nl/blog/2021/01/name-dropping-marketing); [S: Athens course notes](https://eclass-b.uoa.gr/modules/document/file.php/LAW263/4.%20LIMITATION%20OF%20TRADEMARK%20RIGHTS.pdf).
- Article 14(2) applies it only "in accordance with honest practices in industrial or commercial matters". The parallel Directive text is at [P*: Directive 2015/2436 Art. 14](https://legislation.gov.uk/eudr/2015/2436/article/14/data.pdf).
- *Gillette* (C-228/03) and *BMW v Deenik* (C-63/97) are the honest-practice references. A use suggesting a commercial link is not honest — [S: CMS](https://cms.law/en/gbr/legal-updates/use-of-third-party-trade-marks-permissible-provided-use-is-honest).
- Directive 2006/114/EC Article 4 permits comparative advertising only if, among other things, it objectively compares verifiable features (c), does not take unfair advantage of a mark's reputation (f), and "does not present goods or services as imitations or replicas" of goods bearing a protected mark (g) — [P: Directive 2006/114 Art. 4](https://www.legislation.gov.uk/eudr/2006/114/article/4).
- *L'Oréal v Bellure* (C-487/07, 18 June 2009): presenting a product "expressly or by implication" as an imitation or replica is unlawful, and the advantage gained is unfair. No confusion is needed — [S: Dorsey](https://www.dorsey.com/newsresources/publications/2010/05/the-point-is-not-to-compare-like-with-like); [S: Agence Europe](https://agenceurope.eu/en/bulletin/article/9924/34/imitations-infringe-intellectual-property-rights-even-where-they-do-not-create-confusion-in-the-minds-of-consumers). The ruling is criticised — [S: Mondaq](https://mondaq.com/uk/trademark/83952/´limitations-on-imitations-no-more-free-rides´-the-ecj´s-recent-decision-on-comparative-advertising).
- Those sources cite Directive 84/450 Article 3a(1)(g) and (h), which I read as the predecessor of Article 4(f) and (g) in Directive 2006/114. The renumbering is not confirmed by the sources read.

**Ubiquiti's own position**
- Ubiquiti's ToS says marks are its exclusive property and need prior written consent — [P: ui.com legal](https://www.ui.com/legal/). Product documents state that Ubiquiti, the U logo and UniFi are its trademarks — [P*: dl.ui.com QIG](https://dl.ui.com/qig/uc-cast-pro).
- A 2015 copy of Ubiquiti's trademark guidelines bars placing its marks beside products it did not make. It may be out of date, and no current brand page was found — [S: copy on crz.gov.sk](https://www.crz.gov.sk//data/att/4144748.pdf).
- UNIFI is a US registration held by Ubiquiti — [S: USPTO record mirror](https://uspto.report/TM/90543739).

### Inferences
- Article 14(1)(c) fits poorly. *Gillette* and *Deenik* concern accessories for, or services on, the owner's own goods. This product does not work with UniFi hardware, so "UniFi-inspired" refers to a rival rather than to compatibility. That makes the comparative advertising rules the more relevant test.
- "Inspired by" is not the same word as "imitation". I found no case on the phrase, and a court could go either way. But if the marketing says or implies the UI is a UniFi look-alike, it moves towards *L'Oréal*, and a close visual match would strengthen that reading.
- Whether a free, non-commercial hobby project counts as "advertising" at all is unclear. My recollection is that the Directive requires a trade, business, craft or profession (unverified). Taking donations or selling a paid tier would weaken that argument.
- Lower-risk wording: "A self-hosted management interface for MikroTik RouterOS. Not affiliated with Ubiquiti or MikroTik." A comparison page may list objective features, with a plain-text mark and no logos. "Works with RouterOS" is a compatibility statement that fits the referential use logic far better.

### Gaps
- No EU case on "inspired by" software marketing was found.
- *Audi v GQ* (C-334/22) and *Inditex v Buongiorno* (C-361/22) appear relevant to Article 14(1)(c), and the latter's search summary looked unreliable. The Bird & Bird post returned 402, so neither was verified.
- EUIPO registrations for UNIFI were not checked. Check the EU registry and national marks, and run a clearance search on the project's own name.

## Can the UI's look and feel be protected (copyright, trade dress, designs)?

### Takeaway
Not legal advice. Functions, workflows and general layouts are largely free to copy in both the US and EU. Specific visual expression is not: icons, illustrations, exact screen compositions and CSS are the danger. One correction to the brief: *BSA* did not hold GUIs unprotected. EU design law, which the brief did not list, may be the most direct risk for close copying.

### Cited Findings
**EU copyright**
- *BSA v Ministry of Culture* (C-393/09, 22 December 2010): a GUI is not a form of expression of a computer program under the Software Directive, but it can be protected by copyright under the InfoSoc Directive as the author's own intellectual creation — [S: Fieldfisher](https://www.fieldfisher.com/en/insights/graphical-user-interfaces-can-be-protected-by-copyright-rules-european-court); [S: Kluwer Copyright Blog](https://legalblogs.wolterskluwer.com/copyright-blog/protection-of-guis-graphical-user-interfaces-some-comments-about-the-ecj-s-preliminary-ruling-in-bsa-v-ministervo-kultury/).
- *SAS Institute v World Programming* (C-406/10, Grand Chamber, 2 May 2012): copyright does not extend to software functionality, programming languages or data file formats. WPL reproduced functionality by observing, studying and testing the program — [S: Fieldfisher](https://www.fieldfisher.com/en/insights/sas-institute-inc-v-world-programming-limited); [S: European Law Blog](https://www.europeanlawblog.eu/pub/c-40610-sas-institute-v-world-programming-ltd-wpl); [S: SCL](https://www.scl.org/2436-sas-v-wpl-no-copyright-says-ecj/).

**EU design law**
- Regulation (EU) 2024/2822 extends "product" to graphical user interfaces and allows dynamic and animated representations — [S: Dreyfus](https://www.dreyfus.fr/en/2026/01/05/eu-design-protection-complete-guide-to-the-2025-reform/); [S: Herbert Smith Freehills Kramer](https://www.hsfkramer.com/notes/ip/2025-05/eu-design-reform-begins-key-provisions-of-eu-regulation-2024-2822-effective-from-1-may).
- Unregistered EU designs last three years from first disclosure, and registered designs last up to 25 years — [S: Dreyfus](https://www.dreyfus.fr/en/2026/01/05/eu-design-protection-complete-guide-to-the-2025-reform/).

**US copyright**
- *Lotus v. Borland*: the appellate court rejected Lotus's look-and-feel claims, and the Supreme Court took the case at 516 U.S. 233 — [S: Hsu, MIT](https://groups.csail.mit.edu/mac/classes/6.805/student-papers/fall94-papers/hsu-copyright.html); [S: Look and feel](https://en.wikipedia.org/wiki/Look_and_feel).
- *Apple v. Microsoft*, 35 F.3d 1435 (9th Cir. 1994): Apple argued the Macintosh look and feel as a whole was protected, and the suit failed — [S: Richmond JOLT](https://law.richmond.edu/jolt/v1i1/myers_related.html); [S: Seattle Times](https://archive.seattletimes.com/archive/19930602/1704430/apple-microsoft-lawsuit-fizzles-to-a-close).
- *Google v. Oracle* is discussed in commentary as resolving the API dispute in Google's favour — [S: Software Freedom Law Center](https://www.softwarefreedom.org/blog/2022/jan/18/google-oracle-endera/).

**US trade dress**
- *Wal-Mart v. Samara* (2000): unregistered product-design trade dress needs secondary meaning. *TrafFix* made proving non-functionality harder — [S: Banner Witcoff](https://bannerwitcoff.com/media/_docs/library/articles/TradeDressCases.pdf); [S: FindLaw](https://corporate.findlaw.com/intellectual-property/supreme-court-cuts-back-on-trademark-rights-in-traffix.html).
- GUIs are hard to protect as trade dress because they may be seen as functional and may not gain secondary meaning. This is a 2002 view — [S: WilmerHale](https://www.wilmerhale.com/en/sitecore/content/whlaunch/home/research/news-publications/protecting-a-companys-graphical-user-interface-may-6-2002).
- One reported decision rejected trade dress for an online software tool's look and feel as functional. The case name was not shown — [S: Internet Cases](https://internetcases.com/?p=6416).

**Contract**
- Ubiquiti's EULA treats "all user interfaces" as protected Content, bars derivative works and reverse engineering, and applies New York law — [P: Ubiquiti EULA](https://www.ui.com/eula/).

### Inferences
- Reproducing what UniFi does, such as a client list, topology map or dashboard pattern, should be low risk. Reproducing how it looks, screen by screen, is where risk accumulates, particularly under EU design law and copyright combined with the *L'Oréal* point above.
- Practical rules: use your own icons, colours, typography, layout grid and illustrations. Do not copy any Ubiquiti assets, CSS or text. Do not pull anything from UniFi software, because anyone who accepted the EULA may also face contract claims. Keep a short record of independent design decisions.
- Unregistered design rights in older UniFi screens may have lapsed after three years, but newer screens could still be covered. Registered designs could last far longer.

### Gaps
- **No primary judgment was read.** Fetches of *BSA*, *SAS*, *Lotus*, *Apple* and the *Google v. Oracle* slip opinion all failed or could not be rendered.
- Unverified recollections to check: *BSA* says expression dictated by technical function lacks originality. *Lotus* held menu commands a "method of operation" and was affirmed by an equally divided Supreme Court. *Apple* applied thin protection requiring virtual identity. *Google* assumed copyrightability and decided on fair use. Article 5(3) and Article 8 of the Software Directive stop contracts overriding the right to observe and study a program.
- Ubiquiti's registered EU designs and US design patents for UI were **not searched**. This is a real gap and needs a EUIPO and USPTO search.
- The exact text of Regulation 2024/2822 was not read. Its phases and transitional rules rest on law firm summaries.
- No recent appellate survey on app look-and-feel trade dress was found. EU unfair competition law on slavish imitation varies by member state and was not researched.

## Does Ubiquiti have a record of enforcing against third parties?

### Takeaway
Yes. Ubiquiti litigates and files trademark actions, though with mixed results. No action against a look-alike UI or a third-party controller was found, which is only weak comfort because the search was limited. Its GPL history is a credibility issue for Ubiquiti, not a defence for anyone else.

### Cited Findings
- *Ubiquiti v. Cambium* (filed 2018): a 15-count suit including DMCA, RICO, CFAA and copyright claims, over firmware that ran on Ubiquiti hardware — [S: Heather Meeker](https://heathermeeker.com/2019/05/28/dont-try-to-fool-a-fula-ubiquitis-complaint-is-dismissed/).
- The court dismissed the complaint in 2019, finding it unclear what the firmware claims covered, given open-source licensing — [S: Heather Meeker](https://heathermeeker.com/2019/05/28/dont-try-to-fool-a-fula-ubiquitis-complaint-is-dismissed/); [S: Studicata](https://www.studicata.com/summaries/united-states-district-court-northern-district-of-illinois/ubiquiti-networks-inc-v-cambium-networks-inc-2019-dwggg1/).
- Settled December 2020. Cambium agreed to stop selling Elevate for Ubiquiti devices, with no admission of liability. Payment undisclosed — [S: Channel Futures](https://channelfutures.com/regulation-compliance/cambium-networks-settles-federal-lawsuit-by-rival-ubiquiti).
- Software Freedom Conservancy found that neither company complied properly with the GPL, received "no source code at all to review" from either, and opened non-litigation enforcement against both — [S: SFC](https://sfconservancy.org/blog/2019/oct/02/cambium-ubiquiti-gpl-violations/).
- TTAB records show a Ubiquiti proceeding against Unifi Communications, listed as terminated, with outcome unknown — [S: Law360](https://www.law360.com/ttab_cases/565f09742e3b9dfdd9000d59).
- Ubiquiti filed a TTAB opposition on 2025-08-13 relying on its EDGE marks — [P*: TTAB filing](https://ttabvue.uspto.gov/ttabvue/ttabvue-91300988-OPP-1.pdf).
- It lost a domain dispute over `ubiquiti-networks.com` on 2021-12-09, because bad faith was not proved — [P: Forum decision](https://www.adrforum.com/domaindecisions/1970506.htm).
- It sued Brian Krebs for defamation in 2022. Seen only through a forum listing of a Register article — [S: The Register forums](https://forums.theregister.com/forum/all/2022/03/30/ubiquiti_brian_krebs/).

### Inferences
- The most useful reading is that Ubiquiti defends its marks and pursues rivals that touch its hardware or firmware. A MikroTik-only UI touches neither. The exposure would be a name, logo or close visual copy, and a takedown or letter is a more likely first step than a lawsuit.
- Open-source projects are cheap to pressure with platform takedowns, so a project should be able to rebrand or restyle quickly.

### Gaps
- GitHub DMCA notices and app store complaints involving Ubiquiti were not searched directly. Absence here is not proof of absence.
- No EU litigation by Ubiquiti was found, and EU court databases were not searched.
- The outcome of the Unifi Communications proceeding was not established.

## Does MikroTik have naming or trademark rules for third-party tools?

### Takeaway
Yes, brand guidelines exist, and they are strict on logos and implied endorsement. There is no published rule found on third-party product names. Plain-text "for RouterOS" statements look safe, and a logo needs a licence.

### Cited Findings
- Brand assets are "proprietary assets owned exclusively by MikroTik". "Many uses—including our logos, app and product icons, and other designs - will require a license first" — [P: mikrotik.com/logo](https://mikrotik.com/logo).
- Materials must not suggest MikroTik sponsors or endorses them — [P: mikrotik.com/logo](https://mikrotik.com/logo).
- The page gives correct spellings such as MikroTik, RouterOS, WinBox and WebFig, and shows "Made for MikroTik" and "Powered by MikroTik" tagline logos. Its text has no separate third-party rules. Contact is marketing@mikrotik.com — [P: mikrotik.com/logo](https://mikrotik.com/logo).
- MikroTik's software listing says listed tools are third-party, unsupported and used at the user's own risk — [P*: mikrotik.com/mfm/software](https://mikrotik.com/mfm/software).
- The MFM programme is described for those building products around RouterBOARD and RouterOS. Its terms were not seen — [P*: mikrotik.com/mfm](https://mikrotik.com/mfm).
- Third-party tools already use similar names, such as MKController and "MikroTik Manager" — [S: mkcontroller.com](https://mkcontroller.com/docs/voucher/voucher-start/); [S: mikr.app](https://mikr.app/). Their permission status is unknown, so this shows tolerance at most.
- The current status of a "RouterOS" registration was unclear in a 2015 discussion, and a list of Mikrotikls SIA marks exists — [S: Justia owner page](https://trademarks.justia.com/owners/mikrotikls-sia-1398389); [S: NANOG thread](https://seclists.org/nanog/2015/Dec/600).

### Inferences
- "Works with MikroTik RouterOS" in plain text with a non-affiliation note fits the same referential logic as *Gillette*, because the product does work with those goods.
- A name built on a fragment such as "Tik" is not addressed by any rule found. It carries more risk than a neutral name, though less than using the full mark. A short email to MikroTik's brand team would settle it cheaply.
- MikroTik's licence terms matter before shipping anything based on reverse-engineered CMR.

### Gaps
- MikroTik's current EULA terms on reverse engineering were not read. Copies exist on third-party mirrors only in the results.
- EU interoperability decompilation rights under Article 6 of the Software Directive were not researched.
- Current RouterOS and MikroTik registration status in the EU was not checked.
