<script lang="ts">
  import { raw, router, isBackend } from '../../lib/data'
  import { api, session } from '../../lib/backend'
  import { review } from '../../lib/apply.svelte'
  import Icon from '../../lib/Icon.svelte'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'
  import Toggle from '../../lib/ui/Toggle.svelte'

  const users: any[] = raw.users ?? []
  const groups: any[] = raw.userGroups ?? []
  const backups: any[] = raw.backups ?? []
  let u = $state({ name: '', group: 'read' })
  let cmd = $state('')
  let keys = $state<{ name: string; key: string }[]>([])
  // ---------- AI assistants (MCP): src/server/src/mcp.rs ----------
  type McpCfg = { enabled: boolean; privacy: boolean; endpoint: string | null; readWrite: boolean; protocolVersion: string }
  type Proposal = { id: number; t: number; title: string; description: string; commands: string[] }
  const isAdmin = session.me?.role === 'admin'
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

  const newKey = () => (keys = [...keys, { name: `key-${keys.length + 1}`, key: Array.from(crypto.getRandomValues(new Uint8Array(24)), (b) => b.toString(16).padStart(2, '0')).join('') }])
</script>

<Section title="Admins" desc="Who can log in to the router." ros="/user/print">
  <table><thead><tr><th>Name</th><th>Role</th><th>Last login</th><th></th></tr></thead>
    <tbody>{#each users as x}<tr class="nohover"><td><b>{x.name}</b> <span class="muted small">{x.comment ?? ''}</span></td><td><span class="pill">{x.group}</span></td><td>{x['last-logged-in'] ?? '—'}</td><td class="r">{#if x.name === 'admin'}<span class="muted small">Rename the default admin</span>{/if}</td></tr>{/each}</tbody></table>
  <div class="add"><input type="text" bind:value={u.name} placeholder="Username" /><select bind:value={u.group}>{#each groups as g}<option>{g.name}</option>{/each}</select>
    <button class="btn primary" disabled={!u.name} onclick={() => review(`Admin ${u.name}`, [`/user/add name=${u.name} group=${u.group} password="••••" comment="tik:admin"`])}><Icon name="plus" size={14} /> Invite</button></div>
  <Field label="Single sign-on" na="RouterOS has no SSO; use RADIUS for central logins"><span></span></Field>
</Section>

<Section title="Backups" desc="Binary backup plus a readable export, kept on the router and in tikontrol." ros="/file/print where type=backup">
  {#snippet actions()}<button class="btn primary" onclick={() => review('Back up now', [`/system/backup/save name=tik-${new Date().toISOString().slice(0, 10)} dont-encrypt=no`, `/export show-sensitive=no file=tik-${new Date().toISOString().slice(0, 10)}`])}>Back up now</button>{/snippet}
  <table><thead><tr><th>File</th><th>Date</th><th class="r">Size</th><th></th></tr></thead>
    <tbody>{#each backups as b}<tr class="nohover"><td class="mono small">{b.name}</td><td>{b['last-modified']}</td><td class="r">{(b.size / 1024).toFixed(1)} KB</td><td class="r"><button class="btn small" onclick={() => review(`Restore ${b.name}`, [`/system/backup/load name=${b.name}`])}>Restore</button></td></tr>{/each}</tbody></table>
  <div class="row muted small sched">Automatic: <button class="btn small" onclick={() => review('Weekly backups', ['/system/scheduler/add name=tik-backup interval=7d on-event="/system/backup/save name=tik-auto"'])}>Weekly backups</button></div>
</Section>

<Section title="Updates" desc="RouterOS version and channel." ros="/system/package/update/print">
  <Field label="Version"><span>{router.version}</span></Field>
  <a class="btn" href="#/devices/updates">Manage updates…</a>
</Section>

<Section title="API keys" desc="For Home Assistant and scripts. Keys belong to tikontrol, not the router.">
  {#snippet actions()}<button class="btn" onclick={newKey}><Icon name="key" size={14} /> New key</button>{/snippet}
  {#each keys as k}<div class="row small"><b>{k.name}</b><span class="mono">{k.key}</span></div>{:else}<div class="muted small">No API keys.</div>{/each}
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
            <button class="btn small" onclick={() => review(p.title, p.commands)}>Review…</button>
            {#if isAdmin}<button class="btn small" onclick={() => dismiss(p.id)}>Dismiss</button>{/if}
          </span></div>
          {#if p.description}<p class="muted small">{p.description}</p>{/if}
          <pre class="mono cfg">{p.commands.join('\n')}</pre>
        </div>
      {:else}
        <div class="muted small">No proposals. Nothing an assistant suggests runs until you review and apply it here.</div>
      {/each}
      <p class="muted small">Proposals come from the AI assistant: read every command before applying it.</p>
    {/if}
  {/if}
  {#if mcpErr}<div class="err small" role="alert">{mcpErr}</div>{/if}
</Section>

<Section title="Console" desc="Run a RouterOS command. Goes through Safe apply.">
  <div class="add"><span class="mono muted">[admin@{router.identity}] &gt;</span><input type="text" class="mono" bind:value={cmd} placeholder="/ip/address/print" style="flex:1" onkeydown={(e) => e.key === 'Enter' && cmd && review('Console', [cmd])} />
    <button class="btn primary" disabled={!cmd} onclick={() => review('Console', [cmd])}><Icon name="terminal" size={14} /> Run</button></div>
</Section>

<style>
  .add { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; margin-top: 10px; }
  .sched { gap: 8px; margin-top: 8px; }
  .row { gap: 10px; padding: 3px 0; }
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
</style>
