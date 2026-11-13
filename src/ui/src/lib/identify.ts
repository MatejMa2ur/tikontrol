// Who is this client? Maker from the MAC prefix (embedded IEEE table, no lookups), plus a best guess of the
// device type and OS from the DHCP hostname, the DHCP vendor class (option 60) and MNDP/LLDP/CDP neighbours.
// Every input is untrusted text from the device: it is only matched here and shown as plain text.
import { OUI } from './oui.ts'

export type Kind = 'phone' | 'computer' | 'server' | 'vm' | 'speaker' | 'iot' | 'devices' | 'unknown'
export type Guess = { kind: Kind; type: string; os?: string }

// Locally administered bit: phones/laptops with a "private Wi-Fi address", also VMs and containers
export const isPrivateMac = (mac: string) => (parseInt(mac.slice(0, 2), 16) & 2) === 2

// Hypervisor prefixes; QEMU's sets the local bit, so it is checked before the private-address rule
const VIRTUAL: Record<string, string> = { '525400': 'QEMU / KVM', '080027': 'VirtualBox', '000C29': 'VMware', '005056': 'VMware', '00155D': 'Hyper-V' }
let byPrefix: Map<string, string> | undefined
export function makerOf(mac: string): string | undefined {
  const p = mac.replace(/[^0-9A-Fa-f]/g, '').slice(0, 6).toUpperCase()
  if (VIRTUAL[p]) return VIRTUAL[p]
  if (isPrivateMac(mac)) return
  byPrefix ??= new Map(Object.entries(OUI).flatMap(([brand, list]) => list.split(' ').map((x) => [x, brand] as [string, string])))
  return byPrefix.get(p)
}

const g = (kind: Kind, type: string, os?: string): Guess => ({ kind, type, os })
// First match wins. ponytail: plain patterns over hostname + vendor class; add DHCP option 55 fingerprints if these fall short.
const HOST: [RegExp, Guess][] = [
  [/iphone/, g('phone', 'Phone', 'iOS')],
  [/ipad/, g('phone', 'Tablet', 'iPadOS')],
  [/apple-?tv/, g('speaker', 'TV', 'tvOS')],
  [/homepod/, g('speaker', 'Speaker', 'audioOS')],
  [/watch/, g('phone', 'Watch')],
  [/macbook|imac|mac-?(mini|studio|pro)|\bmbp\b/, g('computer', 'Computer', 'macOS')],
  [/galaxy-?tab|\bsm-[tx]\d/, g('phone', 'Tablet', 'Android')],
  [/galaxy|\bsm-[a-z]\d|pixel|oneplus|redmi|poco|^android|huawei|honor|oppo|vivo|moto|xiaomi|nokia/, g('phone', 'Phone', 'Android')],
  [/^(desktop|laptop|win)-|windows/, g('computer', 'Computer', 'Windows')],
  [/raspberrypi|^pi\d*$/, g('computer', 'Single-board computer', 'Linux')],
  [/^esp[_-]|esp32|esp8266|tasmota|shelly|tuya|^wled|sonoff|^hue|bridge|ikea|tradfri|nest|ecobee|ring-|camera|cam\d/, g('iot', 'Smart home')],
  [/chromecast|fire-?tv|^aft|bravia|webos|tizen|\btv\b|-tv|tv-|roku|shield/, g('speaker', 'TV')],
  [/sonos|speaker|google-?home|echo|alexa/, g('speaker', 'Speaker')],
  [/printer|^brw|^npi|epson|canon|^hp[0-9a-f]{6}/, g('iot', 'Printer')],
  [/playstation|^ps[345]|xbox|nintendo|switch|steam-?deck/, g('computer', 'Game console')],
  [/zima|\bnas\b|synology|diskstation|qnap|truenas|unraid|server/, g('server', 'Server')],
  [/qemu|kvm|\bvm\b|docker/, g('vm', 'Virtual machine')],
  [/\bpc\b|-pc|desktop|laptop|thinkpad|notebook/, g('computer', 'Computer')],
  [/arch|ubuntu|debian|fedora/, g('computer', 'Computer', 'Linux')],
]
const MAKER: [RegExp, Guess][] = [
  [/QEMU|VirtualBox|VMware|Hyper-V/, g('vm', 'Virtual machine')],
  [/Sonos|Bose/, g('speaker', 'Speaker')],
  [/Roku|Vizio|Hisense|TCL/, g('speaker', 'TV')],
  [/Espressif|Tuya|Shelly|Philips Hue|IKEA|ecobee|Ring|Nest|Dyson|Withings|Hikvision/, g('iot', 'Smart home')],
  [/Brother|Canon|Epson/, g('iot', 'Printer')],
  [/Nintendo|Valve/, g('computer', 'Game console')],
  [/Raspberry Pi/, g('computer', 'Single-board computer', 'Linux')],
  [/Synology|QNAP/, g('server', 'NAS')],
  [/MikroTik|Ubiquiti|Netgear|Zyxel|D-Link|Cisco|AVM/, g('devices', 'Network gear')],
  [/Intel|Realtek|Dell|Lenovo|ASUS/, g('computer', 'Computer')],
  [/OnePlus|OPPO|vivo|Motorola|Honor/, g('phone', 'Phone', 'Android')],
]
// DHCP option 60: "android-dhcp-14", "MSFT 5.0", "udhcp 1.36.1", "dhcpcd-9.4.1:Linux-6.1:...", "MikroTik"
function osOfClass(cls: string): Guess | undefined {
  const a = cls.match(/^android-dhcp-(\d+)/i)
  if (a) return g('phone', 'Phone', `Android ${a[1]}`)
  if (/^msft/i.test(cls)) return g('computer', 'Computer', 'Windows')
  if (/mikrotik|routeros|ubnt/i.test(cls)) return g('devices', 'Network gear')
  if (/^udhcp/i.test(cls)) return g('iot', 'Embedded device', 'Linux')
  if (/linux/i.test(cls)) return g('unknown', '', 'Linux')
}

