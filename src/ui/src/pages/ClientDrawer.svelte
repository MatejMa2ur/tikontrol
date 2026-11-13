<script lang="ts">
  import { keep, kept } from '../lib/keep'
  import { untrack, onMount } from 'svelte'
  import { subtitle, signalQuality, bps, bytes, dur, events, isBackend, whyBand, raw, type Client } from '../lib/data'
  import type { Intent } from '../lib/apply.svelte'
  import { api } from '../lib/backend'
  import { live, prefs, displayName, iconOf } from '../lib/live.svelte'
  import { review } from '../lib/apply.svelte'
  import { trafficDraft } from './settings/Security.svelte'
  import { heardFrom, identity, saidBy, SOURCES } from '../lib/discovery.svelte'
  import Icon from '../lib/Icon.svelte'
  import Ros from '../lib/Ros.svelte'
  import Field from '../lib/ui/Field.svelte'
  import Toggle from '../lib/ui/Toggle.svelte'

  let { client: c, onclose: close }: { client: Client; onclose: () => void } = $props()
  // focus moves into the panel and back to whatever opened it (WCAG 2.4.3)
  let panel: HTMLElement
  onMount(() => {
    const back = document.activeElement as HTMLElement | null
    panel.focus()
    return () => { if (back?.isConnected) back.focus() }
  })
  let tab = $state<'overview' | 'insights' | 'settings'>(kept('client.tab', 'overview'))
  let editing = $state(false)
  let draft = $state('')

  const sq = $derived(signalQuality(c.signal))
  // device isolation: tikontrol's tik:isolate:<MAC> bridge filters (isolate-device / unisolate-device)
  const isolated = $derived((raw.bridgeFilter ?? []).some((r: any) => r.comment === `tik:isolate:${c.mac}`))
  // maker/model/type improved by what the device itself said over mDNS/SSDP (untrusted, text only)
  const me = $derived(identity(c))
  const lease = (raw.leases ?? []).find((l: any) => String(l['mac-address'] ?? '').toUpperCase() === c.mac.toUpperCase())

  // Ping from the router (server tools.rs: runs as the reader, one diagnostic per 2 s per session)
  let ping = $state<{ busy: boolean; text: string; ok?: boolean }>({ busy: false, text: '' })
  async function runPing() {
    ping = { busy: true, text: '' }
    try {
      const r = await api<{ rows: Record<string, string>[] }>('/tools/ping', { method: 'POST', body: { host: c.ip, count: 3 } })
      const last = r.rows.at(-1) ?? {}
      const got = Number(last.received ?? r.rows.filter((x) => x.time).length)
      ping = { busy: false, ok: got > 0, text: got > 0 ? `${got} of ${last.sent ?? 3} replies${last['avg-rtt'] ? `, ${last['avg-rtt']} average` : ''}` : 'No reply. Phones and laptops that sleep or block ping often don\'t answer.' }
    } catch (e) { ping = { busy: false, ok: false, text: (e as Error).message } }
  }
  const said = $derived(saidBy(c))
  const heard = $derived(heardFrom(c))
  const history = $derived(events.filter((e) => e.text.includes(displayName(c)) || e.text.includes(c.name)).slice(0, 12))
  const save = () => { prefs.aliases[c.mac] = draft.trim(); if (!draft.trim()) delete prefs.aliases[c.mac]; editing = false }

  // settings form (seeded from the client)
  let s = $state({ fixedIp: untrack(() => c.ip), dns: '', vlan: '', limit: '', pause: false, days: 'Mon-Sun', from: '22:00', to: '07:00' })
  // this device's tikontrol access-list entries (VLAN override, own password); the password itself is never read
  const acl = (tag: string) => (raw.accessList ?? []).find((a: any) => a.comment === `tik:${tag}:${c.mac.toUpperCase()}`)
  const vlanNow = $derived(acl('vlan')?.['vlan-id'] as number | undefined)
  const ppskNow = $derived(!!acl('ppsk'))
  // Block and Pause each add a forward drop rule tagged tik:block:<MAC> / tik:pause:<MAC> (src/server/src/apply.rs)
  const ruled = (kind: 'block' | 'pause') => (raw.filter ?? []).some((r: any) => r.comment === `tik:${kind}:${c.mac.toUpperCase()}`)
  const blocked = $derived(ruled('block'))
  const paused = $derived(ruled('pause'))
  const dropCmd = (kind: string) => `/ip/firewall/filter/add chain=forward src-mac-address=${c.mac} action=drop comment=tik:${kind}:${c.mac} place-before=[first forward rule]`
  const removeCmd = (kind: string) => `/ip/firewall/filter/remove [find comment="tik:${kind}:${c.mac}"]`
  let psk = $state('')
  const KINDS = ['phone', 'computer', 'server', 'vm', 'speaker', 'iot', 'devices', 'unknown']

  // 24h per-client traffic (synthetic, seeded by MAC) until history is stored
  const pts = $derived.by(() => {
    let seed = [...c.mac].reduce((a, ch) => a * 31 + ch.charCodeAt(0), 7) >>> 0
    const rnd = () => ((seed = (seed * 1664525 + 1013904223) % 4294967296) / 4294967296)
    const base = (c.bytesDown || 2e7) / 86400 * 8
    return Array.from({ length: 48 }, (_, i) => base * (0.2 + rnd() * (i > 30 ? 1.6 : 0.8)) * (rnd() < 0.05 ? 6 : 1))
  })
  const max = $derived(Math.max(...pts, 1))

  // stored hourly bytes from the server: [unix hour, down, up]
  let hours = $state<[number, number, number][]>([])
  // roaming history: joins from the router log, plus "roamed from X to Y" the server saw in the registration table
  let stored = $state<{ t: number; text: string }[]>([])
  // first/last time the server saw this MAC
  let seen = $state<{ first: number; last: number } | null>(null)
  if (isBackend) api<{ hours: [number, number, number][]; seen?: { first: number; last: number } | null; events?: { t: number; cat: string; text: string }[] }>(`/history/client?mac=${encodeURIComponent(untrack(() => c.mac))}`).then((h) => { hours = (h?.hours ?? []).slice(-24); seen = h?.seen ?? null; stored = (h?.events ?? []).filter((e) => e.cat === 'WiFi') }).catch(() => {})
  const when = (t: number) => new Date(t * 1000).toLocaleString()
  const hhmm = (t: number) => new Date(t * 1000).toTimeString().slice(0, 5)
  const roams = $derived([
    ...c.bandLog.filter((l) => l.joined).map((l) => ({ time: l.time.slice(11, 16) || l.time, text: `Joined ${l.iface} (${l.band ?? '?'}, ${l.signal} dBm)` })),
    ...stored.map((e) => ({ time: hhmm(e.t), text: e.text })),
  ].slice(-12).reverse())
  const hourMax = $derived(Math.max(...hours.map(([, d, u]) => d + u), 1))
  // one Review & apply for the whole form: the server compiles each part and undoes them together
  const pauseRule = () => `pause-${c.mac.replaceAll(':', '').toLowerCase()}`
  function settingsIntents(): Intent[] {
    const out: Intent[] = []
    if (s.fixedIp && (!c.fixedIp || s.fixedIp !== c.ip)) out.push({ kind: 'fixed-ip', mac: c.mac, address: s.fixedIp })
    if (s.dns.trim()) out.push({ kind: 'dns-record', name: `${s.dns.trim()}.lan`, address: s.fixedIp || c.ip })
    if (s.vlan.trim() && !c.wired) out.push({ kind: 'wifi-client-vlan', mac: c.mac, vlan_id: Number(s.vlan) })
    if (s.pause) out.push({ kind: 'traffic-rule', name: pauseRule(), target: 'mac', value: c.mac, action: 'block', from: s.from, to: s.to })
    return out
  }
  const limitIntent = () => { const [down, up] = s.limit.split('/').map((x) => x.trim()); return { kind: 'limit-client', mac: c.mac, ip: c.ip, down: down ?? '', up: up ?? down ?? '' } }
  // "Why is this client on 2.4 GHz?" and the Safe apply fixes for it
  const why = $derived(whyBand(c))
  let minSig = $state(-72)
  const fixes = $derived.by(() => {
    if (!why) return []
    const r = why.r5, path = `/interface/${r.pkg}`, legacy = r.pkg === 'wireless'
    const set = (k: string, v: string) => `${path}/set [find name="${r.name}"] ${k}=${v}`
    // channel 36 at the control position of the current width ("20/40/80mhz-eCee" -> 2nd 20 MHz -> 5200)
    const freq = String(5180 + 20 * Math.max(0, (r.widthSetting?.split('-')[1] ?? 'C').indexOf('C')))
    const width = legacy ? '20/40mhz-XX' : '20/40mhz'
    const ssid = `${(r.ssid ?? '').slice(0, 29)}-5G`
    const out: { title: string; what: string; risk: string; cmds: string[]; intent: Intent }[] = []
    if (r.dfs || r.auto) out.push({ title: 'Fix 5 GHz on channel 36', risk: 'Medium', what: 'No radar moves and no 60 s outages. Trade-off (EU): channels 36–48 allow 200 mW EIRP, channels 100–140 allow 1 W, so leaving DFS can shorten 5 GHz range.',
      cmds: [set(legacy ? 'frequency' : 'channel.frequency', freq)], intent: { kind: 'wifi-radio', package: r.pkg, interface: r.name, frequency: freq } })
    if ((r.width ?? 0) >= 80) out.push({ title: 'Narrow 5 GHz to 40 MHz', risk: 'Medium', what: 'Stronger, steadier edge of coverage; top speed close to the router drops.',
      cmds: [set(legacy ? 'channel-width' : 'channel.width', width)], intent: { kind: 'wifi-radio', package: r.pkg, interface: r.name, width } })
    if (why.sameSsid) out.push({ title: `Give 5 GHz its own name (${ssid})`, risk: 'Medium', what: 'Then you choose the band: join this device to the 5 GHz name and it stays there. Devices on 5 GHz must join the new name once.',
      cmds: [set(legacy ? 'ssid' : 'configuration.ssid', `"${ssid}"`)], intent: { kind: 'wifi-network', package: r.pkg, interfaces: [r.name], ssid } })
    if (!why.limited) out.push({ title: `Only accept good signal on 5 GHz (${minSig} dBm or better)`, risk: 'Medium', what: 'A simple band steering: devices too far away for 5 GHz are turned away and stay on 2.4 GHz instead of bouncing.',
      cmds: [`${path}/access-list/add interface=${r.name} signal-range=${minSig}..120 ${legacy ? 'authentication=yes forwarding=yes' : 'action=accept'} comment=tik:sigrange:${r.name}`, `${path}/access-list/add interface=${r.name} ${legacy ? 'authentication=no forwarding=no' : 'action=reject'} comment=tik:sigrange:${r.name}`],
      intent: { kind: 'wifi-signal-range', package: r.pkg, interface: r.name, min: minSig } })
    return out
  })
  const area = $derived(`0,60 ${pts.map((v, i) => `${(i / 47) * 300},${60 - (v / max) * 56}`).join(' ')} 300,60`)
  $effect(() => { keep('client.tab', tab) })
