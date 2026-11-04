# Feature & page tracker

This is the single source of truth for **what UniFi Network has** and **where tikontrol stands** on each item.
Update the Status column whenever a feature changes state. The progress numbers at the top are recounted by
`src/scripts/progress.py` (run it after editing).

Reference: UniFi Network Application 10.x (latest 10.6.109, Oct 2026), taken from public docs and release notes.
Clean-room: we copy the behaviour, never the assets.

**Status:** ⬜ todo · 🎨 mockup (UI only, fake/snapshot data) · 🟨 in progress · ✅ done (real data, tested) · ➖ won't do
**Backing:** `ROS` = RouterOS API on the router · `CMR` = MikroTik CMR server · `tik` = our own logic/DB · `✗` = nothing can back it

<!-- progress:start -->
**Progress:** 6 ✅ · 142 🟨 · 59 🎨 · 12 ⬜ · 8 ➖ (227 items, 3% done)
<!-- progress:end -->

---

## Pages (navigation)

| # | UniFi page | tikontrol page | Phase | Status |
|---|---|---|---|---|
| P1 | Dashboard | Dashboard | 1 | 🟨 |
| P2 | UniFi Devices (list) | Devices | 3 |🎨 |
| P3 | Topology | Topology (tab in Devices) | 2 | 🟨 |
| P4 | Device detail (overview / ports / insights / settings) | Device drawer | 3 |🎨 |
| P5 | Client Devices (list) | Clients | 1 | 🟨 |
| P6 | Client detail (overview / insights / settings) | Client drawer | 1 | 🟨 |
| P7 | Ports (port manager) | Ports | 2 | 🟨 |
| P8 | Radios (Radio Manager, coverage, connectivity) | Radios | 3 |🎨 |
| P9 | Insights / Traffic flows | Traffic | 3 |🎨 |
| P10 | Alarm Manager | Alerts | 2 |🎨 |
| P11 | System Log (events, admin activity, security, updates) | Events | 1 | 🟨 |
| P12 | Hotspot Manager | Guest portal | 4 |🎨 |
| P13 | Settings → WiFi | Settings · WiFi | 2 |🎨 |
| P14 | Settings → Networks | Settings · Networks | 3 |🎨 |
| P15 | Settings → Internet | Settings · Internet | 3 |🎨 |
| P16 | Settings → VPN | Settings · VPN | 2 |🎨 |
| P17 | Settings → Security / CyberSecure | Settings · Security | 3 |🎨 |
| P18 | Settings → Routing | Settings · Routing | 2 |🎨 |
| P19 | Settings → Profiles | Settings · Profiles | 3 |🎨 |
| P20 | Settings → Control Plane (admins, backups, updates, integrations, console) | Settings · System | 2 |🎨 |
| P21 | Settings → System (site, logging, SNMP, device auth) | Settings · System | 2 |🎨 |
| P22 | Site Manager (multi-site, cloud) | Sites switcher | 4 | ⬜ |
| P23 | Login / onboarding wizard | Login + first-run wizard | 1 | 🟨 |
| P24 | Global search (⌘K) | Command palette | 2 | 🎨 |

---

## Dashboard
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Status-first summary with insights (insecure services, double NAT, low RAM, band flapping, pending firmware) | tik on ROS data | 1 | 🟨 |
| Internet health (WAN up, latency, packet loss) | ROS netwatch + tik | 1 | 🟨 |
| WAN live throughput (down/up) graph | ROS monitor-traffic | 1 | 🟨 |
| Max download/upload speed indicators | tik | 1 | ⬜ |
| ISP name, public IP, WAN IP | ROS `/ip/cloud`, dhcp-client | 1 | 🟨 |
| Client count (wired / wireless / guest) | ROS | 1 | 🟨 |
| Gateway resources (CPU, RAM, temp, uptime) | ROS `/system/resource`, health | 1 | 🟨 |
| Top clients by traffic | ROS kid-control + tik | 1 | 🟨 |
| Top applications | CMR apptraffic | 3 | ⬜ |
| Traffic Identification card (by client now, by app once CMR is wired) | ROS reg-table bytes · CMR | 1 | 🟨 |
| WiFi Technology card (clients by standard + band, activity, experience) | ROS reg-table | 1 | 🟨 |
| Most active clients / access points strips | ROS + tik | 1 | 🟨 |
| Internet Activity chart: client count cells + down/up, 1H/1D/1W | tik history | 2 | 🟨 |
| Gateway column (WAN/gateway IP, uptime, latency to 3 resolvers, channel widths) | ROS | 1 | 🟨 |
| WiFi experience / satisfaction score | tik (signal + rate heuristic) | 2 | 🟨 |
| Recent critical events | ROS log + tik | 1 | 🟨 |
| Firmware update available banner | ROS / CMR | 2 | 🟨 |
| Speed test (run + history) | tik container client | 3 | ⬜ |
| Customisable widgets / layout | tik | 4 | 🟨 |

