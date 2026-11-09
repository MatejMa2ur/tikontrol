<script lang="ts">
  import { raw, adblock } from '../../lib/data'
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

  const vlans: any[] = raw.vlans ?? []
  const v6pools: any[] = raw.ipv6Pools ?? []
  const v6addrs: any[] = raw.ipv6Addresses ?? []
  const FAMILY = '1.1.1.3,1.0.0.3' // Cloudflare for Families: malware + adult content blocked
  // content filter = which resolver the network's DHCP hands out
  const filterOf = (dnsServer: string, gw: string) => (dnsServer === FAMILY ? 'family' : dnsServer === gw && adblock.names ? 'ads' : 'none')
  const dnsFor = (f: string, gw: string, other: string) => (f === 'family' ? FAMILY.split(',') : f === 'ads' ? [gw] : other ? other.split(/[\s,]+/).filter(Boolean) : [gw])

  // existing networks = bridge/VLAN interfaces with an address (+ DHCP server)
  const existing = addrs.filter((a) => a.interface === 'bridge' || vlans.some((v) => v.name === a.interface)).map((a) => {
    const srv = dhcp.find((d) => d.interface === a.interface)
    const net = nets.find((n) => n.address.split('/')[0].split('.').slice(0, 3).join('.') === a.address.split('.').slice(0, 3).join('.'))
    const v = vlans.find((x) => x.name === a.interface)
    const gw = a.address.split('/')[0]
    const v6 = v6addrs.filter((x) => x.interface === a.interface && !String(x.address ?? '').startsWith('fe80'))
    return {
      name: a.interface === 'bridge' ? 'Default' : String(v?.comment ?? '').startsWith('tik:net:') ? v.comment.slice(8) : a.interface, iface: a.interface, vlan: v ? String(v['vlan-id']) : '—',
      subnet: net?.address ?? a.address, gateway: gw, dhcp: srv ? pools.find((p) => p.name === srv['address-pool'])?.ranges?.join(', ') : 'Off', lease: srv?.['lease-time'],
      domain: net?.domain ?? '', dns: String(net?.['dns-server'] ?? ''), filter: filterOf(String(net?.['dns-server'] ?? ''), gw), hasNet: !!net,
      v6: v6.map((x) => x.address).join(', '), v6tik: v6.some((x) => x.comment === `tik:v6lan:${a.interface}`),
    }
  })
  type Net = (typeof existing)[number]
  let ed = $state<(Net & { dnsText: string }) | null>(null)
  let v6pool = $state(v6pools[0]?.name ?? '')
  function editIntent(e: Net & { dnsText: string }) {
    const before = existing.find((x) => x.iface === e.iface)!
    const dns = dnsFor(e.filter, e.gateway, e.dnsText)
    return { kind: 'set-dhcp-network', address: e.subnet, ...(e.domain !== before.domain ? { domain: e.domain } : {}), ...(dns.join(',') !== before.dns ? { dns } : {}) }
  }

  // ---- "New network" form → ~9 RouterOS objects (PLAN §0.6 rule 1) ----
  let open = $state(false)
  let n = $state({ name: 'IoT', vlan: 30, subnet: '192.168.30.0/24', dhcp: true, lease: '1d', dns: 'router', isolate: true, internet: true, mdns: false, igmp: false, ipv6: false, domain: 'iot.lan', filter: 'none', options: '' })
  $effect(() => { n.subnet = `192.168.${n.vlan}.0/24` })
  const gw = $derived(n.subnet.replace(/\.0\/(\d+)$/, '.1'))
  const nDns = $derived(n.filter !== 'none' ? dnsFor(n.filter, gw, '') : n.dns === 'router' ? [] : [n.dns])
  const prefix = $derived(n.subnet.split('/')[1] ?? '24')
  const base = $derived(n.subnet.split('.').slice(0, 3).join('.'))
  const cmds = $derived([
    ...(bridge['vlan-filtering'] ? [] : ['# VLAN filtering is off: tagged ports need it, which is a separate high-risk change']),
    `/interface/vlan/add name=vlan${n.vlan} interface=bridge vlan-id=${n.vlan} comment="tik:${n.name}"`,
    `/interface/bridge/vlan/add bridge=bridge vlan-ids=${n.vlan} tagged=bridge comment="tik:${n.name}"`,
    `/ip/address/add address=${gw}/${prefix} interface=vlan${n.vlan} comment="tik:${n.name}"`,
    ...(n.dhcp ? [
      `/ip/pool/add name=tik-${n.name} ranges=${base}.10-${base}.254`,
      `/ip/dhcp-server/add name=tik-${n.name} interface=vlan${n.vlan} address-pool=tik-${n.name} lease-time=${n.lease}`,
      `/ip/dhcp-server/network/add address=${n.subnet} gateway=${gw} dns-server=${nDns.length ? nDns.join(',') : gw}${n.domain ? ` domain=${n.domain}` : ''}${n.options ? ` dhcp-option=${n.options}` : ''}`,
    ] : []),
    `/interface/list/member/add list=${n.isolate ? 'tik-isolated' : 'LAN'} interface=vlan${n.vlan}`,
    ...(n.isolate ? [`/ip/firewall/filter/add chain=forward in-interface=vlan${n.vlan} out-interface-list=!WAN action=drop comment="tik:${n.name} isolation" place-before=[find comment~"defconf: drop all from WAN"]`] : []),
    ...(!n.internet ? [`/ip/firewall/filter/add chain=forward in-interface=vlan${n.vlan} out-interface-list=WAN action=drop comment="tik:${n.name} no internet"`] : []),
    ...(n.mdns ? [`/ip/dns/set mdns-repeat-ifaces=bridge,vlan${n.vlan}`] : []),
    ...(n.igmp ? ['/interface/bridge/set bridge igmp-snooping=yes'] : []),
    ...(n.ipv6 && v6pool ? [`/ipv6/address/add interface=vlan${n.vlan} from-pool=${v6pool} advertise=yes`] : []),
    ...(n.filter !== 'none' ? [`# content filter: clients get DNS ${nDns.join(',')}`] : []),
  ])
  const objects = $derived(cmds.filter((c) => !c.startsWith('#')).length)
  // the server compiles create-network; extras (mDNS, IGMP, options) stay commands to copy
  const intent = $derived(!n.mdns && !n.igmp && !n.options && (!n.ipv6 || v6pool)
    ? { kind: 'create-network', name: n.name, vlan: n.vlan, subnet: n.subnet, dhcp: n.dhcp, lease_time: n.lease, dns: nDns, domain: n.domain, isolated: n.isolate, internet: n.internet, ...(n.ipv6 ? { ipv6_pool: v6pool } : {}) }
    : undefined)
  const FILTER_HELP = 'Sets the DNS server DHCP hands out. Devices with their own DNS or DNS over HTTPS bypass it.'
