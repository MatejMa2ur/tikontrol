# CI, releases and signing for a Rust (ring/rustls, static musl) + Svelte project on GitHub Actions (as of 2026-11-05)

Research method note: in this session `doc.rust-lang.org`, `docs.docker.com` (and likely other docs hosts) were not reachable from the sandbox. Facts were therefore taken from the **source repositories of those docs on GitHub** (`rust-lang/rust/src/doc/rustc/src/platform-support.md`, `docker/docs/content/...`), from upstream READMEs/CHANGELOGs fetched over raw.githubusercontent.com, from `git ls-remote` for action versions/SHAs, and from github.blog changelog search snippets. Version numbers and SHAs below were read live on 2026-11-05.

## 1. Cross-compiling ring/rustls to musl (x86_64, aarch64, armv7, armv5te)

### Takeaway
x86_64/aarch64 musl are Tier 2 with host tools, armv7 musleabihf is Tier 2 (prebuilt std via `rustup target add`), but **armv5te-unknown-linux-musleabi is Tier 3** (no rustup std, needs nightly `-Zbuild-std` or a toolchain image that ships one), and **ring's own CI does not test armv5te** (only armv6 `arm-unknown-linux-gnueabi` and armv7). `cross` explicitly ships an `armv5te-unknown-linux-musleabi` image (musl 1.2.5, GCC 14.4, QEMU test support), making it the lowest-risk path for armv5te; native `ubuntu-24.04-arm` runners are free and GA for public repos and are the best path for aarch64.

