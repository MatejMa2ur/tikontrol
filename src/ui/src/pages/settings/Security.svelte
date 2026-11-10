<script module lang="ts">
  // set by the client drawer's "Traffic rule" action, read once when this page opens
  export const trafficDraft = $state({ mac: '' })
</script>

<script lang="ts">
  import { raw, adblock, clients } from '../../lib/data'
  import { review, type Intent } from '../../lib/apply.svelte'
  import { prefs, displayName } from '../../lib/live.svelte'
  import { securityAudit, zoneCell, zonesOf } from '../../lib/security'
  import Icon from '../../lib/Icon.svelte'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'

  // Data is read once per mount; the page re-mounts after every apply (App.svelte), so it stays current.
  const rules: any[] = raw.filter ?? []
  const lists: any[] = (raw.addressLists ?? []).filter((l: any) => !l.dynamic)
  const sched: any[] = raw.scheduler ?? []
  /** the part after a tikontrol tag prefix, '' when the row is not ours */
  const tagOf = (r: any, p: string) => (typeof r?.comment === 'string' && r.comment.startsWith(p) ? r.comment.slice(p.length) : '')
  const count = (list: string) => lists.filter((l) => l.list === list).length
  const BATCH = 8 // most intents one batch holds (src/server/src/apply.rs)
  /** one intent, or a batch of up to 8 */
  const send = (title: string, cmds: string[], parts: Intent[]) => review(title, cmds, parts.length === 1 ? parts[0] : { kind: 'batch', title, intents: parts })

  // ---------- audit ----------
  const audit = securityAudit(raw)

  // ---------- zones: from the real rules (src/ui/src/lib/security.ts) ----------
  const zones = zonesOf(raw.listMembers ?? [])
  const fallback = (from: string, to: string) => zoneCell(rules.filter((r) => r.comment !== `tik:zone:${from}:${to}`), from, to).effect
  function setZone(from: string, to: string, e: Event) {
    const sel = e.currentTarget as HTMLSelectElement, policy = sel.value
    sel.value = zoneCell(rules, from, to).policy // the page re-mounts with the new state after apply
    const c = `tik:zone:${from}:${to}`
    review(`Zone ${from} → ${to}: ${policy}`, [`/ip/firewall/filter/remove [find comment="${c}"]`, ...(policy === 'default' ? [] : [`/ip/firewall/filter/add chain=forward in-interface-list=${from} out-interface-list=${to}${policy === 'block' ? ' connection-state=new action=drop' : ' action=accept'} comment=${c} place-before=0`])],
      { kind: 'zone-policy', from, to, policy })
  }

  // ---------- firewall rules ----------
  const desc = (r: any) => [r.protocol, r['src-address'] ?? r['src-address-list'] ?? (r['in-interface-list'] && `from ${r['in-interface-list']}`), (r['dst-address'] ?? r['dst-address-list'] ?? r['out-interface-list']) && `to ${r['dst-address'] ?? r['dst-address-list'] ?? r['out-interface-list']}`,
    r['dst-port'] && `port ${r['dst-port']}`, r['connection-state'], r['tls-host'] && `TLS ${r['tls-host']}`].filter(Boolean).join(' · ') || 'any'
  // an address, a/n or a-b goes as an address; anything else names an IP group
  const isAddr = (s: string) => /^[\d./-]+$/.test(s.trim())
  const where = (k: 'src' | 'dst', v: string) => (!v.trim() ? {} : isAddr(v) ? { [k]: v.trim() } : { [`${k}_list`]: v.trim() })
  let rule = $state({ name: '', action: 'drop', src: '', dst: '', port: '', proto: 'tcp' })
  const ruleIntent = () => ({ kind: 'firewall-rule', name: rule.name.trim(), action: rule.action, protocol: rule.proto, ...where('src', rule.src), ...where('dst', rule.dst), dst_port: rule.proto === 'icmp' ? '' : rule.port.trim() })
  let inp = $state({ name: '', action: 'accept', src: '', port: '', proto: 'tcp' })
  const inpIntent = () => ({ kind: 'input-rule', name: inp.name.trim(), action: inp.action, protocol: inp.proto, ...where('src', inp.src), dst_port: inp.proto === 'icmp' ? '' : inp.port.trim() })
  const removeRule = (r: any) => {
    const fw = tagOf(r, 'tik:fw:'), name = fw || tagOf(r, 'tik:input:')
    review(fw ? `Remove firewall rule ${name}` : `Remove router access rule ${name}`, [`/ip/firewall/filter/remove [find comment="${r.comment}"]`], { kind: fw ? 'remove-firewall-rule' : 'remove-input-rule', name })
  }

  // ---------- block apps: tik:app:<domain> DNS entries (src/server/src/intents/security.rs, block-domain) ----------
  const APPSETS: Record<string, string[]> = {
    YouTube: ['youtube.com', 'googlevideo.com', 'ytimg.com', 'youtu.be'], TikTok: ['tiktok.com', 'tiktokcdn.com', 'tiktokv.com'],
    Instagram: ['instagram.com', 'cdninstagram.com'], Facebook: ['facebook.com', 'fbcdn.net'], Netflix: ['netflix.com', 'nflxvideo.net'],
    Roblox: ['roblox.com', 'rbxcdn.com'], Steam: ['steampowered.com', 'steamcommunity.com'],
  }
  const blockedDomains = [...new Set((raw.dnsStatic ?? []).map((r: any) => tagOf(r, 'tik:app:')).filter(Boolean))] as string[]
  const isBlocked = (d: string) => blockedDomains.includes(d)
  const appState = (a: string) => (APPSETS[a].every(isBlocked) ? 'on' : APPSETS[a].some(isBlocked) ? 'partly' : 'off')
  let apps = $state<Record<string, boolean>>(Object.fromEntries(Object.keys(APPSETS).map((a) => [a, appState(a) === 'on'])))
  let customDomain = $state('')
  // the change from what the router has now: block what's missing for apps turned on, unblock apps turned off
  const appIntents = (): Intent[] => {
    const block = new Set<string>(), unblock = new Set<string>()
    for (const a of Object.keys(APPSETS)) {
      if (apps[a]) APPSETS[a].filter((d) => !isBlocked(d)).forEach((d) => block.add(d))
      else if (appState(a) === 'on') APPSETS[a].forEach((d) => unblock.add(d))
    }
    const d = customDomain.trim().toLowerCase()
    if (d && !isBlocked(d)) block.add(d)
    return [...[...block].map((domain) => ({ kind: 'block-domain', domain, tls: true })), ...[...unblock].map((domain) => ({ kind: 'unblock-domain', domain }))]
  }
  const unblock = (d: string) => review(`Unblock ${d}`, [`/ip/firewall/filter/remove [find comment="tik:app:${d}"]`, `/ip/dns/static/remove [find comment="tik:app:${d}"]`], { kind: 'unblock-domain', domain: d })

  // ---------- ad blocking / content filtering ----------
  const adlists: any[] = raw.adlist ?? []
  let adUrl = $state('')

  // ---------- region blocking, threat feeds, IP groups (all tagged tik:) ----------
  let geo = $state({ countries: '' })
  // one intent per country (a batch holds at most 8); the server checks the codes again
  const geoIntents = () => [...new Set(geo.countries.split(',').map((c) => c.trim().toUpperCase()).filter((c) => /^[A-Z]{2}$/.test(c)))].slice(0, BATCH)
    .map((c) => ({ kind: 'region-block', country: c, url: `https://www.ipdeny.com/ipblocks/data/aggregated/${c.toLowerCase()}-aggregated.zone` }))
  const regions = sched.map((s) => tagOf(s, 'tik:geo:')).filter(Boolean)
  const feeds = sched.map((s) => tagOf(s, 'tik:threat:')).filter(Boolean)
  const SPAMHAUS = 'spamhaus-drop'
  const groups = [...new Set(lists.map((l) => tagOf(l, 'tik:group:')).filter(Boolean))]
  // lists tikontrol doesn't manage (yours, or other tools'): shown, not changed
  const others = [...new Set(lists.filter((l) => !String(l.comment ?? '').startsWith('tik:') && !String(l.list).startsWith('tik-')).map((l) => l.list as string))]
  let group = $state({ name: '', items: '' })

  // ---------- device isolation: tik:isolate:<MAC> bridge filters ----------
  const isolated = [...new Set((raw.bridgeFilter ?? []).map((r: any) => tagOf(r, 'tik:isolate:')).filter(Boolean))] as string[]
  const nameOf = (mac: string) => { const c = clients.find((x) => x.mac === mac); return c ? displayName(c) : mac }
  let isoMac = $state('')

  // Traffic rules: everything tagged tik:rule:<name> (filter rules and simple queues), grouped by name
  const DAYS = ['sun', 'mon', 'tue', 'wed', 'thu', 'fri', 'sat']
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

