<script lang="ts">
  // First run: create the admin (needs the setup token from the log), then connect the router.
  // Read-only is the default and is enforced by the router user's group, not by this page.
  import { api, logout, type Me } from '../lib/backend'
  let { step, me }: { step: 'admin' | 'router'; me?: Me } = $props()

  let error = $state('')
  let busy = $state(false)
  const run = async (f: () => Promise<void>) => {
    busy = true
    error = ''
    try { await f() } catch (e) { error = (e as Error).message } finally { busy = false }
  }

  // admin
  let token = $state('')
  let name = $state('admin')
  let password = $state('')
  let password2 = $state('')

  // router
  let host = $state('')
  let port = $state(8729)
  let pin = $state('')
  let pinOk = $state(false)
  let write = $state(false)
  let confirmWrite = $state(false)
  let tikIp = $state('')
  let reader = $state({ user: 'tikontrol-read', password: '' })
  let writer = $state({ user: 'tikontrol-write', password: '' })
  let commands = $state<string[]>([])
  $effect(() => {
    const q = `?write=${write ? 1 : 0}${tikIp ? `&ip=${encodeURIComponent(tikIp)}` : ''}`
    api<{ commands: string[] }>(`/router/setup-commands${q}`).then((r) => (commands = r.commands)).catch(() => {})
  })

  const admin = (e: SubmitEvent) => { e.preventDefault(); run(async () => {
    if (password !== password2) throw new Error('passwords do not match')
    await api('/setup/admin', { method: 'POST', body: { token: token.trim(), name, password } })
    location.reload()
  }) }
  const probe = () => run(async () => { pinOk = false; pin = (await api<{ pin: string }>('/router/probe', { method: 'POST', body: { host, port } })).pin })
  const save = (e: SubmitEvent) => { e.preventDefault(); run(async () => {
    await api('/router', { method: 'POST', body: { host, port, pin, mode: write ? 'read-write' : 'read-only', reader, writer: write ? writer : null, confirm_write: confirmWrite } })
    location.reload()
  }) }
  const pretty = (p: string) => p.match(/.{1,2}/g)?.join(':') ?? p
  // configured but no snapshot yet: check again shortly (first poll, or the router refused us)
  $effect(() => {
    if (step !== 'router' || !me?.router) return
    const t = setTimeout(() => location.reload(), me.error ? 30_000 : 4_000)
    return () => clearTimeout(t)
  })
</script>

