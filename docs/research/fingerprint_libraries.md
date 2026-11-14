# Existing libraries and datasets for client identification (2026-11)

The owner asked whether existing libraries already do what `identify.ts`, `oui.ts` and the DHCP/mDNS/SSDP listener
in `discovery.rs` do, so we could reuse them instead of keeping our own tables. This note checks the usual
candidates against the rules in `CLAUDE.md`: permissive licences only (MIT, Apache-2.0, BSD, ISC and similar) unless the
owner agrees, as few dependencies as possible, no outbound calls by default.

Licences were read from each project's own `LICENSE` file, crates.io or the npm registry on 2026-11-14. "Fresh" means the
last push or release seen that day. Nothing was added to the repo, and the router was not touched.

## 1. What we need to identify, and what we can actually see

tikontrol sees: the MAC (OUI), DHCP options 55/60/12/81 (receive-only listener), DHCP lease fields from RouterOS,
mDNS/DNS-SD TXT records, SSDP headers and UPnP XML, and MNDP/LLDP/CDP neighbours. It does **not** see TLS handshakes,
HTTP User-Agents, TCP SYNs of other clients, or Wi-Fi probe frames. Any library built on those inputs (p0f, JA3/JA4,
User-Agent parsers, nDPI) cannot help, whatever its licence.

## 2. Candidates

### 2a. MAC prefix (OUI) to maker

| Candidate | Lang / format | Licence | Size / freshness | Notes | Fit |
|---|---|---|---|---|---|
| **IEEE MA-L `oui.csv`** (what `oui.py` already reads) | CSV | No licence stated; facts | ~4 MB, updated daily | The primary source. Build-time extract only. | ✅ keep |
| npm `oui-data` (silverwind) | JSON | BSD-2-Clause (code); data is IEEE | 5.7 MB unpacked, released 2026-11 | Just IEEE as JSON. Far too big for the UI bundle; our extract is a few KB. | ➖ no gain |
| npm `oui` (silverwind) | CLI | BSD-2-Clause | Uses `oui-data` | CLI, not for us. | ➖ |
| npm `mac-oui-lookup`, `@network-utils/vendor-lookup`, `mac-lookup` | JSON in package | MIT / MIT / BSD-2-Clause | 1.7-2.5 MB | Full vendor lists at runtime. Same size problem, and no brand grouping. | ➖ |
| crate `oui-data` (jwalton) | Rust, embedded IEEE | MIT | released 2026-10 | Fresh and permissive, but the server does not need makers: the UI does. | ➖ for now |
| crate `oui` | Rust | MIT OR Apache-2.0 | last release 2021 | Reads **Wireshark `manuf`**, stale. | ⛔ avoid |
| crate `mac_oui` | Rust | "non-standard" on crates.io | 2024 | Licence not SPDX; would need reading and the owner's agreement. | ⛔ avoid |
| Wireshark `manuf` | text | Wireshark is GPL-2.0-or-later; file has no header | daily | Already documented: prefer IEEE so the question never arises. | ⛔ as input |
| Nmap `nmap-mac-prefixes` | text | NPSL (Nmap Public Source License), not OSI-permissive | — | | ⛔ |
| `arp-scan` `ieee-oui.txt` | text | tool GPL-3.0 | — | Just IEEE again. | ⛔ |
| Pi-hole FTL `macvendor.db` | SQLite | Pi-hole is EUPL-1.2 | — | Built from manuf/IEEE. | ⛔ |

**Verdict:** nothing beats `oui.py` + IEEE. Every package is either the same IEEE data, much bigger, or GPL-derived.

### 2b. Model identifiers to a product name (mDNS `model`/`rpMd`/`am`, UPnP `modelName`)

| Candidate | Format | Licence | Size / freshness | What it adds | Fit |
|---|---|---|---|---|---|
| **`kyle-seongwoo-jun/apple-device-identifiers`** | JSON per OS (iOS, macOS, watchOS, tvOS, visionOS) | MIT | ~25 KB total, pushed 2026-08 | `iPhone17,1` → a human product name. Our `MODEL` table only gives the kind. | ✅ adopt as build-time extract |
| `littlebyteorg/appledb` | JSON (`deviceFiles/`) | MIT | large, pushed 2026-11 (daily) | Same mapping plus much more (OS builds, jailbreak data). Freshest, but we would extract a tiny part of a big repo. | 🟨 fallback source |
| npm `ios-device-list` (pbakondy) | npm JSON | MIT | 770 KB, last push 2023 | Stale. | ➖ |
| Matter vendor IDs (`connectedhomeip`, CSA DCL) | C++ / ledger | Apache-2.0 (code); DCL terms unclear | — | `_matter._tcp` TXT `VP=<vendor>+<product>`. Only worth it once Matter devices show up. | ⏸ later |
| UPnP device types | spec URNs | facts | — | `urn:schemas-upnp-org:device:MediaRenderer:1` etc. Already in `MODEL`. | ✅ already done |

