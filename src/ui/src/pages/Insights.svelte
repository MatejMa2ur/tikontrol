<script lang="ts">
  import { keep, kept } from '../lib/keep'
  import { clients, raw, bytes, bps, hw, router, isBackend } from '../lib/data'
  import { api } from '../lib/backend'
  import { live, displayName, iconOf, activity } from '../lib/live.svelte'
  import { review } from '../lib/apply.svelte'
  import Icon from '../lib/Icon.svelte'
  import Tabs from '../lib/ui/Tabs.svelte'
  import Section from '../lib/ui/Section.svelte'
  import ActivityChart from '../lib/ActivityChart.svelte'
  import ClientTrafficChart, { type ClientsHistory, type Win } from '../lib/ClientTrafficChart.svelte'
  import { prefs } from '../lib/live.svelte'
  import { cmr, loadCmr, type CmrApp } from '../lib/cmr.svelte'

  let { tab = 'apps' }: { tab?: string } = $props()
  const flowOn = !!raw.trafficFlow?.enabled
  // the router sends flows to tikontrol's own address (the server reports it)
  let selfIp = $state('')
  $effect(() => { if (isBackend && !flowOn) api<{ selfAddress?: string }>('/router').then((r) => (selfIp = r.selfAddress ?? '')).catch(() => {}) })

  // Traffic by client
  const byClient = [...clients].map((c) => ({ c, total: c.bytesDown + c.bytesUp })).sort((a, b) => b.total - a.total)
  const maxTotal = Math.max(...byClient.map((x) => x.total), 1)

  // Traffic by client over time: hourly bytes the server stores per client (GET /history/clients)
  let win = $state<Win>(kept<string>('insights.win', '24h') === '7d' ? '7d' : '24h')
  let metric = $state<'down' | 'up' | 'total'>('down')
  $effect(() => { keep('insights.win', win) })
  let hist = $state<ClientsHistory | null>(null)
  let histErr = $state(false)
  $effect(() => {
    if (tab !== 'clients') return
    const w = win
    if (!isBackend) { hist = simulateHistory(w); return }
    const load = () => api<ClientsHistory>(`/history/clients?window=${w}&top=8`).then((h) => { hist = h; histErr = false }).catch(() => (histErr = true))
    load()
    const i = setInterval(load, 60_000)
    return () => clearInterval(i)
  })
  // offline clients aren't in the snapshot: fall back to the user's own name for the MAC, then the MAC
  const nameOfMac = (mac: string) => { const c = clients.find((x) => x.mac === mac); return c ? displayName(c) : prefs.aliases[mac] || mac }
  // demo without the server: seeded per MAC, busier in the evening, so it doesn't reshuffle on every render
  function simulateHistory(w: Win): ClientsHistory {
    const hours = w === '24h' ? 24 : 168, end = Math.floor(Date.now() / 3_600_000) * 3600 + 3600, start = end - hours * 3600
    const rows = [...clients].map((c) => {
      let seed = [...c.mac].reduce((a, ch) => (Math.imul(a, 31) + ch.charCodeAt(0)) >>> 0, 7)
      const rnd = () => ((seed = (seed * 1664525 + 1013904223) % 4294967296) / 4294967296)
      const scale = 2e6 + rnd() * 3e8
      const buckets = Array.from({ length: hours }, (_, i) => {
        const t = start + i * 3600, h = new Date(t * 1000).getHours()
        const busy = (h >= 18 && h <= 23 ? 1.6 : h >= 8 && h < 18 ? 0.7 : 0.08) * (rnd() < 0.06 ? 5 : 0.3 + rnd())
        return { t, down: Math.round(scale * busy), up: Math.round(scale * busy * (0.05 + rnd() * 0.15)) }
      })
      return { mac: c.mac, down: buckets.reduce((s, b) => s + b.down, 0), up: buckets.reduce((s, b) => s + b.up, 0), buckets }
    }).sort((a, b) => b.down + b.up - a.down - a.up || (a.mac < b.mac ? -1 : 1))
    const rest = rows.slice(8), other = new Map<number, { t: number; down: number; up: number }>()
    for (const b of rest.flatMap((r) => r.buckets)) { const o = other.get(b.t) ?? { t: b.t, down: 0, up: 0 }; o.down += b.down; o.up += b.up; other.set(b.t, o) }
    return { start, end, step: 3600, clients: rows.slice(0, 8), other: { count: rest.length, down: rest.reduce((s, r) => s + r.down, 0), up: rest.reduce((s, r) => s + r.up, 0), buckets: [...other.values()] } }
  }
  const histTotal = $derived(hist ? hist.clients.reduce((s, c) => s + c.down + c.up, 0) + hist.other.down + hist.other.up : 0)

  // Domains: DNS cache grouped by registrable domain (last two labels; good enough for .com/.net/.dk)
  const cache: any[] = raw.dnsCache ?? []
  const base = (n: string) => n.split('.').slice(-2).join('.')
  const domains = Object.entries(cache.reduce((acc: Record<string, string[]>, d) => ((acc[base(d.name)] ??= []).push(d.name), acc), {}))
    .map(([d, names]) => ({ d, names: names as string[] })).sort((a, b) => b.names.length - a.names.length)
  let dq = $state(kept('insights.q', ''))

  // NetFlow v5 aggregates from the server's collector (TIK_FLOWS)
  type Dest = { ip: string; bytes: number; name: string | null }
  type Flows = { flows: number; since: number; geoip: boolean; countries: { country: string; bytes: number }[]; talkers: { ip: string; down: number; up: number; destinations: Dest[] }[]; destinations: { ip: string; bytes: number; name: string | null }[]; ports: { proto: string; port: number; service: string; bytes: number }[] }
  let flows = $state<Flows | null>(null)
  $effect(() => {
    if (!isBackend || (tab !== 'flows' && tab !== 'countries')) return
    const load = () => api<Flows>('/flows').then((f) => (flows = f)).catch(() => {})
    load()
    const i = setInterval(load, 15_000)
    return () => clearInterval(i)
  })
  let openTalker = $state<string | null>(null)
  const countryName = (cc: string) => { try { return cc === '??' ? 'Unknown' : new Intl.DisplayNames(undefined, { type: 'region' }).of(cc) ?? cc } catch { return cc } }
  const nameOfIp = (ip: string) => { const c = clients.find((x) => x.ip === ip); return c ? displayName(c) : ip }

  // CMR app classifier (src/server/src/cmr.rs): fleet-wide totals per application, [down, up] per period
  $effect(() => { if (tab === 'apps') loadCmr() })
  type Period = 'minute' | 'hour' | 'day' | 'week'
  let period = $state<Period>(kept('insights.period', 'hour') as Period)
  $effect(() => { keep('insights.period', period) })
  const PERIODS: [Period, string][] = [['minute', 'Last minute'], ['hour', 'Last hour'], ['day', 'Last day'], ['week', 'Last week']]
  // RouterOS drops the widest periods from its 80-column table; offer only those it sent
  const periods = $derived(PERIODS.filter(([p]) => cmr.data?.apps?.some((a) => a[p])))
  const dn = (a: CmrApp) => a[period]?.[0] ?? 0
  const up = (a: CmrApp) => a[period]?.[1] ?? 0
  const apps = $derived([...(cmr.data?.apps ?? [])].filter((a) => a[period]).sort((a, b) => dn(b) + up(b) - dn(a) - up(a)))
  const appMax = $derived(Math.max(1, ...apps.map((a) => dn(a) + up(a))))
  const byCat = $derived(Object.entries(apps.reduce((m: Record<string, number>, a) => ((m[a.category ?? 'Unclassified'] = (m[a.category ?? 'Unclassified'] ?? 0) + dn(a) + up(a)), m), {})).sort((a, b) => b[1] - a[1]))
  const CATS = ['Streaming', 'Social', 'Gaming', 'Web', 'Cloud & storage', 'Messaging', 'Updates', 'Other']
  $effect(() => { keep('insights.q', dq) })
