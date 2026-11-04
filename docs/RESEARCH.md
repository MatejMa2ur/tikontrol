# Research: a UniFi-like UI hosted on MikroTik (state as of 2026-11-02)

## 1. Prior art

| Project | What it is | Runs on-router? | Notes |
|---|---|---|---|
| **[MikroTik CMR](https://manual.mikrotik.com/docs/management-tools/cmr/)** (official, 7.26) | Fleet controller: inventory, dashboard, app traffic, WiFi/VLAN provisioning, topology, upgrades, alerts | Server = `cmr` package on arm/arm64/x86 (incl. CHR). Client is built into RouterOS. | No UI except WinBox 4/WebFig. **This is the backend we build on.** |
| [nasnet-panel](https://github.com/nasnet-community/nasnet-panel) | React SPA + Go API, full management panel (interfaces, DHCP, VPN, wireless, wizard) | **Yes**, small container, amd64/arm64/arm-v7 | MIT, pre-1.0, needs RouterOS 7.24+. **Closest to this idea.** |
| [MikroDash](https://github.com/SecOps-7/MikroDash) | Go single binary, live monitoring, alerts, config templates, provisioning | Docker image; on-router use not documented | MIT, about 640★, very active. Uses the binary API and WebSocket. |
| [routeros-lanpulse](https://github.com/mrtian2016/routeros-lanpulse) | Live "who is using bandwidth" page | Off-router (Prometheus) | Shows the useful tricks: per-device traffic via `kid-control`, 1 s resolution via the binary API. |
| [apehost-mikrotik-controller](https://github.com/JungleM0nkey/apehost-mikrotik-controller) | Dashboard, network map, AI diagnostics | Off-router (Node) | Work in progress, not production-ready. |
| [Grafana-Mikrotik](https://github.com/IgorKha/Grafana-Mikrotik) | Prometheus/SNMP dashboards | Off-router | Monitoring only. |
| [ROSM](https://forum.mikrotik.com/t/rosm-free-open-source-dashboard-to-manage-a-fleet-of-mikrotik-routers-macos-windows/271303), [mikrotik-cockpit](https://forum.mikrotik.com/t/a-small-local-ui-for-the-everyday-routeros-tasks-that-scare-beginners-off-winbox-open-source-ssh-only-feedback-wanted/273060) | Fleet manager / beginner UI over SSH | Desktop | Hobby projects. |

**Takeaway:** nobody has built a polished "UniFi Network" equivalent yet. nasnet-panel already proves that the on-router container part works. Decide whether to fork it or build fresh before writing any code.

## 2. Platform limits

### Containers ([manual](https://manual.mikrotik.com/docs/containers/))
- You need the `container` package and an **arm, arm64 or x86** CPU. **MIPS boards are out** (e.g. the classic hEX RB750Gr3 and older hAPs). The EN7562CT (hEX refresh) only runs **arm32v5** images.
- Enabling it needs **physical access**: `/system/device-mode/update container=yes`, then press the reset button or cold-boot.
- Pulling an image needs a lot of free RAM. Boards with small flash should load a `docker save` tarball from USB instead.
- Put root-dir and volumes on an **external disk** (USB/NVMe), not built-in flash.
- Per-container RAM caps: `memory-max` (hard limit, kills the container) and `memory-high` (soft limit, throttles it).
- Networking is a veth on its own bridge plus NAT, and you dst-nat the UI port. The L2/LAN-bridge mode exposes every port, so avoid it.
- No security guarantee from MikroTik. Use a small trusted image and lock down the API user.
- `start-on-boot=yes`, `logging=yes` sends output to `/log`. Healthcheck needs 7.23+.

### REST API ([docs](https://help.mikrotik.com/docs/spaces/ROS/pages/47579162/REST+API))
- Needs the `www-ssl` service (plain HTTP `www` only from 7.9). Uses HTTP Basic auth.
- GET=print, PUT=add, PATCH=set, DELETE=remove, POST=any command.
- **Hard 60 s timeout, no continuous/streaming commands.** `monitor` only works with `once`.
- All values come back as strings.
- **So:** REST is fine for CRUD. Live graphs, log tail and events need the **binary API (8728/8729)** with `listen`/`follow`.

### Router resources
- Router CPU and RAM are shared with routing. Polling heavy tables (connection tracking with 10k+ entries, full routing tables) costs CPU. Prefer `listen` over polling and cache in the container.
- Target **≤ 10 MiB RSS** and a **≤ 10 MB image** for the container: a static Rust (musl) binary with the SPA embedded, in a `scratch` image (see PLAN.md §6).
- If the router reboots, the UI goes down with it. That's acceptable, because WinBox stays the fallback.

### Safety
- REST and API have no Safe Mode, so one bad VLAN or firewall change can lock you out.
  **Must-have:** "apply with auto-revert". Before a risky change, export the config, schedule a revert in N minutes via `/system/scheduler`, and cancel the revert when the user confirms from the UI.
- Use a dedicated RouterOS user and group with only the needed policies. Allow the API only from the container IP.

## 3. UniFi features vs what RouterOS can back

✅ doable · ⚠️ partial or heavy · ❌ not realistic

| UniFi feature | Verdict | How on RouterOS |
|---|---|---|
| Dashboard (WAN throughput, CPU, uptime, client count) | ✅ | `/system/resource`, `/interface/monitor-traffic` via binary API |
| Client list (name, IP, MAC, vendor, wired/wireless, signal) | ✅ | DHCP leases + ARP + `/interface/wifi/registration-table` + bridge hosts; offline OUI DB for vendor |
| Per-client live traffic | ✅ | `/ip/kid-control/device` counters (diffed), or simple queues |
| Per-client history (days/weeks) | ✅ | Container stores samples in SQLite on the external disk |
| App/DPI breakdown (Netflix, YouTube…) | ⚠️ | **CMR (7.26) has app classification per device** (`/cmr/device/apptraffic`; not on mmips/smips/powerpc). Per-client: "top domains" from the DNS cache + Traffic Flow. See PLAN.md §0.5. |
| Block / pause client, bandwidth limit | ✅ | kid-control pause, address-list drop, simple queues |
| Networks (VLAN + subnet + DHCP + isolation) in one form | ✅ (hard) | Compose bridge VLAN + IP + DHCP pool/server + firewall rules. Needs auto-revert. |
| Wi-Fi SSIDs, guest Wi-Fi | ✅ | `/interface/wifi` (new wifi package) or legacy `wireless`, depending on hardware |
| Manage APs centrally | ✅ | CAPsMAN (built in), UI on top |
| Adopt/manage other MikroTik switches and routers | ✅ | Discover via `/ip/neighbor` (MNDP/LLDP), then manage each one over its API |
| Topology map | ⚠️ | Neighbors + bridge host tables. Good for MikroTik gear, weak for third-party gear. |
| Firmware updates (single or fleet) | ✅ | `/system/package/update check-for-updates` / `install` |
| Firewall rules, port forwarding | ✅ | filter / NAT rules, simplified UI |
| VPN: WireGuard server + QR codes, site-to-site | ✅ | `/interface/wireguard` |
| Hotspot / captive portal, vouchers | ✅ | `/ip/hotspot` + User Manager package |
| Ad-blocking | ✅ | `/ip/dns` adlist (recent v7) |
| IDS/IPS (Suricata-style threat management) | ❌ | No IDS on RouterOS. Sniffer-stream to a Suricata container would be too heavy for most boards (maybe RB5009/CCR/x86). |
| Speed test | ⚠️ | Container runs a speedtest client. It measures through the container NAT path. |
| Alerts (device offline, WAN down, new client) | ✅ | Container watches events and sends to email/ntfy/Telegram. `/tool/netwatch` as a helper. |
| Event log | ✅ | `/log/print follow` over the binary API |
| Remote access (like unifi.ui.com) | ⚠️ | WireGuard / MikroTik Back To Home, or a tunnel container. No cloud of our own. |
| Mobile app | ❌ | Responsive PWA instead |
| Multi-site | ✅ | One container can talk to several routers' APIs |

## 4. Proposed stack (unless we fork nasnet-panel)

- *Superseded:* the stack decision is in PLAN.md §6 (Rust + Svelte, one binary).
- **Image:** `FROM scratch`, multi-arch (arm/v5, arm/v7, arm64, amd64). Also publish a tarball for boards that can't pull.
- **Data flow:** browser ⇄ WebSocket ⇄ container ⇄ binary API (live) / REST (CRUD) ⇄ RouterOS.

## 5. The target router: hAP ac²

| | |
|---|---|
| Model | **hAP ac²**, IPQ4018, quad-core ARM → `linux/arm/v7` images |
| RAM | **128 MiB total** |
| Flash | **16 MiB total** |
| Disks | None built in. The board has a USB 2.0 port, so a USB stick is needed for any container. |
| Packages | `wireless` (legacy) and `container` |

**Verdict:** containers technically work, but the RAM left after RouterOS rules out a full controller on the router. That rules out SQLite history, Traffic Flow collection, and many WebSocket clients. A tiny Rust binary capped at about 16 MiB `memory-max` should run (a lite profile with no history), but it would compete with routing and Wi-Fi for the same RAM. Images must be side-loaded from a USB stick as tarballs, because pulling them needs free RAM.

**Recommended:** build it as a normal multi-arch Docker image and **run it on a separate home server** for now. The code is identical and all features are available. Keep the "on-router" target as a build profile with no history DB, for routers with ≥ 1 GB RAM (e.g. hAP ax³, RB5009).

## 6. Next steps

1. Confirm the hardware model, RouterOS version and an external disk (`/system/resource/print`).
2. Spend about an hour running nasnet-panel on the router, then decide whether to fork it or build fresh.
3. MVP: dashboard + client list + per-client traffic. All of it is read-only, so there's no lockout risk.
4. Then write features behind auto-revert: networks/VLANs, Wi-Fi, port forwards.
