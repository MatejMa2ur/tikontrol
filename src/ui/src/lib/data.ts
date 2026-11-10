// Normalises a RouterOS snapshot (src/scripts/snapshot.py) into the UI model.
// The tikontrol server serves this same shape from the live binary API (src/server/src/ros/queries.rs);
// without it the mockup falls back to snapshot.json / sample.json. `load()` swaps in a fresh snapshot.

import { simulateBe3 } from './be3'
import { bandLabel, detectHardware, normalizeWifi, parseChannel, standardOf } from './hardware'
import { preload } from './backend'
import { catOf, guessDevice, isPrivateMac, makerOf, type Kind } from './identify'
import { guestTest } from './guest'
import { securityAudit, type Insight } from './security'
export type { Kind, Insight }

const files = import.meta.glob('./*.json', { eager: true, import: 'default' }) as Record<string, any>
export const simulated = new URLSearchParams(location.search).get('device') === 'be3'
export const isBackend = !!preload.snapshot
export const isSample = !isBackend && !files['./snapshot.json']

export type Client = {
  mac: string
  ip: string
  name: string
  hostname?: string
  vendor?: string // maker from the MAC prefix
  classId?: string // DHCP vendor class (option 60), untrusted
  type?: string // best guess, e.g. 'Phone'
  os?: string // best guess, e.g. 'Android 14'
  kind: Kind
  wired: boolean
  guest: boolean // on a guest network (lib/guest.ts)
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
  bandLog: { time: string; iface: string; band?: string; joined: boolean; signal: number }[] // joins/leaves from the log, oldest first
  ccq?: number // link quality %, legacy wireless only
  standard?: string // 'WiFi 4' | 'WiFi 5' (heuristic)
}
export type Port = { name: string; up: boolean; wan: boolean; clients: string[] }
export type Event = { time: string; cat: 'Clients' | 'WiFi' | 'System' | 'Security' | 'Admin'; level: 'info' | 'warning' | 'error'; text: string }

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
export const subtitle = (c: Client, shown: string) =>
  [c.vendor ?? (c.privateMac ? (c.wired ? 'Private address' : 'Private Wi-Fi address') : c.hostname && c.hostname !== shown ? c.hostname : 'Unknown maker'), c.type].filter(Boolean).join(' · ')

export function signalQuality(dbm?: number) {
  if (dbm == null) return { label: 'Wired', level: 'ok' as const }
  if (dbm >= -55) return { label: 'Excellent', level: 'ok' as const }
  if (dbm >= -67) return { label: 'Good', level: 'ok' as const }
  if (dbm >= -75) return { label: 'Fair', level: 'warn' as const }
  return { label: 'Poor', level: 'bad' as const }
}

