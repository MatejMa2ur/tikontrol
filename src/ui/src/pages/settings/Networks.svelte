<script lang="ts">
  import { raw } from '../../lib/data'
  import { review } from '../../lib/apply.svelte'
  import Icon from '../../lib/Icon.svelte'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'
  import Toggle from '../../lib/ui/Toggle.svelte'
  import Modal from '../../lib/ui/Modal.svelte'

  const addrs: any[] = raw.addresses ?? []
  const dhcp: any[] = raw.dhcpServers ?? []
  const nets: any[] = raw.dhcpNetworks ?? []
  const pools: any[] = raw.pools ?? []
  const dns: any = raw.dnsCfg ?? {}
  const v6off = raw.ipv6?.['disable-ipv6']
  const bridge: any = (raw.bridges ?? [])[0] ?? {}

  // existing networks = bridge/VLAN interfaces with an address + DHCP server
  const existing = addrs.filter((a) => a.interface === 'bridge' || a.interface?.startsWith('vlan')).map((a) => {
    const srv = dhcp.find((d) => d.interface === a.interface)
    const net = nets.find((n) => n.address.split('/')[0].split('.').slice(0, 3).join('.') === a.address.split('.').slice(0, 3).join('.'))
    return { name: a.interface === 'bridge' ? 'Default' : a.interface, iface: a.interface, vlan: a.interface === 'bridge' ? '—' : a.interface.replace('vlan', ''), subnet: net?.address ?? a.address, gateway: a.address.split('/')[0], dhcp: srv ? pools.find((p) => p.name === srv['address-pool'])?.ranges?.join(', ') : 'Off', lease: srv?.['lease-time'] }
  })

  // ---- "New network" form → ~9 RouterOS objects (PLAN §0.6 rule 1) ----
  let open = $state(false)
  let n = $state({ name: 'IoT', vlan: 30, subnet: '192.168.30.0/24', dhcp: true, lease: '1d', dns: 'router', isolate: true, internet: true, mdns: false, igmp: false, ipv6: false, domain: 'iot.lan', filter: 'none', options: '' })
  $effect(() => { n.subnet = `192.168.${n.vlan}.0/24` })
  const gw = $derived(n.subnet.replace(/\.0\/(\d+)$/, '.1'))
  const prefix = $derived(n.subnet.split('/')[1] ?? '24')
  const base = $derived(n.subnet.split('.').slice(0, 3).join('.'))
  const cmds = $derived([
    ...(bridge['vlan-filtering'] ? [] : ['# turn on VLAN filtering once (safe apply guards this step)', '/interface/bridge/set bridge vlan-filtering=yes']),
    `/interface/vlan/add name=vlan${n.vlan} interface=bridge vlan-id=${n.vlan} comment="tik:${n.name}"`,
    `/interface/bridge/vlan/add bridge=bridge vlan-ids=${n.vlan} tagged=bridge comment="tik:${n.name}"`,
    `/ip/address/add address=${gw}/${prefix} interface=vlan${n.vlan} comment="tik:${n.name}"`,
    ...(n.dhcp ? [
      `/ip/pool/add name=tik-${n.name} ranges=${base}.10-${base}.254`,
      `/ip/dhcp-server/add name=tik-${n.name} interface=vlan${n.vlan} address-pool=tik-${n.name} lease-time=${n.lease}`,
      `/ip/dhcp-server/network/add address=${n.subnet} gateway=${gw} dns-server=${n.dns === 'router' ? gw : n.dns}${n.domain ? ` domain=${n.domain}` : ''}${n.options ? ` dhcp-option=${n.options}` : ''}`,
    ] : []),
    `/interface/list/member/add list=${n.isolate ? 'tik-isolated' : 'LAN'} interface=vlan${n.vlan}`,
    ...(n.isolate ? [`/ip/firewall/filter/add chain=forward in-interface=vlan${n.vlan} out-interface-list=!WAN action=drop comment="tik:${n.name} isolation" place-before=[find comment~"defconf: drop all from WAN"]`] : []),
    ...(!n.internet ? [`/ip/firewall/filter/add chain=forward in-interface=vlan${n.vlan} out-interface-list=WAN action=drop comment="tik:${n.name} no internet"`] : []),
    ...(n.mdns ? [`/ip/dns/set mdns-repeat-ifaces=bridge,vlan${n.vlan}`] : []),
    ...(n.igmp ? ['/interface/bridge/set bridge igmp-snooping=yes'] : []),
    ...(n.ipv6 ? [`/ipv6/address/add interface=vlan${n.vlan} from-pool=isp-v6 advertise=yes`] : []),
    ...(n.filter !== 'none' ? [`# content filter: DNS for this network goes to ${n.filter === 'family' ? 'family-safe resolvers' : 'ad-blocking resolver'}`, `/ip/dhcp-server/network/set [find address=${n.subnet}] dns-server=${n.filter === 'family' ? '1.1.1.3,1.0.0.3' : gw}`] : []),
  ])
  const objects = $derived(cmds.filter((c) => !c.startsWith('#')).length)
