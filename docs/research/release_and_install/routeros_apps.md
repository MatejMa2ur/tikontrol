# RouterOS Apps (`/app`, 7.21–7.26): custom YAML schema, install flows and third-party app stores

Research date: 2026-11-05. Stable is 7.25.1 (2026-11-02); testing is 7.26beta5.

**Source access caveat:** the session could not reach help.mikrotik.com, manual.mikrotik.com, forum.mikrotik.com or download.mikrotik.com (DNS/proxy block). The official Apps manual (`manual.mikrotik.com/docs/containers/apps/`, `.../cli-reference/app/`) and Amm0's forum manual were therefore seen only as search snippets. Primary evidence used instead:
(a) the **tikoci/restraml** project, which pulls `GET /rest/app` from a live CHR of each version and publishes it as `docs/<ver>/app.json`, plus a JSON Schema derived from it. These are MikroTik's own built-in app YAMLs, re-emitted by RouterOS.
(b) Changelog text mirrored in GitHub release notes of `elseif/MikroTikPatch`. That repo is a patched-RouterOS project, so it is not trustworthy for binaries, but its release notes reproduce MikroTik's changelog text.
(c) Community guides: GustavoGustik knowledge base (7.22/7.23beta era), the tikoci routeros-app-yaml skill, and Orb's and kernelorg's third-party stores.
Claims below are tagged **[official-snippet]**, **[live-CHR data]**, **[changelog mirror]** or **[community]**. Before relying on any of them, re-check against manual.mikrotik.com once that domain is allowed.

---

## 1. Exact YAML schema for custom apps

### Takeaway
A custom app is one compose-*like* YAML document (not docker-compose). Top level: `name`, `descr`, `page`, `category`, `icon`, `default-credentials`, `url-path`, `auto-update`, `supported-archs` / `compatible-boards` (7.25), `secrets`, `services`, `volumes`, `networks`, `configs`. A service needs only `image`. The YAML has **no memory-limit, network-mode or HTTPS fields**: those are `/app` or `/container` properties set after install. Placeholders are `[routerIP]`, `[networkRouterIP]`, `[accessIP]`, `[accessPort]`, `[accessPort2]`, `[accessProto]`, `[containerIP]`, `[containerInterface]`, `[secret:<name>]` and `[env:<VAR>]` / `[env:<service>:<VAR>]`.

