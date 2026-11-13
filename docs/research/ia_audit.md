# UI information architecture audit (2026-11)

The owner's words: "Currently there are a lot of pages named differently displaying the same things, so please do a
logical audit of displayed stuff."

This note lists every page, tab, card, side panel, settings section and palette entry in `src/ui/src`, says what
each one shows and what it lets you do, and then finds the overlaps. It ends with a simpler navigation and a phased
plan in small steps. **No capability is removed**: section 7 says where each one goes.

Scope and neighbours:

- What goes *inside* the client, router and neighbour side panels belongs to
  [device_client_views.md](device_client_views.md). This note only says which panel a thing belongs in.
- Switches and APs as adopted devices, sites vs devices and the merged topology belong to
  [multi_device.md](multi_device.md). This note keeps the Devices page ready for that, and does not redesign it.
- Simplicity rules are in [PLAN.md §0.6](../PLAN.md) (rule 5: about 8 top-level items; rule 8: plain words).

Method: we read `App.svelte`, `lib/Palette.svelte`, every file in `pages/` and `pages/settings/`, and the shared
components they mount (`Ports`, `Radios`, `AlarmManager`, `CoverageMap`). Then we ran the UI on `sample.json` only
(a copy of `src/ui` without `snapshot.json`, Vite dev server, headless Chromium via Playwright) and opened all 33
routes plus the client and router panels. Every name, address and number in this note is made up or comes from
the fictional sample.

## 1. Navigation today

| Where | Items |
|---|---|
| Left rail (8) | Dashboard, Topology, Devices, Clients, Insights, Logs & Alarms (`#/events`), Hotspot, Settings |
| Top bar | "Network" app pill + Devices and Clients icons (a second copy of three rail items), mode pill, ⌘K search, "Show RouterOS" toggle, theme, site selector, account menu |
| Side panels (over any page) | Client (`#/clients/<mac>`), this router (`#/device/router`), neighbour MikroTik (`#/device/<mac>`) |
| ⌘K palette | The 8 pages, the 9 settings sections, every client, this router, every neighbour, every SSID (→ Settings › WiFi). No tabs, no settings sections inside a page |
| Aliases | `#/ports` → Devices › Ports, `#/radios` → Devices › Radios |

That is 8 rail items, 21 page tabs, 9 settings pages with about 60 sections, 8 dashboard cards plus a sidebar,
3 panels with 6 tabs. Many of them show the same rows under a different name.

## 2. Inventory: page → data → actions

Data names: `raw.*` are snapshot fields (`src/server/src/ros/queries.rs`), API paths are under `/api/v1`. "Review"
means the action goes through Safe apply.

### 2.1 Top-level pages

