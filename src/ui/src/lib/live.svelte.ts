// Live state. With the tikontrol server the numbers arrive over Server-Sent Events (/api/v1/live)
// and history comes from its store; in the mockup they are a random walk around the snapshot values.
import { clients, internet, router, isBackend } from './data'
import { api } from './backend'
import { applyTheme, designs, palettes, DEFAULT_DESIGN, DEFAULT_PALETTE } from './themes'

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
  wanUp: true,
})

function push(rx: number, tx: number, t = now()) {
  live.t = [...live.t.slice(1), t]
  live.rx = [...live.rx.slice(1), rx]
  live.tx = [...live.tx.slice(1), tx]
}

if (isBackend) {
  live.rx = live.rx.map(() => 0)
  live.tx = live.tx.map(() => 0)
  const es = new EventSource('/api/v1/live')
  es.onmessage = (m) => {
    try {
      const f = JSON.parse(m.data)
      push(f.rx, f.tx, f.t)
      live.cpu = f.cpu
      live.wanUp = f.wanUp
      // rates are keyed by MAC from the router: only numbers are taken from them
      for (const [mac, r] of Object.entries(f.rate ?? {}) as [string, any][]) live.rate[mac] = { rx: +r.rx || 0, tx: +r.tx || 0 }
    } catch { /* ignore a malformed frame */ }
  }
} else setInterval(() => {
  const rx = walk(live.rx.at(-1)!, internet.rx || 1e6)
  const tx = walk(live.tx.at(-1)!, internet.tx || 2e5)
  push(rx, tx)
  live.cpu = Math.round(walk(live.cpu, router.cpuLoad + 3, 0.6))
  for (const c of clients) live.rate[c.mac] = { rx: rx * share[c.mac] * (0.5 + Math.random()), tx: tx * share[c.mac] * (0.5 + Math.random()) }
}, 1000)

// ---------- local prefs (aliases live in our DB in the real app) ----------
const load = <T>(k: string, d: T): T => { try { return JSON.parse(localStorage.getItem(k) ?? '') } catch { return d } }
const q = new URLSearchParams(location.search) // ?theme= ?palette= ?design= for screenshots
export const prefs = $state({
  aliases: load<Record<string, string>>('tik.aliases', {}),
  theme: (q.get('theme') as 'dark' | 'light' | null) ?? load<'dark' | 'light'>('tik.theme', 'dark'),
  // Colour and design theme (lib/themes.ts). Unknown names from storage fall back to the defaults.
  palette: ((p: string) => (Object.hasOwn(palettes, p) ? p : DEFAULT_PALETTE))(q.get('palette') ?? load<string>('tik.palette', DEFAULT_PALETTE)),
  design: ((d: string) => (Object.hasOwn(designs, d) ? d : DEFAULT_DESIGN))(q.get('design') ?? load<string>('tik.design', DEFAULT_DESIGN)),
  showRos: load<boolean>('tik.showRos', false),
  icons: load<Record<string, string>>('tik.icons', {}),
  notes: load<Record<string, string>>('tik.notes', {}),
  groups: load<{ name: string; limit: string; members: string[] }[]>('tik.groups', [{ name: 'Family', limit: '', members: [] }, { name: 'IoT', limit: '10M/10M', members: [] }]),
  hidden: load<string[]>('tik.hidden', []),
  // dashboard layout: card ids in order, and the hidden ones (pages/Dashboard.svelte)
  cardOrder: load<string[]>('tik.cardOrder', []),
  hiddenCards: load<string[]>('tik.hiddenCards', []),
  speedtests: load<{ at: string; down: number; up: number; ping: number }[]>('tik.speedtests', []),
})
// Names, icons, notes, groups, hidden clients and the dashboard layout live on the server when there is one (shared by every
// browser and by MCP); the theme (mode, palette, design) and "show RouterOS" stay per browser, which is
// per user as long as people don't share a browser profile.
const SHARED = ['aliases', 'icons', 'notes', 'groups', 'hidden', 'cardOrder', 'hiddenCards'] as const
let loaded = !isBackend
if (isBackend) api('/prefs').then((p) => { for (const k of SHARED) if (p?.[k]) (prefs as any)[k] = p[k]; loaded = true }).catch(() => (loaded = true))
let saveTimer: ReturnType<typeof setTimeout> | undefined
$effect.root(() => {
  $effect(() => {
    localStorage.setItem('tik.theme', JSON.stringify(prefs.theme))
    localStorage.setItem('tik.palette', JSON.stringify(prefs.palette))
    localStorage.setItem('tik.design', JSON.stringify(prefs.design))
    localStorage.setItem('tik.showRos', JSON.stringify(prefs.showRos))
    localStorage.setItem('tik.speedtests', JSON.stringify(prefs.speedtests))
    applyTheme(document.documentElement, prefs.theme, prefs.palette, prefs.design)
  })
  $effect(() => {
    const shared = JSON.stringify(Object.fromEntries(SHARED.map((k) => [k, prefs[k]])))
    if (!isBackend) { for (const k of SHARED) localStorage.setItem(`tik.${k}`, JSON.stringify(prefs[k])); return }
    if (!loaded) return
    clearTimeout(saveTimer)
    saveTimer = setTimeout(() => api('/prefs', { method: 'PUT', body: JSON.parse(shared) }).catch(() => {}), 600)
  })
})
export const displayName = (c: { mac: string; name: string }) => prefs.aliases[c.mac] || c.name
export const iconOf = (c: { mac: string; kind: string }) => prefs.icons[c.mac] || c.kind

// ---------- Internet Activity history ----------
// ponytail: synthetic until the backend stores samples (SQLite, PLAN §2). Seeded so it doesn't reshuffle on every render.
export type Range = '1H' | '1D' | '1W'
type Series = { t: number[]; clients: number[]; rx: number[]; tx: number[] }
const stored = $state<Partial<Record<Range, Series>>>({})
const fetched: Partial<Record<Range, number>> = {}
function fromServer(range: Range): Series {
  if (!fetched[range] || Date.now() - fetched[range]! > 60_000) {
    fetched[range] = Date.now()
    api<Series>(`/history?range=${range}`).then((s) => (stored[range] = s)).catch(() => {})
  }
  const s = stored[range] ?? { t: [], clients: [], rx: [], tx: [] }
  if (!s.rx.length) return s
  return { ...s, rx: [...s.rx.slice(0, -1), live.rx.at(-1) ?? 0], tx: [...s.tx.slice(0, -1), live.tx.at(-1) ?? 0] }
}
const seeded = (seed: number) => () => ((seed = (seed * 1664525 + 1013904223) % 4294967296) / 4294967296)
const online = clients.filter((c) => c.online).length
export function activity(range: Range) {
  if (isBackend) return fromServer(range)
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
// Internet health bar (-12h → now): 48 slots of 15 min. From stored samples with the server ('none' = no data yet).
export const health = $state<string[]>(Array.from({ length: 48 }, (_, i) => (isBackend ? 'none' : i === 7 ? 'warn' : 'ok')))
const refreshHealth = () => api<string[]>('/health/internet').then((h) => health.splice(0, health.length, ...h)).catch(() => {})
if (isBackend) { refreshHealth(); setInterval(refreshHealth, 5 * 60_000) }