### Cited Findings
**Version history**
- The `/app` menu first appeared in 7.21 ("added "/app" menu for simple containerized app installation", which needs the container package and container device-mode). Custom apps ("added support for custom apps") and "configurable app-store URL for custom apps" arrived in 7.22 (2026-03-09). — [GustavoGustik KB changelog extract](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) (docs/14-routeros-changelog.md) [community]; [tikoci routeros-app-yaml skill](https://github.com/tikoci/routeros-skills/blob/main/routeros-app-yaml/SKILL.md) says it was first seen in 7.22beta5.

**Top-level fields (schema derived from live RouterOS output)**
- The `latest` schema lists these top-level properties: `name`, `descr`, `page` (uri, shown as the app's `project-page`), `category`, `icon` (uri, "displayed on the WebFig landing page (when `/app/settings show-in-webfig=yes`, the default)"), `default-credentials` ("`username:password` format. Informative only; may be blank. Can contain runtime placeholders such as `[secret:name]`"), `url-path` ("Path appended to the /app URL, for example `/login`"), `credentials`, `option`, `auto-update` (bool), `supported-archs`, `compatible-boards`, `secrets`, `services` (at least one required), `volumes`, `networks`, `configs`. — [tikoci routeros-app-yaml-schema.latest.json](https://raw.githubusercontent.com/tikoci/restraml/main/docs/routeros-app-yaml-schema.latest.json) [live-CHR derived]
- `supported-archs` is an array of `arm`, `arm64`, `arm_v5`, `x86`. Devices whose arch does not match do not show the app (7.25+). `compatible-boards` is an array of board names with `*` wildcard (e.g. `RDS2216`), and non-matching boards also hide the app. — [schema](https://raw.githubusercontent.com/tikoci/restraml/main/docs/routeros-app-yaml-schema.latest.json); [restraml PR #117](https://github.com/tikoci/restraml/pull/117): every 7.25 built-in declares `supported-archs`, and a 7.25 CHR accepted arbitrary values on `/app/add`.
- Arch breakdown of the 102 built-in apps in 7.25 and 7.26beta3: 71 `[arm64, x86]`, 28 `[arm, arm64, x86]`, 3 `[arm_v5, arm, arm64, x86]`. — computed from [restraml docs/7.25/app.json](https://raw.githubusercontent.com/tikoci/restraml/main/docs/7.25/app.json) [live-CHR data]
- `category` is open in practice. The suggested values (the ones built-ins use) are: productivity, storage, networking, development, communication, file-management, search, video, media, media-management, home-automation, monitoring, database, automation, ai, messaging, radio, security, business. The official manual's examples use `network`, `system`, `utilities`, and 7.25 accepted arbitrary strings. — [PR #117](https://github.com/tikoci/restraml/pull/117); [schema](https://raw.githubusercontent.com/tikoci/restraml/main/docs/routeros-app-yaml-schema.latest.json)
- How often each top-level key appears in the 102 built-ins (7.25): category/descr/name/services/supported-archs 102, page 101, default-credentials 67, networks 13, configs 13, icon 12, url-path 8, volumes 4, secrets 1, auto-update 1. — computed from [7.25 app.json](https://raw.githubusercontent.com/tikoci/restraml/main/docs/7.25/app.json)
- A top-level `version:` key is not used. `deploy:` and `resources:` are not supported. — [tikoci skill](https://github.com/tikoci/routeros-skills/blob/main/routeros-app-yaml/SKILL.md) [community]

**Service fields**
- Only `image` is required. If the image has no registry prefix, the `/container/config` `registry-url` is assumed. Schema service keys: `image`, `container_name` ("name shown in the `/container` menu. Also used in file paths"), `hostname`, `entrypoint`, `command` (string or array), `ports`, `environment` (map or `KEY=val` list), `volumes` (strings), `configs` (`{source,target,mode}`), `restart` (`no|always|on-failure|unless-stopped`), `depends_on` (start order), `devices` (e.g. `/dev/ttyACM0:/dev/ttyACM0`), `user`, `security_opt`, `shm_size` (e.g. `512mb`), `stop_grace_period` (default 30 s), `ulimits`, `build`, `healthcheck` (`test`, `interval`, `timeout`, `retries`, `start_period`; `test` runs inside the container and passes on exit 0), `stdin_open`, `expose`, `secrets` (mounted at `/run/secrets/<name>`), `secret` (singular alias), `attach`. — [schema](https://raw.githubusercontent.com/tikoci/restraml/main/docs/routeros-app-yaml-schema.latest.json)
- Changes in recent versions: 7.24 "allow setting "working_dir" in app YAML". 7.25 "allow the privileged mode to be enabled per service in the container app YAML". 7.23 "allow passing stop signal from YAML" and "allow parsing DNS in YAML". — [7.24 notes](https://github.com/elseif/MikroTikPatch/releases/tag/7.24), [7.25 notes](https://github.com/elseif/MikroTikPatch/releases/tag/7.25), [7.23 notes](https://github.com/elseif/MikroTikPatch/releases/tag/7.23) [changelog mirror]. The exact key names (probably `working_dir`, `privileged`, `stop_signal`, `dns`) are not yet in the tikoci schema and none of the 7.25 built-ins uses `privileged`, so treat the spelling as unverified.
- `healthcheck` is supported. A 7.25 CHR probe turned `retries: '5'` into container `healthcheck-retries=5` and `test: [CMD, true]` into `healthcheck-cmd=CMD,true`, and the container became healthy. The built-in `mikrodash`, `inventree` and `onlyoffice-docs` apps use it. — [PR #117](https://github.com/tikoci/restraml/pull/117); [7.25 app.json](https://raw.githubusercontent.com/tikoci/restraml/main/docs/7.25/app.json). **Conflict:** the GustavoGustik KB (7.22 era) says `healthcheck` is "not supported in YAML" — [KB yaml reference](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase). 7.22 also added "health check for apps, which automatically rewrites the composed YAML" ([KB changelog](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase)), so the KB is probably out of date.
- `depends_on` only controls start order. It has no `condition: service_healthy`. — [GustavoGustik KB](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) [community]
- Service-key frequency across the 158 built-in services (7.25): image 158, volumes 136, environment 103, ports 102, restart 75, secrets 57, command 30, configs 15, healthcheck 7, user 4, hostname 4, entrypoint 4, devices 2, build 2, expose 2. — computed from [7.25 app.json](https://raw.githubusercontent.com/tikoci/restraml/main/docs/7.25/app.json)

**Ports syntax**
- Two string forms are accepted, one style per entry: older `[ip:]host:container[/tcp|/udp][:label]` and 7.23+ `[ip:]host:container[:label][:tcp|:udp]`. There is also a long form (`target`, `published`, `protocol`, `name`, `app_protocol`). Host and IP can be placeholders, e.g. `"[accessIP]:[accessPort]:80:web:tcp"`. — [tikoci skill](https://github.com/tikoci/routeros-skills/blob/main/routeros-app-yaml/SKILL.md); [schema `$comment`](https://raw.githubusercontent.com/tikoci/restraml/main/docs/routeros-app-yaml-schema.latest.json)
- Built-in port styles (7.25): `N:N:web` 92, `N:N` 7, `N:N:web-https` 3, `N:N/udp:dns` 3, `N:N:api` 3, and others. — computed from [7.25 app.json](https://raw.githubusercontent.com/tikoci/restraml/main/docs/7.25/app.json)
- The `web` label marks the port used for the UI URL. A built-in with an unlabeled `'3081:3081'` port shows `firewall-redirects=3081:3081:tcp:web`, which suggests the first port defaults to `web`. — [7.25 app.json, mikrodash entry](https://raw.githubusercontent.com/tikoci/restraml/main/docs/7.25/app.json); [KB](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) [community]
- The question's example `"8443:8443/tcp:web"` is valid old-style. The 7.23+ equivalent is `"8443:8443:web:tcp"`. — [schema](https://raw.githubusercontent.com/tikoci/restraml/main/docs/routeros-app-yaml-schema.latest.json)

**Volumes, configs, networks, secrets**
- Volumes are named only (`data:/data`, and `a/b:/path` is flattened to directory `a-b`). They are stored at `<disk>/apps/<app>/<vol>/`. There are no host bind mounts. — [KB yaml reference](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) [community]
- `configs.<name>.content` holds inline file text, and placeholders expand inside it. A service mounts it with `configs: [{source, target, mode}]`. — [tikoci skill](https://github.com/tikoci/routeros-skills/blob/main/routeros-app-yaml/SKILL.md)
- `networks: { default: { name: lan, external: true } }` puts the app directly on the LAN bridge instead of behind NAT. — [schema](https://raw.githubusercontent.com/tikoci/restraml/main/docs/routeros-app-yaml-schema.latest.json). In 7.25, 13 of 102 built-ins default to `lan` (caddy, chr, docker-with-*, home-assistant, homeassistant-thread, jellyfin, otbr, plex, syncthing, transmission, upsnap) and 89 default to `internal`. — computed from [7.25 app.json](https://raw.githubusercontent.com/tikoci/restraml/main/docs/7.25/app.json)
- Secrets work like this: a top-level `secrets: {name: }` declares them and RouterOS generates the values. They are referenced as `[secret:name]` in env or credentials and mounted at `/run/secrets/name`. Built-in example: `MYSQL_PASSWORD_FILE: /run/secrets/librenms__database_password` together with `DB_PASSWORD: '[secret:librenms__database_password]'`. — [7.25 app.json (librenms)](https://raw.githubusercontent.com/tikoci/restraml/main/docs/7.25/app.json). 7.24 changelog: "use randomly generated secrets in new apps" and "make secrets sensitive to avoid polluting configuration export". — [7.24 notes](https://github.com/elseif/MikroTikPatch/releases/tag/7.24)

**Placeholders and what they expand to**
- `variables-to-use-in-environment` on a 7.25 app lists `[containerIP]`, `[networkRouterIP]`, `[containerInterface]` (e.g. `veth-app-<truncated>`), `[routerIP]`, `[accessIP]`, `[accessPort]`, `[accessProto]`. — [7.25 app.json](https://raw.githubusercontent.com/tikoci/restraml/main/docs/7.25/app.json)
  - `[routerIP]` is the router's configured IP (`/app/settings assumed-router-ip`, set by `/app setup`). — [KB prerequisites](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) [community]
  - `[networkRouterIP]` was added in 7.25 ("add the networkRouterIP variable for container apps"). — [7.25 notes](https://github.com/elseif/MikroTikPatch/releases/tag/7.25). The built-in librenms uses it as `REAL_IP_FROM: '[networkRouterIP]'`, so it is most likely the router's address on the app's own network: the gateway of the `internal` bridge, or the router's IP on the LAN bridge. **Inference, unverified.**
  - `[accessIP]` / `[accessPort]` / `[accessProto]` depend on `use-https`. With HTTP they give a LAN IP, the host port and `http`. With HTTPS they give `<app>.<serial>.routingthecloud.com`, `443` and `https`. — [KB networking](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) [community]
  - `[accessPort2]` is a secondary host port. — [tikoci skill](https://github.com/tikoci/routeros-skills/blob/main/routeros-app-yaml/SKILL.md)
  - `[containerIP]` is the container's address (172.18.0.x on `internal`, or a DHCP lease on the LAN bridge). — [KB](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase)
- `[env:VAR]` and `[env:<service>:VAR]` appear in built-in `default-credentials`, e.g. grafana `'[env:GF_SECURITY_ADMIN_USER]:[env:GF_SECURITY_ADMIN_PASSWORD]'` and nextcloud `'[env:server:NEXTCLOUD_ADMIN_USER]:...'`. — [7.25 app.json](https://raw.githubusercontent.com/tikoci/restraml/main/docs/7.25/app.json)
- 7.25: "show an error when an app variable cannot be resolved". — [7.25 notes](https://github.com/elseif/MikroTikPatch/releases/tag/7.25)
- Strict schema requires env names to match `^[A-Z_][A-Z0-9_]*$`. The editor variant relaxes this, and RouterOS itself accepts mixed case (pihole uses `FTLCONF_webserver_api_password`). — [schema](https://raw.githubusercontent.com/tikoci/restraml/main/docs/routeros-app-yaml-schema.latest.json); [7.25 app.json](https://raw.githubusercontent.com/tikoci/restraml/main/docs/7.25/app.json)

**Re-emitted YAML (7.25+)**
- For store-loaded apps and built-ins, RouterOS re-emits the YAML. It sorts keys and quotes scalars that are not letters-only (`retries: 5` becomes `'5'`, and `"true"` becomes bare `true`). Apps added with `/app/add yaml=` come back verbatim. The parser is string-typed. — [PR #117](https://github.com/tikoci/restraml/pull/117)

**Minimal custom app (constructed from the schema; all values fictional)**
```yaml
name: example-webapp
descr: Example web UI
page: https://example.org
category: networking
supported-archs: [arm64, x86]
default-credentials: admin:[secret:admin_pw]
url-path: /
secrets:
  admin_pw:
services:
  web:
    image: ghcr.io/example/webapp:1.2.3
    ports:
      - "8443:8443:web:tcp"
    environment:
      ROUTER_HOST: "[routerIP]"
      PUBLIC_URL: "[accessProto]://[accessIP]:[accessPort]"
    volumes:
      - data:/data
    restart: unless-stopped
volumes:
  data:
```

### Inferences
- A third-party web app should keep all state in a named volume. Rootfs is replaced on update (see §3).
- Pin image tags and declare `supported-archs: [arm64, x86]` so arm32 devices hide the app.
- Prefer the 7.23+ port style. For apps aimed at 7.22, use the old slash style, which later versions still accept.
- Container users don't go in YAML except `user:`. Non-root is the image's or the `user:` key's responsibility.
- No memory key exists in the YAML schema, so a `memory-max` (7.23+) must be applied on `/container` after install. That conflicts with the "never `/container/set` an app container" advice (§3) and is unresolved.

### Gaps
- The official field reference (manual.mikrotik.com "YAML Field Reference") could not be fetched to confirm the 7.24–7.26 key names (`working_dir`, `privileged`, DNS, stop signal) and the exact meaning of `option` and `credentials`.
- Whether the YAML has a memory limit key (`mem_limit` etc.): there is no evidence that it does.
- Exact semantics of `[networkRouterIP]` and `[accessPort2]` were not confirmed from an official source.

---

## 2. Install flows: `/app/add`, URLs, and third-party app stores

### Takeaway
There are three supported paths. (1) Inline: `/app/add yaml="..."` or `yaml=[/file/get f contents]`. (2) By URL: the `yaml-url` property, used via REST `PUT /rest/app`. (3) Third-party catalog: `/app/settings set app-store-urls=<url>`, which points at a YAML **array** of app definitions. The catalog's apps then appear in WinBox/WebFig next to the built-ins and install with one click. This is a real, working mechanism that Orb and other projects use in production.

### Cited Findings
- Manual flow from the official docs: create a blank app with `/app/add network=lan` (default name "app"), paste YAML with `/app/edit app yaml`, or load from a file with `/app/add yaml=[/file/get <name> contents]`. — [MikroTik Apps manual, search snippet](https://manual.mikrotik.com/docs/containers/apps/) [official-snippet]
- Inline one-liner: `/app add disabled=no network=bridge1 yaml="..."` after `/system/device-mode/update container=yes` and `/app setup`. — [GustavoGustik KB README](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) [community]
- REST: `GET /rest/app` lists built-in and custom apps, each with a raw `yaml` field. `PUT /rest/app` with `{"yaml-url": "https://.../my-app.tikapp.yaml"}` adds a custom app from a URL. Without the `container` package, `/app` returns 404. — [tikoci skill](https://github.com/tikoci/routeros-skills/blob/main/routeros-app-yaml/SKILL.md) [community]
- App store: the official manual's CLI form is `/app/settings set app-store-urls="<custom_url>"`. The URL must return a YAML array with one app per element. — [MikroTik Apps manual, search snippet](https://help.mikrotik.com/docs/spaces/ROS/pages/343244823/Apps) [official-snippet]. One community source writes `/app set app-store-urls=` ([tikoci tikapp manual](https://tikoci.github.io/restraml/tikapp-manual.html)). The official and other sources say `/app/settings`.
- Store format: a plain YAML list (`- name: a ...`, `- name: b ...`) with no `---` separators. The conventional extension is `.tikappstore.yaml`, and single apps use `.tikapp.yaml`. A schema exists at `https://tikoci.github.io/restraml/routeros-app-yaml-store-schema.latest.json`. `/app add` does not accept the array form. — [tikoci skill](https://github.com/tikoci/routeros-skills/blob/main/routeros-app-yaml/SKILL.md); [tikapp manual](https://tikoci.github.io/restraml/tikapp-manual.html); [KB app-store doc](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase)
- Multiple stores are comma-separated: `app-store-urls=https://a/apps.yaml,https://b/apps.yaml`. The server should send `Content-Type: text/yaml`. GitLab Pages' extra charset parameters reportedly break parsing, while GitHub Pages and Cloudflare Pages work. — [KB docs/10-app-store.md](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) [community, unverified]. GitHub raw (`text/plain`) is used successfully by kernelorg (below), so the MIME claim is doubtful.
- `/app print` flags: `c` = custom (added via `/app add yaml=`) and `s` = from app store. In 7.22.1, `c` was lost on reboot, which made custom apps unremovable ("cannot remove builtin app"). — [KB operations](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) [community, 7.22.1-specific]. App objects in 7.25 have `custom`, `from-app-store` and `app-store-url` properties. — [7.25 app.json](https://raw.githubusercontent.com/tikoci/restraml/main/docs/7.25/app.json)
- Changelog: 7.22 "added configurable app-store URL for custom apps". 7.23 "fixed store issue when adding a custom app", "allow updating YAML for existing custom app, forces cleanup", "allow updating name parameter from YAML for custom apps", "fixed YAML not exported for custom apps". — [KB changelog](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase); [7.23 notes](https://github.com/elseif/MikroTikPatch/releases/tag/7.23)

**Examples in the wild**
- **Orb** (commercial network sensor) ships an official store at `https://orb.net/docs/scripts/mikrotik/orb-app-store.yml`, installed with `/app/settings/set app-store-urls=...`. Orb's docs say the catalog refreshes at boot. — [search snippet of orb.net docs](https://orb.net/docs/setup-sensor/mikrotik). A later Orb PR (merged 2026-10-22, tested on 7.24.5) says the catalog "usually loads within seconds", or after WinBox **Apps > Settings > Apply** or a reboot. It loaded about 10 s after setting the URL. — [orbforge/orb-docs PR #134](https://github.com/orbforge/orb-docs/pull/134)
- **kernelorg/mikrotik-apps**: store URL `https://raw.githubusercontent.com/kernelorg/mikrotik-apps/main/apps.yaml`. One source file per app (`apps/<app>.yaml`) is merged by `scripts/build.py`, because RouterOS has no includes. `{{version}}` is substituted with the release tag, and the build checks that the image tag and archs exist in the registry. — [kernelorg/mikrotik-apps](https://github.com/kernelorg/mikrotik-apps)
- **aligorov/routeros-aligorov**: a personal container app store hosted on GitHub Pages. — [GitHub](https://github.com/aligorov/routeros-aligorov) (not inspected)
- **GustavoGustik/mikrotik-custom-apps-knowledgebase**: example `*.app.yml` manifests (mosquitto, zigbee2mqtt, nodered, unifi, ...). — [GitHub](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase)
- **Built-in app YAMLs** per version (7.22, 7.23.8, 7.24.5, 7.25, 7.26beta1/3) are at `https://raw.githubusercontent.com/tikoci/restraml/main/docs/<ver>/app.json`. A browser YAML editor with a "Copy /app/add" button is at tikoci.github.io/restraml/tikapp.html. — [PR #117](https://github.com/tikoci/restraml/pull/117); [tikapp manual](https://tikoci.github.io/restraml/tikapp-manual.html)
- One store maintainer plans to version their store so users can pin the URL for security. — [search snippet, forum](https://forum.mikrotik.com/t/amm0s-manual-for-custom-app-containers-7-22beta/268036) [unverified]

### Inferences
- One-command install of a third-party app is `/app/add yaml-url=https://.../app.tikapp.yaml`. The CLI form is inferred from the REST `yaml-url` field; on the CLI, `yaml=[/tool/fetch ... as-value output=user]` is an alternative. One-click is "add our store URL once, then click Install in WinBox/WebFig Apps".
- A store URL is a supply-chain trust root. Whoever controls that YAML can change the image or env, and so effectively gets code execution on the router's container host. Serve it over HTTPS from a versioned, pinned path.

### Gaps
- The refresh interval of `app-store-urls` (boot only, or periodic) is not stated officially. The two Orb statements are consistent only if setting or applying the URL also triggers a fetch.
- Whether the CLI `/app/add` accepts `yaml-url=` directly (rather than only REST) was not confirmed from official docs.
- No official MikroTik public store index or signing mechanism was found. There is no evidence of signature verification on store YAML.

---

## 3. Networking, storage, memory, HTTPS/UI URL and updates

### Takeaway
Each app gets one VETH, shared by all its services. In the default `internal` mode (172.18.0.0/24 bridge), RouterOS auto-creates dst-nat rules from `routerIP:hostPort`. In `lan` mode the VETH joins the LAN bridge and takes DHCP. With `use-https=yes` (the default) RouterOS also creates a `/ip/reverse-proxy` SNI rule with a Let's Encrypt wildcard cert under `*.<serial>.routingthecloud.com`. Storage goes on an external ext4/btrfs/xfs disk chosen in `/app setup`. Updates replace the rootfs, and old layers are not pruned.

### Cited Findings
**Networking**
- App properties (7.25): `network` (`default` / `internal` / bridge name), `default-network` (from YAML: `internal` or `lan`), `network-pvid`, `network-outgoing-access`, `use-https`, `ui-url`, `firewall-redirects`, `interface`, `cmds`, `container-command-lines`, `required-mounts`, `extra-mounts`, `required-hw-devices`, `devices`, `secrets`, `environment`, `variables-to-use-in-environment`, `custom`, `from-app-store`, `app-store-url`. In the CHR capture, all 102 built-ins showed `use-https=true` and `network-outgoing-access=true`. — [7.25 app.json](https://raw.githubusercontent.com/tikoci/restraml/main/docs/7.25/app.json)
- Internal mode creates an isolated `internal` bridge 172.18.0.0/24 (gateway .1, apps from .2). Ports become `/ip/firewall/nat` dst-nat rules (`dst-address=<routerIP> dst-port=<host> to-addresses=<containerIP> to-ports=<container>`), outbound traffic is masqueraded, and the `internal` bridge is added to the LAN interface list. In LAN/bridge mode, the VETH is a bridge port with `dhcp=yes`, no NAT is created, the host-port number is ignored for access, and the container is reached at `containerIP:containerPort`. — [KB networking / auto-managed infra](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) [community, 7.22-era]
- All services in one app share a single VETH/IP and are reachable via `localhost`. RouterOS fills each container's `hosts=` with every service name pointing at the shared IP. Container names are `app-<app>-<service>`. — [KB](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) [community]
- Change notes: 7.22 "allow configuring bridge port pvid for app". 7.21 "allow app network to be any bridge interface". 7.23 "apps now check for port availability, apps will not start on "internal" if app masks existing service". 7.24 "reserve the app's VETH IP when stopped", "fixed apps not updating firewall redirects when changed in YAML", "show CHR's address instead of the container's". — [KB changelog](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase); [7.23](https://github.com/elseif/MikroTikPatch/releases/tag/7.23); [7.24](https://github.com/elseif/MikroTikPatch/releases/tag/7.24)
- `network-outgoing-access` (yes/no) stops the app's containers from initiating outbound connections. It is listed in **both** the 7.23 notes ("added "network-outgoing-access=yes/no" setting to prevent containers from initiating outbound traffic") and the 7.24 notes ("added "network-outgoing-access" parameter which does not allow app to make outgoing connections"). — [7.23](https://github.com/elseif/MikroTikPatch/releases/tag/7.23); [7.24](https://github.com/elseif/MikroTikPatch/releases/tag/7.24). This version conflict is unresolved. Assume 7.24+ to be safe.

**HTTPS, UI URL, WebFig**
- `use-https=yes` (default) uses `/ip/cloud` DDNS and an ACME wildcard cert `*.<serial>.routingthecloud.com` (DNS-01 handled by MikroTik's cloud). It also creates `/ip/reverse-proxy` SNI rules on port 443 that forward plain HTTP to `containerIP:webPort`. The `www-ssl` service must be moved off 443, otherwise the reverse proxy reports "bind error: cannot bind to port 443". `use-https=no` keeps `[accessIP]` as the LAN IP. — [KB networking / prerequisites](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) [community]
- 7.22: "added reverse-proxy", "show app URL only when it is running", "show DNS URL for app only if it has a reverse-proxy", "changed ui-url parameter for Smokeping and Nextcloud". 7.23: "added IPv6 and VRF support for reverse-proxy", "fixed showing ui-url for apps", "check if certificate already exists before creating a new one". 7.26beta4: "add remove-certificate command for certificates not used by any app". — [KB changelog](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase); [7.23](https://github.com/elseif/MikroTikPatch/releases/tag/7.23); [releases list](https://github.com/elseif/MikroTikPatch/releases)
- `icon` is displayed on the WebFig landing page when `/app/settings show-in-webfig=yes` (the default). — [schema](https://raw.githubusercontent.com/tikoci/restraml/main/docs/routeros-app-yaml-schema.latest.json)

**Storage**
- `/app setup` is a wizard that sets the disk, router IP and LAN bridge. `/app/settings print` shows `disk`, `assumed-router-ip`, `lan-bridge`, `auto-update`, `certificate-status`, `certificate` and also `registry-mirrors`. — [KB prerequisites](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) [community, 7.22]
- Layout: `<disk>/apps/layers` (shared), `<disk>/apps/<app>/<service>_root`, and `<disk>/apps/<app>/<volume>/`. Supported filesystems are ext4 or btrfs, plus XFS since 7.23 ("allow apps on XFS file systems"). — [KB](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase); [7.23](https://github.com/elseif/MikroTikPatch/releases/tag/7.23)
- File-backed small disks work: Orb uses a 64 MB app disk and recommends creating disks with `smb-sharing=no media-sharing=no`, because otherwise RouterOS shares new disks over guest SMB/media. — [Orb PR #134](https://github.com/orbforge/orb-docs/pull/134)
- Swap: 7.22 "enable swap on all devices that use apps". 7.23 swap is created on the mount-point's disk. 7.24 adds a "swap-max" global/per-container limit. 7.25 "require the container device-mode to enable swap". — [KB changelog](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase); [7.23](https://github.com/elseif/MikroTikPatch/releases/tag/7.23); [7.24](https://github.com/elseif/MikroTikPatch/releases/tag/7.24); [7.25](https://github.com/elseif/MikroTikPatch/releases/tag/7.25)

**Memory**
- Memory limits are container-level: 7.20 "added per-container memory limiting"; 7.23 "allow setting memory-max globally and per container" and "detect and show containers killed by out-of-memory killer". `/container/config` has `memory-high`. — [KB changelog/prereqs](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase); [7.23](https://github.com/elseif/MikroTikPatch/releases/tag/7.23)
- The YAML has no `deploy`/`resources`/memory key. — [tikoci skill](https://github.com/tikoci/routeros-skills/blob/main/routeros-app-yaml/SKILL.md); [schema](https://raw.githubusercontent.com/tikoci/restraml/main/docs/routeros-app-yaml-schema.latest.json)

**Updates and lifecycle**
- Command names. Orb (tested on 7.24.5) uses `/app/update` and the WinBox **Update** button. The GustavoGustik KB (7.22) writes `/app/upgrade myapp`. Other commands seen: `/app/restart` (added in 7.23), `/app/cleanup` (destroys volumes, prompts `[y/N]`), `/app/reset` (7.24: "allow "reset" even if disk not configured"), `/app enable|disable|remove`, `/app setup`. — [Orb PR #134](https://github.com/orbforge/orb-docs/pull/134); [KB operations](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase); [7.23](https://github.com/elseif/MikroTikPatch/releases/tag/7.23); [7.24](https://github.com/elseif/MikroTikPatch/releases/tag/7.24). **The update/upgrade naming conflicts.** Orb's is the more recent source.
- Updating an app replaces the container rootfs (Update button, `/app/update`, or auto-update at boot), so state outside volumes is lost. RouterOS "never deletes an app's previous image layers on update" (about 20 MB per update, which eventually causes "extract layer failed"). — [Orb PR #134](https://github.com/orbforge/orb-docs/pull/134)
- `auto-update` (YAML key, per-app `/app/set x auto-update=yes`, or global `/app/settings auto-update`) pulls and restarts on boot according to the tikoci skill, or "periodically" according to the KB. — [tikoci skill](https://github.com/tikoci/routeros-skills/blob/main/routeros-app-yaml/SKILL.md); [KB](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase). Orb says the auto update happens "on boot". — [Orb PR #134](https://github.com/orbforge/orb-docs/pull/134)
- Env vars can be changed without re-adding the app: `/app/set myapp environment=<service>:<KEY>=<value>`. — [KB operations](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) [community]. Orb notes that `/app/cleanup` resets the app's environment. — [Orb PR #134](https://github.com/orbforge/orb-docs/pull/134)
- Do not `/container/set` or `/container/remove` app-managed containers, because that desynchronises `/app`. Only stop, start and shell are safe. — [KB auto-managed infra](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) [community]
- Registry: `/app/settings set registry-mirrors=...` helps with Docker Hub rate limits. — [KB prerequisites](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) [community]

### Inferences
- For a self-updating third-party app, either use immutable tags plus a store update (users click Update), or document a cleanup procedure. Never keep secrets or identity in rootfs.
- An HTTPS-capable web app can rely on the reverse proxy for TLS (container speaks HTTP) or serve its own TLS with `use-https=no`.

### Gaps
- No official confirmation of `/app/update` vs `/app/upgrade`, or of whether there is a `check-for-updates` command.
- No authoritative statement on how often auto-update runs.

---

## 4. Can an app container reach the router's own API (8729)?

### Takeaway
Yes. Containers can reach the router at `[routerIP]` (or `[networkRouterIP]`, 7.25+), and MikroTik's own built-in `mikrodash` app does exactly this. Note that it uses **plain API 8728 with `ROUTER_TLS: false` and user `admin`**. Nothing in `/app` provisions a RouterOS user or certificate. The app developer has to give the user a least-privilege account and TLS 8729 setup.

### Cited Findings
- Built-in `mikrodash` ("The Ultimate MikroTik RouterOS Dashboard", image `docker.io/mikrotik/mikrodash`, page github.com/SecOps-7/MikroDash) sets `ROUTER_HOST: '[routerIP]'`, `ROUTER_PORT: '8728'`, `ROUTER_TLS: false`, `ROUTER_USER: admin`, and leaves `ROUTER_PASS` empty. Its network is `internal`, it uses port `3081:3081`, and its healthcheck is `wget -qO- http://127.0.0.1:3081/healthz`. It was added in 7.23. — [7.25 app.json](https://raw.githubusercontent.com/tikoci/restraml/main/docs/7.25/app.json); [7.23 notes](https://github.com/elseif/MikroTikPatch/releases/tag/7.23)
- In internal mode the `internal` bridge is a member of the LAN interface list, so default-firewall "accept from LAN" rules apply to container traffic toward the router. — [KB networking](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) [community]
- `network-outgoing-access=no` blocks container-initiated outbound connections (7.23 or 7.24). — [7.23](https://github.com/elseif/MikroTikPatch/releases/tag/7.23); [7.24](https://github.com/elseif/MikroTikPatch/releases/tag/7.24)

### Inferences
- A management app such as tikontrol can point at `[routerIP]:8729` with `api-ssl`. The api-ssl service's `address=` allow-list must include the container subnet (internal: 172.18.0.0/24) or the container's LAN IP, and the certificate must be valid for the address used. Verify the cert by pinning it, because `[routerIP]` is an IP.
- Whether `network-outgoing-access=no` also blocks connections to the router itself (input chain vs forward chain) is unknown, so test it before relying on it as a lockdown.
- Use `[secret:...]` for any generated app-internal secret. Router credentials must still be entered by the user and stored encrypted in the app. `/app` has no secret-injection path from the router's user database.

### Gaps
- No official guidance found on app-to-router API access, on firewall rules that `/app` creates for input, or on whether `network-outgoing-access` covers the router itself.

---

## 5. Requirements

### Takeaway
You need RouterOS 7.22+ for custom apps (7.25+ for `supported-archs` filtering), the `container` package, `device-mode container=yes` (physical confirmation), an external or file-backed disk, and arm64 or x86. Some built-ins also list `arm` and `arm_v5`.

### Cited Findings
- 7.21 `/app` requires the "container" package and enabled "container" device-mode. — [KB changelog](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase)
- `/system/device-mode/update container=yes` needs a reset-button press within the timeout, or a cold reboot (CHR: reboot). — [KB prerequisites](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase) [community]
- Third-party stores state the requirements as "RouterOS 7.22 or newer, container package, container mode, arm64 or x86". — [kernelorg/mikrotik-apps](https://github.com/kernelorg/mikrotik-apps)
- Arch values used by built-ins are `arm`, `arm64`, `arm_v5`, `x86`. 71 of 102 are arm64/x86 only. — computed from [7.25 app.json](https://raw.githubusercontent.com/tikoci/restraml/main/docs/7.25/app.json)
- Storage needs an external disk (NVMe, USB, SD) with ext4/btrfs (XFS since 7.23); internal 128 MB flash is insufficient (KB). A 64 MB file-backed disk was enough for a single ~21 MB image (Orb). — [KB](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase); [Orb PR #134](https://github.com/orbforge/orb-docs/pull/134)
- 7.24: "remove reverse-proxy for SMIPS". — [7.24](https://github.com/elseif/MikroTikPatch/releases/tag/7.24)

### Gaps
- No official minimum RAM. The KB's figures (1 GB minimum, 4 GB+ recommended) are community opinion.

---

## 6. WinBox 4 / WebFig UI and one-click install

### Takeaway
WinBox and WebFig have an **Apps** catalog where built-in and store apps install with a click. Store apps refresh via **Apps > Settings > Apply**, and app icons and links appear on the WebFig landing page when `show-in-webfig=yes`.

### Cited Findings
- WinBox **Apps > Settings > Apply** forces a catalog reload. Apps from `app-store-urls` appear in `/app` and WebFig. — [Orb PR #134](https://github.com/orbforge/orb-docs/pull/134); [kernelorg](https://github.com/kernelorg/mikrotik-apps)
- 7.23: "allow filtering by installed apps", "allow picking app category from drop-down". WinBox file picker for container mount sources. 7.25: "winbox - add the container save option". — [7.23](https://github.com/elseif/MikroTikPatch/releases/tag/7.23); [7.25](https://github.com/elseif/MikroTikPatch/releases/tag/7.25)
- Icons show on the WebFig landing page (`show-in-webfig=yes`, the default). — [schema](https://raw.githubusercontent.com/tikoci/restraml/main/docs/routeros-app-yaml-schema.latest.json)
- Built-in apps are read-only (`/app set` cannot change their YAML). To customise one, copy its YAML and `/app add yaml=` it as a custom app. — [tikapp manual](https://tikoci.github.io/restraml/tikapp-manual.html)

### Gaps
- No screenshots or official description of the WinBox 4 Apps UI fields (install dialog, disk/network pickers) were available.

---

## 7. Changelog lines about `/app` (7.21–7.26)

### Takeaway
The pace is fast. 7.22 made custom apps and stores usable, 7.23 added in-place YAML updates, restart and port checks, 7.24 added secrets, working_dir and stable VETH IPs, 7.25 added `networkRouterIP`, per-service privileged mode and arch filtering, and 7.26beta added certificate cleanup.

### Cited Findings
- **7.21 (2026-01-12):** "/app menu for simple containerized app installation"; "shows app URL and "running" status only when port is open"; "allow app network to be any bridge interface"; veth `dhcp` setting. — [KB changelog](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase)
- **7.22 (2026-03-09):** added support for custom apps; configurable app-store URL; health check for apps (rewrites composed YAML); bridge port pvid; swap enabled on app devices; fixed /app/export; fixed port collisions; "show DNS URL for app only if it has a reverse-proxy"; `ip` added reverse-proxy. **7.22.1:** "defconf – fixed architecture detection for apps". — [KB changelog](https://github.com/GustavoGustik/mikrotik-custom-apps-knowledgebase)
- **7.23 (2026-05-25):** network-outgoing-access; restart command; XFS; CLI command-line parameter; stop signal; DNS in YAML; update YAML of custom app (forces cleanup); rename from YAML; port-availability check; automatic device passing; fixed YAML export for custom apps; new apps including mikrodash and docker-with-portainer/dockge/komodo; container restart-policy and memory-max; OOM detection; reverse-proxy IPv6/VRF and SNI logging. — [7.23 notes](https://github.com/elseif/MikroTikPatch/releases/tag/7.23)
- **7.24 (2026-08-14):** network-outgoing-access (again); `working_dir` in YAML; fixed firewall redirects not updating; reserve VETH IP when stopped; secrets sensitive in export; random secrets in new apps; "allow reset even if disk not configured"; container privileged mode, `save`, `swap-max`; "do not print environment variables in log on container startup". — [7.24 notes](https://github.com/elseif/MikroTikPatch/releases/tag/7.24)
- **7.25 (2026-10-22):** "add the networkRouterIP variable for container apps"; "allow the privileged mode to be enabled per service in the container app YAML"; "fix CHR apps getting stuck on "starting""; "show an error when an app variable cannot be resolved"; container `Ctrl-\` shell escape; "require the container device-mode to enable swap". **7.25.1 (2026-11-02):** no app or container lines. — [7.25](https://github.com/elseif/MikroTikPatch/releases/tag/7.25); [7.25.1](https://github.com/elseif/MikroTikPatch/releases/tag/7.25.1)
- **7.26beta4:** "app - add remove-certificate command for certificates not used by any app". **7.26beta5 (2026-11-02):** no app lines. — [releases list](https://github.com/elseif/MikroTikPatch/releases)

### Gaps
- Official `download.mikrotik.com/routeros/<ver>/CHANGELOG` was not reachable, so all lines come from a mirror or a community extract. 7.26beta1–3 app lines were not retrieved.
