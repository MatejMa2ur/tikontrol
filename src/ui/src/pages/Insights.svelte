<script lang="ts">
  import { clients, raw, bytes, bps, hw, router } from '../lib/data'
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
  let dq = $state('')

  const CATS = ['Streaming', 'Social', 'Gaming', 'Web', 'Cloud & storage', 'Messaging', 'Updates', 'Other']
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
    <Section title="Recently resolved domains" desc="From the router's DNS cache. Shows what's being looked up, not how much data moved." ros="/ip/dns/cache/print">
      {#snippet actions()}<input type="search" placeholder="Filter…" bind:value={dq} />{/snippet}
      <table>
        <thead><tr><th>Domain</th><th class="r">Names</th><th>Examples</th><th></th></tr></thead>
        <tbody>
          {#each domains.filter((d) => !dq || d.d.includes(dq)) as d}
            <tr class="nohover"><td><b>{d.d}</b></td><td class="r">{d.names.length}</td><td class="muted small ell">{d.names.slice(0, 3).join(', ')}</td>
              <td class="r"><button class="btn small" onclick={() => review(`Block ${d.d}`, [`/ip/dns/static/add name=${d.d} type=NXDOMAIN match-subdomain=yes comment=tik:block`, `/ip/firewall/filter/add chain=forward tls-host=*.${d.d} action=reject comment=tik:block`])}>Block</button></td></tr>
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
          <button class="btn primary" onclick={() => review('Enable Traffic Flow', ['/ip/traffic-flow/set enabled=yes interfaces=WAN active-flow-timeout=1m', '/ip/traffic-flow/target/add dst-address=10.0.0.50 port=2055 version=ipfix'])}>Enable flows</button>
        </div>
      {/if}
      <table class:dimmed={!flowOn}>
        <thead><tr><th>Time</th><th>Client</th><th>Destination</th><th>Port</th><th>Protocol</th><th class="r">Bytes</th></tr></thead>
        <tbody><tr class="nohover"><td colspan="6" class="muted">No flows collected yet.</td></tr></tbody>
      </table>
    </Section>
  {:else}
    <Section title="Traffic by country" desc="GeoIP lookup on flow destinations. Needs Traffic Flow (see Flows).">
      <div class="empty"><Icon name="globe" size={28} /><b>No flow data yet</b><p class="muted small">Once flows are on, this shows a world map and a top-countries table, with one-click region blocking.</p><a class="btn" href="#/insights/flows">Go to Flows</a></div>
    </Section>
  {/if}
</div>

<style>
  .empty { display: grid; justify-items: center; gap: 8px; text-align: center; padding: 26px 10px; color: var(--muted); }
  .empty b { color: var(--text); }
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
</style>
