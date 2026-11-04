<script lang="ts">
  import { raw, clients, internet } from '../../lib/data'
  import { review } from '../../lib/apply.svelte'
  import Icon from '../../lib/Icon.svelte'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'
  import Toggle from '../../lib/ui/Toggle.svelte'

  const nat: any[] = raw.nat ?? []
  const routes: any[] = raw.routes ?? []
  const dnsStatic: any[] = raw.dnsStatic ?? []
  let fwd = $state({ name: '', port: '', to: '', toPort: '', proto: 'tcp' })
  let rt = $state({ dst: '', gw: '' })
  let dns = $state({ name: '', ip: '' })
  let upnp = $state({ on: !!raw.upnp?.enabled })
</script>

<Section title="Port forwarding" desc="Open a port from the internet to a device." ros="/ip/firewall/nat/print">
  {#if internet.doubleNat}<div class="muted small warn"><Icon name="alert" size={13} /> Behind double NAT: forwards also need to be set on {internet.gateway}.</div>{/if}
  <table><thead><tr><th>Name</th><th>Chain</th><th>Action</th><th>To</th></tr></thead>
    <tbody>{#each nat as r}<tr class="nohover"><td>{r.comment ?? '—'}</td><td>{r.chain}</td><td>{r.action}</td><td class="mono small">{r['to-addresses'] ?? ''}{r['to-ports'] ? `:${r['to-ports']}` : ''}</td></tr>{/each}</tbody></table>
  <div class="add">
    <input type="text" bind:value={fwd.name} placeholder="Name" />
    <select bind:value={fwd.proto}><option>tcp</option><option>udp</option></select>
    <input type="text" bind:value={fwd.port} placeholder="WAN port" style="width:90px" />
    <select bind:value={fwd.to}><option value="">Device…</option>{#each clients as c}<option value={c.ip}>{c.name} ({c.ip})</option>{/each}</select>
    <input type="text" bind:value={fwd.toPort} placeholder="Device port" style="width:100px" />
    <button class="btn primary" disabled={!fwd.name || !fwd.port || !fwd.to} onclick={() => review(`Port forward ${fwd.name}`, [`/ip/firewall/nat/add chain=dstnat in-interface-list=WAN protocol=${fwd.proto} dst-port=${fwd.port} action=dst-nat to-addresses=${fwd.to} to-ports=${fwd.toPort || fwd.port} comment="tik:${fwd.name}"`])}><Icon name="plus" size={14} /> Add</button>
  </div>
</Section>

<Section title="Static routes" ros="/ip/route/print">
  <table><thead><tr><th>Destination</th><th>Gateway</th><th>Distance</th><th>Table</th><th>Type</th></tr></thead>
    <tbody>{#each routes as r}<tr class="nohover"><td class="mono">{r['dst-address']}</td><td class="mono">{r.gateway}</td><td>{r.distance}</td><td>{r['routing-table']}</td><td><span class="pill">{r.dynamic ? 'Auto' : 'Static'}</span></td></tr>{/each}</tbody></table>
  <div class="add"><input type="text" bind:value={rt.dst} placeholder="10.0.0.0/8" /><input type="text" bind:value={rt.gw} placeholder="Gateway" />
    <button class="btn primary" disabled={!rt.dst || !rt.gw} onclick={() => review('Static route', [`/ip/route/add dst-address=${rt.dst} gateway=${rt.gw} comment=tik:route`])}><Icon name="plus" size={14} /> Add</button></div>
</Section>

<Section title="Local DNS records" desc="Names for devices on your network." ros="/ip/dns/static/print">
  <table><thead><tr><th>Name</th><th>Type</th><th>Address</th></tr></thead>
    <tbody>{#each dnsStatic as d}<tr class="nohover"><td class="mono">{d.name}</td><td>{d.type ?? 'A'}</td><td class="mono">{d.address}</td></tr>{/each}</tbody></table>
  <div class="add"><input type="text" bind:value={dns.name} placeholder="nas.lan" /><input type="text" bind:value={dns.ip} placeholder="10.0.0.50" />
    <button class="btn primary" disabled={!dns.name || !dns.ip} onclick={() => review(`DNS ${dns.name}`, [`/ip/dns/static/add name=${dns.name} address=${dns.ip} comment=tik:dns`])}><Icon name="plus" size={14} /> Add</button></div>
</Section>

<Section title="UPnP" desc="Lets games and consoles open ports themselves. Convenient, but any device can use it." ros="/ip/upnp/print">
  <Field label="Enabled"><Toggle bind:checked={upnp.on} label="UPnP" /></Field>
  <div class="r"><button class="btn primary" onclick={() => review('UPnP', upnp.on ? ['/ip/upnp/set enabled=yes', '/ip/upnp/interfaces/add interface=ether1 type=external', '/ip/upnp/interfaces/add interface=bridge type=internal'] : ['/ip/upnp/set enabled=no'])}>Review & apply</button></div>
</Section>

<Section title="Dynamic routing" desc="OSPF and BGP are shown read-only; configure them in RouterOS." ros="/routing/ospf/instance/print; /routing/bgp/connection/print">
  <div class="muted small">No OSPF or BGP instances.</div>
</Section>

<style>
  .add { display: flex; gap: 6px; flex-wrap: wrap; margin-top: 10px; }
  .warn { color: var(--warn); margin-bottom: 8px; }
  .r { text-align: right; margin-top: 10px; }
  .small { font-size: 12px; }
</style>
