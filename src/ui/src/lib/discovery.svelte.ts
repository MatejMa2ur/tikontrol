// What devices say about themselves over mDNS/SSDP, asked by the tikontrol server (src/server/src/discovery.rs),
// what they ask for in DHCP (the server's opt-in listener), and what the user taught by picking an icon.
// Every device value is untrusted text, shown as plain text only.
import { api, preload } from './backend'
import { clients, isBackend, raw, type Client, type Kind } from './data'
import { appleName, fingerprintKey, fromPrl, guessDevice, guessModel, identOn, listed } from './identify'
import { prefs, displayName } from './live.svelte'

export type Said = { ip: string; mac?: string; t: number; kv: [source: string, key: string, value: string][] }
export type Heard = { mac: string; t: number; prl: string; class: string; host: string; fqdn: string }
export const disc = $state({
  auto: false, ident: identOn.v, dhcp: false, listening: false, dhcpError: null as string | null,
  running: false, wait: 0, devices: [] as Said[], fingerprints: [] as Heard[],
})
export const loadDiscovery = () => api<typeof disc>('/discovery').then((v) => Object.assign(disc, v)).catch(() => {})
if (isBackend) { if (preload.disc) Object.assign(disc, preload.disc); else loadDiscovery() }

export const saidBy = (c: { mac: string; ip: string }) => disc.devices.find((d) => d.mac === c.mac) ?? disc.devices.find((d) => !d.mac && d.ip === c.ip)
export const heardFrom = (c: { mac: string }) => disc.fingerprints.find((f) => f.mac === c.mac.toUpperCase())
const pick = (d: Said | undefined, keys: string[]) => d && keys.map((k) => d.kv.find(([, key]) => key === k)?.[2]).find(Boolean)

export type Source = 'you' | 'neighbor' | 'model' | 'learned' | 'list' | 'name'
export const SOURCES: Record<Source, { label: string; confidence: 'high' | 'medium' | 'low' }> = {
  you: { label: 'Your choice', confidence: 'high' },
  neighbor: { label: 'Neighbour announcement (MNDP/LLDP)', confidence: 'high' },
  model: { label: 'Model the device announces (mDNS/SSDP)', confidence: 'high' },
  learned: { label: 'Learned from your choice', confidence: 'medium' },
  list: { label: 'Public device list (Home Assistant discovery rules)', confidence: 'medium' },
  name: { label: 'Guess from host name, DHCP vendor class and maker', confidence: 'low' },
}
const LABEL: Record<string, string> = { phone: 'Phone', computer: 'Computer', server: 'Server', vm: 'Virtual machine', speaker: 'Media', iot: 'Smart home', devices: 'Network gear' }
export type Identity = Client & { model?: string; product?: string; source?: Source; from?: string }

const keyOf = (c: Client) => {
  const f = heardFrom(c)
  return fingerprintKey({ prl: f?.prl, classId: c.classId ?? (f?.class || undefined), hostname: c.hostname ?? (f?.host || undefined), maker: c.vendor })
}
// ponytail: learns only from clients in the current snapshot; store key -> kind on the server if taught clients age out
function learned(c: Client) {
  const key = keyOf(c)
  if (!key) return
  for (const [mac, kind] of Object.entries(prefs.icons)) {
    if (mac === c.mac) continue
    const t = clients.find((x) => x.mac === mac)
    if (t && keyOf(t) === key) return { kind: kind as Kind, from: displayName(t) }
  }
}

/** The client with maker, model, type and kind improved by what the device said, the public device lists and what
 *  you taught, and where that came from. */
export function identity(c: Client): Identity {
  const r = guessed(c)
  if (!disc.ident) return r
  const d = saidBy(c), f = heardFrom(c)
  const product = d?.kv.filter(([, k]) => ['model', 'rpMd', 'am'].includes(k)).map(([, , v]) => appleName(v)).find(Boolean)
  const hit = listed({ host: c.hostname ?? (f?.host || undefined), mac: c.mac, kv: d?.kv ?? [] })
  const out: Identity = { ...r, vendor: r.vendor ?? hit?.brand ?? (product && 'Apple'), product: product ?? hit?.name }
  // the list beats a host-name guess, never your choice, a neighbour or a model the device announced
  if (hit?.kind && (!r.source || r.source === 'name')) return { ...out, kind: hit.kind, type: LABEL[hit.kind], source: 'list' }
  return out
}

function guessed(c: Client): Identity {
  const d = saidBy(c), f = heardFrom(c)
  const maker = c.vendor ?? pick(d, ['manufacturer', 'usb_MFG', 'mfr']) ?? fromPrl(f?.prl)?.maker
  const model = pick(d, ['modelName', 'model', 'md', 'rpMd', 'am', 'ty', 'product', 'usb_MDL'])
  // no lease comment or DHCP host name: the name the device announced (mDNS host, UPnP name, DHCP option 12/81)
  const said = !c.hostname && c.name.endsWith(c.mac.slice(-5)) ? pick(d, ['host', 'friendlyName', 'fn']) || f?.host || f?.fqdn.split('.')[0] : undefined
  const out: Identity = { ...c, vendor: maker, model, ...(said && { name: said }) }
  if (!disc.ident) return out
  const neighbor = Array.isArray(raw.neighbors) && raw.neighbors.some((n: any) => n?.['mac-address'] === c.mac)
  const own = (kind: string, source: Source, from?: string): Identity =>
    ({ ...out, kind: kind as Kind, type: kind === c.kind ? c.type : LABEL[kind], os: kind === c.kind ? c.os : undefined, source, from })
  if (prefs.icons[c.mac]) return own(prefs.icons[c.mac], 'you')
  if (neighbor) return { ...out, source: 'neighbor' }
  const m = d && guessModel(d.kv)
  if (m) return { ...out, ...m, os: m.os ?? c.os, source: 'model' }
  const l = learned(c)
  if (l) return own(l.kind, 'learned', l.from)
  // a device that said something, or a DHCP request the router doesn't keep: guess again with it
  if (d || (f && (!c.hostname || !c.classId))) {
    const hostname = [model, pick(d, ['friendlyName', 'fn']), c.hostname ?? (f?.host || f?.fqdn.split('.')[0])].filter(Boolean).join(' ')
    const g = guessDevice({ hostname, classId: c.classId ?? (f?.class || undefined), prl: f?.prl, maker, priv: c.privateMac, wired: c.wired })
    return { ...out, type: g?.type || c.type, os: c.os ?? g?.os, kind: g && g.kind !== 'unknown' ? g.kind : c.kind, source: g || c.type ? 'name' : undefined }
  }
  return { ...out, source: c.type || c.os ? 'name' : undefined }
}
