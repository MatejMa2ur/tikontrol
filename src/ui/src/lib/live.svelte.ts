// Live state. In the mockup the "live" numbers are a random walk around the snapshot values;
// the real backend pushes the same fields over a WebSocket every second.
import { clients, internet, router } from './data'

const N = 120 // seconds of history in the WAN chart
const now = () => Math.floor(Date.now() / 1000)
const walk = (v: number, base: number, spread = 0.12) => Math.max(0, v + (Math.random() - 0.5) * base * spread + (base - v) * 0.05)

// Share of WAN traffic per client, from their byte counters (wired clients get a small share)
const total = clients.reduce((s, c) => s + c.bytesDown + c.bytesUp, 0) || 1
const share = Object.fromEntries(clients.map((c) => [c.mac, c.wired ? 0.04 : (c.bytesDown + c.bytesUp) / total]))

const history = (base: number) => { let v = base; return Array.from({ length: N }, () => (v = walk(v, base))) }

export const live = $state({
  t: Array.from({ length: N }, (_, i) => now() - N + i),
  rx: history(internet.rx || 1e6),
  tx: history(internet.tx || 2e5),
  cpu: router.cpuLoad,
  rate: Object.fromEntries(clients.map((c) => [c.mac, { rx: 0, tx: 0 }])) as Record<string, { rx: number; tx: number }>,
})

setInterval(() => {
  const rx = walk(live.rx.at(-1)!, internet.rx || 1e6)
  const tx = walk(live.tx.at(-1)!, internet.tx || 2e5)
  live.t = [...live.t.slice(1), now()]
  live.rx = [...live.rx.slice(1), rx]
  live.tx = [...live.tx.slice(1), tx]
  live.cpu = Math.round(walk(live.cpu, router.cpuLoad + 3, 0.6))
  for (const c of clients) live.rate[c.mac] = { rx: rx * share[c.mac] * (0.5 + Math.random()), tx: tx * share[c.mac] * (0.5 + Math.random()) }
}, 1000)

// ---------- local prefs (aliases live in our DB in the real app) ----------
const load = <T>(k: string, d: T): T => { try { return JSON.parse(localStorage.getItem(k) ?? '') } catch { return d } }
export const prefs = $state({
  aliases: load<Record<string, string>>('tik.aliases', {}),
  theme: (new URLSearchParams(location.search).get('theme') as 'dark' | 'light' | null) ?? load<'dark' | 'light'>('tik.theme', 'dark'),
  showRos: load<boolean>('tik.showRos', false),
  icons: load<Record<string, string>>('tik.icons', {}),
  notes: load<Record<string, string>>('tik.notes', {}),
  groups: load<{ name: string; limit: string; members: string[] }[]>('tik.groups', [{ name: 'Family', limit: '', members: [] }, { name: 'IoT', limit: '10M/10M', members: [] }]),
  hidden: load<string[]>('tik.hidden', []),
  speedtests: load<{ at: string; down: number; up: number; ping: number }[]>('tik.speedtests', []),
})
$effect.root(() => {
  $effect(() => {
    localStorage.setItem('tik.aliases', JSON.stringify(prefs.aliases))
    localStorage.setItem('tik.theme', JSON.stringify(prefs.theme))
    localStorage.setItem('tik.showRos', JSON.stringify(prefs.showRos))
    for (const k of ['icons', 'notes', 'groups', 'hidden', 'speedtests'] as const) localStorage.setItem(`tik.${k}`, JSON.stringify(prefs[k]))
    document.documentElement.dataset.theme = prefs.theme
  })
})
export const displayName = (c: { mac: string; name: string }) => prefs.aliases[c.mac] || c.name
export const iconOf = (c: { mac: string; kind: string }) => prefs.icons[c.mac] || c.kind

// ---------- Internet Activity history ----------
// ponytail: synthetic until the backend stores samples (SQLite, PLAN §2). Seeded so it doesn't reshuffle on every render.
export type Range = '1H' | '1D' | '1W'
const seeded = (seed: number) => () => ((seed = (seed * 1664525 + 1013904223) % 4294967296) / 4294967296)
const online = clients.filter((c) => c.online).length
export function activity(range: Range) {
  const [n, step] = { '1H': [60, 60], '1D': [96, 900], '1W': [168, 3600] }[range]
  const rnd = seeded(n), end = Math.floor(Date.now() / 1000 / step) * step
  const t: number[] = [], cl: number[] = [], rx: number[] = [], tx: number[] = []
  let c = online
  for (let i = 0; i < n; i++) {
    const ts = end - (n - 1 - i) * step
    const hour = new Date(ts * 1000).getHours()
    const day = hour >= 7 && hour <= 23 ? 1 : 0.55 // fewer devices overnight
    if (rnd() < 0.25) c = Math.round(Math.max(2, Math.min(online + 3, online * day + (rnd() - 0.5) * 4)))
    const spike = rnd() < 0.04 ? 8 + rnd() * 25 : 1 // occasional downloads, like a real home
    t.push(ts); cl.push(c)
    rx.push((internet.rx || 1e6) * day * (0.12 + rnd() * 0.3) * spike)
    tx.push((internet.tx || 2e5) * day * (0.1 + rnd() * 0.25) * (spike > 1 ? 2 : 1))
  }
  rx[n - 1] = live.rx.at(-1) ?? rx[n - 1]
  tx[n - 1] = live.tx.at(-1) ?? tx[n - 1]
  return { t, clients: cl, rx, tx }
}
// Internet health bar (-12h → now): 48 slots, all green unless latency samples say otherwise
export const health = Array.from({ length: 48 }, (_, i) => (i === 7 ? 'warn' : 'ok'))
