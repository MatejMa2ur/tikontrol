# Install tikontrol on your MikroTik router

One paste in the router's terminal installs tikontrol as a RouterOS App. The script checks your router first and stops with a plain explanation when something is missing. It never formats a disk, never changes the plain `api` service, the firewall or any user.

> **Status:** the steps the script automates were done by hand on a hAP ac² with RouterOS 7.26beta1. The script itself has not been run on hardware yet. If you try it, please report the result in an issue.

## What you need

- **RouterOS 7.22 or later** (32-bit ARM routers need 7.26 or later for Apps).
- **A router that can run containers:** ARM (hAP ac², hAP ac³, RB4011), ARM64 (hAP ax², hAP ax³, hAP be³, RB5009, CCR2xxx) or x86 (CHR). MIPS, PowerPC and Tile routers (hEX, hAP lite, RB750Gr3, CCR1xxx) can't: use a [home server](#home-server-instead) instead. Routers with the EN7562CT chip (hEX refresh, hAP ax S) are 32-bit ARMv5, which the current image does not support.
- **The `container` package** (in the "Extra packages" download for your router) and **device-mode containers enabled** (needs one button press, see below).
- **A USB stick or microSD card formatted ext4.** 1 GB is plenty. Most routers have too little internal flash. A router with at least 100 MB free internal storage can do without.
- About **20 MB of free RAM**.

## Install

Open a terminal (WinBox → New Terminal, WebFig → Terminal, or SSH) and paste:

```
/tool/fetch url="https://raw.githubusercontent.com/MatejMa2ur/tikontrol/master/deploy/install.rsc" dst-path=tikontrol-install.rsc; /import tikontrol-install.rsc
```

Run the same line again after fixing anything it reports; it skips what is already done. At the end it prints:

- the address: `https://tikontrol.lan:8443`, or `https://<app-ip>:8443` (WebFig also lists it under **App → tikontrol**);
- the API-SSL certificate fingerprint, to compare on tikontrol's setup page;
- how to read the setup token: `/container/log/print where message~"setup token"`.

Then continue on tikontrol's setup page: it shows the commands for the read-only RouterOS user (see [DEPLOY.md](DEPLOY.md#first-start)). Your browser warns about the certificate the first time; [Trusted HTTPS](DEPLOY.md#trusted-https) explains how to trust it.

If you have several ext4 disks, choose one first with `:global tikDisk "usb1"`.

## What the script changes

| What | Change | Comment |
|---|---|---|
| Certificate `tikontrol-api` | Self-signed, 10 years, created only if missing | `tik: tikontrol api-ssl` |
| `/ip/service` api-ssl | Enabled. Gets `tikontrol-api` and TLS 1.2 only if it has no certificate yet, and is limited to your LAN subnet if it has no address limit yet. Nothing else in `/ip/service` | – |
| `/app/settings` | `disk=` the ext4 disk (only when the app is new) | – |
| `/app` tikontrol | Added on your LAN bridge, `auto-update=yes`, `use-https=no`, enabled. An existing app is not re-created | found by name |
| `/container/config` | `memory-high=40M memory-max=48M`, only if no limit is set. This is global: it applies to every container | – |
| DHCP lease of the app | Made static, so the address stays | `tik: tikontrol app` |
| `/ip/dns/static` | `tikontrol.lan` → the app's IP | `tik: tikontrol app` |
| File `<disk>/tikontrol-app.yaml` | A copy of the app definition, for reference | – |

App data (settings, encryption key, history) lives in `<disk>/apps/tikontrol/` and survives updates.

## What it can't do for you

- **Enable containers in device-mode.** MikroTik requires someone at the router: run `/system/device-mode/update container=yes`, then within 5 minutes press the reset/mode button briefly or unplug the power. A normal reboot cancels it.
- **Install the container package.** Download "Extra packages" for your architecture and exact RouterOS version from mikrotik.com/download, drag `container-*.npk` into Files, and reboot.
- **Format a disk.** That erases it, so the script only prints the command.

## Update

`auto-update=yes` installs new images automatically. To update now: `/app/update tikontrol`. Your data stays.

## Uninstall

```
/tool/fetch url="https://raw.githubusercontent.com/MatejMa2ur/tikontrol/master/deploy/uninstall.rsc" dst-path=tikontrol-uninstall.rsc; /import tikontrol-uninstall.rsc
```

It removes the static lease and DNS name, removes the certificate if no service uses it, and disables the app but keeps it and its data. To delete the app and its data too, run `:global keepData "no"` first. It never changes api-ssl, users, the firewall or the disk. To remove the memory limit: `/container/config/set memory-high=unlimited memory-max=unlimited`.

## Troubleshooting

| Problem | Fix |
|---|---|
| "Containers are switched off in device-mode" | See above: the command plus a button press or power cut within 5 minutes. |
| `fetch` is not allowed | Device-mode `home` may disable fetch. Run `/system/device-mode/update container=yes fetch=yes` with the button press. |
| `/tool/fetch` fails with a certificate error | Check the router's clock (`/system/clock/print`, or enable `/system/ntp/client`). |
| "none is formatted ext4" | FAT32 does not work for containers. Format the stick as ext4: `/disk/format usb1 file-system=ext4 label=tik`. **This erases everything on the disk**; check the name with `/disk/print` first. |
| "No ext4 disk" / no space | Internal flash is usually a few MB. Use a USB stick or microSD card. |
| The app waits for a "reverse proxy" or certificate | `use-https=yes` waits for a MikroTik-cloud certificate. tikontrol does its own HTTPS: `/app/set [find name=tikontrol] use-https=no`. |
| The app stops or restarts by itself | Low memory. Check `/system/resource/print` and `/container/print`. Free RAM by turning off unused packages, or raise `/container/config memory-max`. |
| "The app has no IP yet" | The image is still downloading. Check `/app/print detail where name=tikontrol` and `/log/print where topics~"container"`, then run the installer again. |
| `tikontrol.lan` does not open | Your devices must use the router as DNS server and `/ip/dns allow-remote-requests` must be `yes`. Use the IP instead. |
| Illegal instruction in the container log | The router is ARMv5 (EN7562CT). Use a home server. |

## Home server instead

Any Linux machine, NAS or Raspberry Pi with Docker works and is the safer host (see [FEASIBILITY.md](FEASIBILITY.md)):

```sh
docker run -d --name tikontrol --restart unless-stopped \
  --read-only --cap-drop ALL --security-opt no-new-privileges \
  --memory 64m -p 8443:8443 -v tikontrol-data:/data ghcr.io/matejma2ur/tikontrol:latest
docker logs tikontrol   # setup token and certificate fingerprint
```

The router still needs API-SSL; tikontrol's setup page shows the commands. More in [DEPLOY.md](DEPLOY.md).

## One-click from the App catalogue (optional)

The repo also publishes a custom app store ([research](research/app_store.md)). After `/app/settings/set app-store-urls="https://raw.githubusercontent.com/MatejMa2ur/tikontrol/master/deploy/app-store.yaml"`, tikontrol appears in WinBox/WebFig **App** and installs with one click. That setting replaces other store URLs. The catalogue only installs the app: you still need the disk, `use-https=no`, API-SSL and the memory limit, so the script above is the easier path.