| Page › tab / card | Shows (entities, data) | Actions |
|---|---|---|
| **Dashboard** sidebar | Router face, identity, specs, RouterOS version, WAN IP, gateway IP, system uptime, double-NAT, public IP, WAN uptime % (ISP history), live WAN rate + peak, latency chips, Internet Health strip (12 h), Speed test, "Default WiFi Speeds" (channel widths per radio), Ad Blocking counts, Security Checks (= `data.insights`: security audit + double NAT + memory + band flapping + RouterBOOT) | Run speed test, set its endpoint (`/speedtest`); links: Release Notes, View Logs, See All (→ Events), Uptime (→ Settings › Internet), Ad Blocking (→ Security), each check |
| Dashboard › Traffic Identification | Top 5 Wi-Fi clients by bytes since connect (donut + table); top CMR apps last hour (`/cmr`) | Row → client panel; › → Insights › Applications |
| Dashboard › WiFi Technology | Wi-Fi clients by standard and band, activity, experience | Rows → Clients (unfiltered); › → Devices › Radios |
| Dashboard › Most Active Clients | 7 clients by live download rate | Tile → client panel |
| Dashboard › Most Active Access Points | One tile per radio of this router, client count | Tile → router panel; › → Radios |
| Dashboard › Internet Activity | WAN throughput and client count, 1H/1D/1W, bytes since boot | Mode and range toggles |
| Dashboard › Radio Link Quality | CCQ buckets per band, channel, width, TX power, noise, overall CCQ | Band toggle |
| Dashboard › WiFi Connectivity | Association/auth/DHCP/DNS success bars, client signal strip | Bubble → client panel; › → Events |
| Dashboard › Recent Changes | Last 5 Admin events (`/events?cat=Admin`) or this session's applied changes | › → Events filtered to Admin |
| Dashboard layout | — | Hide, show, reorder, widen cards (browser prefs) |
| **Topology** | Tree Internet → upstream → this router → neighbours (MNDP/LLDP) → wired clients, VMs, Wi-Fi hub → Wi-Fi clients; port speeds (`raw.etherMon`), counts; side list "All connected devices" (= online clients) | Spotlight search, zoom, hover; node → client / router / neighbour panel |
| **Devices** › MikroTik Devices | This router (model, IP, version, uptime, CPU, memory, client count), CMR devices (`/cmr`), neighbours not in CMR (version warnings) | Row → router or neighbour panel; Find devices (modal); Adopt (CMR pair), Labels, Set up (paste-in guide), Open (switch site) |
| Devices › Ports | Port face, per port: link, speed, PoE, role, profile, traffic, errors, connected clients; bridge summary | Port editor: enable, VLAN access/trunk, saved port profile, speed, PoE, power-cycle, mirroring, bonding, STP/BPDU guard, storm control, 802.1X, LLDP scope |
| Devices › Radios | Per radio: channel, width, DFS, TX power, noise, CCQ, clients; signal histogram; roaming journeys; coverage map (`/floorplan`) | Scan neighbours (writer, confirmed), "Channel & power…" (→ Settings › WiFi), draw floor plan |
| Devices › Updates | Firmware: this router's RouterOS, channel, RouterBOOT; CMR fleet versions; CMR scheduled upgrade rules and jobs | Check, Upgrade firmware, Upgrade now (CMR), add/remove upgrade rule |
| Devices › Supervisor | This router online/CPU/memory; CMR offline devices and alerts; CMR alert rules; **"Config drift: not built yet"** | Enable/disable/remove/add CMR alert |
| **Clients** › Online | Online clients: name, maker/type, IP, fixed pill, connection, experience, live rate, connected time | Row → client panel; filter All/WiFi/Wired/Guests; search; sort; Discover devices (mDNS/SSDP, `/discovery`) + auto checkbox |
| Clients › Offline | ARP/DHCP leftovers + server-remembered MACs (`/clients/offline`): MAC, IP, last seen | Forget (review) |
| Clients › Leases | DHCP leases (`raw.leases`) with pool fill per server | Filter by network, search, CSV, Make fixed (review); name → client panel (online only) |
| Clients › Groups | Client groups (browser prefs), members as chips; groups left on the router | Add group, Apply members (address list), Apply limit (PCQ), delete (with or without router cleanup) |
| **Insights** › Applications | CMR app traffic by period and category | Period select; "Set up CMR…" (→ Devices) |
| Insights › Clients | **Internet activity** chart (1D), traffic by client over time (`/history/clients`), **traffic since connect** | Metric and window toggles; row → client panel |
| Insights › Domains | DNS cache grouped by domain (`raw.dnsCache`) | Block (review, `block-domain`) |
| Insights › Flows | IPFIX talkers, destinations, services (`/flows`) | Enable flows (review) |
| Insights › Countries | Bytes per destination country (needs GeoIP + flows) | Link to Flows |
| **Logs & Alarms** › Events | Router log + derived events + Admin changes; category and level filters, folding | Search, CSV; row → client panel |
| Logs & Alarms › Alarm Manager | Active alarms, alarm rules, quiet hours, history (`/alerts`) | Acknowledge, toggle rules, severity, quiet hours |
| Logs & Alarms › Notifications | ntfy, webhook, Telegram, SMTP (`/alerts/channels`); **Remote syslog / SIEM** | Save, send test; syslog Review / Remove |
| **Hotspot** › Guest portal | Captive portal form + branded page preview (`raw.hotspot*`) | Create, change, remove portal; branded page |
| Hotspot › Vouchers | Voucher batches, on-router users | Create batch, remove batch, print |
| Hotspot › Guests | Active portal sessions (`raw.hotspotActive`) | — |
| Hotspot › Payments | One sentence: "not supported" | — |

### 2.2 Side panels