</script>

<Section title="Networks" desc="Each network is a subnet with its own DHCP, optionally on its own VLAN." ros="/ip/address/print; /ip/dhcp-server/print; /ip/dhcp-server/network/print; /interface/vlan/print">
  {#snippet actions()}<button class="btn primary" onclick={() => (open = true)}><Icon name="plus" size={14} /> New Network</button>{/snippet}
  <table>
    <thead><tr><th>Name</th><th>VLAN</th><th>Subnet</th><th>Gateway</th><th>DHCP range</th><th>Lease</th></tr></thead>
    <tbody>
      {#each existing as e}<tr class="nohover"><td><b>{e.name}</b> <span class="muted small">{e.iface}</span></td><td>{e.vlan}</td><td class="mono">{e.subnet}</td><td class="mono">{e.gateway}</td><td class="mono small">{e.dhcp}</td><td>{e.lease ?? '—'}</td></tr>{/each}
    </tbody>
  </table>
</Section>

<Section title="Network-wide options" ros="/ip/dns/print; /interface/bridge/print; /ipv6/settings/print">
  <Field label="DNS servers"><span class="mono small">{(dns.servers ?? []).join(', ')}</span></Field>
  <Field label="mDNS across networks" help="Lets AirPlay/Chromecast work between VLANs"><span class="pill">{(dns['mdns-repeat-ifaces'] ?? []).length ? 'On' : 'Off'}</span></Field>
  <Field label="IGMP snooping"><span class="pill">{bridge['igmp-snooping'] ? 'On' : 'Off'}</span></Field>
  <Field label="VLAN filtering on bridge"><span class="pill">{bridge['vlan-filtering'] ? 'On' : 'Off'}</span></Field>
  <Field label="IPv6"><span class="pill">{v6off ? 'Disabled' : 'Enabled'}</span></Field>
</Section>

{#if open}
  <Modal title="New network" onclose={() => (open = false)} wide>
    <div class="cols">
      <div>
        <Field label="Name"><input type="text" bind:value={n.name} /></Field>
        <Field label="VLAN ID"><input type="number" bind:value={n.vlan} min="2" max="4094" /></Field>
        <Field label="Subnet" help="Gateway {gw}"><input type="text" bind:value={n.subnet} /></Field>
        <Field label="Isolate network" help="No access to other networks, internet only"><Toggle bind:checked={n.isolate} label="Isolate" /></Field>
        <Field label="Internet access"><Toggle bind:checked={n.internet} label="Internet" /></Field>
        <details><summary class="small">Advanced</summary>
          <Field label="DHCP server"><Toggle bind:checked={n.dhcp} label="DHCP" /></Field>
          <Field label="Lease time"><select bind:value={n.lease}><option>30m</option><option>1h</option><option>1d</option><option>7d</option></select></Field>
          <Field label="DNS for clients"><select bind:value={n.dns}><option value="router">Router ({gw})</option><option value="1.1.1.1">Cloudflare</option><option value="9.9.9.9">Quad9</option></select></Field>
          <Field label="Domain name"><input type="text" bind:value={n.domain} /></Field>
          <Field label="DHCP options" help="e.g. option set name"><input type="text" bind:value={n.options} placeholder="none" /></Field>
          <Field label="Content filtering"><select bind:value={n.filter}><option value="none">None</option><option value="family">Family-safe</option><option value="ads">Block ads</option></select></Field>
          <Field label="mDNS reflector"><Toggle bind:checked={n.mdns} label="mDNS" /></Field>
          <Field label="IGMP snooping"><Toggle bind:checked={n.igmp} label="IGMP" /></Field>
          <Field label="IPv6" help="Needs an IPv6 prefix from your ISP"><Toggle bind:checked={n.ipv6} label="IPv6" /></Field>
        </details>
      </div>
      <div>
        <div class="muted small"><b>{objects}</b> RouterOS objects from one form:</div>
        <pre class="cmds">{cmds.join('\n')}</pre>
      </div>
    </div>
    {#snippet footer()}<button class="btn" onclick={() => (open = false)}>Cancel</button><button class="btn primary" onclick={() => { review(`Create network ${n.name}`, cmds.filter((c) => !c.startsWith('#'))); open = false }}>Review & apply</button>{/snippet}
  </Modal>
{/if}

<style>
  .cols { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 18px; }
  .cmds { background: var(--code-bg); border: 1px solid var(--line); border-radius: 8px; padding: 10px; font: 11px/1.6 var(--mono); white-space: pre-wrap; margin-top: 8px; max-height: 60vh; overflow: auto; }
  details { margin-top: 6px; }
  summary { cursor: pointer; color: var(--muted); padding: 6px 0; }
  .small { font-size: 12px; }
  @media (max-width: 800px) { .cols { grid-template-columns: minmax(0, 1fr); } }
</style>
