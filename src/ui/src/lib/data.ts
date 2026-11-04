// Normalises a RouterOS snapshot (scripts/snapshot.py) into the UI model.
// ponytail: mockup data source. The Rust backend will serve this same shape from the live binary API.

import { simulateBe3 } from './be3'
import { bandLabel, detectHardware, normalizeWifi, standardOf } from './hardware'

const files = import.meta.glob('./*.json', { eager: true, import: 'default' }) as Record<string, any>
export const simulated = new URLSearchParams(location.search).get('device') === 'be3'
export const raw = normalizeWifi(simulated ? simulateBe3(files['./sample.json']) : files['./snapshot.json'] ?? files['./sample.json'])
export const isSample = !files['./snapshot.json']
export const hw = detectHardware(raw)

export type Kind = 'phone' | 'computer' | 'server' | 'vm' | 'speaker' | 'iot' | 'unknown'
export type Client = {
  mac: string
  ip: string
  name: string
  hostname?: string
  vendor?: string
  kind: Kind
  wired: boolean
  port?: string // ether2… or wlan1/wlan2
  band?: '2.4 GHz' | '5 GHz' | '6 GHz'
  ssid?: string
  signal?: number
  txRate?: string
  uptimeS?: number
  bytesDown: number
  bytesUp: number
  fixedIp: boolean
  online: boolean
  privateMac: boolean
  roams: number // band switches seen in the log
  ccq?: number // link quality %, legacy wireless only
  standard?: string // 'WiFi 4' | 'WiFi 5' (heuristic)
}
export type Port = { name: string; up: boolean; wan: boolean; clients: string[] }
export type Event = { time: string; cat: 'Clients' | 'WiFi' | 'System' | 'Security' | 'Admin'; level: 'info' | 'warning' | 'error'; text: string }
export type Insight = { level: 'ok' | 'warn' | 'bad'; title: string; detail: string; ros: string }

// ---------- helpers ----------
export function parseUptime(v: unknown): number | undefined {
  if (typeof v !== 'string' || !v) return
  // :serialize renders durations > 1 day as a 1970 timestamp: "1970-01-21 00:28:01" = 20 d 00:28:01
  const ts = v.match(/^1970-01-(\d\d) (\d\d):(\d\d):(\d\d)$/)
  if (ts) return (+ts[1] - 1) * 86400 + +ts[2] * 3600 + +ts[3] * 60 + +ts[4]
  const hms = v.match(/^(\d+):(\d\d):(\d\d)$/)
  if (hms) return +hms[1] * 3600 + +hms[2] * 60 + +hms[3]
  let s = 0
  for (const [, n, u] of v.matchAll(/(\d+)([wdhms])/g)) s += +n * { w: 604800, d: 86400, h: 3600, m: 60, s: 1 }[u as 'w']!
  return s || undefined
}
export const dur = (s?: number) => {
  if (s == null) return '—'
  const d = Math.floor(s / 86400), h = Math.floor((s % 86400) / 3600), m = Math.floor((s % 3600) / 60)
  return d ? `${d}d ${h}h` : h ? `${h}h ${m}m` : `${m}m`
}
export const bytes = (n: number) => {
  const u = ['B', 'KB', 'MB', 'GB', 'TB']
  let i = 0
  while (n >= 1000 && i < u.length - 1) { n /= 1000; i++ }
  return `${n < 10 && i ? n.toFixed(1) : Math.round(n)} ${u[i]}`
}
export const bps = (n: number) => {
  const u = ['bps', 'Kbps', 'Mbps', 'Gbps']
  let i = 0
  while (n >= 1000 && i < u.length - 1) { n /= 1000; i++ }
  return `${n < 10 && i ? n.toFixed(1) : Math.round(n)} ${u[i]}`
}
// Locally administered bit: phones/laptops with "private Wi-Fi address"
const isPrivateMac = (mac: string) => (parseInt(mac.slice(0, 2), 16) & 2) === 2
// ponytail: tiny OUI table; ship the full IEEE OUI list (~1 MB, gzipped ~300 KB) in Phase 1
const OUI: Record<string, string> = { '38:42:0B': 'Sonos', '52:54:00': 'QEMU / KVM' }

export const subtitle = (c: Client, shown: string) =>
  c.vendor ?? (c.privateMac ? 'Private Wi-Fi address' : c.hostname && c.hostname !== shown ? c.hostname : 'Unknown vendor')

export function signalQuality(dbm?: number) {
  if (dbm == null) return { label: 'Wired', level: 'ok' as const }
  if (dbm >= -55) return { label: 'Excellent', level: 'ok' as const }
  if (dbm >= -67) return { label: 'Good', level: 'ok' as const }
  if (dbm >= -75) return { label: 'Fair', level: 'warn' as const }
  return { label: 'Poor', level: 'bad' as const }
}

