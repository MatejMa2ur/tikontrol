# Automating install and upgrade of a containerised web app on RouterOS 7 (7.18–7.26) without /app

Research date: 2026-11-05. Method note: in this session the MikroTik hosts (help.mikrotik.com, manual.mikrotik.com,
download.mikrotik.com, forum.mikrotik.com) and tangentsoft.com could not be fetched directly (blocked by the session's
network allow-list). Findings about them come from web-search result snippets of those pages, plus direct fetches of
GitHub-hosted READMEs. Every RouterOS property name below should be confirmed on CHR (`/container/add ?`,
`/container/config/print`, etc.) before it is baked into an installer. Release state seen in search: 7.25.1 is current
stable; 7.26 is still beta (7.26beta5, 2026-11-02) — [MikroTik downloads](https://mikrotik.com/download/changelogs?channelFilter=),
[7.26beta forum announcement](https://forum.mikrotik.com/t/7-26beta-development-is-released/273582).

## 1. /tool/fetch from GitHub release URLs, TLS trust, output modes, size limits

### Takeaway
RouterOS now ships a built-in public root CA store, governed by `/certificate/settings builtin-trust-store=`, and the
current default list already includes `fetch` and `container`, so `check-certificate=yes` against GitHub can work without
importing a CA on fresh installs; upgraded devices may need it set. `output=user` is capped at ~64 KB, so a .rsc
installer should be fetched with `output=file dst-path=...` (or kept under 64 KB).

### Cited Findings
- `/certificate/settings` has a `builtin-trust-store` property that controls which services may use the built-in public
  root CAs; the documented default is `fetch mqtt email netwatch container lora dns www reverse-proxy` — [MikroTik Certificates (search snippet)](https://manual.mikrotik.com/docs/authentication-authorization-accounting/certificates/)
- eworm-de's widely used RouterOS script collection tells users to run `/certificate/settings/set builtin-trust-store=fetch;`
  and calls `builtin-trust-store=default` "a reasonable choice"; its fetches use `check-certificate=yes-without-crl` with
  `output=user as-value` — [eworm-de/routeros-scripts README](https://github.com/eworm-de/routeros-scripts/blob/main/README.md)
- The same README warns that a bootstrap fetch of a PEM "does *not* verify server certificate" and that certificate
  lifetime is "checked with local time", so the router clock must be correct (NTP) before TLS validation works — [eworm-de README](https://github.com/eworm-de/routeros-scripts/blob/main/README.md)
- Grokipedia names the property `builtin-trust-anchors=trusted` and says upgraded devices do not trust built-in roots by
  default; this name does not match the official `builtin-trust-store` and should be treated as unreliable — [Grokipedia](https://grokipedia.com/page/Fetch_MikroTik_RouterOS); contradicted by [MikroTik Certificates](https://manual.mikrotik.com/docs/authentication-authorization-accounting/certificates/)
- `check-certificate` values: `yes`, `yes-without-crl`, `no`; default `no`. Without it fetch connects even to servers with
  expired or self-signed certificates — [MikroTik Fetch](https://manual.mikrotik.com/docs/system-information-and-utilities/fetch/)
- With `output=user`, fetched data is capped at 64512 bytes; the CLI reference suggests comparing stored length against the
  limit to detect truncation — [fetch CLI reference](https://manual.mikrotik.com/docs/cli-reference/tool/fetch/); older forum posts say "up to 63k" — [forum](https://forum.mikrotik.com/t/tool-fetch-output-user-example/121595)
- `http-max-redirect-count` sets how many redirects fetch follows; sources disagree on the default (2 vs 0) — [mikrotikdocs.fyi Fetch](https://mikrotikdocs.fyi/tools/fetch/), [MikroTik Fetch](https://help.mikrotik.com/docs/spaces/ROS/pages/8978514/Fetch)
- GitHub release-asset redirects carry a ~9 KB Content-Security-Policy header that broke aiohttp clients (8190-byte header
  limit); fixed in Home Assistant and HACS by raising limits to 64 KiB. No source shows RouterOS fetch failing on it — [home-assistant/core #188329](https://github.com/home-assistant/core/pull/188329), [hacs #5644](https://github.com/hacs/integration/pull/5644)
- No `max-size` argument for `/tool/fetch` was found in any source — [search summary of fetch docs](https://manual.mikrotik.com/docs/cli-reference/tool/fetch/)

### Inferences
- Installer one-liner should explicitly set `http-max-redirect-count=` (e.g. 5) because `github.com/.../releases/download/...`
  302-redirects to `objects.githubusercontent.com` (or `release-assets.githubusercontent.com`) and the default may be 0.
- Safer: serve the .rsc from a stable non-redirecting HTTPS URL (e.g. `raw.githubusercontent.com` pinned to a tag/commit),
  or fetch to a file and check `[/file get <name> size]` > 0 before `/import`.
- Installer preflight should check `/certificate/settings get builtin-trust-store` contains `fetch` and `container`, and
  that `/system/clock` / NTP is synced; refuse `check-certificate=no`.
- The GitHub large-header issue is a plausible but untested risk for RouterOS fetch; test on CHR.

### Gaps
- Exact version where `builtin-trust-store` appeared (believed 7.19-ish per assignment) and the default after upgrade vs.
  fresh install: not confirmed (changelogs unreachable).
- Whether RouterOS fetch handles GitHub's ~9 KB redirect response headers: untested, no source.

## 2. /import of a fetched .rsc: errors, idempotency, integrity (SHA-256)

### Takeaway
Use `:onerror` (returns true/false, captures message) around each step, guard every `add` with `[find ...]`, and run
`/import file=... verbose=yes` (optionally `dry-run`) first. There is no documented built-in way to SHA-256 a fetched
file; integrity currently rests on TLS + pinned URL. One forum post shows `:convert transform=sha512` working on 7.16.2,
but the official reference lists only case transforms — conflicting, test on CHR.

### Cited Findings
- `:onerror <var> in={<command>} do={<expression>}` catches errors and returns true (error) / false (no error), so it works
  in `:if`; for an HTTP error from fetch the second variable holds status code and headers — [MikroTik Scripting (search snippet)](https://help.mikrotik.com/docs/display/ROS/Scripting)
- `/import` gained its own `on-error` parameter in 7.16.x; errors inside the imported script are not caught by it — [skills.cat RouterOS rsc guide (secondary)](https://skills.cat/skills/alexandre-machado/ai-stuffs/mikrotik-routeros-rsc)
- `import test.rsc verbose=yes dry-run` reports syntax errors without applying changes (secondary source) — [skills.cat guide](https://skills.cat/skills/alexandre-machado/ai-stuffs/mikrotik-routeros-rsc)
- A duplicate entry halts an import and nothing further is processed; `verbose=yes` shows which line — [forum, v6.16](https://forum.mikrotik.com/t/6-16-import-stops-when-there-is-a-duplicate-entry/83056)
- There is no built-in idempotent export/import; community pattern is `find where ...` + conditional add/set — [forum: idempotent script](https://forum.mikrotik.com/t/how-to-write-idempotent-script/155596)
- `:convert` with `transform=sha512 to=hex` demonstrated on 7.16.2; SHA-256 in pure script is slow for large inputs — [forum: Base64 and SHA256](https://forum.mikrotik.com/t/base64-and-sha256-function-for-scripting/164889); contradicted by the Scripting reference, which lists only `lc`, `uc`, `lcfirst`, `ucfirst` — [MikroTik Scripting](https://help.mikrotik.com/docs/display/ROS/Scripting)
- Files page: file contents can be read/edited up to 60 KB; larger files via `/file/read` with `offset` and `chunk-size` (older sources cite 4 KB) — [MikroTik Files](https://help.mikrotik.com/docs/spaces/ROS/pages/2555971/Files), [forum: max file size](https://forum.mikrotik.com/t/the-maximum-size-of-a-read-written-file/167507)
- eworm-de (most mature RouterOS script distributor) uses no checksums/signatures, only TLS verification — [eworm-de README](https://github.com/eworm-de/routeros-scripts/blob/main/README.md)

### Inferences
- If `:convert transform=sha256` exists on 7.18+, a small installer (<60 KB) could be self-verified:
  `:local c [/file get inst.rsc contents]; :if ([:convert $c transform=sha256 to=hex] != "<pinned>") do={:error "hash"}`.
  The pinned hash would live in the one-liner the user pastes, giving end-to-end integrity. Must be tested on CHR first.
- Image integrity: pinning the image by digest (`remote-image=ghcr.io/org/app@sha256:...`) is the realistic integrity
  control; whether RouterOS accepts digest references was not confirmed (gap).
- Pattern: one-liner = `/tool/fetch url=... check-certificate=yes dst-path=tikontrol-install.rsc; /import file-name=tikontrol-install.rsc verbose=yes`.
  Installer body: every object tagged with `comment="tikontrol"` and looked up via `[find comment="tikontrol"]`
  so re-runs are idempotent and an uninstall script can remove exactly those objects.

### Gaps
- Exact `/import` parameter name (`file-name=` vs positional) and `dry-run` availability per version: not verified.
- Digest-pinned `remote-image` support: no source found.
- Official confirmation of `:convert transform=sha256`: conflicting sources.

## 3. Device-mode: enabling container, detection, defaults

### Takeaway
`container` is disabled in every device mode by default and can only be enabled by `/system/device-mode/update container=yes`
followed by a physical button press or power cycle within `activation-timeout` (default 5m). This is the one step no script
can automate; the installer must detect it and stop with clear instructions.

### Cited Findings
- Change activates after a button press or power cycle, then the device reboots; if not confirmed within
  `activation-timeout` it is cancelled; a normal (soft) reboot also cancels it — [MikroTik Device mode (search snippet)](https://manual.mikrotik.com/docs/system-information-and-utilities/device-mode/)
- `activation-timeout` range 00:00:10–1d00:00:00, default 5m — [device-mode update CLI ref](https://manual.mikrotik.com/docs/cli-reference/system/device-mode/update/)
- After three unsuccessful attempts updates are refused for a period (attempt-count reset window); running
  `/system/device-mode/update` with no parameters cancels a pending change — [device-mode update CLI ref](https://manual.mikrotik.com/docs/cli-reference/system/device-mode/update/)
- Some EOL products only accept a power cycle, not a button press; holding reset during boot factory-resets — [TechOverflow 2026-10-09](https://techoverflow.net/2026/10/09/mikrotik-device-mode-explained-why-changes-need-a-button-press-or-power-cycle/), [mkcontroller guide](https://mkcontroller.com/blog/news/mikrotik/mikrotik-device-mode-security-ops-guide/)
- `traffic-gen, container, partitions, routerboard` are disabled in every mode unless explicitly enabled — [MikroTik Device-mode](https://help.mikrotik.com/docs/spaces/ROS/pages/93749258/Device-mode)
- Factory assignment: `advanced` (formerly `enterprise`) on CCR and 1100 series, `home` on home routers, `basic` on others;
  device-mode survives reset — [MikroTik Device-mode](https://help.mikrotik.com/docs/spaces/ROS/pages/93749258/Device-mode)
- 7.17 renamed enterprise → advanced, added `basic` mode and the `routerboard`, `install-any-version`, `partitions`
  features — [7.17 notes via forum/summary](https://forum.mikrotik.com/t/v7-17-2-stable-is-released/181257), [MikroTik Online Club v7.17](https://mikrotikonlineclub.com/2024/12/19/what-up-mikrotik-v7-17/)
- Home mode disables (among others) `scheduler, socks, fetch, ... zerotier, container, ...` per the property listing — [MikroTik Device-mode (search snippet)](https://help.mikrotik.com/docs/spaces/ROS/pages/93749258/Device-mode)
- eworm-de requires device-mode to allow at least `scheduler` and `fetch` — [eworm-de README](https://github.com/eworm-de/routeros-scripts/blob/main/README.md)

### Inferences
- Detection: `:if ([/system/device-mode/get container] != true) do={ ... }` (property read via `/system/device-mode/print`).
  On `home` mode `fetch` and `scheduler` may also be off — but then the one-liner fetch itself fails, so the README must
  give the full `/system/device-mode/update container=yes` (plus `fetch=yes scheduler=yes` if needed) command first.
- The installer should be two-phase: phase 1 checks device-mode and prints the command + "press the button within 5
  minutes"; phase 2 (re-run after reboot) does the rest. Never trigger the update from the installer silently.
- Note the snippet claiming home mode disables `fetch` is from a search summary; verify on a home-mode device (e.g. hAP).

### Gaps
- 7.25 WinBox device-mode UI: no source found.
- Exact boolean/string type returned by `/system/device-mode/get container` across versions: not verified.

## 4. Container plumbing: veth, bridge, NAT, config, mounts, envs, disks, limits, restart, healthcheck

### Takeaway
Several property names moved during 7.20–7.23 (envlist → envlists in 7.20, mounts → mountlists in 7.21, veth `dhcp` in
7.21, `cmd` effectively required from 7.22, restart-policy/healthcheck in 7.22/7.23 replacing `auto-restart-interval`).
An installer targeting 7.18–7.26 must branch on `/system/resource get version` or feature-detect.

### Cited Findings
- 7.20: container may reference multiple envlists (`envlists`); 7.20 also added mounting individual files and read-only
  mounts — [7.20 stable forum](https://forum.mikrotik.com/t/v7-20-stable-is-released/265196), [7.20beta2 CHANGELOG](https://download.mikrotik.com/routeros/7.20beta2/CHANGELOG)
- 7.21: container `mounts` converted to `mountlists`; old mount `name` becomes the list name; a list can map to multiple
  mounts — [7.21 stable forum](https://forum.mikrotik.com/t/v7-21-stable-is-released/267773)
- Ansible community.routeros: single `envlist` replaced by `envlists` for RouterOS ≥7.20; `name` removed from
  `/container/mounts` for <7.21 (i.e. `list=` from 7.21) — [community.routeros changelog](https://docs.ansible.com/projects/ansible/devel/collections/community/routeros/changelog.html)
- Current manual examples use `mountlists=` and `envlist=` on `/container/add` — [MikroTik Container (search summary)](https://manual.mikrotik.com/docs/containers/)
- 7.21: veth got `dhcp` setting to auto-configure IPv4 when the veth is bridged with a DHCP server; also fix for veth in
  vlan-aware bridge with multiple DHCP servers — [7.21 stable forum](https://forum.mikrotik.com/t/v7-21-stable-is-released/267773); a component tracker lists it under 7.23 — [mct.hextet.net veth](https://mct.hextet.net/component/veth) (conflict); users on 7.20.2 reported `dhcp=yes` failing — [7.20.2 forum p4](https://forum.mikrotik.com/t/v7-20-2-stable-is-released/265788?page=4)
- 7.25 fixes: container interface handling on CHR; veth left running after restarting a stopped container — [search summary of 7.25 notes](https://mct.hextet.net/component/veth)
- Random veth MAC problem "no longer a problem as of 7.20" — [tangentsoft Bridged Container VETH](https://tangentsoft.com/mikrotik/wiki?name=Bridged+Container+VETH)
- Static veth pattern: `/interface/veth add name=veth1 address=172.17.0.2/16 gateway=172.17.0.1`;
  `/interface/bridge add name=dockers`; `/ip/address add address=172.17.0.1/16 interface=dockers`;
  `/interface/bridge/port add bridge=dockers interface=veth1` — [Fluent-networks/tailscale-mikrotik README](https://github.com/Fluent-networks/tailscale-mikrotik)
- Registry config: `/container/config set registry-url=https://ghcr.io tmpdir=disk1/pull`; container add uses
  `remote-image=... interface=veth1 envlist=... cmd=... root-dir=disk1/containers/... mounts=... start-on-boot=yes hostname=... dns=...`;
  "the cmd parameter is required for RouterOS 7.22+"; `file=disk1/x.tar` replaces `remote-image` for tar installs — [Fluent-networks README](https://github.com/Fluent-networks/tailscale-mikrotik)
- An AdGuard Home installer gist creates veth, bridge, srcnat masquerade and the container (author unsure it works) — [gist kangarie](https://gist.github.com/kangarie/72d8d606d2084eb7d5ce8e2109b0adc3)
- `/container/config`: `memory-high` (global soft limit, throttles/reclaims), `memory-max` (hard, can be per container or
  global; container killed when exceeded), `tmpdir` (extraction dir), `layer-dir` (shared layer storage); some outputs show
  `ram-high` instead — version-specific naming unclear — [MikroTik Container](https://manual.mikrotik.com/docs/containers/), [config CLI ref](https://manual.mikrotik.com/docs/cli-reference/container/config/), [forum HA container](https://forum.mikrotik.com/t/home-assistant-container-on-routeros-fails-to-extract-and-start/181208)
- Setting memory-high to 100M reportedly made RouterOS restart the container — [mikrotikdocs.fyi resource limits](https://mikrotikdocs.fyi/extended-features/container/resource-limits/)
- Paths have no leading slash (`tmpdir=disk1/tmp`); `root-dir` is strict about it — [forum HA container](https://forum.mikrotik.com/t/home-assistant-container-on-routeros-fails-to-extract-and-start/181208)
- Disks: current syntax `/disk/format usb1 file-system=ext4 label=...` (older: `/disk format-drive 0 file-system=ext4`);
  slot names come from bus + number (`usb1`, `sata1`, `nvme1`); root-dir should be on external ext3/ext4, internal storage
  not recommended — [MikroTik Disks 7.25](https://manual.mikrotik.com/docs/7.25/hardware/disks/), [Disks](https://manual.mikrotik.com/docs/hardware/disks/)
- `restart-policy` (`no`, `on-failure`, `always`) with `restart-max-count`, `restart-interval`, `stop-on-unhealthy`;
  healthcheck "implemented starting 7.23" per manual vs `healthcheck-*` for ≥7.22 per Ansible; `auto-restart-interval`
  removed in 7.23 — [MikroTik Container](https://manual.mikrotik.com/docs/containers/), [community.routeros changelog](https://docs.ansible.com/projects/ansible/latest/collections/community/routeros/changelog.html)
- 7.24.1: improved host isolation for `privileged=yes` containers — [7.24.1 forum](https://forum.mikrotik.com/t/7-24-1-stable-is-released/272557)
- 7.26beta: container label support; `/app` for ARM32; container name in CEF logs — [7.26beta forum](https://forum.mikrotik.com/t/7-26beta-development-is-released/273582)
- Setting a property on a running container makes RouterOS restart it; set properties before start — [nasnet-panel PR #1140](https://github.com/nasnet-community/nasnet-panel/pull/1140)

### Inferences
- Version matrix for an installer: <7.20 `envlist=`/`mounts=` with `/container/mounts add name=`; 7.20 `envlists=`;
  ≥7.21 `mountlists=` with `/container/mounts add list=`; ≥7.21 veth `dhcp=yes` possible but static addressing is more
  predictable (needed for a firewall rule keyed on container IP); ≥7.22 always pass `cmd`/entrypoint explicitly;
  ≥7.23 `restart-policy=always` + healthcheck, else `start-on-boot=yes` only.
- Disk detection: `/disk print` and pick the first entry with an ext4 filesystem; never auto-format (destructive) —
  require explicit user parameter.
- Use a dedicated bridge + /30 or small subnet, srcnat masquerade only for that subnet, and dst-nat only if the web UI must
  be reachable on a router port (prefer reaching the container IP directly from LAN via routing).

### Gaps
- Exact `/container/envs` property names after 7.20 (`list`/`key`/`value` vs `name`): not verified.
- `ram-high` vs `memory-high` naming per version: unresolved.
- Container DNS behaviour (`dns=` property vs veth DHCP-provided DNS) beyond the Fluent example: not verified.

## 5. Upgrades: in-place image update, rollback, scheduler

### Takeaway
From 7.22 RouterOS has `/container/repull` (re-pull + re-extract image, then restart) — the cleanest in-place upgrade.
Older versions need stop → remove → re-add with the same root-dir/mounts (data on mounts survives). No built-in
auto-update or check-for-updates for images was found; a scheduler-driven script is the common pattern.

### Cited Findings
- 7.22 added `repull`, which checks the registry for an updated image, stops the container, pulls, and restarts it on the
  new image (community wiki, wording garbled) — [tangentsoft Container Limitations (search snippet)](https://tangentsoft.com/mikrotik/wiki?name=Container+Limitations)
- Manual: `repull | Re-pulls/extracts the container image`, no version given — [MikroTik Container](https://manual.mikrotik.com/docs/containers/)
- Tailscale-mikrotik documents `/container/stop 0` + `/container/update 0`, and an `upgrade.rsc` that does stop →
  remove → add → start, assuming the registry is used — [Fluent-networks README](https://github.com/Fluent-networks/tailscale-mikrotik)
- `/system/package/update check-for-updates` is for RouterOS packages only, not images — [MikroTik Upgrading](https://help.mikrotik.com/docs/spaces/ROS/pages/328142/Upgrading+and+installation)
- eworm-de auto-updates its scripts via a scheduler `start-time=startup interval=1d` calling `$ScriptInstallUpdate`,
  sending notifications when an update needs config changes — [eworm-de README](https://github.com/eworm-de/routeros-scripts/blob/main/README.md)
- No changelog entry for automatic container image updates was found — [search summary](https://tangentsoft.com/mikrotik/wiki?name=Container+Limitations)

### Inferences
- Upgrade path: pin tag `ghcr.io/org/tikontrol:X.Y.Z` (not `latest`), and upgrade by `set remote-image=` new tag then
  `repull` (≥7.22) or remove/re-add (<7.22). Keep all state in a mountlist on disk so recreating the container is safe.
- Rollback = set `remote-image` back to previous tag and repull; keep the previous tag recorded in a comment or a
  `/system/script` environment variable. Consider `/system/backup` of app data dir? (no RouterOS-native snapshot found).
- Version checks: a scheduler could fetch a small `latest.json` with `output=user` (<64 KB) and compare, but per the
  Safe-apply rules the upgrade itself should require user confirmation rather than auto-apply.

### Gaps
- Whether `repull` keeps the same container ID/config and how it behaves on pull failure (rollback): no source.
- Whether `/container/update` (mentioned in Fluent README) is a real command or an alias: unverified.

## 6. Firewall: container → router API-SSL 8729 only from the container IP

### Takeaway
No source specific to this was found; standard RouterOS firewall applies. Recommended shape (inference): restrict
`/ip/service api-ssl address=<container-ip>/32` plus an input-chain accept rule for that source/port placed before the
default drop.

### Cited Findings
- None found specifically for container → api-ssl in this session's sources.

### Inferences
- Rules (to be verified on CHR): `/ip/firewall/filter add chain=input src-address=172.17.0.2 in-interface=<container-bridge> protocol=tcp dst-port=8729 action=accept comment="tikontrol" place-before=[find ...default drop]`;
  `/ip/service set api-ssl certificate=<cert> address=172.17.0.2/32` (note: changing `/ip/service` is a high-risk change
  per project rules and needs explicit warnings + rollback). Also add `in-interface` to prevent spoofing from elsewhere.
- Static veth address is required for this; veth `dhcp=yes` would make the source IP unstable.

### Gaps
- No primary-source example; needs testing on CHR. 7.25 WinBox device-mode UI and 7.26 stable notes also unverified.
