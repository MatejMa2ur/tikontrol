<script lang="ts">
  import { radios, clients, signalQuality, isBackend } from '../lib/data'
  import { api } from '../lib/backend'
  import { displayName } from '../lib/live.svelte'
  import { review } from '../lib/apply.svelte'
  import Icon from '../lib/Icon.svelte'
  import Section from '../lib/ui/Section.svelte'

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

  let scanning = $state<(typeof radios)[number] | null>(null)
  // Floor plan: local image + click to place the AP
  let plan = $state<string | null>(null)
  let ap = $state({ x: 50, y: 50 })
  const load = (e: Event) => { const f = (e.target as HTMLInputElement).files?.[0]; if (f) plan = URL.createObjectURL(f) }
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
          <button class="btn" onclick={() => (scanning = r)}><Icon name="search" size={14} /> Scan neighbours</button>
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
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="plan" onclick={(e) => { const r = (e.currentTarget as HTMLElement).getBoundingClientRect(); ap = { x: ((e.clientX - r.left) / r.width) * 100, y: ((e.clientY - r.top) / r.height) * 100 } }}>
        <img src={plan} alt="Floor plan" />
        <span class="ap" style="left:{ap.x}%;top:{ap.y}%"><Icon name="wifi" size={14} /></span>
      </div>
      <p class="muted small">Click to move the AP.</p>
    {:else}
      <label class="btn"><input type="file" accept="image/*" onchange={load} hidden /> Upload floor plan</label>
    {/if}
  </Section>
</div>

{#if scanning}
  {@const r = scanning}
  <div class="toastbox card">
    <b>Neighbour scan on {r.name}</b>
    <p class="small muted">A scan takes the radio off-channel for its whole duration: every client on {r.name} disconnects. tikontrol never runs it on its own (not while polling, not with the read-only user): copy the command and run it yourself in WinBox or SSH as an admin, when a short Wi-Fi drop is fine. It stops after 10 s.</p>
    <div class="row"><button class="btn" onclick={() => (scanning = null)}>Cancel</button><button class="btn primary" onclick={() => { review(`Scan ${r.name} (disconnects its clients)`, `/interface/${r.pkg}/scan ${r.name} duration=10`); scanning = null }}>Show command</button></div>
  </div>
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
  .plan { position: relative; cursor: crosshair; }
  .plan img { width: 100%; border-radius: 8px; display: block; }
  .ap { position: absolute; transform: translate(-50%, -50%); width: 28px; height: 28px; border-radius: 50%; display: grid; place-items: center; background: var(--accent-bg); color: #fff; box-shadow: 0 0 0 30px color-mix(in srgb, var(--accent) 15%, transparent); }
  .toastbox { position: fixed; bottom: 20px; right: 20px; z-index: 30; max-width: 380px; box-shadow: var(--shadow); }
  .small { font-size: 12px; }
  @media (max-width: 900px) { .cols { grid-template-columns: minmax(0, 1fr); } }
</style>
