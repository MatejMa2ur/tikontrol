<script lang="ts">
  import { raw, insights, adblock } from '../../lib/data'
  import { review } from '../../lib/apply.svelte'
  import Icon from '../../lib/Icon.svelte'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'
  import Toggle from '../../lib/ui/Toggle.svelte'

  const rules: any[] = raw.filter ?? []
  const members: any[] = raw.listMembers ?? []
  const lists: any[] = raw.addressLists ?? []
  const zones = [...new Set(['LAN', 'WAN', ...members.map((m) => m.list)])]
  // zone matrix from the defconf rules: LAN→anything allowed, WAN→LAN only established/dst-nat
  const allow = (from: string, to: string) => (from === to ? 'Allow' : from === 'WAN' ? (to === 'Router' ? 'Block' : 'Return only') : to === 'Router' && from !== 'LAN' ? 'Block' : 'Allow')

  const desc = (r: any) => [r.protocol, r['dst-port'] && `port ${r['dst-port']}`, r['connection-state'], r['in-interface-list'] && `from ${r['in-interface-list']}`].filter(Boolean).join(' · ') || 'any'
  let rule = $state({ name: '', action: 'drop', src: '', dst: '', port: '', proto: 'tcp' })
  let geo = $state({ countries: '' })
  let group = $state({ name: '', items: '' })
  let apps = $state<Record<string, boolean>>({ 'TikTok': false, 'Facebook': false, 'Steam': false, 'Netflix': false })
  const APPDOM: Record<string, string> = { TikTok: 'tiktok.com', Facebook: 'facebook.com', Steam: 'steampowered.com', Netflix: 'netflix.com' }
  const audit = insights.filter((i) => i.level !== 'ok')
</script>

