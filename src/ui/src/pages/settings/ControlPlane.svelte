<script lang="ts">
  import { raw, router, isBackend } from '../../lib/data'
  import { review } from '../../lib/apply.svelte'
  import { api, session, site, switchSite, type Site } from '../../lib/backend'
  import Icon from '../../lib/Icon.svelte'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'
  import Toggle from '../../lib/ui/Toggle.svelte'
  import { cmr, loadCmr } from '../../lib/cmr.svelte'

  const users: any[] = raw.users ?? []
  // full backups plus tikontrol's own text exports (tik-*.rsc, listed by RouterOS as type "script")
  // update window: tikontrol's own scheduler job (intents/system.rs auto-update)
  const updJob: any = ((raw.scheduler ?? []) as any[]).find((j) => j.name === 'tik-auto-update')
  let win = $state({ day: 'daily', hour: 3, install: false })
  const dayNames: Record<string, string> = { daily: 'Every day', mon: 'Monday', tue: 'Tuesday', wed: 'Wednesday', thu: 'Thursday', fri: 'Friday', sat: 'Saturday', sun: 'Sunday' }
  const backups: any[] = [...(raw.backups ?? []), ...((raw.exports ?? []) as any[]).filter((f) => /(^|\/)tik-[\w-]+\.rsc$/.test(f.name ?? ''))]
  // exports carry no passwords or keys, so they may be downloaded; .backup files stay on the router
  const exportUrl = (name: string) => `/api/v1/tools/export?name=${encodeURIComponent(name)}${site ? `&site=${encodeURIComponent(site)}` : ''}`
  // SSH public keys (intents/users.rs): fingerprints only; tikontrol removes only keys it tagged
  const sshKeys: any[] = raw.sshKeys ?? []
  const tikKey = (k: any) => { const m = /^tik:ssh-key:[^:]+:(.+)$/.exec(k.comment ?? ''); return m ? { label: m[1] } : null }
  let nk = $state({ user: users[0]?.name ?? '', label: '', key: '' })
  const addKey = () => review(`Add SSH key ${nk.label} for ${nk.user}`, [`/user/ssh-keys/add user=${nk.user} key="${nk.key.trim().split(/\s+/).slice(0, 2).join(' ')}" comment=tik:ssh-key:${nk.user}:${nk.label}`], { kind: 'ssh-key', user: nk.user, label: nk.label, key: nk.key })
  const isAdmin = !isBackend || session.me?.role === 'admin'

  // tikontrol's own users (backend) — the RouterOS users below are shown read-only
  type Admin = { name: string; role: 'admin' | 'viewer'; you: boolean }
  let admins = $state<Admin[]>([])
  let nu = $state({ name: '', password: '', role: 'viewer' as 'admin' | 'viewer' })
  let pw = $state({ current: '', next: '' })
  let msg = $state('')
  let err = $state('')
  async function loadAdmins() {
    if (!isBackend) return
    try { admins = await api<Admin[]>('/admins') } catch (e) { err = (e as Error).message }
  }
  async function act(f: () => Promise<unknown>, ok: string) {
    err = ''; msg = ''
    try { await f(); msg = ok; await loadAdmins() } catch (e) { err = (e as Error).message }
  }
  const addAdmin = () => act(async () => { await api('/admins', { method: 'POST', body: { name: nu.name, password: nu.password, role: nu.role } }); nu = { name: '', password: '', role: 'viewer' } }, 'User added.')
  const removeAdmin = (name: string) => confirm(`Remove ${name}? Their sessions end now.`) && act(() => api(`/admins?name=${encodeURIComponent(name)}`, { method: 'DELETE' }), `${name} removed.`)
  async function changePassword() {
    err = ''; msg = ''
    try {
      const r = await api<{ csrf: string }>('/admins/password', { method: 'POST', body: { current: pw.current, new: pw.next } })
      session.csrf = r.csrf
      pw = { current: '', next: '' }
      msg = 'Password changed. Your other sessions were signed out.'
    } catch (e) { err = (e as Error).message }
  }
  loadAdmins()

  // optional CMR controller (src/server/src/cmr.rs): a second RouterOS running the cmr package
  $effect(() => { loadCmr() })
  let ctl = $state({ host: '', port: 8729, pin: '', ruser: '', rpass: '', wuser: '', wpass: '', confirm: false })
  let ctlMsg = $state('')
  async function ctlRun(f: () => Promise<unknown>, ok: string) {
    ctlMsg = ''
    try { await f(); ctlMsg = ok; cmr.data = null; loadCmr() } catch (e) { ctlMsg = (e as Error).message }
  }
  const ctlProbe = () => ctlRun(async () => { ctl.pin = (await api<{ pin: string }>('/router/probe', { method: 'POST', body: { host: ctl.host, port: ctl.port } })).pin }, 'Certificate read: compare the fingerprint with the controller\'s before saving.')
  const ctlSave = () => ctlRun(() => api('/controller', { method: 'POST', body: { host: ctl.host, port: ctl.port, pin: ctl.pin, reader: { user: ctl.ruser, password: ctl.rpass }, ...(ctl.wuser ? { writer: { user: ctl.wuser, password: ctl.wpass }, confirm_write: ctl.confirm } : {}) } }), 'Controller saved.')
  const ctlRemove = () => ctlRun(() => api('/controller', { method: 'DELETE' }), 'Controller removed: CMR is read from the router again.')

  let channel = $state<string>(router.channel ?? 'stable')
  const today = () => new Date().toISOString().slice(0, 10)
  const ownFile = (n: string) => /^tik-[A-Za-z0-9_-]{1,40}\.(rsc|backup)$/.test(n)

  // ---------- AI assistants (MCP): src/server/src/mcp.rs ----------
  type McpCfg = { enabled: boolean; privacy: boolean; endpoint: string | null; readWrite: boolean; protocolVersion: string }
  type Proposal = { id: number; t: number; title: string; description: string; commands: string[]; intent?: Record<string, unknown> }
  let mcp = $state<McpCfg | null>(null)
  let token = $state('')
  let copied = $state(false)
  let proposals = $state<Proposal[]>([])
  let mcpErr = $state('')
  async function loadMcp() {
    try {
      mcp = await api<McpCfg>('/mcp')
      proposals = await api<Proposal[]>('/mcp/proposals')
    } catch (e) { mcpErr = (e as Error).message }
  }
  async function mcpPost(path: string, body: unknown = {}) {
    mcpErr = ''
    try {
      const r = await api<{ token?: string }>(path, { method: 'POST', body })
      if (r?.token) { token = r.token; copied = false }
    } catch (e) { mcpErr = (e as Error).message }
    await loadMcp()
  }
  const setEnabled = (on: boolean) => { if (!on) token = ''; mcpPost(on ? '/mcp/enable' : '/mcp/disable') }
  async function dismiss(id: number) {
    try { await api(`/mcp/proposals?id=${id}`, { method: 'DELETE' }) } catch (e) { mcpErr = (e as Error).message }
    await loadMcp()
  }
  async function copyToken() {
    try { await navigator.clipboard.writeText(token); copied = true } catch { mcpErr = 'Copy failed: select the token and copy it by hand.' }
  }
  const snippet = $derived(JSON.stringify({ mcpServers: { tikontrol: { type: 'http', url: mcp?.endpoint ?? 'https://tikontrol.lan:8443/mcp', headers: { Authorization: `Bearer ${token || '<your MCP token>'}` } } } }, null, 2))
  if (isBackend) loadMcp()

  // ---------- API keys: src/server/src/apikeys.rs (read-only, hashed, shown once) ----------
  let keys = $state<{ name: string; created: number }[]>([])
  let keyName = $state('')
  let newKey = $state('')
  let keyErr = $state('')
  async function loadKeys() { if (isBackend && isAdmin) try { keys = await api('/apikeys') } catch (e) { keyErr = (e as Error).message } }
  async function keyAct(f: () => Promise<unknown>) { keyErr = ''; try { await f() } catch (e) { keyErr = (e as Error).message } await loadKeys() }
  const createKey = () => keyAct(async () => { newKey = (await api<{ key: string }>('/apikeys', { method: 'POST', body: { name: keyName } })).key; keyName = '' })
  const revokeKey = (n: string) => confirm(`Revoke ${n}? Scripts using it stop working.`) && keyAct(() => api(`/apikeys?name=${encodeURIComponent(n)}`, { method: 'DELETE' }))
  loadKeys()

  // ---------- Routers (sites): src/server/src/sites.rs. Each has its own credentials, data and history ----------
  let sites = $state<Site[]>([])
  let siteName = $state('')
  let siteErr = $state('')
  async function loadSites() { if (isBackend) try { sites = await api<Site[]>('/sites') } catch (e) { siteErr = (e as Error).message } }
  async function addSite() {
    siteErr = ''
    try { const r = await api<{ id: string }>('/sites', { method: 'POST', body: { name: siteName } }); switchSite(r.id) } catch (e) { siteErr = (e as Error).message }
  }
  async function removeSite(s: Site) {
    if (!confirm(`Remove ${s.name}? tikontrol forgets its credentials and history. Nothing on the router changes.`)) return
    siteErr = ''
    try { await api(`/sites?id=${encodeURIComponent(s.id)}`, { method: 'DELETE' }); if (s.id === site) switchSite(''); else await loadSites() } catch (e) { siteErr = (e as Error).message }
  }
  loadSites()

  // ---------- Blueprint / drift: src/server/src/drift.rs ----------
  type Drift = { saved: number | null; changes: { menu: string; added: Record<string, unknown>[]; removed: Record<string, unknown>[] }[] }
  let drift = $state<Drift | null>(null)
  let driftErr = $state('')
  async function loadDrift() { if (isBackend) try { drift = await api<Drift>('/drift') } catch (e) { driftErr = (e as Error).message } }
  async function saveBlueprint() { driftErr = ''; try { await api('/drift/baseline', { method: 'POST', body: {} }); await loadDrift() } catch (e) { driftErr = (e as Error).message } }
  const fmtRow = (r: Record<string, unknown>) => Object.entries(r).map(([k, v]) => `${k}=${Array.isArray(v) ? v.join(',') : String(v)}`).join(' ')
  loadDrift()

  // ---------- Console: read-only, vetted prints only (src/server/src/ros/queries.rs `console`) ----------
  let cmd = $state('')
  let allowed = $state<string[]>([])
  let con = $state<{ command: string; rows: Record<string, string>[] } | null>(null)
  let conErr = $state('')
  let conBusy = $state(false)
  if (isBackend) api<string[]>('/tools/console').then((a) => (allowed = a)).catch(() => {})
  async function runCmd() {
    conErr = ''; con = null; conBusy = true
    try { con = await api('/tools/console', { method: 'POST', body: { command: cmd } }) } catch (e) { conErr = (e as Error).message } finally { conBusy = false }
  }
  const conCols = $derived(con ? [...new Set(con.rows.flatMap((r) => Object.keys(r)))] : [])

  // tikontrol's own mode: read-only (default) or read & write. Read & write stores nothing new:
  // changes run as the signed-in MikroTik admin and go through Safe apply.
  let rw = $state(session.me?.router?.mode === 'read-write')
  let rwConfirm = $state(false)
  let modeMsg = $state('')
  async function setMode(write: boolean) {
    modeMsg = ''
    try {
      await api('/router/mode', { method: 'PUT', body: write ? { mode: 'read-write', confirm_write: true } : { mode: 'read-only' } })
      rw = write; rwConfirm = false
      location.reload() // readOnly in /me changes everywhere
    } catch (e) { modeMsg = (e as Error).message }
  }
