<script lang="ts">
  import { internet, raw, bps, ports, dur } from '../../lib/data'
  import { review } from '../../lib/apply.svelte'
  import { isp } from '../../lib/live.svelte'
  import Icon from '../../lib/Icon.svelte'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'
  import Toggle from '../../lib/ui/Toggle.svelte'

  const dns: any = raw.dnsCfg ?? {}
  const cloud: any = raw.cloud ?? {}
  let wan = $state({ type: 'dhcp', ip: '', gw: '', user: '', pass: '', vlan: '' })
  // real state: tik-tagged second WAN route, Smart Queue and IPv6 pool from the snapshot
  const routes: any[] = raw.routes ?? []
  const primaryDist = Math.min(...routes.filter((r) => r['dst-address'] === '0.0.0.0/0' && r.active && !String(r.comment ?? '').startsWith('tik:failover:')).map((r) => +r.distance), 99)
  const second = routes.find((r) => String(r.comment ?? '').startsWith('tik:failover:'))
  const secondIf = second ? String(second.comment).slice(13) : ''
  const freePorts = ports.filter((p) => !p.wan).map((p) => p.name)
  const wanUp = (raw.wan ?? [])[0]?.status === 'bound' || routes.some((r) => r['dst-address'] === '0.0.0.0/0' && r.active)
  const sqRow = (raw.queues ?? []).find((q: any) => q.comment === 'tik:smartqueue')
  const mbps = (v?: string) => Math.round(parseInt(v ?? '0') / 1e6) || 0
  let wan2 = $state({ on: !!second, port: secondIf || freePorts.at(-1) || '', gw: second?.gateway ?? '', mode: second && +second.distance <= primaryDist ? 'balance' : 'failover' })
  let sq = $state({ on: !!sqRow, down: mbps(String(sqRow?.['max-limit'] ?? '').split('/')[1]) || 500, up: mbps(String(sqRow?.['max-limit'] ?? '').split('/')[0]) || 50 })
  const servers = Array.isArray(dns.servers) ? dns.servers : String(dns.servers ?? '').split(',').filter(Boolean)
  let d = $state({ servers: servers.join(', '), doh: dns['use-doh-server'] ?? '', ddns: cloud['ddns-enabled'] === true || cloud['ddns-enabled'] === 'yes' || !!cloud['dns-name'] })
  const v6Was = (raw.ipv6Pools ?? []).some((p: any) => p.name === 'tik-v6')
  // tagged WANs tikontrol created (remove-wan-vlan only removes tik:wanvlan:<id>)
  const wanVlans: any[] = (raw.vlans ?? []).filter((v: any) => String(v.comment ?? '') === `tik:wanvlan:${v['vlan-id']}`)
  let v6 = $state({ on: v6Was, pd: 56 })

  // the interface the connection type applies to: the tagged VLAN when one is set
  const wanIf = $derived(wan.vlan ? `wan-vlan${wan.vlan}` : internet.wanIf)
  const wanIntent = () => ({ kind: 'wan-type', interface: wanIf, type: wan.type, address: wan.ip, gateway: wan.gw, user: wan.user, password: wan.pass })
  const wanCmds = () => [
    ...(wan.type === 'dhcp' ? [`/ip/dhcp-client/set [find interface=${wanIf}] disabled=no`] : []),
    ...(wan.type === 'static' ? [`/ip/dhcp-client/set [find interface=${wanIf}] disabled=yes`, `/ip/address/add address=${wan.ip} interface=${wanIf}`, `/ip/route/add dst-address=0.0.0.0/0 gateway=${wan.gw}`] : []),
    ...(wan.type === 'pppoe' ? [`/interface/pppoe-client/add name=pppoe-wan interface=${wanIf} user=${wan.user} password="********" add-default-route=yes use-peer-dns=yes`, '/interface/list/member/add list=WAN interface=pppoe-wan'] : []),
  ]
  const serverList = () => d.servers.split(/[\s,]+/).filter(Boolean)
</script>