### Cited Findings
- Rust platform support (source of the official page): `aarch64-unknown-linux-musl` and `x86_64-unknown-linux-musl` are "Tier 2 with Host Tools" (musl 1.2.5); `armv7-unknown-linux-musleabihf` ("Armv7-A Linux with musl 1.2.5, hardfloat") and `arm-unknown-linux-musleabi` (Armv6) are "Tier 2 without Host Tools"; **`armv5te-unknown-linux-musleabi` is listed under Tier 3** ("Armv5TE Linux with musl 1.2.5"), with std marked available but no host tools — [rust-lang/rust platform-support.md](https://github.com/rust-lang/rust/blob/master/src/doc/rustc/src/platform-support.md)
  - Conflict: a search snippet of the rustc target-spec source (`armv5te_unknown_linux_musleabi.rs`) described the target metadata as "Tier 2"; the platform-support table on master says Tier 3. Treat Tier 3 (no `rustup target add`, no guarantee it builds) as authoritative and verify with `rustup target list` in CI — [rustc target spec](https://doc.rust-lang.org/stable/nightly-rustc/src/rustc_target/spec/targets/armv5te_unknown_linux_musleabi.rs.html)
- armv5te target spec: `max_atomic_width: Some(32)` with the comment "Atomic operations provided by compiler-builtins", features `+soft-float,+strict-align`, and the LLVM triple is `armv5te-unknown-linux-gnueabi` (LLVM doesn't know the musl variant) — [rustc target spec](https://doc.rust-lang.org/stable/nightly-rustc/src/rustc_target/spec/targets/armv5te_unknown_linux_musleabi.rs.html)
- ring CI matrix (main branch) tests `aarch64-unknown-linux-musl`, `arm-unknown-linux-gnueabi` (armv6, "Has assembly"), `armv7-unknown-linux-musleabihf`, `armv7-unknown-linux-gnueabihf`, `i686-unknown-linux-musl`, `x86_64-unknown-linux-musl`; **no armv5te target appears** in `ci.yml` or `mk/cargo.sh` — [ring ci.yml](https://github.com/briansmith/ring/blob/main/.github/workflows/ci.yml), [ring mk/cargo.sh](https://github.com/briansmith/ring/blob/main/mk/cargo.sh)
- ring `Cargo.toml` on main: `rust-version = "1.85"` (MSRV) — [ring Cargo.toml](https://github.com/briansmith/ring/blob/main/Cargo.toml)
- ring historically lacked fallbacks for non-x86/ARM CPUs; generic C/Rust fallbacks were added (tracked in ring #1455, generalised to "every little-endian architecture"); a thumb-only ARM issue was about ARM assembly not building on thumb-only targets — [rustsec/rustsec#962](https://github.com/rustsec/rustsec/issues/962), [ring#1512](https://github.com/briansmith/ring/issues/1512), [ring#1455](https://github.com/briansmith/ring/issues/1455)
- `cross` supported-targets table includes `armv5te-unknown-linux-musleabi` (musl 1.2.5, GCC 14.4.0, C++ yes, QEMU 10.1.0, `test` ✓), plus `armv7-unknown-linux-musleabihf`, `aarch64-unknown-linux-musl` (same toolchain versions); images are based on Ubuntu 24.04. `cross test` runs unit tests sequentially under QEMU, and is slow — [cross README](https://github.com/cross-rs/cross/blob/main/README.md), [cross targets.toml](https://github.com/cross-rs/cross/blob/main/targets.toml)
- Latest `cross` release tag is still **v0.2.5** (git main is far ahead; ripgrep pins a cross version and downloads its prebuilt musl binary "because releases of 'cross' have broken CI" in the past) — `git ls-remote` of cross-rs/cross; [ripgrep release.yml](https://github.com/BurntSushi/ripgrep/blob/master/.github/workflows/release.yml)
- ripgrep's release matrix builds `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `armv7-unknown-linux-musleabihf`, `armv7-unknown-linux-musleabi` etc. using cross (no armv5te) — [ripgrep release.yml](https://github.com/BurntSushi/ripgrep/blob/master/.github/workflows/release.yml)
- cargo-zigbuild (latest tag v0.24.1): uses `zig cc` as linker; caveats: "Some Rust targets aren't recognized by `zig cc`, for example `armv7-unknown-linux-gnueabihf`, workaround by using `-mcpu=generic`"; "zig misses some `compiler_rt` functions that may lead to undefined symbol error for certain targets"; "CPU features are not passed to clang"; only stable/nightly Rust regularly tested. Recommends `*-musl` targets for fully static binaries — [cargo-zigbuild README](https://github.com/rust-cross/cargo-zigbuild/blob/main/README.md)
- GitHub: Linux arm64 hosted runners free for public repos (preview Jan 2025, labels `ubuntu-24.04-arm`, `ubuntu-22.04-arm`), **GA on 2025-08-07**; standard runner 4 vCPU; labels do not work in private repos (need arm64 larger runners) — [GitHub changelog 2025-08-07](https://github.blog/changelog/2025-08-07-arm64-hosted-runners-for-public-repositories-are-now-generally-available/), [GitHub changelog 2025-01-16](https://github.blog/changelog/2025-01-16-linux-arm64-hosted-runners-now-available-for-free-in-public-repositories-public-preview/)
- Arm64 runner images now maintained by GitHub (earlier: by Arm) — [runner-images release ubuntu24/20260906](https://github.com/actions/runner-images/releases/tag/ubuntu24%2F20260906.298)
- Docker's own "GitHub Builder" reusable workflow maps Linux Arm platforms to `ubuntu-24.04-arm` and others to `ubuntu-24.04` — [docker/docs multi-platform.md](https://github.com/docker/docs/blob/main/content/manuals/build/ci/github-actions/multi-platform.md)

### Inferences
- Recommended split: `x86_64-unknown-linux-musl` on `ubuntu-24.04` (needs `musl-tools` or self-contained linking; ring has C/asm so a C cross/musl compiler is needed), `aarch64-unknown-linux-musl` natively on `ubuntu-24.04-arm` (can also run tests natively), `armv7-unknown-linux-musleabihf` and `armv5te-unknown-linux-musleabi` via a **pinned** `cross` (release binary or a pinned git rev) on x86_64 with QEMU test. This mirrors ripgrep's pattern and avoids zig's known ARM `-mcpu`/compiler_rt gaps.
- armv5te is the risky target: Tier 3 means `cross` must build std (cross handles `build-std` via its config, or use nightly + `-Zbuild-std`); the toolchain must be pinned (`rust-toolchain.toml` with a dated nightly for that job only). ring has no CI coverage for armv5te, so the project must run its own smoke test (e.g. `cross test` under QEMU, or at least run the binary under `qemu-arm` for a TLS handshake) before shipping. Pure-Rust fallbacks for ring ARM asm may be selected because ARMv5TE lacks NEON/ARMv7 instructions — unverified, test it.
- 32-bit-only atomics on armv5te (via compiler-builtins/kernel helpers) mean any dependency using `AtomicU64` fails to compile there; with std-only + ring + rustls this should be fine, but CI catches regressions.
- Consider whether `MikroTik arm` devices actually need armv5te vs armv7: dropping armv5te removes the only Tier 3 target.

### Gaps
- No ring issue specifically about `armv5te-unknown-linux-musleabi` found; build success for current ring 0.17.x on armv5te is unverified (GitHub search API was not enabled for that repo in this session).
- Could not confirm whether cross's armv5te-musl image ships a prebuilt std or requires `build-std` (README table marks std ✓ for its own tests; Rust upstream ships none for Tier 3).
- Latest published ring release on crates.io not confirmed (main `Cargo.toml` shows `0.17.16000`, which looks like a dev version, not a release).

## 2. Multi-arch FROM-scratch OCI image (amd64, arm64, arm/v7, arm/v5)

### Takeaway
Do not compile under QEMU in buildx. Build static binaries in the Rust matrix first, then do a single buildx build whose Dockerfile is `FROM scratch` + `COPY` of the per-platform binary selected by `TARGETARCH`/`TARGETVARIANT`. Use current majors: build-push-action v7, metadata-action v6, setup-buildx-action v4, login-action v4 (setup-qemu-action v4 only if a RUN step ever executes target binaries; a COPY-only scratch image needs no emulation).

### Cited Findings
- Docker recommends, in order: native nodes, QEMU, or "cross-compilation with multi-stage builds"; with cross-compilation you pin the build stage to `--platform=$BUILDPLATFORM` "to prevent emulation from kicking in" and use the pre-defined `BUILDPLATFORM`/`TARGETPLATFORM` (and `TARGETARCH`/`TARGETVARIANT`) args — [docker/docs multi-platform.md](https://github.com/docker/docs/blob/main/content/manuals/build/building/multi-platform.md)
- Docker's GHA multi-platform guide: default runner Docker can build and push multi-platform images but cannot `--load` them without the containerd image store; it offers `docker/github-builder` reusable workflows that split platforms per runner and publish the manifest without a separate merge job; examples include `linux/arm/v6`, `linux/arm/v7`, `linux/arm64` — [docker/docs ci multi-platform.md](https://github.com/docker/docs/blob/main/content/manuals/build/ci/github-actions/multi-platform.md)
- Current latest tags (git ls-remote, 2026-11-05): `docker/build-push-action` v7.4.0, `docker/metadata-action` v6.2.0, `docker/setup-buildx-action` v4.4.1, `docker/setup-qemu-action` v4.4.0, `docker/login-action` v4.7.0. (cosign-installer README examples still show older v3.x Docker actions — stale.) — `git ls-remote` of each repo; [cosign-installer README](https://github.com/sigstore/cosign-installer/blob/main/README.md)
- astral-sh/uv's Docker workflow (2026): SHA-pinned `docker/login-action` v4.6.0, `docker/metadata-action` v6.2.0, builds `linux/amd64,linux/arm64` with `provenance: mode=max` and `sbom: true` (via depot build-push-action) — [uv build-docker.yml](https://github.com/astral-sh/uv/blob/main/.github/workflows/build-docker.yml)

### Inferences
- Dockerfile pattern (no RUN, nothing to emulate):
  ```dockerfile
  FROM scratch
  ARG TARGETARCH
  ARG TARGETVARIANT
  COPY --chmod=0755 dist/${TARGETARCH}${TARGETVARIANT}/tikontrol /tikontrol
  USER 65534:65534
  ENTRYPOINT ["/tikontrol"]
  ```
  with the release job laying out `dist/amd64/`, `dist/arm64/`, `dist/armv7/`, `dist/armv5/` from downloaded artifacts and calling build-push-action with `platforms: linux/amd64,linux/arm64,linux/arm/v7,linux/arm/v5`. `TARGETVARIANT` is empty for amd64/arm64 and `v7`/`v5` for arm.
- The image binaries and the release-asset binaries are then byte-identical, so one provenance attestation per binary covers both; the image index digest gets its own attestation.
- A non-root numeric `USER` satisfies the repo's "containers run as non-root" rule without needing /etc/passwd in scratch. CA certificates are not needed if the server never makes outbound TLS calls; if rustls verifies a router cert, bundle the trust anchor explicitly (e.g. webpki-roots or a pinned cert), since scratch has no /etc/ssl.

### Gaps
- Not verified whether RouterOS `container` correctly selects `linux/arm/v5` vs `v7` variant from an index (needs CHR/device test).

## 3. Supply chain hardening (pinning, permissions, harden-runner, Dependabot, audits, workflow linting, Scorecard)

### Takeaway
Pin every action to a full commit SHA with a version comment, set `permissions: {}` / `contents: read` at the top level and grant write scopes per job, set `persist-credentials: false` on checkout, lint workflows with zizmor (+ actionlint), run Dependabot for `github-actions`, `cargo` and `npm` with a cooldown (zizmor recommends 7 days), and run cargo-deny (advisories/licenses/bans/sources) plus `npm audit` in CI. GitHub can now enforce SHA pinning via repository/org Actions policy.

### Cited Findings
- GitHub Actions policy now supports blocking actions (`!` prefix, evaluated last) and **enforcing SHA pinning** (checkbox in allowed-actions policy), announced 2025-08-15 — [GitHub changelog 2025-08-15](https://github.blog/changelog/2025-08-15-github-actions-policy-now-supports-blocking-and-sha-pinning-actions/)
  - Community reports: enforcement is checked at run time (job fails in setup), and also applies to actions referenced inside third-party composite actions — [zenn.dev write-up](https://zenn.dev/shunsuke_suzuki/articles/github-actions-enforce-sha-pinning), [Classmethod](https://dev.classmethod.jp/en/articles/github-actions-require-sha-pinning/)
- zizmor audit list (current docs) includes `artipacked` (checkout credential persistence), `cache-poisoning`, `dangerous-triggers`, `dependabot-cooldown`, `excessive-permissions`, `impostor-commit`, `known-vulnerable-actions`, `ref-version-mismatch` (SHA vs comment mismatch), `secrets-inherit`, `stale-action-refs`, `template-injection`, `undocumented-permissions`, `unpinned-images`, `unpinned-tools`, `unpinned-uses`, `unredacted-secrets` and more — [zizmor audits.md](https://github.com/zizmorcore/zizmor/blob/main/docs/audits.md)
- zizmor `dependabot-cooldown` (since v1.15.0): Dependabot's default cooldown is three days; zizmor recommends seven, because package compromises are typically opportunistic and quickly taken down — [zizmor audits.md](https://github.com/zizmorcore/zizmor/blob/main/docs/audits.md); background: [We should all be using dependency cooldowns](https://blog.yossarian.net/2025/11/21/We-should-all-be-using-dependency-cooldowns)
- zizmor `cache-poisoning` audit exists: caches restored in release/publishing workflows can be poisoned from less-privileged runs — [zizmor audits.md](https://github.com/zizmorcore/zizmor/blob/main/docs/audits.md)
- actions/setup-node (v6+/v7): npm caching is **enabled automatically** when `packageManager`/`devEngines.packageManager` in package.json is npm; README recommends `package-manager-cache: false` "for workflows with elevated privileges or access to sensitive information"; action runs on node24; `always-auth` removed — [setup-node README](https://github.com/actions/setup-node/blob/main/README.md)
- cargo-deny-action: default `command: check`, `arguments: --all-features` (checks advisories, bans, licenses, sources against RustSec DB) — [cargo-deny-action README](https://github.com/EmbarkStudios/cargo-deny-action/blob/main/README.md)
- Latest tags + commit SHAs (dereferenced tag objects, from `git ls-remote`, 2026-11-05). uv's workflow independently uses the same checkout SHA for v7.0.1, cross-checking this method ([uv build-docker.yml](https://github.com/astral-sh/uv/blob/main/.github/workflows/build-docker.yml)):
  - `actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1`
  - `actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1 # v7.1.0`
  - `actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2`
  - `actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333 # v8.0.2`
  - `actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6 # v4.2.2`
  - `docker/build-push-action@c3c9e263c25d99ce0380d002d59b67737d91b0dc # v7.4.0`
  - `docker/metadata-action@dc802804100637a589fabce1cb79ff13a1411302 # v6.2.0`
  - `docker/setup-buildx-action@f87e5991a6d7451dcb8d9637bfbc97413f497069 # v4.4.1`
  - `docker/login-action@81c6f93d4bc991e510b342a00b419b3bb4efc5c6 # v4.7.0`
  - `sigstore/cosign-installer@756fe2d284c7292c4e9b62c8a28f1e5629c59532 # v4.1.3`
  - `step-security/harden-runner@46b06cea07f9e64621fdf066ba77ac6915163d43 # v2.22.3`
  - `Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3`
  - `zizmorcore/zizmor-action@45d7276c9ca790aabd705e31ca99c6185c3e76dd # v0.8.1`
  - `ossf/scorecard-action@fa7117db83c52d787ae5a89f350999701505d78e # v2.5.0`
  - `googleapis/release-please-action@45996ed1f6d02564a971a2fa1b5860e934307cf7 # v5.0.0`
  - `EmbarkStudios/cargo-deny-action@3c6349835b2b7b196a839186cb8b78e02f7b5f25 # v2.1.1`
  - `github/codeql-action@33d7b85da1a04171e211befe2c739303b9a4c066 # v4.39.3`
  - `softprops/action-gh-release@efb35369e0ad2afab669f228072c1b0d510eae64 # v3.0.3`
  - Other latest tags (SHAs not fetched): `actions/attest-build-provenance` v4.2.2, `actions/attest-sbom` v4.1.0, `docker/setup-qemu-action` v4.4.0, `rustsec/audit-check` v2.0.0, `anchore/sbom-action` v0.24.3, `taiki-e/install-action` v2.90.1, `rhysd/actionlint` v1.7.12, `axodotdev/cargo-dist` v1.0.0-rc.1 (pre-release). `dtolnay/rust-toolchain` has no semver tags (used as `@stable`/`@master` branches; pin by SHA).

### Inferences
- Top of every workflow: `permissions: {}` (or `contents: read`), then per job only what it needs (release job: `contents: write`; image job: `packages: write`, `id-token: write`, `attestations: write`; Scorecard: `security-events: write`, `id-token: write`).
- Use `actions/checkout` with `persist-credentials: false` (zizmor `artipacked`).
- Do not restore caches in the tag/release workflow (or use `save-if: false` and accept slower releases) to avoid cache poisoning; set `package-manager-cache: false` on setup-node in release jobs.
- `pull_request_target` and `workflow_run` should be avoided (zizmor `dangerous-triggers`); never interpolate `${{ github.event.* }}` into `run:` (template-injection).
- harden-runner (v2.22.3) in `egress-policy: audit` first, then `block` with an allow list; it is a third-party action with its own trust cost — weigh against the repo's "few dependencies" rule.
- Enable repository settings: "Require actions to be pinned to a full-length commit SHA", "Allow GitHub Actions to create and approve PRs" off, default `GITHUB_TOKEN` read-only, tag protection rulesets for `v*`.

### Gaps
- Could not fetch OpenSSF Scorecard check docs or harden-runner README in this session; specific Scorecard checks (Pinned-Dependencies, Token-Permissions, Signed-Releases, Branch-Protection) are from background knowledge, not verified here.
- `cargo audit` vs `cargo deny` overlap not re-verified; both use the RustSec advisory DB (background knowledge).

## 4. Signing and provenance (cosign, GitHub artifact attestations, SLSA, checksums, RouterOS-safe image index)

### Takeaway
Use `actions/attest` v4 (attest-build-provenance v4 is now just a wrapper) for release binaries (`subject-path` or `subject-checksums: SHA256SUMS`) and for the image digest with `push-to-registry: true`; set `provenance: false` and `sbom: false` on build-push-action so BuildKit does **not** insert `unknown/unknown` attestation manifests into the index, which simple clients like RouterOS may mis-handle. Cosign v3 keyless signing stores signatures as OCI 1.1 referring artifacts (not in the index), so it is also safe. GitHub immutable releases (GA Oct 2025) add an automatic release attestation and lock tags.

### Cited Findings
- `actions/attest-build-provenance`: "As of version 4, `actions/attest-build-provenance` is simply a wrapper on top of `actions/attest`." Signs with short-lived Sigstore certificate; public repos use the public-good Sigstore instance; attestation stored in the GitHub attestations API; verify with `gh attestation`. Available for public repos on all current plans — [attest-build-provenance README](https://github.com/actions/attest-build-provenance/blob/main/README.md)
- `actions/attest` modes: no predicate → SLSA build provenance; `sbom-path` → SBOM attestation (SPDX/CycloneDX, ≤16 MB); custom predicate. Subjects via exactly one of `subject-path`, `subject-digest`, `subject-checksums`. Required permissions: `id-token: write`, `attestations: write`, `artifact-metadata: write` — [actions/attest README](https://github.com/actions/attest/blob/main/README.md)
- `actions/attest` `push-to-registry` (default false) pushes the attestation to the image registry; requires a single fully-qualified OCI image reference with SHA-256 digest; for images use `subject-name` (no tag, e.g. `ghcr.io/user/app`) + `subject-digest`. `create-storage-record` (default true with push) only works for organization-owned repos — [actions/attest README](https://github.com/actions/attest/blob/main/README.md)
- BuildKit: "Provenance attestations with the `mode=min` level are added to images by default"; they are "attached to the image index in a manifest"; each attestation manifest refers to one platform image manifest; disable with `--provenance=false` (and `--sbom=false`) or `BUILDX_NO_DEFAULT_ATTESTATIONS` — [docker/docs attestations/_index.md](https://github.com/docker/docs/blob/main/content/manuals/build/metadata/attestations/_index.md)
- docker/build-push-action default: public repo → provenance `mode=max`; private → `mode=min`; warns that max provenance in public repos includes build-arg values (don't pass secrets as build args). SBOM not added unless `sbom: true` — [docker/docs ci attestations.md](https://github.com/docker/docs/blob/main/content/manuals/build/ci/github-actions/attestations.md)
- Cosign v3.0 (current installer examples pin v3.1.4): new defaults "on by default": standardized Sigstore bundle format, trusted-root/signing-config, and "**container signatures stored as an OCI Image 1.1 referring artifact**"; `--bundle` is required for blob signing output in v3 — [cosign CHANGELOG](https://github.com/sigstore/cosign/blob/main/CHANGELOG.md); keyless example `cosign sign --yes ${images}` — [cosign-installer README](https://github.com/sigstore/cosign-installer/blob/main/README.md)
- Cosign defaults: keyless with Fulcio, always uploads to Rekor (`--tlog-upload` toggles) — [cosign CHANGELOG](https://github.com/sigstore/cosign/blob/main/CHANGELOG.md)
- Immutable releases GA 2025-10-28: tags of immutable releases can't be moved/deleted; releases get signed attestations (Sigstore bundle format) verifiable with GitHub CLI; community reports that manually uploading assets breaks the attestation and that release attestations lacked tlog entries — [GitHub changelog 2025-10-28](https://github.blog/changelog/2025-10-28-immutable-releases-are-now-generally-available/), [community discussion #178351](https://github.com/orgs/community/discussions/178351), [GitHub docs immutable releases](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases)
- RouterOS: 7.9 beta changelog added OCI manifest support to `container pull`; forum threads show pull failures tied to image naming (`library/` prefix) and a tar-upload workaround. **No source found** describing RouterOS behaviour with `unknown/unknown` attestation manifests — [MikroTik forum: Container problems on CHR](https://forum.mikrotik.com/t/container-problems-on-chr/181730), [MikroTik forum: adguard import](https://forum.mikrotik.com/t/import-adguard-dns-container-image-problem/164536?page=2)

### Inferences
- Release flow: build binaries → `sha256sum * > SHA256SUMS` → `actions/attest` with `subject-checksums: SHA256SUMS` (one attestation covering all assets) → upload assets + SHA256SUMS to the release (ideally created as draft, assets attached, then published, so immutable-release attestation covers final assets). Users verify with `sha256sum -c SHA256SUMS` and `gh attestation verify tikontrol-<target> --repo <owner>/tikontrol` (offline-friendly bundle download via `gh attestation download` is available).
- Image flow: build-push-action with `provenance: false`, `sbom: false` → `actions/attest` with `subject-name: ghcr.io/<owner>/tikontrol`, `subject-digest: ${{ steps.build.outputs.digest }}`, `push-to-registry: true` → optionally `cosign sign --yes ghcr.io/<owner>/tikontrol@<digest>`. Then check with `docker buildx imagetools inspect` (or `crane manifest`) that the index lists exactly the four platforms and no `unknown/unknown` entries; test pull on CHR/RouterOS.
- On SLSA: GitHub-hosted runner + `actions/attest` provenance is generally described by GitHub as SLSA v1.0 Build Level 2; reaching L3 needs reusable-workflow isolation (background knowledge, not re-verified here). Cosign signing in addition to GitHub attestations is optional redundancy; pick one primary verification story for docs.
- Don't add SBOM inside the index; if an SBOM is wanted, generate SPDX (e.g. `cargo auditable`/`cargo cyclonedx` or anchore/sbom-action) and attest with `sbom-path` + `push-to-registry`.

### Gaps
- Whether GHCR serves the OCI 1.1 Referrers API (vs cosign/attest falling back to the `sha256-<digest>` tag scheme) was not verified; either way the referrer is not part of the image index, but a fallback tag shows up as an extra tag in the package.
- Exact `gh attestation verify` flags for OCI images (`oci://` prefix, `--bundle-from-oci`) not re-verified in this session (docs.github.com not fetched).
- No authoritative source found for RouterOS handling of attestation manifests; must be proven on CHR.

## 5. Release automation (release-please vs cargo-release vs dist vs plain tags), SemVer and changelog

### Takeaway
For a single-crate + embedded UI repo with `type(scope): subject` commits, release-please (action v5.0.0) gives a reviewable "release PR" that bumps `Cargo.toml` version and writes `CHANGELOG.md` from Conventional Commits; the actual build/sign/publish then runs on the tag. A plain tag-triggered workflow (ripgrep style) plus hand-edited changelog is the simplest alternative. `dist` (formerly cargo-dist) is at v1.0.0-rc.1 and generates its own workflow, but does not cover the custom multi-arch scratch image.

### Cited Findings
- `googleapis/release-please-action` latest tag v5.0.0 — `git ls-remote`
- `dist` (formerly `cargo-dist`): workflow is "commit, `git tag v0.2.0`, push" and dist builds and publishes; latest tag `v1.0.0-rc.1` — [cargo-dist README](https://github.com/axodotdev/cargo-dist/blob/main/README.md)
- ripgrep uses a tag-triggered release workflow with a build matrix and cross pinned to a specific release binary — [ripgrep release.yml](https://github.com/BurntSushi/ripgrep/blob/master/.github/workflows/release.yml)
- `softprops/action-gh-release` latest v3.0.3 — `git ls-remote`

### Inferences
- Simplest secure option consistent with the repo's "few dependencies" rule: tag-triggered (`on: push: tags: ['v*.*.*']`) workflow, `gh release create --draft --verify-tag --notes-file` from the changelog section, upload assets with `gh release upload`, then `gh release edit --draft=false` (no third-party release action needed; `gh` is preinstalled on runners). Add release-please later if manual changelog upkeep becomes a burden.
- Validate in the workflow that the tag equals `Cargo.toml` version and `package.json` version (fail otherwise).
- Protect `v*` tags with a ruleset and enable immutable releases.

### Gaps
- release-please Rust-strategy specifics (Cargo.lock update, combined npm+cargo versions) and cargo-release behaviour not fetched in this session.

## 6. Caching and CI speed

### Takeaway
Use Swatinem/rust-cache v2.9.3 with `save-if` restricted to the default branch and per-target keys; rely on setup-node v7's automatic npm cache in CI (disable in release); build the UI once and pass `dist/` as an artifact to all Rust target jobs.

### Cited Findings
- Swatinem/rust-cache inputs: `shared-key`, `workspaces` (`$workspace -> $target`), `cache-targets`, `cache-on-failure`, `save-if` (example `save-if: ${{ github.ref == 'refs/heads/master' }}`) — [rust-cache README](https://github.com/Swatinem/rust-cache/blob/master/README.md)
- setup-node automatic npm caching when `packageManager` is npm; `package-manager-cache: false` recommended for privileged workflows — [setup-node README](https://github.com/actions/setup-node/blob/main/README.md)
- `cross test` under QEMU is slow and sequential — [cross README](https://github.com/cross-rs/cross/blob/main/README.md)
- Native `ubuntu-24.04-arm` runners are free for public repos (avoid QEMU for arm64) — [GitHub changelog 2025-08-07](https://github.blog/changelog/2025-08-07-arm64-hosted-runners-for-public-repositories-are-now-generally-available/)

### Inferences
- PR CI: (1) lint job: zizmor + actionlint, `cargo fmt --check`, `cargo clippy -D warnings`, `npm ci && npm run check && node --test tests/*.test.ts && npm run build`, `cargo deny check`, `npm audit --audit-level=high`; (2) test on `ubuntu-24.04` (x86_64 musl) and `ubuntu-24.04-arm` (aarch64 musl); (3) build-only (no QEMU test) for armv7 and armv5te on PRs, full QEMU smoke tests on main/tags. Use `concurrency: { group: ${{ github.workflow }}-${{ github.ref }}, cancel-in-progress: true }` for PRs.
- Release: no cache restore (poisoning), reproducible-ish flags (`--locked`, `CARGO_PROFILE_RELEASE_STRIP`, `SOURCE_DATE_EPOCH` from the tag commit).

### Gaps
- No measured build times for ring on these targets; not benchmarked.
