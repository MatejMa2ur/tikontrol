# MikroTik CMR and existing MikroTik controller / UniFi-like projects (state as of 2026-11-04)

Evidence labels used in Cited Findings: **[PRIMARY]** = MikroTik manual, MikroTik download/upgrade servers, MikroTik staff post, a project's own repo or GitHub API. **[VENDOR]** = a third-party vendor's claim about its own product. **[USER]** = forum or Reddit user report. Inference is kept in the Inferences sections.

Method caveat: most web pages were read through a summarising fetch tool. Items I checked byte-for-byte are the release channels and package listings (MikroTik servers), the manual text (raw HTML grep), GitHub numbers (GitHub API), and the normis and sergejs posts of 2026-10-06 (forum JSON). Other forum quotes are as returned by the fetch tool and may have small wording differences.

## Bottom line: is tikontrol needed?

### Takeaway
A new, independent, general "UniFi for MikroTik" is **largely redundant, but not because of CMR alone**. CMR is still beta and deliberately leaves out IP, DHCP, routing, firewall and users. The redundancy comes from third parties: 2GT MikroTik Manager (AGPL, 248 stars, 1.0 stable), MikroDash (639 stars), mikr / mikr.app, nasnet-panel, plus a burst of new projects in 2026. A project is only justified with a narrow differentiator these do not cover, and the most defensible shape is a thin UI over the CMR REST API or a contribution to an existing project.

