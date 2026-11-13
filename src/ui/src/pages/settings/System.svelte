<script lang="ts">
  import { NOTICE } from '../../lib/legal'
  import { raw, router, isBackend } from '../../lib/data'
  import { review } from '../../lib/apply.svelte'
  import { api, session } from '../../lib/backend'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'
  import Toggle from '../../lib/ui/Toggle.svelte'
  import Icon from '../../lib/Icon.svelte'
  import TrustCa from '../../lib/ui/TrustCa.svelte'
  import { prefs } from '../../lib/live.svelte'
  import { designs, looks, palettes, tweaks, type Tweaks } from '../../lib/themes'

  const clock: any = raw.clock ?? {}
  const ntp: any = raw.ntp ?? {}
  const snmp: any = raw.snmp ?? {}
  const flow: any = raw.trafficFlow ?? {}
  const INSECURE = ['telnet', 'ftp', 'www', 'api']
  // ponytail: dedupe by name, the router lists ssh/resolver/detnet several times
  const services = [...new Map((raw.services ?? []).filter((s: any) => !s.name.startsWith('detnet')).map((s: any) => [s.name, s])).values()] as any[]
  let svc = $state(Object.fromEntries(services.map((s) => [s.name, !s.disabled])))
  const ourSyslog = (raw.loggingRules ?? []).some((r: any) => r.action === 'tikremote')

  const servers = (Array.isArray(ntp.servers) ? ntp.servers : []).join(', ')
  const siteInit = { name: router.identity, tz: clock['time-zone-name'] ?? 'UTC', ntp: !!ntp.enabled, servers: servers || 'pool.ntp.org' }
  let site = $state({ name: router.identity, tz: clock['time-zone-name'] ?? 'UTC', ntp: !!ntp.enabled, servers: servers || 'pool.ntp.org' })
  let mon = $state({ snmp: !!snmp.enabled, contact: snmp.contact ?? '', location: snmp.location ?? '', syslog: '', topics: 'info,warning,error,critical', flow: '', flowVersion: '9' })
  // apply buttons show only once a form differs from the router; Cancel puts it back
  const snmpInit = { snmp: mon.snmp, contact: mon.contact, location: mon.location }
  const siteDirty = $derived(JSON.stringify(site) !== JSON.stringify(siteInit))
  const snmpDirty = $derived(mon.snmp !== snmpInit.snmp || mon.contact !== snmpInit.contact || mon.location !== snmpInit.location)
  const svcDirty = $derived(services.some((s) => svc[s.name] === !!s.disabled))
  let safe = $state({ minutes: 5 })
  // tikontrol's own history (src/server/src/live.rs prune): days kept, stored on the tikontrol server
  const isAdmin = isBackend && session.me?.role === 'admin'
  let keep = $state({ days: 90, saved: 90, msg: '' })
  if (isBackend) api<{ days: number }>('/retention').then((r) => { keep.days = keep.saved = r.days }).catch(() => {})
  async function saveRetention() {
    keep.msg = ''
    if (+keep.days < keep.saved && !confirm('Delete history older than this now?')) return
    try { await api('/retention', { method: 'PUT', body: { days: +keep.days } }); keep.saved = +keep.days; keep.msg = 'Saved. Older history was deleted.' } catch (e) { keep.msg = (e as Error).message }
  }
  let diag = $state({ tool: 'ping', host: '1.1.1.1', count: 4 })
  let out = $state<{ command: string; rows: Record<string, string>[]; note?: string | null } | null>(null)
  let diagErr = $state('')
  let busy = $state(false)

  const q = (s: string) => `"${s.replace(/["\\$]/g, (c) => '\\' + c)}"`
  const list = (s: string) => s.split(',').map((x) => x.trim()).filter(Boolean)

  function siteReview() {
    const ns = list(site.servers)
    review('Site settings', [`/system/identity/set name=${q(site.name)}`, `/system/clock/set time-zone-autodetect=no time-zone-name=${site.tz}`, `/system/ntp/client/set enabled=${site.ntp ? 'yes' : 'no'} servers=${ns.join(',')}`],
      { kind: 'site-settings', identity: site.name, time_zone: site.tz, ntp_enabled: site.ntp, ntp_servers: ns })
  }
  function snmpReview() {
    review(`${mon.snmp ? 'Enable' : 'Disable'} SNMP`, [`/snmp/set enabled=${mon.snmp ? 'yes' : 'no'} contact=${q(mon.contact)} location=${q(mon.location)}`],
      { kind: 'set-snmp', enabled: mon.snmp, contact: mon.contact, location: mon.location })
  }
  function syslogReview() {
    const [addr, port] = mon.syslog.split(':')
    const topics = list(mon.topics)
    review('Remote syslog', [`/system/logging/action/add name=tikremote target=remote remote=${addr} remote-port=${port || 514}`, ...topics.map((t) => `/system/logging/add topics=${t} action=tikremote`)],
      { kind: 'remote-syslog', address: addr, port: +(port || 514), topics })
  }
  function flowReview() {
    const [addr, port] = mon.flow.split(':')
    review('Traffic-flow export', ['/ip/traffic-flow/set enabled=yes', ...(addr ? [`/ip/traffic-flow/target/add dst-address=${addr} port=${port || 2055} version=${mon.flowVersion}`] : [])],
      { kind: 'set-traffic-flow', enabled: true, ...(addr ? { address: addr, port: +(port || 2055), version: mon.flowVersion } : {}) })
  }

  const cli = () => diag.tool === 'ping' ? `/ping ${diag.host} count=${diag.count}` : diag.tool === 'traceroute' ? `/tool/traceroute ${diag.host} count=1` : `/ip/dns/cache/print where name=${diag.host}`
  async function runDiag() {
    out = null
    diagErr = ''
    if (!isBackend) { review(`Run ${diag.tool}`, [cli()]); return }
    busy = true
    try {
      const body = diag.tool === 'dns' ? { name: diag.host } : diag.tool === 'ping' ? { host: diag.host, count: diag.count } : { host: diag.host }
      out = await api(`/tools/${diag.tool}`, { method: 'POST', body })
    } catch (e) { diagErr = (e as Error).message } finally { busy = false }
  }
  const cols = $derived(out ? [...new Set(out.rows.flatMap((r) => Object.keys(r)))] : [])
  // every switched service as its own set-service (api-ssl, tikontrol's own link, is refused by the server)
  const serviceIntents = () => services.filter((s) => svc[s.name] === !!s.disabled).slice(0, 8).map((s) => ({ kind: 'set-service', name: s.name as string, enabled: !!svc[s.name] }))
