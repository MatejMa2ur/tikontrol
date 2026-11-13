# One network, several MikroTik devices (2026-11)

The owner runs a main router (the "controller", a hAP ac²) and a hAP lite used as a switch on one of its
ports. Both are connected to tikontrol: the switch as an extra **site**. From the main router's view the switch
shows up correctly as a neighbour. Switching the site selector to the switch showed:

`Internet (public IP) → "Modem" → "hAP lite switch" (subtitle "hAP lite · undefined")`, and "All connected
devices 0", although a client is plugged into the switch.

This note explains why, what shipped as a quick fix, and how multi-device support should work. All addresses
and names below are made up.

## 1. Root cause

Everything a page shows comes from one router's snapshot (`raw` in `src/ui/src/lib/data.ts`). Before the fix, the
only switch/AP detection was in `data.ts`:

```ts
behind = !(raw.wan ?? []).length && defaultGw && bridgeNets.some((n) => inSubnet(defaultGw, n)) ? defaultGw : undefined
```

That fails on a real hAP lite used as a switch, in three ways:

1. **"No DHCP client" was required.** A hAP lite turned into a switch usually keeps the defconf DHCP client
   (on `ether1`, now a bridge port, or on the bridge). `/ip/dhcp-client` is `raw.wan`, so the switch looked like
   a router with a WAN. `behind` stayed unset and Topology took the gateway branch: Internet (with the
   switch's own `/ip/cloud` public address) → an invented "Modem" (`internet.doubleNat ? 'Upstream router' :
   'Modem'`) → the switch.
2. **The address had to sit on a bridge.** `gatewayIp` was the first address on an interface called `bridge`
   (or listed in `/interface/bridge`). A DHCP client on `ether1` puts the address on `ether1`, so `gatewayIp` was
   `undefined`, and the gateway branch printed it with a template string: `` `${router.board} · ${internet.gatewayIp}` ``
   gave "hAP lite · undefined". (The `behind` branch already used `.filter(Boolean)`.)
3. **Clients come from DHCP leases, the Wi-Fi registration table and reachable ARP entries on a bridge.** A
   switch has no DHCP server and no leases. Its ARP table only holds what it talks to itself (the main router),
   and it does not route the client's traffic, so the client never shows up in its ARP table. Its bridge host
   table (`/interface/bridge/host`) does list the client's MAC on the port it is plugged into, but those MACs
   were only used when `behind` was set. With (1) failing, the switch showed 0 clients.

What the main router sees is fine: the switch is an MNDP neighbour on the port it hangs off
(`groupByNeighbor` in `lib/topology.ts`), and the clients behind it appear because the main router has their
leases and the bridge host table puts them on that port.

The deeper problem is the model: a **site** is a whole separate network with its own internet, and every page
assumes the router it shows is a gateway. A switch in the same house is not a separate network.

## 2. Quick fix (shipped)

`uplinkOf(raw)` in `src/ui/src/lib/topology.ts` (tested in `tests/topology.test.ts`) decides the role from
read-only facts, not from whether a DHCP client exists:

- **Behind another router** when the default route (static, or from a bound DHCP client; `192.0.2.1%bridge`
  accepted) points into one of the device's **own subnets on any interface**, and the device runs **no enabled DHCP
  server and no enabled masquerade/src-nat**. A gateway router also has its ISP's gateway in its WAN subnet,
  but it always does one of those two.
- **Uplink port**: the port of the MNDP/LLDP neighbour at the gateway address. When discovery is off, the
  gateway's MAC from ARP is looked up in the bridge host table.
- **Own address** (`gatewayIp`): for a gateway, the LAN (bridge) address as before. For a switch, the address
  facing its gateway. Missing values are never printed as text.

So a switch site now draws Internet → the main router (named from its MNDP announcement when heard) → the switch,
and its bridge hosts off the uplink port are its clients, with the port they are on. They have no name or IP
yet: those live on the main router (phase 2).

Known ceiling: a router with neither a DHCP server nor NAT (a routed public subnet) would read as a switch. If
that case appears, add a role override.

## 3. Device roles

| Role | Read-only signals | What tikontrol shows |
|---|---|---|
| **Gateway router** | Default route via a WAN (DHCP client / PPPoE / static to the ISP) **and** an enabled DHCP server or masquerade; `/ip/cloud` public address | Today's pages: Internet, WAN, clients from leases/ARP/Wi-Fi |
| **Switch (bridge-only)** | Default route into its own subnet, no enabled DHCP server, no masquerade (`uplinkOf`); usually all ethernet ports in one bridge (`/interface/bridge/port`); tikontrol's own switch mode leaves `tik:switch` tags | Ports, VLANs, port clients from `/interface/bridge/host`; no WAN, internet, DHCP or firewall pages |
| **AP (standalone, bridged)** | Switch signals plus enabled Wi-Fi (`/interface/wireless` or `/interface/wifi`) in AP mode | Switch view plus radios and Wi-Fi clients |
| **CAPsMAN cap** | `/interface/wifi/cap` `enabled=yes` (wifi package) or `/interface/wireless/cap` `enabled=yes` (legacy); radios with `master`/`managed by CAPsMAN` | Radios and clients only; SSIDs are configured on the manager, so Wi-Fi settings link there |
| **CAPsMAN manager** | `/interface/wifi/capsman` (or `/caps-man/manager`) `enabled=yes`, remote caps listed | Gateway or switch view plus the caps it manages |
| **Wireless repeater / station bridge** | A Wi-Fi interface in `station`, `station-bridge` or `station-pseudobridge` mode, uplink is that interface | Switch view whose uplink is wireless (dashed link, signal on the edge) |

`hardware.ts` stays about what the board **can** do (ports, radios, PoE, standards). The role describes what it
**is doing** and gets its own pure function (`uplinkOf` today, a `roleOf` later) next to it, never written to the
router. The CAPsMAN and repeater reads (`/interface/wifi/cap`, `/interface/wifi/capsman`, `/interface/wireless/cap`,
`mode` on the wireless interfaces) need adding to `ros/queries.rs` with explicit proplists. A device without
the menu returns an error for that key, which is fine (feature detection).

## 4. Site vs adopted device

UniFi's model: one controller per site, and each switch or AP is a **device adopted** into that site. There is
one client list and one topology. A second site is a different network: another house or office with its own
gateway.

tikontrol should follow the same split:

- **Site** = one network with one gateway router. The site selector stays for that (several houses), and the
  sites code (`sites.rs`, one `App` per router) stays as it is.
- **Adopted device** = an extra MikroTik inside a site (switch, AP, cap, repeater). It is listed under
  Devices, drawn in the site's one topology, and has no entry in the site selector.

One topology per site, built on the server from all of that site's snapshots:

1. Start with the gateway's tree as today.
2. For each adopted device, find its node: the gateway's neighbour row with the same MAC or identity, which
   already gives the gateway port. Behind several switches, follow the chain through the switches' own neighbour
   tables.
3. Attach the device's own bridge hosts as its clients **per port**, minus its uplink port. Take the name, IP,
   hostname and type for each MAC from the gateway's leases, ARP and Wi-Fi tables (the client list is keyed by
   MAC already).
4. A MAC seen on several devices sits at the **deepest** one: on the gateway it is "behind ether3", on the
   switch it is "on port 2". That replaces today's guess that every client on the gateway's port 3 is behind
   that switch.

Clients, Devices, Dashboard counts and Insights then read the merged view. Live data (`live.rs`) keeps
polling each device separately. Merging happens when a snapshot changes, not per request.

What an adopted device's own pages show is cut down by role (table in §3). Its Dashboard has no internet,
WAN, speed test or public IP cards, and Settings has no Internet, DHCP, firewall or port-forward pages.

## 5. Credentials and security

- **Each device has its own least-privilege reader user** (`tikontrol-read` group via the existing one-step
  setup in `policy.rs`), over API-SSL (8729) with a verified certificate, and its own optional writer. A
  credential for the gateway must never be reused on a switch: one leaked device must not open the others.
- **Reuse the sites storage.** An adopted device is a site entry with a `parent` (the gateway site id) and a
  `role`. The sealed per-site credentials (`router-password:<id>:<role>`), one `App` per router, history, Safe
  apply and the read-only mode all keep working unchanged. Only the site listing (selector vs Devices) and the
  merged topology are new.
- **Read-only is enforced per device.** "Read-only mode" means every device in the site connects with a
  reader-only group. The policy check (`policy.rs`) runs per device and the Devices page shows each device's
  detected policies.
- **Safe apply per device.** Each change targets one device and runs that device's own revert job. A change
  that touches two devices (e.g. a VLAN on the gateway and the switch trunk) is two plans, applied in an order
  that never cuts tikontrol off from the far device: the device furthest away first, then the gateway. The
  second plan waits until the first is confirmed. Management address, uplink port, bridge VLAN filtering and
  `/ip/service` on a switch get the same high-risk warnings as on the gateway, plus "you reach this switch only
  through port N".
- **Untrusted data.** Identities, board names and addresses from neighbour tables come from other devices. They
  are matched by MAC (strict format, as `neighborHref` does) and escaped. Adoption never trusts an announced
  address: the user enters or confirms it, and the certificate is pinned at adoption.
- **Discovery is read-only.** The gateway's neighbour table may suggest "a MikroTik on port 3 is not adopted
  yet". Adopting it asks for its address and an admin login once, creates the reader user there, and forgets
  the admin password, the same as the first router.

## 6. Plan

| # | Step | Effort | Notes |
|---|---|---|---|
| 0 | **Quick fix** (done): role from routes + DHCP server + NAT, bridge hosts as switch clients, never "undefined" or an invented Modem for a switch | done | `uplinkOf`, tests |
| 1 | Role-aware pages for a switch/AP site: hide Internet/WAN/speed test/public IP cards and the Internet, DHCP and port-forward settings; a "Switch · behind 192.0.2.1 on port 1" status line | S (½–1 day) | UI only, reads `internet.behind` |
| 2 | Name a switch's clients from the gateway: the server exposes a read-only, MAC-keyed name/IP map of the parent site to its child sites | S–M (1 day) | Needs a `parent` on the site, no merge yet |
| 3 | `parent` + `role` on sites; Devices lists adopted devices under the gateway, the site selector hides them | M (1–2 days) | `store.rs` migration (optional fields), OpenAPI |
| 4 | Server-side merged topology per site (§4 steps 1–4) and the UI drawing it; neighbour nodes link to the adopted device's page | M–L (2–3 days) | Pure merge function with tests, then the UI |
| 5 | CAPsMAN and repeater detection: new queries, `roleOf`, cap/manager badges, radios of caps under the manager | M (1–2 days) | Feature-detected, legacy and wifi package |
| 6 | Adopt from the neighbour list ("Adopt this switch"): one-step reader creation on the new device | M (1–2 days) | Same flow as the first router |
| 7 | Cross-device changes (VLAN end to end: gateway + switch trunk) as ordered per-device Safe apply plans | L (3+ days) | After 1–6 are tested on CHR |

Steps 1–3 fix what the owner sees without the big merge. Step 4 is the "one network" view.

## 7. CHR tests

Two CHRs on one QEMU network (`chr-e2e.py --sites` already starts a second CHR):

- **CHR A, router:** DHCP client on `ether1` (QEMU user net), bridge with `ether2`, DHCP server + masquerade.
- **CHR B, bridge-only:** `ether1` (to A's `ether2`) and `ether2` in one bridge, a DHCP client on `ether1` (the
  failing real-world case), then a second run with a static address on the bridge and a static default route.
  No DHCP server, no NAT. A third QEMU guest or a CHR `veth`/second bridge port as a "client" behind B.

Checks:

1. B's snapshot gives `behind` = A's address, `upPort` = B's port towards A, both with MNDP on and with
   discovery off (ARP fallback).
2. A stays a gateway (`behind` unset) with its WAN DHCP client.
3. B lists the client MAC on the right port. After step 2, its name/IP come from A's lease.
4. B's reader user is rejected on every write (read-only proof per device), and B's credentials don't work on A.
5. After step 4: one topology with the client under B's port, not twice.
6. After step 7: a VLAN applied across A and B in order; reverting the second plan restores both, and
   tikontrol never loses B in between.
