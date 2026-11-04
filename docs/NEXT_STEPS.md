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

## Gates before writing a backend

1. Try MikroDash and 2GT MikroTik Manager for a week, and list what is missing.
2. Answer the questions above.
3. Trim the mockup to what RouterOS can actually report.

## Open decisions

- Replace "UniFi-inspired" in the README with neutral wording? The study suggests it.
- Update `PLAN.md`: home server first, CMR optional, read-only first, MCP as a phase.
- Choose a license. The repo has none yet.

## Open checks

- `src/ui/src/lib/Icon.svelte`: confirm whether any icons come from Feather or Lucide. Credit or redraw them if so.
- Review the 2 low-severity `npm audit` findings.
- Check the mockup against WCAG 2.2 AA.