function guessKind(host = '', vendor = '', priv: boolean, wired: boolean): Kind {
  const h = `${host} ${vendor}`.toLowerCase()
  if (/sonos|speaker|homepod|echo/.test(h)) return 'speaker'
  if (/qemu|kvm|vm\b|arch|ubuntu|debian/.test(h)) return 'vm'
  if (/zima|nas|server|truenas|unraid|synology/.test(h)) return 'server'
  if (/pc|desktop|laptop|macbook|imac|win/.test(h)) return 'computer'
  if (/gw|hub|bridge|hue|ikea|tasmota|esp|shelly/.test(h)) return 'iot'
  if (priv && !wired) return 'phone'
  return 'unknown'
}

// ---------- normalise ----------
const leases: any[] = raw.leases ?? []
const stations: any[] = raw.stations ?? []
const hosts: any[] = (raw.bridgeHosts ?? []).filter((h: any) => !h.local)
const arp: any[] = raw.arp ?? []
const wifi: any[] = raw.wifi ?? []
const log: any[] = Array.isArray(raw.log) ? raw.log : []
const lq: any[] = raw.linkQuality ?? []

const bandOf = (ifname?: string) => {
  const w = wifi.find((x) => x.name === ifname)
  return w ? bandLabel(w.band) as Client['band'] : undefined
}
const roamCount = (mac: string) => log.filter((l) => l.message?.startsWith(mac) && /connected/.test(l.message)).length

const macs = new Set<string>([...leases.map((l) => l['mac-address']), ...stations.map((s) => s['mac-address'])])
// ARP-only devices (static IP, no lease) still count as clients if reachable
for (const a of arp) if (a.status === 'reachable' && a.interface === 'bridge') macs.add(a['mac-address'])

export const clients: Client[] = [...macs].filter(Boolean).map((mac) => {
  const lease = leases.find((l) => l['mac-address'] === mac)
  const st = stations.find((s) => s['mac-address'] === mac)
  const host = hosts.find((h) => h['mac-address'] === mac)
  const a = arp.find((x) => x['mac-address'] === mac)
  const vendor = OUI[mac.slice(0, 8)]
  const priv = isPrivateMac(mac)
  const wired = !st
  const hostname = lease?.['host-name']
  // reg-table `bytes` is "rx,tx" from the AP's side: rx = client upload, tx = client download
  // (sanity check: a Sonos streaming music shows the big number second)
  const [up, down] = String(st?.bytes ?? '0,0').split(',').map(Number)
  return {
    mac,
    ip: lease?.address ?? st?.['last-ip'] ?? a?.address ?? '—',
    hostname,
    vendor,
    name: lease?.comment || hostname || (vendor ? `${vendor} ${mac.slice(-5)}` : priv ? `Private device ${mac.slice(-5)}` : `Device ${mac.slice(-5)}`),
    kind: guessKind(hostname || lease?.comment, vendor, priv, wired),
    wired,
    port: st?.interface ?? host?.['on-interface'],
    band: bandOf(st?.interface),
    ssid: wifi.find((w) => w.name === st?.interface)?.ssid,
    signal: st ? parseInt(st['signal-strength']) : undefined,
    txRate: st?.['tx-rate'],
    uptimeS: parseUptime(st?.uptime),
    bytesDown: down || 0,
    bytesUp: up || 0,
    fixedIp: lease ? lease.dynamic === false : false,
    online: !!st || !!host || a?.status === 'reachable' || lease?.status === 'bound',
    privateMac: priv,
    roams: roamCount(mac),
    ccq: lq.find((x) => x['mac-address'] === mac)?.['tx-ccq'],
    // ponytail: the radio's best standard, the client may negotiate less. The registration table's `band` has the real one.
    standard: st ? standardOf(wifi.find((w) => w.name === st.interface)?.band) : undefined,
  }
})

export const offline = [
  ...arp.filter((a) => a.status !== 'reachable' && !macs.has(a['mac-address'])).map((a) => ({ mac: a['mac-address'] as string, ip: a.address as string, seen: a.status as string })),
  ...leases.filter((l) => l.status !== 'bound').map((l) => ({ mac: l['mac-address'] as string, ip: l.address as string, seen: l['last-seen'] as string })),
].filter((o, i, all) => all.findIndex((x) => x.mac === o.mac) === i)