## UniFi Devices / Topology
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Device list (name, model, IP, version, uptime, state) | CMR `/cmr/device` | 3 |🎨 |
| Device adoption (one-click) | CMR pairing | 3 |🎨 |
| Auto-detect make, model, CPU, memory and WiFi capabilities | ROS `/system/resource`, `/system/routerboard` | 1 | 🟨 |
| Device detail: CPU/RAM/temperature history | CMR dashboard + tik | 3 |🎨 |
| Device restart / locate (blink LED) | CMR reboot · ROS `/system/routerboard` LED | 3 | 🟨 |
| Firmware update per device | CMR / ROS | 3 | 🟨 |
| Scheduled / rolling fleet updates | CMR upgrade rules | 3 |🎨 |
| Topology map (auto layout, wired + wireless links) | CMR layout · ROS neighbor + bridge host | 2 | 🟨 |
| Topology Spotlight (highlight/filter) | tik | 3 | 🟨 |
| Device Supervisor (health anomalies) | CMR state alerts | 4 |🎨 |
| Blueprints / config drift inspector | tik (export diff) | 4 |🎨 |
| Device SSH credentials management | ROS `/user` | 3 | ⬜ |
| Device LED control / Etherlighting | ROS LED · Etherlighting ✗ (UI hw) | 4 | 🟨 |
| Shadow mode / gateway HA | ✗ (VRRP is possible but out of scope) | – | ➖ |

## Client Devices
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Client list: name, IP, MAC, vendor, connection, uptime | ROS leases + ARP + reg-table + bridge host | 1 | 🟨 |
| Device fingerprint / auto icon (phone, TV, PC…) | tik (OUI + hostname + DHCP heuristics) | 1 | 🟨 |
| Wired: switch port · Wireless: SSID, band, signal, rate | ROS bridge host + reg-table | 1 | 🟨 |
| Live per-client throughput | ROS kid-control / reg-table bytes | 1 | 🟨 |
| Per-client traffic history | tik SQLite | 2 | 🟨 |
| Per-client apps / domains | tik DNS correlation (CMR is per-device) | 3 |🎨 |
| Rename / alias, custom icon, notes | tik | 1 | 🟨 |
| Fixed IP (reservation) | ROS lease make-static | 2 | 🟨 |
| Local DNS record for client | ROS `/ip/dns/static` | 2 | 🟨 |
| Block / unblock | ROS address-list + filter | 2 | 🟨 |
| Pause internet / schedules | ROS kid-control | 2 | 🟨 |
| Speed limit (client groups) | ROS simple queues | 2 | 🟨 |
| Kick / reconnect wireless client | ROS reg-table remove | 2 | 🟨 |
| Override network/VLAN for client | ROS (MAC-based VLAN / access list) | 4 |🎨 |
| Offline / historical clients list | tik | 2 | 🟨 |
| Roaming history (which AP, when) | tik (reg-table changes) · CMR wifi-logs | 3 |🎨 |
| Filters (wired / wireless / guest / offline) and search | tik | 1 | 🟨 |
| Client groups | tik | 3 | 🟨 |

## Ports
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Port grid with link state, speed, duplex | ROS `/interface/ethernet` monitor | 2 | 🟨 |
| Which client is on which port | ROS bridge host | 2 | 🟨 |
| Per-port traffic / errors | ROS | 2 | 🟨 |
| PoE status / power cycle | ROS `/interface/ethernet/poe` (hw dependent) | 3 | 🟨 |
| Port profile (VLAN access/trunk, native VLAN) | ROS bridge VLAN · CMR vlan | 3 | 🟨 |
| Port disable / enable | ROS | 2 | 🟨 |
| Port mirroring | ROS switch mirror | 4 |🎨 |
| Link aggregation | ROS bonding | 4 |🎨 |
| STP / loop protection settings | ROS bridge protocol-mode | 4 |🎨 |
| Storm control | ROS switch (hw dependent) | 4 |🎨 |
| 802.1X port auth | ROS dot1x | 4 |🎨 |
| LLDP-MED / voice VLAN | ROS lldp | 4 |🎨 |

