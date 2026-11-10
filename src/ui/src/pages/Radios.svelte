<script lang="ts">
  import { radios, clients, signalQuality, isBackend, hw } from '../lib/data'
  import { api } from '../lib/backend'
  import { displayName } from '../lib/live.svelte'
  import { review } from '../lib/apply.svelte'
  import Icon from '../lib/Icon.svelte'
  import Section from '../lib/ui/Section.svelte'
  import Field from '../lib/ui/Field.svelte'
  import Modal from '../lib/ui/Modal.svelte'
  import CoverageMap from '../lib/CoverageMap.svelte'

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

  <Section title="Client signal distribution" desc="How strongly each Wi-Fi client hears the router." ros={hw.wifiPackage === 'wifi' ? '/interface/wifi/registration-table/print' : '/interface/wireless/registration-table/print'}>
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

  <CoverageMap />
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
  .chk { display: flex; gap: 6px; align-items: center; margin: 8px 0; }
  .bad { color: var(--bad); }
  table { width: 100%; margin-top: 10px; }
  .small { font-size: 12px; }
  @media (max-width: 900px) { .cols { grid-template-columns: minmax(0, 1fr); } }
</style>
