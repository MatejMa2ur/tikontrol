<script lang="ts">
  import { radios, clients, events, signalQuality, hw } from '../lib/data'
  import { displayName } from '../lib/live.svelte'
  import { review } from '../lib/apply.svelte'
  import Icon from '../lib/Icon.svelte'
  import Section from '../lib/ui/Section.svelte'

  const wifi = clients.filter((c) => !c.wired)
  // signal histogram, 5 dB buckets from -90 to -30
  const bins = Array.from({ length: 12 }, (_, i) => -90 + i * 5)
  const hist = bins.map((b) => wifi.filter((c) => (c.signal ?? -99) >= b && (c.signal ?? -99) < b + 5).length)
  const hmax = Math.max(...hist, 1)

  // Roaming journeys from the log: each client's sequence of band joins
  const journeys = wifi.map((c) => ({
    c, steps: events.filter((e) => e.cat === 'WiFi' && e.text.startsWith(displayName(c)) && e.text.includes('joined')).map((e) => ({ t: e.time.slice(11, 16), band: e.text.match(/(2\.4|5) GHz/)?.[0] ?? '?' })).reverse(),
  })).filter((j) => j.steps.length > 1)

  let scanning = $state<string | null>(null)
  // Floor plan: local image + click to place the AP
  let plan = $state<string | null>(null)
  let ap = $state({ x: 50, y: 50 })
  const load = (e: Event) => { const f = (e.target as HTMLInputElement).files?.[0]; if (f) plan = URL.createObjectURL(f) }
</script>

<div class="grid">
  <div class="cols">
    {#each radios as r}
      <Section title="{r.band} · {r.name}" desc="SSID {r.ssid}">
        {#snippet actions()}<span class="pill {r.running ? 'ok' : ''}">{r.running ? 'Broadcasting' : 'Off'}</span>{/snippet}
        <dl>
          <dt>Channel</dt><dd>{r.channel ?? 'auto'} · {r.width ?? '—'} MHz <span class="muted">(max {r.maxWidth})</span></dd>
          <dt>TX power</dt><dd>{r.txPower ?? '—'} dBm</dd>
          <dt>Noise floor</dt><dd>{r.noise ?? '—'} dBm</dd>
          <dt>Link quality (CCQ)</dt><dd>{r.ccq ?? '—'}%</dd>
          <dt>Clients</dt><dd>{r.clients}</dd>
          <dt>Utilisation</dt><dd class="muted">{hw.wifiPackage === 'wifi' ? 'Not collected yet' : 'Not reported by the legacy wireless driver'}</dd>
        </dl>
        <div class="row">
          <button class="btn" onclick={() => (scanning = r.name)}><Icon name="search" size={14} /> Scan neighbours</button>
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

  <Section title="Roaming journeys" desc="Band and AP changes per client from the log (the Connectivity view)." ros="/log/print where topics~&quot;wireless&quot;">
    {#each journeys as j}
      <div class="row journey">
        <a href="#/clients/{j.c.mac}" class="who row"><Icon name={j.c.kind} size={14} /> {displayName(j.c)}</a>
        <div class="steps">{#each j.steps as s}<span class="step {s.band.startsWith('5') ? 'b5' : 'b2'}" title={s.t}>{s.band}<small>{s.t}</small></span>{/each}</div>
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
    <b>Neighbour scan on {r}</b>
    <p class="small muted">A scan takes the radio off-channel for ~10 s and briefly disconnects its clients. tikontrol would run it with a time limit:</p>
    <div class="row"><button class="btn" onclick={() => (scanning = null)}>Cancel</button><button class="btn primary" onclick={() => { review(`Scan ${r}`, `/interface/wireless/scan ${r} duration=10 save-file=scan-${r}`); scanning = null }}>Scan</button></div>
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
  .ap { position: absolute; transform: translate(-50%, -50%); width: 28px; height: 28px; border-radius: 50%; display: grid; place-items: center; background: var(--accent); color: #fff; box-shadow: 0 0 0 30px color-mix(in srgb, var(--accent) 15%, transparent); }
  .toastbox { position: fixed; bottom: 20px; right: 20px; z-index: 30; max-width: 380px; box-shadow: var(--shadow); }
  .small { font-size: 12px; }
  @media (max-width: 900px) { .cols { grid-template-columns: minmax(0, 1fr); } }
</style>