| Panel › tab | Shows | Actions |
|---|---|---|
| Client › Overview | Live rate, experience, connection, standard, link rate, CCQ, IP, MAC, maker, model, type, hostname, vendor class, first/last seen, lease, total traffic, notes; "What the device says"; "Why is it on 2.4 GHz" + band history | Pause/Resume, Limit (switches tab), Fix IP, Reconnect, Traffic rule (→ Security), Isolate, Ping, Block; band fixes (review); rename |
| Client › Insights | 24 h traffic (`/history/client`), roaming history, recent activity | — |
| Client › Settings | Icon, notes, fixed IP, local DNS name, VLAN override, own Wi-Fi password, speed limit, scheduled pause | Review & apply (batch), Set/Remove PSK, Apply/Remove limit |
| Router › Overview | Issues (update, RouterBOOT, memory, storage, heat), face, facts, 24 h CPU/memory/temperature (`/history`) | Issue buttons → Updates, Alarms, Backups |
| Router › Ports | `Ports compact` | Port editor (same as Devices › Ports) |
| Router › Settings | Name, LEDs, SSH | Review & apply; Locate, Restart, Use as a switch / Leave switch mode |
| Neighbour | Facts from MNDP/LLDP, "Behind it" | Open WebFig |

### 2.3 Settings

| Settings page | Sections (data) | Actions (all review) |
|---|---|---|
| WiFi | WiFi networks (SSIDs, clients per SSID), Radios (channel, width, power), Access rules (MAC allow/deny, signal limits, schedules) | Create/edit/remove SSID incl. guest, schedule, roaming, Passpoint, MLO, PPSK note; QR code; radio settings and on/off; MAC rule; remove rows |
| Networks | Networks (subnet, VLAN, DHCP range, **leases used**), Network-wide options (DNS servers, mDNS, IGMP, VLAN filtering, IPv6, **Local DNS records count**), DHCP options | New/edit/remove network, DHCPv6, IPv6; mDNS; IGMP; VLAN filtering; DHCP options |
| Internet | Primary WAN, Second WAN, Smart Queues, DNS, Dynamic DNS, IPv6, **ISP status** (uptime, outages, latency) | WAN type, tagged WAN VLAN, failover/load balancing, SQM, DNS/DoH, DDNS, IPv6 PD |
| VPN | WireGuard server, Connected now, Back To Home, L2TP/IPsec, OpenVPN, VPN users, VPN client, Site-to-site | Add/disable/remove peers with QR, servers, users, client, site-to-site |
| Security | Security audit, Zones, Firewall rules (+ router access rules), Traffic rules, Block apps, Ad blocking & content filtering, Region blocking, IP groups, Device isolation, Threat management | Fix audit item, zone policy, add/remove rules, traffic rules, app and domain blocks, adlists, region blocks, IP groups, isolate, threat feed |
| Routing | Port forwarding, Static routes, **Local DNS records**, UPnP, Policy-based routes, Dynamic routing (read-only) | Add/remove each |
| Profiles | **Speed limit profiles** (per client group), Port profiles (browser only), RADIUS, QoS | Add/remove each |
| Control Plane | Changes from tikontrol (read-only / read & write), Routers (sites), CMR controller, tikontrol users, Router users + SSH keys, Backups, Blueprint & config drift, **Updates** (channel, check, install, update window), API keys, AI assistants (MCP), Console | Mode switch, add/remove site, controller, users, SSH keys, backup/restore, blueprint, channel/install/window, keys, MCP, run print command |
| System | Site (**name**, time zone, NTP), **Router services** (incl. SSH), SNMP, **Remote logging** (syslog + NetFlow), Trust this tikontrol, Data retention, **Debug tools** (ping, traceroute, DNS lookup), Appearance, Safe apply, About | Review & apply each, retention, run tool, appearance prefs |

Other screens: Login, Setup (first run: access mode, commands, address/certificate, credentials), the Safe apply
modal, the "Show in RouterOS" blocks.

## 3. Duplication matrix

Each row is one piece of data or one action that appears in more than one place. **Bold** is the place we propose
to keep as the home; the others become a short summary that links there, or go away.

