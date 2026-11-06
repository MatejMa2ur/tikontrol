# Deploying tikontrol

tikontrol is one static binary (about 2.5 MB, with the UI built in) that uses a few MB of RAM. It runs on a home server (Docker), or as a RouterOS container on an arm64 or x86 router with enough storage. Read [FEASIBILITY.md](FEASIBILITY.md) first: a home server is the safer host, because a container on the router itself cannot notice when a change cuts the LAN off.

Read-only is the default. Nothing is exposed to the internet by default: do not forward port 8443, use WireGuard or Back To Home to reach it from outside.

## Settings

All settings are environment variables.

| Variable | Default | Meaning |
|---|---|---|
| `TIK_DATA` | `./data` (`/data` in the image) | State, encryption key, TLS certificate and history. Back it up: without the `key` file the stored router passwords cannot be decrypted. |
| `TIK_LISTEN` | `0.0.0.0:8443` | HTTPS address. `off` disables HTTPS. |
| `TIK_TLS_CERT`, `TIK_TLS_KEY` | – | Your own certificate and key (PEM), used as they are. Without them tikontrol makes its own local CA and server certificate, see [Trusted HTTPS](#trusted-https). |
| `TIK_TLS_NAMES` | – | Extra names for the generated certificate, comma-separated: private IPv4 addresses or names under `tikontrol`, `localhost`, `.lan`, `.home.arpa`, `.internal`, `.local`. Behind a dst-nat, add the address you open in the browser (for example `192.168.1.1`). |
| `TIK_HTTP` | – | Plain HTTP, loopback addresses only (a local reverse proxy, development). |
| `TIK_POLL` | `30` | Seconds between router snapshots (5–3600). Lower costs router CPU. |
| `TIK_DEBUG_API` | – | Log each RouterOS API command path and the reply types it got (no values). For diagnosing a router that behaves differently. |
| `TIK_DEMO` | – | Path to a snapshot JSON (for example `src/ui/src/lib/sample.json`) to serve instead of a router. |

## First start

1. Start tikontrol and read its log. It prints the HTTPS certificate fingerprint and a one-time **setup token**.
2. Open `https://<host>:8443`, check the certificate fingerprint, and enter the setup token. It proves you run tikontrol and opens a temporary setup session that can only connect the router. A local backup account is optional; add one only if you want to get in while the router is unreachable.
3. The setup page shows the RouterOS commands that create a certificate for API-SSL, disable the plain API, and create a `tikontrol-read` group with only `read,api,test`. Run them in a RouterOS terminal and choose the passwords yourself.
4. Enter the router address, read its certificate, compare the fingerprint with `:put [/certificate/get tikontrol-api fingerprint]`, and connect. tikontrol logs in, reads the user's real group policies, and refuses read-only mode if the user has `write`, `sensitive`, `password`, `policy` or `reboot`.
5. Sign in with your MikroTik account. The first admin sign-in finishes setup: the setup token and the setup session stop working for good, also after a restart.

Once the router is connected, anyone with a MikroTik account on it can sign in to tikontrol with that account. tikontrol checks the password by logging in to the router and never stores it. A user whose group has `write` becomes an admin, everyone else a viewer. Local tikontrol accounts always win over a router account with the same name, and keep working when the router is unreachable.

Read and write has two options, both needing an explicit confirmation:

- **The admin who is logged in (recommended).** Changes run as the MikroTik account you signed in with. Its password stays in memory for your session only (30 minutes idle, 12 hours at most), so tikontrol stores no write password at all. Local tikontrol accounts can then only view.
- **A stored writer user.** A second, separate `tikontrol-write` user, stored encrypted, which lets local accounts make changes. Treat that password like the admin password.

## Trusted HTTPS

Without `TIK_TLS_CERT`/`TIK_TLS_KEY`, tikontrol works like mkcert:

- On first start it creates a **local CA** in `TIK_DATA` (`ca-cert.pem`, `ca-key.pem`, named `tikontrol local CA (xxxx)`, valid 10 years) and prints its SHA-256 in the log. The CA has name constraints: it can only vouch for `tikontrol`, `localhost`, `*.lan`, `*.home.arpa`, `*.internal`, `*.local` and private IPv4 addresses (10/8, 172.16/12, 192.168/16, 127/8).
- The CA signs the **server certificate** (`tls-cert.pem`): EKU serverAuth, valid 397 days (Apple rejects longer ones), for `tikontrol`, `tikontrol.lan`, `localhost`, `127.0.0.1`, the host's private IPv4 addresses (Linux) and `TIK_TLS_NAMES`. A daily check renews it, with the same CA, when fewer than 30 days are left or a name is missing (for example a new IP). The running server picks it up without a restart.
- An older self-signed `tls-cert.pem` from before is replaced on start.

**Once per device**, download the CA from Settings → System → *Trust this tikontrol* (or the link on the sign-in page, or `https://<host>:8443/api/v1/ca.crt`), check that its SHA-256 matches the log, and trust it:

- **macOS:** open the file, add it to the System keychain, then set *Always Trust* in Keychain Access, or `sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain tikontrol-ca.crt`.
- **iOS/iPadOS:** install the downloaded profile in Settings, then enable it in Settings → General → About → Certificate Trust Settings.
- **Windows:** Install Certificate → Local Machine → Trusted Root Certification Authorities.
- **Android:** Settings → Security → Install a certificate → CA certificate.
- **Firefox** keeps its own store: Settings → Certificates → Authorities → Import.

The risk: anyone with `ca-key.pem` could impersonate sites to devices that trust the CA. The name constraints keep that to local names and private IPs, but keep `TIK_DATA` private and remove the CA from your devices when you stop using tikontrol. To start over, delete `ca-*.pem` and `tls-*.pem` and restart (devices then need the new CA).

## Docker (home server)

```sh
docker build -f Containerfile -t tikontrol .
docker run -d --name tikontrol --restart unless-stopped \
  --read-only --cap-drop ALL --security-opt no-new-privileges \
  --memory 64m -p 8443:8443 -v tikontrol-data:/data tikontrol
docker logs tikontrol   # setup token and certificate fingerprint
```

## RouterOS container

Untested on router hardware so far (see [NEXT_STEPS.md](NEXT_STEPS.md)). Needs RouterOS 7.18 or later, the `container` package, device-mode containers enabled (a physical button press), and external storage. The hAP ac² is too small.

### Build the image

The Containerfile builds `linux/arm/v7`, `linux/arm64` and `linux/amd64` (static musl binary, `scratch`, UID 65532). Both build stages run natively on the build machine and cross-compile with `cargo-zigbuild`, so no QEMU is needed. Check your router's architecture with `/system/resource/print` (`architecture-name`: `arm64`, `arm` or `x86_64`).

RouterOS loads a `docker save` tarball of a single architecture, so build one per router:

```sh
# arm64 (hAP ax², ax³, be³, RB5009, CCR2xxx)
docker buildx build --platform linux/arm64 -f Containerfile -t tikontrol:arm64 --output type=docker .
docker save tikontrol:arm64 > tikontrol-arm64.tar

# arm/v7 (32-bit ARM: hAP ac³, RB4011, ...)
docker buildx build --platform linux/arm/v7 -f Containerfile -t tikontrol:armv7 --output type=docker .
docker save tikontrol:armv7 > tikontrol-armv7.tar

# amd64 (CHR, x86 routers, home servers)
docker buildx build --platform linux/amd64 -f Containerfile -t tikontrol:amd64 --output type=docker .
```

All three at once need a `docker-container` builder and an OCI archive (it holds every architecture, so it is for a registry or a home server, not for RouterOS):

```sh
docker buildx create --name tik --driver docker-container --use
docker buildx build --platform linux/arm/v7,linux/arm64,linux/amd64 -f Containerfile \
    -t tikontrol --output type=oci,dest=tikontrol-multiarch.tar .
```

Sizes measured on one build (they change with the code): the image is about 2.4 MB for arm/v7, 2.5 MB for arm64 and 3.0 MB for amd64, and the `docker save` tarball is about the same size (1.3 to 1.5 MB gzipped). The single-architecture builds were run on an arm64 Mac with Docker Desktop. The combined OCI build and running on a router have not been tested yet.

### Run it on the router

Upload the tarball to the router's USB disk (Files, drag and drop, or `scp`), then:

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

## AI assistants (MCP)

tikontrol can act as an [MCP](https://modelcontextprotocol.io) server so an AI assistant can read your network. It is off by default.

- **Enable:** Settings → Control plane → AI assistants (MCP), as an admin. tikontrol creates a random token and shows it once; only its SHA-256 is stored. Turning MCP off revokes the token, and "New token" rotates it.
- **Endpoint:** `POST https://<tikontrol>:8443/mcp`, JSON-RPC 2.0 over the Streamable HTTP transport (protocol `2025-06-18`, plain JSON answers, no SSE stream). Send `Authorization: Bearer <token>`.
- **Who can reach it:** only peers with a loopback, private (RFC 1918), link-local or IPv6 unique-local address. Browser requests from another origin are refused, and five wrong tokens lock an address out for 15 minutes. The check uses the TCP peer address: behind a reverse proxy (`TIK_HTTP`) or Docker port publishing every request looks local, so don't expose those to the internet.
- **Tools:** `router_status`, `list_clients`, `get_client`, `wifi_overview`, `recent_events`, `insights`, `topology`. They read tikontrol's latest snapshot and history only: no router credentials, setup token or secrets are reachable. Every result carries a note that its values are untrusted device data; strings are stripped of control characters and cut at 200 characters.
- **Privacy mode** (on by default each time MCP is turned on): device names, SSIDs, the router name, MAC and IP addresses are replaced with pseudonyms (`device-1a2b3c4d`, `02:…` MACs, `ip-…`) derived with HMAC-SHA256 under a per-install key. They stay the same across calls, so the assistant can follow one device, but can't be turned back into the real values. Names shorter than three characters are not swapped inside free text such as log lines.
- **Proposed changes:** in read-write mode a `propose_change` tool records a proposal (title, description, RouterOS commands as text, at most 20 kept in memory). It never applies anything: proposals appear under the MCP settings, where an admin can review them through Safe apply or dismiss them. In read-only mode the tool is not offered and calls are refused.

Sample client configuration (for clients that take a JSON server list):

```json
{
  "mcpServers": {
    "tikontrol": {
      "type": "http",
      "url": "https://tikontrol.lan:8443/mcp",
      "headers": { "Authorization": "Bearer <your MCP token>" }
    }
  }
}
```

The client has to trust tikontrol's certificate (its own, or the generated one) to connect.

Session routes used by the web UI: `GET /api/v1/mcp` (enabled, privacy, endpoint), `POST /api/v1/mcp/enable` (returns the token once), `POST /api/v1/mcp/disable`, `POST /api/v1/mcp/privacy` `{"privacy": true|false}`, `GET /api/v1/mcp/proposals` and `DELETE /api/v1/mcp/proposals?id=<id>`. Changes need the admin role and the CSRF header.
