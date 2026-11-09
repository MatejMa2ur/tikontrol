<script lang="ts">
  import { clients, internet, router, insights, radios, wifiTech, connectivity, adblock, signalQuality, bytes, bps, dur, hw } from '../lib/data'
  import { live, displayName, activity, health, type Range } from '../lib/live.svelte'
  import ActivityChart from '../lib/ActivityChart.svelte'
  import Donut from '../lib/Donut.svelte'
  import DeviceFace from '../lib/DeviceFace.svelte'
  import Icon from '../lib/Icon.svelte'
  import Ros from '../lib/Ros.svelte'
  import { prefs } from '../lib/live.svelte'
  import { isBackend } from '../lib/data'
  import { api, session } from '../lib/backend'

  // speed test (server side, src/server/src/speedtest.rs)
  type Run = { t: number; down: number; up: number; ping: number; host: string }
  let st = $state<{ url: string; default: string; results: Run[]; running: boolean; wait: number }>({ url: '', default: 'https://speed.cloudflare.com', results: [], running: false, wait: 0 })
  let stUrl = $state(''), stErr = $state('')
  const testHost = $derived((() => { try { return new URL(st.url || st.default).hostname } catch { return '?' } })())
  const lastTest = $derived(st.results.at(-1))
  const loadTest = () => api<typeof st>('/speedtest').then((v) => { st = v; stUrl = v.url === v.default ? '' : v.url }).catch(() => {})
  if (isBackend) loadTest()
  async function runTest() {
    stErr = ''; st.running = true
    try { await api('/speedtest', { method: 'POST' }) } catch (e) { stErr = (e as Error).message }
    await loadTest()
  }
  async function saveUrl() {
    stErr = ''
    try { await api('/speedtest', { method: 'PUT', body: { url: stUrl.trim() || null } }); await loadTest() } catch (e) { stErr = (e as Error).message }
  }

  const online = clients.filter((c) => c.online)
  const wifiClients = online.filter((c) => !c.wired)
  const COLORS = ['var(--accent)', '#2fb4f2', '#3ccf7a', '#f3b33d', '#b26cf5']

  // clock
  let now = $state(new Date())
  $effect(() => { const i = setInterval(() => (now = new Date()), 10000); return () => clearInterval(i) })

  // Traffic Identification: by client until CMR app classification is wired up
  const traffic = [...wifiClients].sort((a, b) => b.bytesDown + b.bytesUp - (a.bytesDown + a.bytesUp)).slice(0, 5)
  const identified = wifiClients.reduce((s, c) => s + c.bytesDown + c.bytesUp, 0)
  const trafficDown = wifiClients.reduce((s, c) => s + c.bytesDown, 0)
  const trafficUp = wifiClients.reduce((s, c) => s + c.bytesUp, 0)

  const expOf = (cs: typeof clients) => signalQuality(cs.reduce((s, c) => s + (c.signal ?? -60), 0) / (cs.length || 1))
  const mostActive = $derived([...online].sort((a, b) => (live.rate[b.mac]?.rx ?? 0) - (live.rate[a.mac]?.rx ?? 0)).slice(0, 7))
  const techActivity = $derived(wifiTech.map((g) => g.clients.reduce((s, c) => s + (live.rate[c.mac]?.rx ?? 0), 0)))
  const techMax = $derived(Math.max(...techActivity, 1))

  // kept in sessionStorage: the page is rebuilt when new router data arrives, and the choice must survive that
  let range = $state<Range>((sessionStorage.getItem('tik:range') as Range) || '1H')
  let mode = $state<'all' | 'down' | 'up'>((sessionStorage.getItem('tik:mode') as 'all') || 'all')
  $effect(() => { sessionStorage.setItem('tik:range', range); sessionStorage.setItem('tik:mode', mode) })
  const hist = $derived(activity(range))
  // index of the first slot with data: before it tikontrol wasn't running yet
  const since = $derived((hist.rx as (number | null)[]).findIndex((v) => v != null))

  let band = $state(radios.find((r) => r.name === sessionStorage.getItem('tik:band'))?.name ?? radios.find((r) => r.band === '5 GHz')?.name ?? radios[0]?.name)
  $effect(() => { if (band) sessionStorage.setItem('tik:band', band) })
  const radio = $derived(radios.find((r) => r.name === band))
  const bandClients = $derived(wifiClients.filter((c) => c.port === band))
  // link-quality buckets (CCQ %), shown like UniFi's TX-retries strip: worse → better
  const buckets = [[0, 50, '<50%'], [50, 60, '50%'], [60, 70, '60%'], [70, 80, '70%'], [80, 90, '80%'], [90, 101, '90%+']] as const
  const sigPos = (dbm: number) => Math.min(100, Math.max(0, ((dbm + 90) / 60) * 100)) // -90…-30 dBm → 0…100%

  const spark = (vals: number[]) => {
    const max = Math.max(...vals, 1)
    return vals.slice(-60).map((v, i) => `${(i / 59) * 100},${28 - (v / max) * 26}`).join(' ')
  }
  const problems = insights.filter((i) => i.level !== 'ok')
  // layout: ids the UI doesn't know are dropped, new cards are appended, so old prefs keep working
  const CARDS = {
    traffic: { title: 'Traffic Identification', body: trafficCard },
    tech: { title: 'WiFi Technology', body: techCard },
    active: { title: 'Most Active Clients', body: activeCard },
    aps: { title: 'Most Active Access Points', body: apsCard },
    activity: { title: 'Internet Activity', body: activityCard },
    link: { title: 'Radio Link Quality', body: linkCard },
    conn: { title: 'WiFi Connectivity', body: connCard },
  }
  type Card = keyof typeof CARDS
  const ALL = Object.keys(CARDS) as Card[]
  const isCard = (id: string): id is Card => Object.hasOwn(CARDS, id)
  let editing = $state(false)
  const order = $derived([...new Set([...prefs.cardOrder.filter(isCard), ...ALL])])
  const visible = $derived(editing ? order : order.filter((id) => !prefs.hiddenCards.includes(id)))
  const move = (id: Card, d: -1 | 1) => {
    const o = [...order], i = o.indexOf(id)
    ;[o[i], o[i + d]] = [o[i + d], o[i]]
    prefs.cardOrder = o
  }
  const toggle = (id: Card) => (prefs.hiddenCards = prefs.hiddenCards.includes(id) ? prefs.hiddenCards.filter((x) => x !== id) : [...prefs.hiddenCards, id])
  const q = signalQuality(wifiClients.reduce((s, c) => s + (c.signal ?? 0), 0) / (wifiClients.length || 1))
