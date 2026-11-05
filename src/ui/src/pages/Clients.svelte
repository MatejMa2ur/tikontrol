<script lang="ts">
  import { keep, kept } from '../lib/keep'
  import { clients, offline, isBackend, subtitle, signalQuality, bps, dur, type Client } from '../lib/data'
  import { live, prefs, displayName, iconOf } from '../lib/live.svelte'
  import { review } from '../lib/apply.svelte'
  import Tabs from '../lib/ui/Tabs.svelte'
  import Section from '../lib/ui/Section.svelte'
  import Icon from '../lib/Icon.svelte'
  import Ros from '../lib/Ros.svelte'
  import { api } from '../lib/backend'
  import ClientDrawer from './ClientDrawer.svelte'

  let { selected }: { selected?: string } = $props()
  const tab = $derived(selected === 'offline' || selected === 'groups' ? selected : 'online')
  let newGroup = $state('')
  let filter = $state<'all' | 'wifi' | 'wired'>(kept('clients.filter', 'all'))
  let q = $state(kept('clients.q', ''))
  let sort = $state<{ key: string; dir: 1 | -1 }>(kept('clients.sort', { key: 'name', dir: 1 }))

  const keyOf: Record<string, (c: Client) => string | number> = {
    name: (c) => displayName(c).toLowerCase(),
    ip: (c) => c.ip.split('.').map((n) => n.padStart(3, '0')).join('.'),
    conn: (c) => (c.wired ? 'z' + (c.port ?? '') : c.band ?? ''),
    signal: (c) => c.signal ?? 0,
    traffic: (c) => live.rate[c.mac]?.rx ?? 0,
    uptime: (c) => c.uptimeS ?? 0,
  }
  const rows = $derived(
    clients
      .filter((c) => filter === 'all' || (filter === 'wifi') === !c.wired)
      .filter((c) => !q || `${displayName(c)} ${c.ip} ${c.mac} ${c.vendor ?? ''} ${c.type ?? ''} ${c.os ?? ''} ${c.hostname ?? ''}`.toLowerCase().includes(q.toLowerCase()))
      .sort((a, b) => (keyOf[sort.key](a) > keyOf[sort.key](b) ? 1 : -1) * sort.dir),
  )
  const sortBy = (key: string) => (sort = { key, dir: sort.key === key ? (-sort.dir as 1 | -1) : 1 })
  const arrow = (key: string) => (sort.key === key ? (sort.dir === 1 ? '↑' : '↓') : '')
  const current = $derived(clients.find((c) => c.mac === selected))
  const nameOf = (mac: string) => { const c = clients.find((x) => x.mac === mac); return c ? displayName(c) : mac }
  // with the server, devices it has seen before (first/last seen) join the router's own offline list
  type Seen = { mac: string; first: number; last: number; interface?: string }
  let seen = $state<Seen[]>([])
  if (isBackend) api<Seen[]>('/clients/offline').then((v) => (seen = Array.isArray(v) ? v : [])).catch(() => {})
  const when = (t: number) => new Date(t * 1000).toLocaleString()
  const offlineRows = $derived([
    ...offline.map((o) => { const s = seen.find((x) => x.mac === o.mac); return { mac: o.mac, ip: o.ip, seen: s ? `last seen ${when(s.last)}` : o.seen } }),
    ...seen.filter((s) => !offline.some((o) => o.mac === s.mac) && !clients.some((c) => c.mac === s.mac)).sort((a, b) => b.last - a.last)
      .map((s) => ({ mac: s.mac, ip: '—', seen: `last seen ${when(s.last)} · first ${when(s.first)}${s.interface ? ` · ${s.interface}` : ''}` })),
  ])
  const slug = (n: string) => n.trim().replace(/[^A-Za-z0-9_-]+/g, '-').slice(0, 32)
  const groupMembers = (g: { members: string[] }) => g.members.map((m) => ({ mac: m, ip: clients.find((c) => c.mac === m)?.ip ?? '' })).filter((m) => m.ip)
  const applyGroup = (g: { name: string; members: string[] }) => {
    const s = slug(g.name), ms = groupMembers(g)
    review(`Group ${g.name}`, [`/ip/firewall/address-list/remove [find list=tik-${s}]`, ...ms.map((m) => `/ip/firewall/address-list/add list=tik-${s} address=${m.ip} comment=tik:group:${s}:${m.mac}`)], { kind: 'client-group', group: s, members: ms })
  }
  const applyLimit = (g: { name: string; limit: string }) => {
    const s = slug(g.name), [down, up] = g.limit.split('/').map((x) => x.trim())
    review(`Limit ${g.name}`, [`/queue/type/add name=tik-${s}-down kind=pcq pcq-rate=${down} pcq-classifier=dst-address`, `/queue/type/add name=tik-${s}-up kind=pcq pcq-rate=${up} pcq-classifier=src-address`, `# + mangle marks on list tik-${s} and /queue/tree entries, comment=tik:profile:${s}`], { kind: 'group-limit', group: s, down: down ?? '', up: up ?? down ?? '' })
  }
  const counts = { all: clients.length, wifi: clients.filter((c) => !c.wired).length, wired: clients.filter((c) => c.wired).length }
  $effect(() => { keep('clients.filter', filter); keep('clients.sort', sort); keep('clients.q', q) })