/** Settings · System · Device identification. Off: no type/OS guesses, only what the router and the device report. */
export const identOn = { v: true }

export type Neighbor = { identity?: string; platform?: string; board?: string; version?: string }
export function guessDevice(o: { hostname?: string; classId?: string; maker?: string; neighbor?: Neighbor; priv: boolean; wired: boolean }): Guess | undefined {
  if (o.neighbor) {
    const n = o.neighbor
    return g('devices', n.board || 'Network gear', [n.platform, n.version].filter(Boolean).join(' ') || undefined)
  }
  if (!identOn.v) return
  const h = (o.hostname ?? '').toLowerCase()
  const byClass = o.classId ? osOfClass(o.classId) : undefined
  const hit = HOST.find(([re]) => re.test(h))?.[1] ?? MAKER.find(([re]) => re.test(o.maker ?? ''))?.[1] ?? (byClass?.type ? byClass : undefined)
  const os = byClass?.os ?? hit?.os ?? (o.maker === 'Apple' && hit?.kind === 'computer' ? 'macOS' : undefined)
  if (hit) return { ...hit, os }
  if (o.priv && !o.wired) return g('phone', 'Phone or laptop', os)
  if (os) return g('unknown', '', os)
}

// Model identifiers devices announce over mDNS TXT (model, md, rpMd, am, ty) and UPnP (modelName, deviceType).
// Our own short table of public identifiers; first match wins. Checked before the host-name guess.
const MODEL: [RegExp, Guess][] = [
  [/^iPhone\d/i, g('phone', 'Phone', 'iOS')],
  [/^iPad\d/i, g('phone', 'Tablet', 'iPadOS')],
  [/^Watch\d/i, g('phone', 'Watch', 'watchOS')],
  [/^AppleTV\d/i, g('speaker', 'TV', 'tvOS')],
  [/^AudioAccessory\d/i, g('speaker', 'Speaker', 'audioOS')],
  [/^(MacBook|iMac|Macmini|MacPro|MacStudio|Mac\d)/i, g('computer', 'Computer', 'macOS')],
  [/^AirPort/i, g('devices', 'Network gear')],
  [/PlayStation|Xbox|Nintendo/i, g('computer', 'Game console')],
  [/Chromecast|Google TV|SHIELD|Fire ?TV|Roku|BRAVIA|webOS/i, g('speaker', 'TV')],
  [/Google Home|Google Nest|Nest (Mini|Audio|Hub)|Sonos|HomePod|Echo/i, g('speaker', 'Speaker')],
  [/:MediaRenderer:/i, g('speaker', 'Media player')],
  [/:(InternetGatewayDevice|WLANAccessPoint):/i, g('devices', 'Network gear')],
  [/:Printer:|LaserJet|OfficeJet|DeskJet|PIXMA|EcoTank|\b(DCP|HL|MFC)-[A-Z]?\d/i, g('iot', 'Printer')],
  [/DiskStation|RackStation|QNAP|TrueNAS/i, g('server', 'NAS')],
  [/:MediaServer:/i, g('server', 'Media server')],
]
const MODEL_KEYS = ['model', 'md', 'rpMd', 'am', 'ty', 'product', 'usb_MDL', 'modelName', 'deviceType']
// mDNS service types that say what a device is when no model matched
const SERVICE: [string, Guess][] = [
  ['_ipp._tcp', g('iot', 'Printer')], ['_printer._tcp', g('iot', 'Printer')],
  ['_googlecast._tcp', g('speaker', 'Cast device')], ['_spotify-connect._tcp', g('speaker', 'Speaker')],
  ['_hap._tcp', g('iot', 'Smart home')],
]
/** Kind from what a device said about itself: [source, key, value] rows from LAN discovery. */
export function guessModel(kv: [string, string, string][]): Guess | undefined {
  for (const [, k, v] of kv) if (MODEL_KEYS.includes(k)) { const hit = MODEL.find(([re]) => re.test(v)); if (hit) return hit[1] }
  for (const [, k, v] of kv) if (k === 'service') { const hit = SERVICE.find(([t]) => v === t); if (hit) return hit[1] }
}

/** What makes two clients "the same kind of device" when the user picks an icon: the DHCP request list and
 *  vendor class, else a host-name pattern (digits and hex ids replaced), with the maker. Undefined when too weak. */
export function fingerprintKey(o: { prl?: string; classId?: string; hostname?: string; maker?: string }): string | undefined {
  const host = (o.hostname ?? '').toLowerCase().replace(/(?=[a-f]*\d)[0-9a-f]{4,}|\d+/g, '#').slice(0, 40)
  if (!o.prl && !o.classId && !/[a-z]{3}/.test(host)) return
  return [o.maker ?? '', o.classId ?? '', o.prl ?? '', o.prl || o.classId ? '' : host].join('|')
}

/** Events category of a RouterOS log line. */
// "login failure for user x" is Security; "... added by ssh-cmd:admin@10.0.0.9 (...)" or "changed by winbox-..." is Admin
export const catOf = (topics: string[], message = ''): 'Clients' | 'WiFi' | 'System' | 'Security' | 'Admin' =>
  topics.includes('wireless') || topics.includes('wifi') ? 'WiFi' : topics.includes('dhcp') ? 'Clients'
    : topics.includes('firewall') || /login failure/.test(message) ? 'Security'
    : topics.includes('account') || / by [\w-]+:[^\s@]+@\S+/.test(message) ? 'Admin' : 'System'