## Radios / WiFi monitoring
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Radio list per AP: band, channel, width, tx power, clients | ROS wireless/wifi · CMR | 3 | 🟨 |
| Channel utilisation | ROS monitor (driver dependent) | 3 | ⬜ |
| Neighbouring networks / interference scan | ROS `scan` (duration-limited) | 3 |🎨 |
| Signal strength distribution | tik from reg-table | 3 | 🟨 |
| WiFi connectivity success (association / auth / DHCP / DNS) | tik from log + leases | 2 | 🟨 |
| Radio link quality (CCQ buckets, channel, power, noise) | ROS wireless monitor | 2 | 🟨 |
| Roaming journeys (Connectivity view) | tik · CMR wifi-logs | 3 |🎨 |
| Coverage map / floorplan (Design Center) | tik (manual floorplan, no RF model) | 4 |🎨 |
| WiFiman (client-side speed/coverage app) | ✗ | – | ➖ |

## Insights / Traffic
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Traffic by application / category | CMR apptraffic | 3 |🎨 |
| Traffic by client over time | tik | 2 | 🟨 |
| Top domains | ROS DNS cache + tik | 3 | 🟨 |
| Flow log (src, dst, port, bytes) | ROS traffic-flow (IPFIX) → tik collector | 3 | 🟨 |
| Traffic by country | tik GeoIP on flows | 4 |🎨 |
| Blocked threats view | ✗ (no IDS) | – | ➖ |

## Alarm Manager / Notifications
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Alert rules (device offline, WAN down, new client, CPU/RAM) | tik · CMR alerts | 2 |🟨 |
| Channels: email, push (ntfy), Telegram, webhook (ntfy/Telegram/webhook built, untested against the real services; email not done) | tik | 2 |🟨 |
| Alert history / acknowledge | tik | 2 |🟨 |
| Quiet hours / per-rule severity | tik | 3 |🟨 |

## System Log / Events
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Live event stream | ROS `/log follow` | 1 | 🟨 |
| Categories: clients, devices, admin activity, security, updates | tik (topic mapping) | 1 | 🟨 |
| Derived events (client joined/left, WAN flap) | tik | 2 | 🟨 |
| Search / filter / export CSV | tik | 2 | 🟨 |
| Remote syslog / SIEM forwarding | ROS logging action | 3 | 🟨 |

## WiFi settings
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Create/edit SSID (name, password, security WPA2/WPA3) | ROS wireless/wifi · CMR wifi | 2 |🟨 |
| Band selection (2.4 / 5 / 6 GHz) | ROS | 2 |🎨 |
| Which APs broadcast it | CMR labels | 3 |🎨 |
| Network/VLAN per SSID | ROS · CMR | 3 |🎨 |
| Guest network (isolation, portal) | ROS + hotspot | 3 |🟨 |
| Client isolation | ROS default-forwarding / wifi datapath | 2 |🟨 |
| Hide SSID | ROS | 2 |🟨 |
| WiFi schedule (on/off by time) | ROS scheduler | 3 |🟨 |
| Per-SSID speed limit | ROS queues | 3 |🎨 |
| MAC filter | ROS access-list | 3 |🟨 |
| Private PSK (per-client passwords) | ROS wifi access-list passphrase (new wifi pkg) | 4 |🎨 |
| Band steering / fast roaming (802.11r/k/v) | ROS wifi (new pkg) | 4 |🎨 |
| MLO (WiFi 7) | ROS wifi-qcom-be (hw dependent) | 4 |🎨 |
| Hotspot 2.0 / Passpoint | ROS wifi interworking | 4 |🎨 |
| QR code to join WiFi | tik (browser) | 2 |🎨 |
| Channel / tx power per radio | ROS · CMR radio | 3 |🟨 |

## Networks
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Create network: name, VLAN, subnet, DHCP in one form | ROS composite (Safe apply) | 3 | 🟨 |
| DHCP range, lease time, DNS, gateway | ROS dhcp-server | 3 | 🟨 |
| DHCP options (custom) | ROS dhcp option | 4 | 🟨 |
| Network isolation | ROS firewall | 3 | 🟨 |
| IPv6 (prefix delegation, SLAAC, DHCPv6) | ROS ipv6 | 4 |🎨 |
| IGMP snooping / multicast | ROS bridge igmp-snooping | 4 | 🟨 |
| mDNS reflector across networks | ROS `/ip/dns mdns-repeat-ifaces` | 3 | 🟨 |
| Content filtering per network | ROS DNS / adlist per network (limited) | 4 |🎨 |
| Domain name / local DNS | ROS dns static | 3 |🎨 |

