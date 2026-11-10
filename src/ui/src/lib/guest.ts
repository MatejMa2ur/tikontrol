// Which clients are guests. A guest is a device on a guest network:
// - logged in to a hotspot (/ip/hotspot/active), or
// - on an interface that is a guest one: tikontrol's guest Wi-Fi (name tik-guest-…, comment tik:guest:…, wifi.rs),
//   a hotspot's interface (hotspot.rs), or an interface, DHCP server or SSID whose name or comment says "guest", or
// - with an IP inside a guest subnet: an /ip/address on such an interface, or with a tik:hotspot / guest comment.
// Names and comments come from the router config; they're only matched, never shown or run.

const GUEST = /guest/i
const str = (v: unknown) => (typeof v === 'string' ? v : '')
const list = (v: unknown): any[] => (Array.isArray(v) ? v : [])

function ip4(s: string): number | undefined {
  const p = s.split('.')
  if (p.length !== 4 || p.some((x) => !/^\d{1,3}$/.test(x) || +x > 255)) return
  return p.reduce((a, x) => a * 256 + +x, 0)
}
/** "10.0.0.1/24" → [network, prefix]; a bare address is a /32 */
function cidr(s: string): [number, number] | undefined {
  const [a, n = '32'] = s.split('/')
  const ip = ip4(a), bits = +n
  if (ip == null || !/^\d{1,2}$/.test(n) || bits > 32) return
  const mask = bits ? (0xffffffff << (32 - bits)) >>> 0 : 0
  return [(ip & mask) >>> 0, bits]
}
const inNet = (ip: number, [net, bits]: [number, number]) => (bits ? ((ip & ((0xffffffff << (32 - bits)) >>> 0)) >>> 0) === net : true)

export type GuestClient = { mac: string; ip: string; port?: string; ssid?: string }

/** Builds a test for one router snapshot (raw data as data.ts reads it, after normalizeWifi). */
export function guestTest(raw: any): (c: GuestClient) => boolean {
  const ifaces = new Set<string>()
  for (const w of list(raw?.wifi)) {
    const name = str(w.name), comment = str(w.comment)
    if (name.startsWith('tik-guest-') || comment.startsWith('tik:guest:') || GUEST.test(name) || GUEST.test(comment) || GUEST.test(str(w.ssid))) ifaces.add(name)
  }
  for (const i of list(raw?.interfaces)) if (GUEST.test(str(i.name)) || GUEST.test(str(i.comment))) ifaces.add(str(i.name))
  for (const h of list(raw?.hotspot)) if (str(h.interface) && h.disabled !== true) ifaces.add(str(h.interface))
  for (const d of list(raw?.dhcpServers)) if (GUEST.test(str(d.name))) ifaces.add(str(d.interface))
  ifaces.delete('')

  const nets: [number, number][] = []
  for (const a of list(raw?.addresses)) {
    const comment = str(a.comment)
    if (ifaces.has(str(a.interface)) || comment === 'tik:hotspot' || GUEST.test(comment)) {
      const n = cidr(str(a.address))
      if (n && n[1] < 32) nets.push([n[0], n[1]])
    }
  }
  const active = list(raw?.hotspotActive)
  const macs = new Set(active.map((h) => str(h['mac-address']).toUpperCase()).filter(Boolean))
  const ips = new Set(active.map((h) => str(h.address)).filter(Boolean))

  return (c) => {
    if (macs.has(c.mac.toUpperCase()) || ips.has(c.ip)) return true
    if (c.port && ifaces.has(c.port)) return true
    if (c.ssid && GUEST.test(c.ssid)) return true
    const ip = ip4(c.ip)
    return ip != null && nets.some((n) => inNet(ip, n))
  }
}
