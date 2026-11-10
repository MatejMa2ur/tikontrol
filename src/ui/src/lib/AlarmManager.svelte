<script lang="ts">
  // Alarm Manager wired to the tikontrol server (/api/v1/alerts). Used by Events.svelte when a
  // backend is present; the mockup keeps its own fake state. All text is interpolated (escaped).
  import { api, session } from './backend'
  import Section from './ui/Section.svelte'
  import Field from './ui/Field.svelte'
  import Toggle from './ui/Toggle.svelte'
  import Modal from './ui/Modal.svelte'

  type Sev = 'info' | 'warning' | 'critical'
  type Security = 'tls' | 'starttls' | 'none'
  type Rule = { enabled: boolean; severity: Sev; threshold?: number }
  type Quiet = { enabled: boolean; start: number; end: number; offsetMinutes: number; criticalBypass: boolean }
  type Alert = { id: number; t: number; rule: string; severity: Sev; text: string; detail?: string; ack: boolean; ackBy?: string; cleared?: number; quiet: boolean; sent: string[]; failed: string[] }
  type ChannelsView = {
    ntfy: { enabled: boolean; minSeverity: Sev; server: string; topic: string; token: { set: boolean } }
    webhook: { enabled: boolean; minSeverity: Sev; url: { set: boolean; host: string } }
    telegram: { enabled: boolean; minSeverity: Sev; chatId: string; token: { set: boolean } }
    email?: { enabled: boolean; minSeverity: Sev; host: string; port: number; security: Security; username: string; password: { set: boolean }; from: string; to: string[] }
    thirdParty: string
  }
  type View = { rules: Record<string, Rule>; quiet: Quiet; channels: ChannelsView; history: Alert[]; unacked: number }

  let { view, unacked = $bindable(0) }: { view: 'alarms' | 'channels'; unacked?: number } = $props()

  const admin = session.me?.role === 'admin'
  const sevs: Sev[] = ['info', 'warning', 'critical']
  const labels: Record<string, string> = {
    wanDown: 'Internet goes down',
    unreachable: 'Router unreachable or poll error',
    newClient: 'New client joins the network',
    cpuHigh: 'CPU load above threshold (5-minute average)',
    memoryHigh: 'Memory use above threshold',
    updateAvailable: 'RouterOS update available',
    insecureService: 'Insecure service enabled (telnet, ftp, www)',
  }
  const order = Object.keys(labels)
  const hours = Array.from({ length: 24 }, (_, h) => h)
  const pad = (h: number) => String(h).padStart(2, '0') + ':00'
  const offset = -new Date().getTimezoneOffset()
  const tz = `UTC${offset >= 0 ? '+' : '-'}${pad(Math.floor(Math.abs(offset) / 60)).slice(0, 2)}:${String(Math.abs(offset) % 60).padStart(2, '0')}`
  const when = (t: number) => new Date(t * 1000).toLocaleString()
  const tone = (s: Sev) => (s === 'critical' ? 'bad' : s === 'warning' ? 'warn' : '')

  let data = $state<View | null>(null)
  let err = $state('')
  let msg = $state('')
  let busy = $state(false)
  let rules = $state<Record<string, Rule>>({})
  let quiet = $state<Quiet>({ enabled: false, start: 23, end: 7, offsetMinutes: offset, criticalBypass: true })
  // channel form; secret inputs stay empty and mean "keep the stored one"
  let ntfy = $state({ enabled: false, minSeverity: 'warning' as Sev, server: '', topic: '', token: '', clearToken: false })
  let webhook = $state({ enabled: false, minSeverity: 'warning' as Sev, url: '', clearUrl: false })
  let telegram = $state({ enabled: false, minSeverity: 'warning' as Sev, chatId: '', token: '', clearToken: false })
  let email = $state({ enabled: false, minSeverity: 'warning' as Sev, host: '', port: 587, security: 'starttls' as Security, username: '', password: '', clearPassword: false, from: '', to: '' })
  const ports: Record<Security, number> = { tls: 465, starttls: 587, none: 25 }
  let confirm = $state<string[] | null>(null)

  const active = $derived((data?.history ?? []).filter((a) => !a.ack))

  function fill(v: View) {
    data = v
    unacked = v.unacked
    rules = JSON.parse(JSON.stringify(v.rules))
    quiet = { ...v.quiet }
    fillChannels(v.channels)
  }
  function fillChannels(c: ChannelsView) {
    ntfy = { enabled: c.ntfy.enabled, minSeverity: c.ntfy.minSeverity, server: c.ntfy.server, topic: c.ntfy.topic, token: '', clearToken: false }
    webhook = { enabled: c.webhook.enabled, minSeverity: c.webhook.minSeverity, url: '', clearUrl: false }
    telegram = { enabled: c.telegram.enabled, minSeverity: c.telegram.minSeverity, chatId: c.telegram.chatId, token: '', clearToken: false }
    const e = c.email
    if (e) email = { enabled: e.enabled, minSeverity: e.minSeverity, host: e.host, port: e.port, security: e.security, username: e.username, password: '', clearPassword: false, from: e.from, to: e.to.join(', ') }
  }

  async function run(f: () => Promise<void>) {
    busy = true
    err = ''
    msg = ''
    try { await f() } catch (e) { err = e instanceof Error ? e.message : String(e) } finally { busy = false }
  }
  const load = () => run(async () => fill(await api<View>('/alerts')))
  load()

  const ack = (id: number) => run(async () => { await api('/alerts/ack', { method: 'POST', body: { id } }); fill(await api<View>('/alerts')) })
  const saveRules = () => run(async () => {
    await api('/alerts/rules', { method: 'PUT', body: { rules, quiet: { ...quiet, offsetMinutes: offset } } })
    fill(await api<View>('/alerts'))
    msg = 'Rules saved.'
  })

  // secrets: omitted = keep, '' = remove, value = replace
  const secret = (v: string, clear: boolean) => (clear ? '' : v.trim() || undefined)
  function newlyEnabled(): string[] {
    const c = data?.channels
    if (!c) return []
    return [
      ntfy.enabled && !c.ntfy.enabled ? 'ntfy' : '',
      webhook.enabled && !c.webhook.enabled ? 'webhook' : '',
      telegram.enabled && !c.telegram.enabled ? 'Telegram' : '',
      email.enabled && !c.email?.enabled ? 'e-mail' : '',
    ].filter(Boolean)
  }
  function askSave() {
    const n = newlyEnabled()
    if (n.length) confirm = n
    else saveChannels()
  }
  const saveChannels = () => run(async () => {
    confirm = null
    const c = await api<ChannelsView>('/alerts/channels', {
      method: 'PUT',
      body: {
        ntfy: { enabled: ntfy.enabled, minSeverity: ntfy.minSeverity, server: ntfy.server.trim(), topic: ntfy.topic.trim(), token: secret(ntfy.token, ntfy.clearToken) },
        webhook: { enabled: webhook.enabled, minSeverity: webhook.minSeverity, url: secret(webhook.url, webhook.clearUrl) },
        telegram: { enabled: telegram.enabled, minSeverity: telegram.minSeverity, chatId: telegram.chatId.trim(), token: secret(telegram.token, telegram.clearToken) },
        email: {
          enabled: email.enabled, minSeverity: email.minSeverity, host: email.host.trim(), port: Number(email.port), security: email.security,
          username: email.username.trim(), password: email.security === 'none' ? (data?.channels.email?.password.set ? '' : undefined) : secret(email.password, email.clearPassword),
          from: email.from.trim(), to: email.to.split(/[,;\s]+/).filter(Boolean),
        },
      },
    })
    if (data) data.channels = c
    fillChannels(c)
    msg = 'Channels saved.'
  })
  const test = (channel: string) => run(async () => {
    const r = await api<{ status: number }>('/alerts/test', { method: 'POST', body: { channel } })
    msg = channel === 'email' ? 'Test e-mail accepted by the mail server.' : `Test sent via ${channel} (HTTP ${r.status}).`
  })