<main class="setup">
  {#if step === 'admin'}
    <form class="card" onsubmit={admin}>
      <h1>Welcome to tikontrol</h1>
      <p class="muted">Create the admin account. The setup token is printed in the tikontrol log (container output) on first start.</p>
      <label>Setup token<input type="text" bind:value={token} required autocomplete="off" spellcheck="false" /></label>
      <label>Admin username<input type="text" bind:value={name} required maxlength="32" autocomplete="username" /></label>
      <label>Password (at least 12 characters)<input type="password" bind:value={password} required minlength="12" maxlength="256" autocomplete="new-password" /></label>
      <label>Repeat password<input type="password" bind:value={password2} required minlength="12" maxlength="256" autocomplete="new-password" /></label>
      {#if error}<p class="err" role="alert">{error}</p>{/if}
      <button class="btn primary" disabled={busy}>Create admin</button>
    </form>
  {:else}
    <form class="card wide" onsubmit={save}>
      <div class="row between"><h1>Connect your router</h1><button type="button" class="btn ghost" onclick={logout}>Sign out</button></div>
      {#if me?.role !== 'admin'}<p class="err">Only an admin can connect the router.</p>{/if}
      {#if me?.router}
        <div class="status" role="status">
          <b>{me.router.host}</b> · {me.router.mode}
          {#if me.error}<p class="err">{me.error}</p><p class="muted small">Fix it on the router, or connect again below. Checking again in 30 s.</p>
          {:else}<p class="muted small">Reading the router for the first time…</p>{/if}
        </div>
      {/if}

      <h2>1. Access mode</h2>
      <fieldset class="modes">
        <legend class="sr">Access mode</legend>
        <label class="mode" class:on={!write}><input type="radio" name="mode" checked={!write} onchange={() => (write = false)} /> <b>Read-only</b> (recommended)<span class="muted small">tikontrol shows everything and changes nothing. The router enforces it: the user has no write, sensitive, password, policy or reboot policy, and no write credential is stored.</span></label>
        <label class="mode" class:on={write}><input type="radio" name="mode" checked={write} onchange={() => (write = true)} /> <b>Read and write</b><span class="muted small">Changes go through Safe apply. The writer user can change the whole router: treat its password like the admin password.</span></label>
      </fieldset>

      <h2>2. Create the RouterOS users</h2>
      <p class="muted small">Run these once in a RouterOS terminal (WinBox → New Terminal, or SSH). Type your own passwords where shown; tikontrol never puts a password in a command.</p>
      <label class="inline">tikontrol's IP address, so the router only accepts it from there <input type="text" bind:value={tikIp} placeholder="e.g. 192.168.88.2" inputmode="decimal" /></label>
      <pre class="cmds">{commands.join('\n')}</pre>

      <h2>3. Router address and certificate</h2>
      <div class="grid2">
        <label>Router address<input type="text" bind:value={host} required placeholder="192.168.88.1" /></label>
        <label>API-SSL port<input type="number" bind:value={port} min="1" max="65535" required /></label>
      </div>
      <button type="button" class="btn" onclick={probe} disabled={busy || !host}>Read certificate</button>
      {#if pin}
        <div class="pin">
          <p class="small">The router presented this certificate. Check it matches <code>/certificate/print detail where name=tikontrol-api</code> (the <code>fingerprint</code> field) before trusting it. tikontrol will refuse any other certificate from now on.</p>
          <code class="fp">{pretty(pin)}</code>
          <label class="check"><input type="checkbox" bind:checked={pinOk} /> This is my router's certificate</label>
        </div>
      {/if}

      <h2>4. Credentials</h2>
      <div class="grid2">
        <label>Reader user<input type="text" bind:value={reader.user} required autocomplete="off" /></label>
        <label>Reader password<input type="password" bind:value={reader.password} required autocomplete="new-password" /></label>
        {#if write}
          <label>Writer user<input type="text" bind:value={writer.user} required autocomplete="off" /></label>
          <label>Writer password<input type="password" bind:value={writer.password} required autocomplete="new-password" /></label>
        {/if}
      </div>
      {#if write}<label class="check warn"><input type="checkbox" bind:checked={confirmWrite} /> I understand the writer credential can change or break the whole router, and tikontrol will store it encrypted.</label>{/if}
      {#if error}<p class="err" role="alert">{error}</p>{/if}
      <button class="btn primary" disabled={busy || !pin || !pinOk || (write && !confirmWrite) || me?.role !== 'admin'}>{busy ? 'Checking…' : 'Check policies and connect'}</button>
      <p class="muted small">tikontrol logs in, reads the user's real group policies and refuses read-only mode if the reader could change anything.</p>
    </form>
  {/if}
</main>

<style>
  .setup { min-height: 100vh; display: grid; place-items: center; padding: 24px 16px; }
  form { width: min(420px, 100%); display: grid; gap: 12px; padding: 24px; }
  form.wide { width: min(760px, 100%); }
  h1 { font-size: 17px; margin: 0; }
  h2 { font-size: 13.5px; margin: 10px 0 0; }
  label { display: grid; gap: 4px; font-weight: 550; }
  label input[type='text'], label input[type='password'], label input[type='number'] { width: 100%; box-sizing: border-box; }
  .inline { font-weight: 450; }
  .grid2 { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  .modes { border: 0; padding: 0; margin: 0; display: grid; gap: 8px; }
  .mode { display: block; border: 1px solid var(--line); border-radius: 8px; padding: 10px 12px; font-weight: 450; cursor: pointer; }
  .mode.on { border-color: var(--accent); }
  .mode span { display: block; margin-top: 4px; }
  .check { display: flex; gap: 8px; align-items: flex-start; font-weight: 450; }
  .warn { color: var(--warn); }
  .cmds { background: var(--code-bg); border: 1px solid var(--line); border-radius: 8px; padding: 10px 12px; font: 12px/1.7 var(--mono); white-space: pre-wrap; overflow: auto; margin: 0; }
  .pin { border: 1px solid var(--line); border-radius: 8px; padding: 10px 12px; display: grid; gap: 8px; }
  .fp { font: 12px var(--mono); word-break: break-all; }
  .err { color: var(--bad); margin: 0; }
  .status { border: 1px solid var(--line); border-radius: 8px; padding: 10px 12px; }
  .status p { margin: 4px 0 0; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
  @media (max-width: 600px) { .grid2 { grid-template-columns: 1fr; } }
</style>
