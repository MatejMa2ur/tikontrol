<script lang="ts">
  import { keep, kept } from '../lib/keep'
  import { events, clients, insights, isBackend, raw, type Event } from '../lib/data'
  import { review, apply } from '../lib/apply.svelte'
  import { api } from '../lib/backend'
  import Ros from '../lib/Ros.svelte'
  import Icon from '../lib/Icon.svelte'
  import Tabs from '../lib/ui/Tabs.svelte'
  import Section from '../lib/ui/Section.svelte'
  import Field from '../lib/ui/Field.svelte'
  import Toggle from '../lib/ui/Toggle.svelte'
  import AlarmManager from '../lib/AlarmManager.svelte'

  let { tab = 'events' }: { tab?: string } = $props()

  // ---- Events: router log + events we derive ourselves (client joined/left, admin changes) ----
  // local "YYYY-MM-DD HH:MM:SS", the format of RouterOS log times, so everything sorts together
  const local = (ms: number) => new Date(ms - new Date(ms).getTimezoneOffset() * 60000).toISOString().replace('T', ' ').slice(0, 19)
  // with the server: the events it derived and stored (client joined/left/roamed, Internet down/back);
  // without it: clients that came online in the last hour, from the snapshot
  let stored = $state<Event[]>([])
  if (isBackend) api<{ t: number; cat: Event['cat']; level: Event['level']; text: string }[]>('/events').then((l) => (stored = (l ?? []).map((e) => ({ time: local(e.t * 1000), cat: e.cat, level: e.level, text: e.text })))).catch(() => {})
  const ownEvents: Event[] = isBackend ? [] : clients.filter((c) => c.online && c.uptimeS != null && c.uptimeS < 3600).map((c) => ({ time: local(Date.now() - c.uptimeS! * 1000), cat: 'Clients', level: 'info', text: `${c.name} came online (${c.wired ? 'wired' : c.band})` }))
  const all = $derived([...apply.done.map((d) => ({ time: local(Date.now()).slice(0, 11) + d.at, cat: 'Admin' as const, level: 'info' as const, text: `Configuration changed: ${d.title}` })), ...ownEvents, ...stored, ...events].sort((a, b) => b.time.localeCompare(a.time)))
  const cats = ['All', 'Clients', 'WiFi', 'System', 'Security', 'Admin'] as const
  let cat = $state<(typeof cats)[number]>(kept('events.cat', 'All'))
  let q = $state(kept('events.q', ''))
  const rows = $derived(all.filter((e) => (cat === 'All' || e.cat === cat) && (!q || e.text.toLowerCase().includes(q.toLowerCase()))))
  const count = (c: string) => (c === 'All' ? all.length : all.filter((e) => e.cat === c).length)
  const lvl = (e: Event) => (e.level === 'error' ? 'bad' : e.level === 'warning' ? 'warn' : '')
  function exportCsv() {
    const csv = ['time,category,level,event', ...rows.map((e) => [e.time, e.cat, e.level, `"${e.text.replace(/^[=+\-@\t\r]/, "'$&").replace(/"/g, '""')}"`].join(','))].join('\n')
    const a = document.createElement('a')
    a.href = URL.createObjectURL(new Blob([csv], { type: 'text/csv' }))
    a.download = `tikontrol-events-${new Date().toISOString().slice(0, 10)}.csv`
    a.click()
  }

  // ---- Alarm Manager ----
  type Rule = { id: string; name: string; on: boolean; sev: 'info' | 'warning' | 'critical'; channels: string[] }
  let rules = $state<Rule[]>([
    { id: 'wan', name: 'Internet goes down', on: true, sev: 'critical', channels: ['push', 'email'] },
    { id: 'offline', name: 'Device goes offline', on: true, sev: 'critical', channels: ['push'] },
    { id: 'newclient', name: 'New client joins the network', on: true, sev: 'info', channels: ['push'] },
    { id: 'cpu', name: 'CPU above 90% for 5 minutes', on: false, sev: 'warning', channels: ['email'] },
    { id: 'mem', name: 'Memory above 85%', on: true, sev: 'warning', channels: ['email'] },
    { id: 'fw', name: 'Firmware update available', on: true, sev: 'info', channels: ['email'] },
    { id: 'login', name: 'Admin login from a new address', on: true, sev: 'warning', channels: ['push', 'email'] },
    { id: 'roam', name: 'Client keeps switching bands', on: false, sev: 'info', channels: [] },
  ])
  let ch = $state({ email: '', ntfy: '', telegram: '', webhook: '' })
  const ourSyslog = (raw.loggingRules ?? []).some((r: any) => r.action === 'tikremote')
  let sl = $state({ server: '', topics: 'info,warning,error,critical' })
  function syslogReview() {
    const [address, port] = sl.server.split(':')
    const topics = sl.topics.split(',').map((t) => t.trim()).filter(Boolean)
    review('Remote syslog', [`/system/logging/action/add name=tikremote target=remote remote=${address} remote-port=${port || 514}`, ...topics.map((t) => `/system/logging/add topics=${t} action=tikremote`)], { kind: 'remote-syslog', address, port: +(port || 514), topics })
  }
  let quiet = $state({ on: false, from: '23:00', to: '07:00' })
  let unacked = $state(0) // from the server when isBackend; AlarmManager keeps it current once opened
  if (isBackend) api<{ unacked: number }>('/alerts').then((v) => (unacked = v.unacked)).catch(() => {})
  let history = $state(insights.filter((i) => i.level !== 'ok').map((i, n) => ({ id: n, title: i.title, sev: i.level === 'bad' ? 'critical' : 'warning', at: 'Today', ack: false })))
  $effect(() => { keep('events.cat', cat); keep('events.q', q) })