</script>

{#if isBackend && session.me?.router}
  <Section title="Changes from tikontrol" desc="Read-only shows everything and changes nothing. Read & write lets admins change the router from tikontrol.">
    <p class="small">tikontrol is <b>{rw ? 'read & write' : 'read-only'}</b>.
      {#if rw}Changes run as the MikroTik admin who is signed in (nothing extra is stored) and every change goes through Safe apply: you review it, and it rolls back by itself if the router stops answering.{/if}</p>
    {#if !isAdmin}<p class="muted small">Only an admin can change this.</p>
    {:else if rw}<button class="btn" onclick={() => setMode(false)}>Switch to read-only</button>
    {:else}
      <label class="small"><input type="checkbox" bind:checked={rwConfirm} /> I understand tikontrol can then change this router's configuration</label>
      <div><button class="btn primary" disabled={!rwConfirm} onclick={() => setMode(true)}>Allow changes</button></div>
    {/if}
    {#if modeMsg}<p class="err" role="alert">{modeMsg}</p>{/if}
  </Section>
{/if}

{#if isBackend}
  <Section title="Routers" desc="Other routers this tikontrol manages, each with its own read-only (and optional writer) user. Switch between them in the top bar.">
    <table><thead><tr><th>Name</th><th>Router</th><th>Address</th><th>Mode</th><th></th></tr></thead>
      <tbody>{#each sites as s (s.id)}<tr class="nohover"><td><b>{s.name}</b>{#if s.id === site} <span class="muted small">(shown)</span>{/if}</td><td>{s.identity ?? '—'}</td><td class="mono small">{s.host ?? 'not connected'}</td>
        <td>{#if s.error}<span class="pill bad" title={s.error}>error</span>{:else}<span class="pill">{s.mode ?? '—'}</span>{/if}</td>
        <td class="r">{#if s.id !== site}<button class="btn small" onclick={() => switchSite(s.id)}>Open</button>{/if}
          {#if isAdmin && s.id}<button class="btn small" onclick={() => removeSite(s)}>Remove</button>{/if}</td></tr>{/each}</tbody></table>
    {#if isAdmin}
      <form class="add" onsubmit={(e) => { e.preventDefault(); addSite() }}><input type="text" bind:value={siteName} maxlength="40" placeholder="Name, e.g. Office" aria-label="New router name" autocomplete="off" />
        <button class="btn primary" disabled={!siteName.trim()}><Icon name="plus" size={14} /> Add router</button></form>
      <p class="muted small">Adding opens the connect page for the new router. Your MikroTik sign-in only works on the main router: give other routers a stored writer user to make changes there.</p>
    {/if}
    {#if siteErr}<div class="err small" role="alert">{siteErr}</div>{/if}
  </Section>

  <Section title="CMR controller" desc="MikroTik CMR (RouterOS 7.26+ cmr package) manages other MikroTik devices: adoption, labels, Wi-Fi, upgrades, alerts, app traffic. If it runs on this router, nothing to set. If it runs elsewhere (a CHR VM, a bigger router), connect it here with its own users: a reader in a group with read,api,test only, and optionally a writer with read,write,api,test,reboot (pairing and upgrades need reboot).">
    {#if cmr.data?.controller}
      <p class="small">Connected to <b class="mono">{cmr.data.controller.host}:{cmr.data.controller.port}</b> as {cmr.data.controller.reader}{cmr.data.controller.writer ? `, changes as ${cmr.data.controller.writer}` : ' (read-only)'}.
        {#if cmr.data.available === false}<span class="err">{cmr.data.error}</span>{:else}{cmr.data.devices.length} devices.{/if}</p>
      {#if isAdmin}<button class="btn" onclick={ctlRemove}>Remove controller</button>{/if}
    {:else}
      <p class="small">{cmr.data?.available === false ? 'This router has no CMR server.' : cmr.data ? `This router is the CMR server (${cmr.data.devices.length} devices).` : ''}</p>
      {#if isAdmin}
        <Field label="Controller address"><input type="text" bind:value={ctl.host} placeholder="192.168.88.5" /><input type="number" bind:value={ctl.port} aria-label="API-SSL port" /><button class="btn small" disabled={!ctl.host} onclick={ctlProbe}>Read certificate</button></Field>
        {#if ctl.pin}<Field label="Certificate SHA-256"><span class="mono small">{ctl.pin}</span></Field>{/if}
        <Field label="Reader" help="read,api,test only: tikontrol refuses more"><input type="text" bind:value={ctl.ruser} placeholder="user" autocomplete="off" /><input type="password" bind:value={ctl.rpass} placeholder="password" autocomplete="off" /></Field>
        <Field label="Writer (optional)" help="For adoption, labels, upgrade rules and alerts through Safe apply"><input type="text" bind:value={ctl.wuser} placeholder="user" autocomplete="off" /><input type="password" bind:value={ctl.wpass} placeholder="password" autocomplete="off" /></Field>
        {#if ctl.wuser}<label class="small"><input type="checkbox" bind:checked={ctl.confirm} /> I understand the writer can change every device the controller manages</label>{/if}
        <div class="r"><button class="btn primary" disabled={!ctl.pin || !ctl.ruser || !ctl.rpass || (!!ctl.wuser && !ctl.confirm)} onclick={ctlSave}>Connect controller</button></div>
      {/if}
    {/if}
    {#if ctlMsg}<p class="small" role="status">{ctlMsg}</p>{/if}
  </Section>

  <Section title="tikontrol users" desc="Who can sign in to tikontrol. Viewers see everything but change nothing.">
    <table><thead><tr><th>Name</th><th>Role</th><th></th></tr></thead>
      <tbody>{#each admins as a}<tr class="nohover"><td><b>{a.name}</b>{#if a.you} <span class="muted small">(you)</span>{/if}</td><td><span class="pill">{a.role}</span></td>
        <td class="r">{#if isAdmin && !a.you}<button class="btn small" onclick={() => removeAdmin(a.name)}>Remove</button>{/if}</td></tr>{/each}</tbody></table>
    {#if isAdmin}
      <div class="add"><input type="text" bind:value={nu.name} placeholder="Username" aria-label="New username" autocomplete="off" /><input type="password" bind:value={nu.password} placeholder="Password (12+ characters)" aria-label="New user's password" autocomplete="new-password" />
        <select bind:value={nu.role} aria-label="Role"><option value="viewer">viewer</option><option value="admin">admin</option></select>
        <button class="btn primary" disabled={!nu.name || nu.password.length < 12} onclick={addAdmin}><Icon name="plus" size={14} /> Add</button></div>
    {/if}
    <div class="add"><input type="password" bind:value={pw.current} placeholder="Current password" aria-label="Current password" autocomplete="current-password" /><input type="password" bind:value={pw.next} placeholder="New password" aria-label="New password" autocomplete="new-password" />
      <button class="btn" disabled={!pw.current || pw.next.length < 12} onclick={changePassword}>Change my password</button></div>
    {#if msg}<div class="ok small" role="status">{msg}</div>{/if}
    {#if err}<div class="err small" role="alert">{err}</div>{/if}
  </Section>
{/if}

<Section title="Router users" desc="Who can log in to the router itself (shown read-only; manage them in WinBox)." ros="/user/print">
  <table><thead><tr><th>Name</th><th>Group</th><th>Last login</th><th></th></tr></thead>
    <tbody>{#each users as x}<tr class="nohover"><td><b>{x.name}</b> <span class="muted small">{x.comment ?? ''}</span></td><td><span class="pill">{x.group}</span></td><td>{x['last-logged-in'] ?? '—'}</td><td class="r">{#if x.name === 'admin'}<span class="muted small">Rename the default admin</span>{/if}</td></tr>{/each}</tbody></table>
  <Field label="Single sign-on" na="RouterOS has no SSO; use RADIUS for central logins"><span></span></Field>
  <h4>SSH keys</h4>
  <table><thead><tr><th>User</th><th>Key</th><th>Fingerprint</th><th></th></tr></thead>
    <tbody>{#each sshKeys as k}{@const own = tikKey(k)}<tr class="nohover"><td><b>{k.user}</b></td><td>{own ? own.label : (k.comment || k.info || '')} <span class="muted small">{k['key-type']} {k.bits ?? ''}</span></td><td class="mono small">{k.fingerprint ?? ''}</td>
      <td class="r">{#if own}<button class="btn small" onclick={() => review(`Remove SSH key ${own.label} from ${k.user}`, [`/user/ssh-keys/remove [find comment="${k.comment}"]`], { kind: 'remove-ssh-key', user: k.user, label: own.label })}>Remove</button>{:else}<span class="muted small" title="tikontrol only removes keys it added">not from tikontrol</span>{/if}</td></tr>
    {:else}<tr class="nohover"><td colspan="4" class="muted">No SSH keys.</td></tr>{/each}</tbody></table>
  <form class="add" onsubmit={(e) => { e.preventDefault(); addKey() }}>
    <select bind:value={nk.user} aria-label="Router user">{#each users as x}<option>{x.name}</option>{/each}</select>
    <input type="text" bind:value={nk.label} placeholder="Label, e.g. laptop" aria-label="Key label" maxlength="32" autocomplete="off" />
    <input type="text" class="mono" bind:value={nk.key} placeholder="ssh-ed25519 AAAA… (public key only)" aria-label="Public key" style="flex:1" autocomplete="off" spellcheck="false" />
    <button class="btn" disabled={!nk.user || !nk.label || !nk.key}><Icon name="key" size={14} /> Add key</button></form>
  <p class="muted small">Public keys only (ed25519, ECDSA, or RSA of 3072+ bits). By default a user with a key can no longer use a password over SSH. Needs a writer with the <code>policy</code> policy.</p>
</Section>

<Section title="Backups" desc="A readable export without passwords and keys, saved on the router; tikontrol can download it. Full backups hold passwords and keys, so they stay on the router (copy them with WinBox or WebFig Files)." ros="/file/print where type=backup">
  {#snippet actions()}<button class="btn primary" onclick={() => review('Back up now', [`/export file=tik-${today()}`], { kind: 'export-config', name: today() })}>Back up now</button>
    <button class="btn" title="Binary backup with passwords and keys, for restoring this same router" onclick={() => review('Full backup', [`/system/backup/save name=tik-${today()} dont-encrypt=yes`], { kind: 'save-backup', name: today() })}>Full backup</button>{/snippet}
  <table><thead><tr><th>File</th><th>Date</th><th class="r">Size</th><th></th></tr></thead>
    <tbody>{#each backups as b}<tr class="nohover"><td class="mono small">{b.name}</td><td>{b['last-modified']}</td><td class="r">{(b.size / 1024).toFixed(1)} KB</td>
      <td class="r">{#if b.name.endsWith('.backup')}<button class="btn small" onclick={() => review(`Restore ${b.name}`, [`/system/backup/load name=${b.name}`], { kind: 'restore-backup', name: b.name })}>Restore</button>
        {:else}{#if isBackend && isAdmin && /\.rsc$/.test(b.name)}<a class="btn small" href={exportUrl(b.name)} download>Download</a>{/if}
          <button class="btn small" title="A script export is restored by hand on a reset router: tikontrol won't import a whole configuration over a running one" onclick={() => review(`Restore ${b.name}`, [`# on a router reset without defaults:`, `/import file-name=${b.name}`])}>How to restore</button>{/if}
        {#if ownFile(b.name)}<button class="btn small" onclick={() => review(`Delete ${b.name}`, [`/file/remove [find name="${b.name}"]`], { kind: 'remove-backup-file', name: b.name })}>Delete</button>{/if}</td></tr>{/each}</tbody></table>
  <div class="row muted small sched">Automatic: <button class="btn small" onclick={() => review('Weekly backups', ['/system/scheduler/add name=tik-backup interval=7d on-event="/export file=tik-auto"'], { kind: 'backup-schedule', enabled: true, interval: '7d' })}>Weekly backups</button></div>
</Section>

{#if isBackend}
  <Section title="Blueprint & config drift" desc="Save this router's configuration as a blueprint, then see what changed since: rows added and removed per menu. Built from tikontrol's read-only view (no passwords or keys), not from /export.">
    {#snippet actions()}{#if isAdmin}<button class="btn" onclick={saveBlueprint}>Save blueprint</button>{/if}{/snippet}
    {#if drift?.saved}
      <p class="muted small">Blueprint from {new Date(drift.saved * 1000).toLocaleString()}.</p>
      {#each drift.changes as c}
        <h4>{c.menu}</h4>
        {#each c.added as r}<div class="mono small ok">+ {fmtRow(r)}</div>{/each}
        {#each c.removed as r}<div class="mono small err">− {fmtRow(r)}</div>{/each}
      {:else}<p class="small">No drift: the configuration matches the blueprint.</p>{/each}
    {:else}<p class="muted small">No blueprint saved yet.</p>{/if}
    {#if driftErr}<div class="err small" role="alert">{driftErr}</div>{/if}
  </Section>
{/if}

<Section title="Updates"desc="RouterOS version and channel. Picking a channel downloads nothing." ros="/system/package/update/print">
  <Field label="Version"><span>{router.version}</span></Field>
  <Field label="Channel"><select bind:value={channel} aria-label="Update channel"><option>stable</option><option>long-term</option><option>testing</option><option>development</option></select>
    <button class="btn" disabled={channel === router.channel} onclick={() => review(`Update channel: ${channel}`, [`/system/package/update/set channel=${channel}`], { kind: 'set-update-channel', channel })}>Review & apply</button></Field>
  <div class="row"><button class="btn" onclick={() => review('Check for updates', ['/system/package/update/check-for-updates'], { kind: 'check-updates' })}>Check for updates</button>
    {#if router.latest && router.latest !== router.version}<button class="btn primary" onclick={() => review(`Install RouterOS ${router.latest}`, ['/system/package/update/install'], { kind: 'install-update' })}>Install {router.latest}</button>{/if}
    <a class="btn" href="#/devices/updates">Manage updates…</a></div>
  <h4>Update window</h4>
  <p class="muted small">{updJob ? `On: next run ${updJob['next-run'] ?? updJob['start-time'] ?? ''} (every ${updJob.interval}).` : 'Off: RouterOS is only updated when you click Install.'} Uses the router's clock.</p>
  <div class="row wrap">
    <select bind:value={win.day} aria-label="Update window day">{#each Object.entries(dayNames) as [v, l]}<option value={v}>{l}</option>{/each}</select>
    <select bind:value={win.hour} aria-label="Update window hour">{#each Array.from({ length: 24 }, (_, h) => h) as h}<option value={h}>{String(h).padStart(2, '0')}:00</option>{/each}</select>
    <label class="chk"><input type="checkbox" bind:checked={win.install} /> Install updates too (restarts the router)</label>
    <button class="btn" onclick={() => review(win.install ? 'Install updates in a window' : 'Check for updates in a window', [`/system/scheduler/add name=tik-auto-update interval=${win.day === 'daily' ? '1d' : '7d'} on-event="/system/package/update/check-for-updates once…"`], { kind: 'auto-update', enabled: true, day: win.day, hour: win.hour, install: win.install })}>Review & apply</button>
    {#if updJob}<button class="btn" onclick={() => review('Stop the update window', ['/system/scheduler/remove [find name="tik-auto-update"]'], { kind: 'auto-update', enabled: false })}>Turn off</button>{/if}
  </div>
</Section>

<Section title="API keys" desc="For Home Assistant and scripts: read-only access to tikontrol's GET API with Authorization: Bearer <key>. Keys belong to tikontrol, not the router.">
  {#if !isBackend}<div class="muted small">Available when tikontrol runs with its server.</div>
  {:else if !isAdmin}<div class="muted small">Only an admin can manage API keys.</div>
  {:else}
    <table><thead><tr><th>Name</th><th>Created</th><th></th></tr></thead>
      <tbody>{#each keys as k}<tr class="nohover"><td><b>{k.name}</b></td><td>{new Date(k.created * 1000).toLocaleString()}</td><td class="r"><button class="btn small" onclick={() => revokeKey(k.name)}>Revoke</button></td></tr>
      {:else}<tr class="nohover"><td colspan="3" class="muted">No API keys.</td></tr>{/each}</tbody></table>
    <div class="add"><input type="text" bind:value={keyName} placeholder="Name, e.g. home-assistant" aria-label="API key name" autocomplete="off" /><button class="btn primary" disabled={!keyName} onclick={createKey}><Icon name="key" size={14} /> New key</button></div>
    {#if newKey}<div class="token" role="status"><div class="small"><b>Your API key.</b> Copy it now: it is shown only once and tikontrol keeps only its hash.</div><div class="row"><code class="mono">{newKey}</code></div></div>{/if}
  {/if}
  {#if keyErr}<div class="err small" role="alert">{keyErr}</div>{/if}
</Section>

<Section id="mcp" title="AI assistants (MCP)" desc="Lets an AI assistant on your network read clients, Wi-Fi, events, insights and topology. Off by default, local network only, with its own token.">
  {#if !isBackend}
    <div class="muted small">Available when tikontrol runs with its server.</div>
  {:else if mcp}
    <Field label="MCP server" help={isAdmin ? 'Turning it on creates a new token; turning it off revokes it.' : 'Only an admin can change this.'}>
      <Toggle bind:checked={() => mcp?.enabled ?? false, (v) => setEnabled(v)} disabled={!isAdmin} label="MCP server" />
      {#if mcp.enabled && isAdmin}<button class="btn small" onclick={() => mcpPost('/mcp/enable')}>New token</button>{/if}
    </Field>
    {#if token}
      <div class="token" role="status">
        <div class="small"><b>Your MCP token.</b> Copy it now: it is shown only once and tikontrol keeps only its hash.</div>
        <div class="row"><code class="mono">{token}</code><button class="btn small" onclick={copyToken}>{copied ? 'Copied' : 'Copy'}</button></div>
      </div>
    {/if}
    <Field label="Privacy mode" help="Swap device names, SSIDs, MAC and IP addresses for stable pseudonyms before data leaves tikontrol.">
      <Toggle bind:checked={() => mcp?.privacy ?? true, (v) => mcpPost('/mcp/privacy', { privacy: v })} disabled={!isAdmin} label="Privacy mode" />
    </Field>
    <Field label="Endpoint" help={`Streamable HTTP, MCP ${mcp.protocolVersion}. Answers loopback and private addresses only.`}><span class="mono">{mcp.endpoint ?? '/mcp'}</span></Field>
    <details>
      <summary class="small">Sample client config</summary>
      <pre class="mono cfg">{snippet}</pre>
      <p class="muted small">tikontrol uses its own certificate: the client must trust it.</p>
    </details>
    <h4>Proposed changes</h4>
    {#if !mcp.readWrite}
      <div class="muted small">Read-only mode: assistants can't propose changes.</div>
    {:else}
      {#each proposals as p (p.id)}
        <div class="prop">
          <div class="row between"><b>{p.title}</b><span class="row">
            {#if p.intent}<span class="pill ok" title="tikontrol compiled this change itself and checked it against the router">Checked</span>{:else}<span class="pill" title="Only text from the assistant: tikontrol can't apply it through Safe apply">Text only</span>{/if}
            <button class="btn small" onclick={() => review(p.title, p.commands, p.intent as any)}>Review…</button>
            {#if isAdmin}<button class="btn small" onclick={() => dismiss(p.id)}>Dismiss</button>{/if}
          </span></div>
          {#if p.description}<p class="muted small">{p.description}</p>{/if}
          <pre class="mono cfg">{p.commands.join('\n')}</pre>
        </div>
      {:else}
        <div class="muted small">No proposals. Nothing an assistant suggests runs until you review and apply it here.</div>
      {/each}
      <p class="muted small">Proposals come from the AI assistant: read every command before applying it. "Checked" proposals are compiled by tikontrol and go through Safe apply; "Text only" ones are never run by tikontrol.</p>
    {/if}
  {/if}
  {#if mcpErr}<div class="err small" role="alert">{mcpErr}</div>{/if}
</Section>

<Section title="Console" desc="Read-only: runs one print command as tikontrol's reader, with a fixed field list. Changes go through Safe apply, never through here.">
  {#if !isBackend}<div class="muted small">Available when tikontrol runs with its server.</div>
  {:else}
    <form class="add" onsubmit={(e) => { e.preventDefault(); runCmd() }}><span class="mono muted">[reader@{router.identity}] &gt;</span><input type="text" class="mono" bind:value={cmd} list="console-cmds" placeholder="/ip/route/print" style="flex:1" aria-label="RouterOS read command" />
      <datalist id="console-cmds">{#each allowed as a}<option value={a}></option>{/each}</datalist>
      <button class="btn" disabled={!cmd || conBusy}><Icon name="terminal" size={14} /> {conBusy ? 'Running…' : 'Run'}</button></form>
    {#if conErr}<div class="err small" role="alert">{conErr}</div>{/if}
    {#if con}<div class="mono muted small">{con.command}</div>
      {#if con.rows.length}<div class="scroll"><table><thead><tr>{#each conCols as c}<th>{c}</th>{/each}</tr></thead><tbody>{#each con.rows as row}<tr class="nohover">{#each conCols as c}<td class="mono small">{row[c] ?? ''}</td>{/each}</tr>{/each}</tbody></table></div>{:else}<div class="muted small">No rows.</div>{/if}
    {/if}
  {/if}
</Section>

<style>
  .add { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; margin-top: 10px; }
  .sched { gap: 8px; margin-top: 8px; }
  .r { text-align: right; }
  .small { font-size: 12px; }
  .btn.small { padding: 3px 9px; }
  .token { border: 1px solid var(--accent); border-radius: 8px; padding: 10px 12px; margin: 6px 0; }
  .token code { word-break: break-all; flex: 1; }
  .cfg { background: var(--code-bg); padding: 10px; border-radius: 6px; overflow-x: auto; white-space: pre; margin: 6px 0; }
  .prop { border-top: 1px solid var(--line); padding: 8px 0; }
  .prop p { margin: 4px 0; }
  h4 { margin: 14px 0 6px; font-size: 12.5px; }
  details { margin: 8px 0; }
  .err { color: var(--bad); margin-top: 6px; }
  .row { gap: 10px; padding: 3px 0; }
  .ok { color: var(--ok); margin-top: 8px; }
  .scroll { overflow-x: auto; max-height: 420px; }
</style>
