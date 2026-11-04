# RouterOS v7 API capability and remote-change safety (as of 2026-11-04)

Evidence labels used on every finding:

- **[P]** Primary: MikroTik manual, product page, or official changelog.
- **[S]** Schema: tikoci/restraml dump of RouterOS `/console/inspect` for 7.25. It is machine-extracted from RouterOS by a third party. It proves a command or argument exists, not that it behaves as expected. The project itself warns that schema and API fidelity is [not perfect](https://tikoci.github.io/restraml/diff.html).
- **[L]** Third-party library author's live tests (tik4net, which marks findings as verified live). The test router is a lab CHR (virtual), not small hardware.
- **[U]** Forum or user report.
- **[I]** My inference. Not established by any source.

Versions at time of writing [P]: [stable 7.25.1](https://upgrade.mikrotik.com/routeros/NEWESTa7.stable), [long-term 7.24.6](https://upgrade.mikrotik.com/routeros/NEWESTa7.long-term), [development 7.26beta5](https://upgrade.mikrotik.com/routeros/NEWESTa7.development). help.mikrotik.com is [frozen](https://help.mikrotik.com/docs/spaces/ROS/pages/47579162/REST+API) and current docs are at manual.mikrotik.com.

No test was run on a real router for these notes. Everything marked [I] or listed under Gaps needs a lab check.

## Overall verdict: are the APIs good enough for a full web controller?

### Takeaway
Yes, but only on the binary API over TLS (8729), with REST as a secondary path at most. REST cannot stream, cannot hold Safe Mode, and serialises each user's requests. Safe writes are possible but not guaranteed: there is no transaction or commit-confirm primitive, MikroTik does not document API use of Safe Mode, and the scheduler fallback is disabled in `home` device mode.

### Cited Findings
- REST "does not stream output", and the reply arrives when the command ends. [P] — [REST API 7.25](https://manual.mikrotik.com/docs/7.25/developer-guides/rest-api/)
- The binary API supports tagged concurrent commands and `listen` pushes. [P] — [API](https://help.mikrotik.com/docs/spaces/ROS/pages/47579160/API)
- Safe Mode through the binary API works on 7.18+ and not over REST. [L] — [tik4net capabilities](https://github.com/danikf/tik4net/wiki/Connection-types-and-capabilities)
- Home mode disables Scheduler, Fetch, Sniffer, Bandwidth Test and RoMON. Re-enabling needs physical confirmation. [P] — [Device mode](https://manual.mikrotik.com/docs/system-information-and-utilities/device-mode/)
- MikroTik recommends a serial console for VLAN filtering work. [P] — [Bridge VLAN Table](https://help.mikrotik.com/docs/spaces/ROS/pages/28606465/Bridge+VLAN+Table)

### Inferences
- [I] Reasons this could fail, in order of severity:
  1. Bridge VLAN and firewall changes cannot be made fully safe remotely.
  2. Safe Mode through the API rests on undocumented behaviour, plus one library's tests.
  3. Rollback after a true lockout can take up to 9 minutes.
  4. Home-mode devices lack the scheduler fallback.
  5. Menus and property names change between 7.x releases.
  6. Polling cost on small hardware is unmeasured.
- [I] None of these is a blocker for read and monitoring features. They constrain what a write UI may offer without warnings.

### Gaps
- No primary statement from MikroTik that a third-party full controller is supported or that its API has stability guarantees. I found none.

## REST API versus binary API

### Takeaway
Both expose nearly the same command tree. The binary API is a persistent session with tags, cancellation and push. REST is simple JSON, but has a hard 60-second limit, runs one request at a time per user, and has had a session-leak bug that was only fixed in 7.25.

### Cited Findings

**Comparison**

| Topic | REST (`/rest`) | Binary API (8728 / 8729) | Label |
|---|---|---|---|
| Transport | `www-ssl` HTTPS; `www` HTTP since v7.9, discouraged | TCP 8728; TLS on 8729 | [P] [REST](https://help.mikrotik.com/docs/spaces/ROS/pages/47579162/REST+API), [API](https://help.mikrotik.com/docs/spaces/ROS/pages/47579160/API) |
| Available since | v7.1beta4 | long before v7 | [P] same |
| Auth | HTTP Basic | `/login` with plaintext password (post-6.43), so use 8729 | [P] same |
| Policies | Needs both `rest-api` and `api` | `api` | [P] [REST 7.25](https://manual.mikrotik.com/docs/7.25/developer-guides/rest-api/) |
| Timeout | 60 s, then `400 Session closed`; `duration` does not extend it | None documented | [P] same |
| Concurrency | One user's requests run one at a time; different users run in parallel | Tags allow several concurrent commands | [P] same; [API](https://help.mikrotik.com/docs/spaces/ROS/pages/47579160/API) |
| Streaming | None | `listen`, plus continuous commands | [P] same |
| Typing | All reply values are strings | Strings | [P] [REST 7.25](https://manual.mikrotik.com/docs/7.25/developer-guides/rest-api/) |
| Safe Mode | No | Yes on 7.18+ | [L] [tik4net](https://github.com/danikf/tik4net/wiki/Connection-types-and-capabilities) |

**REST details [P]** — [REST API 7.25](https://manual.mikrotik.com/docs/7.25/developer-guides/rest-api/)
- The REST server logs in to the API with the user's credentials and keeps that login for 10 minutes. Group policy changes can take up to 10 minutes to apply.
- A long request can make other requests from the same user fail with `Session closed`. The manual recommends separate users for independent clients.
- Long jobs can run in the background through `/execute` without `as-string`, which returns a job ID.
- `/file/read` returns at most 32768 bytes per request and needs `ftp` and `test`.
- No general reply size limit is stated.
- `.query` works only in a `POST` body to `print`.
- Exponent notation in JSON numbers is refused.

**TLS [P]**
- `www-ssl` accepts TLS 1.0, 1.1 and 1.2. `tls-version=only-1.2` refuses the older ones. The manual lists no TLS 1.3. With `certificate=none` it offers only anonymous ciphers, and HTTPS requests then fail. — [REST API 7.25](https://manual.mikrotik.com/docs/7.25/developer-guides/rest-api/)
- API-SSL without a certificate needs an anonymous Diffie-Hellman cipher. — [API](https://help.mikrotik.com/docs/spaces/ROS/pages/47579160/API)

**Binary API limits [P]** — [API](https://help.mikrotik.com/docs/spaces/ROS/pages/47579160/API)
- No regex in queries. Queries apply to `print` only.
- `.proplist` may return extra properties, and order and duplicates are undefined.
- Without `.proplist`, slow properties are included.
- The documented word length limit contradicts itself (5-byte lengths versus "only four-byte length is supported").
- `listen` "does not have the expected effect everywhere".

**Known bugs and quirks**
- REST sessions stuck in `/user/active`: users report unbounded growth on 7.22 through 7.24rc1, with no staff reply in the thread. [U] — [forum thread](https://forum.mikrotik.com/t/users-logged-in-via-rest-api-shown-in-active-users-do-not-disappear/269432). One user saw 339 entries on a hEX after 2 days on 7.21.4. [U] — [forum](https://forum.mikrotik.com/t/7-21-4-long-term-is-released/270024?page=3)
- 7.25 changelog: "user - fix removal of inactive REST API sessions (introduced in 7.21)". [P] — [7.25 changelog](https://upgrade.mikrotik.com/routeros/7.25/CHANGELOG). One user confirms it on one device, and manual removal still fails. [U] — [forum thread](https://forum.mikrotik.com/t/users-logged-in-via-rest-api-shown-in-active-users-do-not-disappear/269432). tik4net's measurements of this bug have an unclear date relative to 7.25. [L] — [findings](https://github.com/danikf/tik4net/blob/master/Docs/findings-rest-api.md)
- 7.25 also fixes "a memory leak when parsing authentication headers" in `www`. Any REST poller on earlier versions sends those headers on every request. [P] — [7.25 changelog](https://upgrade.mikrotik.com/routeros/7.25/CHANGELOG)
- HTTP pipelining: the router answers only the first of several requests queued on one connection, and the others are never answered (7.24.3). [L] — [findings](https://github.com/danikf/tik4net/blob/master/Docs/findings-rest-api.md)
- `PATCH` on multiple ids applies the change and then answers 404. [L] — same
- Non-ASCII JSON escapes: `"ř"` is stored as a single byte `f8`, not UTF-8, so send raw UTF-8. [L] — same
- Cancelling a REST request frees the client, not the router. The router keeps running the command and that user and address get refused until it ends. An abandoned `/tool/torch` blocks for about a minute. [L] — [tik4net](https://github.com/danikf/tik4net/wiki/REST-connection)

**Unavailable commands**
- REST: continuous commands cannot stream. `monitor` needs `once`, `ping` needs `count`. [P] — [REST API 7.25](https://manual.mikrotik.com/docs/7.25/developer-guides/rest-api/)
- Some operations fail per menu, for example removing an interface gives 406. [P] — [REST API](https://help.mikrotik.com/docs/spaces/ROS/pages/47579162/REST+API)
- Session-based commands such as telnet, mac-telnet and SSH are not available through the API, per a forum user, and inline scripting must be run from the console. [U] — this came from a search summary. I could not identify or read the original post, so treat it as a lead only.
- `/execute` takes `script`, `file` and `as-string`, which gives a route to run script code from API or REST. [S] — [restraml](https://tikoci.github.io/restraml/)
- MikroTik publishes no list of commands unavailable through either API. The API page names only `login` and `cancel` as API-specific. [P] — [API](https://help.mikrotik.com/docs/spaces/ROS/pages/47579160/API)

**Schema discovery**
- `POST /rest/console/inspect` can walk the command tree. tikoci publishes per-version dumps, a version diff tool and an OpenAPI 3 schema for newer builds. [U/S] — [restraml diff](https://tikoci.github.io/restraml/diff.html)

**Client libraries**
- librouteros (active, last pushed 2026-10-26) recognises `!empty`. [L] — [librouteros](https://github.com/luqasz/librouteros)
- socialwifi/RouterOS-api (last pushed 2025-03) and go-routeros (last pushed 2025-02) both mention `!empty` in code or changelog. Their last pushes are over a year and a half old. [L] — [RouterOS-api](https://github.com/socialwifi/RouterOS-api), [go-routeros](https://github.com/go-routeros/routeros)

### Inferences
- [I] The 10-minute internal login plus per-user serialisation suggests one REST user for polling and another for writes, as a minimum.
- [I] Basic auth means credentials cross the wire on every request. With TLS 1.2 at best, certificate pinning matters, because self-signed certificates are the common case.

### Gaps
- Throughput numbers for REST versus API on real hardware: none found.
- Maximum concurrent API sessions, and any API idle timeout: not documented.
- `mikrotik-proto`: no library of that name was found on GitHub. I used tik4net instead, which has the deepest published test notes. The tik4net repo contains `.claude` skill files, so its docs may be partly AI-assisted. Treat its conclusions as leads and verify them in the lab.

## Live streaming and polling cost

### Takeaway
Real push exists only on the binary API. Log follow and monitor commands exist as arguments, and client libraries use them, but MikroTik does not document `follow` over the API. No one has published polling cost on small routers.

### Cited Findings
- `listen` pushes `!re` on changes, marks deletions with `=.dead=yes`, runs until `/cancel`, and "may not work" everywhere. [P] — [API](https://help.mikrotik.com/docs/spaces/ROS/pages/47579160/API)
- The API page does not mention `follow` at all. [P] — same
- `print` args in 7.25 include `follow`, `follow-only`, `interval`, `on-event`, `proplist`, `as-value` for `/interface/wifi`, `/interface/wifi/registration-table` and `/log`. [S] — [restraml](https://tikoci.github.io/restraml/)
- `/interface/monitor-traffic` and `/interface/wifi/monitor` take `interval`, `duration`, `once`, `proplist`. [S] — same
- `/interface/wifi/registration-table` has only `find`, `get`, `print`, `remove`. It has no separate `listen` entry, though `listen` is an API verb and not expected in that tree. [S] — same
- REST `/listen` is mapped internally to `print follow-only`, is accepted, and never delivers a byte: 0 B in three 25–30 s windows with real events. Menus without that arg return `unknown parameter follow-only`. [L] — [findings §12](https://github.com/danikf/tik4net/blob/master/Docs/findings-rest-api.md)
- On the binary API, rows are pushed as they happen, and existing rows are not reported by `listen`. [L] — [tik4net streaming](https://github.com/danikf/tik4net/wiki/Monitors-and-streaming)
- A Java client streams `/log/print` over the API, but its author says follow support must be validated on devices. [U] — [PR](https://github.com/praktimarc/mikrotik-facade/pull/12)
- Over non-interactive SSH, `follow-only` returned nothing and exited at once. This may not apply to the API. [U] — [mikrotik-mcp logs](https://github.com/mikrotik-mcp/mikrotik-mcp/blob/master/docs/content/docs/reference/logs.md)
- 7.22 disabled `follow` in `/ip/firewall/connection`. [P] — [7.22 changelog](https://upgrade.mikrotik.com/routeros/7.22/CHANGELOG)
- Polling cost, lab CHR only: aggregate throughput clamps from about 1000 req/s to about 130 req/s (16 vCPU), and to 24–29 req/s (2 vCPU, 7.24.2). More connections do not add throughput. The author suspects hypervisor scheduling, and bare metal is untested. [L] — [throughput findings](https://github.com/danikf/tik4net/blob/master/Docs/findings-router-throughput-ceiling.md)
- A single 260 KB read stalled mid-reply in about 1 of 6–8 reads on the CHR. The cause for that stall was the lab VM's vCPU count. [L] — same

### Inferences
- [I] Plan as follows: `listen` for config tables; `monitor-traffic` with `interval` on a tagged API command for live charts; `/log/print follow-only` tested per version with a polling fallback; a 2–5 s `print` with a tight `.proplist` for the registration table.
- [I] Whether `listen` fires on signal and rate changes in the registration table is unknown. Assume it does not, and poll those fields.
- [I] Every client read must carry a timeout and be able to drop the connection, given the stalls above.

### Gaps
- No measured CPU cost of API or REST polling on hAP-class hardware. Searches returned only unrelated CPU threads. Measure with `/tool profile`.
- No primary confirmation that `follow` and `follow-only` work over the binary API on 7.25.
- `=interval=` behaviour for `print` over the API: not confirmed.

## The `wifi` package

### Takeaway
Dotted names such as `channel.band` and `configuration.ssid` are real parameter names, and since 7.15 `print` shows inherited values. MLO is real on WiFi 7 boards from about 7.21 on, but it is young: 7.24 and 7.25 are full of MLO fixes. Package coverage differs a lot between `wifi-qcom-ac`, `wifi-qcom`, `wifi-qcom-be` and `wifi-mediatek`.

### Cited Findings

**Menus**
- Sub-menus under `/interface/wifi` include aaa, access-list, cap, capsman, channel, configuration, datapath, frequency-scan, interworking, monitor, provisioning, radio, registration-table, scan, security, sniffer, spectral-scan, steering, wps-client, wps-push-button. [P] — [CLI reference 7.25](https://manual.mikrotik.com/docs/7.25/cli-reference/interface/wifi/)
- The menu was `wifiwave2` before 7.13, and console resolves the old name. [P] — [7.13 changelog](https://upgrade.mikrotik.com/routeros/7.13/CHANGELOG)

**Property names**
- Interface `set` args include `channel.band`, `channel.width`, `channel.frequency`, `channel.secondary-frequency`, `channel.afc`, `channel.skip-dfs-channels`, `configuration.ssid`, `configuration.mode`, `configuration.country`, `configuration.tx-power`, `datapath.bridge`, `datapath.vlan-id`, `security.authentication-types`, `security.ft`, plus `master-interface`, `mld-interface`, `mld-name`. [S] — [restraml](https://tikoci.github.io/restraml/)
- `channel.band` enums include `5ghz-ac`, `5ghz-ax`, `5ghz-be`. `channel.width` goes up to combined values like `20/40/80/160/320mhz`. [P] — [CLI reference 7.25](https://manual.mikrotik.com/docs/7.25/cli-reference/interface/wifi/)

**Inheritance in print output**
- Priority: interface settings, then assigned profile, then configuration profile, then that profile's assigned profiles. [P] — [Wi-Fi manual](https://manual.mikrotik.com/docs/wireless/wifi/)
- `print detail` shows all values including inherited ones. `print detail config` shows only values set on the interface. Manual example: `print detail` gives `.ssid="MikroTik"` and `channel.frequency=5180,5260,5500`, while `print detail config` gives only `configuration=common-conf`, `configuration.mode=ap`, `channel=ch-5ghz`. [P] — same
- 7.15 changelog: "show inherited properties with 'print' command (replaces 'actual-configuration') and added 'print config'". [P] — [7.15 changelog](https://upgrade.mikrotik.com/routeros/7.15/CHANGELOG)
- `.ssid=` in CLI detail output is a shorthand that follows `configuration.mode=ap`. Forum output shows `configuration.mode=ap .ssid=MikroTik-8075C3`, and a table view shows the column `CONFIGURATION.SSID`. [U] — via [forum](https://forum.mikrotik.com/t/configure-wifi-v-7-13-solved/172060)

**Registration table**
- Documented fields: authorized, auth-type, band, bytes, interface, last-activity, mac-address, packets, tx-bits-per-second, rx-bits-per-second, rx-rate, tx-rate, signal, ssid, uptime, vlan-id. [P] — [Wi-Fi manual](https://manual.mikrotik.com/docs/wireless/wifi/). tik4net maps the same names. [L] — [entity](https://github.com/danikf/tik4net/blob/master/tik4net.objects/Interface/Wifi/WifiRegistrationTable.cs)
- `print stats` exists on it. [S] — [restraml](https://tikoci.github.io/restraml/)

**6 GHz**
- Bands `6ghz-ax` and `6ghz-be`, and a `6g-channels` field on `/interface/wifi/radio`. [P] — [WiFi](https://help.mikrotik.com/docs/spaces/ROS/pages/224559120/WiFi)
- APs in the 6–7 GHz band default to `wpa3-psk` and `wpa3-psk-gd`. The manual does not state that WPA3 or OWE is mandatory. [P] — [Wi-Fi manual](https://manual.mikrotik.com/docs/wireless/wifi/)
- hAP be³ Media: 6 GHz 802.11ax/be, 2 chains, chip QXM-8086, BE9300, IPQ-5322 at up to 1.5 GHz, 2 GB RAM. [P] — [product page](https://mikrotik.com/product/hap_be3_media)

**MLO**
- hAP be³ Media "supports Wi-Fi 7 Multi-Link Operation (MLO) in TR-MLMR (AP mode)". [P] — [product page](https://mikrotik.com/product/hap_be3_media). Some retailers say MLSR instead; the MikroTik page is authoritative. [U] — e.g. [retailer listings](https://www.shop.wirelessnetware.ca/wireless-for-home-and-office/709-mikrotik-hap-be3-media-wi-fi-7-router-4752224007940.html)
- `mlo` property (default yes), `mld-interface` and `mld-name`. Interfaces sharing an MLD act as one AP MLD. All must share the SSID and be on the same device. [P] — [Wi-Fi manual](https://manual.mikrotik.com/docs/wireless/wifi/)
- Radios have `ml-group`. [P] — [WiFi](https://help.mikrotik.com/docs/spaces/ROS/pages/224559120/WiFi)
- Compatible hardware named by MikroTik: `wifi-qcom-be` (hAP be3 media) and `wifi-mediatek` (hAP be lite). [P] — [Wi-Fi manual](https://manual.mikrotik.com/docs/wireless/wifi/)
- `traffic-processing=on-capsman` is not supported with MLO/MLD. [P] — same
- MLO parameters were documented on 2026-05-26. [P] — [docs changelog](https://manual.mikrotik.com/changelog/changelog-2026-05-26/)
- Maturity from changelogs [P]:
  - 7.19 added the `wifi-mediatek` package.
  - 7.21 "configuration parameters relevant to the upcoming WiFi 7 products".
  - 7.24 MLO roaming and channel-switch work.
  - [7.25](https://upgrade.mikrotik.com/routeros/7.25/CHANGELOG) fixed per-link client statistics, station statistics after link switch, FT for MLD, and MLD in station mode.
  - [7.26beta5](https://upgrade.mikrotik.com/routeros/7.26beta5/CHANGELOG) fixes "missing mld-datapath in CAP default configuration".

**Package differences**

| Package | Scope | Notes | Label |
|---|---|---|---|
| `wifi-qcom-ac` | Wi-Fi 5, ARM only, not MIPS | "much more resource-heavy". No spectral scan, no Superchannel, no `ft-preserve-vlanid`, no dscp-high-3-bits classifier, no VLAN offload to wifi; access-list VLAN needs interface pvid; multi-passphrase vlan-id is unsupported | [P] [Wi-Fi manual](https://manual.mikrotik.com/docs/wireless/wifi/), [WiFi](https://help.mikrotik.com/docs/spaces/ROS/pages/224559120/WiFi) |
| `wifi-qcom` | Wi-Fi 6/6E | Frozen page said "802.11ax and newer", current manual separates BE | [P] same |
| `wifi-qcom-be` | Wi-Fi 7, e.g. hAP be3 media | `distance` added in 7.25 | [P] [7.25 changelog](https://upgrade.mikrotik.com/routeros/7.25/CHANGELOG) |
| `wifi-mediatek` | Wi-Fi 7, e.g. hAP be lite | Separate driver, own fixes | [P] same |

**Differences from legacy `wireless`**
- Different menu. Lost: Nstreme, Nv2, VLAN settings in wireless configuration, and station-bridge compatibility in either direction. [P] — [Wi-Fi manual](https://manual.mikrotik.com/docs/wireless/wifi/)
- Legacy `wireless` is a standalone package since 7.13. [P] — [7.13 changelog](https://upgrade.mikrotik.com/routeros/7.13/CHANGELOG)

### Inferences
- [I] A controller needs two wifi adapters (`/interface/wifi` and legacy `/interface/wireless`), plus per-package capability flags. Legacy CAPsMAN is a third surface.
- [I] `print` shows configured and inherited values, not necessarily the live channel. A setting like `20/40/80mhz` or a frequency list does not tell you what the radio chose. Use `/interface/wifi/monitor` for runtime state.
- [I] When a value is unset everywhere, expect the key to be missing rather than empty. Code must tolerate absence of `channel.band`.
- [I] The 7.25 station-mode MLD fix suggests station-side MLD exists on some driver, despite "AP mode" on the be³ Media page.

### Gaps
- No full field list for `/interface/wifi/monitor`. The manual documents only the puncturing marker.
- Whether the registration table exposes per-link MLD fields: not documented. 7.25 changelog implies per-link statistics exist.
- Which security types MLO allows, and MLO in station mode: not stated.
- Exact menu location of the `mlo` property: not verified.
- Whether print on CAPsMAN-managed or `master-interface` virtual APs shows inherited values the same way: not confirmed.
- 6 GHz availability per regulatory country: not researched.

## Dotted properties in `proplist`, `as-value`, `:serialize`

### Takeaway
Probably works, but I could not confirm it. The arguments exist and the names are valid parameter names, yet no document, forum post or public code I found shows `proplist=` with dotted wifi names. Test it on the target version before relying on it. Separately, `:serialize` has a real trap: numeric-looking strings become JSON numbers by default.

### Cited Findings
- `/interface/wifi/print` has `as-value`, `proplist`, `config`, `show-sensitive`, `where`, `order-by` on 7.25. [S] — [restraml](https://tikoci.github.io/restraml/)
- `as-value` gives "an array of parameters and its values". [P] — [Scripting](https://help.mikrotik.com/docs/spaces/ROS/pages/47579229/Scripting)
- Printing to value includes flags by default since 7.20. [P] — [7.20 changelog](https://upgrade.mikrotik.com/routeros/7.20/CHANGELOG)
- 7.18 "fixed 'get' and 'proplist' for certain settings", so per-setting proplist bugs have existed. [P] — [7.18 changelog](https://upgrade.mikrotik.com/routeros/7.18/CHANGELOG)
- `:serialize`: `to=json|dsv`, `json.pretty`, `json.no-string-conversion`. By default the string `"1234"` serialises as `1234.000000`. With the flag it stays a string. [P] — [Scripting](https://help.mikrotik.com/docs/spaces/ROS/pages/47579229/Scripting)
- `:serialize` was added in 7.13 and `json.no-string-conversion` in 7.17. [P] — [7.13](https://upgrade.mikrotik.com/routeros/7.13/CHANGELOG), [7.17](https://upgrade.mikrotik.com/routeros/7.17/CHANGELOG)
- `:put [:serialize to=json [/system script print as-value]]` is a working pattern. `proplist=name,type` returns `.id`, `name`, `type`. [L] — [tik4net CLI tips](https://github.com/danikf/tik4net/wiki/CLI-tips-and-tricks)
- Raw `as-value` strings have no escaping, and "Before 7.20 a bare dotted value can be refused". That note is about values. [L] — same
- A bare `print as-value` at the console prints nothing and needs `:put [...]`. [L] — same
- `.proplist` in REST and API takes comma-separated names. [P] — [REST API 7.25](https://manual.mikrotik.com/docs/7.25/developer-guides/rest-api/)
- 7.23 and 7.24 made bad command parameters runtime errors. [P] — [7.23](https://upgrade.mikrotik.com/routeros/7.23/CHANGELOG), [7.24](https://upgrade.mikrotik.com/routeros/7.24/CHANGELOG)
- A property listed in `.proplist` but missing from an item evaluates as false in queries. [P] — [API](https://help.mikrotik.com/docs/spaces/ROS/pages/47579160/API)

### Inferences
- [I] A `proplist=` argument is itself a value, so the pre-7.20 dotted-value note may apply. Quote it: `proplist="name,channel.band,channel.width,configuration.ssid,running,disabled"`.
- [I] A JSON key containing a dot should serialise unchanged, since an array key is just a string. In scripts read it with `$row->"channel.band"`.
- [I] A numeric SSID such as `1234` will corrupt the snapshot without `json.no-string-conversion`.
- [I] Non-UTF-8 bytes in an SSID may produce invalid JSON, because RouterOS stores raw bytes. Test this.
- [I] Lowest-risk option: skip the script entirely. Send `/interface/wifi/print` with `=.proplist=...` over the API, or `POST /rest/interface/wifi/print`. If dotted proplist misbehaves, drop the proplist and filter client-side.

### Gaps
- **Could not confirm** whether `proplist` accepts dotted wifi names on any version. GitHub code search for the exact pattern returned nothing useful. tik4net's `InterfaceWifi` entity maps `aaa.*` dotted fields but I found no `channel.*` or `configuration.*` mappings there. A one-line test on a router settles it.
- Whether `running` and `disabled` come through `proplist` as flags in `as-value` on pre-7.20: unknown.

## Security: least privilege and secrets

### Takeaway
Use a custom group, because the built-in `read` group is over-privileged. Policies are global, not per menu, so a writer can change the whole config. The `sensitive` policy works, but its coverage has been patched one property at a time, and secrets leak through backups and through anything the controller stores.

### Cited Findings
- Policies: local, telnet, ssh, web, winbox, password, api, rest-api, ftp, romon, reboot, read, write, policy, test, sensitive, sniff. [P] — [User](https://help.mikrotik.com/docs/spaces/ROS/pages/8978504/User)
- `write` excludes user management. `policy` is needed for it. [P] — same
- The default `read` group includes `sensitive`, `reboot` and others. Create a custom group. [P] — same. The manual also recommends custom groups for REST. [P] — [REST API 7.25](https://manual.mikrotik.com/docs/7.25/developer-guides/rest-api/)
- `sensitive`: "grants rights to change 'hide sensitive' option", and "if this policy is disabled sensitive information is not displayed". [P] — [User](https://help.mikrotik.com/docs/spaces/ROS/pages/8978504/User)
- Failure signatures: no `rest-api` or bad password gives 401; no `api` gives 500 `std failure: not allowed (9)`; no command policy gives 500 `not enough permissions (9)`; `PATCH` without `write` gives 400. [P] — [REST API 7.25](https://manual.mikrotik.com/docs/7.25/developer-guides/rest-api/)
- Sensitive menu list includes `interface/wifi` (passphrase, eap-password), `interface/wireguard`, `ppp/secret`, `ip/ipsec`, `certificate`. Several entries are marked Work in Progress. [P] — [List of sensitive parameters](https://help.mikrotik.com/docs/spaces/ROS/pages/380076066/List+of+menus+with+sensitive+parameters)
- 7.21: "added show-sensitive option for print command, hide sensitive settings in print output by default". [P] — [7.21 changelog](https://upgrade.mikrotik.com/routeros/7.21/CHANGELOG)
- Properties only marked sensitive recently [P]:
  - [7.14](https://upgrade.mikrotik.com/routeros/7.14/CHANGELOG): 6to4 ipsec-secret
  - [7.16](https://upgrade.mikrotik.com/routeros/7.16/CHANGELOG): MQTT broker password in export
  - [7.19](https://upgrade.mikrotik.com/routeros/7.19/CHANGELOG): OSPF auth-key
  - [7.21](https://upgrade.mikrotik.com/routeros/7.21/CHANGELOG): BTH private key
  - [7.22](https://upgrade.mikrotik.com/routeros/7.22/CHANGELOG): hotspot otp-secret
  - [7.24](https://upgrade.mikrotik.com/routeros/7.24/CHANGELOG): app secrets, PPP pin, hotspot mac-auth-password, update mirror password
- Export leak: `hide-sensitive` export exposed WireGuard keys in 7.21rc2, resolved in 7.21rc5. [U] — [forum](https://forum.mikrotik.com/t/critical-flaw-with-hide-sensitive-config-export/267287)
- Backups: without `password`, the file is unencrypted. `rc4` is insecure. [P] — [backup save](https://manual.mikrotik.com/docs/7.25/cli-reference/system/backup/save/)
- `/export` hides sensitive data by default. User passwords and SSH keys cannot be exported. [P] — [Configuration Management](https://manual.mikrotik.com/docs/7.25/getting-started/configuration-management/)
- A pending device-mode change can be left by a remote user and activate at the next button press or power cycle. [P] — [Device mode](https://manual.mikrotik.com/docs/system-information-and-utilities/device-mode/)
- A third-party project fixed proplist injection after splicing names unquoted into CLI commands. [U] — [mikrotik-mcp PR 6](https://github.com/kije/mikrotik-mcp/pull/6)

### Inferences
- [I] Read-only monitor group: `api` + `read`, plus `rest-api` if used, plus `test` only if ping is needed. Writer group adds `write`. Add `sensitive` only for a separate flow that edits passphrases. Never add `policy`.
- [I] No menu-level ACL appears in the policy list, so "firewall only" users are not possible.
- [I] A script or scheduler `on-event` body is not listed as sensitive. Do not embed secrets in revert scripts, because any `read` user can see them.
- [I] Any write user can effectively extract secrets through backups or scripts unless tested otherwise. Treat the writer credential as equal to admin in the threat model.
- [I] The controller's own store of router credentials, exports and backups is the largest leak surface.

### Gaps
- Whether API and REST `print` hide secrets from a user **with** `sensitive` since 7.21, and whether `=show-sensitive=` is needed: no source.
- Which policy `/system/backup/save` needs: not confirmed. The Backup manual page returned 404.
- Whether backups embed passphrases in the clear when unencrypted: inferred, not read in a primary page.

## Safety of remote changes

### Takeaway
Safe Mode is no longer console-only: 7.18 added a scriptable `/safe-mode` menu, and a library's integration tests use it over the binary API. It is not available over REST. MikroTik has not documented API use, and it has sharp limits: router-wide scope, a single owner, a 100-action cap, and rollback that can take up to 9 minutes after a dead connection. The controller must add its own verification and a second safety net.

### Cited Findings

**Safe Mode basics [P]** — [Configuration Management](https://manual.mikrotik.com/docs/7.25/getting-started/configuration-management/)
- Entered by WinBox button or F4 / Ctrl-X in CLI. Ctrl-D exits without saving. `/quit` does not undo.
- All changes made while it is on, including from other sessions, are undone if the session terminates abnormally.
- If the connection is cut, changes undo after the "TCP timeout is 9 minutes".
- Covers the 100 most recent history actions. If there is no room, the session leaves safe mode and nothing is undone automatically.
- A second user can unroll, release, or leave it alone.

**API availability**
- 7.18: "implement scriptable safe-mode commands and safe-mode handler". [P] — [7.18 changelog](https://upgrade.mikrotik.com/routeros/7.18/CHANGELOG)
- `/safe-mode` commands: `get`, `print`, `take`, `release`, `unroll`, `toggle`, `export`. `take` has `on-error`. [S] — [restraml](https://tikoci.github.io/restraml/)
- `on-error=abort|ask|release|unroll`; owner values winbox, script, console. [U] — [forum](https://forum.mikrotik.com/t/scriptable-safe-mode/264989)
- The manual's reference shows only read-only `enabled`, `user`, `current`, `owner`, and does not mention the API. [P] — [safe-mode reference](https://manual.mikrotik.com/docs/7.25/cli-reference/safe-mode/)
- tik4net uses `/safe-mode` over the binary API on 7.18+, with integration tests including a rollback test. REST is unsupported. [L] — [interface source](https://github.com/danikf/tik4net/blob/master/tik4net/ITikSafeModeConnection.cs), [tests](https://github.com/danikf/tik4net/blob/master/tik4net.integrationtests/SafeModeTest.cs)
- Its caveats: stuck lock after a dropped session, rollback lag of a few seconds after clean close, rollback is router-wide, and every command during the hold takes part. [L] — [tik4net Safe Mode](https://github.com/danikf/tik4net/wiki/Safe-Mode)
- Its test comments say RouterOS "may defer the rollback of a graceful close to the connection-tracking timeout on some transports", and the test self-reports Inconclusive past 30 s. [L] — [tests](https://github.com/danikf/tik4net/blob/master/tik4net.integrationtests/SafeModeTest.cs)
- Older forum threads requesting API Safe Mode show it was not available earlier. [U] — [feature request](https://forum.mikrotik.com/t/feature-request-safe-mode-for-api/98028)
- Advice from a forum regular: "I wouldn't trust safe mode too much". No staff replied in the thread. [U] — [forum](https://forum.mikrotik.com/t/scriptable-safe-mode/264989)
- 7.22 "fixed route removal after unexpected safe mode termination", so rollback has had bugs. [P] — [7.22 changelog](https://upgrade.mikrotik.com/routeros/7.22/CHANGELOG)

**Other rollback tools**
- History undo and redo cover actions in `/system/history`. [P] — [Configuration Management](https://manual.mikrotik.com/docs/7.25/getting-started/configuration-management/)
- `/import` got a dry-run parameter for syntax checks in 7.16. [P] — [7.16 changelog](https://upgrade.mikrotik.com/routeros/7.16/CHANGELOG)
- MikroTik has no native commit-confirm, per forum users. Common pattern: back up, schedule a restore in a few minutes, remove the job on success. [U] — [forum](https://forum.mikrotik.com/t/commit-confirm-fashion-command-available/88542), [watchdog](https://forum.mikrotik.com/t/config-watchdog-toggable-auto-restore-for-remote-work/44982)
- A third-party tool uses a self-removing scheduler with explicit revert commands. [U] — [arm_dead_man](https://glama.ai/mcp/servers/thalisantunes/mcp-mikrotik/tools/arm_dead_man)
- Scheduler facts: unavailable in `home` mode; entries run with the policies of the user who added them; `interval=0s` runs once; `start-time=startup` runs before interfaces are up. [P] — [Scheduler](https://manual.mikrotik.com/docs/system-information-and-utilities/scheduler/)
- Device modes: home routers default to `home`, CCR and 1100 default to `advanced`, others `basic`. Pre-mode devices stay `advanced`. Changing needs physical confirmation within 5 minutes. [P] — [Device mode](https://manual.mikrotik.com/docs/system-information-and-utilities/device-mode/)
- `/system/backup` is a binary clone. `/export` omits user passwords, SSH keys and certificates, among others. [P] — [Configuration Management](https://manual.mikrotik.com/docs/7.25/getting-started/configuration-management/)
- Export ordering changed in 7.25 ("improve export order"), so text diffs across upgrades will be noisy. [P] — [7.25 changelog](https://upgrade.mikrotik.com/routeros/7.25/CHANGELOG)

**Bridge VLAN filtering and lockouts [P]** — [Bridge VLAN Table](https://help.mikrotik.com/docs/spaces/ROS/pages/28606465/Bridge+VLAN+Table)
- "you might get locked out from the device because of the lack of management access."
- "It is always recommended to configure VLAN filtering while using a serial console."
- Enabling can disconnect you, because "the bridge must reset itself in order for VLAN filtering to take any effect."
- "PVID has no effect until VLAN filtering is enabled", so set PVIDs first.
- `frame-type` needs `ingress-filtering`. Management over a tagged port needs tag support on the peer.
- Some switch chips have VLAN limits (Marvell-98DX3257 on 1 Gbps ports).

Changelog notes [P]:
- [7.16](https://upgrade.mikrotik.com/routeros/7.16/CHANGELOG) adds a dynamic tagged entry when a VLAN interface is created on a filtering bridge.
- [7.22](https://upgrade.mikrotik.com/routeros/7.22/CHANGELOG) fixed a vlan-filtering performance regression from 7.20.
- [7.25](https://upgrade.mikrotik.com/routeros/7.25/CHANGELOG) warns about VLAN entries when filtering is disabled.

### Inferences
- [I] Safe Mode only reacts to session death. A change that blocks **new** connections but leaves the existing TCP session up will not trigger rollback. Before `release`, verify over a **fresh** connection from the controller.
- [I] Recommended write flow:
  1. Check `/safe-mode/get` for another owner.
  2. Take with `on-error=abort`.
  3. Apply a batch well under 100 actions.
  4. Open a new connection and test reachability.
  5. Release. On failure, unroll or drop the session.
- [I] Worst case after a real lockout is about 9 minutes down, and the UI must tell the user that instead of implying an instant revert.
- [I] Use `abort`, never `unroll`, by default. `unroll` would discard a human admin's in-progress WinBox changes.
- [I] Because rollback is router-wide, the controller must take a per-router lock, and any other writer during the hold is also rolled back.
- [I] Second safety net where the scheduler exists: a one-shot revert job with explicit inverse commands, not `backup load`. A full restore can itself cause a lockout, and I expect it to reboot the router. Neither point is sourced, so test it.
- [I] On `home` devices, a detached `/execute` job with `:delay` might substitute for the scheduler. It would not survive reboot. Untested.
- [I] VLAN filtering toggle is the riskiest action. The UI should pre-check that the controller's path has a bridge VLAN entry and management interface, refuse when the controller's own path crosses the bridge without one, and require an explicit override. Toggling can also reset the bridge and break the Safe Mode session, which would cause an automatic rollback. That is acceptable but needs retry logic.
- [I] Also treat as high risk: `/ip/service`, `available-from`, firewall input chain, interface lists, `/ip/address` on the management interface, and wifi changes when the operator is on that wifi.

### Gaps
- No MikroTik statement that `/safe-mode` over the API is supported. Evidence is schema plus third-party tests.
- Whether `/rest/safe-mode/take` is accepted, and what happens: tik4net says no Safe Mode on REST, but it may be refused or tied to the internal 10-minute login. Do not rely on it.
- Is the 100-action limit counted per command or per item? Which actions are outside history and so not undone (file ops, upgrades, certificates)? Not documented.
- Behaviour across reboot during Safe Mode: not confirmed.
- Whether hAP be³ Media ships in `home` mode: the docs give only categories, not models. Check `/system/device-mode/print`.
- Whether a `backup load` flow restores cleanly on the same device after a package change: not researched. The Backup manual page returned 404.

## API stability and breaking changes across 7.x

### Takeaway
Yes, there are real breaking changes in nearly every minor release: property renames, changed output, and new defaults. Most are small, some have deprecation aliases, and none carry a formal API version. A controller needs a version-gated adapter and a test run per release.

### Cited Findings

Entries are from official changelogs [P].

| Version | Date | Change |
|---|---|---|
| [7.13](https://upgrade.mikrotik.com/routeros/7.13/CHANGELOG) | date not read | `wifiwave2` split into `wifi-qcom`, `wifi-qcom-ac`; menu is now `wifi`; `wireless` separate package |
| [7.15](https://upgrade.mikrotik.com/routeros/7.15/CHANGELOG) | 2024-05-29 | wifi `print` shows inherited values; replaces `actual-configuration` |
| [7.16](https://upgrade.mikrotik.com/routeros/7.16/CHANGELOG) | 2024-09-20 | `follow-strict` removed |
| [7.17](https://upgrade.mikrotik.com/routeros/7.17/CHANGELOG) | 2025-01-16 | device-mode `enterprise` renamed `advanced` |
| [7.18](https://upgrade.mikrotik.com/routeros/7.18/CHANGELOG) | 2025-02-24 | API sends `!empty` when a query returns nothing; `back-to-home-users` renamed |
| [7.19](https://upgrade.mikrotik.com/routeros/7.19/CHANGELOG) | 2025-05-22 | bridge STP monitor fields renamed |
| [7.20](https://upgrade.mikrotik.com/routeros/7.20/CHANGELOG) | 2025-09-29 | flags included in print-to-value; ID string form unified; `/user/active/request-logout` becomes `remove` |
| [7.21](https://upgrade.mikrotik.com/routeros/7.21/CHANGELOG) | 2026-01-12 | print hides sensitive by default; file id format changed; `/quickset` removed |
| [7.22](https://upgrade.mikrotik.com/routeros/7.22/CHANGELOG) | 2026-03-09 | `follow` disabled in `/ip/firewall/connection`; ACME change needs service reconfig |
| [7.23](https://upgrade.mikrotik.com/routeros/7.23/CHANGELOG) | 2026-05-25 | unknown command parameters become runtime errors |
| [7.24](https://upgrade.mikrotik.com/routeros/7.24/CHANGELOG) | 2026-08-14 | `/ip/service` `address` renamed `available-from` (backwards compatible via deprecation); `reauth-timeout` renamed; `factory-software` renamed `minimum-version`; WiFi registration comments restricted |
| [7.25](https://upgrade.mikrotik.com/routeros/7.25/CHANGELOG) | 2026-10-22 | `where` flag filter fix (changes results); globals exposed in API; export order changed |

Other points:
- The `!empty` reply type is documented for 7.18+. [P] — [API](https://help.mikrotik.com/docs/spaces/ROS/pages/47579160/API)
- Upgrades through 7.12 were needed to convert wireless packages automatically. [P] — [7.13 changelog](https://upgrade.mikrotik.com/routeros/7.13/CHANGELOG)
- Regressions are common enough that changelogs label them "introduced in vX". Examples: 7.20 VLAN filtering performance, and 7.21 REST sessions. [P] — [7.22](https://upgrade.mikrotik.com/routeros/7.22/CHANGELOG), [7.25](https://upgrade.mikrotik.com/routeros/7.25/CHANGELOG)
- Point releases also move things. Third-party mirrors report 7.22.1 multi-link VLAN fixes and 7.22.2 REST cancel stability, which I did not check against the official files. [U] — [7.22.2 mirror](https://newreleases.io/project/github/elseif/MikroTikPatch/release/7.22.2-arm64)
- Schema diffs between any two versions are available from a third party. [U] — [restraml diff](https://tikoci.github.io/restraml/diff.html)

### Inferences
- [I] Set a supported floor of 7.18 (Safe Mode, `!empty`). For REST use, a floor of 7.25 avoids the session bug and the auth-header memory leak.
- [I] Feature-detect from `/console/inspect` instead of hard-coding version checks. Run a contract test on CHR in CI for each release, using the schema diff as an alert.
- [I] Prefer long-term 7.24.x as the baseline, but note that many MLO fixes are only in 7.25, so WiFi 7 users may need stable.

### Gaps
- No published API compatibility policy from MikroTik. I found none.
- Changelogs were read only for x.y.0 releases, plus 7.24.6, 7.25.1 and 7.26beta5. Other point releases were not reviewed.
- When the `rest-api` policy was introduced, and whether older groups broke on upgrade: not found in the changelogs I filtered.