</script>

<Section title="Networks" desc="Each network is a subnet with its own DHCP, optionally on its own VLAN." ros="/ip/address/print; /ip/dhcp-server/print; /ip/dhcp-server/network/print; /interface/vlan/print">
  {#snippet actions()}<button class="btn primary" onclick={() => (open = true)}><Icon name="plus" size={14} /> New Network</button>{/snippet}
  <table>
    <thead><tr><th>Name</th><th>VLAN</th><th>Subnet</th><th>Gateway</th><th>DHCP range</th><th>Lease</th><th>Domain</th><th>Filter</th><th>IPv6</th></tr></thead>
    <tbody>
      {#each existing as e}<tr onclick={() => (ed = { ...e, dnsText: e.filter === 'none' ? e.dns : '' })}><td><button class="plain" onclick={(ev) => { ev.stopPropagation(); ed = { ...e, dnsText: e.filter === 'none' ? e.dns : '' } }}><b>{e.name}</b></button> <span class="muted small">{e.iface}</span></td><td>{e.vlan}</td><td class="mono">{e.subnet}</td><td class="mono">{e.gateway}</td><td class="mono small">{e.dhcp}</td><td>{e.lease ?? '—'}</td><td class="mono small">{e.domain || '—'}</td><td>{e.filter === 'family' ? 'Family-safe' : e.filter === 'ads' ? 'Ad-block' : '—'}</td><td class="mono small">{e.v6 || 'Off'}</td></tr>{/each}
    </tbody>
  </table>
</Section>

<Section title="Network-wide options" ros="/ip/dns/print; /interface/bridge/print; /ipv6/settings/print">
  <Field label="DNS servers"><span class="mono small">{(dns.servers ?? []).join(', ')}</span></Field>
  <Field label="mDNS across networks" help="Lets AirPlay/Chromecast work between VLANs"><span class="pill">{(dns['mdns-repeat-ifaces'] ?? []).length ? 'On' : 'Off'}</span></Field>
  <Field label="IGMP snooping"><span class="pill">{bridge['igmp-snooping'] ? 'On' : 'Off'}</span></Field>
  <Field label="VLAN filtering on bridge"><span class="pill">{bridge['vlan-filtering'] ? 'On' : 'Off'}</span></Field>
  <Field label="IPv6"><span class="pill">{v6off ? 'Disabled' : 'Enabled'}</span> <span class="muted small">{v6pools.length ? `Delegated pools: ${v6pools.map((p) => p.name).join(', ')}` : 'No delegated prefix yet (Internet → IPv6)'}</span></Field>
  <Field label="Local DNS records" help="Names the router answers for its networks"><span class="muted small">{(raw.dnsStatic ?? []).length} records · edit them under Routing → DNS</span></Field>
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
          <Field label="Content filtering" help={FILTER_HELP}><select bind:value={n.filter}><option value="none">None</option><option value="family">Family-safe (Cloudflare for Families)</option><option value="ads" disabled={!adblock.names}>Block ads (router DNS + ad list){adblock.names ? '' : ': add an ad list first'}</option></select></Field>
          <Field label="mDNS reflector"><Toggle bind:checked={n.mdns} label="mDNS" /></Field>
          <Field label="IGMP snooping"><Toggle bind:checked={n.igmp} label="IGMP" /></Field>
          <Field label="IPv6 (SLAAC)" na={v6pools.length ? undefined : 'Needs a delegated IPv6 prefix: turn on IPv6 under Internet first'} help="From the delegated prefix {v6pool}"><Toggle bind:checked={n.ipv6} disabled={!v6pools.length} label="IPv6" /></Field>
        </details>
      </div>
      <div>
        <div class="muted small"><b>{objects}</b> RouterOS objects from one form:</div>
        <pre class="cmds">{cmds.join('\n')}</pre>
      </div>
    </div>
    {#snippet footer()}<button class="btn" onclick={() => (open = false)}>Cancel</button><button class="btn primary" onclick={() => { review(`Create network ${n.name}`, cmds.filter((c) => !c.startsWith('#')), intent); open = false }}>Review & apply</button>{/snippet}
  </Modal>
{/if}

{#if ed}
  <Modal title="Edit {ed.name}" onclose={() => (ed = null)}>
    {#if !ed.hasNet}<p class="muted small">This network has no DHCP network entry, so domain and DNS can't be set here.</p>{/if}
    <Field label="Domain name" help="Search domain handed out by DHCP (e.g. home.lan)"><input type="text" bind:value={ed.domain} placeholder="none" disabled={!ed.hasNet} /></Field>
    <Field label="Content filtering" help={FILTER_HELP}><select bind:value={ed.filter} disabled={!ed.hasNet}><option value="none">None</option><option value="family">Family-safe (Cloudflare for Families)</option><option value="ads" disabled={!adblock.names}>Block ads (router DNS + ad list)</option></select></Field>
    {#if ed.filter === 'none'}<Field label="DNS for clients" help="Empty = the router"><input type="text" bind:value={ed.dnsText} placeholder={ed.gateway} disabled={!ed.hasNet} /></Field>{/if}
    <Field label="IPv6 (SLAAC)" na={v6pools.length ? undefined : 'Needs a delegated IPv6 prefix: turn on IPv6 under Internet first'} help={ed.v6 || 'Off'}>
      {#if ed.v6tik}<button class="btn small" onclick={() => { review(`Turn off IPv6 on ${ed!.iface}`, [`/ipv6/address/remove [find comment="tik:v6lan:${ed!.iface}"]`], { kind: 'ipv6-lan', interface: ed!.iface, pool: '' }); ed = null }}>Turn off</button>
      {:else if v6pools.length && !ed.v6}<select bind:value={v6pool}>{#each v6pools as p}<option>{p.name}</option>{/each}</select><button class="btn small" onclick={() => { review(`IPv6 on ${ed!.iface}`, [`/ipv6/address/add interface=${ed!.iface} from-pool=${v6pool} advertise=yes comment=tik:v6lan:${ed!.iface}`], { kind: 'ipv6-lan', interface: ed!.iface, pool: v6pool }); ed = null }}>Turn on</button>{/if}
    </Field>
    {#snippet footer()}<button class="btn" onclick={() => (ed = null)}>Cancel</button><button class="btn primary" disabled={!ed?.hasNet} onclick={() => { const i = editIntent(ed!); review(`DHCP settings for ${ed!.name}`, [`/ip/dhcp-server/network/set [find address="${ed!.subnet}"] domain=${ed!.domain || '""'} dns-server=${dnsFor(ed!.filter, ed!.gateway, ed!.dnsText).join(',')}`], i); ed = null }}>Review & apply</button>{/snippet}
  </Modal>
{/if}

<style>
  .cols { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 18px; }
  .cmds { background: var(--code-bg); border: 1px solid var(--line); border-radius: 8px; padding: 10px; font: 11px/1.6 var(--mono); white-space: pre-wrap; margin-top: 8px; max-height: 60vh; overflow: auto; }
  details { margin-top: 6px; }
  summary { cursor: pointer; color: var(--muted); padding: 6px 0; }
  .small { font-size: 12px; }
  .btn.small { padding: 3px 9px; }
  @media (max-width: 800px) { .cols { grid-template-columns: minmax(0, 1fr); } }
</style>
