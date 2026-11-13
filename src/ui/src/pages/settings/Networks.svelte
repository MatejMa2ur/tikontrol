<script lang="ts">
  import { raw, adblock } from '../../lib/data'
  import { review, type Intent } from '../../lib/apply.svelte'
  import Icon from '../../lib/Icon.svelte'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'
  import Toggle from '../../lib/ui/Toggle.svelte'
  import Modal from '../../lib/ui/Modal.svelte'
  import { poolUsage, overlap, suggest, span } from '../../lib/pool'

  const addrs: any[] = raw.addresses ?? []
  const dhcp: any[] = raw.dhcpServers ?? []
  const nets: any[] = raw.dhcpNetworks ?? []
  const pools: any[] = raw.pools ?? []
  const dns: any = raw.dnsCfg ?? {}
  const v6off = raw.ipv6?.['disable-ipv6']
  const bridge: any = (raw.bridges ?? [])[0] ?? {}
  const bports: any[] = (raw.bridgePorts ?? []).filter((p: any) => p.bridge === bridge.name)
  const bvlans: any[] = (raw.bridgeVlans ?? []).filter((v: any) => v.bridge === bridge.name)
  const dhcpOpts: any[] = raw.dhcpOptions ?? []
  const v6srv: any[] = raw.ipv6DhcpServers ?? []

  const vlans: any[] = raw.vlans ?? []
  const v6pools: any[] = raw.ipv6Pools ?? []
  const v6addrs: any[] = raw.ipv6Addresses ?? []
  const FAMILY = '1.1.1.3,1.0.0.3' // Cloudflare for Families: malware + adult content blocked
  // content filter = which resolver the network's DHCP hands out
  const filterOf = (dnsServer: string, gw: string) => (dnsServer === FAMILY ? 'family' : dnsServer === gw && adblock.names ? 'ads' : 'none')
  const dnsFor = (f: string, gw: string, other: string) => (f === 'family' ? FAMILY.split(',') : f === 'ads' ? [gw] : other ? other.split(/[\s,]+/).filter(Boolean) : [gw])
  // what remove-network removes, newest first (server: intents/networks.rs NET_PARTS)
  const NET_PATHS = ['/ipv6/address', '/ip/firewall/filter', '/interface/list/member', '/ip/dhcp-server/network', '/ip/dhcp-server', '/ip/pool', '/interface/list', '/ip/address', '/interface/bridge/vlan', '/interface/vlan']
  const RANGE = /^(\d{1,3}\.){3}\d{1,3}-(\d{1,3}\.){3}\d{1,3}$/
  const DURATION = /^\d[\dwdhms:]{0,15}$/

  // existing networks = bridge/VLAN interfaces with an address (+ DHCP server)
  const existing = addrs.filter((a) => a.interface === 'bridge' || vlans.some((v) => v.name === a.interface)).map((a) => {
    const srv = dhcp.find((d) => d.interface === a.interface)
    const net = nets.find((n) => n.address.split('/')[0].split('.').slice(0, 3).join('.') === a.address.split('.').slice(0, 3).join('.'))
    const v = vlans.find((x) => x.name === a.interface)
    const gw = a.address.split('/')[0]
    const v6 = v6addrs.filter((x) => x.interface === a.interface && !String(x.address ?? '').startsWith('fe80'))
    const ranges: string[] = pools.find((p) => p.name === srv?.['address-pool'])?.ranges ?? []
    const tikName = String(v?.comment ?? '').startsWith('tik:net:') ? v.comment.slice(8) : ''
    const d6 = v6srv.find((s) => s.interface === a.interface)
    return {
      name: a.interface === 'bridge' ? 'Default' : tikName || a.interface, iface: a.interface, vlan: v ? String(v['vlan-id']) : '—', tikName,
      subnet: net?.address ?? a.address, gateway: gw, dhcp: srv ? ranges.join(', ') : 'Off', lease: String(srv?.['lease-time'] ?? ''),
      server: String(srv?.name ?? ''), pool: String(srv?.['address-pool'] ?? ''), range: ranges.join(','), usage: srv ? poolUsage(ranges, raw.leases ?? []) : null,
      domain: net?.domain ?? '', dns: String(net?.['dns-server'] ?? ''), filter: filterOf(String(net?.['dns-server'] ?? ''), gw), hasNet: !!net,
      v6: v6.map((x) => x.address).join(', '), v6tik: v6.some((x) => x.comment === `tik:v6lan:${a.interface}`),
      v6pool: String(v6.find((x) => x['from-pool'])?.['from-pool'] ?? ''), dhcp6: !!d6, dhcp6tik: d6?.comment === `tik:dhcpv6:${a.interface}`,
    }
  })
  type Net = (typeof existing)[number]
  type Ed = Net & { dnsText: string }
  let ed = $state<Ed | null>(null)
  let v6pool = $state(v6pools[0]?.name ?? '')

  // the edit dialog: only what changed becomes an intent; several changes go as one batch
  function editParts(e: Ed): { intent: Intent; cli: string }[] {
    const before = existing.find((x) => x.iface === e.iface)!
    const out: { intent: Intent; cli: string }[] = []
    const dnsNew = dnsFor(e.filter, e.gateway, e.dnsText)
    if (e.hasNet && (e.domain !== before.domain || dnsNew.join(',') !== before.dns)) {
      out.push({ intent: { kind: 'set-dhcp-network', address: e.subnet, ...(e.domain !== before.domain ? { domain: e.domain } : {}), ...(dnsNew.join(',') !== before.dns ? { dns: dnsNew } : {}) }, cli: `/ip/dhcp-server/network/set [find address="${e.subnet}"] domain=${e.domain || '""'} dns-server=${dnsNew.join(',')}` })
    }
    if (e.server && e.lease.trim() !== before.lease) out.push({ intent: { kind: 'set-dhcp-lease', server: e.server, lease_time: e.lease.trim() }, cli: `/ip/dhcp-server/set [find name="${e.server}"] lease-time=${e.lease.trim()}` })
    if (e.pool && e.range.trim() !== before.range) out.push({ intent: { kind: 'set-dhcp-range', pool: e.pool, range: e.range.trim() }, cli: `/ip/pool/set [find name="${e.pool}"] ranges=${e.range.trim()}` })
    return out
  }
  const edBad = $derived(!!ed && ((!!ed.server && !DURATION.test(ed.lease.trim())) || (!!ed.pool && ed.range.trim() !== existing.find((x) => x.iface === ed!.iface)?.range && !RANGE.test(ed.range.trim()))))
  const edParts = $derived(ed ? editParts(ed) : [])
  function applyEdit(e: Ed) {
    const parts = edParts
    const title = `Settings for ${e.name}`
    review(title, parts.map((p) => p.cli), parts.length === 1 ? parts[0].intent : { kind: 'batch', title, intents: parts.map((p) => p.intent) })
    ed = null
  }

  // ---- "New network" form → ~9 RouterOS objects (PLAN §0.6 rule 1) ----
  const mdnsNow: string[] = dns['mdns-repeat-ifaces'] ?? []
  let open = $state(false)
  // suggest a VLAN ID and subnet nothing uses yet; the checks below and the server refuse clashes anyway
  const addrList: string[] = addrs.map((a) => String(a.address))
  const vlanIds: number[] = vlans.map((v) => Number(v['vlan-id']))
  const first = suggest(vlanIds, addrList)
  let n = $state({ name: 'IoT', vlan: first.vlan, subnet: first.subnet, dhcp: true, lease: '1d', dns: 'router', isolate: true, internet: true, mdns: false, igmp: false, ipv6: false, domain: 'iot.lan', filter: 'none' })
  $effect(() => { n.subnet = `192.168.${n.vlan}.0/24` })
  const gw = $derived(n.subnet.replace(/\.0\/(\d+)$/, '.1'))
  const nDns = $derived(n.filter !== 'none' ? dnsFor(n.filter, gw, '') : n.dns === 'router' ? [] : [n.dns])
  const prefix = $derived(n.subnet.split('/')[1] ?? '24')
  const base = $derived(n.subnet.split('.').slice(0, 3).join('.'))
  const nMdns = $derived([...new Set([...(mdnsNow.length ? mdnsNow : ['bridge']), `vlan${n.vlan}`])])
  const cmds = $derived([
    ...(bridge['vlan-filtering'] ? [] : ['# VLAN filtering is off: tagged ports need it, which is a separate high-risk change']),
    `/interface/vlan/add name=vlan${n.vlan} interface=bridge vlan-id=${n.vlan} comment="tik:net:${n.name}"`,
    `/interface/bridge/vlan/add bridge=bridge vlan-ids=${n.vlan} tagged=bridge comment="tik:net:${n.name}"`,
    `/ip/address/add address=${gw}/${prefix} interface=vlan${n.vlan} comment="tik:net:${n.name}"`,
    ...(n.dhcp ? [
      `/ip/pool/add name=tik-net-${n.name} ranges=${base}.10-${base}.254`,
      `/ip/dhcp-server/add name=tik-net-${n.name} interface=vlan${n.vlan} address-pool=tik-net-${n.name} lease-time=${n.lease}`,
      `/ip/dhcp-server/network/add address=${n.subnet} gateway=${gw} dns-server=${nDns.length ? nDns.join(',') : gw}${n.domain ? ` domain=${n.domain}` : ''}`,
    ] : []),
    `/interface/list/member/add list=LAN interface=vlan${n.vlan}`,
    ...(n.isolate ? [`/ip/firewall/filter/add chain=forward in-interface=vlan${n.vlan} out-interface-list=!WAN action=drop comment="tik:net:${n.name}" place-before=0`] : []),
    ...(!n.internet ? [`/ip/firewall/filter/add chain=forward in-interface=vlan${n.vlan} out-interface-list=WAN action=drop comment="tik:net:${n.name}"`] : []),
    ...(n.mdns ? [`/ip/dns/set mdns-repeat-ifaces=${nMdns.join(',')}`] : []),
    ...(n.igmp && !bridge['igmp-snooping'] ? [`/interface/bridge/set ${bridge.name ?? 'bridge'} igmp-snooping=yes`] : []),
    ...(n.ipv6 && v6pool ? [`/ipv6/address/add interface=vlan${n.vlan} from-pool=${v6pool} advertise=yes`] : []),
    ...(n.filter !== 'none' ? [`# content filter: clients get DNS ${nDns.join(',')}`] : []),
  ])
  const objects = $derived(cmds.filter((c) => !c.startsWith('#')).length)
  const nProblem = $derived(
    !/^[A-Za-z0-9_-]{1,32}$/.test(n.name) ? 'Name: letters, digits, - and _ only (up to 32).'
    : existing.some((e) => e.name.toLowerCase() === n.name.toLowerCase()) ? `A network named ${n.name} exists already.`
    : !Number.isInteger(n.vlan) || n.vlan < 2 || n.vlan > 4094 ? 'VLAN ID must be 2 to 4094.'
    : vlanIds.includes(n.vlan) ? `VLAN ${n.vlan} is in use already.`
    : !/^\d+\.\d+\.\d+\.\d+\/(1[6-9]|2[0-9]|30)$/.test(n.subnet) || !span(n.subnet) ? 'Subnet: like 192.168.30.0/24 (prefix /16 to /30).'
    : overlap(n.subnet, addrList) ? `${n.subnet} overlaps ${overlap(n.subnet, addrList)} on this router.`
    : n.ipv6 && !v6pool ? 'IPv6 needs a delegated prefix.' : '')
  // create-network, plus the network-wide extras it asks for, as one batch (one Safe apply, one undo)
  const intent = $derived.by((): Intent | undefined => {
    if (n.ipv6 && !v6pool) return undefined
    const parts: Intent[] = [
      { kind: 'create-network', name: n.name, vlan: n.vlan, subnet: n.subnet, dhcp: n.dhcp, lease_time: n.lease, dns: nDns, domain: n.domain, isolated: n.isolate, internet: n.internet, ...(n.ipv6 ? { ipv6_pool: v6pool } : {}) },
      ...(n.mdns ? [{ kind: 'set-mdns-repeat', interfaces: nMdns }] : []),
      ...(n.igmp && !bridge['igmp-snooping'] && bridge.name ? [{ kind: 'set-igmp-snooping', bridge: bridge.name, enabled: true }] : []),
    ]
    return parts.length === 1 ? parts[0] : { kind: 'batch', title: `Create network ${n.name}`, intents: parts }
  })
  const FILTER_HELP = 'Sets the DNS server DHCP hands out. Devices with their own DNS or DNS over HTTPS bypass it.'

  // ---- network-wide options ----
  const mdnsSupported = 'mdns-repeat-ifaces' in dns
  const mdnsChoices = [...new Set([...existing.map((e) => e.iface), ...mdnsNow])]
  let mdnsSel = $state<string[]>([...mdnsNow])
  const mdnsChanged = $derived([...mdnsSel].sort().join(',') !== [...mdnsNow].sort().join(','))
  function applyMdns() {
    const list = mdnsChoices.filter((i) => mdnsSel.includes(i))
    if (list.length === 1) return
    review(list.length ? 'mDNS across networks' : 'Turn off mDNS across networks', [`/ip/dns/set mdns-repeat-ifaces=${list.join(',') || '""'}`], { kind: 'set-mdns-repeat', interfaces: list })
  }
  function igmp(on: boolean) {
    review(`${on ? 'Turn on' : 'Turn off'} IGMP snooping`, [`/interface/bridge/set [find name="${bridge.name}"] igmp-snooping=${on ? 'yes' : 'no'}`], { kind: 'set-igmp-snooping', bridge: bridge.name, enabled: on })
  }

  // VLAN filtering is the one lockout risk here: a dialog with the checks and an explicit "I checked" first
  let vf = $state<{ on: boolean; ack: boolean } | null>(null)
  const inVlan = (ids: unknown, v: number) => (Array.isArray(ids) ? ids : String(ids ?? '').split(',')).some((x: string) => { const [a, b] = String(x).split('-').map(Number); return b ? a <= v && v <= b : a === v })
  const orphan = (p: any) => Number(p.pvid ?? 1) !== 1 && !bvlans.some((v) => inVlan(v['vlan-ids'], Number(p.pvid)))
  const orphans = bports.filter(orphan)

  // ---- DHCP options ----
  let opt = $state<{ name: string; code: number; type: string; value: string; network: string } | null>(null)
  const usedBy = (name: string) => nets.filter((x) => (x['dhcp-option'] ?? []).includes(name)).map((x) => x.address)
  const optBad = $derived(!!opt && (!/^[A-Za-z0-9_-]{1,32}$/.test(opt.name) || !(opt.code >= 1 && opt.code <= 254) || !opt.value.trim()
    || (opt.type === 'hex' && !/^([0-9a-fA-F]{2}){1,100}$/.test(opt.value.trim()))
    || (opt.type === 'ip' && !/^(\d{1,3}\.){3}\d{1,3}$/.test(opt.value.trim()))
    || (opt.type === 'text' && !/^[\x20-\x7e]{1,200}$/.test(opt.value.trim())) || /['"\\$]/.test(opt.value)))
  function applyOpt(o: NonNullable<typeof opt>) {
    const v = o.value.trim()
    const val = o.type === 'text' ? `"s'${v}'"` : o.type === 'ip' ? `"'${v}'"` : `0x${v}`
    review(`DHCP option ${o.name}`, [`/ip/dhcp-server/option/add name=${o.name} code=${o.code} value=${val} comment=tik:dhcpopt:${o.name}`, ...(o.network ? [`/ip/dhcp-server/network/set [find address="${o.network}"] dhcp-option=…,${o.name}`] : [])],
      { kind: 'dhcp-option', name: o.name, code: o.code, value_type: o.type, value: v, ...(o.network ? { network: o.network } : {}) })
    opt = null
  }
</script>

<Section title="Networks" desc="Each network is a subnet with its own DHCP, optionally on its own VLAN." ros="/ip/address/print; /ip/pool/print; /ip/dhcp-server/print; /ip/dhcp-server/network/print; /interface/vlan/print; /ipv6/address/print; /ipv6/dhcp-server/print">
  {#snippet actions()}<button class="btn primary" onclick={() => (open = true)}><Icon name="plus" size={14} /> New Network</button>{/snippet}
  <table>
    <thead><tr><th>Name</th><th>VLAN</th><th>Subnet</th><th>Gateway</th><th>DHCP range</th><th>Leases used</th><th>Lease</th><th>Domain</th><th>Filter</th><th>IPv6</th></tr></thead>
    <tbody>
      {#each existing as e}<tr onclick={() => (ed = { ...e, dnsText: e.filter === 'none' ? e.dns : '' })}><td><button class="plain" onclick={(ev) => { ev.stopPropagation(); ed = { ...e, dnsText: e.filter === 'none' ? e.dns : '' } }}><b>{e.name}</b></button> <span class="muted small">{e.iface}</span></td><td>{e.vlan}</td><td class="mono">{e.subnet}</td><td class="mono">{e.gateway}</td><td class="mono small">{e.dhcp}</td><td>{#if e.usage?.size}<span class:warnc={e.usage.pct >= 80} title="{e.usage.used} of {e.usage.size} addresses leased">{e.usage.used}/{e.usage.size}</span>{#if e.usage.pct >= 80} <span class="pill warn">nearly full</span>{/if}{:else}—{/if}</td><td>{e.lease || '—'}</td><td class="mono small">{e.domain || '—'}</td><td>{e.filter === 'family' ? 'Family-safe' : e.filter === 'ads' ? 'Ad-block' : '—'}</td><td class="mono small">{e.v6 || 'Off'}{e.dhcp6 ? ' · DHCPv6' : ''}</td></tr>{/each}
    </tbody>
  </table>
</Section>

<Section title="Network-wide options" ros="/ip/dns/print; /interface/bridge/print; /ipv6/settings/print; /ipv6/pool/print">
  <Field label="DNS servers"><span class="mono small">{(dns.servers ?? []).join(', ')}</span></Field>
  <Field label="mDNS across networks" help="Lets AirPlay/Chromecast work between the networks you tick (at least two)" na={mdnsSupported ? undefined : 'Needs RouterOS 7.16 or later'}>
    {#if mdnsSupported}
      <fieldset class="checks"><legend class="sr">Networks that share mDNS</legend>
        {#each mdnsChoices as i}<label class="small"><input type="checkbox" value={i} bind:group={mdnsSel} /> {existing.find((e) => e.iface === i)?.name ?? i}</label>{/each}
      </fieldset>
      <button class="btn small" disabled={!mdnsChanged || mdnsSel.length === 1} onclick={applyMdns}>Apply</button>
    {:else}<span class="pill">Off</span>{/if}
  </Field>
  <Field label="IGMP snooping" help="Sends multicast (IPTV, some speakers) only to the ports that asked for it" na={bridge.name ? undefined : 'No bridge on this router'}>
    <span class="pill">{bridge['igmp-snooping'] ? 'On' : 'Off'}</span>
    {#if bridge.name}<button class="btn small" onclick={() => igmp(!bridge['igmp-snooping'])}>{bridge['igmp-snooping'] ? 'Turn off' : 'Turn on'}</button>{/if}
  </Field>
  <Field label="VLAN filtering on bridge" help="High risk: decides what every port carries" na={bridge.name ? undefined : 'No bridge on this router'}>
    <span class="pill">{bridge['vlan-filtering'] ? 'On' : 'Off'}</span>
    {#if bridge.name}<button class="btn small danger" onclick={() => (vf = { on: !bridge['vlan-filtering'], ack: false })}>{bridge['vlan-filtering'] ? 'Turn off…' : 'Turn on…'}</button>{/if}
  </Field>
  <Field label="IPv6"><span class="pill">{v6off ? 'Disabled' : 'Enabled'}</span> <span class="muted small">{v6pools.length ? `Delegated pools: ${v6pools.map((p) => p.name).join(', ')}` : 'No delegated prefix yet (Internet → IPv6)'}</span></Field>
  <Field label="Local DNS records" help="Names the router answers for its networks"><span class="muted small">{(raw.dnsStatic ?? []).length} records · edit them under Routing → DNS</span></Field>
</Section>

<Section title="DHCP options" desc="Extra settings DHCP hands out, such as a boot server (code 66) or NTP server (code 42)." ros="/ip/dhcp-server/option/print">
  {#snippet actions()}<button class="btn" onclick={() => (opt = { name: '', code: 66, type: 'text', value: '', network: '' })}><Icon name="plus" size={14} /> Add option</button>{/snippet}
  {#if dhcpOpts.length}
    <table>
      <thead><tr><th>Name</th><th>Code</th><th>Value</th><th>Used by</th><th></th></tr></thead>
      <tbody>
        {#each dhcpOpts as o}<tr><td><b>{o.name}</b></td><td>{o.code}</td><td class="mono small">{String(o.value ?? '')}</td><td class="mono small">{usedBy(o.name).join(', ') || '—'}</td>
          <td class="r">{#if String(o.comment ?? '') === `tik:dhcpopt:${o.name}`}<button class="btn small" onclick={() => review(`Remove DHCP option ${o.name}`, [`/ip/dhcp-server/option/remove [find comment="tik:dhcpopt:${o.name}"]`], { kind: 'remove-dhcp-option', name: o.name })}>Remove</button>{:else}<span class="muted small">{o.comment ?? ''}</span>{/if}</td></tr>{/each}
      </tbody>
    </table>
  {:else}<p class="muted small">No DHCP options.</p>{/if}
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
          <Field label="Content filtering" help={FILTER_HELP}><select bind:value={n.filter}><option value="none">None</option><option value="family">Family-safe (Cloudflare for Families)</option><option value="ads" disabled={!adblock.names}>Block ads (router DNS + ad list){adblock.names ? '' : ': add an ad list first'}</option></select></Field>
          <Field label="mDNS reflector" help="Adds this network to mDNS across networks" na={mdnsSupported ? undefined : 'Needs RouterOS 7.16 or later'}><Toggle bind:checked={n.mdns} disabled={!mdnsSupported} label="mDNS" /></Field>
          <Field label="IGMP snooping" help={bridge['igmp-snooping'] ? 'Already on for the bridge' : 'Turns it on for the whole bridge'}><Toggle bind:checked={n.igmp} disabled={!!bridge['igmp-snooping']} label="IGMP" /></Field>
          <Field label="IPv6 (SLAAC)" na={v6pools.length ? undefined : 'Needs a delegated IPv6 prefix: turn on IPv6 under Internet first'} help="From the delegated prefix {v6pool}"><Toggle bind:checked={n.ipv6} disabled={!v6pools.length} label="IPv6" /></Field>
          <p class="muted small">DHCP options are added under DHCP options once the network exists.</p>
        </details>
      </div>
      <div>
        <div class="muted small"><b>{objects}</b> RouterOS objects from one form:</div>
        <pre class="cmds">{cmds.join('\n')}</pre>
      </div>
    </div>
    {#snippet footer()}{#if nProblem}<span class="small bad grow" role="alert">{nProblem}</span>{/if}<button class="btn" onclick={() => (open = false)}>Cancel</button><button class="btn primary" disabled={!!nProblem} onclick={() => { review(`Create network ${n.name}`, cmds.filter((c) => !c.startsWith('#')), intent); open = false }}>Review & apply</button>{/snippet}
  </Modal>
{/if}

{#if ed}
  <Modal title="Edit {ed.name}" onclose={() => (ed = null)}>
    {#if !ed.hasNet}<p class="muted small">This network has no DHCP network entry, so domain and DNS can't be set here.</p>{/if}
    <Field label="Domain name" help="Search domain handed out by DHCP (e.g. home.lan)"><input type="text" bind:value={ed.domain} placeholder="none" disabled={!ed.hasNet} /></Field>
    <Field label="Content filtering" help={FILTER_HELP}><select bind:value={ed.filter} disabled={!ed.hasNet}><option value="none">None</option><option value="family">Family-safe (Cloudflare for Families)</option><option value="ads" disabled={!adblock.names}>Block ads (router DNS + ad list)</option></select></Field>
    {#if ed.filter === 'none'}<Field label="DNS for clients" help="Empty = the router"><input type="text" bind:value={ed.dnsText} placeholder={ed.gateway} disabled={!ed.hasNet} /></Field>{/if}
    <Field label="DHCP range" help="First-last address handed out; current leases stay until they expire" na={ed.pool ? undefined : 'No DHCP server on this network'}><input type="text" aria-label="DHCP range" bind:value={ed.range} placeholder="10.0.30.10-10.0.30.200" disabled={!ed.pool} /></Field>
    <Field label="Lease time" help="e.g. 30m, 1d" na={ed.server ? undefined : 'No DHCP server on this network'}><input type="text" aria-label="Lease time" bind:value={ed.lease} list="lease-times" disabled={!ed.server} /></Field>
    <datalist id="lease-times"><option value="30m"></option><option value="1h"></option><option value="1d"></option><option value="7d"></option></datalist>
    <Field label="IPv6 (SLAAC)" na={v6pools.length ? undefined : 'Needs a delegated IPv6 prefix: turn on IPv6 under Internet first'} help={ed.v6 || 'Off'}>
      {#if ed.v6tik}<button class="btn small" onclick={() => { review(`Turn off IPv6 on ${ed!.iface}`, [`/ipv6/address/remove [find comment="tik:v6lan:${ed!.iface}"]`], { kind: 'ipv6-lan', interface: ed!.iface, pool: '' }); ed = null }}>Turn off</button>
      {:else if v6pools.length && !ed.v6}<select aria-label="IPv6 pool" bind:value={v6pool}>{#each v6pools as p}<option>{p.name}</option>{/each}</select><button class="btn small" onclick={() => { review(`IPv6 on ${ed!.iface}`, [`/ipv6/address/add interface=${ed!.iface} from-pool=${v6pool} advertise=yes comment=tik:v6lan:${ed!.iface}`], { kind: 'ipv6-lan', interface: ed!.iface, pool: v6pool }); ed = null }}>Turn on</button>{/if}
    </Field>
    <Field label="DHCPv6 server" help={ed.v6pool ? `Hands out addresses from ${ed.v6pool}; SLAAC keeps working` : ''} na={v6off ? 'IPv6 is disabled on this router' : ed.v6pool ? undefined : 'Needs IPv6 (SLAAC) from a delegated pool on this network first'}>
      {#if ed.dhcp6tik}<button class="btn small" onclick={() => { review(`Remove DHCPv6 server on ${ed!.iface}`, [`/ipv6/dhcp-server/remove [find comment="tik:dhcpv6:${ed!.iface}"]`], { kind: 'remove-dhcpv6-server', interface: ed!.iface }); ed = null }}>Turn off</button>
      {:else if ed.dhcp6}<span class="muted small">On (set up outside tikontrol)</span>
      {:else if ed.v6pool && !v6off}<button class="btn small" onclick={() => { review(`DHCPv6 server on ${ed!.iface}`, [`/ipv6/dhcp-server/add name=tik-v6-${ed!.iface} interface=${ed!.iface} address-pool=${ed!.v6pool} comment=tik:dhcpv6:${ed!.iface}`], { kind: 'dhcpv6-server', interface: ed!.iface }); ed = null }}>Turn on</button>{/if}
    </Field>
    {#if ed.tikName}
      <Field label="Remove network" help="Removes everything tikontrol created for it; devices on it go offline"><button class="btn small danger" onclick={() => { review(`Remove network ${ed!.tikName}`, NET_PATHS.map((p) => `${p}/remove [find comment="tik:net:${ed!.tikName}"]`), { kind: 'remove-network', name: ed!.tikName }); ed = null }}>Remove…</button></Field>
    {/if}
    {#snippet footer()}<button class="btn" onclick={() => (ed = null)}>Cancel</button><button class="btn primary" disabled={!edParts.length || edBad} onclick={() => applyEdit(ed!)}>Review & apply</button>{/snippet}
  </Modal>
{/if}

{#if vf}
  <Modal title="{vf.on ? 'Turn on' : 'Turn off'} VLAN filtering" onclose={() => (vf = null)} wide>
    <div class="warnbox small" role="alert"><Icon name="alert" size={14} /> High risk: VLAN filtering changes what every port on {bridge.name} carries. A port whose PVID or tagged VLANs are wrong stops passing traffic, and that can cut off this browser and tikontrol. If that happens, do nothing: the router turns it back {vf.on ? 'off' : 'on'} by itself after 5 minutes.</div>
    {#if vf.on}
      <p class="small">Check before you continue: the port you manage the router from must be untagged in its PVID, and the bridge itself must be tagged in every VLAN the router serves.</p>
      <table>
        <thead><tr><th>Port</th><th>PVID</th><th>Frame types</th><th>Check</th></tr></thead>
        <tbody>{#each bports as p}<tr><td>{p.interface}</td><td>{p.pvid ?? 1}</td><td class="small">{p['frame-types'] ?? '—'}</td><td class="small">{#if orphan(p)}<span class="bad">No bridge VLAN entry for PVID {p.pvid}: untagged traffic will be dropped</span>{:else}OK{/if}</td></tr>{/each}</tbody>
      </table>
      <p class="muted small">{bvlans.length} bridge VLAN entries. Change port VLANs under Ports first.</p>
      {#if orphans.length}<p class="bad small" role="alert">{orphans.length} port(s) above would lose their untagged traffic.</p>{/if}
    {:else}
      <p class="small">With filtering off, the bridge forwards every VLAN on every port: networks that rely on VLANs for separation are no longer kept apart by the switch.</p>
    {/if}
    <label class="small ack"><input type="checkbox" bind:checked={vf.ack} /> I have checked the ports and understand I may be cut off for up to 5 minutes.</label>
    {#snippet footer()}<button class="btn" onclick={() => (vf = null)}>Cancel</button><button class="btn danger" disabled={!vf?.ack} onclick={() => { const on = vf!.on; review(`${on ? 'Turn on' : 'Turn off'} VLAN filtering on ${bridge.name}`, [`/interface/bridge/set [find name="${bridge.name}"] vlan-filtering=${on ? 'yes' : 'no'}`], { kind: 'set-vlan-filtering', bridge: bridge.name, enabled: on }); vf = null }}>Review & apply</button>{/snippet}
  </Modal>
{/if}

{#if opt}
  <Modal title="Add DHCP option" onclose={() => (opt = null)}>
    <Field label="Name" help="Letters, digits, - and _"><input type="text" bind:value={opt.name} placeholder="tftp" /></Field>
    <Field label="Code" help="1-254, e.g. 66 boot server, 42 NTP"><input type="number" bind:value={opt.code} min="1" max="254" /></Field>
    <Field label="Value type"><select bind:value={opt.type}><option value="text">Text</option><option value="ip">IPv4 address</option><option value="hex">Hex bytes</option></select></Field>
    <Field label="Value" help={opt.type === 'hex' ? 'Even number of hex digits, no 0x' : 'No quotes, backslashes or $'}><input type="text" bind:value={opt.value} /></Field>
    <Field label="Attach to network" help="Optional: hand it out on this network"><select bind:value={opt.network}><option value="">None</option>{#each nets as x}<option value={x.address}>{x.address}</option>{/each}</select></Field>
    {#snippet footer()}<button class="btn" onclick={() => (opt = null)}>Cancel</button><button class="btn primary" disabled={optBad} onclick={() => applyOpt(opt!)}>Review & apply</button>{/snippet}
  </Modal>
{/if}

<style>
  .warnc { color: var(--warn); }
  .bad { color: var(--bad); }
  .grow { flex: 1; }
  .cols { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 18px; }
  .cmds { background: var(--code-bg); border: 1px solid var(--line); border-radius: 8px; padding: 10px; font: 11px/1.6 var(--mono); white-space: pre-wrap; margin-top: 8px; max-height: 60vh; overflow: auto; }
  details { margin-top: 6px; }
  summary { cursor: pointer; color: var(--muted); padding: 6px 0; }
  .small { font-size: 12px; }
  .btn.small { padding: 3px 9px; }
  .checks { border: 0; margin: 0; padding: 0; display: flex; flex-wrap: wrap; gap: 4px 12px; justify-content: flex-end; }
  .checks label, .ack { display: inline-flex; gap: 6px; align-items: center; }
  .ack { margin-top: 10px; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
  .warnbox { display: flex; gap: 8px; align-items: flex-start; padding: 8px 10px; border-radius: 8px; background: color-mix(in srgb, var(--warn) 14%, transparent); color: var(--warn); margin: 0 0 8px; }
  .bad { color: var(--bad); }
  .r { text-align: right; }
  @media (max-width: 800px) { .cols { grid-template-columns: minmax(0, 1fr); } }
</style>