| Data / action | Shown in | Problem |
|---|---|---|
| Online clients list | **Clients › Online**, Topology side list "All connected devices", Ports "Connected", Dashboard tiles, neighbour panel "Behind it" | Fine as views, but Topology calls clients "devices" |
| Client traffic since connect | **Insights › Clients "Traffic since connect"**, Dashboard "Traffic Identification", Clients "Down / Up" (live rate) | Dashboard card is named after app identification but shows clients |
| WAN activity chart | **Dashboard › Internet Activity** (1H/1D/1W), Insights › Clients "Internet activity" (1D only) | Same chart twice, the second one is weaker |
| App traffic | **Insights › Applications**, Dashboard Traffic Identification (top 5, last hour) | Fine (summary + link) |
| DHCP leases | **Clients › Leases**, client panel (lease, Fix IP), Settings › Networks "Leases used", Clients › Offline (lease leftovers) | Fine, but Leases only links online clients to a panel |
| Fixed IP | Client panel "Fix IP" button **and** Settings "Fixed IP" field, Leases "Make fixed", Online list "fixed" pill | Three labels for one intent (`fixed-ip`); see glossary |
| Local DNS names | **Settings › Routing › Local DNS records**, Networks (count, pointed to "Routing → DNS"), client Settings "Local DNS name" | Wrong pointer (fixed in this commit); DNS records are not routing |
| Client groups and limits | **Clients › Groups** (members + Apply limit), Settings › Profiles "Speed limit profiles" (the same `group-limit` intent on the same group), Security "Traffic rules" target "Client group" | One intent, two editors with different names ("limit" vs "speed profile") |
| Per-client speed limit | **Client panel Settings "Speed limit"** (`limit-client`), Traffic rules "Limit speed", Wi-Fi "Speed limit per client", Hotspot "Speed limit per guest" | Different scopes, same words; fine if labelled by scope |
| Block / pause a client | **Client panel** Block, Pause, Scheduled pause; Security Traffic rules "Block internet"; Wi-Fi Access rules "Deny" | Scheduled pause in the panel is a traffic rule; the Traffic rules list doesn't say which ones came from a panel |
| Block a domain or app | Insights › Domains "Block", **Security › Block apps**, Traffic rules "Block apps / domains", Ad blocking lists | Domains › Block and Block apps write the same kind of entry under two names |
| Device isolation | Client panel Isolate, **Security › Device isolation** list | Fine (action + list), add a link from the list row to the panel |
| Router name | **Router panel › Settings "Name"**, System › Site "Name" | Same `/system/identity`, two forms |
| SSH on/off | Router panel "Device SSH", **System › Router services** | Same `/ip/service`, two forms |
| Updates | **Devices › Updates** (check, RouterBOOT, fleet, CMR rules), Control Plane › Updates (channel, check, install, window, link "Manage updates…"), router panel issue → Updates | "Check for updates" in two places; install only in Settings, RouterBOOT only in Devices |
| Config drift | Control Plane › **Blueprint & config drift** (built), Devices › Supervisor "Config drift: not built yet" | A stub that contradicts the built feature |
| Health and security checks | Dashboard "Security Checks" (security + double NAT + memory + bands + RouterBOOT), **Settings › Security › Security audit** (security only), router panel issues, Devices › Supervisor, mock alarm history | One list under four names; the dashboard calls non-security items "security" |
| Alarms vs alerts | **Logs & Alarms › Alarms** (tikontrol rules), Devices › Supervisor "Alert rules" (CMR), router panel "Alarms" button | Same idea, "alarm" and "alert" mixed |
| Remote syslog | **Settings › System › Remote logging**, Logs & Alarms › Notifications "Remote syslog / SIEM" | The same form twice |
| Diagnostics | **System › Debug tools** (ping, traceroute, DNS), Control Plane › Console, client panel Ping | Tools split over two settings pages |
| Radios: status vs settings | **Devices › Radios** (status, scan), **Settings › WiFi › Radios** (channel, width, power), Dashboard "Radio Link Quality", "Default WiFi Speeds", "Most Active Access Points" | Status and settings split is right; the dashboard has three radio blocks under three names |
| Ports | **Devices › Ports**, router panel › Ports (compact), Settings › Profiles › Port profiles (templates used only in the port editor) | Port profiles live far from where they are used |
| ISP health | **Settings › Internet › ISP status**, Dashboard Uptime %, Internet Health strip ("See All" goes to Events, not ISP status), latency chips | Wrong link; the full view hides in Settings |
| Guests | Clients filter "Guests" (clients on a guest network), Hotspot › Guests (portal sessions), Wi-Fi "Guest network" | Two different lists both called "Guests" |
| Other MikroTik devices | **Devices › MikroTik Devices**, Topology nodes, neighbour panel, Control Plane › Routers (sites), CMR controller, site selector | Covered by [multi_device.md §4](multi_device.md) (site vs adopted device) |
| Discovery | Clients "Discover devices" (mDNS/SSDP about clients), Devices "Find devices" (MikroTik adoption) | Two buttons with the same meaning in plain English, doing different things |
| Primary nav | Rail, top bar "Network / Devices / Clients" | Three rail items repeated in the top bar |

