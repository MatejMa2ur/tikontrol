<script lang="ts">
  import { untrack } from 'svelte'
  import { subtitle, signalQuality, bps, bytes, dur, events, type Client } from '../lib/data'
  import { live, prefs, displayName, iconOf } from '../lib/live.svelte'
  import { review } from '../lib/apply.svelte'
  import Icon from '../lib/Icon.svelte'
  import Ros from '../lib/Ros.svelte'
  import Field from '../lib/ui/Field.svelte'
  import Toggle from '../lib/ui/Toggle.svelte'

  let { client: c }: { client: Client } = $props()
  let tab = $state<'overview' | 'insights' | 'settings'>('overview')
  let editing = $state(false)
  let draft = $state('')

  const sq = $derived(signalQuality(c.signal))
  const history = $derived(events.filter((e) => e.text.includes(displayName(c)) || e.text.includes(c.name)).slice(0, 12))
  const roams = $derived(history.filter((e) => e.cat === 'WiFi'))
  const close = () => (location.hash = '#/clients')
  const save = () => { prefs.aliases[c.mac] = draft.trim(); if (!draft.trim()) delete prefs.aliases[c.mac]; editing = false }

  // settings form (seeded from the client)
  let s = $state({ fixedIp: untrack(() => c.ip), dns: '', vlan: '', limit: '', pause: false, days: 'Mon-Sun', from: '22:00', to: '07:00' })
  const KINDS = ['phone', 'computer', 'server', 'vm', 'speaker', 'iot', 'unknown']

  // 24h per-client traffic (synthetic, seeded by MAC) until history is stored
  const pts = $derived.by(() => {
    let seed = [...c.mac].reduce((a, ch) => a * 31 + ch.charCodeAt(0), 7) >>> 0
    const rnd = () => ((seed = (seed * 1664525 + 1013904223) % 4294967296) / 4294967296)
    const base = (c.bytesDown || 2e7) / 86400 * 8
    return Array.from({ length: 48 }, (_, i) => base * (0.2 + rnd() * (i > 30 ? 1.6 : 0.8)) * (rnd() < 0.05 ? 6 : 1))
  })
  const max = $derived(Math.max(...pts, 1))
  const area = $derived(`0,60 ${pts.map((v, i) => `${(i / 47) * 300},${60 - (v / max) * 56}`).join(' ')} 300,60`)
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && close()} />
<button class="scrim" aria-label="Close" onclick={close}></button>
<aside class="drawer" aria-label="Client details">
  <header class="row between">
    <div class="row">
      <div class="icon-box lg"><Icon name={iconOf(c)} size={22} /></div>
      <div>
        {#if editing}
          <!-- svelte-ignore a11y_autofocus -->
          <input type="text" bind:value={draft} autofocus onkeydown={(e) => e.key === 'Enter' && save()} onblur={save} placeholder={c.name} />
        {:else}
          <button class="title" onclick={() => { draft = displayName(c); editing = true }} title="Rename">{displayName(c)} <Icon name="edit" size={13} /></button>
        {/if}
        <div class="muted small">{subtitle(c, displayName(c))}</div>
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
      <button class="btn" onclick={() => review('Pause internet', [`/ip/kid-control/add name=tik-paused`, `/ip/kid-control/device/add name="${displayName(c)}" mac-address=${c.mac} user=tik-paused`, '/ip/kid-control/pause [find name=tik-paused]'], { kind: 'pause-client', mac: c.mac })}><Icon name="pause" size={14} /> Pause</button>
      <button class="btn" onclick={() => (tab = 'settings')}><Icon name="gauge" size={14} /> Limit</button>
      <button class="btn" disabled={c.fixedIp} onclick={() => review('Fixed IP', `/ip/dhcp-server/lease/make-static [find mac-address=${c.mac}]`, { kind: 'fixed-ip', mac: c.mac })}><Icon name="pin" size={14} /> {c.fixedIp ? 'Fixed IP' : 'Fix IP'}</button>
      {#if !c.wired}<button class="btn" onclick={() => review('Reconnect client', `/interface/wireless/registration-table/remove [find mac-address=${c.mac}]`, { kind: 'kick-wifi', mac: c.mac })}><Icon name="wifi" size={14} /> Reconnect</button>{/if}
      <button class="btn danger" onclick={() => review('Block client', [`/ip/firewall/address-list/add list=tik-blocked address=${c.ip} comment=${c.mac}`, '/ip/firewall/filter/add chain=forward src-address-list=tik-blocked action=drop comment="tik:block" place-before=0'], { kind: 'block-client', mac: c.mac })}><Icon name="block" size={14} /> Block</button>
    </div>
    <dl>
      <dt>Experience</dt><dd><span class="pill {sq.level}">{sq.label}</span>{#if c.signal}<span class="mono muted"> {c.signal} dBm</span>{/if}</dd>
      <dt>Connection</dt><dd>{c.wired ? `Wired · ${c.port ?? 'unknown port'}` : `${c.ssid} · ${c.band} · ${c.port}`}</dd>
      {#if c.standard}<dt>WiFi standard</dt><dd>{c.standard}</dd>{/if}
      {#if c.txRate}<dt>Link rate</dt><dd class="mono">{c.txRate}</dd>{/if}
      {#if c.ccq != null}<dt>Link quality</dt><dd>{c.ccq}% CCQ</dd>{/if}
      <dt>IP address</dt><dd class="mono">{c.ip} {c.fixedIp ? '(fixed)' : '(DHCP)'}</dd>
      <dt>MAC address</dt><dd class="mono">{c.mac}{c.privateMac ? ' · randomised' : ''}</dd>
      <dt>Connected for</dt><dd>{dur(c.uptimeS)}</dd>
      {#if c.bytesDown || c.bytesUp}<dt>Total traffic</dt><dd>{bytes(c.bytesDown)} down · {bytes(c.bytesUp)} up</dd>{/if}
      {#if prefs.notes[c.mac]}<dt>Notes</dt><dd>{prefs.notes[c.mac]}</dd>{/if}
    </dl>
  {:else if tab === 'insights'}
    <h3>Traffic, last 24 h</h3>
    <svg viewBox="0 0 300 60" class="chart" preserveAspectRatio="none"><polygon points={area} /></svg>
    <div class="row between tiny muted"><span>-24h</span><span>peak {bps(max)}</span><span>now</span></div>
    {#if !c.wired}
      <h3>Roaming history</h3>
      {#each roams as e}<div class="row small ev"><span class="muted mono">{e.time.slice(11, 16)}</span> {e.text}</div>
      {:else}<div class="muted small">Stayed on one band.</div>{/each}
    {/if}
    <h3>Recent activity</h3>
    {#each history as e}<div class="row small ev"><span class="muted mono">{e.time.slice(11, 16)}</span> {e.text}</div>
    {:else}<div class="muted small">Nothing recent.</div>{/each}
    <p class="tiny muted">Traffic chart is simulated until tikontrol stores history.</p>
  {:else}
    <Field label="Icon"><div class="icons">{#each KINDS as k}<button class="ico" class:on={iconOf(c) === k} onclick={() => (prefs.icons[c.mac] = k)} aria-label={k}><Icon name={k} size={15} /></button>{/each}</div></Field>
    <Field label="Notes"><input type="text" value={prefs.notes[c.mac] ?? ''} oninput={(e) => (prefs.notes[c.mac] = (e.target as HTMLInputElement).value)} placeholder="e.g. Living room TV" /></Field>
    <Field label="Fixed IP"><input type="text" bind:value={s.fixedIp} /></Field>
    <Field label="Local DNS name" help="Reach it as name.lan"><input type="text" bind:value={s.dns} placeholder="tv" /></Field>
    <Field label="Network override" help="Put this device on another VLAN"><input type="text" bind:value={s.vlan} placeholder="VLAN ID (none)" /></Field>
    <Field label="Speed limit" help="Download/upload, e.g. 20M/5M"><input type="text" bind:value={s.limit} placeholder="Unlimited" /></Field>
    <Field label="Scheduled pause"><Toggle bind:checked={s.pause} label="Scheduled pause" /></Field>
    {#if s.pause}<Field label="Pause between"><input type="text" bind:value={s.from} /><input type="text" bind:value={s.to} /></Field>{/if}
    <div class="save"><button class="btn primary" onclick={() => review(`Settings for ${displayName(c)}`, [
      ...(s.fixedIp !== c.ip || !c.fixedIp ? [`/ip/dhcp-server/lease/make-static [find mac-address=${c.mac}]`, `/ip/dhcp-server/lease/set [find mac-address=${c.mac}] address=${s.fixedIp}`] : []),
      ...(s.dns ? [`/ip/dns/static/add name=${s.dns}.lan address=${s.fixedIp} comment=tik:${c.mac}`] : []),
      ...(s.vlan ? [`/interface/bridge/port/set [find interface=${c.port}] pvid=${s.vlan}`] : []),
      ...(s.limit ? [`/queue/simple/add name="tik:${displayName(c)}" target=${s.fixedIp} max-limit=${s.limit}`] : []),
      ...(s.pause ? [`/ip/kid-control/add name="tik-${c.mac}" mon=${s.to}-${s.from} tue=${s.to}-${s.from} wed=${s.to}-${s.from} thu=${s.to}-${s.from} fri=${s.to}-${s.from} sat=${s.to}-${s.from} sun=${s.to}-${s.from}`, `/ip/kid-control/device/add mac-address=${c.mac} user="tik-${c.mac}"`] : []),
    ])}>Review & apply</button></div>
  {/if}
  <Ros cmd={`/ip/dhcp-server/lease/print where mac-address=${c.mac}; /interface/wireless/registration-table/print where mac-address=${c.mac}`} />
</aside>

<style>
  .scrim { position: fixed; inset: 0; background: #0006; border: 0; z-index: 20; cursor: default; }
  .drawer { position: fixed; top: 0; right: 0; bottom: 0; width: min(460px, 100vw); background: var(--panel); border-left: 1px solid var(--line); box-shadow: var(--shadow); z-index: 21; padding: 18px; overflow-y: auto; animation: slide .18s ease-out; }
  @keyframes slide { from { transform: translateX(30px); opacity: 0; } }
  .lg { width: 44px; height: 44px; border-radius: 12px; }
  .title { background: none; border: 0; padding: 0; font-size: 17px; font-weight: 650; cursor: text; display: inline-flex; gap: 6px; align-items: center; }
  .title :global(svg) { color: var(--muted); }
  .tabs { margin: 16px 0 6px; }
  .live { gap: 32px; margin: 14px 0; }
  .actions { display: flex; gap: 8px; flex-wrap: wrap; }
  dl { display: grid; grid-template-columns: 120px 1fr; gap: 8px 12px; margin: 20px 0; }
  dt { color: var(--muted); }
  dd { margin: 0; }
  h3 { font-size: 12.5px; margin: 18px 0 8px; }
  .chart { width: 100%; height: 70px; }
  .chart polygon { fill: color-mix(in srgb, var(--down) 30%, transparent); stroke: var(--down); stroke-width: 1; vector-effect: non-scaling-stroke; }
  .ev { padding: 4px 0; align-items: baseline; }
  .tiny { font-size: 11px; }
  .icons { display: flex; gap: 4px; flex-wrap: wrap; }
  .ico { width: 30px; height: 30px; border-radius: 7px; border: 1px solid var(--line); background: var(--panel2); display: grid; place-items: center; cursor: pointer; color: var(--muted); }
  .ico.on { border-color: var(--accent); color: var(--accent); }
  .save { text-align: right; margin-top: 12px; }
</style>
