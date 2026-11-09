<script lang="ts">
  import { radios, clients, signalQuality, isBackend, router } from '../lib/data'
  import { api } from '../lib/backend'
  import { displayName } from '../lib/live.svelte'
  import { review } from '../lib/apply.svelte'
  import Icon from '../lib/Icon.svelte'
  import Section from '../lib/ui/Section.svelte'
  import Field from '../lib/ui/Field.svelte'
  import Modal from '../lib/ui/Modal.svelte'

  const wifi = clients.filter((c) => !c.wired)
  // signal histogram, 5 dB buckets from -90 to -30
  const bins = Array.from({ length: 12 }, (_, i) => -90 + i * 5)
  const hist = bins.map((b) => wifi.filter((c) => (c.signal ?? -99) >= b && (c.signal ?? -99) < b + 5).length)
  const hmax = Math.max(...hist, 1)

  // Roaming journeys: each client's joins from the router log, plus the roams the server saw in the
  // registration table between polls ("X roamed from wlan1 to wlan2", stored in live.rs history)
  const bandOfIf = (i: string) => radios.find((r) => r.name === i)?.band ?? i
  type Step = { t: string; iface: string; band: string }
  let stored = $state<{ t: number; mac?: string; text: string }[]>([])
  if (isBackend) api<typeof stored>('/events?cat=WiFi').then((e) => (stored = (e ?? []).filter((x) => x.mac && / roamed from \S+ to \S+$/.test(x.text)).reverse())).catch(() => {})
  const hhmm = (t: number) => new Date(t * 1000).toTimeString().slice(0, 5)
  const journeys = $derived(wifi.map((c) => {
    const steps: Step[] = c.bandLog.filter((l) => l.joined).map((l) => ({ t: l.time.slice(11, 16) || l.time, iface: l.iface, band: l.band ?? '?' }))
    for (const e of stored.filter((x) => x.mac === c.mac)) { const to = e.text.split(' ').at(-1)!; steps.push({ t: hhmm(e.t), iface: to, band: bandOfIf(to) }) }
    return { c, steps }
  }).filter((j) => j.steps.length > 1))

  // Neighbour scan: one-shot, confirmed, at most 10 s (server: POST /tools/wifi-scan, admin + write access)
  let scanning = $state<(typeof radios)[number] | null>(null)
  let scan = $state({ secs: 5, agree: false, busy: false, error: '', rows: null as Record<string, string>[] | null })
  const scanCmd = (r: (typeof radios)[number]) => `/interface/${r.pkg}/scan ${r.name} duration=${scan.secs}s`
  function openScan(r: (typeof radios)[number]) { scanning = r; scan = { secs: 5, agree: false, busy: false, error: '', rows: null } }
  async function runScan(r: (typeof radios)[number]) {
    scan.busy = true; scan.error = ''
    try { scan.rows = (await api<{ rows: Record<string, string>[] }>('/tools/wifi-scan', { method: 'POST', body: { package: r.pkg, interface: r.name, duration: scan.secs, confirm: true } })).rows }
    catch (e) { scan.error = (e as Error).message } finally { scan.busy = false }
  }

  // Coverage map: one floor plan saved by tikontrol (PUT /floorplan), APs placed by hand. No RF model.
  type Pin = { label: string; x: number; y: number }
  let plan = $state<string | null>(null)
  let pins = $state<Pin[]>([])
  let planMsg = $state('')
  if (isBackend) api<{ image: string; aps: Pin[] } | null>('/floorplan').then((f) => { if (f) { plan = f.image; pins = f.aps } }).catch(() => {})
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
    plan = url; planMsg = ''
    if (!pins.length) pins = radios.length ? [{ label: router.identity ?? 'AP', x: 50, y: 50 }] : []
  }
  let drag = $state(-1)
  const clamp = (v: number) => Math.max(0, Math.min(100, Math.round(v * 10) / 10))
  function move(e: PointerEvent) {
    if (drag < 0) return
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect()
    pins[drag] = { ...pins[drag], x: clamp(((e.clientX - r.left) / r.width) * 100), y: clamp(((e.clientY - r.top) / r.height) * 100) }
  }
  function nudge(i: number, e: KeyboardEvent) {
    const d = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[e.key]
    if (!d) return
    e.preventDefault()
    pins[i] = { ...pins[i], x: clamp(pins[i].x + d[0]), y: clamp(pins[i].y + d[1]) }
  }
  async function savePlan() {
    if (!isBackend) { planMsg = 'Saved in this browser only (no server)'; return }
    try { await api('/floorplan', { method: 'PUT', body: { image: plan, aps: pins } }); planMsg = 'Saved' } catch (e) { planMsg = (e as Error).message }
  }
  async function dropPlan() {
    if (isBackend) await api('/floorplan', { method: 'DELETE' }).catch(() => {})
    plan = null; pins = []; planMsg = ''
  }
