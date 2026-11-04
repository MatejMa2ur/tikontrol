<script lang="ts">
  import { raw, router, isBackend } from '../../lib/data'
  import { review } from '../../lib/apply.svelte'
  import { api } from '../../lib/backend'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'
  import Toggle from '../../lib/ui/Toggle.svelte'

  const clock: any = raw.clock ?? {}
  const ntp: any = raw.ntp ?? {}
  const snmp: any = raw.snmp ?? {}
  const flow: any = raw.trafficFlow ?? {}
  const INSECURE = ['telnet', 'ftp', 'www', 'api']
  // ponytail: dedupe by name, the router lists ssh/resolver/detnet several times
  const services = [...new Map((raw.services ?? []).filter((s: any) => !s.name.startsWith('detnet')).map((s: any) => [s.name, s])).values()] as any[]
  let svc = $state(Object.fromEntries(services.map((s) => [s.name, !s.disabled])))
  const ourSyslog = (raw.loggingRules ?? []).some((r: any) => r.action === 'tik-remote')

  const servers = (Array.isArray(ntp.servers) ? ntp.servers : []).join(', ')
  let site = $state({ name: router.identity, tz: clock['time-zone-name'] ?? 'UTC', ntp: !!ntp.enabled, servers: servers || 'pool.ntp.org' })
  let mon = $state({ snmp: !!snmp.enabled, contact: snmp.contact ?? '', location: snmp.location ?? '', syslog: '', topics: 'info,warning,error,critical', flow: '', flowVersion: '9', retention: '30' })
  let safe = $state({ minutes: 5 })
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
    review('Remote syslog', [`/system/logging/action/add name=tik-remote target=remote remote=${addr} remote-port=${port || 514}`, ...topics.map((t) => `/system/logging/add topics=${t} action=tik-remote`)],
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
</script>

<Section title="Site" ros="/system/identity/print; /system/clock/print; /system/ntp/client/print">
  <Field label="Name"><input type="text" bind:value={site.name} aria-label="Router name" /></Field>
  <Field label="Time zone" help="IANA name, e.g. Europe/Bratislava"><input type="text" bind:value={site.tz} aria-label="Time zone" /></Field>
  <Field label="Sync time (NTP)"><Toggle bind:checked={site.ntp} label="NTP" /></Field>
  <Field label="NTP servers" help="Up to 4, comma separated"><input type="text" bind:value={site.servers} aria-label="NTP servers" /></Field>
  <div class="r"><button class="btn primary" onclick={siteReview}>Review & apply</button></div>
</Section>

<Section title="Router services" desc="Turn off what you don't use. Red ones send passwords unencrypted." ros="/ip/service/print">
  <div class="svc">{#each services as s}<label class="row" class:bad={INSECURE.includes(s.name) && svc[s.name]}><Toggle bind:checked={svc[s.name]} label={s.name} /><span>{s.name}</span><span class="muted mono small">:{s.port}</span></label>{/each}</div>
  <div class="r"><button class="btn primary" onclick={() => review('Router services', services.filter((s) => svc[s.name] === !!s.disabled).map((s) => `/ip/service/set ${s.name} disabled=${svc[s.name] ? 'no' : 'yes'}`), (() => { const off = services.filter((s) => !svc[s.name] && !s.disabled).map((s) => s.name as string); return off.length && off.every((n) => ['telnet', 'ftp', 'www', 'api'].includes(n)) && services.every((s) => svc[s.name] !== !!s.disabled || !svc[s.name]) ? { kind: 'disable-services', names: off } : undefined })())}>Review & apply</button></div>
</Section>

<Section title="SNMP" desc="tikontrol never reads or sets SNMP communities: change the default community in WinBox." ros="/snmp/print">
  <Field label="SNMP"><Toggle bind:checked={mon.snmp} label="SNMP" /></Field>
  <Field label="Contact"><input type="text" bind:value={mon.contact} aria-label="SNMP contact" /></Field>
  <Field label="Location"><input type="text" bind:value={mon.location} aria-label="SNMP location" /></Field>
  <div class="r"><button class="btn primary" onclick={snmpReview}>Review & apply</button></div>
</Section>

<Section title="Remote logging" ros="/system/logging/action/print; /ip/traffic-flow/print">
  {#if ourSyslog}
    <Field label="Remote syslog" help="Set up by tikontrol"><button class="btn" onclick={() => review('Stop remote syslog', ['/system/logging/remove [find action=tik-remote]', '/system/logging/action/remove [find name=tik-remote]'], { kind: 'remove-remote-syslog' })}>Remove</button></Field>
  {:else}
    <Field label="Remote syslog" help="Server address, optional :port"><input type="text" bind:value={mon.syslog} placeholder="192.0.2.50:514" aria-label="Syslog server" /></Field>
    <Field label="Topics" help="Comma separated"><input type="text" bind:value={mon.topics} aria-label="Syslog topics" /></Field>
    <div class="r"><button class="btn primary" disabled={!mon.syslog} onclick={syslogReview}>Review & apply</button></div>
  {/if}
  <Field label="NetFlow / IPFIX" help={flow.enabled ? 'Traffic-flow is on' : 'Collector address, optional :port'}><input type="text" bind:value={mon.flow} placeholder="192.0.2.60:2055" aria-label="Flow collector" /><select bind:value={mon.flowVersion} aria-label="Flow version"><option value="9">v9</option><option value="ipfix">IPFIX</option><option value="5">v5</option></select></Field>
  <div class="r">{#if flow.enabled}<button class="btn" onclick={() => review('Disable traffic-flow', ['/ip/traffic-flow/set enabled=no'], { kind: 'set-traffic-flow', enabled: false })}>Turn off</button> {/if}<button class="btn primary" onclick={flowReview}>Review & apply</button></div>
  <Field label="Keep history for" help="Stored by tikontrol, not on the router (fixed size for now)"><select bind:value={mon.retention} disabled={isBackend}><option value="7">7 days</option><option value="30">30 days</option><option value="365">1 year</option></select></Field>
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

<Section title="Safe apply" desc="Every change arms an automatic undo. If you don't press Keep in time, the router rolls back.">
  <Field label="Revert after" help="5 minutes for now"><select bind:value={safe.minutes} disabled={isBackend}><option value={2}>2 minutes</option><option value={5}>5 minutes</option><option value={10}>10 minutes</option></select></Field>
</Section>

<style>
  .svc { display: grid; grid-template-columns: repeat(auto-fill, minmax(190px, 1fr)); gap: 6px 14px; }
  .svc .row { gap: 8px; }
  .bad span:nth-child(2) { color: var(--bad); }
  .add { display: flex; gap: 6px; flex-wrap: wrap; }
  .r { text-align: right; margin-top: 10px; }
  .small { font-size: 12px; }
  .err { color: var(--bad); margin-top: 8px; }
  .cmd { margin: 10px 0 4px; }
  .scroll { overflow-x: auto; }
</style>