</script>

<div class="grid">
  <h1>Logs & Alarms</h1>
  <Tabs base="events" active={tab} tabs={[{ id: 'events', label: 'Events', count: all.length }, { id: 'alarms', label: 'Alarm Manager', count: isBackend ? unacked : history.filter((h) => !h.ack).length }, { id: 'channels', label: 'Notifications' }]} />

  {#if tab === 'events'}
    <div class="row between wrap">
      <div class="seg">{#each cats as c}<button class:on={cat === c} onclick={() => (cat = c)}>{c} {count(c)}</button>{/each}</div>
      <div class="row"><input type="search" placeholder="Search events…" bind:value={q} /><button class="btn" onclick={exportCsv}><Icon name="download" size={14} /> CSV</button></div>
    </div>
    <section class="card flush">
      <table>
        <thead><tr><th>Time</th><th>Category</th><th>Event</th></tr></thead>
        <tbody>
          {#each rows as e}<tr><td class="mono muted">{e.time.slice(5)}</td><td><span class="pill {lvl(e)}">{e.cat}</span></td><td class="wrap">{e.text}</td></tr>
          {:else}<tr><td colspan="3" class="muted">No events match.</td></tr>{/each}
        </tbody>
      </table>
    </section>
    <Ros cmd="/log/print follow" />
  {:else if isBackend && (tab === 'alarms' || tab === 'channels')}
    <AlarmManager view={tab === 'alarms' ? 'alarms' : 'channels'} bind:unacked />
    {#if tab === 'channels'}{@render syslog()}{/if}
  {:else if tab === 'alarms'}
    <Section title="Active alarms" desc="Things that fired and haven't been acknowledged.">
      {#each history as h}
        <div class="row between alarm" class:ack={h.ack}>
          <span class="row"><span class="dot {h.sev === 'critical' ? 'bad' : 'warn'}"></span><b>{h.title}</b><span class="muted small">{h.at}</span></span>
          <button class="btn" disabled={h.ack} onclick={() => (h.ack = true)}>{h.ack ? 'Acknowledged' : 'Acknowledge'}</button>
        </div>
      {:else}<div class="muted small">All clear.</div>{/each}
    </Section>
    <Section title="Alarm rules" desc="When to notify you. Rules run in tikontrol on live router data; device-level ones can also run as CMR alerts.">
      <table>
        <thead><tr><th>On</th><th>When</th><th>Severity</th><th>Notify via</th></tr></thead>
        <tbody>
          {#each rules as r}
            <tr class="nohover">
              <td><Toggle bind:checked={r.on} label={r.name} /></td>
              <td>{r.name}</td>
              <td><select bind:value={r.sev}><option>info</option><option>warning</option><option>critical</option></select></td>
              <td class="row">{#each ['push', 'email', 'telegram', 'webhook'] as c}<label class="chk"><input type="checkbox" checked={r.channels.includes(c)} onchange={() => (r.channels = r.channels.includes(c) ? r.channels.filter((x) => x !== c) : [...r.channels, c])} /> {c}</label>{/each}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </Section>
    <Section title="Quiet hours" desc="Hold non-critical alarms overnight.">
      <Field label="Enabled"><Toggle bind:checked={quiet.on} label="Quiet hours" /></Field>
      <Field label="From / to"><input type="text" bind:value={quiet.from} /><input type="text" bind:value={quiet.to} /></Field>
    </Section>
  {:else}
    <Section title="Notification channels" desc="Where alarms go. Secrets are stored in tikontrol, never on the router.">
      <Field label="Email" help="SMTP via tikontrol"><input type="text" bind:value={ch.email} placeholder="you@example.com" /></Field>
      <Field label="Push (ntfy)" help="ntfy.sh topic or self-hosted URL"><input type="text" bind:value={ch.ntfy} placeholder="https://ntfy.sh/my-home" /></Field>
      <Field label="Telegram" help="Bot token + chat ID"><input type="text" bind:value={ch.telegram} placeholder="123456:ABC… / chat id" /></Field>
      <Field label="Webhook" help="POST JSON (works with Home Assistant)"><input type="text" bind:value={ch.webhook} placeholder="https://…" /></Field>
      <div class="r"><button class="btn">Send test</button> <button class="btn primary">Save</button></div>
    </Section>
    {@render syslog()}
  {/if}
</div>

{#snippet syslog()}
  <Section title="Remote syslog / SIEM" desc="Forward the router log to a syslog server on your network (unencrypted UDP)." ros="/system/logging/print">
    {#if ourSyslog}
      <Field label="Syslog server" help="Set up by tikontrol"><button class="btn" onclick={() => review('Stop remote syslog', ['/system/logging/remove [find action=tikremote]', '/system/logging/action/remove [find name=tikremote]'], { kind: 'remove-remote-syslog' })}>Remove</button></Field>
    {:else}
      <Field label="Syslog server" help="Address, optional :port"><input type="text" placeholder="192.0.2.50:514" bind:value={sl.server} aria-label="Syslog server" /></Field>
      <Field label="Topics" help="Comma separated"><input type="text" bind:value={sl.topics} aria-label="Syslog topics" /></Field>
      <div class="r"><button class="btn primary" disabled={!sl.server} onclick={syslogReview}>Review & apply</button></div>
    {/if}
  </Section>
{/snippet}

<style>
  .flush { padding: 4px 6px; overflow-x: auto; }
  .wrap { white-space: normal; }
  tbody tr { cursor: default; }
  tbody tr:hover { background: none; }
  .alarm { padding: 8px 0; border-top: 1px solid var(--line); gap: 10px; }
  .alarm:first-of-type { border-top: 0; }
  .alarm .row { gap: 10px; }
  .ack { opacity: .5; }
  .chk { display: inline-flex; gap: 4px; align-items: center; font-size: 12px; margin-right: 10px; color: var(--muted); }
  .r { text-align: right; }
  .small { font-size: 12px; }
  select { background: var(--panel2); color: var(--text); border: 1px solid var(--line); border-radius: 6px; padding: 4px 6px; }
</style>