<Section title="Primary WAN · Port {internet.wanPort}" desc="How this router reaches the internet." ros="/ip/dhcp-client/print; /ip/route/print where dst-address=0.0.0.0/0">
  {#snippet actions()}{#if wanUp}<span class="pill ok"><span class="dot ok"></span>Connected</span>{:else}<span class="pill bad"><span class="dot bad"></span>No default route</span>{/if}{/snippet}
  <div class="kv small">
    <span class="muted">WAN IP</span><span class="mono">{internet.wanIp}</span>
    <span class="muted">Gateway</span><span class="mono">{internet.gateway}</span>
    <span class="muted">Public IP</span><span class="mono">{internet.publicIp ?? '—'}</span>
    <span class="muted">Now</span><span><span class="down">↓ {bps(internet.rx)}</span> <span class="up">↑ {bps(internet.tx)}</span></span>
  </div>
  {#if internet.doubleNat}<div class="warnbox small"><Icon name="alert" size={14} /> Double NAT: the upstream device at {internet.gateway} also does NAT. Put it in bridge mode for working port forwards.</div>{/if}
  <Field label="VLAN tag" help="Some ISPs need a tagged WAN. Add the VLAN first, then set the connection type on it.">
    <input type="number" min="1" max="4094" bind:value={wan.vlan} placeholder="none" />
    <button class="btn" disabled={!wan.vlan} onclick={() => review(`Tagged WAN VLAN ${wan.vlan}`, [`/interface/vlan/add name=wan-vlan${wan.vlan} interface=${internet.wanIf} vlan-id=${wan.vlan}`, `/interface/list/member/add list=WAN interface=wan-vlan${wan.vlan}`], { kind: 'wan-vlan', interface: internet.wanIf, vlan_id: +wan.vlan })}>Add VLAN</button>
  </Field>
  {#each wanVlans as v}
    <div class="muted small">Tagged WAN: <span class="mono">{v.name}</span> (VLAN {v['vlan-id']} on {v.interface}). <button class="btn small" onclick={() => review(`Remove tagged WAN VLAN ${v['vlan-id']}`, [`/interface/list/member/remove [find comment="tik:wanvlan:${v['vlan-id']}"]`, `/interface/vlan/remove [find comment="tik:wanvlan:${v['vlan-id']}"]`], { kind: 'remove-wan-vlan', vlan_id: +v['vlan-id'] })}>Remove</button></div>
  {/each}
  <Field label="Connection type" help={`Applies to ${wanIf}`}><div class="seg">{#each [['dhcp', 'DHCP'], ['static', 'Static'], ['pppoe', 'PPPoE']] as [v, l]}<button class:on={wan.type === v} onclick={() => (wan.type = v)}>{l}</button>{/each}</div></Field>
  {#if wan.type === 'static'}<Field label="IP / prefix"><input type="text" bind:value={wan.ip} placeholder="203.0.113.10/24" /></Field><Field label="Gateway"><input type="text" bind:value={wan.gw} placeholder="203.0.113.1" /></Field>{/if}
  {#if wan.type === 'pppoe'}<Field label="Username"><input type="text" bind:value={wan.user} autocomplete="off" /></Field><Field label="Password" help="Sent to the router only, never stored or shown"><input type="password" bind:value={wan.pass} autocomplete="new-password" /></Field>{/if}
  <div class="warnbox small"><Icon name="alert" size={14} /> High risk: a wrong setting cuts the internet until it reverts by itself after 5 minutes.</div>
  <div class="r"><button class="btn primary" onclick={() => review('WAN settings', wanCmds(), wanIntent())}>Review & apply</button></div>
</Section>

<Section title="Second WAN" desc="Failover to another port (e.g. a 4G/5G modem) when the primary gateway stops answering, or load balancing over both." ros="/ip/route/print where comment~&quot;tik:failover&quot;">
  {#if second}<div class="muted small">Active: {+second.distance <= primaryDist ? 'load balancing' : 'failover'} via <span class="mono">{second.gateway}</span> on {secondIf}. <button class="btn small" onclick={() => review('Remove second WAN', [`/ip/route/remove [find comment="${second.comment}"]`], { kind: 'remove-wan-failover', interface: secondIf })}>Remove</button></div>{/if}
  <Field label="Enabled"><Toggle bind:checked={wan2.on} label="Second WAN" /></Field>
  {#if wan2.on}
    <Field label="Port"><select bind:value={wan2.port}>{#each freePorts as p}<option>{p}</option>{/each}</select></Field>
    <Field label="Its gateway"><input type="text" bind:value={wan2.gw} placeholder="192.168.8.1" /></Field>
    <Field label="Mode"><div class="seg"><button class:on={wan2.mode === 'failover'} onclick={() => (wan2.mode = 'failover')}>Failover</button><button class:on={wan2.mode === 'balance'} onclick={() => (wan2.mode = 'balance')}>Load balance</button></div></Field>
    <div class="r">
      {#if wan2.mode === 'failover'}
        <button class="btn primary" disabled={!wan2.gw || !!second} onclick={() => review('Second WAN failover', [
          `/interface/list/member/add list=WAN interface=${wan2.port}`, `/ip/route/add dst-address=0.0.0.0/0 gateway=${wan2.gw} distance=2 check-gateway=ping`, `/tool/netwatch/add host=${wan2.gw} type=icmp interval=10s`,
        ], { kind: 'wan-failover', interface: wan2.port, gateway: wan2.gw })}>Review & apply</button>
      {:else}
        <button class="btn primary" disabled={!wan2.gw || !!second} onclick={() => review('Load balancing', [`/interface/list/member/add list=WAN interface=${wan2.port}`, `/ip/route/add dst-address=0.0.0.0/0 gateway=${wan2.gw} distance=${primaryDist === 99 ? 1 : primaryDist} check-gateway=ping`], { kind: 'wan-balance', interface: wan2.port, gateway: wan2.gw })}>Review & apply</button>
      {/if}
    </div>
  {/if}
</Section>

<Section title="Smart Queues" desc="Removes bufferbloat (lag while downloading). Set to ~90% of your real line speed." ros="/queue/type/print where kind=cake; /queue/simple/print">
  <Field label="Enabled"><Toggle bind:checked={sq.on} label="Smart Queues" /></Field>
  {#if sq.on}<Field label="Download / upload (Mbps)"><input type="number" bind:value={sq.down} /><input type="number" bind:value={sq.up} /></Field>{/if}
  <div class="r">
    {#if sq.on}<button class="btn primary" disabled={!(sq.down > 0 && sq.up > 0)} onclick={() => review('Smart Queues', ['/queue/type/add name=tik-cake kind=cake cake-diffserv=diffserv4 cake-nat=yes', `/queue/simple/add name=tik-smartqueue target=${internet.wanIf} max-limit=${sq.up}M/${sq.down}M queue=tik-cake/tik-cake comment=tik:smartqueue`], { kind: 'smart-queue', interface: internet.wanIf, down: `${sq.down}M`, up: `${sq.up}M` })}>Review & apply</button>
    {:else if sqRow}<button class="btn" onclick={() => review('Turn off Smart Queues', ['/queue/simple/remove [find comment="tik:smartqueue"]'], { kind: 'remove-smart-queue' })}>Turn off</button>{/if}
  </div>
</Section>

<Section title="DNS" ros="/ip/dns/print">
  <Field label="Upstream DNS servers"><input type="text" bind:value={d.servers} placeholder="1.1.1.1, 9.9.9.9" /></Field>
  <Field label="Encrypted DNS (DoH)" help="e.g. https://cloudflare-dns.com/dns-query"><input type="text" bind:value={d.doh} placeholder="Off" /></Field>
  <div class="r"><button class="btn primary" disabled={!serverList().length} onclick={() => review('DNS', [`/ip/dns/set servers=${serverList().join(',')} use-doh-server=${d.doh || '""'} verify-doh-cert=${d.doh ? 'yes' : 'no'}`], { kind: 'wan-dns', servers: serverList(), doh: d.doh.trim() })}>Review & apply</button></div>
</Section>

<Section title="Dynamic DNS" desc="A stable name for your public IP, from MikroTik's IP Cloud." ros="/ip/cloud/print">
  <Field label="Enabled"><Toggle bind:checked={d.ddns} label="DDNS" /></Field>
  <Field label="Name"><span class="mono small">{cloud['dns-name'] ?? '—'}</span></Field>
  <div class="r"><button class="btn primary" onclick={() => review(`${d.ddns ? 'Enable' : 'Disable'} dynamic DNS`, [`/ip/cloud/set ddns-enabled=${d.ddns ? 'yes' : 'no'}`], { kind: 'ddns', enabled: d.ddns })}>Review & apply</button></div>
</Section>

<Section title="IPv6" desc="Ask the ISP for a prefix and hand it out on your networks.">
  <Field label="IPv6 on WAN"><Toggle bind:checked={v6.on} label="IPv6" /></Field>
  {#if v6.on}<Field label="Prefix delegation size"><select bind:value={v6.pd}><option value={48}>/48</option><option value={56}>/56</option><option value={60}>/60</option><option value={64}>/64</option></select></Field>
    <div class="r"><button class="btn primary" onclick={() => review('IPv6 WAN', [`/ipv6/dhcp-client/add interface=${internet.wanIf} request=prefix pool-name=tik-v6 pool-prefix-length=64 prefix-hint=::/${v6.pd} add-default-route=yes use-peer-dns=yes comment=tik:v6wan`, '/ipv6/address/add interface=bridge from-pool=tik-v6 advertise=yes comment=tik:v6wan'], { kind: 'ipv6-wan', interface: internet.wanIf, lan: 'bridge', prefix_len: +v6.pd })}>Review & apply</button></div>
  {:else if v6Was}<div class="r"><button class="btn" onclick={() => review('Turn off IPv6 WAN', ['/ipv6/address/remove [find comment="tik:v6wan"]', '/ipv6/dhcp-client/remove [find comment="tik:v6wan"]'], { kind: 'remove-ipv6-wan' })}>Turn off</button></div>{/if}
</Section>

<Section title="ISP status" desc="WAN link uptime and outages, from the minutes tikontrol sampled (up to 7 days). Time tikontrol wasn't running counts as unknown.">
  {#if isp.uptime == null}
    <div class="muted small">Nothing recorded yet: tikontrol records this while it runs.</div>
  {:else}
    <div class="kv small">
      <span class="muted">Uptime</span><span><b>{isp.uptime}%</b> of {dur(isp.minutes * 60)} sampled</span>
      <span class="muted">Since</span><span>{new Date(isp.since! * 1000).toLocaleString()}</span>
      <span class="muted">Outages</span><span>{isp.count} · {dur(isp.downMinutes * 60)} down in total</span>
    </div>
    {#if isp.outages.length}
      <table class="small">
        <thead><tr><th>Started</th><th>Duration</th></tr></thead>
        <tbody>{#each isp.outages as o}<tr><td>{new Date(o.start * 1000).toLocaleString()}</td><td>{dur(o.secs)}{#if o.ongoing} <span class="pill bad">ongoing</span>{/if}</td></tr>{/each}</tbody>
      </table>
      {#if isp.count > isp.outages.length}<div class="muted small">Showing the latest {isp.outages.length}.</div>{/if}
    {:else}<div class="muted small">No outages recorded.</div>{/if}
  {/if}
  <div class="muted small">Latency now: {internet.latency.map((l) => `${l.name} ${l.ms ?? '—'} ms`).join(' · ')}</div>
</Section>

<style>
  .kv { display: grid; grid-template-columns: 110px 1fr 110px 1fr; gap: 6px 12px; margin-bottom: 12px; }
  @media (max-width: 760px) { .kv { grid-template-columns: 90px 1fr; } }
  .warnbox { display: flex; gap: 8px; align-items: center; padding: 8px 10px; border-radius: 8px; background: color-mix(in srgb, var(--warn) 14%, transparent); color: var(--warn); margin: 8px 0; }
  .r { text-align: right; margin-top: 10px; }
  .small { font-size: 12px; }
  .btn.small { padding: 3px 9px; }
  .down { color: var(--down); } .up { color: var(--up); }
</style>
