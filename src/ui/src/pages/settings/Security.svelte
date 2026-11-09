<script module lang="ts">
  // set by the client drawer's "Traffic rule" action, read once when this page opens
  export const trafficDraft = $state({ mac: '' })
</script>

<script lang="ts">
  import { raw, insights, adblock, clients } from '../../lib/data'
  import { review } from '../../lib/apply.svelte'
  import { prefs, displayName } from '../../lib/live.svelte'
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
  // one intent per country (a batch holds at most 8); the server checks the codes again
  const geoIntents = () => [...new Set(geo.countries.split(',').map((c) => c.trim().toUpperCase()).filter((c) => /^[A-Z]{2}$/.test(c)))].slice(0, 8)
    .map((c) => ({ kind: 'region-block', country: c, url: `https://www.ipdeny.com/ipblocks/data/aggregated/${c.toLowerCase()}-aggregated.zone` }))
  let group = $state({ name: '', items: '' })
  let apps = $state<Record<string, boolean>>({ 'TikTok': false, 'Facebook': false, 'Steam': false, 'Netflix': false })
  const APPDOM: Record<string, string> = { TikTok: 'tiktok.com', Facebook: 'facebook.com', Steam: 'steampowered.com', Netflix: 'netflix.com' }
  const audit = insights.filter((i) => i.level !== 'ok')

  // Traffic rules: everything tagged tik:rule:<name> (filter rules and simple queues), grouped by name
  const DAYS = ['sun', 'mon', 'tue', 'wed', 'thu', 'fri', 'sat']
  const APPSETS: Record<string, string[]> = {
    YouTube: ['youtube.com', 'googlevideo.com', 'ytimg.com', 'youtu.be'], TikTok: ['tiktok.com', 'tiktokcdn.com', 'tiktokv.com'],
    Instagram: ['instagram.com', 'cdninstagram.com'], Facebook: ['facebook.com', 'fbcdn.net'], Netflix: ['netflix.com', 'nflxvideo.net'],
    Roblox: ['roblox.com', 'rbxcdn.com'], Steam: ['steampowered.com', 'steamcommunity.com'],
  }
  const gslug = (n: string) => n.trim().replace(/[^A-Za-z0-9_-]+/g, '-').slice(0, 32)
  // RouterOS prints `22h-23h59m59s,mon` or `22:00:00-...`: back to HH:MM
  const hhmm = (t: string) => { if (t.includes(':')) return t.slice(0, 5).padStart(5, '0'); const h = +(t.match(/(\d+)h/)?.[1] ?? 0), m = +(t.match(/(\d+)m/)?.[1] ?? 0); return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}` }
  type TR = { name: string; target: string; value: string; action: string; domains: string; down: string; up: string; days: string[]; from: string; to: string }
  const blank = (): TR => ({ name: '', target: 'mac', value: '', action: 'block', domains: '', down: '', up: '', days: [], from: '', to: '' })
  const trules = $derived.by(() => {
    const tagged = [...(raw.filter ?? []), ...(raw.queues ?? [])].filter((r: any) => typeof r.comment === 'string' && r.comment.startsWith('tik:rule:'))
    const by = new Map<string, any[]>()
    for (const r of tagged) by.set(r.comment.slice(9), [...(by.get(r.comment.slice(9)) ?? []), r])
    return [...by].map(([name, rows]): TR => {
      const r0 = rows[0], q = rows.find((r) => 'max-limit' in r)
      const times = rows.map((r) => r.time).filter(Boolean) as string[]
      const [w0, ...d0] = (times[0] ?? '').split(','), wl = (times.at(-1) ?? '').split(',')[0]
      const tgt: string[] = Array.isArray(q?.target) ? q.target : q?.target ? [q.target] : []
      const byIp = tgt.length === 1 ? clients.find((c) => `${c.ip}/32` === tgt[0]) : undefined
      const [target, value] = r0['src-mac-address'] ? ['mac', r0['src-mac-address']] : r0['src-address-list'] ? ['group', String(r0['src-address-list']).replace(/^tik-/, '')]
        : r0['src-address'] ? ['network', r0['src-address']] : byIp ? ['mac', byIp.mac] : tgt.length > 1 ? ['group', ''] : ['network', tgt[0] ?? '']
      const [up, down] = String(q?.['max-limit'] ?? '').split('/')
      const all = d0.length === 7 || !times.length
      const f = times.length ? hhmm(w0.split('-')[0]) : '', t = times.length ? hhmm(wl.split('-')[1] ?? '') : ''
      const allDay = !times.length || (times.length === 1 && f === '00:00' && t === '23:59')
      return {
        name, target, value, action: q ? 'limit' : rows.some((r) => r['tls-host']) ? 'domains' : 'block',
        domains: rows.map((r) => r['tls-host']).filter((h) => h && !h.startsWith('*.')).join(', '), down: down ?? '', up: up ?? '',
        days: all ? [] : d0, from: allDay ? '' : f, to: allDay ? '' : t,
      }
    })
  })
  const who = (t: TR) => t.target === 'mac' ? (clients.find((c) => c.mac === t.value) ? displayName(clients.find((c) => c.mac === t.value)!) : t.value) : t.target === 'group' ? `group ${t.value || '?'}` : t.value
  const whatDo = (t: TR) => t.action === 'limit' ? `limit ${t.down}↓ ${t.up}↑` : t.action === 'domains' ? `block ${t.domains}` : 'block internet'
  const whenStr = (t: TR) => `${t.days.length ? t.days.join(' ') : 'every day'}${t.from ? ` ${t.from}–${t.to}` : ''}`
  let tr = $state<TR>({ ...blank(), value: trafficDraft.mac })
  if (trafficDraft.mac) { trafficDraft.mac = ''; queueMicrotask(() => document.getElementById('traffic-rules')?.scrollIntoView()) }
  const trIntent = (t: TR) => ({
    kind: 'traffic-rule', name: t.name.trim(), target: t.target, value: t.value.trim(), action: t.action,
    domains: t.action === 'domains' ? t.domains.split(/[\s,]+/).filter(Boolean) : [], down: t.action === 'limit' ? t.down.trim() : '', up: t.action === 'limit' ? t.up.trim() : '',
    days: t.days, from: t.from, to: t.to,
  })
  const addApp = (a: string) => (tr.domains = [...new Set([...tr.domains.split(/[\s,]+/).filter(Boolean), ...APPSETS[a]])].join(', '))
  const saveRule = () => review(`Traffic rule ${tr.name}`, [`# traffic rule ${tr.name}: ${whatDo(tr)} for ${who(tr)}, ${whenStr(tr)} (the tikontrol server compiles the commands)`], trIntent(tr))
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
    <button class="btn primary" disabled={!rule.name} onclick={() => review(`Firewall rule ${rule.name}`, [`/ip/firewall/filter/add chain=forward action=${rule.action} protocol=${rule.proto}${rule.src ? ` src-address=${rule.src}` : ''}${rule.dst ? ` dst-address=${rule.dst}` : ''}${rule.port ? ` dst-port=${rule.port}` : ''} comment="tik:fw:${rule.name}"`], { kind: 'firewall-rule', name: rule.name, action: rule.action, protocol: rule.proto, src: rule.src, dst: rule.dst, dst_port: rule.proto === 'icmp' ? '' : rule.port })}><Icon name="plus" size={14} /> Add</button>
  </div>