// ---------- normalise ----------
function build(raw: any) {
const hw = detectHardware(raw)
const leases: any[] = raw.leases ?? []
const stations: any[] = raw.stations ?? []
const hosts: any[] = (raw.bridgeHosts ?? []).filter((h: any) => !h.local)
const arp: any[] = raw.arp ?? []
const wifi: any[] = raw.wifi ?? []
const log: any[] = Array.isArray(raw.log) ? raw.log : []
const lq: any[] = raw.linkQuality ?? []
// a neighbour row with no identity, MAC or address can't be shown or set up: skip it
const neighbors: any[] = (Array.isArray(raw.neighbors) ? raw.neighbors : []).filter((n: any) => n && (n.identity || n['mac-address'] || n.address))

const bandOf = (ifname?: string) => {
  const w = wifi.find((x) => x.name === ifname)
  return w ? bandLabel(w.band) as Client['band'] : undefined
}
const logRe = /^([0-9A-F:]{17})@(w(?:lan|ifi)\d+): (connected|disconnected)[^]*?signal strength (-\d+)/
const bandLogOf = (mac: string) => log.flatMap((l) => {
  const m = String(l.message).match(logRe)
  return m && m[1] === mac ? [{ time: String(l.time ?? ''), iface: m[2], band: bandOf(m[2]), joined: m[3] === 'connected', signal: +m[4] }] : []
})
const roamCount = (mac: string) => log.filter((l) => l.message?.startsWith(mac) && /connected/.test(l.message)).length

const macs = new Set<string>([...leases.map((l) => l['mac-address']), ...stations.map((s) => s['mac-address'])])
// ARP-only devices (static IP, no lease) still count as clients if reachable
for (const a of arp) if (a.status === 'reachable' && a.interface === 'bridge') macs.add(a['mac-address'])

const isGuest = guestTest(raw)
const clients: Client[] = [...macs].filter(Boolean).map((mac) => {
  const lease = leases.find((l) => l['mac-address'] === mac)
  const st = stations.find((s) => s['mac-address'] === mac)
  const host = hosts.find((h) => h['mac-address'] === mac)
  const a = arp.find((x) => x['mac-address'] === mac)
  const vendor = makerOf(mac)
  const priv = isPrivateMac(mac)
  const wired = !st
  const hostname = lease?.['host-name']
  const classId = lease?.['class-id'] || undefined
  const guess = guessDevice({ hostname: hostname || lease?.comment, classId, maker: vendor, neighbor: neighbors.find((n) => n['mac-address'] === mac), priv, wired })
  // reg-table `bytes` is "rx,tx" from the AP's side: rx = client upload, tx = client download
  // (sanity check: a Sonos streaming music shows the big number second)
  const [up, down] = String(st?.bytes ?? '0,0').split(',').map(Number)
  return {
    mac,
    ip: lease?.address ?? st?.['last-ip'] ?? a?.address ?? '—',
    hostname,
    vendor,
    classId,
    type: guess?.type || undefined,
    os: guess?.os,
    name: lease?.comment || hostname || (vendor ? `${vendor} ${mac.slice(-5)}` : priv ? `Private device ${mac.slice(-5)}` : `Device ${mac.slice(-5)}`),
    kind: guess?.kind ?? 'unknown',
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
    bandLog: bandLogOf(mac),
    ccq: lq.find((x) => x['mac-address'] === mac)?.['tx-ccq'],
    // ponytail: the radio's best standard, the client may negotiate less. The registration table's `band` has the real one.
    standard: st ? standardOf(wifi.find((w) => w.name === st.interface)?.band) : undefined,
  }
}).map((c) => ({ ...c, guest: isGuest(c) }))

const offline = [
  // ARP entries on the WAN side (the ISP's gateway) and failed ones without a MAC are not clients
  ...arp.filter((a) => a.status !== 'reachable' && a['mac-address'] && a.interface !== raw.wan?.[0]?.interface && !macs.has(a['mac-address'])).map((a) => ({ mac: a['mac-address'] as string, ip: a.address as string, seen: a.status as string })),
  ...leases.filter((l) => l.status !== 'bound').map((l) => ({ mac: l['mac-address'] as string, ip: l.address as string, seen: l['last-seen'] as string })),
].filter((o, i, all) => all.findIndex((x) => x.mac === o.mac) === i)

const ports: Port[] = (raw.ethernet ?? []).map((e: any) => ({
  name: e.name,
  up: e.running && !e.disabled,
  wan: (raw.wan ?? []).some((w: any) => w.interface === e.name),
  clients: clients.filter((c) => c.port === e.name).map((c) => c.mac),
}))

const res = raw.resource ?? {}
const wan = (raw.wan ?? [])[0] ?? {}
const router = {
  identity: typeof raw.identity === 'string' ? raw.identity : 'router',
  board: res['board-name'],
  model: raw.routerboard?.model,
  version: res.version,
  channel: raw.update?.channel,
  latest: raw.update?.['latest-version'] as string | undefined,
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
const internet = {
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
const ssids = [...new Set(wifi.map((w) => w.ssid))].map((ssid) => ({
  ssid,
  radios: wifi.filter((w) => w.ssid === ssid).map((w) => ({ name: w.name, band: bandOf(w.name)!, running: w.running })),
  clients: clients.filter((c) => c.ssid === ssid).length,
}))

const nameFor = (mac: string) => clients.find((c) => c.mac === mac)?.name ?? mac
const events: Event[] = log
  .map((l) => ({
    time: l.time,
    cat: catOf(l.topics ?? [], String(l.message)),
    level: (l.topics ?? []).includes('error') ? 'error' : (l.topics ?? []).includes('warning') ? 'warning' : 'info',
    // "AA:BB..@wlan2: connected, signal strength -84" -> "Alex's phone connected to 5 GHz (-84 dBm)"
    text: String(l.message).replace(/^([0-9A-F:]{17})@(w(?:lan|ifi)\d): (connected|disconnected)[^,]*(?:, registered to other interface)?, signal strength (-\d+)/,
      (_: string, mac: string, ifn: string, what: string, sig: string) => `${nameFor(mac)} ${what === 'connected' ? 'joined' : 'left'} ${bandOf(ifn) ?? ifn} (${sig} dBm)`)
      .replace(/assigned ([\d.]+) for ([0-9A-F:]{17})/, (_: string, ip: string, mac: string) => `gave ${ip} to ${nameFor(mac)}`),
  }) as Event)
  .reverse()

// ---------- insights: "status first" (PLAN §0.6 rule 4) ----------
// security audit: services, open DNS, admin user, UPnP, RouterOS updates (src/ui/src/lib/security.ts)
const audit = securityAudit(raw)
const memPct = (1 - router.memFree / router.memTotal) * 100
const flappers = clients.filter((c) => c.roams >= 4)
const adlist = (raw.adlist ?? [])[0]

const insights: Insight[] = [
  ...(audit.length ? audit : [{ level: 'ok' as const, title: 'No security issues found', detail: 'Management services, DNS, users, UPnP and RouterOS version look fine.', ros: '/ip/service/print' }]),
  ...(internet.doubleNat
    ? [{ level: 'warn' as const, title: 'Double NAT', detail: `Your WAN address ${internet.wanIp} is private: there's another router (${internet.gateway}) in front. Port forwards and some games/VPNs need it in bridge mode.`, ros: '/ip/dhcp-client/print' }]
    : []),
  ...(memPct > 75
    ? [{ level: 'warn' as const, title: `Memory ${Math.round(memPct)}% used`, detail: `Only ${bytes(router.memFree)} free.${adlist ? ` The DNS ad-block list (${adlist['name-count'].toLocaleString()} names) is the largest optional user.` : ''}`, ros: '/ip/dns/print; /ip/dns/adlist/print' }]
    : []),
  ...(flappers.length
    ? [{ level: 'warn' as const, title: `${flappers.length} device${flappers.length > 1 ? 's' : ''} switching bands`, detail: `${flappers.map((c) => c.name).join(', ')} keep hopping between 2.4 and 5 GHz on the same SSID. Usually weak 5 GHz signal at their location. Open the device to see why and how to fix it.`, ros: '/interface/wireless/registration-table/print', href: flappers.length === 1 ? `#/clients/${flappers[0].mac}` : '#/clients' }]
    : []),
  ...(router.firmware && router.firmwareUpgrade && router.firmware !== router.firmwareUpgrade
    ? [{ level: 'warn' as const, title: 'RouterBOOT upgrade pending', detail: `Firmware ${router.firmware} → ${router.firmwareUpgrade} is ready and applies on next reboot.`, ros: '/system/routerboard/upgrade' }]
    : []),
]

// ---------- radios (monitor; order matches the print order, src/server/src/ros/queries.rs) ----------
const radios = wifi.map((w, i) => {
  const m = (raw.radios ?? [])[i] ?? {}
  const ch = parseChannel(m.channel)
  return {
    name: w.name, ssid: w.ssid, band: bandOf(w.name)!, running: w.running,
    widths: w.band.startsWith('2') ? [20, 40] : w.band.startsWith('6') ? [20, 40, 80, 160, 320] : [20, 40, 80, 160],
    maxWidth: +(String(w['channel-width']).match(/(\d+)mhz/)?.[1] ?? 20),
    widthSetting: w['channel-width'] as string | undefined,
    auto: w.frequency == null || w.frequency === 'auto',
    pkg: (w.pkg ?? 'wireless') as 'wireless' | 'wifi',
    width: ch.width, channel: ch.channel, freq: ch.freq, dfs: ch.dfs,
    txPower: ch.txPower ?? (typeof m['tx-power'] === 'number' ? m['tx-power'] : undefined),
    noise: m['noise-floor'] as number | undefined, ccq: m['overall-tx-ccq'] as number | undefined,
    clients: clients.filter((c) => c.port === w.name).length,
  }
})

// WiFi Technology card: clients grouped by standard + band
const wifiTech = Object.values(
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
const connectivity = [
  { name: 'Association', pct: assoc + assocFail ? Math.round((assoc / (assoc + assocFail)) * 100) : 100 },
  { name: 'Authentication', pct: assoc + authFail ? Math.round((assoc / (assoc + authFail)) * 100) : 100 },
  { name: 'DHCP', pct: leases.length ? Math.round((leases.filter((l) => l.status === 'bound').length / leases.length) * 100) : 100 },
  { name: 'DNS', pct: internet.latency.some((l) => l.ms != null) ? 100 : 0 },
]
const adblock = (raw.adlist ?? []).filter((a: any) => !a.disabled).reduce((s: { names: number; hits: number }, a: any) => ({ names: s.names + a['name-count'], hits: s.hits + a['match-count'] }), { names: 0, hits: 0 })
return { raw, hw, clients, offline, ports, router, internet, ssids, events, insights, radios, wifiTech, connectivity, adblock, neighbors }
}

type Built = ReturnType<typeof build>
// Live bindings: importers read the newest values after `load()`; pages re-mount to pick them up (App.svelte).
export let raw: any, hw: Built['hw'], clients: Built['clients'], offline: Built['offline'], ports: Built['ports'],
  router: Built['router'], internet: Built['internet'], ssids: Built['ssids'], events: Built['events'],
  insights: Built['insights'], radios: Built['radios'], wifiTech: Built['wifiTech'], connectivity: Built['connectivity'],
  adblock: Built['adblock'], neighbors: Built['neighbors']
export function load(snapshot: any) {
  ;({ raw, hw, clients, offline, ports, router, internet, ssids, events, insights, radios, wifiTech, connectivity, adblock, neighbors } = build(normalizeWifi(snapshot)))
}
load(preload.snapshot ?? (simulated ? simulateBe3(files['./sample.json']) : files['./snapshot.json'] ?? files['./sample.json']))

// "Why is this client on 2.4 GHz?" Plain-language reasons from what the router reports; null when there's nothing to explain.
export function whyBand(c: Client) {
  const r5 = radios.find((r) => r.band === '5 GHz'), r2 = radios.find((r) => r.band === '2.4 GHz')
  const switching = new Set(c.bandLog.filter((l) => l.joined).map((l) => l.band)).size > 1
  if (c.wired || !r5 || !r2 || (c.band !== '2.4 GHz' && !switching)) return null
  const avg = (band: string) => {
    const s = c.bandLog.filter((l) => l.band === band).map((l) => l.signal)
    if (c.band === band && c.signal != null) s.push(c.signal)
    return s.length ? Math.round(s.reduce((a, b) => a + b, 0) / s.length) : undefined
  }
  const sig5 = avg('5 GHz'), sig2 = avg('2.4 GHz')
  const reasons: string[] = []
  if (sig5 == null) reasons.push('This device hasn\'t joined 5 GHz in the recent log. It may only support 2.4 GHz (many smart-home gadgets do); then nothing is wrong.')
  else if (sig5 < -70 || (sig2 != null && sig2 - sig5 >= 8)) reasons.push(`Weak 5 GHz signal where it is: about ${sig5} dBm on 5 GHz${sig2 != null ? ` against ${sig2} dBm on 2.4 GHz` : ''}. 5 GHz loses more through walls, so the device moves to the stronger 2.4 GHz.`)
  if (r5.ssid && r5.ssid === r2.ssid) reasons.push(hw.wifiPackage === 'wireless'
    ? 'Both bands share one network name, and the legacy wireless package has no band steering: the device alone picks the band, and switches whenever 2.4 GHz looks stronger.'
    : 'Both bands share one network name, so the device picks the band itself. (The wifi package can steer clients; tikontrol doesn\'t set that up yet.)')
  if (r5.dfs) reasons.push(`5 GHz is on DFS channel ${r5.channel}${r5.auto ? ', picked automatically' : ''}. If the router hears radar it must leave the channel, which drops every client, then listen for 60 s before sending again. For that minute only 2.4 GHz is left.`)
  if ((r5.width ?? 0) >= 80) reasons.push(`The 5 GHz channel is ${r5.width} MHz wide. The same power spread over a wider channel means a weaker, noisier edge of coverage than at 40 MHz.`)
  if (r5.ccq != null && r5.ccq < 50) reasons.push(`5 GHz link quality (CCQ) is ${r5.ccq}%: many frames are sent again, which slows the link.`)
  const limited = (raw.accessList ?? []).some((a: any) => a.interface === r5.name && a['signal-range'])
  return { r5, sig5, sig2, switching, reasons, limited, sameSsid: !!r5.ssid && r5.ssid === r2.ssid }
}