</script>

{#if err}<p class="err" role="alert">{err}</p>{/if}
{#if msg}<p class="okmsg" role="status">{msg}</p>{/if}

{#if !data}
  <Section title="Alarm Manager"><div class="muted small">{busy ? 'Loading…' : 'Could not load alerts.'}</div></Section>
{:else if view === 'alarms'}
  <Section title="Active alarms" desc="Alerts that fired and haven't been acknowledged.">
    {#each active as a (a.id)}
      <div class="row between alarm">
        <span class="row">
          <span class="dot {tone(a.severity)}"></span>
          <span><b>{a.text}</b>{#if a.detail}<span class="muted small"> · {a.detail}</span>{/if}<br />
            <span class="muted small">{when(a.t)}{a.cleared ? ` · cleared ${when(a.cleared)}` : ' · still active'}{a.quiet ? ' · held by quiet hours' : ''}{a.sent.length ? ` · sent via ${a.sent.join(', ')}` : ''}</span>
            {#if a.failed.length}<br /><span class="small warntext">Not delivered: {a.failed.join('; ')}</span>{/if}
          </span>
        </span>
        <button class="btn" disabled={busy} onclick={() => ack(a.id)}>Acknowledge</button>
      </div>
    {:else}<div class="muted small">All clear.</div>{/each}
  </Section>

  <Section title="Alarm rules" desc="Checked every 30 seconds on live router data. An alert fires once when its condition starts and again only after it has cleared.">
    <div class="scroll">
      <table>
        <thead><tr><th>On</th><th>When</th><th>Threshold</th><th>Severity</th></tr></thead>
        <tbody>
          {#each order as id (id)}
            {#if rules[id]}
              <tr class="nohover">
                <td><Toggle bind:checked={rules[id].enabled} label={labels[id]} disabled={!admin} /></td>
                <td>{labels[id]}</td>
                <td>{#if rules[id].threshold !== undefined}<label class="row th"><input type="number" min="1" max="100" bind:value={rules[id].threshold} disabled={!admin} aria-label="{labels[id]} threshold in percent" /> %</label>{:else}<span class="muted">–</span>{/if}</td>
                <td><select bind:value={rules[id].severity} disabled={!admin} aria-label="{labels[id]} severity">{#each sevs as s}<option value={s}>{s}</option>{/each}</select></td>
              </tr>
            {/if}
          {/each}
        </tbody>
      </table>
    </div>
  </Section>

  <Section title="Quiet hours" desc="Alerts are still recorded; notifications are held back. Times are in your browser's time zone ({tz}).">
    <Field label="Enabled"><Toggle bind:checked={quiet.enabled} label="Quiet hours" disabled={!admin} /></Field>
    <Field label="From / to">
      <select bind:value={quiet.start} disabled={!admin} aria-label="Quiet hours start">{#each hours as h}<option value={h}>{pad(h)}</option>{/each}</select>
      <select bind:value={quiet.end} disabled={!admin} aria-label="Quiet hours end">{#each hours as h}<option value={h}>{pad(h)}</option>{/each}</select>
    </Field>
    <Field label="Critical alerts" help="Still notify during quiet hours"><Toggle bind:checked={quiet.criticalBypass} label="Critical alerts bypass quiet hours" disabled={!admin} /></Field>
    {#if admin}<div class="r"><button class="btn primary" disabled={busy} onclick={saveRules}>Save rules</button></div>{:else}<p class="muted small">Only an admin can change rules.</p>{/if}
  </Section>

  <Section title="History" desc="The last 500 alerts, newest first.">
    <div class="scroll">
      <table>
        <thead><tr><th>Time</th><th>Severity</th><th>Alert</th><th>Status</th></tr></thead>
        <tbody>
          {#each data.history as a (a.id)}
            <tr class="nohover"><td class="mono muted">{when(a.t)}</td><td><span class="pill {tone(a.severity)}">{a.severity}</span></td><td class="wraptext">{a.text}</td><td class="muted small">{a.ack ? `acknowledged${a.ackBy ? ' by ' + a.ackBy : ''}` : 'open'}</td></tr>
          {:else}<tr><td colspan="4" class="muted">No alerts yet.</td></tr>{/each}
        </tbody>
      </table>
    </div>
  </Section>
{:else}
  <Section title="Notification channels" desc="All channels are off by default. Secrets are encrypted in tikontrol and never shown again.">
    <p class="third" role="note"><b>Sends data to a third party.</b> {data.channels.thirdParty} Nothing is sent until you enable a channel.</p>

    <h4>Push (ntfy)</h4>
    <Field label="Enabled" help="Sends to the ntfy server below (a third party unless you host it)"><Toggle bind:checked={ntfy.enabled} label="ntfy enabled" disabled={!admin} /></Field>
    <Field label="Server" help="https://, or http:// to a local address"><input type="text" bind:value={ntfy.server} placeholder="https://ntfy.example.com" disabled={!admin} aria-label="ntfy server" /></Field>
    <Field label="Topic"><input type="text" bind:value={ntfy.topic} placeholder="my-home-alerts" disabled={!admin} aria-label="ntfy topic" /></Field>
    <Field label="Access token" help={data.channels.ntfy.token.set ? 'A token is stored. Leave empty to keep it; changing the server removes it.' : 'Optional'}>
      <input type="password" bind:value={ntfy.token} autocomplete="off" disabled={!admin || ntfy.clearToken} aria-label="ntfy access token" />
      {#if data.channels.ntfy.token.set}<label class="chk"><input type="checkbox" bind:checked={ntfy.clearToken} disabled={!admin} /> remove</label>{/if}
    </Field>
    <Field label="Minimum severity"><select bind:value={ntfy.minSeverity} disabled={!admin} aria-label="ntfy minimum severity">{#each sevs as s}<option value={s}>{s}</option>{/each}</select></Field>

    <h4>Webhook</h4>
    <Field label="Enabled" help="POSTs JSON to the URL (works with Home Assistant)"><Toggle bind:checked={webhook.enabled} label="Webhook enabled" disabled={!admin} /></Field>
    <Field label="URL" help={data.channels.webhook.url.set ? `Stored for ${data.channels.webhook.url.host}. Leave empty to keep it.` : 'https://, or http:// to a local address'}>
      <input type="password" bind:value={webhook.url} autocomplete="off" placeholder="https://…" disabled={!admin || webhook.clearUrl} aria-label="Webhook URL" />
      {#if data.channels.webhook.url.set}<label class="chk"><input type="checkbox" bind:checked={webhook.clearUrl} disabled={!admin} /> remove</label>{/if}
    </Field>
    <Field label="Minimum severity"><select bind:value={webhook.minSeverity} disabled={!admin} aria-label="Webhook minimum severity">{#each sevs as s}<option value={s}>{s}</option>{/each}</select></Field>

    <h4>Telegram</h4>
    <Field label="Enabled" help="Sends to Telegram (api.telegram.org)"><Toggle bind:checked={telegram.enabled} label="Telegram enabled" disabled={!admin} /></Field>
    <Field label="Bot token" help={data.channels.telegram.token.set ? 'A token is stored. Leave empty to keep it.' : 'From @BotFather: 123456:ABC…'}>
      <input type="password" bind:value={telegram.token} autocomplete="off" disabled={!admin || telegram.clearToken} aria-label="Telegram bot token" />
      {#if data.channels.telegram.token.set}<label class="chk"><input type="checkbox" bind:checked={telegram.clearToken} disabled={!admin} /> remove</label>{/if}
    </Field>
    <Field label="Chat ID"><input type="text" bind:value={telegram.chatId} placeholder="-1001234567890" disabled={!admin} aria-label="Telegram chat id" /></Field>
    <Field label="Minimum severity"><select bind:value={telegram.minSeverity} disabled={!admin} aria-label="Telegram minimum severity">{#each sevs as s}<option value={s}>{s}</option>{/each}</select></Field>

    <h4>E-mail (SMTP)</h4>
    <Field label="Enabled" help="Sends through your mail server, with TLS and a verified certificate"><Toggle bind:checked={email.enabled} label="E-mail enabled" disabled={!admin} /></Field>
    <Field label="Server" help="Host name of your mail provider's SMTP server"><input type="text" bind:value={email.host} placeholder="smtp.example.com" disabled={!admin} aria-label="SMTP server" /></Field>
    <Field label="Encryption" help={email.security === 'none' ? 'Only to a mail relay on your own network, and never with a password' : ''}>
      <select bind:value={email.security} onchange={() => (email.port = ports[email.security])} disabled={!admin} aria-label="SMTP encryption">
        <option value="starttls">STARTTLS (port 587)</option>
        <option value="tls">TLS (port 465)</option>
        <option value="none">None (local relay only)</option>
      </select>
    </Field>
    <Field label="Port"><input type="number" min="1" max="65535" bind:value={email.port} disabled={!admin} aria-label="SMTP port" /></Field>
    {#if email.security !== 'none'}
      <Field label="User name" help="Optional"><input type="text" bind:value={email.username} autocomplete="off" disabled={!admin} aria-label="SMTP user name" /></Field>
      <Field label="Password" help={data.channels.email?.password.set ? 'A password is stored. Leave empty to keep it; changing the server, port or user name removes it.' : 'An app password is best'}>
        <input type="password" bind:value={email.password} autocomplete="new-password" disabled={!admin || email.clearPassword} aria-label="SMTP password" />
        {#if data.channels.email?.password.set}<label class="chk"><input type="checkbox" bind:checked={email.clearPassword} disabled={!admin} /> remove</label>{/if}
      </Field>
    {/if}
    <Field label="From"><input type="email" bind:value={email.from} placeholder="router@example.com" disabled={!admin} aria-label="Sender address" /></Field>
    <Field label="To" help="Up to 10 addresses, separated by commas"><input type="text" bind:value={email.to} placeholder="me@example.com" disabled={!admin} aria-label="Recipient addresses" /></Field>
    <Field label="Minimum severity"><select bind:value={email.minSeverity} disabled={!admin} aria-label="E-mail minimum severity">{#each sevs as s}<option value={s}>{s}</option>{/each}</select></Field>

    {#if admin}
      <div class="r row end">
        <button class="btn" disabled={busy} onclick={() => test('ntfy')}>Test ntfy</button>
        <button class="btn" disabled={busy} onclick={() => test('webhook')}>Test webhook</button>
        <button class="btn" disabled={busy} onclick={() => test('telegram')}>Test Telegram</button>
        <button class="btn" disabled={busy} onclick={() => test('email')}>Test e-mail</button>
        <button class="btn primary" disabled={busy} onclick={askSave}>Save</button>
      </div>
      <p class="muted small">Tests use the saved settings, even for a channel that is switched off.</p>
    {:else}<p class="muted small">Only an admin can change channels.</p>{/if}
  </Section>
{/if}

{#if confirm}
  <Modal title="Send alerts to a third party?" onclose={() => (confirm = null)}>
    <p>Enabling {confirm.join(', ')} sends every matching alert text to that service. Alert texts can include client names, MAC addresses and RouterOS versions.</p>
    {#snippet footer()}
      <button class="btn" onclick={() => (confirm = null)}>Cancel</button>
      <button class="btn primary" onclick={saveChannels}>Enable and save</button>
    {/snippet}
  </Modal>
{/if}

<style>
  .alarm { padding: 8px 0; border-top: 1px solid var(--line); gap: 10px; }
  .alarm:first-of-type { border-top: 0; }
  .scroll { overflow-x: auto; }
  .wraptext { white-space: normal; }
  .th input { width: 70px; }
  .r { text-align: right; margin-top: 10px; }
  .end { justify-content: flex-end; flex-wrap: wrap; }
  .chk { display: inline-flex; gap: 4px; align-items: center; font-size: 12px; color: var(--muted); }
  .err { color: var(--bad); margin: 0; }
  .okmsg { color: var(--ok); margin: 0; }
  .warntext { color: var(--warn); }
  .third { border: 1px solid var(--warn); border-radius: 8px; padding: 8px 12px; margin: 0 0 8px; }
  h4 { margin: 16px 0 2px; font-size: 13px; }
  select { background: var(--panel2); color: var(--text); border: 1px solid var(--line); border-radius: 6px; padding: 4px 6px; }
</style>
