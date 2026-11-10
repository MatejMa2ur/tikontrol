<script lang="ts">
  // First run. Usually one form: a MikroTik admin account proves you own the router; tikontrol then creates its
  // read-only user there. Fallback: the setup token from the log, then paste commands (step 'router').
  // Read-only is enforced by the router user's group, not by this page.
  import { onMount, untrack } from 'svelte'
  import { api, ApiError, logout, site, switchSite, type Me } from '../lib/backend'
  let { step, me, hint = null }: { step: 'admin' | 'router'; me?: Me; hint?: string | null } = $props()

  let error = $state('')
  let busy = $state(false)
  const run = async (f: () => Promise<void>) => {
    busy = true
    error = ''
    try { await f() } catch (e) { error = (e as Error).message } finally { busy = false }
  }

  // one-step setup with a router admin account
  let useToken = $state(false)
  let user = $state('admin')
  let adminPw = $state('')
  let remember = $state(localStorage.getItem('tik.remember') !== 'false')
  let fix = $state<string[]>([])
  const findRouter = () => run(async () => {
    pin = ''
    fix = []
    try { pin = (await api<{ pin: string }>('/setup/probe', { method: 'POST', body: { host, port } })).pin }
    catch (e) { fix = (e instanceof ApiError && e.data?.commands) || []; throw e }
  })
  const connect = (e: SubmitEvent) => { e.preventDefault(); run(async () => {
    try { await api('/setup/router', { method: 'POST', body: { host, port, pin, user, password: adminPw, remember } }) }
    catch (err) { adminPw = ''; throw err }
    localStorage.setItem('tik.remember', String(remember))
    location.reload()
  }) }
  const fixCli = ':put [/certificate/get [find where name=[/ip/service/get api-ssl certificate]] fingerprint]'

  // setup token
  let token = $state('')
  let backup = $state(false)
  let name = $state('local-admin')
  let password = $state('')
  let password2 = $state('')

  // router
  // initial values only; the hint may be `host:port` (TIK_ROUTER)
  const hinted = untrack(() => (me?.routerHint ?? hint ?? '').match(/^([^:]*)(?::(\d+))?$/))
  // a router added from the Devices setup guide (ControlPlane ?add=); the server validates it again
  let host = $state((site && sessionStorage.getItem('tik.addHost')) || (hinted?.[1] ?? untrack(() => me?.routerHint ?? hint ?? '')))
  let port = $state(Number(hinted?.[2] ?? 8729))
  let pin = $state('')
  let pinOk = $state(false)
  let write = $state(false)
  let confirmWrite = $state(false)
  // 'me': changes run as the MikroTik admin who is logged in, nothing is stored. 'stored': a separate writer user.
  let writeAs = $state<'me' | 'stored'>('me')
  const stored = $derived(write && writeAs === 'stored')
  let tikIp = $state('')
  let reader = $state({ user: 'tikontrol-read', password: '' })
  let writer = $state({ user: 'tikontrol-write', password: '' })
  let commands = $state<string[]>([])
  $effect(() => {
    if (step !== 'router') return // needs a session; the one-step form has none yet
    const q = `?write=${stored ? 1 : 0}${tikIp ? `&ip=${encodeURIComponent(tikIp)}` : ''}`
    api<{ commands: string[] }>(`/router/setup-commands${q}`).then((r) => (commands = r.commands)).catch(() => {})
  })

  const admin = (e: SubmitEvent) => { e.preventDefault(); run(async () => {
    if (backup) {
      if (password !== password2) throw new Error('passwords do not match')
      await api('/setup/admin', { method: 'POST', body: { token: token.trim(), name, password } })
    } else {
      await api('/setup/start', { method: 'POST', body: { token: token.trim() } })
    }
    location.reload()
  }) }
  const probe = () => run(async () => { pinOk = false; pin = (await api<{ pin: string }>('/router/probe', { method: 'POST', body: { host, port } })).pin })
  // address known (TIK_ROUTER or the default gateway): read its certificate right away
  onMount(() => { if (host) (step === 'admin' ? findRouter : step === 'router' && !me?.router ? probe : () => {})() })
  const save = (e: SubmitEvent) => { e.preventDefault(); run(async () => {
    await api('/router', { method: 'POST', body: { host, port, pin, mode: write ? 'read-write' : 'read-only', reader, writer: stored ? writer : null, confirm_write: confirmWrite } })
    location.reload()
  }) }
  const pretty = (p: string) => p.match(/.{1,2}/g)?.join(':') ?? p
  // configured but no snapshot yet (first poll, or the router refused us): ask quietly in the background and
  // reload once, when the dashboard can open or the router's answer changed. A slow router takes several polls.
  $effect(() => {
    if (step !== 'router' || !me?.router || (me.setup && !me.error)) return
    const was = me.error
    const t = setInterval(async () => {
      const ready = await api('/snapshot').then(() => true, () => false)
      const m = ready ? null : await api<Me>('/me').catch(() => null)
      if (ready || (m && m.error !== was)) location.reload()
    }, 2_000)
    return () => clearInterval(t)
  })