## 4. Glossary: one term per concept

| Concept | Use | Instead of | Where to change |
|---|---|---|---|
| Something on the network that gets an address (phone, TV, server, VM) | **Client** | device (Topology "All connected devices", "Forget device", "Discover devices", Offline column "Device"), guest (except on a guest network) | Topology, Clients › Offline, Clients button |
| A MikroTik that tikontrol shows as infrastructure (router, switch, AP) | **Device** (the one tikontrol is connected to: **this router**) | network device, neighbour (keep "neighbour" only for "heard over MNDP/LLDP, not managed") | Topology stats, Devices tab "MikroTik Devices" → "Devices" |
| Another network with its own gateway, in the site selector | **Site** (UI says "Router" today) | router (Control Plane › Routers) | See [multi_device.md §4](multi_device.md) |
| Something that happened (log line, join, change) | **Event** | log (the page title "Logs & Alarms", "View Logs") | Page title, dashboard links |
| A rule that notifies you, and what it fired | **Alarm** (rule: "alarm rule") | alert, Alarm Manager (as a tab) | CMR alert rules keep MikroTik's own name "CMR alert" in brackets |
| Where alarms are sent | **Notification channel** | — | — |
| Status list with fixes (security, NAT, memory, firmware) | **Health checks**; the security-only subset is the **Security audit** | Security Checks (dashboard), Supervisor anomalies | Dashboard (done), Supervisor |
| Lease that keeps its address | **Fixed IP** (verb: **Fix IP**, pill: "fixed") | Make fixed, static | Clients › Leases button → "Fix IP" |
| A name the router answers for | **Local DNS name** (list: Local DNS records) | DNS (as a pointer) | Networks (done) |
| Devices grouped by MAC for limits and rules | **Client group** | speed profile, group | Profiles "Speed limit profiles" → part of Client groups |
| Addresses grouped for firewall rules | **IP group** | address list (in prose) | — |
| A per-device speed cap | **Speed limit** (scope in the label: per client, per guest, per group member) | Limit, profile | Client panel "Limit" button (see [device_client_views.md](device_client_views.md) step 4) |
| A wireless interface on a device | **Radio** | access point (for one band of the router), "WiFi Speeds", "WiFi Technology" | Dashboard card titles |
| The Wi-Fi brand | **Wi-Fi** in prose and labels | WiFi | All labels. Not a pure rename: event category `WiFi` (server `live.rs`, `tests/events.test.ts`) and `hw.wifiStandard` "WiFi 5" (`tests/hardware.test.ts`) need a display mapping first |
| Login page for a guest network | **Guest portal** (the RouterOS feature name "Hotspot" in brackets once) | Hotspot (as the page title), captive portal | Rail label and page title |
| People logged in on the portal | **Portal sessions** | Guests | Hotspot tab |
| Read-only vs read & write | **Access mode** (same as the first-run wizard) | "Changes from tikontrol" | Control Plane (done) |
| Discovering what a client is | **Identify clients** | Discover devices | Clients (done) |
| Finding MikroTiks to manage | **Find devices** | — | — |

## 5. Gaps: actions missing where users expect them

1. **Offline clients are a dead end.** The rows show a MAC only (no alias, maker or last IP name) and open no
   panel, so you can't rename, see history or fix an IP for a device that is asleep. Show the alias/maker, and open
   the client panel in an "offline" state (Forget lives there too).
2. **Leases link only online clients.** A lease for a known but offline client shows the host name and no link.
3. **Neighbour panel has no "Set up" or "Open".** The Devices row has both; the panel only has WebFig. Covered by
   [device_client_views.md](device_client_views.md) step 10 and [multi_device.md](multi_device.md) step 6.
