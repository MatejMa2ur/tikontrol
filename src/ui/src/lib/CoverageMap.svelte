<script lang="ts">
  // Coverage map: a floor plan saved by tikontrol (GET/PUT/DELETE /floorplan), APs placed by hand, and a predicted
  // signal heatmap from lib/coverage.ts (scale, walls, per-band AP power, measured points). Nothing reaches the router.
  import { onDestroy, onMount } from 'svelte'
  import { radios, clients, isBackend, router } from './data'
  import { api } from './backend'
  import { displayName } from './live.svelte'
  import Icon from './Icon.svelte'
  import Section from './ui/Section.svelte'
  import {
    BANDS, DEFAULT_CELL, DEFAULT_GAIN, DEFAULT_N, DEFAULT_TX, LEVELS, MATERIALS, MATERIAL_KEYS, NONE, PRESETS,
    atLeast, best, fitN, levelOf, packWalls, predict, scaleFrom, shares, upsample,
    type Band, type Grid, type Input, type Material, type Source, type Wall,
  } from './coverage'

  type BandCfg = { band: Band; tx: number; radio?: string }
  type Pin = { label: string; x: number; y: number; bands: BandCfg[]; gain?: number }
  type Scale = { x1: number; y1: number; x2: number; y2: number; meters: number; mpu: number }
  type Meas = { x: number; y: number; band: Band; dbm: number; label: string }
  type Model = { n: number; cell: number }
  type Saved = { image: string; aps: (Omit<Pin, 'bands'> & { bands?: BandCfg[] })[]; scale?: Scale; walls?: Wall[]; measurements?: Meas[]; model?: Model }
  type Pt = { x: number; y: number }

  const bandOf = (label: string): Band => (label.startsWith('2') ? '2.4' : label.startsWith('6') ? '6' : '5')
  // router names and client names are untrusted: no control or invisible format characters, short
  const clean = (s: string, max = 32) => s.replace(/[\p{Cc}\p{Cf}]/gu, '').trim().slice(0, max)
  const r1 = (v: number) => Math.round(v * 10) / 10
  const clamp = (v: number) => Math.max(0, Math.min(100, r1(v)))
  const fmt = (v: number) => (Number.isFinite(v) ? `${Math.round(v)} dBm` : 'no signal')
  const pctTxt = (v: number) => `${Math.round(v * 100)} %`

  /** The router's own radios, power prefilled (first pin), or the usual defaults for an extra AP. */
  function defaultBands(own: boolean): BandCfg[] {
    if (own && radios.length) {
      const seen = new Set<Band>()
      return radios.flatMap((r) => { const b = bandOf(r.band); if (seen.has(b)) return []; seen.add(b); return [{ band: b, tx: r.txPower ?? DEFAULT_TX[b], radio: clean(r.name) || undefined }] })
    }
    return [{ band: '2.4', tx: DEFAULT_TX['2.4'] }, { band: '5', tx: DEFAULT_TX['5'] }]
  }

  let plan = $state<string | null>(null)
  let pins = $state<Pin[]>([])
  let scale = $state<Scale | null>(null)
  let walls = $state<Wall[]>([])
  let meas = $state<Meas[]>([])
  let model = $state<Model>({ n: DEFAULT_N, cell: DEFAULT_CELL })
  let aspect = $state(0) // image height / width, from the loaded image
  let planMsg = $state('')

  if (isBackend) api<Saved | null>('/floorplan').then((f) => {
    if (!f) return
    plan = f.image
    pins = f.aps.map((p) => ({ ...p, bands: p.bands ?? defaultBands(false) })) // plans saved before the RF model
    scale = f.scale ?? null; walls = f.walls ?? []; meas = f.measurements ?? []; model = f.model ?? { n: DEFAULT_N, cell: DEFAULT_CELL }
  }).catch(() => {})

  // scale down in the browser so the upload stays under the server's limit (~200 KB)
  async function load(e: Event) {
    const f = (e.target as HTMLInputElement).files?.[0]
    if (!f) return
    const img = await createImageBitmap(f)
    const k = Math.min(1, 1280 / Math.max(img.width, img.height))
    const cv = document.createElement('canvas')
    cv.width = Math.round(img.width * k); cv.height = Math.round(img.height * k)
    cv.getContext('2d')!.drawImage(img, 0, 0, cv.width, cv.height)
    let q = 0.8, url = cv.toDataURL('image/jpeg', q)
    while (url.length > 190_000 && q > 0.2) url = cv.toDataURL('image/jpeg', (q -= 0.15))
    if (url.length > 190_000) { planMsg = 'Image too detailed: crop it or use a simpler plan'; return }
    plan = url; planMsg = ''; scale = null; walls = []; meas = []
    if (!pins.length) pins = radios.length ? [{ label: clean(router.identity ?? '') || 'AP', x: 50, y: 50, bands: defaultBands(true) }] : []
  }

  // ---------- tools: move (APs, markers, select walls), scale line, walls, measurements ----------
  type Tool = 'move' | 'scale' | 'wall' | 'measure'
  const TOOLS: { k: Tool; label: string }[] = [{ k: 'move', label: 'Move' }, { k: 'scale', label: 'Set scale' }, { k: 'wall', label: 'Draw walls' }, { k: 'measure', label: 'Measure' }]
  let tool = $state<Tool>('move')
  let material = $state<Material>('drywall')
  let surface = $state<HTMLDivElement>()
  let drag = $state<{ kind: 'ap' | 'm'; i: number } | null>(null)
  let hover = $state<Pt | null>(null)
  let sel = $state(-1) // selected wall
  let wallStart = $state<Pt | null>(null)
  let scaleStart = $state<Pt | null>(null)
  let refLine = $state<{ x1: number; y1: number; x2: number; y2: number } | null>(null)
  let refMeters = $state<number | null>(null)
  let refInput = $state<HTMLInputElement>()
  let pending = $state<Pt | null>(null)
  let draft = $state({ client: '', band: '5' as Band, dbm: -60 as number | null, label: '' })
  let widthInput = $state<number | null>(null)

  function setTool(t: Tool) { tool = t; wallStart = null; scaleStart = null; if (t !== 'measure') pending = null }

  function at(e: PointerEvent | MouseEvent): Pt {
    const r = surface!.getBoundingClientRect()
    return { x: clamp(((e.clientX - r.left) / r.width) * 100), y: clamp(((e.clientY - r.top) / r.height) * 100) }
  }
  // wall ends snap to existing ends nearby (closed rooms); Shift keeps the wall straight
  function snap(p: Pt, e: PointerEvent | MouseEvent): Pt {
    if (wallStart && e.shiftKey) p = Math.abs(p.x - wallStart.x) >= Math.abs(p.y - wallStart.y) * aspect ? { x: p.x, y: wallStart.y } : { x: wallStart.x, y: p.y }
    let d = 1.5, q = p
    for (const w of walls) for (const [x, y] of [[w.x1, w.y1], [w.x2, w.y2]]) { const k = Math.hypot(x - p.x, (y - p.y) * aspect); if (k < d) { d = k; q = { x, y } } }
    return q
  }
  function down(e: PointerEvent) {
    if ((e.target as HTMLElement).closest('button')) return
    const p = at(e)
    if (tool === 'scale') {
      if (!scaleStart) { scaleStart = p; refLine = null; return }
      if (p.x === scaleStart.x && p.y === scaleStart.y) return
      refLine = { x1: scaleStart.x, y1: scaleStart.y, x2: p.x, y2: p.y }; scaleStart = null; refMeters = null
      queueMicrotask(() => refInput?.focus())
    } else if (tool === 'wall') {
      const q = snap(p, e)
      if (wallStart && (q.x !== wallStart.x || q.y !== wallStart.y)) {
        if (walls.length >= 500) { planMsg = 'At most 500 walls'; return }
        walls = [...walls, { x1: wallStart.x, y1: wallStart.y, x2: q.x, y2: q.y, m: material }]
      }
      wallStart = q
    } else if (tool === 'measure') pending = p
    else sel = -1
  }
  function move(e: PointerEvent) {
    hover = at(e)
    if (!drag) return
    if (drag.kind === 'ap') pins[drag.i] = { ...pins[drag.i], ...hover }
    else meas[drag.i] = { ...meas[drag.i], ...hover }
  }
  function nudge(e: KeyboardEvent, list: Pt[], i: number) {
    const d = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[e.key]
    if (!d) return
    e.preventDefault()
    list[i] = { ...list[i], x: clamp(list[i].x + d[0]), y: clamp(list[i].y + d[1]) }
  }
  function key(e: KeyboardEvent) {
    if (e.key === 'Escape') { wallStart = null; scaleStart = null; pending = null }
    const t = e.target as HTMLElement
    if ((e.key === 'Delete' || e.key === 'Backspace') && sel >= 0 && !t.closest('input, select, textarea')) { e.preventDefault(); delWall(sel) }
  }
  function delWall(i: number) { walls = walls.filter((_, j) => j !== i); sel = -1 }

  function applyRef() {
    const mpu = refLine && refMeters ? scaleFrom(refLine, refMeters, aspect) : null
    if (!refLine || !mpu || !(refMeters! >= 0.1 && refMeters! <= 2000)) { planMsg = 'Enter the real length of the line, 0.1-2000 m'; return }
    scale = { ...refLine, meters: refMeters!, mpu }; refLine = null; planMsg = ''; setTool('move')
  }
  function applyWidth() {
    if (!widthInput || widthInput < 1 || widthInput > 2000) { planMsg = 'Enter the plan width, 1-2000 m'; return }
    scale = { x1: 0, y1: 0, x2: 100, y2: 0, meters: widthInput, mpu: widthInput / 100 }; planMsg = ''
  }

  function pickClient(mac: string) {
    const c = clients.find((x) => x.mac === mac)
    if (!c) return
    draft = { client: mac, band: bandOf(c.band ?? '5'), dbm: c.signal ?? null, label: clean(displayName(c)) }
  }
  function record() {
    if (!pending || draft.dbm == null || !(draft.dbm >= -120 && draft.dbm <= 0)) { planMsg = 'Enter a signal of -120 to 0 dBm'; return }
    if (meas.length >= 200) { planMsg = 'At most 200 measurements'; return }
    meas = [...meas, { ...pending, band: draft.band, dbm: Math.round(draft.dbm), label: clean(draft.label) }]
    pending = null; planMsg = ''
  }

  // ---------- prediction ----------
  let band = $state<Band>('5')
  let opacity = $state(0.55)
  let grid = $state.raw<Grid | null>(null)
  let busy = $state(false)
  const mpu = $derived(scale?.mpu ?? 0)
  const nEff = $derived(Math.min(4, Math.max(1.5, +model.n || DEFAULT_N))) // an emptied or out-of-range field still predicts
  const ready = $derived(!!plan && mpu > 0 && aspect > 0)
  const sourcesFor = (b: Band) => pins.flatMap((p, i) => {
    const c = p.bands.find((x) => x.band === b)
    return c ? [{ i, s: { x: p.x * mpu, y: p.y * aspect * mpu, tx: c.tx, gain: p.gain ?? DEFAULT_GAIN } as Source }] : []
  })
  const src = $derived(sourcesFor(band))
  const packed = $derived(packWalls(walls, band, mpu, aspect))

  let worker: Worker | null = null
  let job = 0
  function run(input: Input) {
    const id = ++job
    if (busy && worker) { worker.terminate(); worker = null } // a newer layout replaces an unfinished one
    busy = true
    try {
      if (!worker) {
        worker = new Worker(new URL('./coverage.worker.ts', import.meta.url), { type: 'module' })
        worker.onmessage = (e: MessageEvent<{ id: number; grid: Grid }>) => { if (e.data.id === job) { grid = e.data.grid; busy = false } }
        worker.onerror = () => { worker?.terminate(); worker = null; grid = predict(input); busy = false }
      }
      worker.postMessage({ id, input })
    } catch { grid = predict(input); busy = false }
  }
  $effect(() => {
    if (!ready) { grid = null; return }
    const input: Input = { mpu, aspect, band, n: nEff, cell: model.cell, sources: src.map((x) => x.s), walls: $state.snapshot(walls) }
    const t = setTimeout(() => run(input), 120)
    return () => clearTimeout(t)
  })
  onDestroy(() => worker?.terminate())

  // colours follow the theme: excellent = ok, good = halfway to warn, fair = warn, poor = bad, none = clear
  let canvas = $state<HTMLCanvasElement>()
  let themeTick = $state(0)
  onMount(() => {
    const mo = new MutationObserver(() => themeTick++)
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'data-palette', 'style'] })
    return () => mo.disconnect()
  })
  function rgb(name: string): number[] {
    const h = getComputedStyle(document.documentElement).getPropertyValue(name).trim().replace('#', '')
    return /^[0-9a-f]{6}$/i.test(h) ? [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16)) : [128, 128, 128]
  }
  $effect(() => {
    void themeTick
    const g = grid, cv = canvas
    if (!g || !cv) return
    const ok = rgb('--ok'), warn = rgb('--warn'), bad = rgb('--bad')
    const colours = [ok, ok.map((v, i) => Math.round((v + warn[i]) / 2)), warn, bad]
    const u = upsample(g, Math.max(1, Math.min(6, Math.floor(1200 / g.cols)))) // smooth level boundaries
    cv.width = u.cols; cv.height = u.rows
    const ctx = cv.getContext('2d')
    if (!ctx) return
    const img = ctx.createImageData(u.cols, u.rows)
    u.dbm.forEach((v, i) => { const l = levelOf(v); if (l < NONE) img.data.set([...colours[l], 255], i * 4) })
    ctx.putImageData(img, 0, 0)
  })

  const share = $derived(grid ? shares(grid) : null)
  const goodShare = $derived(grid ? atLeast(grid, -67) : 0)
  const readout = $derived.by(() => {
    if (!hover || !ready || !src.length) return null
    const b = best(src.map((x) => x.s), hover.x * mpu, hover.y * aspect * mpu, packed, { band, n: nEff })
    return { dbm: b.dbm, level: levelOf(b.dbm), from: pins[src[b.ap]?.i]?.label ?? '' }
  })
  const predictedAt = (m: Meas) => {
    const s = sourcesFor(m.band)
    if (!ready || !s.length) return null
    return best(s.map((x) => x.s), m.x * mpu, m.y * aspect * mpu, packWalls(walls, m.band, mpu, aspect), { band: m.band, n: nEff }).dbm
  }
  const compared = $derived(meas.map((m) => ({ m, p: predictedAt(m) })))
  let fitMsg = $state('')
  function fit() {
    const r = fitN(meas.map((m) => ({ x: m.x * mpu, y: m.y * aspect * mpu, dbm: m.dbm, band: m.band, sources: sourcesFor(m.band).map((x) => x.s), w: packWalls(walls, m.band, mpu, aspect) })))
    if (!r) { fitMsg = 'Record a measurement near an AP on one of its bands first'; return }
    model.n = Math.min(4, Math.max(1.5, r.n)); fitMsg = `n = ${r.n.toFixed(2)} fits best, ±${r.rmse.toFixed(1)} dB on average (RMS)`
  }

  // ---------- save ----------
  async function savePlan() {
    const body = {
      image: plan,
      aps: pins.map((p) => ({ ...p, label: clean(p.label) || 'AP', bands: p.bands.map((b) => ({ band: b.band, tx: r1(b.tx), ...(b.radio ? { radio: clean(b.radio) } : {}) })) })),
      ...(scale ? { scale } : {}), walls, measurements: meas, model: { n: nEff, cell: model.cell },
    }
    if (!isBackend) { planMsg = 'Saved in this browser only (no server)'; return }
    if (JSON.stringify(body).length > 255_000) { planMsg = 'Too much to store: use a smaller plan image or fewer walls'; return }
    try { await api('/floorplan', { method: 'PUT', body }); planMsg = 'Saved' } catch (e) { planMsg = (e as Error).message }
  }
  async function dropPlan() {
    if (isBackend) await api('/floorplan', { method: 'DELETE' }).catch(() => {})
    plan = null; pins = []; scale = null; walls = []; meas = []; planMsg = ''; grid = null; aspect = 0
  }
  const toggleBand = (p: Pin, b: Band, on: boolean) => { p.bands = on ? [...p.bands, { band: b, tx: DEFAULT_TX[b] }].sort((x, y) => BANDS.indexOf(x.band) - BANDS.indexOf(y.band)) : p.bands.filter((x) => x.band !== b) }
  function link(c: BandCfg, name: string) {
    const r = radios.find((x) => x.name === name)
    c.radio = name ? clean(name) : undefined
    if (r?.txPower != null) c.tx = r.txPower
  }
  const wifi = clients.filter((c) => !c.wired && c.signal != null)
  const mName = (m: Material) => MATERIALS[m].label
  const wallLen = (w: Wall) => (mpu ? `${(Math.hypot(w.x2 - w.x1, (w.y2 - w.y1) * aspect) * mpu).toFixed(1)} m` : '')
