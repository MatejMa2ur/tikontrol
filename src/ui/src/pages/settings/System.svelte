<script lang="ts">
  import { raw, router } from '../../lib/data'
  import { review } from '../../lib/apply.svelte'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'
  import Toggle from '../../lib/ui/Toggle.svelte'

  const clock: any = raw.clock ?? {}
  const ntp: any = raw.ntp ?? {}
  const snmp: any = raw.snmp ?? {}
  const INSECURE = ['telnet', 'ftp', 'www', 'api']
  // ponytail: dedupe by name, the router lists ssh/resolver/detnet several times
  const services = [...new Map((raw.services ?? []).filter((s: any) => !s.name.startsWith('detnet')).map((s: any) => [s.name, s])).values()] as any[]
  let svc = $state(Object.fromEntries(services.map((s) => [s.name, !s.disabled])))

  let site = $state({ name: router.identity, tz: clock['time-zone-name'] ?? 'UTC', ntp: !!ntp.enabled })
  let mon = $state({ snmp: !!snmp.enabled, syslog: '', retention: '30' })
  let safe = $state({ minutes: 5 })
  let diag = $state({ tool: 'ping', host: '1.1.1.1' })
</script>

<Section title="Site" ros="/system/identity/print; /system/clock/print; /system/ntp/client/print">
  <Field label="Name"><input type="text" bind:value={site.name} /></Field>
  <Field label="Time zone"><input type="text" bind:value={site.tz} /></Field>
  <Field label="Sync time (NTP)"><Toggle bind:checked={site.ntp} label="NTP" /></Field>
  <div class="r"><button class="btn primary" onclick={() => review('Site settings', [`/system/identity/set name="${site.name}"`, `/system/clock/set time-zone-name=${site.tz}`, `/system/ntp/client/set enabled=${site.ntp ? 'yes' : 'no'} servers=pool.ntp.org`])}>Review & apply</button></div>
</Section>

<Section title="Router services" desc="Turn off what you don't use. Red ones send passwords unencrypted." ros="/ip/service/print">
  <div class="svc">{#each services as s}<label class="row" class:bad={INSECURE.includes(s.name) && svc[s.name]}><Toggle bind:checked={svc[s.name]} label={s.name} /><span>{s.name}</span><span class="muted mono small">:{s.port}</span></label>{/each}</div>
  <div class="r"><button class="btn primary" onclick={() => review('Router services', services.filter((s) => svc[s.name] === !!s.disabled).map((s) => `/ip/service/set ${s.name} disabled=${svc[s.name] ? 'no' : 'yes'}`))}>Review & apply</button></div>
</Section>

<Section title="Monitoring" ros="/snmp/print; /system/logging/action/print">
  <Field label="SNMP"><Toggle bind:checked={mon.snmp} label="SNMP" /></Field>
  <Field label="Remote syslog"><input type="text" bind:value={mon.syslog} placeholder="10.0.0.50" /></Field>
  <Field label="NetFlow / IPFIX"><a class="btn" href="#/insights/flows">Configure in Insights</a></Field>
  <Field label="Keep history for" help="Stored by tikontrol, not on the router"><select bind:value={mon.retention}><option value="7">7 days</option><option value="30">30 days</option><option value="365">1 year</option></select></Field>
  <div class="r"><button class="btn primary" onclick={() => review('Monitoring', [`/snmp/set enabled=${mon.snmp ? 'yes' : 'no'}`, ...(mon.syslog ? [`/system/logging/action/add name=tik-remote target=remote remote=${mon.syslog}`, '/system/logging/add topics=info,warning,error action=tik-remote'] : [])])}>Review & apply</button></div>
</Section>

<Section title="Debug tools">
  <div class="add"><select bind:value={diag.tool}><option>ping</option><option>traceroute</option><option value="dns">DNS lookup</option><option value="torch">Torch</option></select><input type="text" bind:value={diag.host} />
    <button class="btn" onclick={() => review(`Run ${diag.tool}`, [diag.tool === 'ping' ? `/ping ${diag.host} count=5` : diag.tool === 'traceroute' ? `/tool/traceroute ${diag.host} count=1` : diag.tool === 'dns' ? `:put [:resolve ${diag.host}]` : '/tool/torch interface=bridge duration=10'])}>Run</button></div>
</Section>

<Section title="Safe apply" desc="Every change arms an automatic undo. If you don't press Keep in time, the router rolls back.">
  <Field label="Revert after"><select bind:value={safe.minutes}><option value={2}>2 minutes</option><option value={5}>5 minutes</option><option value={10}>10 minutes</option></select></Field>
</Section>

<style>
  .svc { display: grid; grid-template-columns: repeat(auto-fill, minmax(190px, 1fr)); gap: 6px 14px; }
  .svc .row { gap: 8px; }
  .bad span:nth-child(2) { color: var(--bad); }
  .add { display: flex; gap: 6px; flex-wrap: wrap; }
  .r { text-align: right; margin-top: 10px; }
  .small { font-size: 12px; }
</style>
