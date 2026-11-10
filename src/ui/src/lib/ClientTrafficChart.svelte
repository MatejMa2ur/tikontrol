<script lang="ts" module>
  // Shape of GET /api/v1/history/clients (src/server/src/live.rs top_clients)
  export type Bucket = { t: number; down: number; up: number }
  export type ClientsHistory = { start: number; end: number; step: number; clients: { mac: string; down: number; up: number; buckets: Bucket[] }[]; other: { count: number; down: number; up: number; buckets: Bucket[] } }
  export type Win = '24h' | '7d'
</script>

<script lang="ts">
  // Traffic by client over time: stacked bars per hour (24 h) or per local day (7 d), one colour per client.
  // Names come from other devices: always rendered as text ({...}), never as HTML.
  import { bytes } from './data'

  let { data, win, name, metric = 'down' }: { data: ClientsHistory; win: Win; name: (mac: string) => string; metric?: 'down' | 'up' | 'total' } = $props()

  const SLOTS = 8
  const OTHER = 'other'
  // colour follows the client, not its rank: a client keeps its slot while it stays in the list
  let slots = new Map<string, number>()
  const colourOf = $derived.by(() => {
    const keepers = new Map([...slots].filter(([m]) => data.clients.some((c) => c.mac === m)))
    const used = new Set(keepers.values())
    for (const c of data.clients.slice(0, SLOTS)) {
      if (keepers.has(c.mac)) continue
      const free = [...Array(SLOTS).keys()].find((i) => !used.has(i))
      if (free == null) break
      keepers.set(c.mac, free); used.add(free)
    }
    slots = keepers
    return (mac: string) => (mac === OTHER || !keepers.has(mac) ? 'var(--muted)' : `var(--series-${keepers.get(mac)! + 1})`)
  })

  const val = (b: Bucket) => (metric === 'down' ? b.down : metric === 'up' ? b.up : b.down + b.up)
  const dayOf = (t: number) => { const d = new Date(t * 1000); d.setHours(0, 0, 0, 0); return d.getTime() / 1000 }
  // bucket starts: every hour of the window, or every local calendar day it touches
  const times = $derived.by(() => {
    if (win === '24h') return Array.from({ length: Math.round((data.end - data.start) / 3600) }, (_, i) => data.start + i * 3600)
    const out: number[] = []
    for (let d = dayOf(data.start); d < data.end; d = dayOf(d + 26 * 3600)) out.push(d)
    return out
  })
  const slotOf = (t: number) => (win === '24h' ? t : dayOf(t))
  // series: shown clients (biggest first, at the baseline), then everyone else as "Other"
  const series = $derived.by(() => {
    const s = data.clients.slice(0, SLOTS).map((c) => ({ key: c.mac, label: name(c.mac), buckets: c.buckets }))
    const rest = [...data.clients.slice(SLOTS).flatMap((c) => c.buckets), ...data.other.buckets]
    const n = data.other.count + Math.max(0, data.clients.length - SLOTS)
    if (n) s.push({ key: OTHER, label: `Other (${n})`, buckets: rest })
    return s.map((x) => {
      const v = new Map<number, number>()
      for (const b of x.buckets) v.set(slotOf(b.t), (v.get(slotOf(b.t)) ?? 0) + val(b))
      return { ...x, colour: colourOf(x.key), v: times.map((t) => v.get(t) ?? 0) }
    })
  })
  const totals = $derived(times.map((_, i) => series.reduce((s, x) => s + x.v[i], 0)))
  // y axis: a round step so labels read 0, 250 MB, 500 MB …
  const ticks = $derived.by(() => {
    const max = Math.max(...totals, 1)
    const raw = max / 4, mag = 10 ** Math.floor(Math.log10(raw))
    const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw)!
    return Array.from({ length: Math.ceil(max / step) + 1 }, (_, i) => i * step)
  })
  const top = $derived(ticks[ticks.length - 1] || 1)

  let w = $state(600)
  const H = 240, L = 64, R = 8, T = 8, B = 24, GAP = 2
  const pw = $derived(Math.max(w - L - R, 50))
  const band = $derived(pw / Math.max(times.length, 1))
  const bw = $derived(Math.max(2, Math.min(band * 0.72, 48)))
  const y = (v: number) => T + (H - T - B) * (1 - v / top)
  // stacked segments for one column; the top one gets 4 px rounded corners, 2 px panel gaps between
  function segs(i: number) {
    const out: { key: string; colour: string; y0: number; y1: number; last: boolean }[] = []
    let acc = 0
    for (const s of series) {
      const v = s.v[i]
      if (!v) continue
      out.push({ key: s.key, colour: s.colour, y0: y(acc), y1: y(acc + v), last: false })
      acc += v
    }
    if (out.length) out[out.length - 1].last = true
    return out
  }
  function path(x: number, y0: number, y1: number, round: boolean) {
    const h = Math.max(y0 - y1, 1), r = round ? Math.min(4, bw / 2, h) : 0
    const top = y0 - h
    return `M${x},${y0}V${top + r}${r ? `Q${x},${top} ${x + r},${top}` : ''}H${x + bw - r}${r ? `Q${x + bw},${top} ${x + bw},${top + r}` : ''}V${y0}Z`
  }
  const label = (t: number) => (win === '24h' ? new Date(t * 1000).toTimeString().slice(0, 5) : new Date(t * 1000).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric' }))
  const every = $derived(win === '24h' ? (pw < 420 ? 6 : 3) : 1)
  const longLabel = (t: number) => (win === '24h' ? `${new Date(t * 1000).toLocaleString(undefined, { weekday: 'short', hour: '2-digit', minute: '2-digit' })}–${new Date((t + 3600) * 1000).toTimeString().slice(0, 5)}` : new Date(t * 1000).toLocaleDateString(undefined, { weekday: 'long', day: 'numeric', month: 'short' }))
  const what = $derived(metric === 'down' ? 'downloaded' : metric === 'up' ? 'uploaded' : 'transferred')

  // hover and keyboard: one column at a time
  let at = $state<number | null>(null)
  function key(e: KeyboardEvent) {
    if (!times.length) return
    if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
      e.preventDefault()
      const d = e.key === 'ArrowRight' ? 1 : -1
      at = at == null ? (d > 0 ? 0 : times.length - 1) : Math.min(times.length - 1, Math.max(0, at + d))
    } else if (e.key === 'Escape') at = null
  }
  function move(e: PointerEvent) {
    const r = (e.currentTarget as SVGElement).getBoundingClientRect()
    const i = Math.floor((e.clientX - r.left - L) / band)
    at = i >= 0 && i < times.length ? i : null
  }
  const tipX = $derived(at == null ? 0 : Math.min(L + at * band + band / 2 + 12, w - 220))