</script>

<div class="grid">
  <div class="cols">
    {#each radios as r}
      <Section title="{r.band} · {r.name}" desc="SSID {r.ssid}" ros="/interface/{r.pkg}/monitor {r.name} once">
        {#snippet actions()}<span class="pill {r.running ? 'ok' : ''}">{r.running ? 'Broadcasting' : 'Off'}</span>{/snippet}
        <dl>
          <dt>Channel</dt><dd>{r.channel ?? '—'}{r.freq ? ` (${r.freq} MHz)` : ''} · {r.auto ? 'picked automatically' : 'fixed'}</dd>
          <dt>Width</dt><dd>{r.width ?? '—'} MHz <span class="muted">(max {r.maxWidth})</span></dd>
          {#if r.dfs != null}<dt>DFS (radar) channel</dt><dd>{#if r.dfs}<span class="pill warn">Yes</span> <span class="muted small">radar can force a move, clients drop</span>{:else}No{/if}</dd>{/if}
          {#if r.txPower != null}<dt>TX power</dt><dd>{r.txPower} dBm</dd>{/if}
          {#if r.noise != null}<dt>Noise floor</dt><dd>{r.noise} dBm</dd>{/if}
          {#if r.ccq != null}<dt>Overall CCQ</dt><dd>{r.ccq}%{#if r.ccq < 50}<span class="muted small"> · low: many retries</span>{/if}</dd>{/if}
          <dt>Clients</dt><dd>{r.clients}</dd>
          <dt>Utilisation</dt><dd class="muted small">Not reported: RouterOS's {r.pkg} monitor gives no channel busy time. Noise floor, CCQ and a neighbour scan are the closest signs of a crowded channel.</dd>
        </dl>
        <div class="row">
          <button class="btn" onclick={() => openScan(r)}><Icon name="search" size={14} /> Scan neighbours</button>
          <a class="btn" href="#/settings/wifi">Channel & power…</a>
        </div>
      </Section>
    {/each}
  </div>

  <Section title="Client signal distribution" desc="How strongly each Wi-Fi client hears the router." ros="/interface/wireless/registration-table/print">
    <div class="histo">
      {#each bins as b, i}
        {@const q = signalQuality(b + 2)}
        <div class="col"><i class={q.level} style="height:{(hist[i] / hmax) * 100}%">{hist[i] || ''}</i><span>{b}</span></div>
      {/each}
    </div>
  </Section>

  <Section title="Roaming journeys" desc="Radio and band joins per client from the router log{isBackend ? ', plus roams tikontrol saw between polls' : ''}." ros="/log/print where topics~&quot;wireless&quot;">
    {#each journeys as j}
      <div class="row journey">
        <a href="#/clients/{j.c.mac}" class="who row"><Icon name={j.c.kind} size={14} /> {displayName(j.c)}</a>
        <div class="steps">{#each j.steps as s}<span class="step {s.band.startsWith('2') ? 'b2' : 'b5'}" title="{s.iface} at {s.t}">{s.band}<small>{s.iface} · {s.t}</small></span>{/each}</div>
      </div>
    {:else}<div class="muted small">No roaming in the recent log.</div>{/each}
  </Section>

  <Section title="Coverage map" desc="Upload a floor plan and place your access points. (No RF prediction, just placement.)">
    {#if plan}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="plan" onpointermove={move} onpointerup={() => (drag = -1)} onpointerleave={() => (drag = -1)}>
        <img src={plan} alt="Floor plan" draggable="false" />
        {#each pins as p, i}
          <button class="ap" style="left:{p.x}%;top:{p.y}%" title={p.label} aria-label="{p.label}: drag, or use the arrow keys to move" onpointerdown={(e) => { e.preventDefault(); drag = i }} onkeydown={(e) => nudge(i, e)}><Icon name="wifi" size={14} /></button>
        {/each}
      </div>
      <div class="row pins">
        {#each pins as p, i}<span class="row"><input type="text" bind:value={p.label} maxlength="32" aria-label="Access point {i + 1} name" /><button class="btn small" onclick={() => (pins = pins.filter((_, j) => j !== i))} aria-label="Remove {p.label}">×</button></span>{/each}
      </div>
      <div class="row">
        <button class="btn" disabled={pins.length >= 32} onclick={() => (pins = [...pins, { label: `AP ${pins.length + 1}`, x: 50, y: 50 }])}><Icon name="plus" size={14} /> Add AP</button>
        <button class="btn primary" onclick={savePlan}>Save</button>
        <button class="btn ghost" onclick={dropPlan}>Remove plan</button>
        <span class="muted small" role="status">{planMsg}</span>
      </div>
    {:else}
      <label class="btn"><input type="file" accept="image/png,image/jpeg,image/webp" onchange={load} hidden /> Upload floor plan</label>
      {#if planMsg}<p class="muted small" role="status">{planMsg}</p>{/if}
    {/if}
  </Section>
</div>

{#if scanning}
  {@const r = scanning}
  <Modal title="Neighbour scan on {r.name}" onclose={() => (scanning = null)} wide={!!scan.rows}>
    <p class="pill warn">High risk: Wi-Fi drop</p>
    <p class="small">A scan takes the radio off-channel for its whole duration: <b>every client on {r.name} disconnects</b>, including this browser if it uses that radio. tikontrol never scans on its own (not while polling, not with the read-only user); it runs once, as the writer, when you confirm, and stops by itself.</p>
    <Field label="Duration"><select bind:value={scan.secs}><option value={5}>5 seconds</option><option value={10}>10 seconds (longest)</option></select></Field>
    <label class="chk small"><input type="checkbox" bind:checked={scan.agree} /> Clients on {r.name} may disconnect for {scan.secs} s</label>
    {#if scan.error}<p class="small bad" role="alert">{scan.error}</p>{/if}
    {#if scan.rows}
      <table class="small">
        <thead><tr><th>SSID</th><th>Address</th><th>Channel</th><th>Signal</th><th>Security</th></tr></thead>
        <tbody>{#each scan.rows as n}<tr><td>{n.ssid || '(hidden)'}</td><td>{n.address}</td><td>{n.channel ?? ''}</td><td>{n.signal ?? n['signal-strength'] ?? ''}</td><td>{n.security ?? ''}</td></tr>{:else}<tr><td colspan="5" class="muted">Nothing heard.</td></tr>{/each}</tbody>
      </table>
    {/if}
    {#snippet footer()}
      <button class="btn" onclick={() => review(`Scan ${r.name} (disconnects its clients)`, scanCmd(r))}>Show command</button>
      {#if isBackend}<button class="btn danger" disabled={!scan.agree || scan.busy} onclick={() => runScan(r)}>{scan.busy ? `Scanning ${scan.secs} s…` : 'Scan now'}</button>{/if}
    {/snippet}
  </Modal>
{/if}

<style>
  .cols { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 14px; }
  dl { display: grid; grid-template-columns: 140px 1fr; gap: 7px 10px; margin: 0 0 14px; }
  dt { color: var(--muted); } dd { margin: 0; }
  .histo { display: grid; grid-template-columns: repeat(12, 1fr); gap: 6px; height: 140px; align-items: end; }
  .col { display: flex; flex-direction: column; justify-content: flex-end; height: 100%; text-align: center; gap: 4px; }
  .col i { display: block; min-height: 2px; border-radius: 4px 4px 0 0; font: 600 11px var(--font); color: #fff; background: var(--line); }
  .col i.ok { background: var(--ok); } .col i.warn { background: var(--warn); } .col i.bad { background: var(--bad); }
  .col span { font-size: 10.5px; color: var(--muted); }
  .journey { padding: 8px 0; border-top: 1px solid var(--line); gap: 14px; }
  .journey:first-of-type { border-top: 0; }
  .who { width: 190px; color: var(--text); flex: none; }
  .steps { display: flex; gap: 4px; flex-wrap: wrap; }
  .step { display: grid; padding: 2px 8px; border-radius: 6px; font-size: 11.5px; font-weight: 600; text-align: center; }
  .step small { font-weight: 400; font-size: 10px; opacity: .8; }
  .b5 { background: color-mix(in srgb, var(--accent) 18%, transparent); color: var(--accent); }
  .b2 { background: color-mix(in srgb, var(--warn) 18%, transparent); color: var(--warn); }
  .plan { position: relative; touch-action: none; user-select: none; margin-bottom: 10px; }
  .plan img { width: 100%; border-radius: 8px; display: block; }
  .ap { position: absolute; transform: translate(-50%, -50%); width: 28px; height: 28px; padding: 0; border: 0; border-radius: 50%; display: grid; place-items: center; cursor: grab; background: var(--accent-bg); color: #fff; box-shadow: 0 0 0 30px color-mix(in srgb, var(--accent) 15%, transparent); }
  .ap:focus-visible { outline: 2px solid var(--text); outline-offset: 2px; }
  .pins { flex-wrap: wrap; gap: 8px; margin-bottom: 10px; }
  .pins input { width: 130px; }
  .chk { display: flex; gap: 6px; align-items: center; margin: 8px 0; }
  .bad { color: var(--bad); }
  table { width: 100%; margin-top: 10px; }
  .small { font-size: 12px; }
  @media (max-width: 900px) { .cols { grid-template-columns: minmax(0, 1fr); } }
</style>
