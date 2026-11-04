<script lang="ts">
  import { raw, router, isBackend } from '../../lib/data'
  import { review } from '../../lib/apply.svelte'
  import { api, session } from '../../lib/backend'
  import Icon from '../../lib/Icon.svelte'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'

  const users: any[] = raw.users ?? []
  const backups: any[] = raw.backups ?? []
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

  let channel = $state<string>(router.channel ?? 'stable')
  const today = () => new Date().toISOString().slice(0, 10)
  const ownFile = (n: string) => /^tik-[A-Za-z0-9_-]{1,40}\.(rsc|backup)$/.test(n)

  let keys = $state<{ name: string; key: string }[]>([])
  const newKey = () => (keys = [...keys, { name: `key-${keys.length + 1}`, key: Array.from(crypto.getRandomValues(new Uint8Array(24)), (b) => b.toString(16).padStart(2, '0')).join('') }])

  // Console: copy only. tikontrol never runs free-form RouterOS commands (no arbitrary command execution).
  let cmd = $state('')
  let copied = $state(false)
  async function copy() {
    try { await navigator.clipboard.writeText(cmd); copied = true; setTimeout(() => (copied = false), 1500) } catch { copied = false }
  }
</script>

{#if isBackend}
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
</Section>

<Section title="Backups" desc="A readable export without passwords and keys, saved on the router. Download it with WinBox or WebFig Files." ros="/file/print where type=backup">
  {#snippet actions()}<button class="btn primary" onclick={() => review('Back up now', [`/export file=tik-${today()}`], { kind: 'export-config', name: today() })}>Back up now</button>{/snippet}
  <table><thead><tr><th>File</th><th>Date</th><th class="r">Size</th><th></th></tr></thead>
    <tbody>{#each backups as b}<tr class="nohover"><td class="mono small">{b.name}</td><td>{b['last-modified']}</td><td class="r">{(b.size / 1024).toFixed(1)} KB</td>
      <td class="r"><button class="btn small" onclick={() => review(`Restore ${b.name}`, [`/system/backup/load name=${b.name}`])}>Restore</button>
        {#if ownFile(b.name)}<button class="btn small" onclick={() => review(`Delete ${b.name}`, [`/file/remove [find name="${b.name}"]`], { kind: 'remove-backup-file', name: b.name })}>Delete</button>{/if}</td></tr>{/each}</tbody></table>
  <div class="row muted small sched">Automatic: <button class="btn small" onclick={() => review('Weekly backups', ['/system/scheduler/add name=tik-backup interval=7d on-event="/export file=tik-auto"'])}>Weekly backups</button></div>
</Section>

<Section title="Updates" desc="RouterOS version and channel. Picking a channel downloads nothing." ros="/system/package/update/print">
  <Field label="Version"><span>{router.version}</span></Field>
  <Field label="Channel"><select bind:value={channel} aria-label="Update channel"><option>stable</option><option>long-term</option><option>testing</option><option>development</option></select>
    <button class="btn" disabled={channel === router.channel} onclick={() => review(`Update channel: ${channel}`, [`/system/package/update/set channel=${channel}`], { kind: 'set-update-channel', channel })}>Review & apply</button></Field>
  <a class="btn" href="#/devices/updates">Manage updates…</a>
</Section>

<Section title="API keys" desc="For Home Assistant and scripts. Keys belong to tikontrol, not the router.">
  {#if isBackend}<div class="muted small">Not available yet.</div>
  {:else}
    <button class="btn" onclick={newKey}><Icon name="key" size={14} /> New key</button>
    {#each keys as k}<div class="row small"><b>{k.name}</b><span class="mono">{k.key}</span></div>{:else}<div class="muted small">No API keys.</div>{/each}
  {/if}
</Section>

<Section title="Console" desc="tikontrol does not run free-form commands. Write or paste one here and copy it to the WinBox or SSH terminal.">
  <div class="add"><span class="mono muted">[admin@{router.identity}] &gt;</span><input type="text" class="mono" bind:value={cmd} placeholder="/ip/address/print" style="flex:1" aria-label="RouterOS command" />
    <button class="btn" disabled={!cmd} onclick={copy}><Icon name="terminal" size={14} /> {copied ? 'Copied' : 'Copy to WinBox'}</button></div>
</Section>

<style>
  .add { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; margin-top: 10px; }
  .sched { gap: 8px; margin-top: 8px; }
  .r { text-align: right; }
  .small { font-size: 12px; }
  .btn.small { padding: 3px 9px; }
  .row { gap: 10px; padding: 3px 0; }
  .ok { color: var(--ok); margin-top: 8px; }
  .err { color: var(--bad); margin-top: 8px; }
</style>