</script>

<div class="chart" bind:clientWidth={w}>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <svg width={w} height={H} role="img" tabindex="0" aria-label="Bytes {what} per {win === '24h' ? 'hour' : 'day'} by client, stacked. Use the left and right arrow keys to read each {win === '24h' ? 'hour' : 'day'}; the table below has the totals."
    onpointermove={move} onpointerleave={() => (at = null)} onkeydown={key} onblur={() => (at = null)}>
    {#each ticks as v}
      <line x1={L} x2={w - R} y1={y(v)} y2={y(v)} class:base={v === 0} />
      <text x={L - 8} y={y(v) + 4} text-anchor="end">{v ? bytes(v) : '0'}</text>
    {/each}
    {#each times as t, i}
      {#if at === i}<rect class="hover" x={L + i * band} y={T} width={band} height={H - T - B} />{/if}
      {#each segs(i) as s (s.key)}
        <path d={path(L + i * band + (band - bw) / 2, s.y0 - (s.y0 < y(0) ? GAP / 2 : 0), s.y1 + GAP / 2, s.last)} fill={s.colour} />
      {/each}
      {#if i % every === 0}<text x={L + i * band + band / 2} y={H - 6} text-anchor="middle">{label(t)}</text>{/if}
    {/each}
  </svg>
  <p class="sr" aria-live="polite">{at == null ? '' : `${longLabel(times[at])}: ${series.filter((s) => s.v[at!]).map((s) => `${s.label} ${bytes(s.v[at!])}`).join(', ') || 'no traffic recorded'}`}</p>
  {#if at != null}
    <div class="tip" style="left:{tipX}px">
      <b>{longLabel(times[at])}</b>
      {#each series.filter((s) => s.v[at!]).toReversed() as s}
        <div class="row"><i class="sw" style="background:{s.colour}"></i><span class="ell">{s.label}</span><span class="mono">{bytes(s.v[at!])}</span></div>
      {:else}
        <div class="muted">No traffic recorded</div>
      {/each}
      {#if totals[at]}<div class="row sum"><span>Total</span><span class="mono">{bytes(totals[at])}</span></div>{/if}
    </div>
  {/if}
  <ul class="legend" aria-label="Clients in the chart">
    {#each series as s}<li><i class="sw" style="background:{s.colour}"></i>{s.label}</li>{/each}
  </ul>
</div>

<style>
  .chart { position: relative; }
  .sr { position: absolute; width: 1px; height: 1px; margin: 0; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  svg { display: block; overflow: visible; touch-action: pan-y; }
  svg:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: 4px; }
  line { stroke: var(--line); stroke-width: 1; }
  line.base { stroke: var(--muted); }
  text { fill: var(--muted); font-size: 11px; font-variant-numeric: tabular-nums; }
  .hover { fill: var(--panel2); }
  .tip { position: absolute; top: 8px; width: 210px; padding: 8px 10px; background: var(--panel); border: 1px solid var(--line); border-radius: 8px; box-shadow: var(--shadow-sm); font-size: 12px; pointer-events: none; display: grid; gap: 4px; z-index: 2; }
  .tip .row { display: grid; grid-template-columns: 10px minmax(0, 1fr) auto; gap: 6px; align-items: center; }
  .tip .sum { grid-template-columns: 1fr auto; border-top: 1px solid var(--line); padding-top: 4px; font-weight: 600; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sw { display: inline-block; width: 10px; height: 10px; border-radius: 3px; flex: none; }
  .legend { display: flex; flex-wrap: wrap; gap: 6px 14px; list-style: none; margin: 10px 0 0; padding: 0; font-size: 12px; color: var(--text); }
  .legend li { display: flex; align-items: center; gap: 6px; max-width: 220px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
