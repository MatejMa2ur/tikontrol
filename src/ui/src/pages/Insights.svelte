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
  import { prefs } from '../lib/live.svelte'

  let { tab = 'apps' }: { tab?: string } = $props()
  const flowOn = !!raw.trafficFlow?.enabled

  // Traffic by client
  const byClient = [...clients].map((c) => ({ c, total: c.bytesDown + c.bytesUp })).sort((a, b) => b.total - a.total)
  const maxTotal = Math.max(...byClient.map((x) => x.total), 1)

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

  const CATS = ['Streaming', 'Social', 'Gaming', 'Web', 'Cloud & storage', 'Messaging', 'Updates', 'Other']
  $effect(() => { keep('insights.q', dq) })
</script>

<div class="grid">
  <h1>Insights</h1>
  <Tabs base="insights" active={tab} tabs={[{ id: 'apps', label: 'Applications' }, { id: 'clients', label: 'Clients' }, { id: 'domains', label: 'Domains', count: domains.length }, { id: 'flows', label: 'Flows' }, { id: 'countries', label: 'Countries' }]} />

  {#if tab === 'apps'}
    <Section title="Traffic by application" desc="Per-app and per-category traffic (last minute / hour / day / week) comes from MikroTik CMR's classifier on the gateway." ros="/cmr/device/apptraffic">
      <div class="empty">
        <Icon name="traffic" size={28} />
        <b>Application identification needs the CMR server</b>
        <p class="muted small">{hw.canHostCmr ? `Install the cmr package on this ${router.board} and pair it` : `Your ${router.board} can't run the CMR server package. Run it on a CHR VM and pair this router as a client`}, then this tab fills in with categories like:</p>
        <div class="cats">{#each CATS as c}<span class="pill">{c}</span>{/each}</div>
        <a class="btn primary" href="#/devices/list">Set up CMR…</a>
      </div>
    </Section>
  {:else if tab === 'clients'}
    <Section title="Internet activity" desc="Client count and throughput over time.">
      {#key prefs.theme + prefs.palette}<ActivityChart data={activity('1D')} height={260} />{/key}
    </Section>
    <Section title="Traffic by client" desc="Total bytes since each client connected (Wi-Fi counters; wired clients need kid-control accounting)." ros="/interface/wireless/registration-table/print; /ip/kid-control/device/print stats">
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
          <button class="btn primary" onclick={() => review('Enable Traffic Flow', ['/ip/traffic-flow/set enabled=yes interfaces=WAN active-flow-timeout=1m', '/ip/traffic-flow/target/add dst-address=<tikontrol-ip> port=2055 version=5'])}>Enable flows</button>
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
  .bar-row { display: grid; grid-template-columns: 200px 1fr 80px 80px; gap: 12px; align-items: center; padding: 6px 0; color: var(--text); }
  .who { overflow: hidden; white-space: nowrap; }
  .track { display: flex; height: 8px; background: var(--panel2); border-radius: 4px; overflow: hidden; }
  .track i { display: block; height: 100%; }
  .d { background: var(--down); } .u { background: var(--up); }
  .sw { display: inline-block; width: 10px; height: 10px; border-radius: 3px; vertical-align: middle; }
  .legend { gap: 14px; margin-top: 8px; }
  .r { text-align: right; }
  .small { font-size: 12px; }
  .btn.small { padding: 3px 9px; }
  .ell { max-width: 360px; overflow: hidden; text-overflow: ellipsis; }
  .dimmed { opacity: .4; }
  .link { background: none; border: 0; padding: 0; color: var(--text); cursor: pointer; text-align: left; }
  .link:hover { text-decoration: underline; }
  .sub td { padding-top: 2px; padding-bottom: 2px; }
</style>
