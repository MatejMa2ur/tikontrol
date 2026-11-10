# Feature & page tracker

This is the single source of truth for **what UniFi Network has** and **where tikontrol stands** on each item.
Update the Status column whenever a feature changes state. The progress numbers at the top are recounted by
`src/scripts/progress.py` (run it after editing).

Reference: UniFi Network Application 10.x (latest 10.6.109, Oct 2026), taken from public docs and release notes.
Clean-room: we copy the behaviour, never the assets.

**Status:** ⬜ todo · 🎨 mockup (UI only, fake/snapshot data) · 🟨 in progress · ✅ done (real data, tested) · ➖ won't do
**Hardware flag:** 🔌 = built and unit-tested, but needs a test on real equipment (MikroTik hardware, a Wi-Fi AP, a physical port, a real ISP line or a real outside service). The text after 🔌 says what to check; all of them are listed in [HARDWARE_TESTS.md](HARDWARE_TESTS.md).
**Backing:** `ROS` = RouterOS API on the router · `CMR` = MikroTik CMR server · `tik` = our own logic/DB · `✗` = nothing can back it

<!-- progress:start -->
**Progress:** 120 ✅ · 105 🟨 · 0 🎨 · 1 ⬜ · 9 ➖ (235 items, 53% done) · 106 🔌 waiting for a hardware test ([list](HARDWARE_TESTS.md))
<!-- progress:end -->

---

## Pages (navigation)