</script>

<div class="dash">
  <!-- ===== left: gateway column ===== -->
  <aside class="side">
    <div class="clock muted">{now.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit', timeZoneName: 'short' })}</div>
    <div class="hw"><DeviceFace /></div>
    <b class="dev">{router.identity} <span class="muted">· {router.board}</span></b>
    <div class="muted tiny">{hw.specs}</div>
    <div class="muted tiny">RouterOS {router.version} ({router.channel})</div>
    <div class="tiny links"><a href="https://mikrotik.com/download/changelogs" target="_blank" rel="noreferrer">Release Notes</a> | <a href="#/events">View Logs</a></div>

    <div class="kv">
      <span>WAN IP (Port {internet.wanPort})</span><span class="mono">{internet.wanIp}</span>
      <span>Gateway IP</span><span class="mono">{internet.gatewayIp}</span>
    </div>
    <hr />
    <div class="kv"><b>System Uptime</b><span>{dur(router.uptimeS)}</span></div>
    <hr />
    <div class="kv">
      <span>Internet</span><span class="row isp"><Icon name="globe" size={13} /> {internet.doubleNat ? 'Behind NAT' : 'Direct'}</span>
      {#if internet.publicIp}<span>Public IP</span><span class="mono">{internet.publicIp}</span>{/if}
      <span>Uptime</span><span class="good">100%</span>
      <span>Activity</span><span><span class="down">↓ {bps(live.rx.at(-1) ?? 0)}</span> <span class="up">↑ {bps(live.tx.at(-1) ?? 0)}</span></span>
      <span title="Highest 1-minute average in tikontrol's stored history">Peak</span><span><span class="down">↓ {bps(hist.peakAll.rx)}</span> <span class="up">↑ {bps(hist.peakAll.tx)}</span></span>
    </div>
    <svg class="spark" viewBox="0 0 100 30" preserveAspectRatio="none"><polyline points={spark(live.rx)} stroke="var(--down)" /><polyline points={spark(live.tx)} stroke="var(--up)" /></svg>
    <div class="lat">
      {#each internet.latency as l}
        <span class="chip" title="{l.name} · {l.loss ?? '?'}% loss"><i class="b {l.name}">{l.name[0]}</i>{l.ms == null ? '—' : `${Math.round(l.ms)} ms`}</span>
      {/each}
    </div>
    <hr />
    <div class="row between"><b>Internet Health</b><a class="small" href="#/events">See All</a></div>
    <div class="health" title="Internet health, last 12 hours">{#each health as h}<i class={h}></i>{/each}</div>
    <div class="row between tiny muted"><span>-12h</span><span>Now</span></div>
    <hr />
    <div class="row between"><b>Speed Test</b>
      {#if isBackend && session.me?.role === 'admin'}<button class="link small" disabled={st.running || st.wait > 0} onclick={runTest} title={st.wait > 0 ? `Next run possible in ${Math.ceil(st.wait / 60)} min` : `Contacts ${testHost}`}>{st.running ? 'Running…' : 'Run Now'}</button>{/if}
    </div>
    <div class="tiny muted">{#if isBackend}Opt-in: runs only when an admin presses Run Now and contacts <b>{testHost}</b>{testHost === 'speed.cloudflare.com' ? ' (Cloudflare)' : ''}. It measures the connection of the host tikontrol runs on, which is the router's line only when tikontrol runs on the router.{:else}Needs the tikontrol server.{/if}</div>
    {#if stErr}<div class="tiny bad" role="alert">{stErr}</div>{/if}
    <div class="row between last"><b>Last Speed Test</b>{#if lastTest}<span class="tiny" title={new Date(lastTest.t * 1000).toLocaleString()}><span class="down">↓ {bps(lastTest.down)}</span> <span class="up">↑ {bps(lastTest.up)}</span> · {lastTest.ping} ms</span>{:else}<span class="tiny muted">Never</span>{/if}</div>
    {#if st.results.length || (isBackend && session.me?.role === 'admin')}
      <details class="tiny"><summary>History ({st.results.length})</summary>
        <table class="mini">
          <thead><tr><th>When</th><th class="r">Down</th><th class="r">Up</th><th class="r">Ping</th></tr></thead>
          <tbody>{#each [...st.results].reverse() as r}<tr><td>{new Date(r.t * 1000).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })}</td><td class="r down">{bps(r.down)}</td><td class="r up">{bps(r.up)}</td><td class="r">{r.ping} ms</td></tr>{/each}</tbody>
        </table>
        {#if session.me?.role === 'admin'}
          <label class="row">Endpoint <input type="url" bind:value={stUrl} placeholder={st.default} aria-label="Speed test endpoint (HTTPS)" /></label>
          <button class="link small" onclick={saveUrl}>Save endpoint</button> <span class="muted">Must speak Cloudflare's /__down and /__up.</span>
        {/if}
      </details>
    {/if}
    <hr />
    <div class="row between"><b>Default WiFi Speeds</b></div>
    <div class="tiny muted">Channel Widths (MHz)</div>
    {#each radios as r}
      <div class="widths"><span class="tiny muted">{r.band}</span>{#each r.widths as w}<span class:on={w === r.width} class:off={w > r.maxWidth}>{w}</span>{/each}</div>
    {/each}
    <hr />
    <a class="row between card-link" href="#/settings"><b>Ad Blocking</b><span class="small good">Active ›</span></a>
    <div class="kv tiny"><span>Blocked domains</span><span>{adblock.names.toLocaleString()}</span><span>Blocked requests</span><span>{adblock.hits.toLocaleString()}</span></div>
    <hr />
    <a class="row between card-link" href="#/settings"><b class="row"><Icon name="shield" size={14} /> Security Checks</b><span>›</span></a>
    {#each problems as p}
      {#if p.href}<a class="row tiny check" href={p.href} title={p.detail}><span class="dot {p.level}"></span>{p.title} ›</a>
      {:else}<div class="row tiny check" title={p.detail}><span class="dot {p.level}"></span>{p.title}</div>{/if}
    {/each}
    <Ros cmd="/system/resource/print; /ip/address/print; /ping 1.1.1.1 count=3" />
  </aside>

  <!-- ===== right: cards (order and visibility in prefs, "Edit layout") ===== -->
  <div class="cards">
    <div class="layout-bar row small">
      {#if editing}
        <span class="muted">Hide, show and reorder the cards.</span>
        <button class="btn" onclick={() => { prefs.cardOrder = []; prefs.hiddenCards = [] }}>Reset layout</button>
        <button class="btn primary" onclick={() => (editing = false)}>Done</button>
      {:else}
        <button class="btn" onclick={() => (editing = true)}><Icon name="settings" size={13} /> Edit layout</button>
      {/if}
    </div>
    {#each visible as id, i (id)}
      {@const off = prefs.hiddenCards.includes(id)}
      <div class="slot" class:wide={id === 'activity'} class:off>
        {#if editing}
          <div class="edit row small" role="group" aria-label="{CARDS[id].title} card">
            <b>{CARDS[id].title}</b>{#if off}<span class="muted">Hidden</span>{/if}
            <span class="row">
              <button class="btn" disabled={i === 0} onclick={() => move(id, -1)} aria-label="Move {CARDS[id].title} up">↑ Up</button>
              <button class="btn" disabled={i === visible.length - 1} onclick={() => move(id, 1)} aria-label="Move {CARDS[id].title} down">↓ Down</button>
              <button class="btn" onclick={() => toggle(id)} aria-pressed={!off} aria-label="{off ? 'Show' : 'Hide'} {CARDS[id].title}">{off ? 'Show' : 'Hide'}</button>
            </span>
          </div>
        {/if}
        {@render CARDS[id].body()}
      </div>
    {/each}
  </div>
</div>

{#snippet trafficCard()}
    <section class="card">
      <header class="row between"><h3>Traffic Identification</h3><span class="row small"><span class="down">↓ {bytes(trafficDown)}</span><span class="up">↑ {bytes(trafficUp)}</span><span class="muted">›</span></span></header>
      <div class="split">
        <Donut parts={traffic.map((c, i) => ({ v: c.bytesDown + c.bytesUp, color: COLORS[i] }))} value={bytes(identified)} label="Identified Traffic" />
        <table class="mini">
          <thead><tr><th>Name</th><th class="r">Download</th><th class="r">Upload</th><th class="r">Total</th></tr></thead>
          <tbody>
            {#each traffic as c, i}
              <tr onclick={() => (location.hash = `#/clients/${c.mac}`)}>
                <td><span class="row"><i class="sw" style="background:{COLORS[i]}"></i><Icon name={c.kind} size={13} /><a class="ell plain" href="#/clients/{c.mac}">{displayName(c)}</a></span></td>
                <td class="r down">{bytes(c.bytesDown)}</td><td class="r up">{bytes(c.bytesUp)}</td><td class="r">{bytes(c.bytesDown + c.bytesUp)}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <div class="note tiny muted">By client. Per-app identification arrives with MikroTik CMR (Phase 3).</div>
    </section>
{/snippet}

{#snippet techCard()}
    <section class="card">
      <header class="row between"><h3>WiFi Technology</h3><span class="muted">›</span></header>
      <div class="split">
        <Donut parts={wifiTech.map((g, i) => ({ v: g.clients.length, color: COLORS[i] }))} value={String(wifiClients.length)} label="Total Clients" />
        <table class="mini">
          <thead><tr><th>Type</th><th>Activity</th><th>Experience</th><th class="r">Clients</th></tr></thead>
          <tbody>
            {#each wifiTech as g, i}
              {@const e = expOf(g.clients)}
              <tr onclick={() => (location.hash = '#/clients')}>
                <td><a class="row plain" href="#/clients"><i class="sw" style="background:{COLORS[i]}"></i>{g.type} <span class="muted">|</span> {g.band}</a></td>
                <td><div class="act"><i style="width:{(techActivity[i] / techMax) * 100}%"></i></div></td>
                <td class={e.level === 'ok' ? 'good' : e.level}>{e.label}</td>
                <td class="r">{g.clients.length}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>
{/snippet}

{#snippet activeCard()}
    <section class="card">
      <header class="row between"><h3>Most Active Clients</h3><a href="#/clients" class="muted">›</a></header>
      <div class="tiles">
        {#each mostActive as c, i}
          <a class="tile" class:first={i === 0} href="#/clients/{c.mac}" title="{displayName(c)} · {bps(live.rate[c.mac]?.rx ?? 0)}">
            <span class="ti"><Icon name={c.kind} size={20} /></span><span class="ell">{displayName(c)}</span>
          </a>
          {#if i === 0}<span class="sep"></span>{/if}
        {/each}
      </div>
    </section>
{/snippet}

{#snippet apsCard()}
    <section class="card">
      <header class="row between"><h3>Most Active Access Points</h3><a href="#/devices" class="muted">›</a></header>
      <div class="tiles">
        {#each radios as r}
          <a class="tile" href="#/ports" title="{r.name} · {r.clients} clients"><span class="ti ap"><Icon name="wifi" size={20} /></span><span class="ell">{router.identity} {r.band}</span></a>
        {/each}
      </div>
    </section>
{/snippet}

{#snippet activityCard()}
    <section class="card">
      <header class="row between wrap">
        <h3>Internet Activity</h3>
        <div class="row small wrap">
          <div class="seg">
            <button class:on={mode === 'all'} onclick={() => (mode = 'all')}>All</button>
            <button class:on={mode === 'down'} onclick={() => (mode = 'down')}>Download</button>
            <button class:on={mode === 'up'} onclick={() => (mode = 'up')}>Upload</button>
          </div>
          <span class="muted" title="Peak 1-minute average in this range">Peak <span class="down">↓ {bps(hist.peak.rx)}</span> <span class="up">↑ {bps(hist.peak.tx)}</span></span>
          <span class="down" title="Since boot">↓ {bytes(internet.bytesDown)}</span>
          <span class="up" title="Since boot">↑ {bytes(internet.bytesUp)}</span>
          <span class="row muted"><Icon name="clients" size={13} /> {online.length}</span>
          <div class="seg">{#each ['1H', '1D', '1W'] as const as r}<button class:on={range === r} onclick={() => (range = r)}>{r}</button>{/each}</div>
        </div>
      </header>
      {#key prefs.theme + prefs.palette}<ActivityChart data={hist} {mode} />{/key}
      {#if since > 0}<p class="muted small">History starts {new Date(hist.t[since] * 1000).toLocaleString([], { weekday: 'short', hour: '2-digit', minute: '2-digit' })}, when tikontrol started collecting.</p>{:else if since < 0}<p class="muted small">No history yet: tikontrol collects a sample every poll.</p>{/if}
      <Ros cmd="/interface/monitor-traffic {internet.wanIf}; /interface/print stats where name={internet.wanIf}" />
    </section>
{/snippet}

{#snippet linkCard()}
    <section class="card">
      <header class="row between">
        <h3>Radio Link Quality</h3>
        <div class="seg small">{#each radios as r}<button class:on={band === r.name} onclick={() => (band = r.name)}>{r.band}</button>{/each}</div>
      </header>
      <div class="buckets">
        {#each buckets as [lo, hi, label], i}
          {@const n = bandClients.filter((c) => (c.ccq ?? 0) >= lo && (c.ccq ?? 0) < hi).length}
          <div><i class="q{i}" class:has={n}>{n || ''}</i><span class="tiny muted">{label}</span></div>
        {/each}
      </div>
      {#if radio}
        <div class="kv tiny radio-kv">
          <span>Channel</span><span>{radio.channel ?? '—'} · {radio.width ?? '—'} MHz</span>
          <span>TX power</span><span>{radio.txPower ?? '—'} dBm</span>
          <span>Noise floor</span><span>{radio.noise ?? '—'} dBm</span>
          <span>Overall CCQ</span><span>{radio.ccq ?? '—'}%</span>
        </div>
      {/if}
    </section>
{/snippet}

{#snippet connCard()}
    <section class="card">
      <header class="row between"><h3>WiFi Connectivity <span class="good small">Success {Math.min(...connectivity.map((c) => c.pct))}%</span></h3><span class="muted">›</span></header>
      <div class="conn">
        {#each connectivity as c}
          <div><span class="tiny muted">{c.name}</span><div class="cbar"><i style="width:{c.pct}%"></i></div><span class="tiny good">{c.pct}%</span></div>
        {/each}
      </div>
      <div class="row small density"><Icon name={q.level === 'ok' ? 'check' : 'alert'} size={14} /> <span class={q.level === 'ok' ? 'good' : q.level}>Client Signal is {q.label}</span></div>
      <div class="sig">
        {#each wifiClients as c}<a class="bub" href="#/clients/{c.mac}" style="left:{sigPos(c.signal ?? -60)}%" title="{displayName(c)} {c.signal} dBm"><Icon name={c.kind} size={10} /></a>{/each}
      </div>
      <div class="row between tiny muted">{#each [-90, -80, -70, -60, -50, -40, -30] as d}<span>{d}</span>{/each}</div>
    </section>
{/snippet}

<style>
  .dash { display: grid; grid-template-columns: 250px minmax(0, 1fr); min-height: calc(100vh - 46px); }
  /* left column */
  .side { border-right: 1px solid var(--line); padding: 14px 14px 30px; display: flex; flex-direction: column; gap: 6px; font-size: 12.5px; }
  .clock { text-align: center; padding-bottom: 8px; border-bottom: 1px solid var(--line); margin: 0 -14px 6px; }
  .hw { margin: 10px 0 14px; }
  .dev { font-size: 13px; }
  .tiny { font-size: 11px; }
  .links { margin-bottom: 8px; }
  .kv { display: grid; grid-template-columns: auto 1fr; gap: 7px 8px; }
  .kv > :nth-child(odd) { color: var(--text); }
  .kv > :nth-child(even) { text-align: right; color: var(--muted); justify-self: end; }
  .kv .mono { font-size: 11.5px; }
  .isp { color: var(--muted); }
  hr { border: 0; border-top: 1px solid var(--line); margin: 8px -14px; width: calc(100% + 28px); }
  .spark { width: 100%; height: 30px; }
  .spark polyline { fill: none; stroke-width: 1.2; vector-effect: non-scaling-stroke; }
  .lat { display: flex; gap: 10px; }
  .chip { display: inline-flex; gap: 5px; align-items: center; font-weight: 550; }
  .b { font-style: normal; width: 15px; height: 15px; border-radius: 3px; display: grid; place-items: center; font-size: 9px; font-weight: 800; color: #fff; }
  .b.Cloudflare { background: #f38020; } .b.Google { background: #4285f4; } .b.Quad9 { background: #d62f6e; }
  .health { display: flex; gap: 1px; height: 7px; border-radius: 4px; overflow: hidden; margin-top: 4px; }
  .health i { flex: 1; background: var(--ok); } .health i.warn { background: var(--warn); } .health i.bad { background: var(--bad); } .health i.none { background: var(--cells); }
  .link { background: none; border: 0; color: var(--accent); cursor: pointer; padding: 0; }
  .last { margin-top: 8px; }
  .widths { display: grid; grid-template-columns: 52px repeat(4, 1fr); gap: 2px; align-items: center; text-align: center; font-size: 11.5px; }
  .widths span { padding: 2px 0; border-radius: 4px; color: var(--muted); }
  .widths span.on { background: color-mix(in srgb, var(--accent) 20%, transparent); color: var(--accent); font-weight: 650; }
  .widths span.off { opacity: .35; }
  .widths span:first-child { text-align: left; }
  .card-link { color: var(--text); }
  .check { gap: 7px; color: var(--muted); }
  .good, .side .good { color: var(--ok); }
  .warn { color: var(--warn); } .bad { color: var(--bad); }
  .down { color: var(--down); } .up { color: var(--up); }

  /* right cards */
  .cards { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 14px; padding: 14px; align-content: start; }
  .wide { grid-column: span 2; }
  .slot { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .slot > :global(.card) { flex: 1; }
  .slot.off > :global(.card) { opacity: .4; }
  .layout-bar { grid-column: 1 / -1; justify-content: flex-end; gap: 8px; margin-bottom: -6px; }
  .edit { justify-content: space-between; gap: 8px; padding: 6px 8px; border: 1px dashed var(--line); border-radius: 6px; }
  .edit .row { gap: 6px; }
  .card { padding: 14px 16px; }
  h3 { margin: 0; font-size: 13px; font-weight: 650; display: flex; gap: 10px; align-items: baseline; }
  header { margin-bottom: 12px; gap: 12px; flex-wrap: wrap; }
  header .row { gap: 12px; }
  .split { display: flex; gap: 22px; align-items: center; }
  .mini { font-size: 12px; }
  .mini th { border: 0; padding: 4px 6px; cursor: default; }
  .mini td { border: 0; padding: 5px 6px; }
  .r { text-align: right; }
  .sw { width: 7px; height: 7px; border-radius: 50%; flex: none; }
  .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 150px; }
  .act { width: 100px; height: 5px; border-radius: 3px; background: var(--line); }
  .act i { display: block; height: 100%; border-radius: 3px; background: var(--accent); transition: width .5s; }
  .note { margin-top: 8px; }
  .tiles { display: flex; gap: 6px; align-items: flex-start; overflow-x: auto; padding-bottom: 2px; }
  .tile { display: grid; justify-items: center; gap: 6px; width: 78px; font-size: 11px; color: var(--muted); text-align: center; }
  .tile .ell { max-width: 78px; }
  .ti { width: 44px; height: 44px; display: grid; place-items: center; border-radius: 9px; border: 1.5px solid var(--accent); background: color-mix(in srgb, var(--accent) 8%, var(--panel)); color: var(--text); }
  .tile.first .ti { box-shadow: 0 0 0 6px color-mix(in srgb, var(--accent) 20%, transparent); }
  .tile:hover { color: var(--text); }
  .sep { width: 1px; height: 30px; background: var(--line); margin: 7px 6px; }
  .buckets { display: grid; grid-template-columns: repeat(6, 1fr); gap: 4px; text-align: center; }
  .buckets i { display: block; height: 18px; border-radius: 3px; font: 600 11px/18px var(--font); color: #fff; opacity: .25; }
  .buckets i.has { opacity: 1; }
  .q0 { background: var(--bad); } .q1 { background: #f2805a; } .q2 { background: var(--warn); } .q3 { background: #a6cf5a; } .q4, .q5 { background: var(--ok); }
  .radio-kv { margin-top: 14px; grid-template-columns: auto 1fr auto 1fr; }
  .conn { display: grid; grid-template-columns: repeat(4, 1fr); gap: 4px; }
  .cbar { height: 5px; background: var(--line); border-radius: 3px; margin: 6px 0 4px; }
  .cbar i { display: block; height: 100%; background: var(--ok); border-radius: 3px; }
  .density { margin: 18px 0 6px; gap: 6px; }
  .density :global(svg) { color: var(--ok); }
  .sig { position: relative; height: 26px; background: linear-gradient(90deg, color-mix(in srgb, var(--bad) 35%, transparent), color-mix(in srgb, var(--warn) 30%, transparent) 40%, color-mix(in srgb, var(--ok) 30%, transparent) 70%); border-radius: 4px; margin: 6px 0 4px; }
  .bub { position: absolute; top: 3px; transform: translateX(-50%); width: 20px; height: 20px; border-radius: 50%; display: grid; place-items: center; background: var(--panel); border: 1.5px solid var(--ok); color: var(--text); }

  @media (max-width: 1100px) { .cards { grid-template-columns: minmax(0, 1fr); } .wide { grid-column: auto; } }
  @media (max-width: 760px) { .dash { grid-template-columns: minmax(0, 1fr); } .side { border-right: 0; border-bottom: 1px solid var(--line); } .split { flex-direction: column; align-items: stretch; } }
</style>