</script>

<!-- a dialog opened from the panel closes first; one Esc closes one layer -->
<svelte:window onkeydown={(e) => e.key === 'Escape' && !document.querySelector('[role="dialog"]') && close()} />
<button class="side-scrim" aria-label="Close" onclick={close}></button>
<aside class="side-panel" aria-label="Client details" tabindex="-1" bind:this={panel}>
  <header class="row between">
    <div class="row">
      <div class="icon-box lg"><Icon name={iconOf(me)} size={22} /></div>
      <div>
        {#if editing}
          <!-- svelte-ignore a11y_autofocus -->
          <input type="text" bind:value={draft} autofocus onkeydown={(e) => e.key === 'Enter' && save()} onblur={save} placeholder={c.name} />
        {:else}
          <button class="title" onclick={() => { draft = displayName(c); editing = true }} title="Rename">{displayName(c)} <Icon name="edit" size={13} /></button>
        {/if}
        <div class="muted small">{subtitle(me, displayName(c))}</div>
      </div>
    </div>
    <button class="btn ghost" onclick={close} aria-label="Close"><Icon name="close" /></button>
  </header>

  <div class="seg tabs">
    <button class:on={tab === 'overview'} onclick={() => (tab = 'overview')}>Overview</button>
    <button class:on={tab === 'insights'} onclick={() => (tab = 'insights')}>Insights</button>
    <button class:on={tab === 'settings'} onclick={() => (tab = 'settings')}>Settings</button>
  </div>

  {#if tab === 'overview'}
    <div class="live row">
      <div><div class="muted small"><Icon name="down" size={12} /> Download</div><div class="big">{bps(live.rate[c.mac]?.rx ?? 0)}</div></div>
      <div><div class="muted small"><Icon name="up" size={12} /> Upload</div><div class="big">{bps(live.rate[c.mac]?.tx ?? 0)}</div></div>
    </div>
    <div class="actions">
      {#if paused}
        <button class="btn" onclick={() => review(`Resume internet for ${displayName(c)}`, [removeCmd('pause')], { kind: 'resume-client', mac: c.mac })}><Icon name="check" size={14} /> Resume</button>
      {:else}
        <button class="btn" onclick={() => review('Pause internet', [dropCmd('pause')], { kind: 'pause-client', mac: c.mac })}><Icon name="pause" size={14} /> Pause</button>
      {/if}
      <button class="btn" onclick={() => (tab = 'settings')}><Icon name="gauge" size={14} /> Limit</button>
      <button class="btn" disabled={c.fixedIp} onclick={() => review('Fixed IP', `/ip/dhcp-server/lease/make-static [find mac-address=${c.mac}]`, { kind: 'fixed-ip', mac: c.mac })}><Icon name="pin" size={14} /> {c.fixedIp ? 'Fixed IP' : 'Fix IP'}</button>
      {#if !c.wired}<button class="btn" onclick={() => review('Reconnect client', `/interface/wireless/registration-table/remove [find mac-address=${c.mac}]`, { kind: 'kick-wifi', mac: c.mac })}><Icon name="wifi" size={14} /> Reconnect</button>{/if}
      <button class="btn" onclick={() => { trafficDraft.mac = c.mac; location.hash = '#/settings/security' }}><Icon name="shield" size={14} /> Traffic rule</button>
      <button class="btn" onclick={() => review(isolated ? `End isolation for ${displayName(c)}` : `Isolate ${displayName(c)}`, [`/interface/bridge/filter/${isolated ? 'remove' : 'add'} … comment=tik:isolate:${c.mac}`], { kind: isolated ? 'unisolate-device' : 'isolate-device', mac: c.mac })}><Icon name="lock" size={14} /> {isolated ? 'End isolation' : 'Isolate'}</button>
      {#if isBackend && c.ip}<button class="btn" disabled={ping.busy} onclick={runPing}><Icon name="gauge" size={14} /> {ping.busy ? 'Pinging…' : 'Ping'}</button>{/if}
      {#if blocked}
        <button class="btn" onclick={() => review(`Unblock ${displayName(c)}`, [removeCmd('block')], { kind: 'unblock-client', mac: c.mac })}><Icon name="check" size={14} /> Unblock</button>
      {:else}
        <button class="btn danger" onclick={() => review('Block client', [dropCmd('block')], { kind: 'block-client', mac: c.mac })}><Icon name="block" size={14} /> Block</button>
      {/if}
    </div>
    {#if ping.text}<p class="small" class:state={!ping.ok} role="status">{ping.text}</p>{/if}
    {#if blocked || paused}<p class="small state" role="status"><Icon name="alert" size={13} /> {blocked ? 'Blocked: this device has no network access through the router.' : 'Paused: this device\'s internet is paused.'}</p>{/if}
    <dl>
      <dt>Experience</dt><dd><span class="pill {sq.level}">{sq.label}</span>{#if c.signal}<span class="mono muted"> {c.signal} dBm</span>{/if}</dd>
      <dt>Connection</dt><dd>{c.wired ? `Wired · ${c.port ?? 'unknown port'}` : `${c.ssid} · ${c.band} · ${c.port}`}</dd>
      {#if c.standard}<dt>WiFi standard</dt><dd>{c.standard}</dd>{/if}
      {#if c.txRate}<dt>Link rate</dt><dd class="mono">{c.txRate}</dd>{/if}
      {#if c.ccq != null}<dt>Link quality</dt><dd>{c.ccq}% CCQ</dd>{/if}
      <dt>IP address</dt><dd class="mono">{c.ip} {c.fixedIp ? '(fixed)' : '(DHCP)'}</dd>
      <dt>MAC address</dt><dd class="mono">{c.mac}</dd>
      <dt>Maker</dt><dd>{me.vendor ?? (c.privateMac ? (c.wired ? 'Private address' : 'Private Wi-Fi address') : 'Unknown')}{#if c.privateMac && !me.vendor}<div class="tiny muted">The device made up this address for privacy, so it can't name its maker.</div>{/if}</dd>
      {#if me.model}<dt>Model</dt><dd>{me.model}</dd>{/if}
      {#if me.type || me.os}<dt>Type</dt><dd>{[me.type, me.os].filter(Boolean).join(' · ')}{#if me.source}<div class="tiny muted">{SOURCES[me.source].label}{me.from ? ` for ${me.from}` : ''} · {SOURCES[me.source].confidence} confidence</div>{/if}</dd>{/if}
      {#if c.hostname && c.hostname !== displayName(c)}<dt>Hostname</dt><dd class="mono">{c.hostname}</dd>{/if}
      {#if c.classId}<dt>DHCP vendor class</dt><dd class="mono">{c.classId}</dd>{/if}
      {#if seen}<dt>First seen</dt><dd>{when(seen.first)}</dd><dt>Last seen</dt><dd>{when(seen.last)}</dd>{/if}
      <dt>Connected for</dt><dd>{dur(c.uptimeS)}</dd>
      {#if lease?.['expires-after']}<dt>Lease renews</dt><dd>in {lease['expires-after']}</dd>{/if}
      {#if c.bytesDown || c.bytesUp}<dt>Total traffic</dt><dd>{bytes(c.bytesDown)} down · {bytes(c.bytesUp)} up</dd>{/if}
      {#if prefs.notes[c.mac]}<dt>Notes</dt><dd>{prefs.notes[c.mac]}</dd>{/if}
    </dl>
    {#if said || heard}
      <section>
        <h3>What the device says</h3>
        {#each said?.kv ?? [] as [src, k, v]}<div class="row small ev"><span class="pill tiny">{src === 'mdns' ? 'mDNS' : 'SSDP'}</span> <span class="muted">{k}:</span> <span class="said">{v}</span></div>{/each}
        {#if heard}{#each [['asks for (55)', heard.prl], ['vendor class (60)', heard.class], ['host name (12)', heard.host], ['FQDN (81)', heard.fqdn]].filter(([, v]) => v) as [k, v]}<div class="row small ev"><span class="pill tiny">DHCP</span> <span class="muted">{k}:</span> <span class="said mono">{v}</span></div>{/each}{/if}
        <p class="tiny muted">{#if said}Asked {new Date(said.t * 1000).toLocaleString()} at {said.ip}. {/if}{#if heard}DHCP request heard {new Date(heard.t * 1000).toLocaleString()}. {/if}Devices describe themselves; treat it as a hint.</p>
      </section>
    {/if}
    {#if why}
      <section class="why" id="why-band">
        <h3>{why.switching ? 'Why does it keep switching bands?' : 'Why is it on 2.4 GHz?'}</h3>
        <ul>{#each why.reasons as r}<li>{r}</li>{/each}</ul>
        <h4>Band history</h4>
        {#each c.bandLog.slice(-10) as l}
          <div class="row small ev"><span class="muted mono">{l.time.slice(11, 16) || l.time}</span> {l.joined ? 'Joined' : 'Left'} {l.band ?? '?'} <span class="mono muted">{l.signal} dBm</span></div>
        {:else}<div class="muted small">No band changes in the recent log. Now: {c.band}{c.signal != null ? `, ${c.signal} dBm` : ''}.</div>{/each}
        {#if fixes.length}
          <h4>Fixes</h4>
          {#each fixes as f}
            <div class="fix">
              <div class="row between"><b class="small">{f.title}</b><span class="pill warn">{f.risk} risk</span></div>
              <p class="tiny muted">{f.what} Wi-Fi clients on {why.r5.band} reconnect when it applies; it reverts by itself unless you confirm.</p>
              {#if f.intent.kind === 'wifi-signal-range'}<label class="tiny">Minimum signal <input type="number" min="-90" max="-50" bind:value={minSig} /> dBm</label>{/if}
              <button class="btn small" onclick={() => review(f.title, f.cmds, f.intent)}>Review…</button>
            </div>
          {/each}
        {/if}
      </section>
    {/if}
  {:else if tab === 'insights'}
    <h3>Traffic, last 24 h</h3>
    {#if isBackend}
      {#each hours as [t, d, u]}
        <div class="hour small"><span class="muted mono">{new Date(t * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</span>
          <span class="track"><i class="d" style="width:{(d / hourMax) * 100}%"></i><i class="u" style="width:{(u / hourMax) * 100}%"></i></span>
          <span class="mono">{bytes(d + u)}</span></div>
      {:else}<div class="muted small">No stored history for this device yet.</div>{/each}
    {:else}
      <svg viewBox="0 0 300 60" class="chart" preserveAspectRatio="none"><polygon points={area} /></svg>
      <div class="row between tiny muted"><span>-24h</span><span>peak {bps(max)}</span><span>now</span></div>
    {/if}
    {#if !c.wired}
      <h3>Roaming history</h3>
      {#each roams as e}<div class="row small ev"><span class="muted mono">{e.time}</span> {e.text}</div>
      {:else}<div class="muted small">Stayed on one band.</div>{/each}
    {/if}
    <h3>Recent activity</h3>
    {#each history as e}<div class="row small ev"><span class="muted mono">{e.time.slice(11, 16)}</span> {e.text}</div>
    {:else}<div class="muted small">Nothing recent.</div>{/each}
    {#if !isBackend}<p class="tiny muted">Traffic chart is simulated without the tikontrol server.</p>{/if}
  {:else}
    <Field label="Icon"><div class="icons">{#each KINDS as k}<button class="ico" class:on={iconOf(c) === k} onclick={() => (prefs.icons[c.mac] = k)} aria-label={k}><Icon name={k} size={15} /></button>{/each}</div></Field>
    <Field label="Notes"><input type="text" value={prefs.notes[c.mac] ?? ''} oninput={(e) => (prefs.notes[c.mac] = (e.target as HTMLInputElement).value)} placeholder="e.g. Living room TV" /></Field>
    <Field label="Fixed IP"><input type="text" bind:value={s.fixedIp} /></Field>
    <Field label="Local DNS name" help="Reach it as name.lan"><input type="text" bind:value={s.dns} placeholder="tv" /></Field>
    <Field label="Network override" help={c.wired ? 'Wi-Fi clients only: this router\'s switch chip has no per-device VLAN for wired ports (use a port profile instead)' : 'Put this Wi-Fi device on another VLAN'}><input type="text" inputmode="numeric" bind:value={s.vlan} placeholder={vlanNow ? `VLAN ${vlanNow} now` : 'VLAN ID (none)'} disabled={c.wired} />
      {#if vlanNow}<button class="btn ghost" onclick={() => review(`Remove VLAN override for ${displayName(c)}`, [`# remove-wifi-client-vlan ${c.mac}`], { kind: 'remove-wifi-client-vlan', mac: c.mac })}>Remove</button>{/if}</Field>
    {#if !c.wired}
      <Field label="Own Wi-Fi password" help={ppskNow ? 'Set: this device joins with its own password (never shown)' : 'Private PSK: this device joins with its own password instead of the shared one'}>
        <input type="password" bind:value={psk} autocomplete="new-password" placeholder="8-63 characters" />
        <button class="btn" disabled={psk.length < 8} onclick={() => { review(`Own Wi-Fi password for ${displayName(c)}`, [`# wifi-private-psk ${c.mac} (password not shown)`], { kind: 'wifi-private-psk', mac: c.mac, passphrase: psk }); psk = '' }}>Set</button>
        {#if ppskNow}<button class="btn ghost" onclick={() => review(`Remove own Wi-Fi password for ${displayName(c)}`, [`# remove-wifi-private-psk ${c.mac}`], { kind: 'remove-wifi-private-psk', mac: c.mac })}>Remove</button>{/if}</Field>
    {/if}
    <Field label="Speed limit" help="Download/upload, e.g. 20M/5M"><input type="text" bind:value={s.limit} placeholder="Unlimited" />
      <button class="btn" disabled={!s.limit} onclick={() => { const i = limitIntent(); review(`Limit ${displayName(c)}`, [`/queue/simple/add name=tik-limit-${c.mac.replaceAll(':', '')} target=${c.ip}/32 max-limit=${i.up}/${i.down} comment=tik:limit:${c.mac}`], i) }}>Apply limit</button>
      <button class="btn ghost" onclick={() => review(`Remove limit for ${displayName(c)}`, [`/queue/simple/remove [find comment="tik:limit:${c.mac}"]`], { kind: 'unlimit-client', mac: c.mac })}>Remove</button></Field>
    <Field label="Scheduled pause"><Toggle bind:checked={s.pause} label="Scheduled pause" /></Field>
    {#if s.pause}<Field label="Pause between"><input type="text" bind:value={s.from} /><input type="text" bind:value={s.to} /></Field>{/if}
    <div class="save"><button class="btn primary" disabled={!settingsIntents().length} onclick={() => { const parts = settingsIntents(); review(`Settings for ${displayName(c)}`, parts.map((i) => `# ${i.kind}`), parts.length === 1 ? parts[0] : { kind: 'batch', title: `Settings for ${displayName(c)}`, intents: parts }) }}>Review & apply</button></div>
  {/if}
  <Ros cmd={`/ip/dhcp-server/lease/print where mac-address=${c.mac}; /interface/wireless/registration-table/print where mac-address=${c.mac}`} />
</aside>

<style>
  .lg { width: 44px; height: 44px; border-radius: 12px; }
  .title { background: none; border: 0; padding: 0; font-size: 17px; font-weight: 650; cursor: text; display: inline-flex; gap: 6px; align-items: center; }
  .title :global(svg) { color: var(--muted); }
  .tabs { margin: 16px 0 6px; }
  .live { gap: 32px; margin: 14px 0; }
  .actions { display: flex; gap: 8px; flex-wrap: wrap; }
  .state { color: var(--warn); margin: 10px 0 0; display: flex; gap: 6px; align-items: center; }
  dl { display: grid; grid-template-columns: 120px 1fr; gap: 8px 12px; margin: 20px 0; }
  dt { color: var(--muted); }
  dd { margin: 0; }
  h3 { font-size: 12.5px; margin: 18px 0 8px; }
  .chart { width: 100%; height: 70px; }
  .chart polygon { fill: color-mix(in srgb, var(--down) 30%, transparent); stroke: var(--down); stroke-width: 1; vector-effect: non-scaling-stroke; }
  .ev { padding: 4px 0; align-items: baseline; }
  .why ul { margin: 0; padding-left: 18px; display: grid; gap: 6px; font-size: 12.5px; }
  h4 { font-size: 12px; margin: 14px 0 6px; color: var(--muted); }
  .fix { border: 1px solid var(--line); border-radius: 8px; padding: 10px; margin-bottom: 8px; }
  .fix p { margin: 6px 0; }
  .fix input { width: 64px; }
  .tiny { font-size: 11px; }
  .said { overflow-wrap: anywhere; }
  .icons { display: flex; gap: 4px; flex-wrap: wrap; }
  .ico { width: 30px; height: 30px; border-radius: 7px; border: 1px solid var(--line); background: var(--panel2); display: grid; place-items: center; cursor: pointer; color: var(--muted); }
  .ico.on { border-color: var(--accent); color: var(--accent); }
  .save { text-align: right; margin-top: 12px; }
  .hour { display: grid; grid-template-columns: 50px 1fr 70px; gap: 8px; align-items: center; padding: 2px 0; }
  .track { display: flex; height: 8px; background: var(--panel2); border-radius: 4px; overflow: hidden; }
  .track i { display: block; height: 100%; }
  .d { background: var(--down); } .u { background: var(--up); }
</style>
