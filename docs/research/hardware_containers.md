# RouterOS containers on hAP be³ Media and hAP ac²: hardware and container feasibility (as of 2026-11-04)

Evidence tags: **[Primary]** = MikroTik product page, manual, changelog or staff post. **[User]** = forum or community report. **[Retailer]** = shop listing. Anything inferred is listed only under Inferences.

Method caveat: pages were read through an extraction tool, and some forum and manual items came from search-result summaries (marked "search summary"). Quoted strings should be rechecked against the page before being reused verbatim. No hardware was tested.

## Is the hAP be³ Media shipping, and what are its real specs?

### Takeaway
It is shipping in real but constrained volume: owners have had units since about July to September 2026, even though MikroTik's own product page still says "Coming soon!". The core specs are confirmed. The USB layout is still contradictory in MikroTik's own material, and the physical form factor is not stated by any primary source.

### Cited Findings
Availability:
- [Primary] The product page, fetched 2026-11-04, lists it as "New" with "Coming soon!", suggested price $179.00 — [MikroTik product page](https://mikrotik.com/product/hap_be3_media)
- [User] In February-March 2026 a poster quoted a manufacturer statement that the first production batch was expected in late April 2026 — [forum](https://forum.mikrotik.com/t/hap-be3-on-website/268837)
- [Primary] In the same thread, staff member normis said on March 3 that the product page had errors and missing information and was rushed, and advised caution — [forum](https://forum.mikrotik.com/t/hap-be3-on-website/268837)
- [User] On July 20, 2026 a forum member said the product had just gone to distributors; on July 24 an owner photographed the in-box paper about microSD — [forum](https://forum.mikrotik.com/t/when-will-microsd-be-supported-for-hap-be-media/271766)
- [User] An owner received a unit on September 11, 2026, about two months after pre-ordering — [forum](https://forum.mikrotik.com/t/my-new-mikrotik-hap-be-3-media/273109)
- [Retailer] LinITX (UK) shows "On Order", 0 available, £161.99 inc VAT, with next arrivals 11-Dec-2026 (40 still available, 30 pre-sold) and 25-Dec-2026 (30 available) — [LinITX](https://linitx.com/product/mikrotik-hap-be3-media-wifi-7-router-access-point-ma53ug+hbeh/18454)
- [Retailer] Baltic Networks files the item in a "preorder-reserve" collection — [Baltic Networks](https://www.balticnetworks.com/en-ca/collections/preorder-reserve/products/mikrotik-hap-be-wi-fi-7-media-ma53ug-hbeh)

Confirmed specs ([Primary], all from the [product page](https://mikrotik.com/product/hap_be3_media); RAM, NAND and power repeated in the [hardware manual](https://manual.mikrotik.com/hardware/hap-be-media/)):
- CPU IPQ-5322, quad-core, up to 1.5 GHz, 64-bit ARM. RAM 2 GB. Storage 512 MB NAND. RouterOS v7, license level 6.
- PoE-in 802.3af/at and DC jack, both 24-57 V. Max 29 W, 20 W without attachments. Included adapter 24 V 1.5 A.
- Triple-band 2x2 Wi-Fi 7 (BE9300): 2.4 GHz 688 Mbit/s, 5 GHz 2882 Mbit/s, 6 GHz 5764 Mbit/s, with MLO.
- 5 x 2.5G Ethernet, QCA8386 switch chip, Bluetooth 5.4, Thread radio. Passive cooling, IP20, -30 to 50 °C.

Conflicts and open spec points:
- USB: MikroTik's feature text says USB-C plus two USB-A; its spec table says 3 x USB 3.0 Type-A, max 1.5 A. The inconsistency appears on both the [product page](https://mikrotik.com/product/hap_be3_media) and the [manual](https://manual.mikrotik.com/hardware/hap-be-media/). LinITX lists 1 x USB-C plus 2 x USB-A 3.0 — [LinITX](https://linitx.com/product/mikrotik-hap-be3-media-wifi-7-router-access-point-ma53ug+hbeh/18454)
- microSD: [Primary via user quote] the 7.24rc3 changelog (2026-07-28) was quoted as "system - added microSD card support for hAP be3 Media" — [forum](https://forum.mikrotik.com/t/when-will-microsd-be-supported-for-hap-be-media/271766). The extraction of the [7.24 changelog](https://download.mikrotik.com/routeros/7.24/CHANGELOG) found no microSD line, and LinITX says it "requires RouterOS v7.23.3" — [LinITX](https://linitx.com/product/mikrotik-hap-be3-media-wifi-7-router-access-point-ma53ug+hbeh/18454). [User] One owner runs a 32 GB SD card as ext4 for logging — [forum](https://forum.mikrotik.com/t/my-new-mikrotik-hap-be-3-media/273109)
- Retailer errors exist: search summaries showed one listing with 1.1 GHz and one with 574/1200 Mbit/s band rates, against the official figures — [search summary of retailer listings](https://b2b.100mega.com/en/330915-mikrotik-hap-be3-media)
- Form factor: the product page gives no category or dimensions, only a "Case Base", IP20, passive cooling, and base-plate and wall-mounting documents — [product page](https://mikrotik.com/product/hap_be3_media). [Retailer] Dimensions 187 x 142 x 95 mm — [Baltic Networks](https://www.balticnetworks.com/en-ca/collections/preorder-reserve/products/mikrotik-hap-be-wi-fi-7-media-ma53ug-hbeh). [User] The microSD slot is on top near the USB ports — [forum](https://forum.mikrotik.com/t/when-will-microsd-be-supported-for-hap-be-media/271766). A Home Assistant thread (search summary) mentions a compartment for a Zigbee radio — [HA community](https://community.home-assistant.io/t/mikrotik-hap-be-media-as-home-assistant-host/1013575)

### Inferences
- The "Coming soon!" label looks stale rather than true, since many owners post about retail units. Buyers should still plan on pre-order lead times of weeks to months, at least in the UK and EU.
- The spec table's "3 x USB 3.0 Type-A" probably counts the USB-C port as one of the three. This is unverified.
- At 187 x 142 x 95 mm with a case base it is probably a desk or shelf unit, with wall mounting as an option.

### Gaps
- No primary source states the exact USB layout, and I did not inspect a product photo or block diagram.
- No official ship date or volume statement was found. Stock was checked at only a few retailers.
- It is unconfirmed which stable release first carried microSD support.

## Does the hAP be³ Media support containers, and are there reports or known bugs?

### Takeaway
Yes: MikroTik advertises container support and ships a container package and one-click setup, and at least some owners run containers. Evidence is thin, though: I found no measured performance report. The serious early bugs were in base Ethernet and firmware, not containers.

### Cited Findings
- [Primary] The product page lists container support, a container package in default downloads and a one-click setup page in WinBox — [product page](https://mikrotik.com/product/hap_be3_media); also [manual](https://manual.mikrotik.com/hardware/hap-be-media/)
- [Primary] Staff member normis said Thread is exposed to the container system and managed through the /App menu, with an OTBR app or a Home Assistant stack — [forum](https://forum.mikrotik.com/t/hap-be3-on-website/268837)
- [User] Factory units shipped with RouterBOOT 7.25_ab170, RouterOS 7.23.2 and container package 7.23.2 — [forum](https://forum.mikrotik.com/t/hap-be3-media-recovery-to-a-stable-7-25beta5-baseline/273364)
- [User] The `otbr` app crashed at start on 7.23.3 and 7.24rc3 (Aug 6-7, 2026). Staff member kristsd linked an upstream OpenThread regression, and a user reported success after the image was updated and re-pulled on Aug 11. This was an image bug, not a RouterOS container bug — [forum](https://forum.mikrotik.com/t/hap-be-media-cant-start-thread-border-router-otbr/272143)
- [User] A search summary reported an owner saying containers "run great"; I could not tie this to a specific post, so treat it as weak — [forum search area](https://forum.mikrotik.com/t/my-new-mikrotik-hap-be-3-media/273109)
- [User] A Home Assistant thread has no first-hand container measurements; the stack was tested on CHR and a Chateau 5G, and one poster speculated that 2 GB might be tight — [HA community](https://community.home-assistant.io/t/mikrotik-hap-be-media-as-home-assistant-host/1013575)
- [User] Ethernet went dead after updating new units to 7.24.2 stable; 7.25beta3 restored it, with one report of random drops. Minimum firmware 7.25 blocks downgrade — [forum, Sept 11, 2026](https://forum.mikrotik.com/t/hap-be-media-ethernet-ports-not-working-on-stable-routeros/273058)
- [User] Ports 2-5 stopped after reboot or power cycle on one unit; a clean 7.25beta5 baseline, later 7.26beta1, showed no recurrence by Oct 11. Another poster said these posts are written by an AI assistant for the owner, so weigh accordingly — [forum](https://forum.mikrotik.com/t/hap-be3-media-recovery-to-a-stable-7-25beta5-baseline/273364)
- [Primary] 7.24 disabled EEE on this board and 7.25 stable (2026-10-22) has "ethernet - fix occasional packet loss on hAP be3 Media" — [7.24 changelog](https://download.mikrotik.com/routeros/7.24/CHANGELOG); [7.25 release thread](https://forum.mikrotik.com/t/7-25-stable-is-released/273987)
- [Primary] 7.25.1 (2026-11-02) is the listed stable; extraction found no container or hAP be3 lines in it — [7.25.1 changelog](https://download.mikrotik.com/routeros/7.25.1/CHANGELOG)
- [User] Also reported: 6 GHz and Wi-Fi latency complaints (thread titles only, not read) — [6 GHz thread](https://forum.mikrotik.com/t/hap-be3-media-terrible-6-ghz-performance/271900); [latency thread](https://forum.mikrotik.com/t/hap-be-media-7-23-3-severe-wi-fi-latency-loss-vs-archer-c6/272359)

### Inferences
- Any deployment should start by upgrading to 7.25.1 or later before relying on the unit. Stable releases before 7.25 are risky on this board.
- arm64 with 2 GB RAM is the supported, mainstream container path, so a small web app is very likely to work. "Very likely" rests on the architecture and on MikroTik's claims, not on benchmarks.

### Gaps
- No report was found of a custom third-party web app running long-term on this board, and no RAM, CPU or uptime figures.
- I found no container-specific RouterOS bug tied to IPQ-5322. That is absence of evidence from a young product, not proof of none.
- No evidence was gathered on other IPQ-53xx boards.
- Whether the 7.24.2 Ethernet fault is fully gone on 7.25 stable has no confirming report; the 7.25 thread had no be3 reports.

## What are the container requirements?

### Takeaway
You need the container package, a physical button press to enable device-mode, and external storage (strongly advised). Pull and tar import both work, and networking is veth behind NAT by default. The physical confirmation cannot be done remotely, so it is a hard step in any install flow.

### Cited Findings
All [Primary]. "Manual" = [Container manual, labelled 7.26](https://manual.mikrotik.com/docs/containers/); "help page" = the older [frozen help page](https://help.mikrotik.com/docs/spaces/ROS/pages/84901929/Container), which itself points to manual.mikrotik.com.
- Architectures: arm, arm64, x86. EN7562CT CPUs (hEX Refresh) only run arm32v5 images — manual
- Device-mode: disabled by default; `/system/device-mode/update container=yes`, confirmed by reset button, or cold reboot on x86. Once enabled, containers can be managed remotely — manual
- The confirmation also has an `activation-timeout`, after which the request is cancelled (search summary) — [device-mode update reference](https://manual.mikrotik.com/docs/cli-reference/system/device-mode/update/)
- 7.25 added device-mode configuration to WinBox, and swap now needs the container device-mode — [7.25 release thread](https://forum.mikrotik.com/t/7-25-stable-is-released/273987)
- Pull: set `registry-url` (default `https://lscr.io/`) and `tmpdir`, then use `remote-image`. Import: save an image with Podman or Docker, upload it, use `file=`. Build for the target architecture on a PC — manual
- RAM during pull: "considerable free main memory" with no figure; 16 MB SPI flash boards should use pre-built images on USB or other disk — manual
- Storage: `root-dir` holds the container store, `tmpdir` is for extraction, `layer-dir` holds shared layers. Internal storage is "not recommended", and volumes should avoid built-in storage — manual
- Disk target: at least 100 MB/s sequential and 10K random IOPS. Test with `/disk/test` — manual; [Apps manual](https://manual.mikrotik.com/docs/containers/apps/)
- Limits: `memory-max` is hard (container terminated), minimum 1 MB from 7.23. `memory-high` is soft (throttled, never killed). `swap-max` is available. Per-container and global limits both apply, and the lower wins — manual
- Veth: gateway-mode example uses 172.17.0.1, a srcnat masquerade outbound, and dst-nat for inbound. `dhcp=yes` on a bridge is available. A veth is not running until its container starts; stop the container before changing veth settings. DHCP leases do not supply DNS — [VETH manual](https://manual.mikrotik.com/docs/containers/veth/)
- Layer 2 mode exposes all container ports and is "considered insecure" — manual
- DNS: without DNS configured "the container will not start!" — manual
- Mounts: `/container/mounts` with `src` and `dst`, attached with `mountlists`. If `src` is missing on first use it is filled from the container's `dst` — [help page](https://help.mikrotik.com/docs/spaces/ROS/pages/84901929/Container)
- `logging=yes` sends output to the RouterOS log; `start-on-boot=yes` starts at boot; a new container is not started automatically after add — manual
- Health and restart: healthcheck from 7.23 with `stop-on-unhealthy`; `restart-policy=no/always/on-failure`, OOM-kill detection and global `memory-max` also came in 7.23 — manual; [7.23 changelog](https://download.mikrotik.com/routeros/7.23/CHANGELOG)
- Apps (`/app`) need arm64 or x86. They create the veth and NAT, and map ports automatically. Custom YAML apps exist from 7.22. New disks must be ext4 or btrfs — [Apps manual](https://manual.mikrotik.com/docs/containers/apps/). XFS was allowed in 7.23 — [7.23 changelog](https://download.mikrotik.com/routeros/7.23/CHANGELOG)
- Port conflict: apps will not start on "internal" if they mask an existing router service — [7.23 changelog](https://download.mikrotik.com/routeros/7.23/CHANGELOG)

### Inferences
- A custom YAML app is probably the cleanest packaging on the be³ Media because it removes hand-built veth and NAT steps.
- Tar import avoids registry and tmpdir issues and suits small or offline installs, but pull is fine with 2 GB RAM.
- USB SSD is the safest storage. A typical microSD card is unlikely to reach 10K random IOPS, so treat it as below MikroTik's guidance.
- Web apps should avoid router-owned ports (80, 443) inside the "internal" app network, or map them elsewhere.

### Gaps
- No official RAM figure for pulls was found.
- The manual extraction did not show a privileged option, while [7.24](https://download.mikrotik.com/routeros/7.24/CHANGELOG) added privileged mode. Its property name and limits were not confirmed.

## Can a RouterOS container talk to the host's own API/REST?

### Takeaway
Probably yes, as ordinary IP traffic to the router's veth or bridge address, but I found no MikroTik page that documents this as a method and no report of REST from a container working. The likely blockers are the input firewall and service restrictions. Test this early on real hardware.

### Cited Findings
- [Primary] The VETH manual only says the router is the container's gateway (for example 172.17.0.1) and does not explicitly describe container-to-router service access — [VETH manual](https://manual.mikrotik.com/docs/containers/veth/)
- [Primary] The Apps manual says custom apps can optionally "interact with our API" but gives no details — [Apps manual](https://manual.mikrotik.com/docs/containers/apps/)
- [Primary] 7.25 added "app - add the networkRouterIP variable for container apps" — [7.25 release thread](https://forum.mikrotik.com/t/7-25-stable-is-released/273987). I found no documentation for this variable.
- [Secondary, not MikroTik] A third-party page lists a `[routerIP]` YAML placeholder — [claudskills](https://claudskills.com/skills/routeros-app-yaml/SKILL.md). The manual's CLI reference has `router-ip`, `router-ip-alt` and `assumed-router-ip` (search summary) — [app CLI reference](https://manual.mikrotik.com/docs/7.24/cli-reference/app)
- [User] A 2023 poster's container could reach the internet but not the router's SSH at 192.168.88.1. A forum member suspected NAT hairpinning, and using a same-subnet setup was suggested. The cause was not proven, and the same member said calling into the host is usually a design smell but has legitimate narrow uses — [forum](https://forum.mikrotik.com/t/connect-to-router-via-ssh-from-container/169810)
- [User] Input-chain drop rules are a reported blocker, with the fix of adding the veth or bridge to the LAN interface list (search summary) — [AdGuard thread](https://forum.mikrotik.com/t/help-with-adguard-container-setup/175728). A guide uses a service on 172.17.0.1 from a container this way (search summary) — [VLESS thread](https://forum.mikrotik.com/t/vless-proxy-tunnel-on-mikrotik-via-containers/172770)
- [Primary] REST uses HTTP Basic auth over www-ssl (HTTPS) or www (HTTP), and MikroTik advises against plain HTTP. `rest-plain` and `rest-secure` can disable it (403). Commands are limited to 60 seconds ("Session closed", 400), and one user's requests run one at a time. A 401 can mean wrong password, a group without the `rest-api` policy, or an address restriction. These came from a search summary of the manual — [REST API manual](https://manual.mikrotik.com/docs/developer-guides/rest-api/)
- [Primary] `www-ssl` `max-sessions` defaults to 20, and with no certificate only anonymous ciphers are offered, so normal clients fail (search summary) — [webserver reference](https://manual.mikrotik.com/docs/cli-reference/ip/service/webserver/)
- [Primary] Apps have `network-outgoing-access=yes/no` to block outbound connections — [7.23 changelog](https://download.mikrotik.com/routeros/7.23/CHANGELOG)

### Inferences
- Likely working recipe: call `https://<veth gateway IP>/rest` with a dedicated least-privilege user, allow only that source and port in the input chain, and limit the service and user `address` to the container subnet.
- Adding the container's interface to the LAN list is too broad for a controller; a narrow input rule is better.
- The hairpin theory matters mainly if the container targets a LAN address through NAT. Using the directly connected gateway address should avoid it.
- The serial per-user behaviour and 60 s limit mean a controller should use short calls, and possibly separate users for slow and fast jobs.
- Credentials for the host must live in the container, so a compromised container gets whatever that user can do.
- `network-outgoing-access=no` may also block host access. This is untested.

### Gaps
- No primary statement that REST from a container is supported, and no first-hand report of it.
- 8728/8729 API from a container was not confirmed either.
- The manual extraction suggested the Postgres guide gives 172.17.0.2 as a router-side address. That looks like a misread or doc quirk (172.17.0.2 is normally the container), so I did not rely on it.

## Is the hAP ac² a realistic container host?

### Takeaway
Not for a controller. The arm architecture is allowed on paper and the package is tiny, and users seem to have run light containers with USB storage. However, the board has only hundreds of KB of flash left, 128 MB RAM and no Apps support, and experienced forum users say it is not meant for this.

### Cited Findings
- [Primary] IPQ-4018, quad-core 488-896 MHz, ARM 32-bit, 128 MB RAM, 16 MB flash, 1 x USB Type-A (1 A), license 4; the page does not mention containers, and v7.25.1 is listed for it — [product page](https://mikrotik.com/product/hap_ac2)
- [Primary] The container feature supports arm; 16 MB SPI flash boards should use pre-built images on USB or other disk — [Container manual](https://manual.mikrotik.com/docs/containers/)
- [Primary] Apps require arm64 or x86, so the one-click and YAML path excludes this board — [Apps manual](https://manual.mikrotik.com/docs/containers/apps/)
- [User] On a 16 MB device at 7.16.2: container package about 96 KiB, routeros 11.1 MiB, wifi-qcom-ac about 2.6 MiB, about 272 KiB free (search summary; board model not confirmed) — [forum](https://forum.mikrotik.com/t/mystery-space-hogs-16mb-flash/266728)
- [User] With container data on USB, starting an Unbound DNS container took free flash from about 150 KB to zero on 7.19.3; the same config on 7.16.2 left 456 KB. Another user also saw flash consumed by container starts, and the poster said files seem to be written to flash, not RAM. The poster also runs Pi-hole on USB storage — [forum, July 2025](https://forum.mikrotik.com/t/running-out-of-space-on-hap-ac2/172510?page=6)
- [User] In the same thread: "I don't think these things are really meant to run containers." — [forum](https://forum.mikrotik.com/t/running-out-of-space-on-hap-ac2/172510?page=6)
- [User] 7.13.5 with qcom drivers used about 35-40 MB RAM with 600-650 KB free disk; adding zerotier reached 0 KB and a restart reset the device to defaults. Another member called the board with wifi-qcom "at the limit" even as a simple AP — [forum](https://forum.mikrotik.com/t/hapac2-out-of-hdd-space-safe-free-disk-space/173946)
- [User] Running out of storage forced a NetInstall in one case — [forum](https://forum.mikrotik.com/t/hapac2-out-of-hdd-space-safe-free-disk-space/173946)
- [User] 7.18.1 CAPsMAN units with routeros and wireless packages had about 1 MB of 16 free — [forum](https://forum.mikrotik.com/t/hap-ac2-low-memory/182434)

### Inferences
- USB storage is effectively mandatory, since no image fits in the free flash.
- Roughly 85 MB of RAM may remain before any container (128 minus 35-40 MB), and pulling images is unwise, so tar import from USB is the only sensible route.
- The real danger is not that the container fails to start. It is that it fills flash and damages a working router, which could need NetInstall.
- The IPQ-4018 is a 32-bit ARMv7-class part, so an arm/v7 image would be required. Many modern runtimes treat this as second-class. A tiny static binary is conceivable; a Node or .NET web app with a database is not realistic.
- The hAP ac² is better treated as a managed device than a host: a controller elsewhere can reach it over REST or API.

### Gaps
- No primary statement confirms or denies container support on this exact board.
- No report gives container RAM use or a container OOM on this board.
- Forum posts from optio imply a hAP ac² from thread context, but the excerpt does not name the board outright.
- Whether the flash consumption bug is fixed in 7.2x is unknown; no post confirms a fix.

## What are the known limits, stability problems and security concerns on v7?

### Takeaway
MikroTik's own warnings are blunt: no security guarantee, and a compromised container could leave a persistent backdoor. Operational risks are flash wear, USB mounts failing after reboot, and heat. Recent 7.23-7.25 releases improved restart, memory and health handling, but no measured data on routing or Wi-Fi impact was found.

### Cited Findings
Security:
- [Primary] "If you run a container, there is no security guarantee of any kind"; a compromise may leave a backdoor that survives container removal, and a RouterBOOT or pre-loader vulnerability may not be fixed by Netinstall — [Container manual](https://manual.mikrotik.com/docs/containers/)
- [Primary] The older page also warns of root access and firmware-level damage — [help page](https://help.mikrotik.com/docs/spaces/ROS/pages/84901929/Container)
- [Primary] 7.24 added privileged-mode containers, and 7.25 allows privileged mode per service in app YAML — [7.24 changelog](https://download.mikrotik.com/routeros/7.24/CHANGELOG); [7.25 release thread](https://forum.mikrotik.com/t/7-25-stable-is-released/273987)
- [Primary] 7.24 stopped printing env variables in the log at startup, and app secrets are now sensitive in exports — [7.24 changelog](https://download.mikrotik.com/routeros/7.24/CHANGELOG)

Storage and reboots:
- [Primary] Internal storage is not recommended — [Container manual](https://manual.mikrotik.com/docs/containers/). 7.24 "reduced writes to flash when running health check" — [7.24 changelog](https://download.mikrotik.com/routeros/7.24/CHANGELOG)
- [User] Users move containers to USB to avoid flash wear; a forum member warned of wear, filesystem corruption and full disks (search summary) — [forum](https://forum.mikrotik.com/t/uptime-kuma-container-failing-on-usb-mount/270504)
- [User] On RB5009 and hAP ax³, some USB3 sticks return as USB2 after reboot, changing the mount path so containers fail. A USB power-reset script is the workaround (search summary) — [forum](https://forum.mikrotik.com/t/rb5009-routeros-7-9-1-usb-slot-keeps-changing/167185/15)
- [User] A thread exists on containers not starting after power loss (title and snippet only) — [forum](https://forum.mikrotik.com/t/containers-wont-start-after-power-loss/170942)
- [Primary] 7.24 fixed `start-on-boot` not retrying on certain startup errors, which confirms that boot start had real failure modes — [7.24 changelog](https://download.mikrotik.com/routeros/7.24/CHANGELOG)
- [User] A be³ owner calls high temperatures a downside, and another poster doubts USB and SD media would last ten years at 70 °C or more. No be³ reading was given — [forum](https://forum.mikrotik.com/t/my-new-mikrotik-hap-be-3-media/273109)

Performance:
- [User] A forum reply says there is no L2 hardware offload for the be³ switch chip in RouterOS, so bridging runs on the CPU; the port table showing HW "yes" was not reconciled, and no numbers were given — [forum](https://forum.mikrotik.com/t/hardware-offload-on-hap-be3-media/272425)
- [Primary] `cpu-list` can restrict containers to chosen cores — [Container manual](https://manual.mikrotik.com/docs/containers/)

RouterOS 7.2x changes ([Primary]):
- 7.22: custom YAML apps — [Apps manual](https://manual.mikrotik.com/docs/containers/apps/)
- 7.23: restart policy, OOM detection, global `memory-max`, healthcheck, `network-outgoing-access`, reverted routing-rule priorities for containers introduced in 7.22 — [7.23 changelog](https://download.mikrotik.com/routeros/7.23/CHANGELOG)
- 7.24: `save` command, swap limits, privileged mode, initial RKE2 support — [7.24 changelog](https://download.mikrotik.com/routeros/7.24/CHANGELOG)
- 7.25: `networkRouterIP`, per-service privileged YAML, veth-left-running fix, Ctrl-] shell escape, swap needs device-mode — [7.25 release thread](https://forum.mikrotik.com/t/7-25-stable-is-released/273987)
- 7.25.1 (2026-11-02): no container lines found — [7.25.1 changelog](https://download.mikrotik.com/routeros/7.25.1/CHANGELOG)
- The [changelog index](https://mikrotik.com/download/changelogs) loads notes dynamically and shows 7.24.6 (2026-11-02) as Long-term. Its "Stable" label on 6.49.23 appears to be a page labelling quirk, because product pages give 7.25.1 as stable.

### Inferences
- A controller that holds router credentials is exactly the high-value case MikroTik warns about. Pin and own the image, do not use Layer 2 mode or privileged mode, and use a restricted router user.
- Use a quality USB SSD rather than a stick, set `memory-max`, and keep the database and logs off NAND.
- Build a boot-time check that the disk is mounted before relying on `start-on-boot`, because USB path changes are a known failure on other boards.
- A light web app is unlikely to harm routing noticeably on a quad-core CPU, but since bridging is on the CPU too, heavy LAN traffic plus heavy container load could compete. `cpu-list` is a possible tuning knob. This is a reasoned guess with no data.

### Gaps
- No measured data on routing or Wi-Fi throughput with containers running, on these or similar boards.
- No NAND endurance figure for the be³ Media, and no measured flash-wear failure attributed to containers.
- No documented behaviour for what happens to RouterOS itself when the container disk fills.
- No container-specific CVE search was carried out.