<Section title="Security audit" desc="Things on this router worth fixing." ros="/ip/service/print">
  {#each audit as a}<div class="row audit"><Icon name="alert" size={14} /><b>{a.title}</b><span class="muted small">{a.detail}</span></div>{:else}<div class="muted small">No issues found.</div>{/each}
</Section>

<Section title="Zones" desc="Interface lists act as zones. Default policy between zones:" ros="/interface/list/member/print">
  <table class="matrix"><thead><tr><th>From \ To</th>{#each [...zones, 'Router'] as z}<th>{z}</th>{/each}</tr></thead>
    <tbody>{#each zones as f}<tr class="nohover"><td><b>{f}</b></td>{#each [...zones, 'Router'] as t}{@const v = allow(f, t)}<td><span class="pill" class:ok={v === 'Allow'} class:bad={v === 'Block'}>{v}</span></td>{/each}</tr>{/each}</tbody></table>
</Section>

<Section title="Firewall rules" desc="{rules.length} rules, in order. tikontrol adds its own rules tagged tik:." ros="/ip/firewall/filter/print">
  <table>
    <thead><tr><th>#</th><th>Chain</th><th>Action</th><th>Match</th><th>Comment</th></tr></thead>
    <tbody>{#each rules as r, i}<tr class="nohover" class:off={r.disabled}><td>{i}</td><td>{r.chain}</td><td><span class="pill" class:bad={r.action === 'drop' || r.action === 'reject'} class:ok={r.action === 'accept'}>{r.action}</span></td><td class="small">{desc(r)}</td><td class="muted small">{r.comment ?? ''}</td></tr>{/each}</tbody>
  </table>
  <div class="add">
    <input type="text" bind:value={rule.name} placeholder="Rule name" />
    <select bind:value={rule.action}><option>drop</option><option>accept</option><option>reject</option></select>
    <input type="text" bind:value={rule.src} placeholder="Source IP / group" />
    <input type="text" bind:value={rule.dst} placeholder="Destination" />
    <select bind:value={rule.proto}><option>tcp</option><option>udp</option><option>icmp</option></select>
    <input type="text" bind:value={rule.port} placeholder="Port" style="width:80px" />
    <button class="btn primary" disabled={!rule.name} onclick={() => review(`Firewall rule ${rule.name}`, [`/ip/firewall/filter/add chain=forward action=${rule.action} protocol=${rule.proto}${rule.src ? ` src-address=${rule.src}` : ''}${rule.dst ? ` dst-address=${rule.dst}` : ''}${rule.port ? ` dst-port=${rule.port}` : ''} comment="tik:${rule.name}" place-before=[find comment~"drop all from WAN"]`])}><Icon name="plus" size={14} /> Add</button>
  </div>
</Section>

<Section title="Block apps" desc="Domain-based blocking (DNS + TLS SNI). True per-app detection needs CMR.">
  <div class="apps">{#each Object.keys(apps) as a}<label class="row"><Toggle bind:checked={apps[a]} label={a} /> {a}</label>{/each}</div>
  <div class="r"><button class="btn primary" onclick={() => review('App blocking', Object.entries(apps).filter(([, v]) => v).flatMap(([a]) => [`/ip/dns/static/add name=${APPDOM[a]} type=NXDOMAIN match-subdomain=yes comment="tik:app ${a}"`, `/ip/firewall/filter/add chain=forward tls-host=*${APPDOM[a]} action=reject comment="tik:app ${a}"`]))}>Review & apply</button></div>
</Section>

<Section title="Ad blocking" desc="RouterOS DNS adlist." ros="/ip/dns/adlist/print">
  <Field label="Blocked names"><span>{adblock.names.toLocaleString()} · {adblock.hits.toLocaleString()} blocked lookups</span></Field>
</Section>

<Section title="Region blocking" desc="Drop traffic from countries. Uses address lists filled from a GeoIP feed.">
  <Field label="Countries" help="ISO codes, comma separated"><input type="text" bind:value={geo.countries} placeholder="e.g. KP, IR" /></Field>
  <div class="r"><button class="btn primary" disabled={!geo.countries} onclick={() => review('Region blocking', geo.countries.split(',').map((c) => c.trim().toUpperCase()).flatMap((c) => [`/tool/fetch url="https://www.ipdeny.com/ipblocks/data/aggregated/${c.toLowerCase()}-aggregated.zone" dst-path=tik-geo-${c}.txt`, `/ip/firewall/raw/add chain=prerouting in-interface-list=WAN src-address-list=tik-geo-${c} action=drop comment="tik:geo ${c}"`]))}>Review & apply</button></div>
</Section>

<Section title="IP & port groups" desc="Reusable lists for rules." ros="/ip/firewall/address-list/print">
  {#if !lists.length}<div class="muted small">No address lists yet.</div>{/if}
  {#each lists as l}<div class="mono small">{l.list}: {l.address}</div>{/each}
  <div class="add"><input type="text" bind:value={group.name} placeholder="Group name" /><input type="text" bind:value={group.items} placeholder="IPs, comma separated" style="flex:1" />
    <button class="btn primary" disabled={!group.name} onclick={() => review(`Group ${group.name}`, group.items.split(',').map((ip) => `/ip/firewall/address-list/add list=${group.name} address=${ip.trim()}`))}><Icon name="plus" size={14} /> Add</button></div>
</Section>

<Section title="Threat management">
  <Field label="IDS / IPS" na="Not available in RouterOS (no Suricata on this hardware)"><span></span></Field>
  <Field label="Honeypot" na="Not available in RouterOS"><span></span></Field>
  <Field label="Threat feeds" help="Drop known-bad IPs (Spamhaus DROP)"><button class="btn" onclick={() => review('Threat feed', ['/system/script/add name=tik-feeds source=":do {/tool/fetch url=https://www.spamhaus.org/drop/drop.txt dst-path=drop.txt} on-error={}"', '/system/scheduler/add name=tik-feeds interval=1d on-event=tik-feeds', '/ip/firewall/raw/add chain=prerouting src-address-list=tik-drop action=drop comment="tik:threat feed"'])}>Enable</button></Field>
</Section>

<style>
  .audit { gap: 8px; padding: 6px 0; color: var(--warn); flex-wrap: wrap; }
  .audit b { color: var(--text); }
  .add { display: flex; gap: 6px; flex-wrap: wrap; margin-top: 10px; }
  .apps { display: flex; gap: 18px; flex-wrap: wrap; }
  .off { opacity: .45; }
  .r { text-align: right; margin-top: 10px; }
  .small { font-size: 12px; }
</style>
