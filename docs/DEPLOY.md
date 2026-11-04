# Deploying tikontrol

tikontrol is one static binary (about 2 MB, with the UI built in) that uses a few MB of RAM. It runs on a home server (Docker), or as a RouterOS container on an arm64 or x86 router with enough storage. Read [FEASIBILITY.md](FEASIBILITY.md) first: a home server is the safer host, because a container on the router itself cannot notice when a change cuts the LAN off.

Read-only is the default. Nothing is exposed to the internet by default: do not forward port 8443, use WireGuard or Back To Home to reach it from outside.

## Settings

All settings are environment variables.

| Variable | Default | Meaning |
|---|---|---|
| `TIK_DATA` | `./data` (`/data` in the image) | State, encryption key, TLS certificate and history. Back it up: without the `key` file the stored router passwords cannot be decrypted. |
| `TIK_LISTEN` | `0.0.0.0:8443` | HTTPS address. `off` disables HTTPS. |
| `TIK_TLS_CERT`, `TIK_TLS_KEY` | – | Your own certificate and key (PEM). Without them a self-signed certificate is created on first start and its SHA-256 is printed in the log. |
| `TIK_HTTP` | – | Plain HTTP, loopback addresses only (a local reverse proxy, development). |
| `TIK_POLL` | `30` | Seconds between router snapshots (5–3600). Lower costs router CPU. |
| `TIK_DEMO` | – | Path to a snapshot JSON (for example `src/ui/src/lib/sample.json`) to serve instead of a router. |

## First start

1. Start tikontrol and read its log. It prints the HTTPS certificate fingerprint and a one-time **setup token**.
2. Open `https://<host>:8443`, check the certificate fingerprint, and create the admin account with the setup token. The token stops working once the admin exists.
3. The setup page shows the RouterOS commands that create a certificate for API-SSL, disable the plain API, and create a `tikontrol-read` group with only `read,api,test`. Run them in a RouterOS terminal and choose the passwords yourself.
4. Enter the router address, read its certificate, compare the fingerprint with `/certificate/print detail where name=tikontrol-api`, and connect. tikontrol logs in, reads the user's real group policies, and refuses read-only mode if the user has `write`, `sensitive`, `password`, `policy` or `reboot`.

Read and write needs a second, separate `tikontrol-write` user and an explicit confirmation. Treat that password like the admin password.

## Docker (home server)

```sh
docker build -f Containerfile -t tikontrol .
docker run -d --name tikontrol --restart unless-stopped \
  --read-only --cap-drop ALL --security-opt no-new-privileges \
  --memory 64m -p 8443:8443 -v tikontrol-data:/data tikontrol
docker logs tikontrol   # setup token and certificate fingerprint
```

## RouterOS container

Untested on hardware so far (see [NEXT_STEPS.md](NEXT_STEPS.md)). Needs RouterOS 7.18 or later, the `container` package, device-mode containers enabled (a physical button press), and external storage. The hAP ac² is too small.

```sh
docker buildx build --platform linux/arm64 -f Containerfile -t tikontrol --output type=docker .
docker save tikontrol > tikontrol.tar   # upload to the router's USB disk
```

```
/interface/veth/add name=veth-tik address=172.17.0.2/24 gateway=172.17.0.1
/interface/bridge/add name=containers
/ip/address/add address=172.17.0.1/24 interface=containers
/interface/bridge/port/add bridge=containers interface=veth-tik
/container/mounts/add name=tik-data src=usb1/tikontrol dst=/data
/container/add file=usb1/tikontrol.tar interface=veth-tik root-dir=usb1/tik-root mounts=tik-data \
    logging=yes start-on-boot=yes memory-high=64M
/container/start [find where interface=veth-tik]
```

- Always cap memory. The property name has changed between RouterOS versions (`memory-high`, `memory-max`), so check `/container/add` on yours; this is unverified.
- The container runs as UID 65532, unprivileged, from a `scratch` image (no shell).
- Point tikontrol at the router through the veth gateway (`172.17.0.1`) and allow API-SSL only from `172.17.0.2/32`, as the setup commands do with tikontrol's IP.
- Reach the UI from the LAN with a dst-nat to `172.17.0.2:8443`, limited to the LAN interface list.
- The setup token appears in the container log, which RouterOS writes to its own log. It is useless once the admin exists.

## Updating and backups

- Replace the binary or image and restart; `TIK_DATA` carries over.
- Back up `TIK_DATA/key` and `TIK_DATA/state.json` together, and keep the backup private: together they decrypt the router passwords.
