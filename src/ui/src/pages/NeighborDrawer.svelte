<script lang="ts">
  // Another MikroTik heard over MNDP/LLDP (#/device/<MAC>): read-only, from its own announcement. tikontrol manages
  // only the router it is connected to, so there are no actions here, just a link to the device's own WebFig.
  // Every field comes from that device: shown as plain text only, never put into a command.
  import { onMount } from 'svelte'
  import { router, clients, neighbors, parseUptime, dur } from '../lib/data'
  import { displayName } from '../lib/live.svelte'
  import { groupByNeighbor, webfigUrl, type NeighborRow } from '../lib/topology'
  import Icon from '../lib/Icon.svelte'

  let { neighbor: n, onclose: close }: { neighbor: NeighborRow; onclose: () => void } = $props()

  let panel: HTMLElement
  onMount(() => {
    const back = document.activeElement as HTMLElement | null
    panel.focus()
    return () => { if (back?.isConnected) back.focus() }
  })

  const mac = $derived(String(n['mac-address'] ?? '').toUpperCase())
  const port = $derived(String(n.interface ?? '').split(',')[0])
  // the bridge only knows the router's port: clients on it sit behind the first device heard there
  const all = groupByNeighbor(clients.filter((c) => c.online), neighbors).devices
  const head = $derived(all.find((d) => d.id === `nb:${mac}`))
  const behind = $derived(head?.clients ?? [])
  const chained = $derived(head?.devices ?? [])
  const via = $derived(head ? undefined : all.find((d) => d.devices.some((x) => x.id === `nb:${mac}`)))
  const up = $derived(parseUptime(n.uptime))
  const web = $derived(webfigUrl(n.address))
  const swos = $derived(/^CSS/i.test(n.board ?? '')) // Cloud Smart Switches run SwOS, not RouterOS
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && !document.querySelector('[role="dialog"]') && close()} />
<button class="side-scrim" aria-label="Close" onclick={close}></button>
<aside class="side-panel" aria-label="Device details" tabindex="-1" bind:this={panel}>
  <header class="row between">
    <div class="row"><span class="icon-box lg"><Icon name="devices" size={22} /></span><div><b class="title">{n.identity || n.board || mac}</b><div class="muted small">{[n.board, n.version && `${swos ? 'SwOS' : 'RouterOS'} ${n.version}`].filter(Boolean).join(' · ')}</div></div></div>
    <button class="btn ghost" onclick={close} aria-label="Close"><Icon name="close" /></button>
  </header>

  <p class="note small"><Icon name="alert" size={14} /> <span>tikontrol manages <b>{router.identity}</b> only. This device is shown from what it announces over neighbour discovery (MNDP/LLDP); change it in its own WebFig or WinBox.</span></p>

  <dl>
    <dt>Name</dt><dd>{n.identity || '—'}</dd>
    <dt>Model</dt><dd>{n.board || '—'}{#if n.platform}<span class="muted">{` · ${n.platform}`}</span>{/if}</dd>
    <dt>IP address</dt><dd class="mono">{n.address || '—'}</dd>
    <dt>MAC address</dt><dd class="mono">{mac || '—'}</dd>
    <dt>Version</dt><dd>{n.version || '—'}</dd>
    <dt>Connected to</dt><dd>{router.identity} · {port || '—'}{#if n['interface-name']}<span class="muted">{` (its ${n['interface-name']})`}</span>{/if}</dd>
    <dt>Uptime</dt><dd>{up != null ? dur(up) : '—'}</dd>
  </dl>

  {#if web}
    <p><a class="btn" href={web} target="_blank" rel="noopener noreferrer"><Icon name="globe" size={14} /> Open WebFig</a>
      <span class="muted small">Opens {web} in a new tab; needs the www-ssl service on that device.</span></p>
  {/if}

  <h3 class="small">Behind it</h3>
  {#if via}
    <p class="muted small">Heard on the same port as {via.label}: the router can't tell which of the two each client is on, so they are listed there.</p>
  {:else if !behind.length && !chained.length}
    <p class="muted small">No clients seen on {port || 'its port'}.</p>
  {:else}
    <ul class="list">
      {#each chained as d}<li><span class="icon-box"><Icon name="devices" size={15} /></span>{#if d.href}<a href={d.href}>{d.label}</a>{:else}{d.label}{/if}<span class="muted small mono">{d.sub}</span></li>{/each}
      {#each behind as c (c.mac)}<li><span class="icon-box"><Icon name={c.kind} size={15} /></span><a href="#/clients/{c.mac}">{displayName(c)}</a><span class="muted small mono">{c.ip}</span></li>{/each}
    </ul>
  {/if}
</aside>

<style>
  .small { font-size: 12px; }
  .lg { width: 44px; height: 44px; }
  .title { font-size: 17px; }
  .note :global(svg) { flex: none; margin-top: 2px; }
  .note { display: flex; gap: 8px; align-items: flex-start; margin: 16px 0; padding: 10px 12px; border-radius: 8px; border: 1px solid var(--line); border-left: 3px solid var(--accent); }
  dl { display: grid; grid-template-columns: 110px 1fr; gap: 8px 12px; }
  dt { color: var(--muted); } dd { margin: 0; overflow-wrap: anywhere; }
  h3 { margin: 18px 0 6px; }
  .list { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
  .list li { display: grid; grid-template-columns: auto 1fr auto; gap: 10px; align-items: center; }
  .list a { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