## Internet (WAN)
| Feature | Backing | Phase | Status |
|---|---|---|---|
| WAN type: DHCP / static / PPPoE | ROS | 3 | 🟨 |
| VLAN tagged WAN | ROS | 3 | 🟨 |
| Multi-WAN failover | ROS routes + check-gateway / netwatch | 3 | 🟨 |
| Load balancing | ROS PCC / ECMP | 4 |🎨 |
| Smart Queues (bufferbloat / CAKE / fq_codel) | ROS queue types | 3 | 🟨 |
| DNS servers / DoH (encrypted DNS) | ROS `/ip/dns` use-doh-server | 2 | 🟨 |
| Dynamic DNS | ROS `/ip/cloud` ddns | 2 | 🟨 |
| IPv6 WAN | ROS | 4 | 🟨 |
| ISP status / outage history | tik | 2 | 🟨 |

## VPN
| Feature | Backing | Phase | Status |
|---|---|---|---|
| WireGuard server + peers + QR | ROS `/interface/wireguard` | 2 |🟨 |
| OpenVPN server | ROS ovpn-server | 4 |🟨 |
| L2TP/IPsec server | ROS | 4 |🟨 |
| VPN client (to a provider) | ROS wg / ovpn client | 3 |🎨 |
| Site-to-site (WireGuard / IPsec) | ROS | 3 |🎨 |
| Teleport (one-click remote access) | ROS Back To Home (`/ip/cloud`) | 2 |🎨 |
| Policy-based routing through VPN | ROS routing rules / mangle | 3 |🎨 |
| Connected VPN users view | ROS | 2 | 🟨 |

## Security / CyberSecure
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Zone-based firewall (zone matrix) | ROS interface-lists → filter rules | 3 | 🟨 |
| Firewall policies (custom rules) | ROS filter | 3 | 🟨 |
| Simple app blocking | ROS tls-host + DNS · CMR app classes | 3 | 🟨 |
| Traffic rules (block/limit by client, app, time) | ROS filter + queues + scheduler | 3 | ⬜ |
| Ad blocking | ROS `/ip/dns/adlist` | 2 | 🟨 |
| Content filtering (family, adult) | ROS adlist / DNS | 2 | 🟨 |
| Region (country) blocking | ROS address-lists + tik feed | 4 | 🟨 |
| IP groups / port groups | ROS address-lists | 3 | 🟨 |
| Device isolation (per client) | ROS bridge filter | 4 | 🟨 |
| IDS/IPS (signature-based) | ✗ | – | ➖ |
| Honeypot | ✗ | – | ➖ |
| Threat intelligence feeds (CyberSecure) | tik blocklist into address-list (partial) | 4 | 🟨 |
| Security audit (telnet/ftp/http on, default creds) | tik | 2 | 🟨 |

## Routing
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Port forwarding | ROS NAT dst-nat | 2 | 🟨 |
| Static routes | ROS `/ip/route` | 2 | 🟨 |
| Policy-based routes | ROS routing rules | 3 |🎨 |
| UPnP / NAT-PMP | ROS `/ip/upnp` | 2 | 🟨 |
| Local DNS records | ROS dns static | 2 | 🟨 |
| Dynamic routing (OSPF / BGP) | ROS (advanced: show only) | 4 |🎨 |

## Profiles
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Speed limit profiles (client groups) | ROS queues | 3 | 🟨 |
| Port profiles | ROS bridge VLAN | 3 | 🟨 |
| RADIUS profiles | ROS `/radius` | 4 | 🟨 |
| QoS / DSCP rules | ROS mangle + queue tree | 4 | 🟨 |

## Hotspot / Guest portal
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Captive portal (branded) | ROS `/ip/hotspot` + custom html | 4 | 🟨 |
| Vouchers (create, print, expiry) | ROS User Manager | 4 | 🟨 |
| Terms acceptance / password portal | ROS hotspot | 4 | 🟨 |
| Payments (Stripe etc.) | ✗ out of scope | – | ➖ |
| Guest session overview | ROS hotspot active | 4 |🎨 |

## Control plane / System
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Admin users + roles | tik (own users) + ROS `/user` | 2 | 🟨 |
| SSO / cloud account | ✗ local auth only | – | ➖ |
| Sign in with MikroTik account (role from router group, password only in session memory, changes run as that user) | tik + ROS login | 2 | 🟨 |
| Backups (scheduled, download, restore) | ROS `/system/backup` + `/export` | 2 | 🟨 |
| Firmware/app updates (channel, auto-update window) | ROS / CMR | 2 | 🟨 |
| Integrations / local API (+ API keys) | tik REST | 4 |🎨 |
| Console (terminal in browser) | ROS (SSH/API terminal) | 3 |🎨 |
| Site settings (name, country, timezone, NTP) | ROS `/system/clock`, ntp | 2 | 🟨 |
| SNMP | ROS `/snmp` | 3 | 🟨 |
| Remote logging / NetFlow export settings | ROS | 3 | 🟨 |
| Data retention settings | tik | 2 |🎨 |
| Device auth / SSH settings, service hardening | ROS `/ip/service` | 2 | 🟨 |
| Debug tools: ping, traceroute, packet capture | ROS `/ping`, `/tool/traceroute`, sniffer | 3 | 🟨 |
| Safe apply (auto-revert) | tik + ROS scheduler | 3 | 🟨 |
| "Show in RouterOS" for every object | tik | 2 | 🟨 |