<Section title="Security audit" desc="Things on this router worth fixing." ros="/ip/service/print; /ip/dns/print; /user/print; /ip/upnp/print; /system/package/update/print">
  {#each audit as a}<div class="row audit {a.level}"><Icon name="alert" size={14} /><b>{a.title}</b><span class="muted small grow">{a.detail}</span>
    {#if a.fix}{@const f = a.fix}<button class="btn small" onclick={() => review(f.title, [a.ros], f.intent)}>{f.label}…</button>{/if}</div>
  {:else}<div class="muted small">No issues found.</div>{/each}
</Section>

<Section title="Zones" desc="Interface lists act as zones. What new connections from a zone (row) to another (column) get. Default = what your other firewall rules do; Allow or Block adds a tikontrol rule first in the forward chain." ros="/interface/list/member/print; /ip/firewall/filter/print where comment~&quot;tik:zone:&quot;">
  {#if zones.length < 2}<div class="muted small">Zones need at least two interface lists with members (/interface/list).</div>
  {:else}
  <div class="scroll"><table class="matrix"><thead><tr><th scope="col">From \ To</th>{#each zones as z}<th scope="col">{z}</th>{/each}<th scope="col">Router</th></tr></thead>
    <tbody>{#each zones as f}{@const rc = zoneCell(rules, f, 'Router')}<tr class="nohover"><th scope="row">{f}</th>
      {#each zones as t}<td>{#if f === t}<span class="muted">—</span>{:else}{@const c = zoneCell(rules, f, t)}
        <span class="pill" class:ok={c.effect === 'Allow'} class:bad={c.effect === 'Block'} title={c.why}>{c.effect}</span>
        <select class="zsel" aria-label="{f} to {t}" value={c.policy} onchange={(e) => setZone(f, t, e)}>
          <option value="default">Default ({fallback(f, t)})</option><option value="allow">Allow</option><option value="block">Block</option></select>
        <span class="muted tiny why">{c.why}</span>{/if}</td>{/each}
      <td><span class="pill" class:ok={rc.effect === 'Allow'} class:bad={rc.effect === 'Block'} title={rc.why}>{rc.effect}</span><span class="muted tiny why">{rc.why}</span></td></tr>{/each}</tbody></table></div>
  <div class="muted tiny">Router = connections to the router itself (input chain), read only here: change it with router access rules below. Interfaces in several lists can make a cell approximate.</div>
  {/if}
</Section>

<Section title="Firewall rules" desc="{rules.length} rules, in order. tikontrol's own rules (tagged tik:fw: and tik:input:) can be removed here." ros="/ip/firewall/filter/print">
  <div class="scroll"><table>
    <thead><tr><th>#</th><th>Chain</th><th>Action</th><th>Match</th><th>Comment</th><th><span class="sr">Actions</span></th></tr></thead>
    <tbody>{#each rules as r, i}<tr class="nohover" class:off={r.disabled}><td>{i}</td><td>{r.chain}</td><td><span class="pill" class:bad={r.action === 'drop' || r.action === 'reject'} class:ok={r.action === 'accept'}>{r.action}</span></td><td class="small">{desc(r)}</td><td class="muted small">{r.comment ?? ''}</td>
      <td class="r0">{#if tagOf(r, 'tik:fw:') || tagOf(r, 'tik:input:')}<button class="btn small" onclick={() => removeRule(r)}>Remove</button>{/if}</td></tr>{/each}</tbody>
  </table></div>
  <h4>Add a forward rule</h4>
  <div class="add">
    <input type="text" bind:value={rule.name} placeholder="Rule name" aria-label="Rule name" maxlength="32" />
    <select bind:value={rule.action} aria-label="Action"><option>drop</option><option>accept</option><option>reject</option></select>
    <input type="text" bind:value={rule.src} placeholder="Source IP or IP group" aria-label="Source" />
    <input type="text" bind:value={rule.dst} placeholder="Destination IP or IP group" aria-label="Destination" />
    <select bind:value={rule.proto} aria-label="Protocol"><option>tcp</option><option>udp</option><option>icmp</option></select>
    <input type="text" bind:value={rule.port} placeholder="Port" aria-label="Port" style="width:80px" disabled={rule.proto === 'icmp'} />
    <button class="btn primary" disabled={!rule.name.trim()} onclick={() => review(`Firewall rule ${rule.name}`, [`# /ip/firewall/filter/add chain=forward action=${rule.action} protocol=${rule.proto} … comment=tik:fw:${rule.name} (first in the chain)`], ruleIntent())}><Icon name="plus" size={14} /> Add</button>
  </div>
  <h4 id="input-rules">Router access rules (input chain)</h4>
  <div class="warnbox small" role="note"><Icon name="alert" size={14} /> High risk: these rules decide who can reach the router itself, including WinBox, SSH, WebFig and the API tikontrol uses. A wrong rule locks you and tikontrol out. If that happens, wait: Safe apply removes the rule by itself after 5 minutes.</div>
  <div class="add">
    <input type="text" bind:value={inp.name} placeholder="Rule name" aria-label="Router access rule name" maxlength="32" />
    <select bind:value={inp.action} aria-label="Router access action"><option>accept</option><option>drop</option><option>reject</option></select>
    <input type="text" bind:value={inp.src} placeholder="Source IP or IP group" aria-label="Router access source" />
    <select bind:value={inp.proto} aria-label="Router access protocol"><option>tcp</option><option>udp</option><option>icmp</option></select>
    <input type="text" bind:value={inp.port} placeholder="Port, e.g. 8291" aria-label="Router access port" style="width:110px" disabled={inp.proto === 'icmp'} />
    <button class="btn primary" disabled={!inp.name.trim() || (inp.action !== 'accept' && !inp.src.trim() && !(inp.proto !== 'icmp' && inp.port.trim()))} onclick={() => review(`Router access rule ${inp.name}`, [`# /ip/firewall/filter/add chain=input action=${inp.action} protocol=${inp.proto} … comment=tik:input:${inp.name} (first in the chain)`], inpIntent())}><Icon name="plus" size={14} /> Add</button>
  </div>
  {#if inp.action !== 'accept'}<div class="muted tiny">A drop or reject rule needs a source or a port: dropping everything would lock you out.</div>{/if}
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


<Section title="Block apps" desc="Domain-based blocking (DNS + TLS SNI) for every device. True per-app detection needs CMR. For one client or a schedule, use a traffic rule above." ros="/ip/dns/static/print where comment~&quot;tik:app:&quot;">
  <div class="apps">{#each Object.keys(APPSETS) as a}<label class="row"><input type="checkbox" bind:checked={apps[a]} /> {a}{#if appState(a) === 'partly'}<span class="muted tiny">(partly blocked)</span>{/if}</label>{/each}</div>
  <Field label="Other domain" help="Subdomains are included"><input type="text" bind:value={customDomain} placeholder="example.com" aria-label="Other domain to block" /></Field>
  {#if appIntents().length > BATCH}<div class="muted tiny">That is {appIntents().length} domain changes; one apply holds at most {BATCH}. Apply some apps first.</div>{/if}
  <div class="r"><button class="btn primary" disabled={!appIntents().length || appIntents().length > BATCH} onclick={() => { const parts = appIntents(); send('App blocking', parts.map((i) => `# ${i.kind} ${i.domain}`), parts) }}>Review & apply</button></div>
  {#if blockedDomains.length}
    <table><thead><tr><th>Blocked domain</th><th><span class="sr">Actions</span></th></tr></thead>
      <tbody>{#each blockedDomains as d}<tr class="nohover"><td class="mono small">{d}</td><td class="r0"><button class="btn small" onclick={() => unblock(d)}>Unblock</button></td></tr>{/each}</tbody></table>
  {/if}
</Section>

<Section title="Ad blocking & content filtering" desc="RouterOS DNS adlists: every name in a list resolves to nothing. Add an ad-block, family or adult-content list by its https URL (hosts format)." ros="/ip/dns/adlist/print">
  <Field label="Blocked names"><span>{adblock.names.toLocaleString()} · {adblock.hits.toLocaleString()} blocked lookups</span></Field>
  {#if adlists.length}
    <div class="scroll"><table><thead><tr><th>List</th><th>Names</th><th>Blocked</th><th>Status</th><th><span class="sr">Actions</span></th></tr></thead>
      <tbody>{#each adlists as l}<tr class="nohover" class:off={l.disabled}><td class="mono small url">{l.url ?? '(local file)'}</td><td>{(l['name-count'] ?? 0).toLocaleString()}</td><td>{(l['match-count'] ?? 0).toLocaleString()}</td>
        <td><span class="pill" class:ok={!l.disabled}>{l.disabled ? 'Off' : 'On'}</span></td>
        <td class="r0">{#if l.url}<button class="btn small" onclick={() => review(`${l.disabled ? 'Enable' : 'Disable'} ad-block list`, [`/ip/dns/adlist/set [find url="${l.url}"] disabled=${l.disabled ? 'no' : 'yes'}`], { kind: 'set-adlist', url: l.url, disabled: !l.disabled })}>{l.disabled ? 'Enable' : 'Disable'}</button>
          <button class="btn small" onclick={() => review('Remove content filter list', [`/ip/dns/adlist/remove [find url="${l.url}"]`], { kind: 'remove-adlist', url: l.url })}>Remove</button>{/if}</td></tr>{/each}</tbody></table></div>
  {:else}<div class="muted small">No lists yet.</div>{/if}
  <div class="add"><input type="url" bind:value={adUrl} placeholder="https://… hosts list" aria-label="List URL" style="flex:1" />
    <button class="btn primary" disabled={!adUrl.trim().startsWith('https://')} onclick={() => review('Add content filter list', [`/ip/dns/adlist/add url=${adUrl.trim()} ssl-verify=yes`], { kind: 'add-adlist', url: adUrl.trim() })}><Icon name="plus" size={14} /> Add</button></div>
</Section>

<Section title="Region blocking" desc="Drop traffic to and from countries. Address lists filled daily from the ipdeny.com GeoIP feed." ros="/system/scheduler/print where comment~&quot;tik:geo:&quot;">
  {#if regions.length}
    <table><thead><tr><th>Country</th><th>Addresses loaded</th><th><span class="sr">Actions</span></th></tr></thead>
      <tbody>{#each regions as cc}<tr class="nohover"><td><b>{cc}</b></td><td>{count(`tik-geo-${cc}`).toLocaleString()}</td>
        <td class="r0"><button class="btn small" onclick={() => review(`Unblock region ${cc}`, [`/system/scheduler/remove [find comment="tik:geo:${cc}"]`, `/system/script/remove [find comment="tik:geo:${cc}"]`, `/ip/firewall/raw/remove [find comment="tik:geo:${cc}"]`, `/ip/firewall/address-list/remove [find comment="tik:geo:${cc}"]`], { kind: 'remove-region-block', country: cc })}>Remove</button></td></tr>{/each}</tbody></table>
  {/if}
  <Field label="Block countries" help="ISO codes, comma separated, up to {BATCH} at once"><input type="text" bind:value={geo.countries} placeholder="e.g. KP, IR" aria-label="Countries to block" /></Field>
  <div class="r"><button class="btn primary" disabled={!geoIntents().length} onclick={() => { const parts = geoIntents(); send('Region blocking', parts.map((i) => `# block ${i.country} from ${i.url}`), parts) }}>Review & apply</button></div>
</Section>

<Section title="IP groups" desc="Reusable address lists for rules: type a group name as the source or destination of a firewall rule." ros="/ip/firewall/address-list/print">
  {#each groups as g}{@const rows = lists.filter((l) => l.comment === `tik:group:${g}`)}
    <div class="row grp"><b>{g}</b><span class="mono small grow">{rows.map((l) => l.address).join(', ')}</span>
      <button class="btn small" onclick={() => review(`Remove IP group ${g}`, [`/ip/firewall/address-list/remove [find comment="tik:group:${g}"]`], { kind: 'remove-address-group', name: g })}>Remove</button></div>
  {/each}
  {#each others as o}<div class="row grp"><b>{o}</b><span class="muted small grow">{count(o)} address{count(o) === 1 ? '' : 'es'} · not made by tikontrol</span></div>{/each}
  {#if !groups.length && !others.length}<div class="muted small">No address lists yet.</div>{/if}
  <div class="add"><input type="text" bind:value={group.name} placeholder="Group name" aria-label="Group name" maxlength="32" /><input type="text" bind:value={group.items} placeholder="IPs, networks or ranges, comma separated" aria-label="Group addresses" style="flex:1" />
    <button class="btn primary" disabled={!group.name.trim() || !group.items.trim()} onclick={() => { const addresses = group.items.split(',').map((ip) => ip.trim()).filter(Boolean); review(`Group ${group.name}`, addresses.map((ip) => `/ip/firewall/address-list/add list=${group.name.trim()} address=${ip} comment=tik:group:${group.name.trim()}`), { kind: 'address-group', name: group.name.trim(), addresses }) }}><Icon name="plus" size={14} /> Add</button></div>
  <div class="muted tiny">Port groups are not offered: RouterOS has no port lists, so rules take ports directly.</div>
</Section>

<Section title="Device isolation" desc="Stop a device from talking to other devices on the same bridge. It keeps internet access." ros="/interface/bridge/filter/print where comment~&quot;tik:isolate:&quot;">
  {#each isolated as m}<div class="row grp"><b>{nameOf(m)}</b><span class="mono small grow">{m}</span>
    <button class="btn small" onclick={() => review(`End isolation for ${nameOf(m)}`, [`/interface/bridge/filter/remove [find comment="tik:isolate:${m}"]`], { kind: 'unisolate-device', mac: m })}>End isolation</button></div>
  {:else}<div class="muted small">No isolated devices.</div>{/each}
  <div class="add"><select bind:value={isoMac} aria-label="Device to isolate"><option value="">Device…</option>{#each clients.filter((c) => !isolated.includes(c.mac)) as c}<option value={c.mac}>{displayName(c)} · {c.mac}</option>{/each}</select>
    <button class="btn primary" disabled={!isoMac} onclick={() => review(`Isolate ${nameOf(isoMac)}`, [`/interface/bridge/filter/add chain=forward src-mac-address=${isoMac}/FF:FF:FF:FF:FF:FF action=drop comment=tik:isolate:${isoMac}`, `/interface/bridge/filter/add chain=forward dst-mac-address=${isoMac}/FF:FF:FF:FF:FF:FF action=drop comment=tik:isolate:${isoMac}`], { kind: 'isolate-device', mac: isoMac })}>Isolate</button></div>
</Section>

<Section title="Threat management">
  <Field label="IDS / IPS" na="Not available in RouterOS (no Suricata on this hardware)"><span></span></Field>
  <Field label="Honeypot" na="Not available in RouterOS"><span></span></Field>
  <Field label="Threat feed" help="Drop known-bad IPs (Spamhaus DROP), refreshed daily">
    {#if feeds.includes(SPAMHAUS)}<span class="pill ok">On</span> <span class="muted small">{count(`tik-threat-${SPAMHAUS}`).toLocaleString()} addresses</span>
      <button class="btn small" onclick={() => review('Remove threat feed', [`# remove the ${SPAMHAUS} script, schedule, raw rules and address list (tik:threat:${SPAMHAUS})`], { kind: 'remove-threat-feed', name: SPAMHAUS })}>Turn off</button>
    {:else}<span class="pill">Off</span> <button class="btn" onclick={() => review('Threat feed', ['# Spamhaus DROP list, refreshed daily'], { kind: 'threat-feed', name: SPAMHAUS, url: 'https://www.spamhaus.org/drop/drop.txt' })}>Turn on</button>{/if}
  </Field>
  {#each feeds.filter((f) => f !== SPAMHAUS) as f}<Field label="Feed {f}"><span class="pill ok">On</span> <span class="muted small">{count(`tik-threat-${f}`).toLocaleString()} addresses</span>
    <button class="btn small" onclick={() => review(`Remove threat feed ${f}`, [`# remove tik:threat:${f}`], { kind: 'remove-threat-feed', name: f })}>Remove</button></Field>{/each}
</Section>

<style>
  .audit { gap: 8px; padding: 6px 0; flex-wrap: wrap; color: var(--warn); }
  .audit.bad { color: var(--bad); }
  .audit b { color: var(--text); }
  .grow { flex: 1; min-width: 0; }
  .add { display: flex; gap: 6px; flex-wrap: wrap; margin-top: 10px; }
  .apps { display: flex; gap: 18px; flex-wrap: wrap; }
  .apps .row { gap: 6px; }
  .off { opacity: .45; }
  .days { display: flex; gap: 2px; }
  .day { padding: 4px 7px; border: 1px solid var(--line); background: var(--panel2); border-radius: 6px; cursor: pointer; font-size: 12px; color: var(--muted); }
  .day[aria-pressed='true'] { border-color: var(--accent); color: var(--accent); }
  .r0 { text-align: right; white-space: nowrap; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
  .r { text-align: right; margin-top: 10px; }
  .small { font-size: 12px; }
  .scroll { overflow-x: auto; }
  .matrix td { vertical-align: top; }
  .zsel { display: block; margin-top: 4px; font-size: 12px; }
  .why { display: block; max-width: 160px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .url { max-width: 360px; overflow-wrap: anywhere; }
  .grp { gap: 10px; padding: 6px 0; border-top: 1px solid var(--line); }
  h4 { margin: 16px 0 4px; font-size: 13px; }
  .warnbox { display: flex; gap: 8px; align-items: center; padding: 8px 10px; border-radius: 8px; background: color-mix(in srgb, var(--warn) 14%, transparent); color: var(--warn); margin: 8px 0; }
</style>