</script>

<Section title="Site" ros="/system/identity/print; /system/clock/print; /system/ntp/client/print">
  <Field label="Name"><input type="text" bind:value={site.name} aria-label="Router name" /></Field>
  <Field label="Time zone" help="IANA name, e.g. Europe/Bratislava"><input type="text" bind:value={site.tz} aria-label="Time zone" /></Field>
  <Field label="Sync time (NTP)"><Toggle bind:checked={site.ntp} label="NTP" /></Field>
  <Field label="NTP servers" help="Up to 4, comma separated"><input type="text" bind:value={site.servers} aria-label="NTP servers" /></Field>
  {#if siteDirty}<div class="r"><button class="btn" onclick={() => (site = { ...siteInit })}>Cancel</button> <button class="btn primary" disabled={!site.name.trim()} onclick={siteReview}>Review & apply</button></div>{/if}
</Section>

<Section title="Router services" desc="Turn off what you don't use. Red ones send passwords unencrypted." ros="/ip/service/print">
  <div class="svc">{#each services as s}<label class="row" class:bad={INSECURE.includes(s.name) && svc[s.name]}><Toggle bind:checked={svc[s.name]} label={s.name} /><span>{s.name}</span><span class="muted mono small">:{s.port}</span></label>{/each}</div>
  {#if svcDirty}<div class="r"><button class="btn" onclick={() => (svc = Object.fromEntries(services.map((s) => [s.name, !s.disabled])))}>Cancel</button> <button class="btn primary" onclick={() => { const parts = serviceIntents(); review('Router services', services.filter((s) => svc[s.name] === !!s.disabled).map((s) => `/ip/service/set ${s.name} disabled=${svc[s.name] ? 'no' : 'yes'}`), parts.length === 1 ? parts[0] : { kind: 'batch', title: 'Router services', intents: parts }) }}>Review & apply</button></div>{/if}
</Section>

<Section title="SNMP" desc="tikontrol never reads or sets SNMP communities: change the default community in WinBox." ros="/snmp/print">
  <Field label="SNMP"><Toggle bind:checked={mon.snmp} label="SNMP" /></Field>
  <Field label="Contact"><input type="text" bind:value={mon.contact} aria-label="SNMP contact" /></Field>
  <Field label="Location"><input type="text" bind:value={mon.location} aria-label="SNMP location" /></Field>
  {#if snmpDirty}<div class="r"><button class="btn" onclick={() => Object.assign(mon, snmpInit)}>Cancel</button> <button class="btn primary" onclick={snmpReview}>Review & apply</button></div>{/if}
</Section>

<Section title="Remote logging" ros="/system/logging/print; /ip/traffic-flow/print">
  {#if ourSyslog}
    <Field label="Remote syslog" help="Set up by tikontrol"><button class="btn" onclick={() => review('Stop remote syslog', ['/system/logging/remove [find action=tikremote]', '/system/logging/action/remove [find name=tikremote]'], { kind: 'remove-remote-syslog' })}>Remove</button></Field>
  {:else}
    <Field label="Remote syslog" help="Server address, optional :port"><input type="text" bind:value={mon.syslog} placeholder="192.0.2.50:514" aria-label="Syslog server" /></Field>
    <Field label="Topics" help="Comma separated"><input type="text" bind:value={mon.topics} aria-label="Syslog topics" /></Field>
    <div class="r"><button class="btn primary" disabled={!mon.syslog} title={mon.syslog ? '' : 'Enter a syslog server first'} onclick={syslogReview}>Review & apply</button></div>
  {/if}
  <Field label="NetFlow / IPFIX" help={flow.enabled ? 'Traffic-flow is on' : 'Collector address, optional :port'}><input type="text" bind:value={mon.flow} placeholder="192.0.2.60:2055" aria-label="Flow collector" /><select bind:value={mon.flowVersion} aria-label="Flow version"><option value="9">v9</option><option value="ipfix">IPFIX</option><option value="5">v5</option></select></Field>
  <div class="r">{#if flow.enabled}<button class="btn" onclick={() => review('Disable traffic-flow', ['/ip/traffic-flow/set enabled=no'], { kind: 'set-traffic-flow', enabled: false })}>Turn off</button> {/if}<button class="btn primary" disabled={!mon.flow.trim()} title={mon.flow.trim() ? '' : 'Enter a collector address first'} onclick={flowReview}>Review & apply</button></div>
</Section>

{#if isBackend}
<Section title="Trust this tikontrol" desc="Stop the certificate warning on your devices.">
  <TrustCa />
</Section>
{/if}

<Section title="Data retention" desc="How long tikontrol keeps its own history: traffic charts, events and per-client usage. Stored on the tikontrol server, not the router. Minute detail is kept for at most 7 days.">
  <Field label="Keep history for" help={isBackend ? (isAdmin ? 'Shortening it deletes older history right away' : 'Only an admin can change this') : 'Available with the tikontrol server'}>
    <select bind:value={keep.days} disabled={!isAdmin} aria-label="History retention">{#each [[1, '1 day'], [7, '7 days'], [30, '30 days'], [90, '90 days']] as [d, l]}<option value={d}>{l}</option>{/each}</select>
    <button class="btn" disabled={!isAdmin || +keep.days === keep.saved} onclick={saveRetention}>Save</button></Field>
  {#if keep.msg}<div class="muted small" role="status">{keep.msg}</div>{/if}
</Section>

<Section title="Debug tools" desc="Run from the router. Read-only: ping, traceroute and a DNS cache lookup. Packet capture needs the sniff policy, which tikontrol doesn't use.">
  <div class="add">
    <select bind:value={diag.tool} aria-label="Tool"><option value="ping">Ping</option><option value="traceroute">Traceroute</option><option value="dns">DNS cache lookup</option></select>
    <input type="text" bind:value={diag.host} aria-label="Host" />
    {#if diag.tool === 'ping'}<select bind:value={diag.count} aria-label="Count">{#each [1, 4, 10] as n}<option value={n}>{n}×</option>{/each}</select>{/if}
    <button class="btn" disabled={busy || !diag.host} onclick={runDiag}>{busy ? 'Running…' : 'Run'}</button>
  </div>
  {#if diagErr}<div class="err small" role="alert">{diagErr}</div>{/if}
  {#if out}
    <div class="mono muted small cmd">{out.command}</div>
    {#if out.rows.length}
      <div class="scroll"><table><thead><tr>{#each cols as c}<th>{c}</th>{/each}</tr></thead>
        <tbody>{#each out.rows as row}<tr class="nohover">{#each cols as c}<td class="mono small">{row[c] ?? ''}</td>{/each}</tr>{/each}</tbody></table></div>
    {:else}<div class="muted small">No results.</div>{/if}
    {#if out.note}<div class="muted small">{out.note}</div>{/if}
  {/if}
</Section>

<Section title="Appearance" desc="Changes show right away and are saved in this browser, not on the router.">
  <Field label="Look" help="Sets mode, colours and design in one go; fine-tune them below"><div class="opts" role="radiogroup" aria-label="Look">
    {#each Object.entries(looks) as [id, l]}{@const c = palettes[l.palette][l.mode]}
      <label class="opt" class:on={!prefs.auto && prefs.theme === l.mode && prefs.palette === l.palette && prefs.design === l.design} title={l.desc}><input type="radio" name="look" value={id} onchange={() => Object.assign(prefs, { auto: false, theme: l.mode, palette: l.palette, design: l.design, tweaks: {} })} />
        <span class="sw" style:background={c.bg} style:border-color={c.line} style:border-radius={designs[l.design].radiusSm} aria-hidden="true"><i style:background={c.panel}></i><i style:background={c.accentBg}></i><i style:background={c.text}></i></span>{l.label}</label>
    {/each}
  </div></Field>
  <Field label="Mode"><div class="opts" role="radiogroup" aria-label="Mode">
    {#each [['auto', 'Auto (system)'], ['dark', 'Dark'], ['light', 'Light']] as const as [v, l]}{@const on = v === 'auto' ? prefs.auto : !prefs.auto && prefs.theme === v}
      <label class="opt" class:on><input type="radio" name="mode" value={v} checked={on} onchange={() => { prefs.auto = v === 'auto'; if (v !== 'auto') prefs.theme = v }} />{l}</label>{/each}
  </div></Field>
  <Field label="Colours" help="Every palette meets WCAG AA contrast in light and dark"><div class="opts" role="radiogroup" aria-label="Colours">
    {#each Object.entries(palettes) as [id, p]}{@const c = p[prefs.theme]}
      <label class="opt" class:on={prefs.palette === id}><input type="radio" name="palette" value={id} bind:group={prefs.palette} />
        <span class="sw" style:background={c.bg} style:border-color={c.line} aria-hidden="true"><i style:background={c.panel}></i><i style:background={c.accentBg}></i><i style:background={c.text}></i></span>{p.label}</label>
    {/each}
  </div></Field>
  <Field label="Design" help="Corners, spacing, borders or shadows, font and icon lines"><div class="opts" role="radiogroup" aria-label="Design">
    {#each Object.entries(designs) as [id, d]}
      <label class="opt" class:on={prefs.design === id}><input type="radio" name="design" value={id} bind:group={prefs.design} />
        <span class="shape" style:border-radius={d.radiusSm} style:border-width={d.cardBorder} style:box-shadow={d.cardShadow} aria-hidden="true"></span>{d.label}</label>
    {/each}
  </div></Field>
  <Field label="Fine-tune" help="Change one part of the design; Design default keeps the design's own choice"><div class="tweaks">
    {#each Object.entries(tweaks) as [k, t]}
      <label class="tw"><span class="small muted">{t.label}</span>
        <select value={prefs.tweaks[k as keyof Tweaks] ?? ''} onchange={(e) => { const v = e.currentTarget.value; const n: Tweaks = { ...prefs.tweaks }; if (v) n[k as keyof Tweaks] = v; else delete n[k as keyof Tweaks]; prefs.tweaks = n }} aria-label={t.label}>
          <option value="">Design default</option>
          {#each Object.entries(t.options) as [id, o]}<option value={id}>{o.label}</option>{/each}
        </select></label>
    {/each}
    {#if Object.keys(prefs.tweaks).length}<button class="btn small" type="button" onclick={() => (prefs.tweaks = {})}>Reset</button>{/if}
  </div></Field>
  <div class="preview" role="group" aria-label="Preview">
    <div class="card"><div class="row between"><b>Preview</b><span class="pill ok"><span class="dot ok"></span>Online</span></div>
      <p class="muted small">Muted text, <a href="#/settings/system">a link</a> and status: <span class="pill warn">Warning</span> <span class="pill bad">Offline</span></p>
      <div class="row"><button class="btn primary" type="button">Primary</button><button class="btn" type="button"><Icon name="edit" size={14} />Button</button><Toggle checked label="Example switch" /></div>
    </div>
  </div>
</Section>

<Section title="Safe apply" desc="Every change arms an automatic undo. If you don't press Keep in time, the router rolls back.">
  <Field label="Revert after" help="5 minutes for now"><select bind:value={safe.minutes} disabled={isBackend}><option value={2}>2 minutes</option><option value={5}>5 minutes</option><option value={10}>10 minutes</option></select></Field>
</Section>

<Section title="About" desc="What tikontrol is, and what it is not.">
  <p class="muted small about">{NOTICE}</p>
</Section>

<style>
  .tweaks { display: flex; flex-wrap: wrap; gap: 10px; align-items: flex-end; }
  .tw { display: flex; flex-direction: column; gap: 3px; }
  .svc { display: grid; grid-template-columns: repeat(auto-fill, minmax(190px, 1fr)); gap: 6px 14px; }
  .svc .row { gap: 8px; }
  .bad span:nth-child(2) { color: var(--bad); }
  .add { display: flex; gap: 6px; flex-wrap: wrap; }
  .r { text-align: right; margin-top: 10px; }
  .small { font-size: 12px; }
  .opts { display: flex; flex-wrap: wrap; gap: 6px; justify-content: flex-end; }
  .opt { display: inline-flex; align-items: center; gap: 7px; padding: 5px 10px; border: 1px solid var(--line); border-radius: var(--radius-sm); background: var(--panel2); cursor: pointer; font-weight: 550; }
  .opt.on { border-color: var(--accent); }
  .opt input { margin: 0; accent-color: var(--accent-bg); }
  .sw { display: inline-flex; gap: 2px; padding: 2px; border: 1px solid; border-radius: 4px; }
  .sw i { width: 9px; height: 12px; border-radius: 2px; }
  .shape { width: 18px; height: 14px; background: var(--line); border: 0 solid var(--muted); }
  .preview { margin-top: 12px; padding: var(--pad); background: var(--bg); border-radius: var(--radius); }
  .preview p { margin: 8px 0 10px; }
  @media (max-width: 700px) { .opts { justify-content: flex-start; } }
  .err { color: var(--bad); margin-top: 8px; }
  .cmd { margin: 10px 0 4px; }
  .scroll { overflow-x: auto; }
  .about { margin: 0; max-width: 70ch; line-height: 1.5; }
</style>