## Platform
| Feature | Backing | Phase | Status |
|---|---|---|---|
| Multi-site / multi-router | tik + CMR | 4 | ⬜ |
| Mobile app | ✗ → responsive PWA instead | – | ➖ |
| Responsive layout / PWA install | tik | 1 | 🟨 |
| Dark / light theme | tik | 1 | ✅ |
| Runs as RouterOS container (arm/v7, arm64) | tik build | 1 | ⬜ |
| Runs on Docker (home server) | tik build | 0 | 🟨 |
| CHR dev/test router | – | 0 | ⬜ |

## Themes
Not a UniFi feature: this is our own addition. Light or dark, the colour theme and the design theme are three separate choices. Themes are sets of CSS variables, so they need no router support.

| Feature | Backing | Phase | Status |
|---|---|---|---|
| Colour themes: choose a palette, in both light and dark | tik | 2 | ✅ |
| Design themes: corner rounding, spacing and density, borders and shadows, fonts, icon style | tik | 2 | 🟨 |
| MikroTik-style colour theme: own palette based on MikroTik's current look, no logos or implied endorsement | tik | 2 | ✅ |
| Theme picker with live preview, saved per user | tik | 2 | ✅ |
| Every theme meets WCAG 2.2 AA contrast | tik | 2 | ✅ |

## Opening tikontrol from RouterOS
Not a UniFi feature: this is our own addition. We want a way to reach tikontrol from RouterOS, and the method is still undecided. A real tab inside WinBox looks impossible: no plugin or extension system was found, though this is untested. Findings are in [research/winbox_integration.md](research/winbox_integration.md).

| Feature | Backing | Phase | Status |
|---|---|---|---|
| Decide how users open tikontrol from RouterOS | tik | 2 | 🟨 |
| Package tikontrol as a custom RouterOS App, with a `ui-url` link | ROS `/app` (arm64, x86) | 4 | 🟨 |
| Link with our own icon on the WebFig login page (`show-in-webfig`) | ROS `/app` | 4 | 🟨 |
| Check on hardware how an App appears in WinBox and what opening it does | ROS | 0 | ⬜ |

## Read-only mode
Not a UniFi feature: this is our own addition. The router enforces it, not tikontrol. tikontrol connects with a RouterOS user that has no `write` policy, so RouterOS refuses edits even if the app has a bug or is compromised.

| Feature | Backing | Phase | Status |
|---|---|---|---|
| Choose read-only or read and write at setup, with read-only as the default | tik | 1 | 🟨 |
| Router-enforced read-only: connect with a user that has no `write` policy | ROS user groups | 1 | 🟨 |
| Guided setup: show the commands to create the read-only group and user | tik + ROS | 1 | 🟨 |
| Detect the connected user's real policies and show them, instead of trusting a setting | ROS `/user` | 1 | 🟨 |
| Read-only badge, with edit controls disabled and the reason shown | tik | 1 | 🟨 |
| Store no write credential in read-only mode | tik | 1 | ✅ |
| Switching to read and write needs a new credential and an explicit confirmation | tik | 2 | 🟨 |
| MCP follows the same mode: no proposed changes in read-only mode | tik | 2 | 🟨 |
| Test on CHR that the router rejects writes from the read-only user | tik | 0 | ⬜ |

## AI assistants (MCP)
Not a UniFi feature: this is our own addition. An MCP server lets an AI assistant query tikontrol and propose changes.

| Feature | Backing | Phase | Status |
|---|---|---|---|
| MCP server: off by default, local network only, own access token | tik | 2 | 🟨 |
| Read-only tools: clients, Wi-Fi, events, insights, topology | tik + ROS | 2 | 🟨 |
| Proposed changes, applied only after approval in Review & apply | tik + Safe apply | 3 | 🟨 |
| Privacy mode: hide or swap names and addresses before data leaves tikontrol | tik | 2 | 🟨 |
| Treat device names and SSIDs as untrusted input (prompt injection) | tik | 2 | 🟨 |
| Never expose router credentials through MCP | tik | 2 | 🟨 |
