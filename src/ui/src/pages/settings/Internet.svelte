<script lang="ts">
  import { internet, raw, bps } from '../../lib/data'
  import { review } from '../../lib/apply.svelte'
  import Icon from '../../lib/Icon.svelte'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'
  import Toggle from '../../lib/ui/Toggle.svelte'

  const dns: any = raw.dnsCfg ?? {}
  const cloud: any = raw.cloud ?? {}
  let wan = $state({ type: 'dhcp', ip: '', gw: '', user: '', pass: '', vlan: '' })
  let wan2 = $state({ on: false, port: 'ether5', type: 'dhcp', mode: 'failover' })
  let sq = $state({ on: false, down: 500, up: 50 })
  let d = $state({ servers: (dns.servers ?? []).join(', '), doh: dns['use-doh-server'] ?? '', ddns: !!cloud['dns-name'] })
  let v6 = $state({ on: false, pd: 56 })

  const wanCmds = () => [
    ...(wan.vlan ? [`/interface/vlan/add name=wan-vlan interface=${internet.wanIf} vlan-id=${wan.vlan}`] : []),
    ...(wan.type === 'dhcp' ? [`/ip/dhcp-client/set [find interface=${internet.wanIf}] disabled=no`] : []),
    ...(wan.type === 'static' ? [`/ip/dhcp-client/disable [find interface=${internet.wanIf}]`, `/ip/address/add address=${wan.ip} interface=${internet.wanIf}`, `/ip/route/add gateway=${wan.gw}`] : []),
    ...(wan.type === 'pppoe' ? [`/interface/pppoe-client/add name=pppoe-wan interface=${wan.vlan ? 'wan-vlan' : internet.wanIf} user=${wan.user} password="••••" add-default-route=yes use-peer-dns=yes`, '/interface/list/member/add list=WAN interface=pppoe-wan'] : []),
  ]
</script>

