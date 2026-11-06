# Listing tikontrol in the RouterOS App catalogue

Checked 2026-11-07 from MikroTik's Apps manual and a public third-party store. Not tested on hardware.

## Findings

- RouterOS 7.22 added a configurable app store URL. `/app/settings set app-store-urls="<url>"`: "The URL must point to a YAML array where each application is an element within the array", served as plain text from a server the router can reach. The manual's example is an array of normal app definitions (`name`, `descr`, `page`, `category`, `icon`, `default-credentials`, `services`) ([MikroTik Apps manual](https://manual.mikrotik.com/docs/containers/apps/)).
- Apps installed from a store show `from-app-store` and `app-store-url` in `/app/print` ([same manual](https://manual.mikrotik.com/docs/containers/apps/)).
- A public store served from `raw.githubusercontent.com` works the same way: one `apps.yaml` array, added with `/app/settings set app-store-urls=".../apps.yaml"` ([kernelorg/mikrotik-apps](https://github.com/kernelorg/mikrotik-apps)). Orb ships its sensor like this; on 7.24.5 the catalogue updated about 10 seconds after setting the URL ([Orb docs PR #134](https://github.com/orbforge/orb-docs/pull/134)).
- The setting holds the store URL list as a whole: setting it replaces other stores unless they are listed too (inferred from it being one property; unverified).
- The YAML has no keys for `use-https`, network, memory limits or the api-ssl certificate. Those are `/app` or `/container` properties, so a store install still needs the steps in [INSTALL.md](../INSTALL.md). More background: [release_and_install.md](release_and_install.md).

## Decision

The format is documented, so the repo ships [`deploy/app-store.yaml`](../../deploy/app-store.yaml): a one-element array whose entry equals `deploy/routeros-app.yaml` (a test keeps them identical). `deploy/install.rsc` stays the recommended path, because it also does the parts the catalogue can't.

## To test on hardware

- Does the entry appear in WinBox and WebFig App after setting the URL, and does Install work?
- Does `raw.githubusercontent.com` (content type `text/plain`) parse?
- Does `/app/update` on a store app follow the image tag the same way as for a custom app?