| # | UniFi page | tikontrol page | Phase | Status |
|---|---|---|---|---|
| P1 | Dashboard | Dashboard | 1 | ✅ |
| P2 | UniFi Devices (list) | Devices | 3 | ✅ |
| P3 | Topology 🔌 a real downstream MikroTik switch and AP heard over MNDP/LLDP | Topology (tab in Devices) | 2 | 🟨 |
| P4 | Device detail (overview / ports / insights / settings) | Device drawer | 3 | ✅ |
| P5 | Client Devices (list) | Clients | 1 | ✅ |
| P6 | Client detail (overview / insights / settings) | Client drawer | 1 | ✅ |
| P7 | Ports (port manager) | Ports | 2 | ✅ |
| P8 | Radios (Radio Manager, coverage, connectivity; needs hardware (Wi-Fi AP)) 🔌 a real Wi-Fi AP: radios, monitor, scan, coverage | Radios | 3 | 🟨 |
| P9 | Insights / Traffic flows | Traffic | 3 | ✅ |
| P10 | Alarm Manager | Alerts | 2 | ✅ |
| P11 | System Log (events, admin activity, security, updates) | Events | 1 | ✅ |
| P12 | Hotspot Manager (loads the router's guest hotspot, vouchers, branded login page) 🔌 run the guest portal checks below on a CHR with a client | Guest portal | 4 | 🟨 |
| P13 | Settings → WiFi 🔌 a real AP on both the `wireless` and `wifi` packages: create, edit and remove SSIDs | Settings · WiFi | 2 | 🟨 |
| P14 | Settings → Networks (every control sends an intent: DHCP range/lease, options, mDNS, IGMP, VLAN filtering, remove) 🔌 toggle IGMP and VLAN filtering on a CHR and let the 5-minute revert undo a lockout | Settings · Networks | 3 | 🟨 |
| P15 | Settings → Internet 🔌 a real ISP line: DHCP/PPPoE/static, tagged WAN VLAN, failover, prefix delegation | Settings · Internet | 3 | 🟨 |
| P16 | Settings → VPN 🔌 connect a phone over a real WAN with WireGuard and L2TP/IPsec | Settings · VPN | 2 | 🟨 |
| P17 | Settings → Security / CyberSecure (all sections on real data and intents) 🔌 run the security checks below on a CHR | Settings · Security | 3 | 🟨 |
| P18 | Settings → Routing (add and remove port forwards, routes, DNS records) 🔌 add, remove and revert each on a CHR; hand-made rows must have no Remove | Settings · Routing | 2 | 🟨 |
| P19 | Settings → Profiles (real speed profiles, QoS rules and RADIUS servers, with remove) 🔌 remove a speed profile, a QoS rule and a tagged RADIUS server on a CHR | Settings · Profiles | 3 | 🟨 |
| P20 | Settings → Control Plane (admins, backups, updates, integrations, console) 🔌 backups, update window, console and API keys on a CHR | Settings · System | 2 | 🟨 |
| P21 | Settings → System (site, logging, SNMP, device auth) 🔌 site settings, remote syslog, SNMP and service hardening on a CHR | Settings · System | 2 | 🟨 |
| P22 | Site Manager (multi-site, cloud) | Sites switcher (local routers only; no cloud: tikontrol never phones home) | 4 | ✅ |
| P23 | Login / onboarding wizard | Login + first-run wizard | 1 | ✅ |
| P24 | Global search (⌘K): clients, devices, SSIDs, settings pages | Command palette | 2 | ✅ |

---

## Dashboard
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Status-first summary with insights (insecure services, double NAT, low RAM, band flapping, pending firmware) | tik on ROS data | 1 | ✅ |
| Internet health (WAN up, latency, packet loss) | ROS netwatch + tik | 1 | ✅ |
| WAN live throughput (down/up) graph | ROS monitor-traffic | 1 | ✅ |
| Max download/upload speed indicators | tik | 1 | ✅ |
| ISP name, public IP, WAN IP (needs a real ISP WAN: the CHR sits behind QEMU NAT) 🔌 a real ISP WAN: public IP and WAN IP shown; the ISP name has no source without an outside lookup, so it is not shown | ROS `/ip/cloud`, dhcp-client | 1 | 🟨 |
| Client count (wired / wireless / guest; guests from hotspot users, guest Wi-Fi interfaces and guest subnets) 🔌 a real guest SSID and hotspot: guests counted, main-LAN clients not | ROS | 1 | 🟨 |
| Gateway resources (CPU, RAM, temp, uptime) (CPU/RAM/uptime verified on CHR; temperature needs a sensor) 🔌 temperature on a board with a `/system/health` sensor | ROS `/system/resource`, health | 1 | 🟨 |
| Top clients by traffic (Wi-Fi from the registration table, wired from kid-control's device table) 🔌 check that `/ip/kid-control/device` counts wired clients on a real router (it may need a kid-control profile) | ROS kid-control + tik | 1 | 🟨 |
| Top applications | CMR apptraffic | 3 | ✅ |
| Traffic Identification card (by client now, by app once CMR is wired) 🔌 by-app view with a real CMR server | ROS reg-table bytes · CMR | 1 | 🟨 |
| WiFi Technology card (clients by standard + band, activity, experience) 🔌 Wi-Fi clients on a real AP (standards and bands) | ROS reg-table | 1 | 🟨 |
| Most active clients / access points strips 🔌 real Wi-Fi clients and traffic | ROS + tik | 1 | 🟨 |
| Internet Activity chart: client count cells + down/up, 1H/1D/1W | tik history | 2 | ✅ |
| Gateway column (WAN/gateway IP, uptime from stored history, latency to 3 resolvers, channel widths) 🔌 real WAN latency and Wi-Fi channel widths | ROS | 1 | 🟨 |
| WiFi experience / satisfaction score 🔌 scores match how real Wi-Fi clients behave | tik (signal + rate heuristic) | 2 | 🟨 |
| Recent critical events 🔌 real log errors and a WAN flap show up | ROS log + tik | 1 | 🟨 |
| Firmware update available banner 🔌 a router with a real pending RouterOS/RouterBOOT update | ROS / CMR | 2 | 🟨 |
| Speed test (run + history) 🔌 run against Cloudflare on a real WAN; results near the line speed | tik server (HTTPS, Cloudflare by default, opt-in) | 3 | 🟨 |
| Customisable widgets / layout (reorder, hide/show, normal or full width per card; stored on the server and checked in a browser) | tik | 4 | ✅ |

## UniFi Devices / Topology
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Device list (name, model, IP, version, uptime, state; other devices' stats need the CMR server) (this router and CMR-managed devices verified on CHR) | ROS this router + `/ip/neighbor` · CMR `/cmr/device` | 3 | ✅ |
| Device adoption (one-click; needs CMR server) | CMR pairing | 3 | ✅ |
| Auto-detect make, model, CPU, memory and WiFi capabilities | ROS `/system/resource`, `/system/routerboard` | 1 | ✅ |
| Device drawing: our own SVG of the port side per detected model (case colour, ports, USB, live link LEDs; generic box for unknown models) | tik (`hardware.ts` faceOf) | 2 | ✅ |
| Device detail: CPU/RAM/temperature history (this router; temperature only where `/system/health` has a sensor) 🔌 temperature history on a board with a sensor | tik history · CMR dashboard for others | 3 | 🟨 |
| Device restart / locate (blink LED) 🔌 the LED blinks on a real RouterBOARD; restart through CMR | CMR reboot · ROS `/system/routerboard` LED | 3 | 🟨 |
| Firmware update per device 🔌 CMR upgrade of a real managed device | CMR / ROS | 3 | 🟨 |
| Scheduled / rolling fleet updates (needs CMR server) | CMR upgrade rules | 3 | ✅ |
| Topology map (auto layout, wired + wireless links; MNDP/LLDP neighbours as intermediate switch/AP nodes with their port's clients) 🔌 check with a real switch and AP behind the router, including a daisy chain | CMR layout · ROS neighbor + bridge host | 2 | 🟨 |
| Topology Spotlight (highlight/filter) 🔌 check on a real multi-device topology | tik | 3 | 🟨 |
| Device Supervisor (health anomalies; needs CMR server) | CMR state alerts | 4 | ✅ |
| Blueprints / config drift inspector (diff of the reader's structured prints, not `/export`: over the API with only `read` it returns an empty `!done`, and exports risk secrets) | tik (snapshot diff) | 4 | ✅ |
| Device SSH credentials management (public keys only; writer needs the `policy` policy) | ROS `/user/ssh-keys` | 3 | ✅ |
| Device LED control / Etherlighting 🔌 `set-leds` turns the LEDs off on a real board | ROS LED · Etherlighting ✗ (UI hw) | 4 | 🟨 |
| Shadow mode / gateway HA | ✗ (VRRP is possible but out of scope) | – | ➖ |

## Client Devices
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Client list: name, IP, MAC, vendor, connection, uptime (leases + ARP verified on CHR; wireless rows need Wi-Fi hardware) 🔌 wireless rows on a real AP | ROS leases + ARP + reg-table + bridge host | 1 | 🟨 |
| Device fingerprint / auto icon (phone, TV, PC…): maker from the MAC (embedded IEEE table, private-address note), type + OS guess from hostname, DHCP vendor class and MNDP/LLDP neighbours, first/last seen | tik (OUI + hostname + DHCP class-id) · ROS `/ip/neighbor` | 1 | ✅ |
| Active LAN discovery, opt-in: mDNS/DNS-SD + SSDP/UPnP names and models ("What the device says"), improves maker and type 🔌 mDNS/SSDP answers from real LAN devices (TV, printer, speaker) | tik (server on the LAN, link-local multicast only) | 2 | 🟨 |
| Wired: switch port · Wireless: SSID, band, signal, rate 🔌 physical ports with clients and a real AP | ROS bridge host + reg-table | 1 | 🟨 |
| Live per-client throughput (wired via kid-control device counters) 🔌 compare a wired client's rate with `/interface/monitor-traffic` on its port | ROS kid-control / reg-table bytes | 1 | 🟨 |
| Per-client traffic history (hourly bytes, wired and Wi-Fi) 🔌 a day of history on a real router with wired and Wi-Fi clients | tik SQLite | 2 | 🟨 |
| Per-client apps / domains (flow destinations per client, named from the DNS cache; the cache itself has no client field) | tik flows + DNS cache (CMR is per-device) 🔌 real traffic-flow export plus the DNS cache | 3 | 🟨 |
| Rename / alias, custom icon, notes (stored on the server, checked end to end in a browser: `src/scripts/ui-e2e.mjs`) | tik | 1 | ✅ |
| Fixed IP (reservation) | ROS lease make-static | 2 | ✅ |
| Local DNS record for client | ROS `/ip/dns/static` | 2 | ✅ |
| Block / unblock | ROS address-list + filter | 2 | ✅ |
| Pause internet / schedules | ROS kid-control | 2 | ✅ |
| Speed limit (client groups) | ROS simple queues | 2 | ✅ |
| "Why is this client on 2.4 GHz?": reasons, band history from the log, Safe apply fixes (channel 36, 40 MHz, own 5 GHz SSID, signal-range access list) 🔌 a dual-band AP with a client that hops bands | tik from log + monitor · ROS wireless/wifi | 3 | 🟨 |
| Kick / reconnect wireless client 🔌 kick a client on a real AP with each Wi-Fi package | ROS reg-table remove | 2 | 🟨 |
| Override network/VLAN for client (client drawer: set/remove; needs hardware (Wi-Fi AP), CHR refusal tested) 🔌 a client lands in the chosen VLAN on a real AP | ROS (MAC-based VLAN / access list) | 4 | 🟨 |
| Offline / historical clients list | tik | 2 | ✅ |
| Roaming history (which AP, when) 🔌 several APs and a client walking between them | tik (reg-table changes) · CMR wifi-logs | 3 | 🟨 |
| Filters (wired / wireless / guest / offline) and search | tik | 1 | ✅ |
| Client groups (deleting one offers to remove its address list, mangle and queue objects in one batch; leftovers listed) 🔌 apply members and a limit, then delete with "Remove from router" and revert | tik | 3 | 🟨 |

## Ports
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Port grid with link state, speed, duplex (link state verified on CHR; speed/duplex need a physical port (virtio reports none)) | ROS `/interface/ethernet` monitor 🔌 speed and duplex on physical ports | 2 | 🟨 |
| Which client is on which port 🔌 physical switch ports with clients | ROS bridge host | 2 | 🟨 |
| Per-port traffic / errors | ROS | 2 | ✅ |
| PoE status / power cycle (status, auto/off/forced per port, 5 s power-cycle) 🔌 power-cycle a powered device too; a PoE-out board (hEX PoE, CRS…P): print/monitor field names, set-poe and its revert; a board without PoE only logs a `poe` error | ROS `/interface/ethernet/poe` (hw dependent) | 3 | 🟨 |
| Port profile (VLAN access/trunk, native VLAN) | ROS bridge VLAN · CMR vlan | 3 | ✅ |
| Port disable / enable | ROS | 2 | ✅ |
| Port mirroring | ROS switch mirror 🔌 a switch chip that supports mirroring | 4 | 🟨 |
| Link aggregation | ROS bonding | 4 | ✅ |
| STP / loop protection settings | ROS bridge protocol-mode | 4 | ✅ |
| Storm control 🔌 a CRS3xx switch with storm-rate | ROS switch (hw dependent) | 4 | 🟨 |
| 802.1X port auth | ROS dot1x | 4 | ✅ |
| LLDP-MED / voice VLAN 🔌 an LLDP-MED phone gets the voice VLAN | ROS lldp | 4 | 🟨 |

## Radios / WiFi monitoring
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Radio list per AP: band, channel, width, tx power, clients (needs hardware (Wi-Fi AP)) 🔌 a real Wi-Fi AP | ROS wireless/wifi · CMR | 3 | 🟨 |
| Channel utilisation (channel, width, DFS, tx power, noise, CCQ; busy time not reported by RouterOS; needs hardware (Wi-Fi AP)) 🔌 monitor values from a real AP | ROS monitor (driver dependent) | 3 | 🟨 |
| Neighbouring networks / interference scan (one-shot, confirmed, admin + write access, max 10 s, disconnects the radio's clients; or copy the command; needs hardware (Wi-Fi AP)) 🔌 a confirmed scan on a real AP returns neighbours | ROS `scan` | 3 | 🟨 |
| Signal strength distribution (needs hardware (Wi-Fi AP)) 🔌 real Wi-Fi clients | tik from reg-table | 3 | 🟨 |
| WiFi connectivity success (association / auth / DHCP / DNS) 🔌 real associations and auth failures in the log | tik from log + leases | 2 | 🟨 |
| Radio link quality (CCQ buckets, channel, power, noise; needs hardware (Wi-Fi AP)) 🔌 CCQ and noise from a real AP | ROS wireless monitor | 2 | 🟨 |
| Roaming journeys (Connectivity view) 🔌 several APs, or CMR wifi-logs | tik · CMR wifi-logs | 3 | 🟨 |
| Coverage map / floorplan (Design Center): saved plan, drag/arrow-key AP pins, server-validated (PNG/JPEG/WebP, 200 KB, 32 APs); predicted Wi-Fi heatmap per band (2.4/5/6 GHz) from a log-distance model with drawn walls and materials, set scale, measured points and fit of the path-loss exponent; model and grid tested, checked end to end in a browser (`src/scripts/ui-e2e.mjs`) 🔌 calibrate on a real AP: measure 5-10 points per band, check wall losses and that linked tx power is real output | tik (floorplan + RF prediction, Web Worker) | 4 | ✅ |
| WiFiman (client-side speed/coverage app) | ✗ | – | ➖ |

## Insights / Traffic
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Traffic by application / category | CMR apptraffic | 3 | ✅ |
| Traffic by client over time (top clients per hour or day, 24 h / 7 d, stacked chart with a totals table) 🔌 a few hours on a real router: Wi-Fi and wired clients both appear, per-client sums match the client drawer, days group correctly across midnight | tik | 2 | 🟨 |
| Top domains | ROS DNS cache + tik | 3 | ✅ |
| Flow log (src, dst, port, bytes) | ROS traffic-flow (IPFIX) → tik collector | 3 | ✅ |
| Traffic by country (opt-in `TIK_GEOIP` local CSV, nothing downloaded) 🔌 real traffic-flow export with a `TIK_GEOIP` CSV | tik GeoIP on flows | 4 | 🟨 |
| Blocked threats view | ✗ (no IDS) | – | ➖ |

## Alarm Manager / Notifications
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Alert rules (device offline, WAN down, new client, CPU/RAM) 🔌 real WAN down, router unreachable and CPU/RAM thresholds fire and clear | tik · CMR alerts | 2 | 🟨 |
| Channels: email (SMTP, TLS/STARTTLS), push (ntfy), Telegram, webhook 🔌 send a test through each real service (an SMTP provider on 465 and 587, ntfy.sh, a Telegram bot, a Home Assistant webhook) | tik | 2 | 🟨 |
| Alert history / acknowledge | tik | 2 | ✅ |
| Quiet hours / per-rule severity 🔌 a real channel holds alerts back in quiet hours | tik | 3 | 🟨 |

## System Log / Events
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Live event stream | ROS `/log follow` | 1 | ✅ |
| Categories: clients, devices, admin activity, security, updates | tik (topic mapping) | 1 | ✅ |
| Derived events (client joined/left, WAN flap) (shown from the server; joins need Wi-Fi clients, a WAN flap a real WAN) 🔌 real Wi-Fi joins and a real WAN flap | tik | 2 | 🟨 |
| Search / filter / export CSV | tik | 2 | ✅ |
| Remote syslog / SIEM forwarding | ROS logging action | 3 | ✅ |

## WiFi settings
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Create/edit SSID (name, password, security WPA2/WPA3) 🔌 create and edit an SSID on both Wi-Fi packages; a phone joins with WPA2 and WPA3 | ROS wireless/wifi · CMR wifi | 2 | 🟨 |
| Band selection (2.4 / 5 / 6 GHz) 🔌 a real multi-band AP | ROS | 2 | 🟨 |
| Which APs broadcast it (label selection verified on CHR; broadcasting needs hardware AP) 🔌 real APs through CMR labels broadcast the SSID | CMR labels | 3 | 🟨 |
| Network/VLAN per SSID 🔌 a Wi-Fi client gets an address from the SSID's VLAN | ROS bridge port PVID | 3 | 🟨 |
| Guest network (isolation, portal, remove; guest SSID + portal in one change) 🔌 a phone joins the new guest SSID and sees the portal; on the wifi package check the virtual AP isn't bridged; create and remove a guest SSID on both Wi-Fi packages; guests isolated | ROS + hotspot | 3 | 🟨 |
| Client isolation 🔌 two clients on one SSID can't reach each other | ROS default-forwarding / wifi datapath | 2 | 🟨 |
| Hide SSID 🔌 the SSID disappears from a phone's list; joining by name still works | ROS | 2 | 🟨 |
| WiFi schedule (on/off by time, remove) 🔌 the radio turns off and on at the set times | ROS scheduler | 3 | 🟨 |
| Per-SSID speed limit (per client, legacy `wireless` default-*-tx-limit) | ROS wireless 🔌 a legacy `wireless` AP limits each client | 3 | 🟨 |
| MAC filter (allow/deny per MAC, remove) 🔌 add and remove a rule on a `wireless` board and a `wifi` board | ROS access-list | 3 | 🟨 |
| Private PSK (per-client passwords, client drawer; needs hardware (Wi-Fi AP)) | ROS access-list `passphrase` (wifi) / `private-pre-shared-key` (wireless) 🔌 a client joins only with its own password | 4 | 🟨 |
| Band steering / fast roaming (802.11r/k/v; needs hardware (Wi-Fi AP on the new wifi package)) 🔌 an AP on the new wifi package; a client roams fast | ROS wifi `security.ft`, `steering.rrm/wnm` (new pkg) | 4 | 🟨 |
| MLO (WiFi 7; needs hardware (Wi-Fi 7 AP: wifi-qcom-be / wifi-mediatek)) 🔌 a Wi-Fi 7 AP and an MLO-capable client | ROS `/interface/wifi/network mlo` | 4 | 🟨 |
| Hotspot 2.0 / Passpoint (needs hardware (Wi-Fi AP on the new wifi package) + WPA-Enterprise/RADIUS) 🔌 a wifi-package AP with RADIUS; a Passpoint phone connects | ROS wifi interworking | 4 | 🟨 |
| QR code to join WiFi (password typed in the browser, never stored) 🔌 scan with a real phone and join | tik (browser) | 2 | 🟨 |
| Channel / tx power per radio 🔌 set channel and power on a real AP radio | ROS · CMR radio | 3 | 🟨 |

## Networks
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Create network: name, VLAN, subnet, DHCP in one form | ROS composite (Safe apply) | 3 | ✅ |
| DHCP range, lease time, DNS, gateway | ROS dhcp-server | 3 | ✅ |
| DHCP options (custom) | ROS dhcp option | 4 | ✅ |
| Network isolation | ROS firewall | 3 | ✅ |
| IPv6 (prefix delegation, SLAAC, DHCPv6 server per network) 🔌 a real ISP prefix: check `address-pool` with a delegated pool on 7.18+, a client getting an address with the RA managed flag, and remove + revert | ROS ipv6 | 4 | 🟨 |
| IGMP snooping / multicast | ROS bridge igmp-snooping | 4 | ✅ |
| mDNS reflector across networks | ROS `/ip/dns mdns-repeat-ifaces` | 3 | ✅ |
| Content filtering per network (DHCP DNS: family-safe resolver or router ad list; bypassable) 🔌 a client on that network gets the filtered DNS | ROS DNS / adlist per network (limited) | 4 | 🟨 |
| Domain name / local DNS 🔌 the DHCP domain reaches clients; names resolve | ROS dhcp network domain + dns static | 3 | 🟨 |

## Internet (WAN)
| Feature | Backing | Phase | Status |
|---|---|---|---|
| WAN type: DHCP / static / PPPoE | ROS | 3 | ✅ |
| VLAN tagged WAN | ROS | 3 | ✅ |
| Multi-WAN failover | ROS routes + check-gateway / netwatch | 3 | ✅ |
| Load balancing (ECMP second default route) 🔌 two real WANs share traffic | ROS PCC / ECMP | 4 | 🟨 |
| Smart Queues (bufferbloat / CAKE / fq_codel) | ROS queue types | 3 | ✅ |
| DNS servers / DoH (encrypted DNS) | ROS `/ip/dns` use-doh-server | 2 | ✅ |
| Dynamic DNS | ROS `/ip/cloud` ddns | 2 | ✅ |
| IPv6 WAN | ROS | 4 | ✅ |
| ISP status / outage history (uptime % and outages from 7 days of minute samples; "down" = WAN link not running) 🔌 pull the WAN cable for 2+ minutes: an outage with the right length appears and uptime drops | tik | 2 | 🟨 |

## VPN
| Feature | Backing | Phase | Status |
|---|---|---|---|
| WireGuard server + peers + QR (enable/disable and remove peers) 🔌 a phone peer connects over a real WAN, disable stops it, remove + revert | ROS `/interface/wireguard` | 2 | 🟨 |
| OpenVPN server (on/off, VPN users; refuses without a certificate and explains how to make one; generates its own CA and server certificate, signs them on the router and offers the CA for download) 🔌 older versions print `certificate=none`; log in with an ovpn VPN user | ROS ovpn-server | 4 | 🟨 |
| L2TP/IPsec server (`use-ipsec=required` with a masked IPsec secret, VPN users add/remove) 🔌 enable with a secret, check the dynamic IPsec peer, log in from a phone, revert leaves `use-ipsec` as it was | ROS | 4 | 🟨 |
| VPN client (to a provider) | ROS wg / ovpn client | 3 | ✅ |
| Site-to-site (WireGuard / IPsec) | ROS | 3 | ✅ |
| Teleport (one-click remote access) | ROS Back To Home (`/ip/cloud`) | 2 | ✅ |
| Policy-based routing through VPN | ROS routing rules / mangle | 3 | ✅ |
| Connected VPN users view (`/ppp/active` + WireGuard peers with a handshake in the last 3 minutes) 🔌 field names `caller-id`/`encoding` on 7.18+ and WireGuard `last-handshake` format over the API | ROS | 2 | 🟨 |

## Security / CyberSecure
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Zone-based firewall (zone matrix) | ROS interface-lists → filter rules | 3 | ✅ |
| Firewall policies (custom rules) | ROS filter | 3 | ✅ |
| Simple app blocking (saved state, several apps in one batch, unblock) 🔌 block and revert the 4 YouTube domains as one change; check apps really stop | ROS tls-host + DNS · CMR app classes | 3 | 🟨 |
| Traffic rules (block/limit by client, app, time) | ROS filter + queues + scheduler | 3 | ✅ |
| Ad blocking | ROS `/ip/dns/adlist` | 2 | ✅ |
| Content filtering (family, adult): presets (adult, gambling, family: third-party lists the router fetches, labelled), plus add, enable/disable and remove ad-lists by URL 🔌 add each preset: `name-count` > 0 and memory OK on a small router; `set-adlist` on an https list; a blocked name stops resolving | ROS adlist / DNS | 2 | 🟨 |
| Region (country) blocking | ROS address-lists + tik feed | 4 | ✅ |
| IP groups / port groups | ROS address-lists | 3 | ✅ |
| Device isolation (per client) | ROS bridge filter | 4 | ✅ |
| IDS/IPS (signature-based) | ✗ | – | ➖ |
| Honeypot | ✗ | – | ➖ |
| Threat intelligence feeds (CyberSecure) | tik blocklist into address-list (partial) | 4 | ✅ |
| Security audit (insecure services, unrestricted management services, open DNS resolver, default admin user, UPnP, outdated RouterOS) 🔌 check that an unrestricted service's empty `address` comes back over the API and the finding clears once restricted | tik | 2 | 🟨 |

## Routing
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Port forwarding | ROS NAT dst-nat | 2 | ✅ |
| Static routes | ROS `/ip/route` | 2 | ✅ |
| Policy-based routes | ROS routing rules | 3 | ✅ |
| UPnP / NAT-PMP | ROS `/ip/upnp` | 2 | ✅ |
| Local DNS records | ROS dns static | 2 | ✅ |
| Dynamic routing (OSPF / BGP) 🔌 a router with OSPF/BGP neighbours | ROS (advanced: show only) | 4 | 🟨 |

## Profiles
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Speed limit profiles (client groups) | ROS queues | 3 | ✅ |
| Port profiles | ROS bridge VLAN | 3 | ✅ |
| RADIUS profiles | ROS `/radius` | 4 | ✅ |
| QoS / DSCP rules | ROS mangle + queue tree | 4 | ✅ |

## Hotspot / Guest portal
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Captive portal (branded: title, message and colour written as an escaped `login.html` in its own folder) 🔌 `/file/add` with contents over the API; whether RouterOS serves its built-in status/logout pages for files missing from `tik-guest-html`; trial and voucher logins work | ROS `/ip/hotspot` + custom html | 4 | 🟨 |
| Vouchers (create, print, expiry) | ROS User Manager | 4 | ✅ |
| Terms acceptance / password portal | ROS hotspot | 4 | ✅ |
| Payments (Stripe etc.) | ✗ out of scope | – | ➖ |
| Guest session overview (active guests, vouchers grouped by batch with used state) 🔌 real hotspot guests logged in; a used voucher shows uptime | ROS hotspot active | 4 | 🟨 |

## Control plane / System
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Admin users + roles | tik (own users) + ROS `/user` | 2 | ✅ |
| SSO / cloud account | ✗ local auth only | – | ➖ |
| Sign in with MikroTik account (role from router group, password only in session memory, changes run as that user) 🔌 sign in with a router admin and a read-only user on a CHR | tik + ROS login | 2 | 🟨 |
| Backups (scheduled, download, restore): list verified on CHR; tikontrol downloads its own `tik-*.rsc` exports (admin only, no secrets), full `.backup` files stay on the router 🔌 download a tik-*.rsc as the reader with `/file/read` (RouterOS 7.13+); check which policy the reader needs | ROS `/system/backup` + `/export` | 2 | 🟨 |
| Firmware/app updates (channel, auto-update window: daily or weekly check, optional install; on CHR the window fires and logs, and a job without the `policy` permission logs a warning instead of stopping silently: RouterOS gives a job only its creator's rights, and the check needs `policy`, install also `reboot`, which the least-privilege writer lacks) 🔌 on a spare router with internet: a real check and an install + restart, with a writer whose group has policy,reboot | ROS / CMR | 2 | 🟨 |
| Integrations / local API (+ API keys) | tik REST | 4 | ✅ |
| Console (terminal in browser) | ROS (SSH/API terminal) | 3 | ✅ |
| Site settings (name, country, timezone, NTP) | ROS `/system/clock`, ntp | 2 | ✅ |
| SNMP | ROS `/snmp` | 3 | ✅ |
| Remote logging / NetFlow export settings | ROS | 3 | ✅ |
| Data retention settings | tik | 2 | ✅ |
| Trusted HTTPS: local CA (name-constrained) signs a 397-day server cert, auto-renewed; CA download + per-device trust steps | tik `/api/v1/ca.crt` | 2 | ✅ |
| Device auth / SSH settings, service hardening | ROS `/ip/service` | 2 | ✅ |
| Debug tools: ping, traceroute, DNS cache lookup (verified on CHR) | ROS `/ping`, `/tool/traceroute`, DNS cache | 3 | ✅ |
| Packet capture: not offered, because it needs the `sniff` policy, which tikontrol's users must not have (least privilege); use WinBox Tools → Packet Sniffer | ✗ by design | – | ➖ |
| Safe apply (auto-revert) | tik + ROS scheduler | 3 | ✅ |
| "Show in RouterOS" for every object (every router-backed section; read-only commands with secret fields left out, guarded by `tests/ros.test.ts`) 🔌 paste the `proplist=`, `count-only` and `where` forms on a router, incl. dotted wifi fields | tik | 2 | 🟨 |

## Platform
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Multi-site / multi-router (own sealed credentials, data, history and Safe apply per router) | tik | 4 | ✅ |
| Mobile app | ✗ → responsive PWA instead | – | ➖ |
| Responsive layout / PWA install | tik | 1 | ✅ |
| Dark / light theme, or Auto following the OS setting | tik | 1 | ✅ |
| Runs as RouterOS container (arm/v7, arm64) 🔌 an arm/v7 and an arm64 router: the container reaches its own API on 8729 within its memory limit | tik build | 1 | 🟨 |
| Runs on Docker (home server) 🔌 a Docker host reaches a real router through NAT | tik build | 0 | 🟨 |
| One-paste RouterOS installer (`deploy/install.rsc`, checks, api-ssl, App, static lease, DNS) and uninstaller ([INSTALL.md](INSTALL.md)) 🔌 run `install.rsc` and `uninstall.rsc` on a real router | tik + ROS `/app` | 1 | 🟨 |
| CHR dev/test router (`src/scripts/chr.sh`, QEMU) | – | 0 | ✅ |

## Themes
Not a UniFi feature: this is our own addition. Light or dark, the colour theme and the design theme are three separate choices. Themes are sets of CSS variables, so they need no router support.

| Feature | Backing | Phase | Status |
|---|---|---|---|
| Colour themes: choose a palette, in both light and dark | tik | 2 | ✅ |
| Design themes: corner rounding, spacing and density, borders and shadows, fonts, icon style (presets plus separate fine-tune choices, saved per browser) | tik | 2 | ✅ |
| MikroTik-style colour theme: own palette based on MikroTik's current look, no logos or implied endorsement | tik | 2 | ✅ |
| Theme picker with live preview, saved per user | tik | 2 | ✅ |
| One-click looks: Controller (UniFi-like), Router admin (MikroTik-like), Minimal — mode, colours and design together | tik | 2 | ✅ |
| Every theme meets WCAG 2.2 AA contrast | tik | 2 | ✅ |

## Opening tikontrol from RouterOS
Not a UniFi feature: this is our own addition. We want a way to reach tikontrol from RouterOS, and the method is still undecided. A real tab inside WinBox looks impossible: no plugin or extension system was found, though this is untested. Findings are in [research/winbox_integration.md](research/winbox_integration.md).

| Feature | Backing | Phase | Status |
|---|---|---|---|
| Decide how users open tikontrol from RouterOS (RouterOS App link + WebFig login link where Apps run, a browser bookmark to `tikontrol.lan` elsewhere; see the research note) | tik | 2 | ✅ |
| Package tikontrol as a custom RouterOS App, with a `ui-url` link 🔌 install `deploy/routeros-app.yaml` on a 7.22+ arm64/x86 router and check the generated `ui-url` opens tikontrol over HTTPS | ROS `/app` (arm64, x86) | 4 | 🟨 |
| Link with our own icon on the WebFig login page (`show-in-webfig`) 🔌 check the link appears; find the `icon` format and embed our icon without an outbound fetch | ROS `/app` | 4 | 🟨 |
| Check on hardware how an App appears in WinBox and what opening it does 🔌 is the App listed in WinBox, is `ui-url` clickable, does it open a browser | ROS | 0 | ⬜ |

## Read-only mode
Not a UniFi feature: this is our own addition. The router enforces it, not tikontrol. tikontrol connects with a RouterOS user that has no `write` policy, so RouterOS refuses edits even if the app has a bug or is compromised.

| Feature | Backing | Phase | Status |
|---|---|---|---|
| Choose read-only or read and write at setup, with read-only as the default 🔌 both choices end to end on a CHR | tik | 1 | 🟨 |
| Router-enforced read-only: connect with a user that has no `write` policy | ROS user groups | 1 | ✅ |
| Guided setup: show the commands to create the read-only group and user 🔌 paste the commands on a real router; tikontrol connects | tik + ROS | 1 | 🟨 |
| One-step setup: sign in with a MikroTik admin, tikontrol creates its read-only group and user | tik + ROS | 1 | ✅ |
| Detect the connected user's real policies and show them, instead of trusting a setting | ROS `/user` | 1 | ✅ |
| Read-only badge, with edit controls disabled and the reason shown | tik | 1 | ✅ |
| Store no write credential in read-only mode | tik | 1 | ✅ |
| Switching to read and write needs a new credential and an explicit confirmation 🔌 switch with a new writer credential on a CHR | tik | 2 | 🟨 |
| MCP follows the same mode: no proposed changes in read-only mode | tik | 2 | ✅ |
| Test on CHR that the router rejects writes from the read-only user | tik | 0 | ✅ |

## AI assistants (MCP)
Not a UniFi feature: this is our own addition. An MCP server lets an AI assistant query tikontrol and propose changes.

| Feature | Backing | Phase | Status |
|---|---|---|---|
| MCP server: off by default, local network only, own access token | tik | 2 | ✅ |
| Read-only tools: clients, Wi-Fi, events, insights, topology | tik + ROS | 2 | ✅ |
| Proposed changes, applied only after approval in Review & apply: the assistant sends a tikontrol intent, compiled read-only when proposed and applied through Safe apply (text-only proposals are never run) 🔌 propose a change from a real MCP client (e.g. Claude Desktop) against a CHR, then apply and revert it | tik + Safe apply | 3 | 🟨 |
| Privacy mode: hide or swap names and addresses before data leaves tikontrol | tik | 2 | ✅ |
| Treat device names and SSIDs as untrusted input (prompt injection) 🔌 a real AI client against a crafted SSID that tries to inject instructions | tik | 2 | 🟨 |
| Never expose router credentials through MCP | tik | 2 | ✅ |