export const ports: Port[] = (raw.ethernet ?? []).map((e: any) => ({
  name: e.name,
  up: e.running && !e.disabled,
  wan: (raw.wan ?? []).some((w: any) => w.interface === e.name),
  clients: clients.filter((c) => c.port === e.name).map((c) => c.mac),
}))

const res = raw.resource ?? {}
const wan = (raw.wan ?? [])[0] ?? {}
export const router = {
  identity: typeof raw.identity === 'string' ? raw.identity : 'router',
  board: res['board-name'],
  model: raw.routerboard?.model,
  version: res.version,
  channel: raw.update?.channel,
  firmware: raw.routerboard?.['current-firmware'],
  firmwareUpgrade: raw.routerboard?.['upgrade-firmware'],
  cpuCount: res['cpu-count'],
  cpuLoad: res['cpu-load'] ?? 0,
  memTotal: res['total-memory'] ?? 1,
  memFree: res['free-memory'] ?? 0,
  hddTotal: res['total-hdd-space'] ?? 1,
  hddFree: res['free-hdd-space'] ?? 0,
  uptimeS: parseUptime(res.uptime),
}
export const internet = {
  wanIp: wan.address?.split('/')[0],
  gateway: wan.gateway,
  wanIf: wan.interface ?? 'ether1',
  publicIp: raw.cloud?.['public-address'],
  ddns: raw.cloud?.['dns-name'],
  bth: raw.cloud?.['back-to-home-vpn'] === 'enabled',
  rx: raw.wanTraffic?.['rx-bits-per-second'] ?? 0,
  tx: raw.wanTraffic?.['tx-bits-per-second'] ?? 0,
  gatewayIp: (raw.addresses ?? []).find((a: any) => a.interface === 'bridge')?.address?.split('/')[0],
  wanPort: (wan.interface ?? 'ether1').replace('ether', ''),
  bytesDown: (raw.wanStats ?? [])[0]?.['rx-byte'] ?? 0,
  bytesUp: (raw.wanStats ?? [])[0]?.['tx-byte'] ?? 0,
  latency: Object.entries(raw.latency ?? {}).map(([name, v]: [string, any]) => ({ name, ms: v.ms as number | null, loss: v.loss as number | null })),
  doubleNat: /^(10\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.)/.test(wan.address ?? ''),
}
export const ssids = [...new Set(wifi.map((w) => w.ssid))].map((ssid) => ({
  ssid,
  radios: wifi.filter((w) => w.ssid === ssid).map((w) => ({ name: w.name, band: bandOf(w.name)!, running: w.running })),
  clients: clients.filter((c) => c.ssid === ssid).length,
}))

const catOf = (topics: string[]): Event['cat'] =>
  topics.includes('wireless') ? 'WiFi' : topics.includes('dhcp') ? 'Clients' : topics.includes('account') ? 'Admin' : topics.includes('firewall') ? 'Security' : 'System'
const nameFor = (mac: string) => clients.find((c) => c.mac === mac)?.name ?? mac
export const events: Event[] = log
  .map((l) => ({
    time: l.time,
    cat: catOf(l.topics ?? []),
    level: (l.topics ?? []).includes('error') ? 'error' : (l.topics ?? []).includes('warning') ? 'warning' : 'info',
    // "AA:BB..@wlan2: connected, signal strength -84" -> "Alex's phone connected to 5 GHz (-84 dBm)"
    text: String(l.message).replace(/^([0-9A-F:]{17})@(w(?:lan|ifi)\d): (connected|disconnected)[^,]*(?:, registered to other interface)?, signal strength (-\d+)/,
      (_: string, mac: string, ifn: string, what: string, sig: string) => `${nameFor(mac)} ${what === 'connected' ? 'joined' : 'left'} ${bandOf(ifn) ?? ifn} (${sig} dBm)`)
      .replace(/assigned ([\d.]+) for ([0-9A-F:]{17})/, (_: string, ip: string, mac: string) => `gave ${ip} to ${nameFor(mac)}`),
  }) as Event)
  .reverse()

// ---------- insights: "status first" (PLAN §0.6 rule 4) ----------
const svc: any[] = raw.services ?? []
const insecure = svc.filter((s) => ['telnet', 'ftp', 'www'].includes(s.name) && !s.disabled).map((s) => s.name)
const memPct = (1 - router.memFree / router.memTotal) * 100
const flappers = clients.filter((c) => c.roams >= 4)
const adlist = (raw.adlist ?? [])[0]