</script>

<div class="grid">
  <h1>Insights</h1>
  <Tabs base="insights" active={tab} tabs={[{ id: 'apps', label: 'Applications' }, { id: 'clients', label: 'Clients' }, { id: 'domains', label: 'Domains', count: domains.length }, { id: 'flows', label: 'Flows' }, { id: 'countries', label: 'Countries' }]} />

  {#if tab === 'apps'}
    <Section title="Traffic by application" desc="Per-app and per-category traffic (last minute / hour / day / week) from MikroTik CMR's classifier, across every device it runs on (CMR apptraffic-devices)." ros="/cmr/device/apptraffic [find] once">
      {#if cmr.data?.apps?.length}
        <div class="row"><select bind:value={period} aria-label="Period">{#each periods as [p, label]}<option value={p}>{label}</option>{/each}</select></div>
        {#each apps as a}
          <div class="bar-row">
            <span class="who">{a.name}{#if a.category}<span class="muted small"> · {a.category}</span>{/if}</span>
            <span class="track"><i class="d" style="width:{(dn(a) / appMax) * 100}%"></i><i class="u" style="width:{(up(a) / appMax) * 100}%"></i></span>
            <span class="mono small">{bytes(dn(a) + up(a))}</span><span class="mono small muted">{bytes(dn(a))} / {bytes(up(a))}</span>
          </div>
        {/each}
        <div class="row small muted legend"><span><i class="sw d"></i> Download</span><span><i class="sw u"></i> Upload</span></div>
        <h3 class="small">By category</h3>
        <div class="cats">{#each byCat as [c, n]}<span class="pill">{c} · {bytes(n)}</span>{/each}</div>
      {:else if cmr.data && cmr.data.available !== false}
        <div class="muted small">The CMR server reports no application traffic yet{cmr.data.apptrafficDevices ? ` (classifier on: ${cmr.data.apptrafficDevices})` : ''}. It classifies on the gateway by default; <code>/cmr set apptraffic-devices=all</code> adds every device.</div>
      {:else}
      <div class="empty">
        <Icon name="traffic" size={28} />
        <b>Application identification needs the CMR server</b>
        <p class="muted small">{hw.canHostCmr ? `Install the cmr package on this ${router.board} and pair it` : `Your ${router.board} can't run the CMR server package. Run it on a CHR VM and pair this router as a client`}, then this tab fills in with categories like:</p>
        <div class="cats">{#each CATS as c}<span class="pill">{c}</span>{/each}</div>
        <a class="btn primary" href="#/devices/list">Set up CMR…</a>
      </div>
      {/if}
    </Section>
  {:else if tab === 'clients'}
    <Section title="Internet activity" desc="Client count and throughput over time.">
      {#key prefs.theme + prefs.palette}<ActivityChart data={activity('1D')} height={260} />{/key}
    </Section>
    <Section title="Traffic by client over time" desc="Bytes per {win === '24h' ? 'hour' : 'day'} for the clients that used the most in the window; everyone else is summed as Other. tikontrol records it while it runs: Wi-Fi from the registration table, wired clients from kid-control accounting." ros={hw.wifiPackage === 'wifi' ? '/interface/wifi/registration-table/print; /ip/kid-control/device/print stats' : '/interface/wireless/registration-table/print; /ip/kid-control/device/print stats'}>
      {#snippet actions()}
        <div class="seg" role="group" aria-label="Show">{#each [['down', 'Download'], ['up', 'Upload'], ['total', 'Both']] as const as [m, l]}<button class:on={metric === m} aria-pressed={metric === m} onclick={() => (metric = m)}>{l}</button>{/each}</div>
        <div class="seg" role="group" aria-label="Window">{#each [['24h', '24 h'], ['7d', '7 days']] as const as [v, l]}<button class:on={win === v} aria-pressed={win === v} onclick={() => (win = v)}>{l}</button>{/each}</div>
      {/snippet}
      {#if !isBackend}<p class="small muted">Simulated data: there is no tikontrol server, so this chart is made up.</p>{/if}
      {#if histErr && !hist}
        <p class="muted small">Couldn't load the client history from the tikontrol server.</p>
      {:else if !hist}
        <p class="muted small">Loading…</p>
      {:else if !histTotal}
        <p class="muted small">No client traffic recorded in this window yet. tikontrol stores hourly bytes per client while it runs (kept 7 days).</p>
      {:else}
        {#key prefs.theme + prefs.palette}<ClientTrafficChart data={hist} {win} {metric} name={nameOfMac} />{/key}
        <div class="scroll"><table class="totals">
          <caption class="sr">Totals per client, {win === '24h' ? 'last 24 hours' : 'last 7 days'}</caption>
          <thead><tr><th scope="col">Client</th><th scope="col" class="r">Download</th><th scope="col" class="r wide">Upload</th><th scope="col" class="r">Total</th><th scope="col" class="r wide">Share</th></tr></thead>
          <tbody>
            {#each hist.clients as c}
              <tr><td><a href="#/clients/{c.mac}">{nameOfMac(c.mac)}</a></td><td class="r mono">{bytes(c.down)}</td><td class="r mono wide">{bytes(c.up)}</td><td class="r mono">{bytes(c.down + c.up)}</td><td class="r mono muted wide">{Math.round(((c.down + c.up) / histTotal) * 100)}%</td></tr>
            {/each}
            {#if hist.other.count}
              <tr class="nohover"><td class="muted">Other ({hist.other.count})</td><td class="r mono">{bytes(hist.other.down)}</td><td class="r mono wide">{bytes(hist.other.up)}</td><td class="r mono">{bytes(hist.other.down + hist.other.up)}</td><td class="r mono muted wide">{Math.round(((hist.other.down + hist.other.up) / histTotal) * 100)}%</td></tr>
            {/if}
          </tbody>
        </table></div>
      {/if}
    </Section>
    <Section title="Traffic since connect" desc="Total bytes since each client connected (Wi-Fi counters; wired clients need kid-control accounting)." ros={hw.wifiPackage === 'wifi' ? '/interface/wifi/registration-table/print; /ip/kid-control/device/print stats' : '/interface/wireless/registration-table/print; /ip/kid-control/device/print stats'}>
      {#each byClient as { c, total }}
        <a class="bar-row" href="#/clients/{c.mac}">
          <span class="row who"><Icon name={iconOf(c)} size={14} /> {displayName(c)}</span>
          <span class="track"><i class="d" style="width:{(c.bytesDown / maxTotal) * 100}%"></i><i class="u" style="width:{(c.bytesUp / maxTotal) * 100}%"></i></span>
          <span class="mono small">{total ? bytes(total) : '—'}</span>
          <span class="mono small muted">{bps(live.rate[c.mac]?.rx ?? 0)}</span>
        </a>
      {/each}
      <div class="row small muted legend"><span><i class="sw d"></i> Download</span><span><i class="sw u"></i> Upload</span></div>
    </Section>
  {:else if tab === 'domains'}
    <Section title="Recently resolved domains" desc="From the router's DNS cache. Shows what's being looked up, not how much data moved, and not who asked: RouterOS doesn't record the client per cache entry. Per-client sites are under Flows (flow destinations named from this cache)." ros="/ip/dns/cache/print">
      {#snippet actions()}<input type="search" placeholder="Filter…" bind:value={dq} />{/snippet}
      <table>
        <thead><tr><th>Domain</th><th class="r">Names</th><th>Examples</th><th></th></tr></thead>
        <tbody>
          {#each domains.filter((d) => !dq || d.d.includes(dq)) as d}
            <tr class="nohover"><td><b>{d.d}</b></td><td class="r">{d.names.length}</td><td class="muted small ell">{d.names.slice(0, 3).join(', ')}</td>
              <td class="r"><button class="btn small" onclick={() => review(`Block ${d.d}`, [`/ip/dns/static/add name=${d.d} type=NXDOMAIN match-subdomain=yes comment=tik:app:${d.d}`, '/ip/dns/cache/flush'], { kind: 'block-domain', domain: d.d })}>Block</button></td></tr>
          {/each}
        </tbody>
      </table>
    </Section>
  {:else if tab === 'flows'}
    <Section title="Traffic flows" desc="Every connection with source, destination, port and bytes. RouterOS exports flows (IPFIX) to tikontrol's collector." ros="/ip/traffic-flow/print">
      {#if !flowOn}
        <div class="empty">
          <Icon name="route" size={28} />
          <b>Traffic Flow is off on the router</b>
          <p class="muted small">Turning it on costs some CPU on the {router.board}. tikontrol would collect flows on port 2055 and keep a day of history.</p>
          <button class="btn primary" onclick={() => review('Enable Traffic Flow', ['/ip/traffic-flow/set enabled=yes', `/ip/traffic-flow/target/add dst-address=${selfIp || '<tikontrol-ip>'} port=2055 version=9`], selfIp ? { kind: 'set-traffic-flow', enabled: true, address: selfIp } : undefined)}>Enable flows</button>
          <p class="muted small">tikontrol's collector is off by default: start the server with <code>TIK_FLOWS=0.0.0.0:2055</code>.</p>
        </div>
      {/if}
      {#if flows && flows.flows}
        <p class="muted small">{flows.flows.toLocaleString()} flows since {new Date(flows.since * 1000).toLocaleString()} · in memory only</p>
        <div class="fgrid">
          <table><thead><tr><th>Client</th><th class="r">Down</th><th class="r">Up</th></tr></thead>
            <tbody>{#each flows.talkers as t}
              <tr class="nohover"><td><button class="link" aria-expanded={openTalker === t.ip} onclick={() => (openTalker = openTalker === t.ip ? null : t.ip)}>{nameOfIp(t.ip)}</button></td><td class="r">{bytes(t.down)}</td><td class="r">{bytes(t.up)}</td></tr>
              {#if openTalker === t.ip}{#each t.destinations ?? [] as d}<tr class="nohover sub"><td class="mono small muted">↳ {d.name ?? d.ip}</td><td class="r small" colspan="2">{bytes(d.bytes)}</td></tr>{/each}{/if}
            {/each}</tbody></table>
          <table><thead><tr><th>Destination</th><th class="r">Bytes</th></tr></thead>
            <tbody>{#each flows.destinations as d}<tr class="nohover"><td class="mono small">{d.name ?? d.ip}</td><td class="r">{bytes(d.bytes)}</td></tr>{/each}</tbody></table>
          <table><thead><tr><th>Service</th><th>Port</th><th class="r">Bytes</th></tr></thead>
            <tbody>{#each flows.ports as p}<tr class="nohover"><td>{p.service || '—'}</td><td class="mono small">{p.proto}/{p.port}</td><td class="r">{bytes(p.bytes)}</td></tr>{/each}</tbody></table>
        </div>
      {:else}
        <table class:dimmed={!flowOn}>
          <thead><tr><th>Client</th><th>Destination</th><th>Port</th><th class="r">Bytes</th></tr></thead>
          <tbody><tr class="nohover"><td colspan="4" class="muted">No flows collected yet.</td></tr></tbody>
        </table>
      {/if}
    </Section>
  {:else}
    <Section title="Traffic by country" desc="Offline GeoIP lookup on flow destinations. Needs Traffic Flow (see Flows) and a local IPv4-to-country CSV; nothing is downloaded.">
      {#if flows?.geoip && flows.countries.length}
        <table>
          <thead><tr><th>Country</th><th class="r">Bytes</th></tr></thead>
          <tbody>{#each flows.countries as c}<tr class="nohover"><td>{countryName(c.country)} <span class="muted small mono">{c.country}</span></td><td class="r">{bytes(c.bytes)}</td></tr>{/each}</tbody>
        </table>
      {:else}
        <div class="empty"><Icon name="globe" size={28} /><b>{flows?.geoip ? 'No flow data yet' : 'GeoIP is off'}</b>
          <p class="muted small">{flows?.geoip ? 'Once the router exports flows, this lists bytes per destination country.' : 'Start the server with TIK_GEOIP=/path/to/ip-to-country.csv (rows of start,end,country code, for example the DB-IP Lite or IP2Location LITE DB1 CSV). tikontrol never fetches it itself.'}</p>
          <a class="btn" href="#/insights/flows">Go to Flows</a></div>
      {/if}
    </Section>
  {/if}
</div>

<style>
  .empty { display: grid; justify-items: center; gap: 8px; text-align: center; padding: 26px 10px; color: var(--muted); }
  .empty b { color: var(--text); }
  .fgrid { display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: 16px; }
  .empty p { max-width: 520px; margin: 0; }
  .cats { display: flex; flex-wrap: wrap; gap: 6px; justify-content: center; margin: 6px 0; }
  .bar-row { display: grid; grid-template-columns: minmax(0, 200px) 1fr 64px 72px; gap: 12px; align-items: center; padding: 6px 0; color: var(--text); }
  @media (max-width: 760px) { .bar-row { grid-template-columns: minmax(0, 1fr) 56px 64px; } .bar-row .track { display: none; } }
  .who { overflow: hidden; white-space: nowrap; }
  .track { display: flex; height: 8px; background: var(--panel2); border-radius: 4px; overflow: hidden; }
  .track i { display: block; height: 100%; }
  .d { background: var(--down); } .u { background: var(--up); }
  .sw { display: inline-block; width: 10px; height: 10px; border-radius: 3px; vertical-align: middle; }
  .legend { gap: 14px; margin-top: 8px; }
  .r { text-align: right; }
  .scroll { overflow-x: auto; margin-top: 14px; }
  @media (max-width: 560px) { .totals .wide { display: none; } }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .small { font-size: 12px; }
  .btn.small { padding: 3px 9px; }
  .ell { max-width: 360px; overflow: hidden; text-overflow: ellipsis; }
  .dimmed { opacity: .4; }
  .link { background: none; border: 0; padding: 0; color: var(--text); cursor: pointer; text-align: left; }
  .link:hover { text-decoration: underline; }
  .sub td { padding-top: 2px; padding-bottom: 2px; }
</style>