4. **Client groups are edited only on the Groups tab.** The client panel can't add a client to a group, and
   doesn't show which groups it is in.
5. **Traffic rules don't link back.** A rule made by the panel's Scheduled pause shows in Security › Traffic rules
   with no link to the client; the client panel doesn't list the rules that target it.
6. **Port forwarding is not offered from the client.** The client with the fixed IP is the target of a port
   forward, but you have to go to Routing and type its IP.
7. **Dashboard links go to the wrong place.** Internet Health "See All" → Events (should be Settings › Internet ›
   ISP status). WiFi Technology rows → unfiltered Clients (should filter by band). WiFi Connectivity › → Events
   (should filter the WiFi category, as Recent Changes does with Admin). Most Active Access Points tiles → the
   router panel (should be Devices › Radios). "Ad Blocking: Active" is always shown, even with no list.
8. **Updates are split.** You can install RouterOS only in Settings › Control Plane, and upgrade RouterBOOT only
   in Devices › Updates.
9. **⌘K finds pages, not features.** "port forward", "backup", "leases", "firewall" or "VPN user" find nothing,
   because the palette knows the 9 settings pages but not their sections or the page tabs. Its settings list is a
   hand-kept copy of `Settings.svelte` (a `ponytail:` comment already says so).
10. **Domains › Block is for every device.** There is no "block for this client" from a domain row (Traffic rules
    can, but you have to retype the domain).

## 6. Merge, demote or delete

| Item | Proposal | Why |
|---|---|---|
| Topology (rail item) | Becomes the **Map** view of Devices (List / Map), keeps `#/topology` as an alias | FEATURES.md already calls it "Topology (tab in Devices)"; after [multi_device.md](multi_device.md) step 4 the map is the site's device tree. Rail 8 → 7 |
| Devices › Supervisor | Merge into **Logs & Alarms › Alarms** (a "CMR alerts" section) and the router panel's issue list; delete the drift stub (link to Blueprint & drift) | Without CMR it is one line about this router plus a "not built" note |
| Devices › Ports, Devices › Radios | Keep for now. Once devices other than this router are adopted, each device's panel gets Ports and Radios tabs and these tabs become "all ports / all radios" across devices | Ports already lives in the router panel |
| Insights › Countries | A section at the bottom of **Flows** | It is empty without flows, and links to Flows anyway |
| Insights › Clients "Internet activity" | Remove (the Dashboard card is better) | Duplicate chart |
| Logs & Alarms › Notifications "Remote syslog" | Remove; a link to **System › Remote logging** | Duplicate form |
| Control Plane › Updates | Move into **Devices › Updates** (channel, install, window next to RouterBOOT and CMR) | One place for versions |
| Profiles › Speed limit profiles | Show as the limit field of each **client group** (Clients › Groups); Profiles keeps Port profiles, RADIUS, QoS | One intent, one editor |
| Profiles › Port profiles | Keep, and add "Manage port profiles" in the port editor's profile picker | Used only there |
| Debug tools + Console | One **Tools** section in System (ping, traceroute, DNS lookup, read-only console) | Same purpose |
| Routing › Local DNS records | Move to **Networks** (it serves the networks; Networks already counts them) | Not routing |
| Hotspot › Payments | Delete (FEATURES row → ➖) | One sentence, nothing to do |
| Hotspot › Guests | Rename "Portal sessions"; also count them in the Clients "Guests" filter | Name clash with the Clients filter |
| Control Plane | Rename **Access** and keep: Access mode, tikontrol users, Router users and SSH keys, API keys, AI assistants (MCP). Move Backups and Blueprint & drift to System; Routers (sites) and CMR controller follow [multi_device.md](multi_device.md) (Devices page / site selector) | "Control Plane" means nothing to a home user and mixes six topics |
| System › Site "Name" and router panel "Name"; System › Router services "ssh" and router panel "SSH" | Keep both (the panel is per device, [device_client_views.md](device_client_views.md) §3.3), but they must send the same intent and show the same state. When a site has several devices, the name moves to each device's panel only | Two forms for one setting |
| Top bar "Network / Devices / Clients" | Remove; keep the rail | Second copy of the nav; "Network" pill is an app switcher with one app |
| Dashboard sidebar "Default WiFi Speeds" | Fold into Radio Link Quality (width per band is already there) | Third radio block |
| Dashboard "Traffic Identification" | Rename "Top Traffic" until app identification fills it; then "Applications" | Shows clients, not apps |

