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

- Does a read-only user really get rejected on every write?
- Do dotted names such as `channel.band` work in `proplist`?
- Can a container reach its own router's API (8729) and REST?
- How does `/safe-mode` behave over the binary API?
- Which device mode is the router in? (`/system/device-mode/print`)
- What does polling cost on small hardware?
- How does a RouterOS App appear in WinBox, and is its link clickable? (See [research/winbox_integration.md](research/winbox_integration.md))

## Backend: written but not yet tested on RouterOS

The Rust server in `src/server/` is implemented and unit-tested against a fake router only. Nothing below has run
on real RouterOS or CHR, which is why those rows in FEATURES.md are 🟨, not ✅. Test on a disposable CHR first.

- **Read-only enforcement:** create the reader with the setup commands, then prove every write is rejected by the router.
- **API details:** dotted proplists (`channel.band`), `place-before` with an `.id` for drop rules, `!trap` handling of
  each intent, and the snapshot queries' field names on 7.18+ (including the optional CMR fields).
- **Safe apply:** the scheduler `start-date` format (old `jan/02/2026` vs new `2026-01-02`), whether `policy=read,write,test`
  is enough for every inverse, behaviour under device mode, and that the revert really fires after 5 minutes.
- **Inverses not complete:** Wi-Fi passphrase and PPPoE changes restore values read beforehand; check none of these
  secrets end up in the scheduler script or the log, and decide whether such changes should be non-revertible instead.
- **Containers:** whether a container can reach its own router's API on 8729; `memory-high` vs `memory-max` on the
  RouterOS version in use; `deploy/routeros-app.yaml` is untested.
- **Setup token:** printed to the container log, which RouterOS may forward to its own log. Check who can read that.
- **MCP:** behaviour behind a reverse proxy (the LAN-only check uses the peer address).
- **Alerts:** SMTP e-mail is not implemented; ntfy, webhook and Telegram are, but untested against real services.
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