</Section>

<Section id="traffic-rules" title="Traffic rules" desc="Block internet, block apps and domains, or limit speed for a client, a client group or a network, optionally on a schedule." ros="/ip/firewall/filter/print where comment~&quot;tik:rule:&quot;; /queue/simple/print where comment~&quot;tik:rule:&quot;">
  <table>
    <thead><tr><th>Name</th><th>Applies to</th><th>Action</th><th>When</th><th><span class="sr">Actions</span></th></tr></thead>
    <tbody>{#each trules as t}<tr class="nohover"><td><b>{t.name}</b></td><td>{who(t)}</td><td class="small">{whatDo(t)}</td><td class="small">{whenStr(t)}</td>
      <td class="r0"><button class="btn ghost" aria-label="Edit {t.name}" onclick={() => (tr = { ...t, days: [...t.days] })}><Icon name="edit" size={14} /></button>
        <button class="btn ghost" aria-label="Delete {t.name}" onclick={() => review(`Remove traffic rule ${t.name}`, [`/ip/firewall/filter/remove [find comment="tik:rule:${t.name}"]`, `/queue/simple/remove [find comment="tik:rule:${t.name}"]`], { kind: 'remove-traffic-rule', name: t.name })}><Icon name="trash" size={14} /></button></td></tr>
    {:else}<tr class="nohover"><td colspan="5" class="muted small">No traffic rules yet.</td></tr>{/each}</tbody>
  </table>
  <Field label="Name" help="Saving an existing name replaces that rule"><input type="text" bind:value={tr.name} placeholder="e.g. bedtime" maxlength="32" /></Field>
  <Field label="Applies to">
    <select bind:value={tr.target} aria-label="Target type" onchange={() => (tr.value = '')}><option value="mac">Client</option><option value="group">Client group</option><option value="network">Network</option></select>
    {#if tr.target === 'mac'}<select bind:value={tr.value} aria-label="Client"><option value="">Client…</option>{#if tr.value && !clients.some((c) => c.mac === tr.value)}<option value={tr.value}>{tr.value}</option>{/if}{#each clients as c}<option value={c.mac}>{displayName(c)} · {c.mac}</option>{/each}</select>
    {:else if tr.target === 'group'}<select bind:value={tr.value} aria-label="Client group"><option value="">Group…</option>{#each prefs.groups as g}<option value={gslug(g.name)}>{g.name}</option>{/each}</select>
    {:else}<input type="text" bind:value={tr.value} placeholder="e.g. 10.0.20.0/24" aria-label="Network" />{/if}
  </Field>
  <Field label="Action">
    <select bind:value={tr.action} aria-label="Action"><option value="block">Block internet</option><option value="domains">Block apps / domains</option><option value="limit">Limit speed</option></select>
    {#if tr.action === 'limit'}<input type="text" bind:value={tr.down} placeholder="Down, e.g. 20M" aria-label="Download limit" style="width:110px" /><input type="text" bind:value={tr.up} placeholder="Up (= down)" aria-label="Upload limit" style="width:110px" />{/if}
  </Field>
  {#if tr.action === 'domains'}
    <Field label="Domains" help="Comma separated; subdomains are included">
      <div class="apps">{#each Object.keys(APPSETS) as a}<button class="btn" onclick={() => addApp(a)}><Icon name="plus" size={12} /> {a}</button>{/each}</div>
      <input type="text" bind:value={tr.domains} placeholder="example.com, example.net" aria-label="Domains" />
    </Field>
  {/if}
  <Field label="Schedule" help="No days = every day; no times = all day. Uses the router's clock.">
    <div class="days" role="group" aria-label="Days">{#each DAYS as d}<button class="day" aria-pressed={tr.days.includes(d)} onclick={() => (tr.days = tr.days.includes(d) ? tr.days.filter((x) => x !== d) : [...tr.days, d])}>{d}</button>{/each}</div>
    <input type="time" bind:value={tr.from} aria-label="From" /> – <input type="time" bind:value={tr.to} aria-label="To" />
  </Field>
  <div class="r"><button class="btn ghost" onclick={() => (tr = blank())}>Clear</button>
    <button class="btn primary" disabled={!tr.name.trim() || !tr.value.trim() || (tr.action === 'limit' && !tr.down.trim()) || (tr.action === 'domains' && !tr.domains.trim()) || !tr.from !== !tr.to} onclick={saveRule}>Review & apply</button></div>
</Section>

<Section title="Block apps" desc="Domain-based blocking (DNS + TLS SNI). True per-app detection needs CMR.">
  <div class="apps">{#each Object.keys(apps) as a}<label class="row"><Toggle bind:checked={apps[a]} label={a} /> {a}</label>{/each}</div>
  <div class="r"><button class="btn primary" onclick={() => { const on = Object.entries(apps).filter(([, v]) => v).map(([a]) => APPDOM[a]); review('App blocking', on.flatMap((d) => [`/ip/dns/static/add name=${d} type=NXDOMAIN match-subdomain=yes comment=tik:app:${d}`, `/ip/firewall/filter/add chain=forward protocol=tcp dst-port=443 tls-host=*.${d} action=reject reject-with=tcp-reset comment=tik:app:${d}`]), on.length === 1 ? { kind: 'block-domain', domain: on[0], tls: true } : undefined) }}>Review & apply</button></div>
</Section>

<Section title="Ad blocking" desc="RouterOS DNS adlist." ros="/ip/dns/adlist/print">
  <Field label="Blocked names"><span>{adblock.names.toLocaleString()} · {adblock.hits.toLocaleString()} blocked lookups</span></Field>
</Section>

<Section title="Region blocking" desc="Drop traffic from countries. Uses address lists filled from a GeoIP feed.">
  <Field label="Countries" help="ISO codes, comma separated"><input type="text" bind:value={geo.countries} placeholder="e.g. KP, IR" /></Field>
  <div class="r"><button class="btn primary" disabled={!geoIntents().length} onclick={() => { const parts = geoIntents(); review('Region blocking', parts.map((i) => `# block ${i.country} from ${i.url}`), parts.length === 1 ? parts[0] : { kind: 'batch', title: 'Region blocking', intents: parts }) }}>Review & apply</button></div>
</Section>

<Section title="IP & port groups" desc="Reusable lists for rules." ros="/ip/firewall/address-list/print">
  {#if !lists.length}<div class="muted small">No address lists yet.</div>{/if}
  {#each lists as l}<div class="mono small">{l.list}: {l.address}</div>{/each}
  <div class="add"><input type="text" bind:value={group.name} placeholder="Group name" /><input type="text" bind:value={group.items} placeholder="IPs, comma separated" style="flex:1" />
    <button class="btn primary" disabled={!group.name} onclick={() => review(`Group ${group.name}`, group.items.split(',').map((ip) => `/ip/firewall/address-list/add list=${group.name} address=${ip.trim()} comment=tik:group:${group.name}`), { kind: 'address-group', name: group.name, addresses: group.items.split(',').map((ip) => ip.trim()).filter(Boolean) })}><Icon name="plus" size={14} /> Add</button></div>
</Section>

<Section title="Threat management">
  <Field label="IDS / IPS" na="Not available in RouterOS (no Suricata on this hardware)"><span></span></Field>
  <Field label="Honeypot" na="Not available in RouterOS"><span></span></Field>
  <Field label="Threat feeds" help="Drop known-bad IPs (Spamhaus DROP)"><button class="btn" onclick={() => review('Threat feed', ['# Spamhaus DROP list, refreshed daily'], { kind: 'threat-feed', name: 'spamhaus-drop', url: 'https://www.spamhaus.org/drop/drop.txt' })}>Enable</button></Field>
</Section>

<style>
  .audit { gap: 8px; padding: 6px 0; color: var(--warn); flex-wrap: wrap; }
  .audit b { color: var(--text); }
  .add { display: flex; gap: 6px; flex-wrap: wrap; margin-top: 10px; }
  .apps { display: flex; gap: 18px; flex-wrap: wrap; }
  .off { opacity: .45; }
  .days { display: flex; gap: 2px; }
  .day { padding: 4px 7px; border: 1px solid var(--line); background: var(--panel2); border-radius: 6px; cursor: pointer; font-size: 12px; color: var(--muted); }
  .day[aria-pressed='true'] { border-color: var(--accent); color: var(--accent); }
  .r0 { text-align: right; white-space: nowrap; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
  .r { text-align: right; margin-top: 10px; }
  .small { font-size: 12px; }
</style>
