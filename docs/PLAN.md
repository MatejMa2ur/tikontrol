# Plan: a UniFi-style app for MikroTik

## 0. Ground rules for the "reverse engineering"

- **Clean-room.** We study UniFi's *behaviour*: what pages exist, what each one answers, and which workflows it collapses into one form. Sources are public docs, release notes and the official Integration API shape.
- We don't copy UniFi code, icons, illustrations, wording or trade dress. The visual language is our own, MikroTik-flavoured (§5).
- Every UniFi feature gets mapped to the RouterOS objects that back it. If RouterOS can't back a feature, it isn't in the plan, or it's marked partial.

## 0.5 MikroTik CMR changes the plan

[CMR](https://manual.mikrotik.com/docs/management-tools/cmr/) (RouterOS 7.26) is MikroTik's own fleet controller. It's the *backend* half of a UniFi controller, but its only UI is WinBox 4/WebFig.

| CMR does | CMR does not |
|---|---|
| Inventory, pairing, labels; dashboard (CPU/RAM/disk, bandwidth, alerts, APs/stations per band, interfaces) | IP addressing, DHCP, routing, firewall, users. It won't adopt or edit existing config. |
| **Application traffic** per device: `/cmr/device/apptraffic`, per app or category, 1 min / 1 h / 1 d / 1 w (not on mmips/smips/powerpc) | Per-*client* app breakdown (not documented) |
| WiFi networks + radios applied by labels; VLAN access/trunk port provisioning | Roaming coordination, central auth |
| Topology layouts (`/cmr/layout`, `rebuild-links`) | Polished UX: WinBox 4 tables only |
| Firmware upgrade rules (channel/pinned, sequential/parallel, fail policy) | |
| State/event alerts → log / script / webhook | Email/push, unless you go through a webhook |

**On the hAP ac²:** the CMR **client** is built in (`/cmr/client enabled: no`). The **server** needs the separate `cmr` package (arm/arm64/x86). With so little free flash and RAM, the hAP ac² isn't a realistic server. The server **doesn't have to be the gateway**, though. Clients reach it over routed networks, NAT or VPN, discovering it via MNDP, `_cmr._tcp.lan` DNS, or `controller-addresses`.

**New positioning:** tikontrol = **the UniFi-style UI that CMR doesn't have**, plus the per-router config CMR deliberately skips.

```
browser ─ tikontrol ─┬─ CMR server API (/cmr/...)   → devices, dashboard, apptraffic, wifi, vlan, layout, upgrades, alerts
                     └─ each router's API directly  → clients, DHCP, firewall, NAT, routing, WireGuard, queues, kid-control
```

- **Don't rebuild what CMR does:** Devices, Topology, WiFi provisioning, firmware, alerts and app traffic become views and forms over `/cmr/*`. That deletes a big chunk of Phases 2–4 below.
- **Where the CMR server runs:**
  - today: **CHR (x86) as a VM on a home server**, with the hAP ac² as a client. Caveat: the free CHR tier is speed-limited, which is likely fine for management traffic but needs verifying.
  - later: on the hAP be³ Media itself (arm64, 2 GB).
- CMR alert webhooks → tikontrol → email/ntfy/Telegram.
- **Insights page becomes partly real:** CMR app classification replaces the "no DPI" gap at the device level. Per-client stays kid-control + DNS correlation.

## 0.6 Simplicity rules (the actual product)

Features are table stakes; CMR and RouterOS already have them. **What we're building is the simplicity.** Every screen has to pass these:

1. **Intent, not objects.** The user says "IoT network, VLAN 30, isolated". We generate the bridge VLAN, interface, address, pool, DHCP server, DHCP network, interface list and firewall rules. RouterOS takes about 9 objects for that; our form has about 5 fields.
2. **Right defaults, advanced hidden.** Each form works with only the first block filled in. "Advanced" is collapsed and remembered per user.
3. **Names, not identifiers.** "Alex's iPhone 📱", not `AA:BB:CC:DD:EE:FF`. MAC, IP and lease details live in the client drawer.
4. **Status first.** Every page opens with "is it OK?": green/amber/red plus one sentence. Numbers come second.
5. **Few pages.** At most about 8 top-level items. If a feature needs a new nav entry, it's probably a tab somewhere else.
6. **Live by default.** Counters, clients and signal update without refresh.
7. **Safe to click.** Risky changes go through Safe apply (§3). Nothing can lock you out permanently.
8. **Plain words.** "Block", "Pause internet", "Limit speed", not "add to address-list", "kid-control", "simple queue".
9. **The router is one click away, never in your face.** "Show in RouterOS" reveals the exact commands. That's where we differ from UniFi: simple by default, transparent on demand.

## 1. UniFi Network information architecture → tikontrol

UniFi Network 10.x (latest 10.6.109, Oct 2026) is organised as a left nav with **monitoring pages** plus a **Settings** tree. We mirror the *structure*, because that structure is what makes it feel easy.

### Monitoring pages

| UniFi page | What it answers | tikontrol page | RouterOS backing | Phase |
|---|---|---|---|---|
| **Dashboard** | Is the internet OK? How busy? Who's on? | Dashboard | `/system/resource`, `/interface/monitor-traffic` (WAN, binary API stream), DHCP/ARP counts, `/ip/cloud` public IP (with `.proplist`, never print secrets), `/tool/netwatch` for WAN health | 1 |
| **Topology** | How is everything connected? | Topology | `/ip/neighbor` (MNDP/LLDP/CDP) + `/interface/bridge/host` (11 entries) + wireless reg-table → derived tree: WAN → router → port/SSID → client | 2 |
| **UniFi Devices** | My APs, switches, gateways, firmware | Devices | This router + discovered MikroTiks (managed by their API). Firmware: `/system/package/update` (channel `development`), `/system/routerboard` | 3 |
| **Client Devices** | Who's on my network, how much, how good the signal is | Clients | DHCP leases + ARP + `/interface/wireless/registration-table` (signal, tx-rate, uptime) or `/interface/wifi/registration-table` on new hw + bridge hosts; offline OUI DB for vendor; user-editable alias/icon stored in our DB | 1 |
| Client detail | One client's history and actions | Client drawer | Per-client bytes via `/ip/kid-control/device` counters (needs a catch-all kid-control profile); history from our SQLite. Actions: block (address-list + drop rule), pause (kid-control), rate-limit (simple queue), fixed IP (make lease static), rename | 1–2 |
| **Ports** | Port status, speed, PoE, which client is where | Ports | `/interface/ethernet` (+ `monitor once` for speed), bridge host → port, `/interface/ethernet/poe` where present | 2 |
| **Radios / Radio Manager** | Channels, utilisation, interference, roaming | Radios | Legacy `/interface/wireless` (+ `monitor once`, `scan` duration-limited) or `/interface/wifi`; roaming = registration-table changes logged over time | 3 |
| **Insights / Traffic Flows** | Which apps and countries, top talkers | Traffic | ⚠️ No DPI. Top talkers per client (kid-control) + **top domains** (DNS cache snapshot, correlated with client by connection tracking) + optional Traffic Flow/IPFIX into our collector (disabled, CPU cost: opt-in) | 3 |
| **Alarm Manager** | Notify me when X | Alerts | Our rule engine on our own data: WAN down, device offline, new client, CPU/RAM high, firmware available → email / ntfy / Telegram / webhook | 2 |
| **System Log / Events** | What happened? | Events | `/log/print follow` (binary API) + our own derived events (client joined/left, WAN flap) | 1 |

### Settings tree

| UniFi setting | tikontrol | RouterOS backing | Risk | Phase |
|---|---|---|---|---|
| **WiFi** (SSIDs, security, guest, band steering) | WiFi | Legacy: `/interface/wireless` + `security-profiles` + virtual APs. New hw: `/interface/wifi` + `configuration`/`security`/`datapath` | med | 2 |
| **Networks** (name + VLAN + subnet + DHCP + isolation, *one form*) | Networks | Composite write: bridge VLAN filtering + `/interface/vlan` + `/ip/address` + `/ip/pool` + `/ip/dhcp-server` + `/ip/dhcp-server/network` + interface-list membership + firewall isolation rules | **high** (lockout) | 3 |
| **Internet** (WAN, failover, load balance) | Internet | `/ip/dhcp-client` / PPPoE / static; failover via route `distance` + `check-gateway` or netwatch | **high** | 3 |
| **VPN** (server, client, site-to-site, Teleport) | VPN | `/interface/wireguard` + peers, QR export generated in the browser (keys never stored in logs); Back To Home ≈ Teleport (`/ip/cloud`) | med | 2 |
| **Security → Zone-based firewall** | Firewall | RouterOS has no zones. **Zones = interface lists** (`WAN`, `LAN` exist, we add `GUEST`, `IOT`…). A zone matrix compiles to `/ip/firewall/filter` rules with `in-interface-list`/`out-interface-list`. 16 filter rules exist. We only manage rules carrying our comment tag and leave the rest alone. | **high** | 3 |
| Security → simple app/domain blocking | Blocking | `/ip/firewall` + `tls-host` (SNI) matches, DNS static `NXDOMAIN` / adlist | low | 3 |
| Security → ad-blocking | Ad-block | `/ip/dns/adlist` (show its RAM cost) | low | 2 |
| Security → region blocking | Geo-block | Address lists from a public country-IP feed, refreshed by us | low | 4 |
| Security → IDS/IPS, honeypot | ❌ | Not backed by RouterOS. Out of scope (maybe a Suricata container on big hardware later) | – | – |
| **Routing** (static, policy routes, port forward) | Routing / Port forwarding | `/ip/route`, `/routing/rule` + mangle, `/ip/firewall/nat` dst-nat (2 NAT rules) | med | 2 |
| **Profiles** (bandwidth/QoS, port profiles) | Profiles | Simple queues / queue tree; bridge port VLAN settings | med | 3 |
| **Hotspot** (portal, vouchers) | Guest portal | `/ip/hotspot` (+ User Manager package for vouchers) | med | 4 |
| **Control plane / System** (admins, backups, updates, remote access) | System | `/user`, `/system/backup` + `/export`, `/system/package/update`, `/ip/cloud` BTH, service hardening (telnet/ftp/www currently **on**) | med | 2 |
| **Integrations** (local API) | API | Our own REST API, shaped like UniFi's `/v1/sites/{site}/devices|clients`, so it's easy to script and to point Home Assistant at | low | 4 |

### Explicitly not doing
DPI app signatures, IDS/IPS, a native mobile app (a responsive PWA instead), a Ubiquiti-style cloud portal, and adopting non-MikroTik hardware.

## 2. Architecture

```
browser (SPA, PWA)
   │  HTTPS + WebSocket
tikontrol (one Rust binary, one container)
   ├── poller/streamer ── binary API 8728/8729 ── RouterOS (live: monitor-traffic, log follow, listen)
   ├── writer ─────────── REST /rest (CRUD) or binary API
   ├── store ──────────── SQLite (aliases, history, events, alert rules)
   └── notifier ───────── email / ntfy / Telegram / webhook
```

- **One process, one binary, one container image** for linux/amd64, arm64, arm/v7 and arm/v5. A home server today, hAP be³ Media later.
- **RouterOS is the source of truth.** Our DB holds only what RouterOS can't hold: aliases, history, alert rules. On start and on reconnect we re-read config. We never cache config and write it back blind.
- **Ownership tagging:** every object we create gets `comment="tik:<id>"`. We edit only tagged objects and show untagged ones read-only ("managed outside tikontrol"). This is how the app coexists with WinBox.
- **Least privilege:** dedicated RouterOS user and group (`read,write,api,rest-api,test` — no `ftp`, `password`, `policy`, `sensitive`). The API is reachable only from the controller's IP. No `sensitive` policy also stops secrets like the BTH private key from coming back over the API.
- **History budget:** 1 s live (memory only), 1 min samples for 7 days, 1 h for 90 days. Size it at well under 100 MB of disk for a home network.

## 3. Safety: the "Safe apply" pipeline (non-negotiable before Phase 3)

Every risky write (networks, firewall, WAN, VLANs) goes through the same steps:

1. **Diff preview:** show the RouterOS commands that will run.
2. **Snapshot:** `/export` and save it in our DB.
3. **Arm revert:** add a `/system/scheduler` job that runs in 5 min and restores the snapshot (or reboots into a backup).
4. **Apply** the commands.
5. **Health check** from the browser and from the controller: can we still reach the router, the WAN, and the UI?
6. **Confirm:** the user clicks "Keep changes" and the scheduler job is removed. No click means it reverts automatically.

This replaces WinBox's Safe Mode, which the API doesn't offer.

## 4. Phases

| Phase | Scope | Writes? | Done when |
|---|---|---|---|
| **0 — Dev env** | Rust skeleton, SPA shell, connect to the router; **CHR (Cloud Hosted Router) in a VM or Docker on a home server as a disposable test router** | no | Shows router identity + uptime |
| **1 — Read-only UniFi feel** | Dashboard, Clients (with alias, vendor, signal), Events, per-client live traffic | aliases only (our DB) | You can leave it open on a tablet and it's useful |
| **2 — Safe writes** | Client actions (block, pause, limit, fixed IP), WiFi SSIDs, port forwards, WireGuard + QR, ad-block, backups/updates, alerts, Topology, Ports | yes, low/med risk | Common tasks are done in the UI, not WinBox |
| **3 — Structural** | Safe-apply pipeline, then Networks/VLANs, zone firewall, WAN/failover, QoS profiles, Radios, Traffic insights | yes, high risk, via Safe apply | You can create a "IoT" network with isolation in one form |
| **4 — Fleet & extras** | Manage other MikroTiks (CAPsMAN / per-device API), guest portal + vouchers, geo-block, public API, on-router image profile | yes | Second device is managed from the same UI |

## 5. "MikroTik style" design direction

- Dense but calm, like WinBox's data-first tables with a modern dashboard on top. Tables are first-class: sortable, filterable, column chooser.
- Our own palette, built around a MikroTik-like deep blue on neutral greys. Dark mode is default-capable. We don't use MikroTik's logo either; it's "for MikroTik", not "by MikroTik".
- Every object has a **"Show in RouterOS"** affordance that displays the exact CLI path and the commands we'd run. This is the main way we differ from UniFi: we don't hide the router, we explain it.

## 6. Stack

**Decision: Rust backend + Svelte frontend, both embedded in one binary.** Smallest footprint wins, because a hAP ac² has very little free RAM.

| Estimate (to be measured in Phase 0) | Rust | Go |
|---|---|---|
| Binary incl. SQLite + embedded UI | ~4–7 MB | ~12–18 MB |
| Idle RAM (RSS) | **~3–8 MiB** | ~15–30 MiB (GC) |
| Fits on hAP ac² under `memory-max=16M` | yes | barely or no |
| Toolchain on the dev machine | cargo already installed | not installed |

- **Backend:** `axum` + `tokio` (current-thread runtime, which keeps RAM low), `mikrotik-proto` (sans-IO RouterOS binary API, actively maintained) or our own ~200-line implementation, `rusqlite` with bundled SQLite, `rust-embed` for the UI.
- **Build:** `opt-level="z"`, `lto`, `codegen-units=1`, `panic="abort"`, `strip`. musl static targets: `x86_64`, `aarch64`, `armv7-…-musleabihf`, `armv5te-…-musleabi`, built with `cargo-zigbuild`.
- **Frontend:** Svelte 5 + Vite, compiled to static files, pre-gzipped and embedded. No component library: our own small CSS design system (§5), so the UI bundle stays well under 100 KB gzipped. Charts: `uPlot` (~45 KB) for live graphs.
- **Image:** `FROM scratch` + binary + CA certs. Target **< 10 MB**.
- Tests: unit tests on the "compile to RouterOS commands" layer (pure functions: desired state → command list), plus integration tests against CHR.

## 7. Open decisions

1. Build our own (this plan) or start from [nasnet-panel](https://github.com/nasnet-community/nasnet-panel) (MIT, Go+React, already runs on-router)? [MikroTik Manager](https://github.com/2GT-Media-Group-LLC/mikrotik-manager) is AGPL-3.0, so borrowing from it would make tikontrol AGPL too. **Recommendation:** build our own around the UniFi IA above, and read nasnet-panel for RouterOS edge cases.
2. Legacy `wireless` (hAP ac²) or only the new `wifi` package (hAP be³ Media)? **Recommendation:** abstract both behind one "SSID" model and do the legacy one first, because that's the hardware you have today.
