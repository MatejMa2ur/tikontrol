# Next steps

Where to pick up on a device that can reach a RouterOS router. Read `CLAUDE.md` first: its rules are mandatory.

## Set up

```sh
git clone https://github.com/MatejMa2ur/tikontrol.git
cd tikontrol/src/ui
npm install
npm run dev
```

Before committing from a new device, check that `git config user.name` and `user.email` match the existing commits.

## First session on the router

Read-only only. Make no changes to a live router, and test anything that writes on a disposable CHR.

1. Run the snapshot from the repo root, adding `user@host` if the default doesn't fit:
   ```sh
   python src/scripts/snapshot.py > src/ui/src/lib/snapshot.json
   ```
2. Check that it finishes, and whether the new `wifi` queries only print warnings on a router without that package.
3. Open the UI and compare it with the router. It uses `snapshot.json` automatically when the file exists.
4. Keep the results private. `snapshot.json` is gitignored and must never be committed. Real values must not reach issues, docs, screenshots or commits.

## Questions to answer on hardware or CHR

From [FEASIBILITY.md](FEASIBILITY.md):

- Do dotted names such as `channel.band` work in `proplist`?
- Can a container reach its own router's API (8729) and REST?
- How does `/safe-mode` behave over the binary API?
- Which device mode is the router in? (`/system/device-mode/print`)
- What does polling cost on small hardware?
- How does a RouterOS App appear in WinBox, and is its link clickable? (See [research/winbox_integration.md](research/winbox_integration.md))

## Test router (CHR)

`src/scripts/chr.sh` boots a disposable CHR (RouterOS arm64) in QEMU and forwards its API to localhost (api-ssl on
8729). It runs under TCG emulation, because with HVF on Apple Silicon the CHR kernel panics. A fresh CHR has no
API-SSL certificate and no `WAN` interface list: create both before testing. QEMU's user network puts NAT between
tikontrol and the router, like Docker's bridge network does.

## Verified on CHR 7.25.1

Driven through tikontrol's HTTP API, the same endpoints the UI calls. The UI itself was not clicked through.

- **One-step setup** creates the reader group and user. It now restricts the reader to the address the router sees
  (`/user/active`), not tikontrol's own address. Before this fix, setup behind NAT locked the reader out.
- **Read-only enforcement:** a user in the reader group gets `not enough permissions` for every write tried:
  identity, address, firewall, user and group, own password, services, scheduler, script, reboot, file and DNS flush.
  `/export` over the API returns nothing, and a WireGuard private key never appears in any answer.
- **Safe apply:** the new `start-date` format is accepted, and `policy=read,write,test` is enough for every inverse
  tried. Keep and Revert both remove the revert job. With no confirmation, the router reverted the change by
  itself after 5 minutes and logged it.
- **Intents** work end to end, including keep and revert: block and unblock, pause and resume, limit and unlimit,
  DNS record, port forward, static route, UPnP, ad-list toggle, disable services and WireGuard peer.
- **Fixed in the same pass:**
  - Fixed IP silently did nothing when there was no lease.
  - Reverting a Fixed IP on a lease that was already static deleted that lease. tikontrol now refuses the change.
  - Kick only knew the legacy `wireless` path. It now also handles `wifi`.
  - The change history was ordered by random id instead of by time.

## Backend: still to test on RouterOS

- **API details:** dotted proplists (`channel.band`) and Wi-Fi intents need wireless hardware (CHR has none), plus
  the optional CMR fields. Fixed IP on a dynamic lease and Kick have only been tested in their refusal paths.
- **Safe apply:** behaviour under device mode; revert after a router reboot inside the window.
- **Inverses not complete:** Wi-Fi passphrase and PPPoE changes restore values read beforehand; check none of these
  secrets end up in the scheduler script or the log, and decide whether such changes should be non-revertible instead.
- **Containers:** whether a container can reach its own router's API on 8729; `memory-high` vs `memory-max` on the
  RouterOS version in use; `deploy/routeros-app.yaml` is untested.
- **Setup token:** printed to the container log, which RouterOS may forward to its own log. Check who can read that.
- **MCP:** behaviour behind a reverse proxy (the LAN-only check uses the peer address).
- **Alerts:** e-mail (SMTP), ntfy, webhook and Telegram are built but untested against real services (see HARDWARE_TESTS.md).
- **Not done:** multi-router / multi-site, GeoIP traffic by country, max-speed indicators.
- **Before a release:** `npm audit`, `cargo audit`, a `/security-review` pass and a CHANGELOG entry.

## Open decisions

- Replace "UniFi-inspired" in the README with neutral wording? The study suggests it.
- The README still describes tikontrol as a mockup; update it once the backend is tested on CHR.
- Update `PLAN.md`: home server first, CMR optional, read-only first, MCP as a phase.
- Choose a license. The repo has none yet.
- Decide how users open tikontrol from RouterOS.

## Open checks

- `src/ui/src/lib/Icon.svelte`: confirm whether any icons come from Feather or Lucide. Credit or redraw them if so.
- Review the 2 low-severity `npm audit` findings.
- Check the mockup against WCAG 2.2 AA.