Near-empty or dead (keep the honest "not available" rows, but don't give them a page):

- Hotspot › Payments (one sentence).
- Devices › Supervisor › Config drift ("not built yet", while the feature exists in Control Plane).
- Logs & Alarms › Alarm rules / Notification channels **without the server**: the toggles and Save / Send test
  do nothing (mockup only). Say "needs the tikontrol server" instead of fake controls.
- Security › Threat management: IDS/IPS and Honeypot rows are "not available" (fine, one row each).
- Control Plane › Router users "Single sign-on: RouterOS has no SSO" (fine).
- System › Safe apply: the select is disabled with the server ("5 minutes for now"). Show the value as text.

## 7. Proposed navigation

Rail (7, down from 8):

| Rail item | Contains | Comes from |
|---|---|---|
| **Dashboard** | Sidebar (router, internet, health checks, speed test, ad blocking) + cards | Same, with the link fixes in §5.7 and the card renames |
| **Devices** | List · Map · Ports · Radios · Updates | Devices (MikroTik Devices → List), Topology (→ Map), Control Plane › Updates (→ Updates), Supervisor (split, see §6) |
| **Clients** | Online · Offline · Leases · Groups (filters All / Wi-Fi / Wired / Guests) | Same; Profiles › Speed limit profiles join Groups |
| **Insights** | Applications · Clients · Domains · Flows (with countries) | Insights minus the duplicate chart and the Countries tab |
| **Events** | Events · Alarms (incl. CMR alerts) · Notifications | "Logs & Alarms", Supervisor alerts |
| **Guest portal** | Portal · Vouchers · Sessions | Hotspot minus Payments |
| **Settings** | Wi-Fi · Networks (+ Local DNS records) · Internet · VPN · Security · Routing · Profiles · Access · System (+ Backups, Blueprint & drift, Tools) | Settings; Control Plane → Access + System |

Side panels stay as they are (client, device, neighbour; contents per [device_client_views.md](device_client_views.md)).
⌘K indexes pages, tabs, settings sections (by title) and their aliases, so "port forward" opens Settings › Routing
at that section.

Where every current item goes (only the ones that move):

| Today | Proposed |
|---|---|
| Topology | Devices › Map (`#/topology` alias) |
| Devices › MikroTik Devices | Devices › List |
| Devices › Supervisor › router line, CMR alerts and rules | Events › Alarms (CMR alerts section); router panel issues |
| Devices › Supervisor › Config drift stub | deleted (link to System › Blueprint & drift) |
| Control Plane › Updates | Devices › Updates |
| Control Plane › Routers, CMR controller | per [multi_device.md](multi_device.md) §4 and step 3 |
| Control Plane › Backups, Blueprint & config drift | System |
| Control Plane › Console | System › Tools |
| Control Plane › Access mode, users, SSH keys, API keys, MCP | Settings › Access |
| System › Debug tools | System › Tools |
| Insights › Clients › Internet activity | deleted (Dashboard › Internet Activity) |
| Insights › Countries | Insights › Flows (section) |
| Logs & Alarms › Notifications › Remote syslog | link to System › Remote logging |
| Routing › Local DNS records | Networks |
| Profiles › Speed limit profiles | Clients › Groups (limit per group) |
| Hotspot › Payments | deleted |
| Hotspot › Guests | Guest portal › Sessions |
| Top bar app pill and icons | deleted (rail) |

## 8. Plan

Small steps, each shippable on its own with `npm run check`, the build and the tests clean. Effort: S ≤ ½ day,
M ½–2 days, L more. Steps 0–2 change no structure.

| # | Step | Files | Effort |
|---|---|---|---|
| 0 | **Shipped with this note** (labels only): Events tab "Alarm Manager" → "Alarms" (and the AlarmManager fallback title); Dashboard "Security Checks" → "Health Checks"; Control Plane "Changes from tikontrol" → "Access mode"; Networks pointer "Routing → DNS" → "Routing → Local DNS records"; Clients "Discover devices" → "Identify clients"; Supervisor "alerts are in Logs & Alarms" → "alarms are in Logs & Alarms → Alarms" | 7 Svelte files | done |
| 1 | More labels: Topology "All connected devices" → "Connected clients", "network devices" → "devices"; Leases "Make fixed" → "Fix IP"; Offline column "Device" → "Client", "Forget device" → "Forget client"; Hotspot tab "Guests" → "Portal sessions"; Devices tab "MikroTik Devices" → "Devices"; Dashboard "Traffic Identification" → "Top Traffic", "Most Active Access Points" → "Most Active Radios"; page title "Logs & Alarms" → "Events" (rail, h1, Supervisor text) | Topology, Clients, Hotspot, Devices, Dashboard, App, Events | S |
| 2 | Dashboard link fixes (§5.7): Internet Health → ISP status; WiFi Technology rows → Clients with a band filter; WiFi Connectivity → Events with `keep('events.cat', 'WiFi')`; radio tiles → Devices › Radios; Ad Blocking shows "Off" without a list | Dashboard, Clients (band filter) | S |
| 3 | Remove duplicates: Insights "Internet activity" section; Notifications syslog form (link instead); Supervisor drift stub (link instead); Payments tab; top bar app pill | Insights, Events, Devices, Hotspot, App | S |
| 4 | ⌘K: index tabs and settings sections from one list exported by `Settings.svelte` (remove the hand-kept copy) and each page's tab list; open at the section's `id` | Palette, Settings, Section (ids) | M |
| 5 | Updates in one place: move channel, install and update window into Devices › Updates; Control Plane keeps a one-line link until step 8 | Devices, ControlPlane | S |
| 6 | Topology as Devices › Map (`#/topology` and `#/devices/map` both work; rail 8 → 7). Do it after [multi_device.md](multi_device.md) step 4 if that lands first, so the map is moved once | App, Devices, Topology | S–M |
| 7 | Supervisor split: CMR alerts and rules into Events › Alarms, router line into the router panel issues; remove the tab | Devices, Events, AlarmManager | M |
| 8 | Settings regroup: Control Plane → **Access**; Backups, Blueprint & drift and Tools (Debug tools + Console) → System; Local DNS records → Networks; old `#/settings/control` alias → `access` | settings/*, Settings, Palette | M |
| 9 | Client groups own their limit: Profiles › Speed limit profiles becomes the group's limit field; groups listed in the client panel with "Add to group" | Clients, Profiles, ClientDrawer (coordinate with [device_client_views.md](device_client_views.md) step 6) | M |
| 10 | Offline clients open the client panel (offline state: alias, maker, last seen, Forget, Fix IP); Leases link known offline MACs | Clients, ClientDrawer, App (`isPanel`) | M |
| 11 | Cross-links: Traffic rules ↔ client panel; isolation list → panel; "Port forward to this client" prefill; "Block for one client" from Domains | Security, Routing, Insights, ClientDrawer | M |
| 12 | Wi-Fi spelling: add a display mapping for the `WiFi` event category and the `WiFi 5` standard strings, then change labels app-wide; tests updated with it | data.ts, hardware.ts, events, tests | S–M |
| 13 | Rail "Hotspot" → "Guest portal"; Insights Countries into Flows | App, Hotspot, Insights | S |
| 14 | Update `docs/FEATURES.md` page names (P3, P9–P12, P20 still say Traffic, Alerts, Events, Guest portal, System) and run `src/scripts/progress.py` | docs | S |

Order: 0–3 are safe today. 4 and 5 are independent. 6 waits for the multi-device topology. 7–11 can go in any
order, one at a time. Every step keeps old hashes working through `alias` in `App.svelte`, so bookmarks and the
links in docs keep working.

## 9. Checks for each step

- `npm run check` and `npm run build` in `src/ui`, `node --test tests/*.test.ts`.
- Open each moved route and its old alias on `sample.json` (no `snapshot.json`) at 1400 px and 375 px, light and
  dark; keyboard through the rail, tabs and panels (visible focus, WCAG 2.2 AA).
- For anything that moves a Safe apply action: the same intent kind, the same review text, and a CHR run of
  `src/scripts/chr-e2e.py` where that row exists.
