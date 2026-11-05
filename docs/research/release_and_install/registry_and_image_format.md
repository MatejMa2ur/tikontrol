# RouterOS container registry pulls and image format (ghcr.io, multi-arch, FROM scratch)

Research date: 2026-11-05. Scope: RouterOS 7.18 to 7.26beta.

**Source-quality warning.** This session could not reach help.mikrotik.com, manual.mikrotik.com,
forum.mikrotik.com, download.mikrotik.com, tangentsoft.com, mikrotikdocs.fyi, bigmike.help or
blog.gntech.me. They returned DNS failures because they are outside the allowed network list. The
GitHub API was also refused. Findings marked "(search snippet)" come from search-engine summaries of
those pages, not from reading the pages. Treat them as leads to check, not as confirmed. Findings from
pages I read in full are marked "(fetched)". To check these notes properly, allow those hosts in the
network settings and read the official Container page and the 7.18–7.26 CHANGELOG files.

## Q1. Can RouterOS pull public images from ghcr.io? How do registry-url, a full path in remote-image, and auth work?

### Takeaway
Yes. Pulling public images from ghcr.io works and is documented by real projects. Set
`/container/config set registry-url=https://ghcr.io tmpdir=<disk>/pull`, then use
`remote-image=<owner>/<image>:<tag>`. RouterOS 7.18 and later also accept the registry host inside
`remote-image`, and `/container/config` has `username`/`password` (since 7.8) for private registries.

