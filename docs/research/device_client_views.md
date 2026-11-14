# Client and device panels: fewer tabs, a clearer top (2026-11)

The owner likes how the side panels look, but feels they have more in them than they need. They also feel that UniFi's
device and client views read better. This note compares the two, says why, and proposes an original reorganisation
for tikontrol. **No capability is removed.** Everything that is reachable today is still reachable, only in a
different place.

Scope: `ClientDrawer.svelte`, `DeviceDrawer.svelte`, `NeighborDrawer.svelte` and the rows that open them in
`Clients.svelte` and `Devices.svelte`. App-wide navigation, page names and duplicate pages belong to
`docs/research/ia_audit.md`. Multi-device and switch support belongs to `docs/research/multi_device.md`. This note
only covers what goes inside a panel and keeps the device panel shell generic, so that a CMR device or a second
router can fill it later. UniFi is used here only as the comparison (a local study kept out of the repo; its notes
in the study's scratchpad). Nothing was copied: not its layout, icons, text or images.

Method: we read the three drawers and the two list pages and ran the UI on `sample.json` (fictional data, no
`snapshot.json`). We took screenshots at 1400 px and 375 px in light and dark and measured each panel's scroll height.
The values in the wireframes below are made up.

## 1. What the panels hold today

### Client panel (`ClientDrawer.svelte`, 3 tabs)

| Tab | Contents | Measured |
|---|---|---|
| Header | Icon, name (click to rename), maker · type subtitle, close | — |
| **Overview** | Live down/up in large type. **9 equal-weight buttons**: Pause/Resume, Limit (only switches to Settings), Fix IP, Reconnect, Traffic rule (jumps to Security), Isolate, Ping, Block/Unblock. Up to **19 fact rows**: experience, connection, standard, link rate, CCQ, IP, MAC, maker, model, type guess, hostname, DHCP vendor class, first/last seen, connected for, lease renews, total traffic, notes. Then "What the device says" (mDNS/SSDP), then "Why is it on 2.4 GHz?" with band history and up to 4 Safe apply fixes | 1612 px tall at 1400 px wide, 1735 px at 375 px (about 2 screens). 16 controls, 12 facts with sample data |
| **Insights** | 24 h traffic (simulated without the server, hourly bars with it), Roaming history, Recent activity | Fits one screen and is mostly empty |
| **Settings** | Icon picker, Notes (both stored in tikontrol), Fixed IP, Local DNS name, Network override (VLAN) with its own Remove, Own Wi-Fi password with its own **Set**/Remove, Speed limit with its own **Apply limit**/Remove, Scheduled pause, then one **Review & apply** for the rest | 18 controls and three separate ways to apply |
| Footer | "Show in RouterOS" commands | — |

### Device panel for this router (`DeviceDrawer.svelte`, 3 tabs)

| Tab | Contents |
|---|---|
| Header | Icon, identity, board · RouterOS version, close. **No status and no key numbers.** |
| **Overview** | Issues (update, RouterBOOT, memory, storage, heat), each with a link. Device face. 9 facts (model, hardware, IP, uptime, CPU, memory, storage, temperature, firmware). 24 h CPU/memory/temperature sparklines (server only) |
| **Ports** | `Ports compact`: port, status, speed, connected. At 375 px the Connected column is cut off |
| **Settings** | Name, LEDs, SSH (one dirty-tracked form with a sticky footer, which is good). **Manage**: Locate (with a port select), Restart, Use as a switch / Leave switch mode (a modal) |

### Neighbour panel (`NeighborDrawer.svelte`, no tabs)

A header and a "managed elsewhere" note. 7 facts, two of which (Name, Version) repeat the header. Open WebFig.
"Behind it" lists the clients and devices on that port. It is a separate component with its own layout, so it looks
like a different product from the router panel.

### What is duplicated or weak

1. **Client, the same events three times.** Band history (Overview › Why), Roaming history (Insights) and Recent
   activity (Insights) all show the same joins and leaves from the log. With sample data, each join appears three
   times.
2. **Client, actions point somewhere else.** "Limit" only switches tabs. "Fix IP" is both a button and a Settings
   field. "Pause" (now) is in Overview, but Scheduled pause is in Settings. Notes are shown in Overview but edited
   in Settings. Rename is in the header, but the icon is chosen in Settings.
3. **Client, there is no hierarchy in the actions.** Nine buttons of the same weight, three rows on a phone. Block is
   red, which is right, but Ping and Isolate carry as much weight as Pause.
4. **Client, the chart is behind a tab.** The single most glanceable thing (activity over time) sits on Insights,
   while the default tab opens with "0 bps" in 26 px type for an idle device.
5. **Client, the top is not about state.** The header says *what* the device is, not *how it is doing*: whether it
   is online, on which network, with what signal. You have to read the fact list to find out.
6. **Client, Settings applies in three ways.** The password and limit fields apply on their own, the rest applies as
   a batch. UniFi uses one Apply for the whole form, and tikontrol's own device Settings already does.
7. **Device, the header has no status or numbers.** "Is it fine?" is answered only by reading facts and the issue
   list. Firmware appears twice (as an issue and as a fact row), and Model and Hardware overlap.
8. **Device, the Ports tab doesn't fit a phone** (WCAG 1.4.10 reflow).
9. **Neighbour and router panels have different shapes**, even though the local UniFi study asks for one panel for
   every device.
10. **Tabs use different markup.** The device tabs have `role="tab"` but no arrow keys or `aria-controls`. The client
    tabs are plain buttons.

## 2. Why the UniFi views feel better

The data isn't richer. With fake offline devices, UniFi's panels had *less* to show than tikontrol's. The difference
is ordering and restraint:

- **Status leads.** The first card under the name is the problem, if there is one: a bold headline, one sentence and
  the fixing action. When nothing is wrong, there is no card. tikontrol's device Issues list already does this, but
  the client panel has no equivalent. Its "Why 2.4 GHz" sits at the bottom.
- **One small, type-specific chart near the top** (TX retries for an AP, activity for a client) with a −24 h…now
  axis. It is there on open, not behind a tab.
- **The property list is short and right-aligned, with only the rows that have data.** An offline device shows few
  rows, not dashes. Identifiers (MAC, OS, vendor class) are there, but below the state.
- **Actions are not on the overview.** Quick actions and Manage sit at the end of Settings as rows, each with its
  consequence. The overview stays something you read.
- **One Apply per panel.** Nothing applies per field. A sticky footer appears only when something changed.
- **The same panel component for every device type and every entry point** (list, topology), with the page behind it
  still usable.
- **Cards with headings, collapsible.** Rare things are folded, not deleted.

What tikontrol already does better, so keep it: exact commands and their undo (`Ros`, Safe apply), the issue lines
with plain causes, "What the device says", the 2.4 GHz explainer with real fixes, honest "simulated" and "no
sensor" notes, and the non-modal panel on wide screens (the scrim is already hidden from 1100 px).

## 3. Proposal

The principle: **the top answers "is it OK and what is it doing", the middle answers "what happened", and the rest is
one click away.** There are two tabs where there were three. Actions go in one compact toolbar with a "More" menu.
Rarely used identifiers go in a closed "Details" disclosure.

### 3.1 Shared panel shell (`lib/ui/SidePanel.svelte`, new, small)

Header, key-number strip, toolbar, optional tabs, body, sticky footer slot. The client, router, neighbour and later
CMR device panels all use it, which removes four copies of the focus, Escape and scrim code.

```
┌ header ─────────────────────────────────────────────┐
│ [icon] Name ✎                         [status] [×]  │
│        subtitle: what it is                         │
├ key numbers (2–4 tiles, live) ──────────────────────┤
├ toolbar: 2–3 primary actions · [More ▾] ────────────┤
├ tabs (only when there is more than one) ────────────┤
│ body (scrolls)                                      │
├ sticky footer (only when a form changed) ───────────┤
```

### 3.2 Client panel

**Header:** icon, display name with the rename pencil (unchanged), subtitle `maker · type`, and a **status chip**:
`Online`, `Paused`, `Blocked` or `Offline` (text, not colour alone).

**Key numbers** (3 tiles): **Experience** (pill + dBm, or "Wired"), **Down / Up now**, **Connected for**.
"Down / Up now" replaces the two oversized rate numbers.

**Toolbar:** `Pause`/`Resume` · `Block`/`Unblock` (danger style) · `More ▾`.
More holds: Reconnect (Wi-Fi only), Ping, Fix IP (hidden when already fixed), Isolate / End isolation, Add traffic
rule…, Show in RouterOS. "Limit" leaves the toolbar because it only switched tabs. The limit stays in Settings.

**Tabs: Overview · Settings.** Insights is merged into Overview.

**Overview, top to bottom:**

1. **Attention card** (only when needed): "Blocked: no network access", "Paused", or the band question ("Keeps
   switching bands"). Each has a one-line headline and a `See why` that unfolds the reasons and fixes in place (the
   current "Why" section, unchanged, with its Safe apply fixes and the minimum-signal input). Ping results also show
   here as a status line.
2. **Activity, 24 h**: the traffic chart moved from Insights (simulated area without the server, hourly bars with
   it) with a text summary under it ("Peak 94 kbps at 18:00 · 357 KB today").
3. **Connection** (at most 6 rows): Network (SSID · band · interface, or "Wired · port"), IP (fixed/DHCP), Wi-Fi
   standard and link rate on one row, Lease renews, Total traffic, Notes (when set, with an edit link that focuses
   the Notes field in Settings).
4. **Timeline**: **one** list made from band history, roaming history and recent activity, deduplicated by time and
   text, newest first, 8 rows, with `Show all` for the full list. It replaces three lists.
5. **Details** (`<details>`, closed): MAC, Maker (with the private-address explanation), Model, Type guess, Hostname,
   DHCP vendor class, First/Last seen, Link quality (CCQ), "What the device says" (mDNS/SSDP).
6. `Show in RouterOS` (as now, at the end).

**Settings, as one form with one Review & apply** (the sticky footer from the device panel):

- *In tikontrol only* (saved at once, says so): Name (the same alias as the header pencil), Icon, Notes.
- *On the router* (batched): Fixed IP, Local DNS name, Speed limit, Scheduled pause, Network override (VLAN), Own
  Wi-Fi password. Each existing "Remove" becomes a "Remove" row action that adds a removal to the same batch. The
  separate Set and Apply limit buttons go away. The batch intent already exists (`kind: 'batch'`). The password stays
  in the intent only, is never echoed, and the form clears it after review, as today.

Result: 2 tabs instead of 3, 3 visible actions instead of 9, about 7 visible facts instead of 19, 1 timeline instead
of 3, and 1 way to apply.

### 3.3 Device panel (router, neighbour, later CMR and other routers)

**Header:** icon, identity, subtitle `board · RouterOS x.y` (or `SwOS`), and a **status chip**: `Online`,
`Not managed here` (neighbour) or `Offline` (CMR, later).

**Key numbers** (4 tiles, router): **CPU %**, **Memory %**, **Clients**, **Uptime** (temperature takes the Uptime slot
only when ≥ 75 °C, and it is already an issue). Neighbour: **Port**, **Clients behind**, **Uptime**.

**Toolbar:** router: `Locate` · `Restart…` · `More ▾` (Use as a switch / Leave switch mode, Updates, Show in
RouterOS). Locate keeps its port picker, which moves into a small popover from the button. Neighbour: `Open WebFig`
(when there is an address) · `Set up…` (the existing guide, moved from the Devices modal into the panel, as P3 in
the local UniFi study suggests).

**Tabs: Overview · Ports · Settings** for the router. Ports stays a tab: it is a different kind of content (a table)
and the owner uses it. **Neighbour: no tabs**, just Overview.

**Overview:**

1. **Issues** (unchanged logic). For a neighbour, the "managed elsewhere" note takes this slot in the same card
   style.
2. **Device face**. A port on the face opens the Ports tab with that port selected, so the face isn't just
   decoration.
3. **Last 24 h**: CPU / memory / temperature sparklines (moved up from the bottom).
4. **Facts** (at most 6): Model (board + model code, merged with Hardware into one row), IP address, Firmware (only
   the version, since a pending upgrade is already an issue), Storage, CPU cores, Temperature. Neighbour: IP, MAC,
   Connected to (port + its interface), Version only when the subtitle doesn't show it. Drop Name and Version rows
   that repeat the header.
5. **Behind it** (neighbour, and later each switch/AP in `multi_device.md`): devices and clients on that port.
6. **Details** (closed): MAC addresses, full hardware string, serial/model code, platform.

**Ports:** at phone width each port becomes a two-line row (name + status on line 1, speed + connected on line 2)
instead of a cut-off table.

**Settings:** Name, LEDs, SSH as today (already one form). Manage moves to the toolbar and More, keeping each item's
consequence text in the confirm/review step. Switch mode stays a guided form, but opens from More.

## 4. Wireframes

Desktop, client panel (460 px panel over the list, no scrim at ≥ 1100 px):

```
┌──────────────────────────────────────────────┐
│ [▭] living-room-tv ✎               ● Online ×│
│     Acme · TV                                │
│ ┌──────────┐ ┌──────────────┐ ┌────────────┐ │
│ │Experience│ │Down / Up now │ │Connected   │ │
│ │Good −61  │ │1.2 M / 80 k  │ │3d 4h       │ │
│ └──────────┘ └──────────────┘ └────────────┘ │
│ [❙❙ Pause] [⊘ Block]               [More ▾]  │
│ ─ Overview ─  Settings                       │
│ ┌ ⚠ Keeps switching bands       [See why ▸]┐ │
│ └──────────────────────────────────────────┘ │
│ Activity, 24 h                               │
│ ▁▂▁▃▂▁▁▅▂▁▂▃▁▂▇▃▂▁▂▃▄▂▁▂▃▅▆▃▂▁                │
│ −24 h        peak 4.1 Mbps at 21:00     now  │
│ Network       ExampleNet · 5 GHz · wlan2     │
│ IP            10.0.0.42 (DHCP)               │
│ Wi-Fi         WiFi 6 · 866 Mbps              │
│ Lease renews  in 6h                          │
│ Total         1.2 GB down · 80 MB up         │
│ Timeline                                     │
│ 21:04  Joined 5 GHz (−58 dBm)                │
│ 20:57  Left 2.4 GHz (−74 dBm)                │
│ …                              [Show all]    │
│ ▸ Details (MAC, maker, hostname, mDNS…)      │
│ Show in RouterOS                             │
└──────────────────────────────────────────────┘
```

Desktop, router panel:

```
┌──────────────────────────────────────────────┐
│ [⌂] Home router                    ● Online ×│
│     hAP example · RouterOS 7.x               │
│ ┌──────┐ ┌──────┐ ┌───────┐ ┌──────┐         │
│ │CPU 3%│ │Mem 41│ │Clients│ │Up 9d │         │
│ └──────┘ └──────┘ │  12   │ └──────┘         │
│                   └───────┘                  │
│ [◎ Locate] [↻ Restart…]            [More ▾]  │
│ ─ Overview ─  Ports  Settings                │
│ ┌ ⚠ RouterOS 7.y is available   [Updates] ┐  │
│ └─────────────────────────────────────────┘  │
│ [ device face: ports clickable ]             │
│ Last 24 h  CPU  ╱╲__╱╲_   3%                 │
│            Mem  ‾‾‾‾‾‾‾  41%                 │
│ Model     hAP example (RBxxxx)               │
│ IP        10.0.0.1                           │
│ Firmware  7.x                                │
│ Storage   4 MB free of 16 MB                 │
│ ▸ Details                                    │
└──────────────────────────────────────────────┘
```

Phone (375 px, full-width panel with scrim; the header shrinks to one line once the body scrolls):

```
┌───────────────────────────────┐
│ [▭] living-room-tv ✎   ●  ×   │
│ Acme · TV                     │
│ ┌─────────────┬─────────────┐ │
│ │Good −61 dBm │1.2 M / 80 k │ │
│ ├─────────────┴─────────────┤ │
│ │Connected 3d 4h            │ │
│ └───────────────────────────┘ │
│ [❙❙ Pause][⊘ Block][More ▾]   │
│ ─Overview─  Settings          │
│ ⚠ Keeps switching bands  ▸    │
│ Activity, 24 h  ▁▂▅▂▇▃▂▁      │
│ Network  ExampleNet · 5 GHz   │
│ IP       10.0.0.42 (DHCP)     │
│ …                             │
└───────────────────────────────┘

Ports tab on a phone:
│ ether1  ● Link up             │
│   1 Gbps · Upstream           │
│ ether2  ● Link up             │
│   100 Mbps · smart-hub        │
```

## 5. Accessibility (WCAG 2.2 AA)

- **Tabs**: one `PanelTabs` with `role="tablist"`, `role="tab"`, `aria-selected`, `aria-controls` and
  `tabindex` roving with ←/→/Home/End. The panel body is `role="tabpanel"`. This fixes both drawers (4.1.2,
  2.1.1).
- **More menu**: a disclosure button (`aria-expanded`, `aria-controls`) over a plain list of buttons, not an ARIA
  `menu` (simpler and correct with Tab). Esc closes the menu first, then the panel (one layer per Esc, as now).
  Focus goes back to the More button.
- **Status chips and pills** carry text (Online, Paused, Good), never colour alone (1.4.1). Check that the chip and
  pill colours reach 4.5:1 for text and 3:1 for the dot against `--panel` in both themes (1.4.3, 1.4.11).
- **Live numbers**: no `aria-live` on rates that update every second, because it is too noisy. Ping results and
  "Blocked/Paused" keep `role="status"`.
- **Chart**: `role="img"` with an `aria-label` summary (peak and total). The hourly bars stay available as a list
  under `Show all` for screen readers and keyboard users (1.1.1).
- **Details disclosure**: native `<details>/<summary>` (keyboard and screen readers for free).
- **Sticky header and footer** must not hide the focused field (2.4.11 Focus Not Obscured). Add
  `scroll-padding-top/bottom` on the panel equal to the header and footer heights.
- **Target size**: toolbar buttons and tiles at least 24×24 px (2.5.8). The icon picker squares are 30 px, which is
  fine.
- **Rename**: the pencil button gets `aria-label="Rename <name>"`, and the input gets a label.
- **Reflow**: the Ports table becomes rows under 480 px so the panel never scrolls sideways at 320 px (1.4.10).
- **Focus**: keep the current focus-in/focus-back behaviour. When the panel is non-modal (≥ 1100 px) it is
  `role="complementary"` with a label, and on phones it stays modal-like with the scrim.
- **Untrusted text** (hostnames, mDNS fields, neighbour identity) stays plain text via Svelte escaping. Nothing in
  this change builds commands from it.

## 6. Implementation plan

Small steps. Each one ships alone, keeps every action reachable, and is checked with `npm run check`, the build and a
Playwright pass on `sample.json` (light/dark, 1400/375). Effort: S = under a day, M = a few days.

| # | Step | Files | Effort |
|---|---|---|---|
| 1 | `PanelTabs` with full tab semantics and arrow keys. Use it in the client and device drawers | `lib/ui/PanelTabs.svelte`, both drawers | S |
| 2 | Client timeline: one deduplicated list from bandLog, stored roams and events (pure function + `node --test` test), shown in Insights for now | `lib/timeline.ts`, `tests/timeline.test.ts`, `ClientDrawer.svelte` | S |
| 3 | Client header status chip and 3 key-number tiles. Remove the large rate block | `ClientDrawer.svelte` | S |
| 4 | Client toolbar: Pause/Resume, Block/Unblock, More (Reconnect, Ping, Fix IP, Isolate, Traffic rule, Show in RouterOS). Drop the Limit button | `ClientDrawer.svelte`, `lib/ui/MoreMenu.svelte` | S |
| 5 | Merge Insights into Overview: attention card (blocked/paused/band question with `See why`), Activity chart, Connection (≤ 6 rows), Timeline, Details disclosure. Remove the Insights tab and map a kept `client.tab = 'insights'` to Overview | `ClientDrawer.svelte` | M |
| 6 | Client Settings as one form: tikontrol-only fields saved at once, router fields in one batch with a sticky footer, Remove rows add to the batch. Server: confirm `wifi-private-psk`, `limit-client` and the `remove-*` intents compile inside `batch` (they are intents already; add a test if one is missing) | `ClientDrawer.svelte`, `src/server/src/intents/mod.rs` test | M |
| 7 | `SidePanel` shell (header, numbers, toolbar, tabs, footer slot, focus/Esc/scrim once). Move all three drawers onto it | `lib/ui/SidePanel.svelte`, three drawers | M |
| 8 | Device header status chip, 4 key tiles, toolbar (Locate popover, Restart, More with switch mode and Updates). Settings keeps Name/LEDs/SSH only | `DeviceDrawer.svelte` | S |
| 9 | Device Overview order: Issues → face (ports clickable to the Ports tab) → 24 h → ≤ 6 facts → Details. Merge Model/Hardware. Drop the duplicate firmware note | `DeviceDrawer.svelte`, `DeviceFace.svelte` | S |
| 10 | Neighbour panel in the same shell: chip "Not managed here", tiles, WebFig + Set up in the toolbar (guide moved from the Devices modal), Behind it. Drop repeated rows | `NeighborDrawer.svelte`, `Devices.svelte` | S–M |
| 11 | Ports compact view as rows under 480 px | `Ports.svelte` | S |
| 12 | Update `docs/FEATURES.md` rows for the client and device panels, then run `src/scripts/progress.py` | docs | S |

Order: 1–4 are independent quick wins that the owner can judge early. 5–6 are the real change to the client panel.
7–10 bring the devices in line and give `multi_device.md` one shell to add CMR devices and other routers to. In total
about 6–8 working days.

## 7. Where each current item goes

| Today | After |
|---|---|
| Client Overview: Pause, Block | Toolbar |
| Reconnect, Ping, Fix IP, Isolate, Traffic rule | More menu |
| Limit button | Removed (it only switched tabs). Speed limit stays in Settings |
| Live down/up | "Down / Up now" key tile |
| Experience, Connected for | Key tiles |
| Connection, IP, standard, link rate, lease renews, total traffic, notes | Overview › Connection |
| MAC, maker, model, type, hostname, vendor class, first/last seen, CCQ, What the device says | Overview › Details (closed) |
| Why 2.4 GHz, band history, fixes | Attention card › See why (band history also in the Timeline) |
| Insights: 24 h traffic | Overview › Activity |
| Insights: roaming history, recent activity | Overview › Timeline |
| Settings: icon, notes, fixed IP, DNS, VLAN, own password, limit, scheduled pause | Settings (one form) |
| Device Overview: issues, face, facts, 24 h | Overview (reordered), CPU/memory/uptime also as tiles |
| Device Ports | Ports (rows on phones) |
| Device Settings: name, LEDs, SSH | Settings |
| Manage: Locate, Restart | Toolbar |
| Manage: Use as a switch / Leave switch mode | More menu |
| Neighbour: facts, WebFig, behind it | Same shell. WebFig and Set up in the toolbar |
| Devices page "Set up" modal for neighbours | Set up in the neighbour panel (the modal can stay as a fallback for neighbours without a MAC) |
| Show in RouterOS | Unchanged, at the end of each panel and in More |