</script>

<svelte:window onkeydown={key} />

<Section title="Coverage map" desc="Upload a floor plan, place your access points, set the scale and draw the walls to see a predicted signal map. It is an estimate from a simple model: check it with a few measured points.">
  {#if plan}
    <div class="row wrap tools" role="group" aria-label="Plan tool">
      {#each TOOLS as t}<button class="btn small" aria-pressed={tool === t.k} onclick={() => setTool(t.k)}>{t.label}</button>{/each}
      {#if tool === 'wall'}
        <label class="row small">Material
          <select bind:value={material} aria-label="New wall material">{#each MATERIAL_KEYS as m}<option value={m}>{mName(m)} (~{MATERIALS[m].loss[band]} dB)</option>{/each}</select>
        </label>
        {#if wallStart}<button class="btn small" onclick={() => (wallStart = null)}>Finish line</button>{/if}
        <button class="btn small" disabled={!walls.length} onclick={() => delWall(walls.length - 1)}>Undo wall</button>
      {/if}
    </div>
    <p class="muted small hint">
      {#if tool === 'scale'}{scaleStart ? 'Click the other end of something you know the length of.' : 'Click both ends of something you know the length of, such as a wall or a door.'}
      {:else if tool === 'wall'}{wallStart ? 'Click to add the next corner. Double-click, Escape or Finish line to stop. Shift keeps it straight; ends snap together.' : 'Click where a wall starts.'}
      {:else if tool === 'measure'}Stand somewhere, click that spot on the plan, then record the signal your device reports there.
      {:else}Drag the access points (or focus one and use the arrow keys). Click a wall to select it; Delete removes it.{/if}
    </p>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="plan tool-{tool}" bind:this={surface} onpointerdown={down} onpointermove={move} onpointerup={() => (drag = null)} onpointerleave={() => { drag = null; hover = null }} ondblclick={() => (wallStart = null)}>
      <img src={plan} alt="Floor plan" draggable="false" onload={(e) => { const i = e.currentTarget as HTMLImageElement; aspect = i.naturalHeight / i.naturalWidth || 0.75 }} />
      {#if grid}<canvas bind:this={canvas} aria-hidden="true" style="opacity:{opacity};width:{(grid.cols * grid.cell / grid.widthM) * 100}%;height:{(grid.rows * grid.cell / grid.heightM) * 100}%"></canvas>{/if}
      <svg viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
        {#each walls as w, i}
          <line class="halo" x1={w.x1} y1={w.y1} x2={w.x2} y2={w.y2} stroke-width={2 + MATERIALS[w.m].loss['2.4'] / 4 + 3} />
          <line class="wall m-{w.m}" class:sel={sel === i} x1={w.x1} y1={w.y1} x2={w.x2} y2={w.y2} stroke-width={2 + MATERIALS[w.m].loss['2.4'] / 4} />
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <line class="hit" x1={w.x1} y1={w.y1} x2={w.x2} y2={w.y2} onclick={() => { if (tool === 'move') sel = i }} />
        {/each}
        {#if scale && tool === 'scale' && !refLine}<line class="ref" x1={scale.x1} y1={scale.y1} x2={scale.x2} y2={scale.y2} />{/if}
        {#if refLine}<line class="ref" x1={refLine.x1} y1={refLine.y1} x2={refLine.x2} y2={refLine.y2} />{/if}
        {#if scaleStart && hover}<line class="ref" x1={scaleStart.x} y1={scaleStart.y} x2={hover.x} y2={hover.y} />{/if}
        {#if wallStart && hover}<line class="draft" x1={wallStart.x} y1={wallStart.y} x2={hover.x} y2={hover.y} />{/if}
      </svg>
      {#each meas as m, i}
        {@const p = compared[i]?.p}
        <button class="meas" style="left:{m.x}%;top:{m.y}%" title="{m.label || 'Measured'}: {m.dbm} dBm measured{p != null ? `, ${fmt(p)} predicted` : ''} ({m.band} GHz)" aria-label="Measurement {i + 1}, {m.dbm} dBm on {m.band} GHz: drag, or use the arrow keys to move" onpointerdown={(e) => { e.preventDefault(); if (tool === 'move' || tool === 'measure') drag = { kind: 'm', i } }} onkeydown={(e) => nudge(e, meas, i)}>{m.dbm}</button>
      {/each}
      {#each pins as p, i}
        <button class="ap" style="left:{p.x}%;top:{p.y}%" title={p.label} aria-label="{p.label}: drag, or use the arrow keys to move" onpointerdown={(e) => { e.preventDefault(); drag = { kind: 'ap', i } }} onkeydown={(e) => nudge(e, pins, i)}><Icon name="wifi" size={14} /></button>
      {/each}
      {#if pending}<span class="pending" style="left:{pending.x}%;top:{pending.y}%" aria-hidden="true"></span>{/if}
    </div>

    {#if refLine}
      <div class="row wrap form">
        <label class="row small">Real length of that line <input type="number" min="0.1" max="2000" step="0.1" bind:value={refMeters} bind:this={refInput} onkeydown={(e) => e.key === 'Enter' && applyRef()} /> m</label>
        <button class="btn small primary" onclick={applyRef}>Set scale</button>
        <button class="btn small" onclick={() => (refLine = null)}>Cancel</button>
      </div>
    {/if}
    {#if pending}
      <div class="row wrap form">
        <label class="row small">Client
          <select value={draft.client} onchange={(e) => pickClient(e.currentTarget.value)}>
            <option value="">Type it in…</option>
            {#each wifi as c}<option value={c.mac}>{displayName(c)} · {c.band} · {c.signal} dBm</option>{/each}
          </select>
        </label>
        <label class="row small">Band <select bind:value={draft.band}>{#each BANDS as b}<option value={b}>{b} GHz</option>{/each}</select></label>
        <label class="row small">Signal <input type="number" min="-120" max="0" step="1" bind:value={draft.dbm} /> dBm</label>
        <label class="row small">Note <input type="text" maxlength="32" bind:value={draft.label} /></label>
        <button class="btn small primary" onclick={record}>Record</button>
        <button class="btn small" onclick={() => (pending = null)}>Cancel</button>
      </div>
      <p class="muted small">The client's signal comes from the router's last reading; walk there with it, wait a few seconds and reload to refresh it.</p>
    {/if}

    {#if hover && readout}
      <p class="small readout">At the cursor: <b>{fmt(readout.dbm)}</b> on {band} GHz · {readout.level < NONE ? LEVELS[readout.level].label : 'No coverage'}{readout.from ? ` · from ${readout.from}` : ''}</p>
    {/if}

    <div class="row wrap tools">
      <div class="row" role="group" aria-label="Band">
        {#each BANDS as b}<button class="btn small" aria-pressed={band === b} onclick={() => (band = b)}>{b} GHz</button>{/each}
      </div>
      <label class="row small">Heatmap opacity <input type="range" min="0.1" max="0.9" step="0.05" bind:value={opacity} /></label>
      {#if busy}<span class="muted small">Calculating…</span>{/if}
    </div>
    <ul class="legend small" aria-label="Signal legend">
      {#each LEVELS as l, i}<li><i class="sw l{i}"></i>{l.label} {i ? `${l.min} to ${LEVELS[i - 1].min - 1}` : `${l.min} or better`} dBm</li>{/each}
      <li><i class="sw none"></i>No colour: below {LEVELS.at(-1)!.min} dBm</li>
    </ul>

    {#if !scale}
      <p class="small note">Set the scale to see the predicted signal: use <b>Set scale</b> on something you know the length of, or type the plan's width.</p>
    {:else if grid && share}
      <p class="small summary" aria-live="polite">
        On {band} GHz, <b>{pctTxt(goodShare)}</b> of the plan is predicted at −67 dBm or better
        ({LEVELS.map((l, i) => `${l.label.toLowerCase()} ${pctTxt(share[i])}`).join(', ')}, none {pctTxt(share[NONE])}).
        {#if !src.length}No access point uses {band} GHz.{/if}
        <span class="muted">Grid {grid.cell.toFixed(2)} m over {grid.widthM.toFixed(1)} × {grid.heightM.toFixed(1)} m, n = {nEff}.</span>
      </p>
    {/if}

    <details open>
      <summary>Scale and model</summary>
      <div class="row wrap form">
        <span class="small">{scale ? `Scale: ${scale.meters} m reference, plan ${(100 * scale.mpu).toFixed(1)} m wide` : 'No scale yet'}</span>
        <label class="row small">Plan width <input type="number" min="1" max="2000" step="0.1" bind:value={widthInput} placeholder={scale ? (100 * scale.mpu).toFixed(1) : ''} /> m</label>
        <button class="btn small" onclick={applyWidth}>Use width</button>
      </div>
      <div class="row wrap form">
        <label class="row small">Building
          <select value={PRESETS.find((p) => p.n === model.n)?.n ?? ''} onchange={(e) => { const v = +e.currentTarget.value; if (v) model.n = v }}>
            {#each PRESETS as p}<option value={p.n}>{p.label} (n = {p.n})</option>{/each}
            {#if !PRESETS.some((p) => p.n === model.n)}<option value="">Custom</option>{/if}
          </select>
        </label>
        <label class="row small">Path-loss exponent <input type="number" min="1.5" max="4" step="0.05" bind:value={model.n} /></label>
        <label class="row small">Grid <select bind:value={model.cell}>{#each [0.25, 0.5, 1, 2] as c}<option value={c}>{c} m</option>{/each}</select></label>
      </div>
      <p class="muted small">Model: received signal = transmit power + antenna gain − free-space loss at 1 m − 10·n·log10(distance) − wall losses. Wall losses are typical approximate values per crossing (a little higher at 5 and 6 GHz); real walls vary. Bigger plans use a coarser grid to stay fast.</p>
    </details>

    <details open>
      <summary>Access points ({pins.length})</summary>
      {#each pins as p, i}
        <div class="apcfg">
          <span class="row"><input type="text" bind:value={p.label} maxlength="32" aria-label="Access point {i + 1} name" /><button class="btn small" onclick={() => (pins = pins.filter((_, j) => j !== i))} aria-label="Remove {p.label}">×</button></span>
          {#each BANDS as b}
            {@const c = p.bands.find((x) => x.band === b)}
            <span class="row bandcfg">
              <label class="row small"><input type="checkbox" checked={!!c} onchange={(e) => toggleBand(p, b, e.currentTarget.checked)} /> {b} GHz</label>
              {#if c}
                <label class="row small"><span class="sr">{p.label} {b} GHz transmit power</span><input type="number" class="num" min="0" max="36" step="1" bind:value={c.tx} /> dBm</label>
                {#if radios.some((r) => bandOf(r.band) === b)}
                  <select class="small" aria-label="{p.label} {b} GHz: router radio" value={c.radio ?? ''} onchange={(e) => link(c, e.currentTarget.value)}>
                    <option value="">No radio linked</option>
                    {#each radios.filter((r) => bandOf(r.band) === b) as r}<option value={r.name}>{r.name}{r.txPower != null ? ` (${r.txPower} dBm)` : ''}</option>{/each}
                  </select>
                {/if}
              {/if}
            </span>
          {/each}
          <label class="row small">Antenna <input type="number" class="num" min="-5" max="20" step="0.5" value={p.gain ?? DEFAULT_GAIN} onchange={(e) => (p.gain = +e.currentTarget.value)} /> dBi</label>
        </div>
      {/each}
    </details>

    <details open={walls.length > 0}>
      <summary>Walls ({walls.length})</summary>
      {#if walls.length}
        <ul class="list">
          {#each walls as w, i}
            <li class="row" class:sel={sel === i}>
              <span class="small">Wall {i + 1}{wallLen(w) ? `, ${wallLen(w)}` : ''}</span>
              <select class="small" aria-label="Wall {i + 1} material" bind:value={w.m}>{#each MATERIAL_KEYS as m}<option value={m}>{mName(m)}</option>{/each}</select>
              <button class="btn small" aria-label="Delete wall {i + 1}" onclick={() => delWall(i)}><Icon name="trash" size={13} /></button>
            </li>
          {/each}
        </ul>
      {:else}<p class="muted small">No walls yet: use Draw walls.</p>{/if}
    </details>

    <details open={meas.length > 0}>
      <summary>Measured vs predicted ({meas.length})</summary>
      {#if meas.length}
        <div class="scroll"><table class="small">
          <thead><tr><th>Where</th><th>Band</th><th>Measured</th><th>Predicted</th><th>Difference</th><th><span class="sr">Delete</span></th></tr></thead>
          <tbody>
            {#each compared as { m, p }, i}
              <tr><td>{i + 1}. {m.label || 'Measured point'}</td><td>{m.band} GHz</td><td>{m.dbm} dBm</td><td>{p != null ? fmt(p) : '—'}</td><td>{p != null && Number.isFinite(p) ? `${m.dbm - Math.round(p) > 0 ? '+' : ''}${m.dbm - Math.round(p)} dB` : '—'}</td>
                <td><button class="btn small" aria-label="Delete measurement {i + 1}" onclick={() => (meas = meas.filter((_, j) => j !== i))}><Icon name="trash" size={13} /></button></td></tr>
            {/each}
          </tbody>
        </table></div>
        <div class="row wrap form"><button class="btn small" disabled={!scale} onclick={fit}>Fit n to measurements</button><span class="muted small">{fitMsg}</span></div>
      {:else}<p class="muted small">None yet: use Measure to compare the prediction with what a device really sees.</p>{/if}
    </details>

    <div class="row wrap">
      <button class="btn" disabled={pins.length >= 32} onclick={() => (pins = [...pins, { label: `AP ${pins.length + 1}`, x: 50, y: 50, bands: defaultBands(false) }])}><Icon name="plus" size={14} /> Add AP</button>
      <button class="btn primary" onclick={savePlan}>Save</button>
      <button class="btn ghost" onclick={dropPlan}>Remove plan</button>
      <span class="muted small" role="status">{planMsg}</span>
    </div>
  {:else}
    <label class="btn"><input type="file" accept="image/png,image/jpeg,image/webp" onchange={load} hidden /> Upload floor plan</label>
    {#if planMsg}<p class="muted small" role="status">{planMsg}</p>{/if}
  {/if}
</Section>

<style>
  .tools { gap: 8px; margin-bottom: 8px; align-items: center; }
  .tools [aria-pressed='true'] { background: var(--accent-bg); border-color: var(--accent-bg); color: #fff; }
  .hint { margin: 0 0 8px; }
  .plan { position: relative; touch-action: none; user-select: none; margin-bottom: 8px; overflow: hidden; border-radius: 8px; }
  .plan img { width: 100%; display: block; }
  .tool-scale, .tool-wall, .tool-measure { cursor: crosshair; }
  canvas { position: absolute; left: 0; top: 0; pointer-events: none; }
  svg { position: absolute; inset: 0; width: 100%; height: 100%; overflow: visible; }
  svg line { vector-effect: non-scaling-stroke; stroke-linecap: round; fill: none; pointer-events: none; }
  .halo { stroke: var(--panel); opacity: .85; }
  .wall { stroke: var(--text); }
  .wall.m-glass { stroke-dasharray: 6 4; }
  .wall.sel { stroke: var(--accent); }
  .hit { stroke: transparent; stroke-width: 14; }
  .tool-move .hit { pointer-events: stroke; cursor: pointer; }
  .ref { stroke: var(--accent); stroke-width: 3; stroke-dasharray: 8 5; }
  .draft { stroke: var(--accent); stroke-width: 3; }
  .ap, .meas { position: absolute; transform: translate(-50%, -50%); padding: 0; border: 0; display: grid; place-items: center; }
  .ap { width: 28px; height: 28px; border-radius: 50%; cursor: grab; background: var(--accent-bg); color: #fff; box-shadow: 0 0 0 2px var(--panel); }
  .meas { min-width: 30px; height: 20px; padding: 0 4px; border-radius: 4px; font: 600 10.5px var(--font); background: var(--panel); color: var(--text); border: 1.5px solid var(--text); cursor: grab; }
  .tool-wall .ap, .tool-scale .ap, .tool-wall .meas, .tool-scale .meas { pointer-events: none; }
  .ap:focus-visible, .meas:focus-visible { outline: 2px solid var(--text); outline-offset: 2px; }
  .pending { position: absolute; width: 14px; height: 14px; transform: translate(-50%, -50%); border-radius: 50%; border: 3px solid var(--accent); background: var(--panel); }
  .form { gap: 8px; margin: 6px 0; align-items: center; }
  .form input[type='number'] { width: 80px; }
  .readout { margin: 0 0 6px; }
  .legend { display: flex; flex-wrap: wrap; gap: 4px 14px; list-style: none; padding: 0; margin: 4px 0 8px; }
  .legend li { display: flex; align-items: center; gap: 6px; }
  .sw { width: 14px; height: 14px; border-radius: 3px; border: 1px solid var(--text); display: inline-block; }
  .l0 { background: var(--ok); } .l1 { background: color-mix(in srgb, var(--ok) 50%, var(--warn)); } .l2 { background: var(--warn); } .l3 { background: var(--bad); }
  .sw.none { background: transparent; border-style: dashed; }
  .summary, .note { margin: 0 0 10px; }
  details { border-top: 1px solid var(--line); padding: 8px 0; }
  summary { cursor: pointer; font-weight: 600; font-size: 12.5px; }
  .apcfg { display: flex; flex-wrap: wrap; gap: 6px 14px; align-items: center; padding: 8px 0; border-top: 1px dashed var(--line); }
  .apcfg:first-of-type { border-top: 0; }
  .apcfg input[type='text'] { width: 130px; }
  .bandcfg { gap: 6px; }
  .num { width: 64px; }
  .list { list-style: none; padding: 0; margin: 6px 0; display: grid; gap: 4px; }
  .list li { gap: 8px; }
  .list li.sel { outline: 2px solid var(--accent); border-radius: 6px; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .scroll { overflow-x: auto; }
  table { width: 100%; margin-top: 6px; }
  .small { font-size: 12px; }
</style>
