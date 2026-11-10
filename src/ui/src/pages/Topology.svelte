<script lang="ts">
  // Whole-network map: Internet → upstream router → this router → ports / Wi-Fi → clients → VMs.
  // Built from bridge hosts (which port a MAC sits on), the Wi-Fi registration table and DHCP leases.
  import { clients, internet, router, raw, signalQuality, bps, type Client } from '../lib/data'
  import { live, displayName } from '../lib/live.svelte'
  import Icon from '../lib/Icon.svelte'
  import Ros from '../lib/Ros.svelte'
  import { groupByNeighbor, type Downstream } from '../lib/topology'

  type N = { id: string; label: string; sub?: string; icon: string; kind: 'net' | 'dev' | 'client'; link?: string; wireless?: boolean; level?: string; href?: string; children: N[]; x?: number; y?: number }
  const mon: any[] = raw.etherMon ?? []
  const speed = (port?: string) => mon.find((m) => m.name === port)?.rate

  const clientNode = (c: Client): N => ({
    id: c.mac, label: displayName(c), sub: c.ip, icon: c.kind, kind: 'client', href: `#/clients/${c.mac}`, wireless: !c.wired,
    link: c.wired ? `Port ${c.port?.replace('ether', '') ?? '?'}${speed(c.port) ? ` · ${speed(c.port)}` : ''}` : `${c.band} · ${c.signal} dBm`,
    level: c.wired ? undefined : signalQuality(c.signal).level, children: [],
  })
  // A QEMU/KVM guest on the same port as a server is drawn under that server (VM on a host).
  const wired = clients.filter((c) => c.wired && c.online)
  const hosts = wired.filter((c) => c.kind === 'server')
  const vmOf = (c: Client) => (c.kind === 'vm' ? hosts.find((h) => h.port === c.port) : undefined)
  const withVms = (list: Client[]) => list.filter((c) => !vmOf(c)).map((c) => {
    const n = clientNode(c)
    n.children = wired.filter((v) => vmOf(v) === c).map((v) => ({ ...clientNode(v), link: 'Virtual machine' }))
    return n
  })
  // switches and APs heard over MNDP/LLDP sit between this router and the clients on their port
  const grouped = groupByNeighbor(clients.filter((c) => c.online), raw.neighbors ?? [])
  const devNode = (d: Downstream<Client>): N => ({
    id: d.id, label: d.label, sub: [d.board, d.sub].filter(Boolean).join(' · '), icon: 'devices', kind: 'dev',
    link: `Port ${d.port.replace('ether', '')}${speed(d.port) ? ` · ${speed(d.port)}` : ''}`,
    children: [...d.devices.map(devNode), ...withVms(d.clients)],
  })
  const neighborNodes = grouped.devices.map(devNode)
  const topWired = withVms(grouped.direct.filter((c) => c.wired))
  const wireless = grouped.direct.filter((c) => !c.wired).sort((a, b) => (a.band ?? '').localeCompare(b.band ?? '')).map(clientNode)

  const tree: N = {
    id: 'internet', label: 'Internet', sub: internet.publicIp, icon: 'globe', kind: 'net', children: [{
      id: 'upstream', label: internet.doubleNat ? 'Upstream router' : 'Modem', sub: internet.gateway, icon: 'devices', kind: 'dev', link: `WAN · Port ${internet.wanPort}${speed(internet.wanIf) ? ` · ${speed(internet.wanIf)}` : ''}`, children: [{
        id: 'gw', label: router.identity, sub: `${router.board} · ${internet.gatewayIp}`, icon: 'devices', kind: 'dev', href: '#/devices', link: speed(internet.wanIf), children: [...neighborNodes, ...topWired, ...wireless],
      }],
    }],
  }

  // tidy-ish tree layout: leaves get equal slots, parents centre over their children
  const W = 132, H = 150
  let slot = 0
  function place(n: N, depth: number) {
    n.y = depth * H + 40
    if (!n.children.length) n.x = slot++ * W + W / 2
    else { n.children.forEach((c) => place(c, depth + 1)); n.x = (n.children[0].x! + n.children.at(-1)!.x!) / 2 }
  }
  place(tree, 0)
  const all: N[] = []
  const walk = (n: N) => { all.push(n); n.children.forEach(walk) }
  walk(tree)
  const width = slot * W, height = Math.max(...all.map((n) => n.y!)) + 110
  const edges = all.flatMap((p) => p.children.map((c) => ({ p, c })))

  let q = $state('')
  let zoom = $state(1)
  let hover = $state<string | null>(null)
  const match = (n: N) => !q || `${n.label} ${n.sub ?? ''} ${n.id}`.toLowerCase().includes(q.toLowerCase())
  // Spotlight: highlight matches and their path up to the internet
  const lit = $derived.by(() => {
    if (!q && !hover) return null
    const set = new Set<string>()
    const up = (n: N, path: N[]): boolean => {
      const hit = (q ? match(n) : n.id === hover)
      let any = hit
      for (const c of n.children) if (up(c, [...path, n])) any = true
      if (any) { set.add(n.id); path.forEach((p) => set.add(p.id)) }
      return any
    }
    up(tree, [])
    return set
  })

  const wiredN = clients.filter((c) => c.online && c.wired).length
  const wifiN = clients.filter((c) => c.online && !c.wired).length
  const portsUp = mon.filter((m) => m.status === 'link-ok').length
  const listed = $derived(clients.filter((c) => c.online).filter((c) => !q || `${displayName(c)} ${c.ip} ${c.mac}`.toLowerCase().includes(q.toLowerCase())))
