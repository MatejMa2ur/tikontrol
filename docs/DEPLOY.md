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