<Section title="Primary WAN · Port {internet.wanPort}" desc="How this router reaches the internet." ros="/ip/dhcp-client/print; /ip/route/print where dst-address=0.0.0.0/0">
  {#snippet actions()}<span class="pill ok"><span class="dot ok"></span>Connected</span>{/snippet}
  <div class="kv small">
    <span class="muted">WAN IP</span><span class="mono">{internet.wanIp}</span>
    <span class="muted">Gateway</span><span class="mono">{internet.gateway}</span>
    <span class="muted">Public IP</span><span class="mono">{internet.publicIp ?? '—'}</span>
    <span class="muted">Now</span><span><span class="down">↓ {bps(internet.rx)}</span> <span class="up">↑ {bps(internet.tx)}</span></span>
  </div>
  {#if internet.doubleNat}<div class="warnbox small"><Icon name="alert" size={14} /> Double NAT: the upstream device at {internet.gateway} also does NAT. Put it in bridge mode for working port forwards.</div>{/if}
  <Field label="Connection type"><div class="seg">{#each [['dhcp', 'DHCP'], ['static', 'Static'], ['pppoe', 'PPPoE']] as [v, l]}<button class:on={wan.type === v} onclick={() => (wan.type = v)}>{l}</button>{/each}</div></Field>
  {#if wan.type === 'static'}<Field label="IP / prefix"><input type="text" bind:value={wan.ip} placeholder="203.0.113.10/24" /></Field><Field label="Gateway"><input type="text" bind:value={wan.gw} /></Field>{/if}
  {#if wan.type === 'pppoe'}<Field label="Username"><input type="text" bind:value={wan.user} /></Field><Field label="Password"><input type="password" bind:value={wan.pass} /></Field>{/if}
  <Field label="VLAN tag" help="Some ISPs need a tagged WAN"><input type="text" bind:value={wan.vlan} placeholder="none" /></Field>
  <div class="r"><button class="btn primary" onclick={() => review('WAN settings', wanCmds())}>Review & apply</button></div>
</Section>

<Section title="Second WAN" desc="Failover or load balancing over another port (e.g. a 4G/5G modem).">
  <Field label="Enabled"><Toggle bind:checked={wan2.on} label="Second WAN" /></Field>
  {#if wan2.on}
    <Field label="Port"><select bind:value={wan2.port}><option>ether4</option><option>ether5</option></select></Field>
    <Field label="Mode"><div class="seg"><button class:on={wan2.mode === 'failover'} onclick={() => (wan2.mode = 'failover')}>Failover</button><button class:on={wan2.mode === 'balance'} onclick={() => (wan2.mode = 'balance')}>Load balance</button></div></Field>
    <div class="r"><button class="btn primary" onclick={() => review('Second WAN', [
      `/interface/list/member/add list=WAN interface=${wan2.port}`, `/interface/bridge/port/remove [find interface=${wan2.port}]`, `/ip/dhcp-client/add interface=${wan2.port} default-route-distance=${wan2.mode === 'failover' ? 2 : 1}`,
      ...(wan2.mode === 'failover' ? [`/ip/route/set [find dst-address=0.0.0.0/0 gateway=${internet.gateway}] check-gateway=ping`] : ['/ip/firewall/mangle/add chain=prerouting in-interface-list=LAN connection-state=new per-connection-classifier=both-addresses:2/0 action=mark-connection new-connection-mark=wan1', '/ip/firewall/mangle/add chain=prerouting in-interface-list=LAN connection-state=new per-connection-classifier=both-addresses:2/1 action=mark-connection new-connection-mark=wan2']),
    ])}>Review & apply</button></div>
  {/if}
</Section>

<Section title="Smart Queues" desc="Removes bufferbloat (lag while downloading). Set to ~90% of your real line speed." ros="/queue/type/add name=cake kind=cake">
  <Field label="Enabled"><Toggle bind:checked={sq.on} label="Smart Queues" /></Field>
  {#if sq.on}<Field label="Download / upload (Mbps)"><input type="number" bind:value={sq.down} /><input type="number" bind:value={sq.up} /></Field>{/if}
  {#if sq.on}<div class="r"><button class="btn primary" onclick={() => review('Smart Queues', ['/queue/type/add name=tik-cake kind=cake cake-diffserv=diffserv4 cake-nat=yes', `/queue/simple/add name=tik-smartqueue target=${internet.wanIf} max-limit=${sq.up}M/${sq.down}M queue=tik-cake/tik-cake`])}>Review & apply</button></div>{/if}
</Section>

<Section title="DNS" ros="/ip/dns/print">
  <Field label="Upstream DNS servers"><input type="text" bind:value={d.servers} /></Field>
  <Field label="Encrypted DNS (DoH)" help="e.g. https://cloudflare-dns.com/dns-query"><input type="text" bind:value={d.doh} placeholder="Off" /></Field>
  <div class="r"><button class="btn primary" onclick={() => review('DNS', [`/ip/dns/set servers=${d.servers.replace(/\s/g, '')}${d.doh ? ` use-doh-server=${d.doh} verify-doh-cert=yes` : ''}`])}>Review & apply</button></div>
</Section>

<Section title="Dynamic DNS" desc="A stable name for your public IP, from MikroTik's IP Cloud." ros="/ip/cloud/print">
  <Field label="Enabled"><Toggle bind:checked={d.ddns} label="DDNS" /></Field>
  <Field label="Name"><span class="mono small">{cloud['dns-name'] ?? '—'}</span></Field>
</Section>

<Section title="IPv6" desc="Ask the ISP for a prefix and hand it out on your networks.">
  <Field label="IPv6 on WAN"><Toggle bind:checked={v6.on} label="IPv6" /></Field>
  {#if v6.on}<Field label="Prefix delegation size"><select bind:value={v6.pd}><option value={48}>/48</option><option value={56}>/56</option><option value={60}>/60</option><option value={64}>/64</option></select></Field>
    <div class="r"><button class="btn primary" onclick={() => review('IPv6 WAN', [`/ipv6/dhcp-client/add interface=${internet.wanIf} request=prefix pool-name=isp-v6 pool-prefix-length=64 prefix-hint=::/${v6.pd} add-default-route=yes`, '/ipv6/address/add interface=bridge from-pool=isp-v6 advertise=yes'])}>Review & apply</button></div>{/if}
</Section>

<Section title="ISP status" desc="Outages and latency history are recorded once tikontrol runs continuously.">
  <div class="muted small">No outages recorded. Latency now: {internet.latency.map((l) => `${l.name} ${l.ms ?? '—'} ms`).join(' · ')}</div>
</Section>

<style>
  .kv { display: grid; grid-template-columns: 110px 1fr 110px 1fr; gap: 6px 12px; margin-bottom: 12px; }
  .warnbox { display: flex; gap: 8px; align-items: center; padding: 8px 10px; border-radius: 8px; background: color-mix(in srgb, var(--warn) 14%, transparent); color: var(--warn); margin-bottom: 8px; }
  .r { text-align: right; margin-top: 10px; }
  .small { font-size: 12px; }
  .down { color: var(--down); } .up { color: var(--up); }
</style>