</script>

<div class="grid">
  <Tabs base="clients" active={tab} tabs={[{ id: 'online', label: 'Online', count: clients.length }, { id: 'offline', label: 'Offline', count: offlineRows.length }, { id: 'groups', label: 'Groups', count: prefs.groups.length }]} />
  {#if tab === 'online'}
  <div class="row between wrap">
    <h1>Clients <span class="muted">{rows.length}</span></h1>
    <div class="row">
      <div class="seg" role="tablist">
        {#each ['all', 'wifi', 'wired'] as const as f}
          <button class:on={filter === f} onclick={() => (filter = f)}>{f === 'all' ? 'All' : f === 'wifi' ? 'WiFi' : 'Wired'} {counts[f]}</button>
        {/each}
      </div>
      <input type="search" placeholder="Search name, IP, MAC…" bind:value={q} />
    </div>
  </div>

  <section class="card flush">
    <table>
      <thead>
        <tr>
          <th onclick={() => sortBy('name')}>Name {arrow('name')}</th>
          <th onclick={() => sortBy('ip')}>IP address {arrow('ip')}</th>
          <th onclick={() => sortBy('conn')}>Connection {arrow('conn')}</th>
          <th onclick={() => sortBy('signal')}>Experience {arrow('signal')}</th>
          <th onclick={() => sortBy('traffic')} class="num">Down / Up {arrow('traffic')}</th>
          <th onclick={() => sortBy('uptime')} class="num">Connected {arrow('uptime')}</th>
        </tr>
      </thead>
      <tbody>
        {#each rows as c (c.mac)}
          {@const sq = signalQuality(c.signal)}
          <tr onclick={() => (location.hash = `#/clients/${c.mac}`)} class:sel={c.mac === selected}>
            <td>
              <div class="row">
                <div class="icon-box"><Icon name={iconOf(c)} /></div>
                <div>
                  <div class="name">{displayName(c)}</div>
                  <div class="muted small">{subtitle(c, displayName(c))}</div>
                </div>
              </div>
            </td>
            <td class="mono">{c.ip}{#if c.fixedIp}<span class="pill tiny">fixed</span>{/if}</td>
            <td>
              <span class="row small">
                <Icon name={c.wired ? 'wired' : 'wifi'} size={14} />
                {c.wired ? (c.port ? `Port ${c.port.replace('ether', '')}` : 'Wired') : `${c.ssid} · ${c.band}`}
              </span>
            </td>
            <td>
              <span class="pill {sq.level}">{sq.label}</span>
              {#if c.signal}<span class="muted small mono"> {c.signal} dBm</span>{/if}
            </td>
            <td class="num mono">{bps(live.rate[c.mac]?.rx ?? 0)} <span class="muted">/ {bps(live.rate[c.mac]?.tx ?? 0)}</span></td>
            <td class="num muted">{dur(c.uptimeS)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>
  <Ros cmd="/ip/dhcp-server/lease/print; /interface/wireless/registration-table/print; /interface/bridge/host/print; /ip/arp/print" />
  {:else if tab === 'offline'}
    <Section title="Recently seen" desc="Devices the router still remembers (ARP / DHCP) that aren't connected right now." ros="/ip/arp/print where status!=reachable; /ip/dhcp-server/lease/print where status!=bound">
      <table>
        <thead><tr><th>Device</th><th>IP address</th><th>Last state</th><th></th></tr></thead>
        <tbody>
          {#each offlineRows as o}
            <tr class="nohover"><td class="row"><span class="icon-box"><Icon name="unknown" /></span><span class="mono">{o.mac}</span></td><td class="mono">{o.ip}</td><td class="muted">{o.seen}</td>
              <td class="r"><button class="btn" onclick={() => review('Forget device', [`/ip/arp/remove [find mac-address=${o.mac}]`, `/ip/dhcp-server/lease/remove [find mac-address=${o.mac} dynamic]`])}>Forget</button></td></tr>
          {:else}<tr><td colspan="4" class="muted">Nobody offline.</td></tr>{/each}
        </tbody>
      </table>
    </Section>
  {:else}
    <Section title="Client groups" desc="Group devices to apply a shared speed limit or schedule. Each group becomes a RouterOS address list + queue.">
      {#snippet actions()}<input type="text" bind:value={newGroup} placeholder="New group name" /><button class="btn" disabled={!newGroup} onclick={() => { prefs.groups.push({ name: newGroup, limit: '', members: [] }); newGroup = '' }}><Icon name="plus" size={14} /> Add</button>{/snippet}
      {#each prefs.groups as g, gi}
        <div class="group">
          <div class="row between"><b>{g.name}</b><span class="row"><input type="text" bind:value={g.limit} placeholder="Per device, down/up e.g. 20M/5M" aria-label="Speed limit" />
            <button class="btn primary" onclick={() => applyGroup(g)}>Apply members</button>
            <button class="btn" disabled={!g.limit} onclick={() => applyLimit(g)}>Apply limit</button>
            <button class="btn ghost" aria-label="Delete group" onclick={() => prefs.groups.splice(gi, 1)}><Icon name="trash" size={14} /></button></span></div>
          <div class="members">
            {#each clients as c}<label class="chip" class:on={g.members.includes(c.mac)}><input type="checkbox" hidden checked={g.members.includes(c.mac)} onchange={() => (g.members = g.members.includes(c.mac) ? g.members.filter((m) => m !== c.mac) : [...g.members, c.mac])} /><Icon name={iconOf(c)} size={12} /> {nameOf(c.mac)}</label>{/each}
          </div>
        </div>
      {/each}
    </Section>
  {/if}
</div>

{#if current}
  {#key current.mac}<ClientDrawer client={current} />{/key}
{/if}

<style>
  .flush { padding: 4px 6px; overflow-x: auto; }
  .name { font-weight: 550; }
  .num { text-align: right; }
  tr.sel { background: color-mix(in srgb, var(--accent) 10%, transparent); }
  .group { padding: 12px 0; border-top: 1px solid var(--line); }
  .group .row { gap: 8px; }
  .members { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 10px; }
  .chip { display: inline-flex; gap: 5px; align-items: center; padding: 3px 9px; border-radius: 99px; border: 1px solid var(--line); cursor: pointer; font-size: 12px; color: var(--muted); }
  .chip.on { border-color: var(--accent); color: var(--text); background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .r { text-align: right; }
  .tiny { margin-left: 6px; font-size: 10.5px; padding: 0 6px; }
  input[type='search'] { width: 240px; }
  @media (max-width: 700px) { th:nth-child(2), td:nth-child(2), th:nth-child(6), td:nth-child(6) { display: none; } input[type='search'] { width: 140px; } }
</style>