Product names and model identifiers are facts. MIT still asks us to keep the copyright notice, so ship it in
`THIRD_PARTY_LICENSES.txt`.

### 2c. Brand/product from mDNS service, SSDP and DHCP hostname patterns

| Candidate | Format | Licence | Size / freshness | What it adds | Fit |
|---|---|---|---|---|---|
| **Home Assistant `homeassistant/generated/{dhcp,zeroconf,ssdp}.py`** | Generated Python dicts (from each integration's `manifest.json`) | Apache-2.0 | dhcp 35 KB (~390 matchers), zeroconf 25 KB (~250 matchers + HomeKit model prefixes), ssdp 12 KB; pushed daily | Community-maintained matchers: DHCP **hostname glob + OUI** → integration (e.g. `neo-*` + an OUI → a thermostat brand), mDNS service type + TXT property/name globs → integration, HomeKit `md` prefixes → brand, SSDP `manufacturer`/`deviceType`/`st` → integration. `integrations.json` maps the integration to a display name. Exactly the "brand of this smart-home box" gap our `HOST` and `SERVICE` tables guess at. | ✅ **adopt** as build-time extract |
| `home-assistant/netdisco` | Python | Apache-2.0 | archived 2021 | The predecessor of the above. | ➖ superseded |
| `python-zeroconf` | Python | LGPL-2.1 | — | A resolver, not a dataset. | ⛔ |
| Avahi `service-types` | text | LGPL-2.1 | — | Service type → label. Easy to write ourselves. | ⛔ |

Caveats for the HA data: matchers answer "is there an integration for this?", not "what is this device?". Some are
broad (an OUI alone, or `*` hostnames with an OUI), some are only `registered_devices: true` (useless to us), and one
integration can cover many products. Treat a hit as a **maker/brand hint with medium confidence**, below a model id
and above our host-name regexes. Integration names are brand names: show them as text only, no logos.

### 2d. DHCP option 55/60 fingerprint databases (OS family / device)

| Candidate | Format | Licence | Size / freshness | Fit |
|---|---|---|---|---|
| **Fingerbank** (Inverse/Akamai) | SQLite DB or Cloud API | Commercial. The SQLite DB is licensed; the API has a free tier of 300 requests/hour. Old 2012 dumps were ODbL/DbCL (share-alike) | best coverage | ⛔ bundle; API only opt-in (see `client_identification.md`) |
| Satori `fingerprints/dhcp.xml` (xnih/satori) | XML, option 55 / option list / vendor class tests | **GPL-2.0** (repo `LICENSE`, no separate data licence) | 458 KB, `last_updated` 2025-05; many entries credited to PacketFence | ⛔ GPL, and partly the old PacketFence/Fingerbank lineage |
| PacketFence `dhcp_fingerprints.conf` | conf | GPL-2.0 | legacy | ⛔ |
| nDPI (ntop) | C library | LGPL-3.0 | active | ⛔ copyleft, C/FFI, built for DPI |
| JA4D (FoxIO) | DHCP fingerprint *format* | **FoxIO License 1.1**, patent-pending, not permissive for monetisation | 2026 | ⛔ do not implement or name our key "JA4D" |
| Nmap `nmap-os-db` | text | NPSL | — | ⛔ and TCP/IP-stack based anyway |

**Verdict:** no permissive DHCP fingerprint database exists. Keep step 4 of `client_identification.md`: our own small
option-55 table, written from RFC option numbers and our own observed (anonymised) packets, never copied from Satori,
PacketFence or Fingerbank. Our `fingerprintKey` (request list + vendor class + maker) is our own simple format and
stays that way.

### 2e. Signals we cannot see (for completeness)

| Candidate | Licence | Why not |
|---|---|---|
| p0f v3 | LGPL-2.1, last real update ~2016 | Needs other clients' TCP SYNs. |
| JA3 (Salesforce) | BSD-3-Clause, archived | Needs TLS ClientHello. |
| JA4 TLS client | BSD-3-Clause | Needs TLS; other JA4+ methods are FoxIO License. |
| `uap-core` regexes | Apache-2.0 | Needs HTTP User-Agents. |
| `ua-parser-js` | **AGPL-3.0-or-later since v2** (v1 was MIT) | Needs User-Agents, and AGPL. ⛔ |
| Zeek | BSD-3-Clause | A full traffic analyser, not a dataset; would need a mirror port. |

### 2f. Rust protocol crates (replacing our parsers, not our tables)

| Crate | Licence | Fresh | Comment |
|---|---|---|---|
| `dhcproto` | MIT | 2026-05 | Complete DHCPv4/v6 parser. Ours is ~50 lines, receive-only, bounded and tested; a full codec adds surface we don't use. |
| `simple-dns` | MIT | 2026-10 | Small DNS packet parser; the best choice if passive mDNS (step 5) makes our parser grow. |
| `mdns-sd` | Apache-2.0 OR MIT | 2026-11 | Full responder/browser with its own threads. Too much: we only listen and query. |
| `hickory-proto` | MIT OR Apache-2.0 | 2026-09 | Large; overkill. |

**Verdict:** keep our parsers. Revisit `simple-dns` only if mDNS handling grows.

## 3. Recommendation (ranked)

| # | Do | Why | How it plugs in |
|---|---|---|---|
| 1 | **Home Assistant discovery matchers** (Apache-2.0) | Hundreds of maintained hostname+OUI, mDNS and SSDP → brand matchers; fills the smart-home gap with no runtime dependency. | `src/scripts/ha_match.py <core checkout> <date> > src/ui/src/lib/hamatch.ts`, like `oui.py`: parse `generated/{dhcp,zeroconf,ssdp}.py` with `ast.literal_eval` (never import/execute them), keep matchers that have a hostname or TXT/name pattern, map domain → display name from `integrations.json`, drop `registered_devices`. Expect ~20-40 KB of TS; check the bundle. Use as a "maker hint" in `guessModel`/`guessDevice`. |
| 2 | **Apple model identifiers** (MIT, `kyle-seongwoo-jun/apple-device-identifiers`) | Turns `rpMd`/`model` values into a product name for the client panel; tiny. | Build-time extract into a `Record<string, string>`, or merge into the same generated file. `appledb` as a fallback source if it falls behind. |
| 3 | Keep **IEEE + `oui.py`** as is | Nothing better exists; every package is bigger or GPL-derived. | Make the IEEE CSV the documented default input (already noted). |
| 4 | Keep **our own option-55 table** | No permissive DHCP fingerprint DB exists. | Grow it from observed packets, per `client_identification.md` step 4. |
| 5 | Later: Matter `VP`, `simple-dns` | Only when the need is real. | — |

**Avoid:** Fingerbank data (commercial), Satori/PacketFence DHCP XML (GPL-2.0), nDPI (LGPL-3.0), Wireshark `manuf`
(GPL tool), Nmap files (NPSL), p0f (LGPL), JA4+/JA4D (FoxIO licence, patent-pending), `ua-parser-js` v2 (AGPL),
crate `mac_oui` (non-standard licence), crate `oui` (stale `manuf` data).

## 4. Legal notes

- **Data extracts still carry the licence.** Apache-2.0 (HA) and MIT (Apple identifiers) require the licence text
  and copyright notice when we redistribute a derived table. `src/scripts/third_party.py` only covers crates and npm
  packages today, so it needs a small "bundled data" section listing source, commit/date and licence text for each
  extract, or `THIRD_PARTY_LICENSES.txt` will be incomplete. Apache-2.0 §4(b) also asks for a note that we changed the
  files: put "extracted and reformatted by `<script>`" in the generated header.
- **GPL data is the main trap.** Deriving a table "by hand" from Satori's `dhcp.xml` is still a derivative of GPL-2.0
  data. Write the option-55 table only from RFCs and our own observations.
- **JA4D**: the method is patent-pending under the FoxIO License. Don't implement it or reuse its name.
- **Trademarks:** HA integration names and Apple product names are brand names. Show them as plain text, never with
  logos, consistent with the "Trademarks" section of the README.
- **Privacy:** all of the above is offline and build-time. Nothing calls out at runtime; the Fingerbank API stays
  an opt-in idea only.

## 5. Status

- ✅ #1 and #2 adopted (2026-11-14): `src/scripts/devdata.py` extracts the Home Assistant DHCP, zeroconf, HomeKit and
  SSDP matchers plus integration names and categories, and the Apple identifiers, into `src/ui/src/lib/devdata.ts`
  (~50 KB, ~9 KB gzipped). `listed()` and `appleName()` in `identify.ts` use it; the source shows as "Public device
  list" with medium confidence, below the user's choice, neighbours and announced models. Bare matches on protocols
  many brands speak (`_companion-link`, `_hap`, `_ipp`, `_matter`, `_meshcop`, `_googlecast`, …) are dropped.
  Licence texts are in `src/scripts/devdata/` and `THIRD_PARTY_LICENSES.txt`. Refresh by re-running the script with
  newer commits.
