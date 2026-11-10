<script lang="ts">
  import { keep, kept } from '../lib/keep'
  import { clients, offline, isBackend, raw, subtitle, signalQuality, bps, dur, type Client } from '../lib/data'
  import { live, prefs, displayName, iconOf } from '../lib/live.svelte'
  import { review, type Intent } from '../lib/apply.svelte'
  import Tabs from '../lib/ui/Tabs.svelte'
  import Section from '../lib/ui/Section.svelte'
  import Icon from '../lib/Icon.svelte'
  import Ros from '../lib/Ros.svelte'
  import { api, session } from '../lib/backend'
  import { disc, identity, loadDiscovery } from '../lib/discovery.svelte'
  import ClientDrawer from './ClientDrawer.svelte'

  let { selected }: { selected?: string } = $props()
  const tab = $derived(selected === 'offline' || selected === 'groups' ? selected : 'online')
  let newGroup = $state('')
  let filter = $state<'all' | 'wifi' | 'wired' | 'guest'>(kept('clients.filter', 'all'))
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
  // a static lease or ARP entry alone is no proof the device is there: those are in Offline
  const online = $derived(clients.filter((c) => c.online))
  const rows = $derived(
    online
      .filter((c) => filter === 'all' || (filter === 'guest' ? c.guest : (filter === 'wifi') === !c.wired))
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
  // What a group made on the router: Apply members → address list tik-<slug> with comments tik:group:<slug>:<MAC>;
  // Apply limit → PCQ types, mangle marks and queue tree tagged tik:profile:<slug> (src/server/src/intents/profiles.rs).
  // (IP groups from Security use tik:group:<name> without a MAC and are not counted.)
  const MEMBER = /^tik:group:([A-Za-z0-9_-]{1,32}):[0-9A-Fa-f:]{17}$/
  const PROFILE = /^tik:profile:([A-Za-z0-9_-]{1,32})$/
  const madeOf = (s: string) => ({
    members: (raw.addressLists ?? []).filter((a: any) => !a.dynamic && a.list === `tik-${s}` && String(a.comment ?? '').match(MEMBER)?.[1] === s).length,
    limit: (raw.queueTree ?? []).some((t: any) => String(t.comment ?? '').match(PROFILE)?.[1] === s),
  })
  const onRouter = (m: { members: number; limit: boolean }) => m.members > 0 || m.limit
  const describe = (s: string, m: { members: number; limit: boolean }) => [
    m.members ? `${m.members} device${m.members === 1 ? '' : 's'} in address list tik-${s}` : '',
    m.limit ? `a speed limit (queue tree, mangle marks and PCQ types tagged tik:profile:${s})` : '',
  ].filter(Boolean).join(' and ')
  // the inverse of Apply limit and Apply members: remove-group-limit, then an empty client-group (removes list tik-<slug>)
  function removeFromRouter(s: string, name: string) {
    const m = madeOf(s)
    const intents: Intent[] = [...(m.limit ? [{ kind: 'remove-group-limit', group: s }] : []), ...(m.members ? [{ kind: 'client-group', group: s, members: [] }] : [])]
    const cmds = [
      ...(m.limit ? [`/queue/tree/remove [find comment="tik:profile:${s}"]`, `/ip/firewall/mangle/remove [find comment="tik:profile:${s}"]`, `/queue/type/remove [find name="tik-${s}-down"]`, `/queue/type/remove [find name="tik-${s}-up"]`] : []),
      ...(m.members ? [`/ip/firewall/address-list/remove [find list="tik-${s}"]`] : []),
    ]
    review(`Remove group ${name} from the router`, cmds, { kind: 'batch', title: `Remove group ${name} from the router`, intents })
  }
  let deleting = $state<number | null>(null)
  function del(gi: number) {
    if (onRouter(madeOf(slug(prefs.groups[gi].name)))) deleting = gi
    else prefs.groups.splice(gi, 1)
  }
  // on the router but no group here any more (deleted earlier, or made from another browser before it synced)
  const leftovers = $derived([...new Set([
    ...(raw.addressLists ?? []).map((a: any) => String(a.comment ?? '').match(MEMBER)?.[1]),
    ...(raw.queueTree ?? []).map((t: any) => String(t.comment ?? '').match(PROFILE)?.[1]),
  ])].filter((s): s is string => !!s && !prefs.groups.some((g) => slug(g.name) === s)).map((s) => ({ s, made: madeOf(s) })).filter((x) => onRouter(x.made)))
  // LAN discovery (server side, opt-in): run now, or every 10 min when the setting is on
  const isAdmin = isBackend && session.me?.role === 'admin'
  let discErr = $state('')
  async function discover() {
    discErr = ''; disc.running = true
    try { await api('/discovery/run', { method: 'POST' }) } catch (e) { discErr = (e as Error).message }
    await loadDiscovery()
  }
  async function setAuto(auto: boolean) {
    discErr = ''
    try { await api('/discovery', { method: 'PUT', body: { auto } }) } catch (e) { discErr = (e as Error).message }
    await loadDiscovery()
  }
  const counts = $derived({ all: online.length, wifi: online.filter((c) => !c.wired).length, wired: online.filter((c) => c.wired).length, guest: online.filter((c) => c.guest).length })
  $effect(() => { keep('clients.filter', filter); keep('clients.sort', sort); keep('clients.q', q) })
</script>

<div class="grid">
  <Tabs base="clients" active={tab} tabs={[{ id: 'online', label: 'Online', count: online.length }, { id: 'offline', label: 'Offline', count: offlineRows.length }, { id: 'groups', label: 'Groups', count: prefs.groups.length }]} />
  {#if tab === 'online'}
  <div class="row between wrap">
    <h1>Clients <span class="muted">{rows.length}</span></h1>
    <div class="row wrap">
      <div class="seg" role="tablist">
        {#each ['all', 'wifi', 'wired', 'guest'] as const as f}
          <button class:on={filter === f} onclick={() => (filter = f)}>{f === 'all' ? 'All' : f === 'wifi' ? 'WiFi' : f === 'wired' ? 'Wired' : 'Guests'} {counts[f]}</button>
        {/each}
      </div>
      <input type="search" placeholder="Search name, IP, MAC…" bind:value={q} />
      {#if isAdmin}<button class="btn" disabled={disc.running || disc.wait > 0} onclick={discover} title="Asks devices on your LAN over mDNS and SSDP; nothing leaves your network">{disc.running ? 'Asking…' : 'Discover devices'}</button>{/if}
    </div>
  </div>

  <section class="card flush">
    <table>
      <thead>
        <tr>
          <th><button class="plain" onclick={() => sortBy('name')}>Name {arrow('name')}</button></th>
          <th><button class="plain" onclick={() => sortBy('ip')}>IP address {arrow('ip')}</button></th>
          <th><button class="plain" onclick={() => sortBy('conn')}>Connection {arrow('conn')}</button></th>
          <th><button class="plain" onclick={() => sortBy('signal')}>Experience {arrow('signal')}</button></th>
          <th class="num"><button class="plain" onclick={() => sortBy('traffic')}>Down / Up {arrow('traffic')}</button></th>
          <th class="num"><button class="plain" onclick={() => sortBy('uptime')}>Connected {arrow('uptime')}</button></th>
        </tr>
      </thead>
      <tbody>
        {#each rows as c (c.mac)}
          {@const sq = signalQuality(c.signal)}
          {@const id = identity(c)}
          <tr onclick={() => (location.hash = `#/clients/${c.mac}`)} class:sel={c.mac === selected}>
            <td>
              <div class="row">
                <div class="icon-box"><Icon name={iconOf(id)} /></div>
                <div>
                  <a class="name plain" href="#/clients/{c.mac}">{displayName(c)}</a>
                  <div class="muted small">{subtitle(id, displayName(c))}</div>
                </div>
              </div>
            </td>
            <td class="mono">{c.ip}{#if c.fixedIp}<span class="pill tiny">fixed</span>{/if}</td>
            <td>
              <span class="row small">
                <Icon name={c.wired ? 'wired' : 'wifi'} size={14} />
                {c.wired ? (c.port ? `Port ${c.port.replace('ether', '')}` : 'Wired') : `${c.ssid} · ${c.band}`}
                {#if c.guest}<span class="pill tiny" title="On a guest network">guest</span>{/if}
              </span>
            </td>
            <td>
              <span class="pill {sq.level}">{sq.label}</span>
              {#if c.signal}<span class="muted small mono"> {c.signal} dBm</span>{/if}
            </td>
            <td class="num mono">{bps(live.rate[c.mac]?.rx ?? 0)} <span class="muted">/ {bps(live.rate[c.mac]?.tx ?? 0)}</span></td>
            <td class="num muted">{dur(c.uptimeS)}</td>
          </tr>
        {:else}
          <tr class="nohover"><td colspan="6" class="muted">{q || filter !== 'all' ? 'No client matches.' : 'No clients online right now. Devices seen before are under Offline.'}</td></tr>
        {/each}
      </tbody>
    </table>
  </section>
  {#if isAdmin}
    <label class="row small muted"><input type="checkbox" checked={disc.auto} onchange={(e) => setAuto((e.target as HTMLInputElement).checked)} /> Ask devices on the LAN for details (mDNS/SSDP, stays on your network) every 10 minutes</label>
    {#if discErr}<p class="small bad" role="alert">{discErr}</p>{/if}
  {/if}
  <Ros cmd="/ip/dhcp-server/lease/print; /interface/wireless/registration-table/print; /interface/bridge/host/print; /ip/arp/print" />
  {:else if tab === 'offline'}
    <Section title="Recently seen" desc="Devices the router still remembers (ARP / DHCP) that aren't connected right now." ros="/ip/arp/print where status!=reachable; /ip/dhcp-server/lease/print where status!=bound">
      <table>
        <thead><tr><th>Device</th><th>IP address</th><th>Last state</th><th></th></tr></thead>
        <tbody>
          {#each offlineRows as o}
            <tr class="nohover"><td class="row"><span class="icon-box"><Icon name="unknown" /></span><span class="mono">{o.mac}</span></td><td class="mono">{o.ip}</td><td class="muted">{o.seen}</td>
              <td class="r"><button class="btn" onclick={() => review('Forget device', [`/ip/arp/remove [find mac-address=${o.mac}]`, `/ip/dhcp-server/lease/remove [find mac-address=${o.mac} dynamic]`], { kind: 'forget-client', mac: o.mac })}>Forget</button></td></tr>
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
            <button class="btn ghost" aria-label="Delete group {g.name}" onclick={() => del(gi)}><Icon name="trash" size={14} /></button></span></div>
          {#if deleting === gi}
            {@const s = slug(g.name)}
            <div class="confirm" role="alert">
              <p class="small">{g.name} is still on the router: {describe(s, madeOf(s))}. Traffic rules that target this group stop matching once it is removed.</p>
              <div class="row wrap">
                <button class="btn danger" onclick={() => { removeFromRouter(s, g.name); prefs.groups.splice(gi, 1); deleting = null }}>Remove from router and delete</button>
                <button class="btn" onclick={() => { prefs.groups.splice(gi, 1); deleting = null }}>Delete here, keep on router</button>
                <button class="btn ghost" onclick={() => (deleting = null)}>Cancel</button>
              </div>
            </div>
          {/if}
          <div class="members">
            {#each clients as c}<label class="chip" class:on={g.members.includes(c.mac)}><input type="checkbox" hidden checked={g.members.includes(c.mac)} onchange={() => (g.members = g.members.includes(c.mac) ? g.members.filter((m) => m !== c.mac) : [...g.members, c.mac])} /><Icon name={iconOf(c)} size={12} /> {nameOf(c.mac)}</label>{/each}
          </div>
        </div>
      {/each}
    </Section>
    {#if leftovers.length}
      <Section title="Left on the router" desc="Made by a client group that no longer exists here. Remove it from the router, or add a group with the same name to manage it again." ros='/ip/firewall/address-list/print where comment~"^tik:group:"; /queue/tree/print where comment~"^tik:profile:"'>
        <table>
          <thead><tr><th>Group</th><th>On the router</th><th></th></tr></thead>
          <tbody>
            {#each leftovers as l (l.s)}
              <tr class="nohover"><td class="mono">{l.s}</td><td class="small">{describe(l.s, l.made)}</td>
                <td class="r"><button class="btn" onclick={() => removeFromRouter(l.s, l.s)}>Remove from router…</button></td></tr>
            {/each}
          </tbody>
        </table>
      </Section>
    {/if}
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
  .confirm { margin-top: 10px; padding: 10px; border: 1px solid var(--warn); border-radius: 8px; }
  .confirm p { margin: 0 0 8px; }
  .tiny { margin-left: 6px; font-size: 10.5px; padding: 0 6px; }
  input[type='search'] { width: 240px; }
  @media (max-width: 700px) { th:nth-child(2), td:nth-child(2), th:nth-child(6), td:nth-child(6) { display: none; } input[type='search'] { width: 140px; } }
</style>
