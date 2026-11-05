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

export type Neighbor = { identity?: string; platform?: string; board?: string; version?: string }
export function guessDevice(o: { hostname?: string; classId?: string; maker?: string; neighbor?: Neighbor; priv: boolean; wired: boolean }): Guess | undefined {
  if (o.neighbor) {
    const n = o.neighbor
    return g('devices', n.board || 'Network gear', [n.platform, n.version].filter(Boolean).join(' ') || undefined)
  }
  const h = (o.hostname ?? '').toLowerCase()
  const byClass = o.classId ? osOfClass(o.classId) : undefined
  const hit = HOST.find(([re]) => re.test(h))?.[1] ?? MAKER.find(([re]) => re.test(o.maker ?? ''))?.[1] ?? (byClass?.type ? byClass : undefined)
  const os = byClass?.os ?? hit?.os ?? (o.maker === 'Apple' && hit?.kind === 'computer' ? 'macOS' : undefined)
  if (hit) return { ...hit, os }
  if (o.priv && !o.wired) return g('phone', 'Phone or laptop', os)
  if (os) return g('unknown', '', os)
}