</script>

<div class="topo">
  <div class="main">
    <div class="row between bar wrap">
      <h1>Topology</h1>
      <div class="row">
        <div class="stats row small muted">
          <span><b>{2 + all.filter((n) => n.id.startsWith('nb:')).length}</b> network devices</span><span><b>{wiredN}</b> wired</span><span><b>{wifiN}</b> WiFi</span><span><b>{portsUp}</b>/{mon.length} ports up</span>
        </div>
        <input type="search" placeholder="Spotlight: name, IP, MAC…" bind:value={q} />
        <div class="seg"><button onclick={() => (zoom = Math.max(0.5, zoom - 0.15))} aria-label="Zoom out">−</button><button onclick={() => (zoom = 1)}>{Math.round(zoom * 100)}%</button><button onclick={() => (zoom = Math.min(1.6, zoom + 0.15))} aria-label="Zoom in">+</button></div>
      </div>
    </div>

    <div class="canvas card">
      <div class="stage" style="width:{width * zoom}px;height:{height * zoom}px">
        <div style="transform:scale({zoom});transform-origin:0 0;width:{width}px;height:{height}px;position:relative">
          <svg {width} {height}>
            {#each edges as { p, c }}
              {@const midY = (p.y! + 34 + c.y!) / 2}
              <path d="M{p.x} {p.y! + 34} V{midY} H{c.x} V{c.y! - 30}" class:wifi={c.wireless} class:dim={lit && !lit.has(c.id)} class:hot={lit?.has(c.id)} />
              {#if c.link}<text x={c.x} y={c.y! - 38} class:dim={lit && !lit.has(c.id)}>{c.link}</text>{/if}
            {/each}
          </svg>
          {#each all as n}
            <a class="node {n.kind}" href={n.href} style="left:{n.x}px;top:{n.y}px" class:dim={lit && !lit.has(n.id)} class:hit={q && match(n)}
              onmouseenter={() => (hover = n.id)} onmouseleave={() => (hover = null)}>
              <span class="ic {n.level ?? ''}"><Icon name={n.icon} size={n.kind === 'client' ? 18 : 22} /></span>
              <b>{n.label}</b>
              {#if n.sub}<small>{n.sub}</small>{/if}
              {#if n.kind === 'client'}<small class="rate">↓ {bps(live.rate[n.id]?.rx ?? 0)}</small>{/if}
            </a>
          {/each}
        </div>
      </div>
    </div>
    <div class="legend row small muted"><span><i class="ln"></i> Wired</span><span><i class="ln wifi"></i> WiFi</span><span><span class="dot ok"></span> Good signal</span><span><span class="dot warn"></span> Fair</span><span><span class="dot bad"></span> Poor</span></div>
    <Ros cmd="/interface/bridge/host/print where !local; /interface/wireless/registration-table/print; /ip/neighbor/print; /interface/ethernet/monitor [find] once" />
  </div>

  <aside class="list card">
    <h3>All connected devices <span class="muted">{listed.length}</span></h3>
    {#each listed as c (c.mac)}
      {@const sq = signalQuality(c.signal)}
      <a class="item row" href="#/clients/{c.mac}" onmouseenter={() => (hover = c.mac)} onmouseleave={() => (hover = null)}>
        <span class="icon-box"><Icon name={c.kind} size={15} /></span>
        <span class="grow"><b>{displayName(c)}</b><small class="muted">{c.ip} · {c.wired ? `Port ${c.port?.replace('ether', '')}` : c.band}</small></span>
        <span class="dot {c.wired ? 'ok' : sq.level}" title={sq.label}></span>
      </a>
    {/each}
  </aside>
</div>

<style>
  .topo { display: grid; grid-template-columns: minmax(0, 1fr) 280px; gap: 14px; }
  .bar { margin-bottom: 12px; gap: 10px; }
  .stats { gap: 14px; }
  .stats b { color: var(--text); }
  input[type='search'] { width: 220px; }
  .seg button { min-width: 30px; }
  .canvas { overflow: auto; padding: 10px; background-image: radial-gradient(var(--line) 1px, transparent 1px); background-size: 18px 18px; min-height: 560px; }
  .stage { position: relative; margin: 0 auto; }
  svg { position: absolute; inset: 0; overflow: visible; }
  path { fill: none; stroke: var(--muted); stroke-width: 1.5; opacity: .7; }
  path.wifi { stroke-dasharray: 4 4; stroke: var(--down); }
  path.hot { stroke: var(--accent); opacity: 1; stroke-width: 2; }
  text { font: 10.5px var(--font); fill: var(--muted); text-anchor: middle; }
  .dim { opacity: .18 !important; }
  .node { position: absolute; transform: translate(-50%, -30px); width: 122px; display: grid; justify-items: center; gap: 2px; text-align: center; color: var(--text); transition: opacity .15s; }
  .node b { font-size: 12px; max-width: 120px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .node small { font-size: 10.5px; color: var(--muted); }
  .node .rate { color: var(--down); }
  .ic { width: 60px; height: 60px; border-radius: 14px; display: grid; place-items: center; background: var(--panel); border: 1.5px solid var(--line); margin-bottom: 4px; }
  .client .ic { width: 46px; height: 46px; border-radius: 50%; }
  .dev .ic { border-color: var(--accent); color: var(--accent); }
  .net .ic { border-style: dashed; }
  .ic.ok { border-color: var(--ok); } .ic.warn { border-color: var(--warn); } .ic.bad { border-color: var(--bad); }
  .node:hover .ic, .hit .ic { box-shadow: 0 0 0 5px color-mix(in srgb, var(--accent) 25%, transparent); }
  .legend { gap: 16px; margin-top: 8px; }
  .ln { display: inline-block; width: 20px; border-top: 2px solid var(--muted); vertical-align: middle; }
  .ln.wifi { border-top: 2px dashed var(--down); }
  .list { padding: 12px; align-self: start; position: sticky; top: 62px; max-height: calc(100vh - 80px); overflow: auto; }
  h3 { margin: 2px 4px 10px; font-size: 13px; }
  .item { padding: 6px; border-radius: 8px; color: var(--text); }
  .item:hover { background: var(--panel2); }
  .grow { flex: 1; min-width: 0; display: grid; }
  .grow b { font-size: 12.5px; font-weight: 550; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .grow small { font-size: 11px; }
  @media (max-width: 1000px) { .topo { grid-template-columns: minmax(0, 1fr); } .list { position: static; max-height: none; } }
</style>