export const insights: Insight[] = [
  insecure.length
    ? { level: 'bad', title: `${insecure.join(', ')} enabled`, detail: 'These services send your router password unencrypted. Disable them or limit them to the LAN.', ros: `/ip/service/disable ${insecure.join(',')}` }
    : { level: 'ok', title: 'Management services secured', detail: 'No plain-text management services enabled.', ros: '/ip/service/print' },
  ...(internet.doubleNat
    ? [{ level: 'warn' as const, title: 'Double NAT', detail: `Your WAN address ${internet.wanIp} is private: there's another router (${internet.gateway}) in front. Port forwards and some games/VPNs need it in bridge mode.`, ros: '/ip/dhcp-client/print' }]
    : []),
  ...(memPct > 75
    ? [{ level: 'warn' as const, title: `Memory ${Math.round(memPct)}% used`, detail: `Only ${bytes(router.memFree)} free.${adlist ? ` The DNS ad-block list (${adlist['name-count'].toLocaleString()} names) is the largest optional user.` : ''}`, ros: '/ip/dns/print; /ip/dns/adlist/print' }]
    : []),
  ...(flappers.length
    ? [{ level: 'warn' as const, title: `${flappers.length} device${flappers.length > 1 ? 's' : ''} switching bands`, detail: `${flappers.map((c) => c.name).join(', ')} keep hopping between 2.4 and 5 GHz on the same SSID. Usually weak 5 GHz signal at their location.`, ros: '/interface/wireless/registration-table/print' }]
    : []),
  ...(router.firmware && router.firmwareUpgrade && router.firmware !== router.firmwareUpgrade
    ? [{ level: 'warn' as const, title: 'RouterBOOT upgrade pending', detail: `Firmware ${router.firmware} → ${router.firmwareUpgrade} is ready and applies on next reboot.`, ros: '/system/routerboard/upgrade' }]
    : []),
]

// ---------- radios (legacy wireless monitor; order matches /interface/wireless [find]) ----------
export const radios = wifi.map((w, i) => {
  const m = (raw.radios ?? [])[i] ?? {}
  // "5180/20-Ceee/ac(23dBm)" -> freq 5180, extension letters give the real width (1 letter = 20 MHz)
  const ch = String(m.channel ?? '').match(/^(\d+)\/20-([A-Za-z]+)\/[^(]*(?:\((\d+)dBm\))?/)
  const width = ch ? 20 * ch[2].length : undefined
  const freq = ch ? +ch[1] : undefined
  const channel = freq ? (freq < 3000 ? (freq - 2407) / 5 : freq > 5900 ? (freq - 5950) / 5 : (freq - 5000) / 5) : undefined
  return {
    name: w.name, ssid: w.ssid, band: bandOf(w.name)!, running: w.running,
    widths: w.band.startsWith('2') ? [20, 40] : w.band.startsWith('6') ? [20, 40, 80, 160, 320] : [20, 40, 80, 160],
    maxWidth: +(String(w['channel-width']).match(/(\d+)mhz/)?.[1] ?? 20),
    width, channel, txPower: ch?.[3] ? +ch[3] : undefined,
    noise: m['noise-floor'] as number | undefined, ccq: m['overall-tx-ccq'] as number | undefined,
    clients: clients.filter((c) => c.port === w.name).length,
  }
})

// WiFi Technology card: clients grouped by standard + band
export const wifiTech = Object.values(
  clients.filter((c) => !c.wired).reduce((acc: Record<string, { type: string; band: string; clients: Client[] }>, c) => {
    const k = `${c.standard}|${c.band}`
    ;(acc[k] ??= { type: c.standard!, band: c.band!, clients: [] }).clients.push(c)
    return acc
  }, {}),
).sort((a, b) => b.clients.length - a.clients.length)

// WiFi Connectivity (association / auth / DHCP / DNS success), derived from the log + leases
const assoc = log.filter((l) => /: connected/.test(l.message)).length
const assocFail = log.filter((l) => /rejected|failed to connect|unicast key exchange timeout/i.test(l.message)).length
const authFail = log.filter((l) => /handshake timeout|wrong password|unicast key exchange timeout/i.test(l.message)).length
export const connectivity = [
  { name: 'Association', pct: assoc + assocFail ? Math.round((assoc / (assoc + assocFail)) * 100) : 100 },
  { name: 'Authentication', pct: assoc + authFail ? Math.round((assoc / (assoc + authFail)) * 100) : 100 },
  { name: 'DHCP', pct: leases.length ? Math.round((leases.filter((l) => l.status === 'bound').length / leases.length) * 100) : 100 },
  { name: 'DNS', pct: internet.latency.some((l) => l.ms != null) ? 100 : 0 },
]
export const adblock = (raw.adlist ?? []).filter((a: any) => !a.disabled).reduce((s: { names: number; hits: number }, a: any) => ({ names: s.names + a['name-count'], hits: s.hits + a['match-count'] }), { names: 0, hits: 0 })