### Cited Findings
- (fetched) Fluent-networks/tailscale-mikrotik documents a ghcr.io pull: `/container/config set registry-url=https://ghcr.io tmpdir=disk1/pull`, then `/container add remote-image=fluent-networks/tailscale-mikrotik:latest ... root-dir=disk1/containers/tailscale`. Its README also notes "the **cmd** parameter is required for RouterOS 7.22+" for that image, and gives a `file=disk1/tailscale.tar` alternative. Routers without external storage can use a tmpfs disk (`/disk add type=tmpfs tmpfs-max-size=200M`) — [tailscale-mikrotik README](https://github.com/Fluent-networks/tailscale-mikrotik)
- (search snippet) The official docs list `registry-url`, `username` and `password` under `/container/config`, with username/password "starting in ROS 7.8". The default registry-url is `https://lscr.io/`. remote-image is "the container image name to be installed if an external registry is used" — [MikroTik Container docs](https://help.mikrotik.com/docs/spaces/ROS/pages/84901929/Container), [manual.mikrotik.com/docs/containers](https://manual.mikrotik.com/docs/containers/)
- (search snippet) A RouterOS changelog line reads "allow specifying registry using remote-image property". It is quoted in a forum thread about a `registry auth response: 403` on 7.18.2 with Docker Hub, which was fixed by putting the registry host at the start of `remote-image`. The thread also mentions a companion line about HTTP redirects for container registries. I could not confirm which version introduced either line — [forum 183044](https://forum.mikrotik.com/t/cont-not-ok-registry-auth-response-403-when-adding-a-container/183044)
- (search snippet) A forum user says that since 7.18 a fully qualified `remote-image` that includes the registry host works (Docker Hub example) — [forum 183332](https://forum.mikrotik.com/t/mosquitto-401-when-trying-to-install-from-docker/183332/5)
- (search snippet) Self-hosted registry with basic auth: a 401 was fixed by setting `password=` as its own property instead of putting credentials in the URL — [forum 181472](https://forum.mikrotik.com/t/self-hosted-registry-with-basic-http-auth/181472)
- (search snippet) An older thread (about 500 days old, roughly 2023 to 2024) reports "error response getting manifests" with ghcr.io as registry-url, plus 401/404 errors depending on the URL. A reply said it needed a MikroTik bug fix. **Possibly fixed:** later projects (tailscale-mikrotik above) document working ghcr.io pulls — [forum 164536](https://forum.mikrotik.com/t/import-adguard-dns-container-image-problem/164536)
- GHCR authentication uses a personal access token (classic). Pulling needs `read:packages`. Public packages can be pulled anonymously — [GitHub Docs: Container registry](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry)

### Inferences
- Use the `registry-url=https://ghcr.io` plus `owner/image:tag` form in tikontrol install docs, because a published project shows it working. Mention the full-host form (`ghcr.io/owner/image:tag`) as an alternative for 7.18+ only after testing on CHR.
- `registry-url` is global (`/container/config`). Pointing it at ghcr.io changes the default for every later `/container/add` on the router. The full-host `remote-image` form avoids that side effect on 7.18+, which is a reason to prefer it once it is verified.
- Make the package public so routers need no stored GHCR PAT. A stored PAT in `/container/config password=` sits in router config and exports, which conflicts with the project's secrets rules.

### Gaps
- No primary source read for the exact remote-image grammar (host/path/tag) or for whether `ghcr.io/` without `https://` is accepted.
- I could not confirm whether username/password apply per registry or only to the configured registry-url, or how they are stored (export visibility, `sensitive` policy).

## Q2. How does RouterOS choose an architecture from a manifest list or OCI index? Attestations, zstd, gzip, docker vs OCI tars, buildx flags

### Takeaway
RouterOS pulls multi-arch images by OS/arch (the docs suggest `os=linux arch=arm` style matching). No
MikroTik-specific bug report about OCI indexes or buildx attestation entries (`unknown/unknown`) was
found. zstd layer extraction was added in the 7.22 cycle (7.22rc2), so older releases likely need gzip
layers. The conservative recipe is gzip layers plus `--provenance=false --sbom=false`. That recipe is
inferred, not reported as required by MikroTik.

### Cited Findings
- (fetched) A NetBird user's hEX Refresh pulled `registry-1.docker.io/netbirdio/netbird:latest` with RouterOS showing `os="linux"` and `arch="arm"`. It then failed with "Process exited with signal 4 (Illegal instruction) : CPU not supported" because the arm entry was ARMv7. This shows RouterOS selects by arch `arm` and does not pick an arm/v5 variant on its own when the index lacks one (or perhaps ignores variant) — [netbird #3847](https://github.com/netbirdio/netbird/issues/3847)
- (search snippet) The newreleases.io mirror of the RouterOS **7.22rc2** changelog lists a container entry adding zstd extraction support. A 7.22 write-up says "support for unpacking images in zstd format". 7.22 stable is dated 2026-03-09. I could not confirm the wording in MikroTik's own changelog — [newreleases 7.22rc2-arm64](https://newreleases.io/project/github/elseif/MikroTikPatch/release/7.22rc2-arm64), [bigmike.help 7.22](https://bigmike.help/en/posts/routeros-7-22-apps-reverse-proxy-and-bgp-updates/)
- (search snippet) "failed to extract layer" on an ARMv5 Docker Hub image on an EN7562CT device was **fixed by upgrading to 7.19.3** — [forum 263061](https://forum.mikrotik.com/t/solved-cant-import-armv5-image-from-dockerhub-failed-to-extract-layer-routeros-container-hex-s-e60iugs-en7562ct-cpu/263061)
- (search snippet) Other "failed to extract layer" reports on direct pulls (Tailscale) were worked around by importing a tar instead — [Level1Techs](https://forum.level1techs.com/t/mikrotik-routeros-struggles-with-tailscale-containers-and-terraform/246181)
- (search snippet) Old 7.4beta thread: tar import error "could not find image manifest in archive" — [forum t=186889](https://forum.mikrotik.com/viewtopic.php?t=186889) (very old, likely irrelevant now)
- (fetched) jmrplens/mikroscope publishes one OCI image index with four platforms (linux/amd64, arm64, arm/v7 and, added in PR 7, **linux/arm/v5**) "so `--remote-image` asks the operator nothing". It also ships per-arch tars and writes the OCI `variant` matching the GOARM level instead of a fixed v7. This is a RouterOS-targeted project that pulls from an OCI index across arches — [mikroscope PR 7](https://github.com/jmrplens/mikroscope/pull/7)
- Buildx adds provenance attestations by default. On GHCR this shows as an `unknown/unknown` platform entry in the index, and `--provenance=false --sbom=false` removes it — [GitHub community #45969](https://github.com/orgs/community/discussions/45969), [docker buildx build reference](https://docs.docker.com/reference/cli/docker/buildx/build/)
- Some non-Docker runtimes fail on buildx's default attestation-wrapped indexes. The drone-buildx plugin switched its default to `--provenance=false` so single-arch pushes publish a plain image manifest — [drone-buildx PR 137](https://github.com/drone-plugins/drone-buildx/pull/137), [Fly.io community](https://community.fly.io/t/images-built-with-the-latest-docker-buildx-and-or-github-actions-work-again/10816)

### Inferences
- Recommended build for ghcr.io, to be verified on CHR and real arm/arm64 devices: `docker buildx build --platform linux/amd64,linux/arm64,linux/arm/v7,linux/arm/v5 --provenance=false --sbom=false --output type=image,push=true,compression=gzip,oci-mediatypes=true ...`. gzip keeps routers older than 7.22 working (zstd support arrived in 7.22). Removing attestations removes the `unknown/unknown` entry so a simple arch matcher cannot select it. I found no report that `oci-mediatypes=false` is needed. mikroscope's OCI index works, which is indirect evidence.
- Because RouterOS appears to match `arch=arm` without reliably honouring `variant`, the index entry order may matter on 32-bit ARM. If RouterOS takes the first `arm` entry, putting `arm/v5` before `arm/v7` would make every 32-bit device get v5 (it runs on both, only slower). This is unverified. Test on an ARMv7 device and on an EN7562CT device.
- A FROM-scratch image with a single static binary has one layer, which limits exposure to layer-extraction bugs.

### Gaps
- No primary source on how RouterOS handles the OCI `variant` field, whether it skips `unknown/unknown` entries, or how it orders index entries.
- I did not find whether RouterOS accepts `application/vnd.oci.image.index.v1+json` from every 7.18+ release (indirect evidence: mikroscope).
- No confirmed list of supported layer media types (uncompressed tar, gzip, zstd) per version.

## Q3. Arm variants: which arm images do arm v7 and arm v5 (EN7562CT) need?

### Takeaway
RouterOS reports `architecture-name=arm` for both ARMv7 and EN7562CT (ARMv5) boards. EN7562CT
(hEX Refresh, hAP ax S, hEX S E60iUGS) runs only arm32v5 soft-float binaries, and ARMv7 hard-float
binaries crash there with SIGILL. Publish a `linux/arm/v5` entry, or build 32-bit ARM as v5 for all
32-bit devices.

### Cited Findings
- (fetched) "The EN7562CT container runtime tolerates only arm32v5 soft-float (ARMv7 / hard-float SIGILL)". This affects hAP ax S and hEX Refresh. `architecture-name` = `arm` maps to the arm32 tar and `arm64` maps to the arm64 tar — [tor-mikrotik README](https://github.com/n4tash4/tor-mikrotik)
- (fetched) An ARMv7 image on hEX Refresh fails with "signal 4 (Illegal instruction) : CPU not supported". The issue was still open as of Feb 2026. hAP ax S uses the same CPU. An ARMv5 Debian-based tailscale build works (53.5 MiB vs 24.1 MiB for ARMv7 Alpine, because no official ARMv5 Alpine image exists) — [netbird #3847](https://github.com/netbirdio/netbird/issues/3847)
- (search snippet) MikroTik docs: "for devices with EN7562CT CPU like the hEX Refresh, only arm32v5 container images are supported" — [MikroTik Container docs](https://help.mikrotik.com/docs/spaces/ROS/pages/84901929/Container)
- (fetched) mikroscope sets `--goarm` default to 5, "the level that starts on every 32-bit ARM MikroTik ships", and adds linux/arm/v5 to its index and CI smoke tests — [mikroscope PR 7](https://github.com/jmrplens/mikroscope/pull/7)
- (fetched) belkone/udpxy_arm32v5 exists specifically for "Mikrotik's devices with EN7562CT CPU like the hEX Refresh, only arm32v5" — [udpxy_arm32v5](https://github.com/belkone/udpxy_arm32v5)
- (search snippet) The container package exists for arm, arm64 and x86 only (no MIPS, no PPC, no tile) — [Feature support by architecture](https://manual.mikrotik.com/docs/getting-started/software-specifications/feature-support-based-on-architecture/)

### Inferences
- For a Go or Rust static binary FROM scratch, build GOARM=5 (or `armv5te-unknown-linux-musleabi`) and publish it as `linux/arm/v5`. Optionally also publish `linux/arm/v7` for speed. Because of the selection uncertainty in Q2, the safest single 32-bit entry is v5 only.

### Gaps
- I could not confirm whether RouterOS on EN7562CT asks for `variant=v5` when pulling from an index. The NetBird log shows only `arch="arm"`.

## Q4. Tar import (`file=`): format, arch, size, tmpdir, layer-dir, RAM

### Takeaway
`file=` imports a single-image tar made by `docker save` (or a Docker/Podman tar for one platform).
Build or pull it on a PC with an explicit `--platform`. Since 7.24, `/container/save` exports an OCI
archive that Docker/Podman can read. Pulls need disk-backed `tmpdir` and `root-dir`, or a tmpfs disk on
devices without storage.

### Cited Findings
- (search snippet) Docs: a `docker save` tar can be uploaded and imported with `/container/add file=...`. The docs' own example builds with `docker buildx --platform arm64`. The docs suggest pulling with explicit arch (e.g. `podman pull --arch=arm docker.io/pihole/pihole`) and saving on a PC — [MikroTik Container docs](https://help.mikrotik.com/docs/spaces/ROS/pages/84901929/Container), [manual.mikrotik.com](https://manual.mikrotik.com/docs/containers/)
- (fetched) RouterOS **7.24** changelog (mirror): "save" command added "to allow saving container images" — via [elseif/MikroTikPatch releases](https://github.com/elseif/MikroTikPatch/releases) (search snippet for 7.24 text; 7.24 dated 2026-08-14). (search snippet) `/container/save` writes an OCI archive readable by Podman/Docker — [manual.mikrotik.com](https://manual.mikrotik.com/docs/containers/)
- (fetched) tailscale-mikrotik produces the tar with Buildx and a `PLATFORM` variable set per router CPU, using `tmpdir=disk1/pull` and a tmpfs fallback (`tmpfs-max-size=200M`) — [tailscale-mikrotik](https://github.com/Fluent-networks/tailscale-mikrotik)
- (fetched) mikroscope publishes separate per-arch tars (`-armv5.tar`, `-armv7.tar`, arm64, amd64) and prints a note when handed the v7 tar — [mikroscope PR 7](https://github.com/jmrplens/mikroscope/pull/7)
- (search snippet) Fix for "error getting the layer file" on tar import: re-save with `docker save -o` and check free disk — [forum 180943](https://forum.mikrotik.com/t/docker-built-containers-wont-work-on-mikrotik/180943)

### Inferences
- Ship one tar per platform (`docker buildx build --platform linux/arm/v5 --provenance=false -o type=docker,dest=x-armv5.tar`), not a multi-arch OCI-layout tar. No source shows RouterOS picking a platform out of a multi-arch tar.

### Gaps
- No primary source for the maximum tar size, a RAM requirement, or the `layer-dir` property. The search snippets did not mention them.
- I did not establish whether `type=oci` tars import as well as `type=docker` tars on 7.18–7.23.

## Q5. Digest pinning, tags, repull and update checks

### Takeaway
`/container/repull` (re-pull and re-extract the image) arrived in **7.22**. I found no source that
confirms `remote-image=...@sha256:` is accepted. Tag-based (`:latest` or semver tag) plus repull is the
documented update path.

### Cited Findings
- (search snippet) Official docs list `repull` as "Re-pulls/extracts the container image" — [manual.mikrotik.com](https://manual.mikrotik.com/docs/containers/)
- (search snippet) Tangentsoft: "RouterOS 7.22 finally gives us the repull command to check the OCI registry for an updated image" — [Tangentsoft Container Limitations](https://tangentsoft.com/mikrotik/wiki?name=Container+Limitations)
- (search snippet) 7.22 (2026-03-09) changelog via Telegram mirror: "app - clean the backup directory after container repull;", "container - fixed nftables/iptables not working with "Message too long" error;", plus fixes for a container lost after reboot and a missing config.json when upgrading from 7.20.8 or older — [t.me/s/mikrotik_releases](https://t.me/s/mikrotik_releases)
- (fetched) If the router's DNS points at the container being repulled, repull cuts DNS off and the pull fails — [mikrotik-adguardhome PR 15](https://github.com/maximpri/mikrotik-adguardhome/pull/15)
- (search snippet) The docs use `@sha256` only in `docker pull` examples on a PC, not in `remote-image` — [MikroTik Container docs](https://help.mikrotik.com/docs/display/ROS/Container)

### Inferences
- For reproducible installs, pin by immutable version tag (e.g. `:1.2.3`), not by digest, until digest syntax is tested on CHR. Offer a tar with a published sha256 checksum for users who want verified bytes.

### Gaps
- Digest pinning in `remote-image`: unknown. Test on CHR.
- I found no "check for updates" (update-available) property for container images.

## Q6. RouterOS changelog lines 7.18–7.26 about containers and images

### Takeaway
The relevant lines found are: registry in remote-image (version unconfirmed, by 7.18), zstd extraction
(7.22rc2), repull (7.22), `/container/save` (7.24), and none about registries in 7.25, 7.25.1, 7.24.6 or
7.26beta4/5.

### Cited Findings
- (fetched) 7.25 container/app lines (mirror): "app - add the networkRouterIP variable for container apps;", "app - allow the privileged mode to be enabled per service in the container app YAML;", "container - add a Ctrl-] escape sequence to exit a stuck container shell;", "container - fix container interface handling on CHR;", "container - fix veth interface left running after restarting a stopped container;", "container - require the container device-mode to enable swap;". 7.25.1, 7.24.6, 7.26beta4 and 7.26beta5 have no container lines, and no release on that page mentions image, registry, OCI, layer, digest, manifest, tar or repull — [elseif/MikroTikPatch releases](https://github.com/elseif/MikroTikPatch/releases)
- (search snippet) 7.24 (2026-08-14): "save" command for container images, a start-on-boot retry fix, and fewer flash writes during health checks — [elseif/MikroTikPatch releases](https://github.com/elseif/MikroTikPatch/releases)
- (search snippet) 7.22: zstd extraction (rc2), repull, and app repull backup cleanup (see Q2 and Q5). A container TUN/ip-rule regression in 7.22 was fixed in 7.23rc2 — [forum 269160](https://forum.mikrotik.com/t/fixed-routeros-7-22-container-tun-gateway-broken-sing-box-mihomo/269160)
- (search snippet) "allow specifying registry using remote-image property", plus an HTTP-redirect-for-registries line, quoted from a 7.18-era thread — [forum 183044](https://forum.mikrotik.com/t/cont-not-ok-registry-auth-response-403-when-adding-a-container/183044)

### Gaps
- I could not read the official download.mikrotik.com/routeros/<ver>/CHANGELOG files (host blocked). The 7.19–7.21 and 7.23 container lines are not covered.
