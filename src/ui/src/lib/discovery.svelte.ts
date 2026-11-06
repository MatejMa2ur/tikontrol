// What devices say about themselves over mDNS/SSDP, asked by the tikontrol server (src/server/src/discovery.rs).
// Opt-in on the server; every value is untrusted device text, shown as plain text only.
import { api } from './backend'
import { isBackend, type Client } from './data'
import { guessDevice } from './identify'

export type Said = { ip: string; mac?: string; t: number; kv: [source: string, key: string, value: string][] }
export const disc = $state({ auto: false, running: false, wait: 0, devices: [] as Said[] })
export const loadDiscovery = () => api<typeof disc>('/discovery').then((v) => Object.assign(disc, v)).catch(() => {})
if (isBackend) loadDiscovery()

export const saidBy = (c: { mac: string; ip: string }) => disc.devices.find((d) => d.mac === c.mac) ?? disc.devices.find((d) => !d.mac && d.ip === c.ip)
const pick = (d: Said, keys: string[]) => keys.map((k) => d.kv.find(([, key]) => key === k)?.[2]).find(Boolean)

/** The client with maker, model and type improved by what the device said, if it said anything. */
export function identity(c: Client): Client & { model?: string } {
  const d = saidBy(c)
  if (!d) return c
  const maker = c.vendor ?? pick(d, ['manufacturer', 'usb_MFG', 'mfr'])
  const model = pick(d, ['modelName', 'model', 'md', 'rpMd', 'am', 'ty', 'product', 'usb_MDL'])
  const g = guessDevice({ hostname: [model, pick(d, ['friendlyName', 'fn']), c.hostname].filter(Boolean).join(' '), classId: c.classId, maker, priv: c.privateMac, wired: c.wired })
  return { ...c, vendor: maker, model, type: g?.type || c.type, os: c.os ?? g?.os, kind: g && g.kind !== 'unknown' ? g.kind : c.kind }
}
