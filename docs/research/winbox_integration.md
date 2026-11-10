# Opening tikontrol from WinBox or RouterOS

Checked 2026-11-04 from MikroTik's documentation and forum. Nothing here was tested on hardware.

**Goal:** have some way to reach tikontrol from RouterOS. The method is undecided.

## Findings

- **A real tab inside WinBox looks impossible.** No plugin, extension or custom menu system was found. This comes from the docs not mentioning one, so it is not proof.
- WinBox 4 is a native app for Windows, Linux and macOS. Its menus depend on installed RouterOS packages, and the container package adds Container and App menus ([WinBox manual](https://manual.mikrotik.com/docs/management-tools/winbox/)).
- The WinBox 4.5 release post mentions UI components and table rendering, and no extensions ([forum](https://forum.mikrotik.com/t/winbox-4-5-released/274230)).
- RouterOS Apps have a read-only `ui-url`, the generated URL of the app's web interface. `show-in-webfig` (default `yes`) controls links on the WebFig login page, and custom apps can set an `icon` for it ([Apps manual](https://manual.mikrotik.com/docs/containers/apps/)).
- Custom app YAML has no field for adding a menu item, tab or embedded page. The Apps manual does not mention plugins, extensions or skins, and does not describe app access in WinBox.

## Options

| Option | Result | Works on |
|---|---|---|
| RouterOS App | Listed as an app, with a `ui-url` link to its web interface | arm64 or x86, such as the hAP be³ Media. Not the hAP ac² |
| WebFig login page | Link with our own icon | Same as above |
| Browser | Open tikontrol's address directly | Everything, including a home server |
| Modifying WinBox | Rejected: fragile, and would break with updates | - |

## To test on hardware

- Does a custom App appear in the WinBox App menu?
- Is its `ui-url` clickable in WinBox, or only shown as text?
- Does it open in a browser or inside WinBox?
- Can a router link to a tikontrol that runs on a separate home server, without an App?

## Decision (2026-11-10)

- **Primary: the RouterOS App.** On arm64 and x86 routers tikontrol is installed with `deploy/routeros-app.yaml`; RouterOS generates `ui-url` from the `web` port, and `show-in-webfig` (default yes) puts a link on the WebFig login page. Nothing in WinBox is modified.
- **Everywhere else (hAP ac², home server): the browser.** The installer adds a static DNS name (`tikontrol.lan`), so users bookmark `https://tikontrol.lan:8443`.
- **Icon:** not set yet. The manual says custom apps may set `icon`, but its format could not be checked from the build environment, and an icon loaded from GitHub would be an outbound call from the user's browser, which tikontrol avoids. It must be embedded (or served by tikontrol itself) once the format is confirmed on hardware.
- The open questions above stay as hardware checks (🔌 in FEATURES.md).