### Cited Findings
- MikroTik staff openly invite third parties to build UIs on CMR: "All CMR stuff is available via REST API. Any missing feature or design improvement you could ever want, can be made by LLM, you could build a tiny Container "App" which runs a small webserver and a nice Web GUI" (normis, 2026-10-06) **[PRIMARY]** — [Forum](https://forum.mikrotik.com/t/claude-and-codex-try-out-cmr-in-7-26beta-mostly-successful/273613)
- CMR's scope excludes "IP addresses, DHCP, routing, firewall, users, or other device settings" **[PRIMARY]** — [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/)
- 7.26 is not stable: the development channel is 7.26beta5 (2026-10-30), stable and testing are 7.25.1 (2026-11-02), long-term is 7.24.6 **[PRIMARY]** — [stable](https://upgrade.mikrotik.com/routeros/NEWESTa7.stable), [development](https://upgrade.mikrotik.com/routeros/NEWESTa7.development)
- 2GT MikroTik Manager already covers VLAN and SSID editing, firewall, firmware rollouts, backups, lockout prevention and auto-revert **[PRIMARY, project README]** — [Repo](https://github.com/2GT-Media-Group-LLC/mikrotik-manager)

### Inferences
- Evidence for redundancy is strong for the feature list. It is weaker for quality: I did not run any of these tools, and star counts are not a measure of reliability.
- What still looks open, as a hypothesis to test: a single-box, on-router, home/SMB product with a UniFi-grade guided experience (adopt, then manage networks as intents across gateway, switches and APs) that combines CMR's zero-touch client with the gateway functions CMR refuses to touch. Nobody I found clearly delivers this today, but several are close and all are moving fast (see the projects section).
- Biggest strategic risk: CMR is one month old in public and is labelled "initial implementation". MikroTik can extend it, and its WebFig UI already has dashboard and topology. Any feature built only on CMR's gaps may be absorbed.
- Lowest-regret paths, in order: (1) use CMR plus MikroTik Manager or MikroDash and do not build; (2) contribute to or fork an existing MIT/AGPL project; (3) build a thin opinionated front end on CMR REST with a small backend proxy; (4) build a full independent controller, which is the option the evidence argues against.

### Gaps
- Nothing here measures whether the existing tools are actually good enough for a specific user. That requires hands-on trials.

## MikroTik CMR: what it is, status, cost

### Takeaway
CMR is MikroTik's first-party fleet platform: one RouterOS device runs the `cmr` package as server, and other devices connect with a client built into RouterOS. It is **beta only** (introduced in 7.26beta1, about 2026-10-05, not in any stable channel). No price or licence fee was found anywhere; it ships as a free extra package.

### Cited Findings
- Described as "a centralized monitoring platform for fleets of RouterOS devices" **[PRIMARY]** — [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/)
- Features listed: inventory (identity, board, version, packages, state, labels); fleet upgrades by channel or pinned version with schedules, group order and fail policies; alert rules that log, run scripts or call webhooks; run-script and commands on selected devices; live dashboard of resources, interfaces, traffic and WiFi; topology maps in the GUI; application traffic statistics; WiFi network and radio provisioning; VLAN provisioning for access and trunk ports **[PRIMARY]** — [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/)
- Changelog 7.26beta1: "add initial implementation of centralised platform for RouterOS devices". Betas 3 to 5 carry many CMR fixes, including a beta4 fix for "VLAN rules being applied to all ports when port-labels is not set" **[PRIMARY]** — [7.26beta thread](https://forum.mikrotik.com/t/7-26beta-development-is-released/273582)
- Staff: "initial implementation" or WIP, "Things will improve and adapt" (strods) **[PRIMARY, staff role per forum search result]** — [7.26beta thread](https://forum.mikrotik.com/t/7-26beta-development-is-released/273582)
- sergejs (2026-10-06): "7.26 will be out soon"; a client pairing mode "was removed/frozen on the final steps of release" **[PRIMARY]** — [Forum](https://forum.mikrotik.com/t/claude-and-codex-try-out-cmr-in-7-26beta-mostly-successful/273613)
- As of 2026-10-27 the docs changelog referred to a "RouterOS 7.26 test build" **[PRIMARY]** — [Docs changes](https://manual.mikrotik.com/changelog/)
- Clients find the server through MNDP, DNS (`_cmr._tcp.lan`, `_cmr._tcp.<domain>`) or `controller-addresses`, and connect on TCP 54321 **[PRIMARY]** — [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/)
- CMR-managed objects are read-only on clients and stay active when the server is unreachable **[PRIMARY]** — [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/)
- Application classification sees traffic the device routes (so normally the gateway) and is unavailable on mmips, smips and powerpc **[PRIMARY]** — [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/)
- Sharp edges in the manual: VLAN provisioning moves ports into `cmr-bridge`, which "can cut the device's management access", and a rule with no labels hits all devices and ports; removing a CMR WiFi network does not restore the earlier radio config; only one upgrade job at a time; changing some settings restarts CMR on the server **[PRIMARY]** — [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/)
- CMR "works best with devices that run their default configuration" and "does not adapt its objects to custom bridge, VLAN, or WiFi setups" **[PRIMARY]** — [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/)
- Community test on 7.26beta1 CHR: pairing, run-script, alerts, webhooks and backup restore mostly worked; `/cmr/export` omits server settings and labels; the default stable upgrade rule offered a downgrade **[USER]** — [Forum](https://forum.mikrotik.com/t/claude-and-codex-try-out-cmr-in-7-26beta-mostly-successful/273613)
- Security report: a controller login without `write` or `reboot` could still run `run-script` and reboot clients; no staff reply seen **[USER]** — [Forum](https://forum.mikrotik.com/t/claude-and-codex-try-out-cmr-in-7-26beta-mostly-successful/273613)

### Inferences
- No separate charge is likely because the package sits in the normal free `all_packages` download, but the manual never says "free".
- CMR takes over rather than adopts, so it suits greenfield or near-default networks far better than an existing hand-tuned MikroTik network.

### Gaps
- No official 7.26 stable date. No official statement on pricing, future paid tiers or a long-term roadmap. No mikrotik.com news post on CMR was found.
- Does CMR manage legacy `wireless` package radios? The manual does not say. The only package named is `wifi-qcom`, in an upgrade example.

## CMR server requirements: hAP be³ Media, hAP ac², CHR

### Takeaway
Officially only the architectures are published: **arm, arm64, x86**. RAM, storage, licence level and device limits are unpublished. hAP be³ Media should be a good fit. hAP ac² is formally on a supported architecture but is a poor candidate and untested. CHR works in community tests, and the free tier's 1 Mbit limit is a concern mainly for pushing firmware.

### Cited Findings
- "The `cmr` package is supported only on devices with the arm, arm64, and x86 architectures." Clients: any device except smips and powerpc **[PRIMARY]** — [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/), [Packages](https://manual.mikrotik.com/docs/getting-started/installation-and-upgrade/packages/)
- Package files in 7.26beta5: `cmr-7.26beta5-arm64.npk` 266,385 bytes, `-arm.npk` 266,385 bytes, x86 319,633 bytes; no cmr package in the mipsbe or mmips bundles **[PRIMARY, inspected zip listings]** — [download.mikrotik.com](https://download.mikrotik.com/routeros/7.26beta5/all_packages-arm64-7.26beta5.zip)
- normis (2026-10-05): "CMR server should only be installed on powerful ARM64 routers with enough space"; next day: "RB4011 is powerful, you can use CMR there". Also: "CMR client does not take up extra space and is built into RouterOS." **[PRIMARY, via fetch tool]** — [Thread p.2](https://forum.mikrotik.com/t/7-26beta-development-is-released/273582?page=2), [Thread p.3](https://forum.mikrotik.com/t/7-26beta-development-is-released/273582?page=3)
- Upgrade packages are stored by the server; if the directory is unusable it stores them in RAM. A `packages-cache-limit` above available storage is accepted and then uses all available space **[PRIMARY]** — [CLI reference](https://manual.mikrotik.com/docs/cli-reference/cmr/), [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/)
- "No published device limits or performance figures were found" **[USER]** — [Forum](https://forum.mikrotik.com/t/claude-and-codex-try-out-cmr-in-7-26beta-mostly-successful/273613)
- hAP be³ Media: ARM 64-bit, IPQ-5322, 4 cores up to 1.5 GHz, 2 GB RAM, 512 MB storage **[PRIMARY]** — [Product page](https://mikrotik.com/product/hap_be3_media). A retailer lists licence level 6 **[retailer]** — [100mega](https://b2b.100mega.com/en/330915-mikrotik-hap-be3-media)
- hAP ac²: IPQ-4018 quad-core ARM 32-bit, 128 MB RAM, 16 MB flash **[PRIMARY]** — [Product page](https://mikrotik.com/product/hap_ac2)
- CHR ran the server in community tests on 7.26beta1 **[USER]** — [Forum](https://forum.mikrotik.com/t/claude-and-codex-try-out-cmr-in-7-26beta-mostly-successful/273613); the Go project also validated against a 7.26 CHR, there as a client device **[PRIMARY, project README]** — [go-mikrotik-cmr](https://github.com/AthenaNetworks/go-mikrotik-cmr)
- CHR free tier: 1 Mbit upload per interface; P1 1 Gbit $45, P10 10 Gbit $95, P-Unlimited $250; 60-day trial of paid levels. Minimum 256 MB RAM for v7, with at least 1024 MiB advised **[PRIMARY, older help.mikrotik.com page, may lag the current manual]** — [CHR docs](https://help.mikrotik.com/docs/spaces/ROS/pages/18350234/Cloud+Hosted+Router+CHR)

### Inferences
- **hAP be³ Media**: arm64 with 2 GB RAM fits the staff guidance. It is likely the right class of device, but "powerful" is undefined and nobody has published a test on this exact model. The 512 MB of storage must also hold the package cache, so use microSD or USB if serving many architectures.
- **hAP ac²**: the arm package exists and is only 266 KB, so it may well install. With 128 MB RAM, 16 MB flash and no room for a firmware cache, it falls far outside "powerful ... with enough space". Treat as client only. Being 32-bit ARM is not in itself the blocker: normis approved the RB4011, which from background knowledge (not verified in this run) is also 32-bit ARM.
- **Free CHR**: the limit is on upload per interface, so telemetry and API use are probably fine for a small fleet. Pushing firmware from the server to clients would be slow; a roughly 15 MB package at 1 Mbit takes around two minutes per device, and that package size is an estimate. CMR's package distribution goes through the server, so this cap applies to it. A P1 licence removes the issue.

### Gaps
- No primary source for: minimum RAM, storage, licence level, or supported device counts. No confirmation that CMR works on free-tier CHR specifically (the licence used in forum tests is not stated). No measured CMR memory use anywhere.

## CMR UI, API, and what is missing versus UniFi Network

### Takeaway
CMR has a web UI: **WebFig is the primary UI**, WinBox 4 also works, WinBox 3 does not. It is served by the router, not as a separate app. It has **REST access to all CMR menus per staff**, but no CMR-specific REST reference, no versioning, and real limits for browser apps. Versus UniFi it is missing most gateway and policy functions.

### Cited Findings
- "Use WinBox 4 or WebFig for the CMR menus. WinBox 3 does not support them." **[PRIMARY]** — [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/)
- normis: "For most of the functionality of CMR, we suggest using Webfig right now, it has more features, like maps." strods: "some things are available only through WebFig and not through WInBox yet" **[PRIMARY, via fetch tool]** — [7.26beta thread](https://forum.mikrotik.com/t/7-26beta-development-is-released/273582)
- Early users called the WebFig UI "very good" for an initial implementation and "really nice" **[USER]** — [Thread](https://forum.mikrotik.com/t/7-26beta-development-is-released/273582), [p.3](https://forum.mikrotik.com/t/7-26beta-development-is-released/273582?page=3)
- CLI menus are documented: `/cmr`, `/cmr/device` (pair, run-script, upgrade, reboot, wifi-logs, apptraffic, dashboard), `/cmr/upgrade`, `/cmr/alert`, `/cmr/layout`, `/cmr/wifi`, `/cmr/wifi/radio`, `/cmr/vlan`, `/cmr/client`, `/cmr/push-button` **[PRIMARY]** — [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/)
- REST: "All CMR stuff is available via REST API" (normis). The manual mentions REST only in passing, for returning WiFi passphrases to users with the `sensitive` policy **[PRIMARY]** — [Forum](https://forum.mikrotik.com/t/claude-and-codex-try-out-cmr-in-7-26beta-mostly-successful/273613), [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/)
- REST pain points: "RouterOS is still missing good CORS support", so an SPA needs a proxy; polling only, no SSE or WebSockets (Amm0). `/cmr/device/dashboard` over REST never returns and other REST calls wait 10 to 30 s behind it (Amm0Bot). 60 s REST timeout, no continuous commands; strings for values; uptime serialised as a date; counters above 2^53; no webhook retries, event IDs, scoped tokens or stable versioned API; a dead client hung a REST `run-script` (Larsa and others) **[USER]** — [Forum](https://forum.mikrotik.com/t/claude-and-codex-try-out-cmr-in-7-26beta-mostly-successful/273613)
- Third-party use already exists: a Home Assistant integration (beta) and normis's own HA plugin **[PRIMARY, repo and staff post]** — [ha-cmr](https://github.com/trakais/ha-cmr), [Forum](https://forum.mikrotik.com/t/claude-and-codex-try-out-cmr-in-7-26beta-mostly-successful/273613)
- The CMR wire protocol has been reverse engineered: a clean-room Go controller pairs stock devices (open pairing only), reads inventory and pushes VLAN config, with an HTTP admin API. Password pairing is not implemented. 0 stars, one author, created 2026-10-16 **[PRIMARY, project README and GitHub API]** — [go-mikrotik-cmr](https://github.com/AthenaNetworks/go-mikrotik-cmr)
- CMR explicit non-goals: no IP, DHCP, routing, firewall or user config. "It does not coordinate roaming between access points or act as a central authentication server." **[PRIMARY]** — [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/)
- A competitor says CMR lacks scheduled backups with diffs, CVE matching, searchable fleet-wide logs, per-site roles and audit trail **[VENDOR, biased]** — [mikr.app](https://mikr.app/)

### Inferences
- Likely gaps versus UniFi Network, by comparing the CMR manual with general knowledge of UniFi (no UniFi primary source consulted): no zone firewall or gateway UI, no guest portal, no per-client policy, no central config backups, no multi-site cloud access, no fleet RBAC, no mobile app for CMR, no adoption of existing config, limited client history.
- A third-party UI needs a backend proxy anyway (CORS, credentials, polling), so "pure SPA on the router" is not viable. A small container or external server is required.
- The REST surface mirrors CLI menus and is beta, so a CMR-based product should expect breaking changes. Using the reverse-engineered protocol is riskier still and brings the pairing limitation.

### Gaps
- No published REST schema or endpoint list for CMR was found. REST for CMR is confirmed by staff and by user integrations, not by a reference page.
- WebFig CMR screens were not personally inspected. The Codex/Claude forum test states it did not inspect WinBox or WebFig rendering.

## CAPsMAN and its relation to CMR

### Takeaway
CMR is not CAPsMAN and, per the manual, does not integrate it. CMR pushes config to APs per radio and can take radios away from CAPsMAN; CAPsMAN remains separate. Staff have not said whether it will be replaced.

### Cited Findings
- "A CMR network takes over the radios it selects and replaces their existing configuration, including radios managed by CAPsMAN and radios with a local configuration." One radio can stay under CAPsMAN while another is under CMR **[PRIMARY]** — [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/)
- "CMR WiFi writes the configuration to each access point. It does not coordinate roaming between access points or act as a central authentication server." **[PRIMARY]** — [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/)
- A CMR WiFi network without a `vlan-id` gives clients no network access, and WiFi interfaces join `cmr-bridge` **[PRIMARY]** — [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/)
- Legacy and new CAPsMAN are separate: wifi CAPsMAN docs are distinct from the `wireless` package docs **[PRIMARY]** — [WiFi CAPsMAN](https://manual.mikrotik.com/docs/wireless/wifi/capsman/). Users report new CAPsMAN is in the base package, legacy needs the `wireless` package, and neither can manage the other's CAPs **[USER]** — [Forum](https://forum.mikrotik.com/t/capsman-on-legacy-and-new-wireless/268774)
- Users asked whether CMR replaces CAPsMAN; replies were guesses ("I guess it does not replace CAPsMAN") **[USER]** — [Thread p.3](https://forum.mikrotik.com/t/7-26beta-development-is-released/273582?page=3)

### Inferences
- On a mixed fleet with ac-generation APs, the legacy-versus-wifi split is a practical trap for any controller. 2GT Manager claims to handle both `wifi` and legacy `wlan`, which is one area where third parties exceed CMR's documented scope.
- CMR's direction (per-AP config, labels, VLAN) looks like a broader successor in spirit, but this is speculation.

### Gaps
- No staff statement on CAPsMAN's future. Did not confirm if CMR WiFi works on `wifi-qcom-ac` devices.

## MikroTik's own roadmap: will it obsolete a third-party UI?

### Takeaway
MikroTik is actively moving in this direction: The Dude was reportedly retired in March 2026, WinBox 4 is replacing WinBox 3, and CMR arrived with WebFig as its lead UI. No public roadmap exists. MikroTik seems to want to own the protocol and basic fleet UI, while explicitly leaving nicer front ends to others.

### Cited Findings
- 2022: sergejs announced "MikroTik is planning to develop and build a controller app for MikroTik Devices"; mrz later gave the initial goal as "a protocol to apply and monitor config" **[PRIMARY, via fetch tool]** — [Forum](https://forum.mikrotik.com/t/mikrotik-devices-controller/158354)
- 2024: a user asked for updates and got no substantive reply **[USER]** — [Forum](https://forum.mikrotik.com/t/any-updates-on-mikrotik-controller-development/176670)
- The Dude: users on 2026-03-25/26 describe it as officially retired, citing an announcement with "no further updates, enhancements are planned for the foreseeable future"; no staff reply on the page and no named successor **[USER]** — [Forum](https://forum.mikrotik.com/t/the-dude-officially-retired/269426). Contradicting a clean removal, the `dude` package is still built for 7.26beta5, dated 2026-10-30 **[PRIMARY, zip listing]** — [download.mikrotik.com](https://download.mikrotik.com/routeros/7.26beta5/all_packages-arm64-7.26beta5.zip)
- WinBox v4.0rc was announced around 2026-02-27 **[PRIMARY]** — [Forum](https://forum.mikrotik.com/t/winbox-v4-0rc-released/268353). A third-party guide says v4.1 was stable by April 2026, unverified — [codegenes](https://www.codegenes.net/blog/winbox-download/). Users say code for WinBox 3 was cut in 7.26 to make room: "as a result we lost Winbox3" **[USER]** — [Thread p.2](https://forum.mikrotik.com/t/7-26beta-development-is-released/273582?page=2)
- Back To Home: WireGuard remote access via a phone app, with relay for NAT; needs RouterOS 7.12+ and ARM, ARM64 or TILE hardware **[PRIMARY]** — [Back To Home](https://manual.mikrotik.com/docs/network-management/cloud/back-to-home)
- Container apps are a staff-promoted extension route (normis) **[PRIMARY]** — [Forum](https://forum.mikrotik.com/t/claude-and-codex-try-out-cmr-in-7-26beta-mostly-successful/273613)

### Inferences
- Timeline suggests a 4-year gestation (2022 to 2026) from "controller app" to CMR, then faster iteration. Basic fleet dashboards, topology and upgrades are now vendor territory and should not be a third-party's main value.
- Short term, third-party UIs are safe where MikroTik has declared non-goals (IP, DHCP, firewall, safety rails). Medium term, the risk is real but unknowable. MikroTik's UI history suggests function over polish, so a polished UX may stay open, though that is a judgement rather than a finding.

### Gaps
- The original Dude retirement announcement was not located. Nothing was found on the MikroTik mobile app's fleet plans, a WebFig redesign, or Quick Set's future. No mention of CMR in the mobile app was found.

## Existing projects

### Takeaway
The field is **crowded and very active, not abandoned**. Stronger than expected: 2GT MikroTik Manager, MikroDash, and mikr. Most of the serious ones appeared in 2026 and are single-maintainer or LLM-assisted, so durability is unproven. Older attempts mostly stalled, but I found no written post-mortems.

### Cited Findings
Stats from GitHub API on 2026-11-04 unless noted. "Pushed" is last push date.

**Direct competitors (multi-device web management)**

| Project | Stars / forks | Created / last commit | License | On-router | State |
|---|---|---|---|---|---|
| [2GT MikroTik Manager](https://github.com/2GT-Media-Group-LLC/mikrotik-manager) | 248 / 50 | 2026-04 / 2026-11-03 | AGPL-3.0 | Docker Compose; on-router not confirmed | v1.0.5 stable, 1.1 betas near daily, 8 contributors, 57 open issues |
| [MikroDash](https://github.com/SecOps-7/MikroDash) | 639 | pushed 2026-10-29 | MIT | Yes, RouterOS container; amd64, arm64, arm/v7 | Active |
| [mikr / mikr.app](https://github.com/hreskiv/mikr) | 123 / 14 | 2026-02 / 2026-11-03 | No SPDX license on GitHub; commercial | Yes, arm64 container per vendor | v2.2.0, one contributor, 0 open issues |
| [nasnet-panel](https://github.com/nasnet-community/nasnet-panel) | 27 / 8 | 2025-11 / 2026-11-03 | MIT | Yes, RouterOS 7.24+ container | Pre-1.0, 565 commits, 2 main humans, 167 open issues + PRs |
| [Mikrotik-UI](https://github.com/UJBenda/Mikrotik-UI) | 0 / 0 | 2026-10-17 / 2026-10-31 | None | No, Debian server + SSTP tunnels | Multi-tenant, explicit UniFi parity doc, one author, young but long feature list |
| [netfleet](https://github.com/ITConnectGE/netfleet) | 2 | 2026-05 / 2026-11-04 | Apache-2.0 | Unknown | MSP, multi-vendor |
| [mktk-map](https://github.com/JsemOndra/mktk-map) | 1 | 2026-10 / 2026-11-01 | MIT | Unknown | Small network dashboard, topology, Wi-Fi |

- 2GT highlights: Change Guard (device-side auto-revert, verified on a CRS on 7.23.3), lockout prediction, Config Health, VLAN management, per-port clients, SSID management across APs for `wifi` and legacy `wlan`, RF health, firmware rollouts, multi-channel alerts. Its README says it is built with Claude. **[PRIMARY, project claims, not tested]** — [Repo](https://github.com/2GT-Media-Group-LLC/mikrotik-manager)
- mikr pricing: Community free up to 10 devices, then €99 / €149 / €399 one-time with optional yearly updates; about 130 MB RAM for 25 devices; listed as on MikroTik's "Made for MikroTik" page **[VENDOR]** — [mikr.app](https://mikr.app/). The MFM listing itself could not be verified.
- Note: "MikroTik Manager" is the name of two different projects (2GT and mikr). 
- MikroDash: "Manage many routers from one install", configuration pages when given write rights, read-only otherwise **[PRIMARY]** — [Repo](https://github.com/SecOps-7/MikroDash)
- nasnet-panel: dashboard, interfaces, IP, routing, DHCP, VPN, wireless, firewall, subnet scan, batch commands with rollback; React and Go; small container; Sentry on by default in production builds **[PRIMARY]** — [Repo](https://github.com/nasnet-community/nasnet-panel)

**CMR-adjacent**
- [ha-cmr](https://github.com/trakais/ha-cmr): HA integration for CMR, 2 stars, MIT, created 2026-10. [Anrijs/ha-cmr](https://github.com/Anrijs/ha-cmr) is a fork. [go-mikrotik-cmr](https://github.com/AthenaNetworks/go-mikrotik-cmr): clean-room controller. **[PRIMARY]**
- [unifi-simulator](https://github.com/MartinDejmal/unifi-simulator): shows MikroTik devices as switches in the real UniFi Network, 15 stars, MIT, created 2026-10; read-only by design per its [fork's README](https://github.com/joazif/unifi-simulator) **[PRIMARY]**

**Home Assistant and monitoring**
- [homeassistant-mikrotik_router](https://github.com/tomaae/homeassistant-mikrotik_router): 475 stars, Apache-2.0, last commit 2026-09-11, 20 contributors **[PRIMARY]**
- [Grafana-Mikrotik](https://github.com/IgorKha/Grafana-Mikrotik): 418 stars, GPL-3.0, last push 2023-12 **[PRIMARY]**
- Dude replacements: [Yet Another Dude](https://yetanotherdude.io/), [mikroHUB](https://mikrohub.io/en/the-dude-alternative/) **[VENDOR]**; CoreBit, NetXMS and Zabbix suggested in [the Dude thread](https://forum.mikrotik.com/t/the-dude-officially-retired/269426) **[USER]**

**Commercial / MSP**
- [MKController](https://mkcontroller.com/docs/cloudcontroller/pricing): cloud remote management, from US$3 per device; sources disagree on plan structure **[VENDOR]**
- [Altostrat SDX](https://www.g2.com/products/altostrat-sdx/pricing): cloud fleet management, backups, monitoring, DNS filtering; Business plan is "Contact Us"; altostrat.io/pricing returned 404 when fetched
- [Unimus](https://unimus.net/pricing): config backup and automation, 10 free devices with no time limit; paid per device per year. MikroTik support not confirmed in this run.
- [ISPApp](https://sites.google.com/ispapp.co/cloud-management-mikrotik/home): cloud controller for several brands **[VENDOR]**

**Stalled or abandoned (no commits for a year or more unless noted)**
- [apehost-mikrotik-controller](https://github.com/JungleM0nkey/apehost-mikrotik-controller): 22 stars, MIT, created and last touched in about two weeks, last commit 2025-11-05, one author
- [ROSzetta](https://github.com/VFireman/ROSzetta): 3 stars, last commit 2026-05-26, about one month of activity
- [vponomarev/mikrotik-capsman](https://github.com/vponomarev/mikrotik-capsman): CAPsMAN web UI, 19 stars, last push 2020
- [Mikrotik_API](https://github.com/hatamiarash7/Mikrotik_API): 88 stars, last push 2023-12
- Several SDN controller student projects are archived or dormant, e.g. [LTI-SDN-Controller](https://github.com/monico18/LTI-SDN-Controller) (archived) **[all PRIMARY, GitHub API]**

### Inferences
- A GitHub search in October and November 2026 turns up new MikroTik fleet or topology tools almost daily, many one-author and low-star. LLM-assisted coding has made "yet another dashboard" cheap, as normis himself suggested, so raw feature count is no longer a moat. Trust, safety, and long-term maintenance are.
- The common failure pattern looks like a one-author burst followed by silence. This is inferred from commit dates, not from stated reasons.
- Against "this always fails": 2GT, MikroDash and mikr all show real adoption within months. Demand is real, but those projects capture it.
- MikroDash and nasnet-panel show on-router containers are practical, which removes "runs on-router" as a unique selling point.

### Gaps
- I found no post-mortem explaining why any project stopped. MikroDash full activity data and nasnet-panel's true multi-device scope were not checked. 2GT's ability to run on-router was not confirmed. A search of 2GT issues for "CMR" returned nothing, so no CMR integration is visible there. Splynx was not researched; it is ISP billing, not a controller. Remote Winbox was not found as a distinct product.

## User demand and what stopped past attempts

### Takeaway
Demand is real and long-standing, shown by the 2022 staff thread and the rapid stars on 2026 projects. Reddit evidence here is thin and second-hand. Stated blockers are technical and cultural: RouterOS's flexibility, lack of transactional config, and many users who prefer CLI or see UniFi-style simplification as a loss.

### Cited Findings
- Reception of the 2022 controller announcement was very positive ("killing feature", "extremely exciting"). Requests: staged upgrades, templates, config versioning, backup and diff, web UI, Grafana-style graphs, UISP-style maps **[USER]** — [Forum](https://forum.mikrotik.com/t/mikrotik-devices-controller/158354)
- Objections in the same thread: volga629 called UniFi "too many pictures, no functionality"; others suggested improving The Dude; dvreshta advised small-business scope (CCR, CRS/CSS, CAPsMAN), claiming that covers over 70% of installations **[USER]** — [Forum](https://forum.mikrotik.com/t/mikrotik-devices-controller/158354)
- 2024: "UniFi, TP-Link, Grandstream, and Aruba Instant On now offer unified control solutions", asking about MikroTik's plan **[USER]** — [Forum](https://forum.mikrotik.com/t/any-updates-on-mikrotik-controller-development/176670)
- Reddit-derived snippets, via mirrors and search summaries only, so wording is uncertain: users found nothing like UniFi for MikroTik; Zabbix "better then nothing"; "Mikrotik for a more budget tinker approach, Unifi for a more expensive but easier hands off approach" **[USER, weak]** — [Mirror](https://redlib.hbubli.cc/r/mikrotik/comments/1jioz7u/im_considering_migrating_to_mikrotik) (returned 403 when fetched directly)
- A technical reason noted by a project: RouterOS applies each command immediately with "no transaction, no rollback, and no cross-object validation" **[PRIMARY, project claim]** — [2GT README](https://github.com/2GT-Media-Group-LLC/mikrotik-manager)
- CMR itself avoids this by creating its own objects and preferring default configs **[PRIMARY]** — [CMR manual](https://manual.mikrotik.com/docs/management-tools/cmr/)

### Inferences
- The hard problem is not the UI. It is representing arbitrary RouterOS configs safely. MikroTik sidesteps it by owning only its own objects, and 2GT tackles it with lockout simulation. A new entrant must pick one of those or restrict supported topologies.
- Buyers wanting UniFi simplicity often just buy UniFi, and MikroTik's enthusiasts often prefer raw control, so the addressable audience is narrower than the volume of forum requests suggests. This is judgement, weakly supported by the quotes above.

### Gaps
- Direct Reddit threads on CMR were not retrievable; searches returned nothing usable, so community reaction to CMR outside the MikroTik forum is unknown. There is no quantified demand data beyond stars and forum replies.