</script>

<main class="setup">
  {#if step === 'admin' && !useToken}
    <form class="card" onsubmit={connect}>
      <h1>Sign in with your MikroTik admin account</h1>
      {#if !hint}
        <div class="row"><label class="grow">Router address<input type="text" bind:value={host} required placeholder="192.168.88.1" /></label>
          <button type="button" class="btn" onclick={findRouter} disabled={busy || !host}>Find</button></div>
      {/if}
      {#if pin}
        <p class="small">Router <b>{host}</b>, certificate SHA-256:<br /><code class="fp">{pretty(pin)}</code></p>
        <details class="adv"><summary>Is this my router?</summary><p class="small muted">To be sure, run <code>{fixCli}</code> in the router terminal: it must show the same value.</p></details>
        <label>Username<input type="text" bind:value={user} required maxlength="64" autocomplete="username" /></label>
        <label>Password<input type="password" bind:value={adminPw} required maxlength="256" autocomplete="current-password" /></label>
        <label class="check"><input type="checkbox" bind:checked={remember} /> Keep me signed in for 30 days</label>
        <p class="small muted">tikontrol will create on your router (comment <code>tik: tikontrol</code>):</p>
        <ul class="small what">
          <li>group <code>tikontrol-read</code>: read, api, test only</li>
          <li>user <code>tikontrol-read</code> with a random password, only from tikontrol's address (reset if it exists)</li>
        </ul>
        {#if error}<p class="err" role="alert">{error}</p>{/if}
        <button class="btn primary" disabled={busy}>{busy ? 'Connecting…' : 'Connect'}</button>
      {:else if fix.length}
        <p class="err" role="alert">{error}</p>
        <p class="small">{host} has no API-SSL yet. Paste this in the router terminal (WinBox → New Terminal), then try again:</p>
        <div class="cmdbox"><pre class="cmds">{fix.join('\n')}</pre>
          <button type="button" class="btn ghost copy" onclick={() => navigator.clipboard?.writeText(fix.filter((c) => !c.startsWith('#')).join('\n'))}>Copy</button></div>
        <button type="button" class="btn primary" onclick={findRouter} disabled={busy}>Try again</button>
      {:else if busy}<p class="muted small">Looking for the router…</p>
      {:else if error}<p class="err" role="alert">{error}</p>{/if}
      <button type="button" class="btn ghost" onclick={() => { useToken = true; error = '' }}>No admin account? Use the setup token from the log</button>
    </form>
  {:else if step === 'admin'}
    <form class="card" onsubmit={admin}>
      <h1>Set up with the setup token</h1>
      <label>Setup token<input type="text" bind:value={token} required autocomplete="off" spellcheck="false" /></label>
      <p class="hint">Printed in tikontrol's log at first start. It stops working once you sign in with your MikroTik account.</p>
      <label class="check"><input type="checkbox" bind:checked={backup} /> Also create a local backup account</label>
      {#if backup}
        <p class="hint">For when the router is unreachable. Use a name that is <b>not</b> your MikroTik username.</p>
        <label>Username<input type="text" bind:value={name} required maxlength="32" autocomplete="username" /></label>
        <label>Password (at least 12 characters)<input type="password" bind:value={password} required minlength="12" maxlength="256" autocomplete="new-password" /></label>
        <label>Repeat password<input type="password" bind:value={password2} required minlength="12" maxlength="256" autocomplete="new-password" /></label>
      {/if}
      {#if error}<p class="err" role="alert">{error}</p>{/if}
      <button class="btn primary" disabled={busy}>Continue</button>
      <button type="button" class="btn ghost" onclick={() => { useToken = false; error = '' }}>Back</button>
    </form>
  {:else if me?.router && !me.error && !me.setup}
    <!-- set up and connected, first read still running: nothing to do but wait (the effect above reloads once) -->
    <div class="card" role="status" aria-live="polite">
      <h1>Reading your router…</h1>
      <p class="muted">Connected to <b>{me.router.host}</b>. The first read takes a few seconds on a small router; the dashboard opens by itself.</p>
    </div>
  {:else}
    <form class="card wide" onsubmit={save}>
      <div class="row between"><h1>Connect your router</h1><span class="row">{#if site}<button type="button" class="btn ghost" onclick={() => switchSite('')}>Back to the main router</button>{/if}<button type="button" class="btn ghost" onclick={logout}>Sign out</button></span></div>
      {#if me?.role !== 'admin'}<p class="err">Only an admin can connect the router.</p>{/if}
      {#if me?.router}
        <div class="status" role="status">
          <b>{me.router.host}</b> · {me.router.mode}
          {#if me.error}<p class="err">{me.error}</p><p class="muted small">Fix it on the router, or connect again below. This page updates by itself.</p>
          {:else if me.setup}<p class="small">Connected. Last step: sign in with your <b>MikroTik username and password</b>. That finishes setup and the setup token stops working.</p>
            <button type="button" class="btn primary" onclick={logout}>Sign in with MikroTik account</button>
          {:else}<p class="muted small">Connected. Reading the router for the first time…</p>{/if}
        </div>
      {/if}

      <h2>1. Access mode</h2>
      <fieldset class="modes">
        <legend class="sr">Access mode</legend>
        <label class="mode" class:on={!write}><input type="radio" name="mode" checked={!write} onchange={() => (write = false)} /> <b>Read-only</b> (recommended)<span class="muted small">Shows everything, changes nothing. No write credential is stored.</span></label>
        <label class="mode" class:on={write}><input type="radio" name="mode" checked={write} onchange={() => (write = true)} /> <b>Read and write</b><span class="muted small">Changes go through Safe apply.</span></label>
      </fieldset>
      <p class="muted small">Read-only is enforced by the router: the reader's group has only <code>read</code>, <code>api</code> and <code>test</code>, and tikontrol stops if it finds more.</p>

      <h2>2. Run these commands on the router</h2>
      <p class="muted small">In a RouterOS terminal (WinBox → New Terminal). Replace <code>&lt;choose-a-long-password&gt;</code>; you type it again in step 4.</p>
      <label class="inline">tikontrol's IP address <input type="text" bind:value={tikIp} placeholder="e.g. 192.168.88.2" inputmode="decimal" /></label>
      <div class="cmdbox">
        <pre class="cmds">{commands.join('\n')}</pre>
        <button type="button" class="btn ghost copy" onclick={() => navigator.clipboard?.writeText(commands.filter((c) => !c.startsWith('#')).join('\n'))}>Copy</button>
      </div>
      <details class="adv"><summary>For advanced users</summary>
        <ul class="small muted">
          <li>Already have a certificate on API-SSL? Skip the first block and keep yours; step 3 pins whatever the router presents.</li>
          <li>RouterOS 7.24+ calls the service setting <code>available-from</code>; the older name <code>address</code> still works.</li>
          <li>On a router running tikontrol in a container, use the container's address (e.g. <code>172.17.0.2</code>) as tikontrol's IP.</li>
          <li>Check afterwards: <code>/user/print where name=tikontrol-read</code> and <code>/ip/service/print detail where name=api-ssl</code>.</li>
        </ul>
      </details>

      <h2>3. Router address and certificate</h2>
      <div class="grid2">
        <label>Router address<input type="text" bind:value={host} required placeholder="192.168.88.1" /></label>
        <label>API-SSL port<input type="number" bind:value={port} min="1" max="65535" required /></label>
      </div>
      <button type="button" class="btn" onclick={probe} disabled={busy || !host}>Read certificate</button>
      {#if pin}
        <div class="pin">
          <p class="small">Run <code>{fixCli}</code> on the router; it must match:</p>
          <code class="fp">{pretty(pin)}</code>
          <label class="check"><input type="checkbox" bind:checked={pinOk} /> This is my router's certificate</label>
        </div>
      {/if}

      <h2>4. Credentials</h2>
      <p class="muted small">From step 2. tikontrol checks the user's real policies before saving; the password is stored encrypted.</p>
      <div class="grid2">
        <label>Reader user<input type="text" bind:value={reader.user} required autocomplete="off" /></label>
        <label>Reader password<input type="password" bind:value={reader.password} required autocomplete="new-password" /></label>
        {#if stored}
          <label>Writer user<input type="text" bind:value={writer.user} required autocomplete="off" /></label>
          <label>Writer password<input type="password" bind:value={writer.password} required autocomplete="new-password" /></label>
        {/if}
      </div>
      {#if write}
        <fieldset class="modes">
          <legend class="small">Who makes the changes</legend>
          <label class="mode" class:on={writeAs === 'me'}><input type="radio" name="writeAs" checked={writeAs === 'me'} onchange={() => (writeAs = 'me')} /> <b>The admin who is logged in</b> (recommended)<span class="muted small">Changes run as you; no write password is stored.</span></label>
          <label class="mode" class:on={writeAs === 'stored'}><input type="radio" name="writeAs" checked={writeAs === 'stored'} onchange={() => (writeAs = 'stored')} /> <b>A stored writer user</b><span class="muted small">Stored encrypted; can change the whole router.</span></label>
        </fieldset>
        <label class="check warn"><input type="checkbox" bind:checked={confirmWrite} /> {stored ? 'I understand the writer credential can change or break the whole router, and tikontrol will store it encrypted.' : 'I understand that admins who sign in with their MikroTik account can change the router through tikontrol.'}</label>
      {/if}
      {#if error}<p class="err" role="alert">{error}</p>{/if}
      <button class="btn primary" disabled={busy || !pin || !pinOk || (write && !confirmWrite) || me?.role !== 'admin'}>{busy ? 'Checking…' : 'Check policies and connect'}</button>
      <details class="adv"><summary>For advanced users</summary>
        <ul class="small muted">
          <li>Router passwords are encrypted with ChaCha20-Poly1305. The key is the separate <code>key</code> file in <code>TIK_DATA</code>: back up both together and keep them private.</li>
          <li>After connecting, sign out and sign in with your MikroTik account. Its password is checked by the router and never stored.</li>
          <li>The reader logs in about every 30 seconds (<code>TIK_POLL</code>); those logins appear in the router log.</li>
        </ul>
      </details>
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
  .grow { flex: 1; }
  .row:has(.grow) { align-items: end; }
  .what { margin: 0; padding-left: 18px; display: grid; gap: 3px; }
  .hint { margin: -6px 0 0; font-size: 12px; color: var(--muted); }
  .adv { border-top: 1px solid var(--line); padding-top: 6px; }
  .adv summary { cursor: pointer; color: var(--muted); font-size: 12px; }
  .adv ul { margin: 6px 0 0; padding-left: 18px; display: grid; gap: 4px; }
  .cmdbox { position: relative; }
  .copy { position: absolute; top: 6px; right: 6px; padding: 3px 9px; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
  @media (max-width: 600px) { .grid2 { grid-template-columns: 1fr; } }
</style>
