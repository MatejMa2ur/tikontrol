# Patterns from a self-hosted UniFi Network Application (2026-11)

We ran UniFi Network Application 10.6 locally in Docker, bound to localhost, with a throwaway account, made-up
networks and three fake offline devices. Nothing on a real LAN was discovered or adopted. Agents walked every page,
tab, panel and form and compared it with tikontrol running on `sample.json`.

These notes describe patterns in our own words. No screenshots, text, icons or layouts were copied into the repo, and
tikontrol stays a personal MikroTik tool, not a UniFi replacement. Effort S/M/L, value 1–3.

Not reachable without a real gateway: the zone-based firewall UI (described from the app's labels only), the ports
page, and any live data (charts, client details, radio stats).

## 1. Navigation: side panels over the page

UniFi opens item details and create/edit forms in a right-hand panel. On wide screens the page behind stays usable:
no dimming, the open row is highlighted, and clicking another row swaps the panel.

| Idea | Where | Effort | Value |
|---|---|---|---|
| Client details open over the current page instead of switching to Clients | `App.svelte`, `ClientDrawer.svelte` | S | 3 (done) |
| No scrim on wide screens; highlight the open row; clicking another row swaps the panel; keep the scrim on phones | `ClientDrawer.svelte`, device drawer | M | 3 |
| One device panel for the router, CMR devices and neighbours, routed as `#/devices/<id>`, opened from Devices, Topology and search | `Devices.svelte`, `Topology.svelte`, `Palette.svelte` | S–M | 3 |
| Settings create/edit (network, Wi-Fi, rules, VPN peers) in the same panel with a sticky footer, instead of a centred modal | `pages/settings/*` | M | 3 |
| Port details as a panel step instead of a modal over the device drawer | `Ports.svelte` | M | 2 |

## 2. Forms and applying changes

| Idea | Where | Effort | Value |
|---|---|---|---|
| One form per panel: track changes, show Cancel / Review & apply only when something changed, as one batch | device Settings tab, `settings/System.svelte` | S | 3 |
| Give every disabled primary button a visible reason | Hotspot, Setup, System | S | 2 |
| "This will: …" consequence lines in the Safe apply review, not only a risk label | `lib/ui/SafeApply.svelte` | S | 3 |
| Warn at the risky control the moment it is changed, naming the uplink port or VLAN at risk | `Ports.svelte`, VLAN settings | M | 3 |
| Pause (`disabled=yes`) instead of delete for SSIDs, VPN peers, rules, routes and port forwards | settings pages | S | 3 |
| Auto/Manual advanced blocks that show the effective defaults greyed out instead of a collapsed `<details>` | settings forms | S | 2 |
| Ask for the current password again before role changes, read & write, API keys and MCP; checked on the server | `api.rs`, Control plane | M | 3 |

Safe apply gaps found while comparing (fix soon):
- No warning when leaving or closing the tab while a change waits for "Keep".
- `review()` in `lib/apply.svelte.ts` doesn't refuse a second review while a change is armed.
- The server should flag input-chain, `/ip/service` and management-address changes as high risk itself, not only the UI.

Where tikontrol is already ahead: exact commands and their undo are shown; the revert runs on the router and survives
a closed browser; the VLAN-filtering checklist and input-chain warning go further.

## 3. Networks, firewall, ports

| Idea | RouterOS backing | Effort | Value |
|---|---|---|---|
| New network: subnet calculator, next free VLAN/subnet suggested, overlap checks on client and server | `/ip/address`, `/interface/vlan` | S | 3 |
| DHCP used/free per network, amber when nearly full | `/ip/dhcp-server/lease`, `/ip/pool/used` | S | 3 |
| Leases tab under Clients: type, expiry, network filter, make fixed, CSV | `/ip/dhcp-server/lease` (`expires-after`, `dynamic`) | S–M | 3 |
| Zone matrix cell filters the rule list to that pair and shows its rule count | firewall filter | S | 3 |
| Hit counters on firewall, NAT and port-forward rows | `bytes`, `packets` fields | S | 2 |
| Lock icon and read-only panel for rules tikontrol didn't create | comment `tik:` tag | S | 2 |
| Port forward: limit the source; warn when the target has no fixed IP, offer "make it fixed" in the same batch | `/ip/firewall/nat`, leases | S | 2 |
| Port problem badges in plain words: errors rising, flapping, blocked by STP/loop protect, PoE fault, slow link | interface stats `link-downs`, `rx-fcs-error`; bridge port `role`/`status`; `loop-protect-status`; PoE monitor | M | 3 |
| Hotspot walled garden and "block private networks for guests" on by default | `/ip/hotspot/walled-garden`, firewall | M | 3 |
| Split the long Security page into sub-sections; add a Settings overview | `settings/Security.svelte` | S | 2 |

## 4. Devices, topology, radios

| Idea | Where | Effort | Value |
|---|---|---|---|
| Issues card at the top of device Overview: one headline and its fix per problem (firmware, memory, old RouterOS, offline, port errors) | `Devices.svelte` | S | 3 |
| "Manage" block at the end of device Settings: locate, restart, mode, each with its consequence | `Devices.svelte` | S | 2 |
| "Connected via" column and topology sort in the device list | `groupByNeighbor` | S | 2 |
| Stats tab with 1D/1W/1M ranges and per-device event history | history store | S–M | 2 |
| Topology label toggles (port/speed, band, signal, SSID) and a status filter, fewer labels by default | `Topology.svelte` | S | 2 |
| Channel plan strip, spectrum chart for scans, PoE budget meter | Radios, Ports | M | 2 |

Bugs found: device-drawer ports tab clips client chips; the device drawer stays open across Devices tabs and its scrim
blocks Ports; Status is the last column and hides under the drawer at 1440 px; Topology header missing in a full-page shot.

## 5. Dashboard, events, alerts, clients

| Idea | Where | Effort | Value |
|---|---|---|---|
| Log every tikontrol sign-in (success, failure, lockout, method, source) as an Admin event | `src/server/src/api.rs` | S | 3 |
| Event rows with a MAC open the client panel (`Ev.mac` exists, UI drops it) | `Events.svelte`, `lib/data.ts` | S | 3 |
| Severity filter chips with counts; time range presets + `datetime-local`; validated server params | `Events.svelte`, `/api/v1/events` | S–M | 3 |
| Fold repeated events into one row with ×N and first/last time (pure function + test) | `Events.svelte` | S–M | 3 |
| "Showing 500 of N" when the events endpoint caps | `/api/v1/events` | S | 2 |
| Latency and outage marks on the internet activity chart | `live.rs` | M | 3 |
| More feature-detected alerts: DHCP pool full, port/PoE down, health limits, certificate expiry, low disk | `alerts.rs`, `AlarmManager.svelte` | M | 3 |
| "Recent changes" dashboard card linking to Admin events | Dashboard | S | 2 |
| Make the dead › chevrons on Dashboard cards real links with filters prefilled | `Dashboard.svelte` | S | 2 |
| Empty states inside the chart frame with a time-to-data estimate | charts | S | 2 |
| Client panel: ping, IPv6 addresses, lease expiry | `ClientDrawer.svelte`, `/ipv6/neighbor` | S | 2 |
| Backup schedule (interval, time, keep N) instead of one weekly button | `settings/System.svelte` | M | 2 |
| Admin detail panel: last login, role, password reset, alert channels | Control plane | M | 2 |
| Show-password toggle on Login and Setup; year in event times | Login, Setup, Events | S | 1 |

## Patterns not to copy

Recurring upsell modals, account sign-in pushed during setup, version shown before login, faint disabled filters,
icon-only toggles without labels, and a 60-event × 2-channel notification matrix (our per-channel minimum severity
with quiet hours is simpler).
