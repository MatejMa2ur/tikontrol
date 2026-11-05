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
  let backup = $state(false)
  let name = $state('local-admin')
  let password = $state('')
  let password2 = $state('')

  // router
  let host = $state(me?.routerHint ?? '')
  let port = $state(8729)
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
  if (step === 'router' && !me?.router && host) probe()
  const save = (e: SubmitEvent) => { e.preventDefault(); run(async () => {
    await api('/router', { method: 'POST', body: { host, port, pin, mode: write ? 'read-write' : 'read-only', reader, writer: stored ? writer : null, confirm_write: confirmWrite } })
    location.reload()
  }) }
  const pretty = (p: string) => p.match(/.{1,2}/g)?.join(':') ?? p
  // configured but no snapshot yet: check again shortly (first poll, or the router refused us)
  $effect(() => {
    if (step !== 'router' || !me?.router || (me.setup && !me.error)) return
    const t = setTimeout(() => location.reload(), me.error ? 30_000 : 4_000)
    return () => clearTimeout(t)
  })
</script>

<main class="setup">
  {#if step === 'admin'}
    <form class="card" onsubmit={admin}>
      <h1>Welcome to tikontrol</h1>
      <p class="muted">Three steps, no extra account needed:</p>
      <ol class="steps small">
        <li class="now"><b>Enter the setup token</b>: proves you run tikontrol.</li>
        <li><b>Connect your router</b>: a read-only RouterOS user lets tikontrol show live data.</li>
        <li><b>Sign in with your MikroTik account</b>: from then on, your router group decides what you can do.</li>
      </ol>

      <label>Setup token<input type="text" bind:value={token} required autocomplete="off" spellcheck="false" /></label>
      <p class="hint">A one-time code tikontrol printed in its log when it first started (the terminal or container output). It proves you are the person running tikontrol, so nobody else on your network can set it up. It stops working once you sign in with your MikroTik account.</p>
      <label class="check"><input type="checkbox" bind:checked={backup} /> Also create a local backup account (optional)</label>
      {#if backup}
        <p class="hint">Only needed to get into tikontrol while the router is unreachable. Pick a name that is <b>not</b> your MikroTik username, e.g. <code>local-admin</code>: a local account always wins over a router account with the same name.</p>
        <label>Username<input type="text" bind:value={name} required maxlength="32" autocomplete="username" /></label>
        <label>Password (at least 12 characters)<input type="password" bind:value={password} required minlength="12" maxlength="256" autocomplete="new-password" /></label>
        <label>Repeat password<input type="password" bind:value={password2} required minlength="12" maxlength="256" autocomplete="new-password" /></label>
      {/if}
      {#if error}<p class="err" role="alert">{error}</p>{/if}
      <button class="btn primary" disabled={busy}>Continue</button>
      <details class="adv"><summary>For advanced users</summary>
        <ul class="small muted">
          <li>The token opens a temporary setup session that can only be used to connect the router; it ends when the first admin signs in. You can add local accounts later under Settings → Control plane.</li>
          <li>A local password is stored as a PBKDF2-SHA256 hash (600,000 rounds) in <code>state.json</code> inside <code>TIK_DATA</code>; the plain password is never kept.</li>
          <li>Lost the token? Restart tikontrol while no account exists: it prints a new one.</li>
          <li>Sessions end after 30 minutes idle or 12 hours. Five wrong passwords lock the address out for a while.</li>
        </ul>
      </details>
    </form>
  {:else}
    <form class="card wide" onsubmit={save}>
      <div class="row between"><h1>Connect your router</h1><button type="button" class="btn ghost" onclick={logout}>Sign out</button></div>
      {#if me?.role !== 'admin'}<p class="err">Only an admin can connect the router.</p>{/if}
      {#if me?.router}
        <div class="status" role="status">
          <b>{me.router.host}</b> · {me.router.mode}
          {#if me.error}<p class="err">{me.error}</p><p class="muted small">Fix it on the router, or connect again below. Checking again in 30 s.</p>
          {:else if me.setup}<p class="small">Connected. Last step: sign in with your <b>MikroTik username and password</b>. That finishes setup and the setup token stops working.</p>
            <button type="button" class="btn primary" onclick={logout}>Sign in with MikroTik account</button>
          {:else}<p class="muted small">Connected. Reading the router for the first time…</p>{/if}
        </div>
      {/if}

      <p class="muted">tikontrol talks to the router over the encrypted RouterOS API (API-SSL). Four steps, about five minutes:</p>
      <ol class="steps small">
        <li><b>Access mode</b>: read-only to start; you can switch later.</li>
        <li><b>RouterOS commands</b>: create a certificate and a read-only user on the router.</li>
        <li><b>Certificate check</b>: make sure tikontrol talks to <i>your</i> router.</li>
        <li><b>Credentials</b>: tikontrol checks what the user can do before saving anything.</li>
      </ol>

      <h2>1. Access mode</h2>
      <fieldset class="modes">
        <legend class="sr">Access mode</legend>
        <label class="mode" class:on={!write}><input type="radio" name="mode" checked={!write} onchange={() => (write = false)} /> <b>Read-only</b> (recommended)<span class="muted small">tikontrol shows everything and changes nothing. The router enforces it: the user has no write, sensitive, password, policy or reboot policy, and no write credential is stored.</span></label>
        <label class="mode" class:on={write}><input type="radio" name="mode" checked={write} onchange={() => (write = true)} /> <b>Read and write</b><span class="muted small">Changes go through Safe apply. The writer user can change the whole router: treat its password like the admin password.</span></label>
      </fieldset>
      <details class="adv"><summary>For advanced users</summary>
        <ul class="small muted">
          <li>Read-only is enforced by the <b>router</b>: the reader's group has only <code>read</code>, <code>api</code> and <code>test</code>. tikontrol checks the group on every poll and stops if it finds <code>write</code>, <code>sensitive</code>, <code>password</code>, <code>policy</code> or <code>reboot</code>.</li>
          <li><code>sensitive</code> is left out on purpose, so tikontrol never sees Wi-Fi passwords, VPN keys or other secrets.</li>
          <li>Read and write uses Safe apply: each change arms an automatic undo on the router, and you confirm it within 5 minutes or it rolls back.</li>
        </ul>
      </details>

      <h2>2. Run these commands on the router</h2>
      <p class="muted small">Open a RouterOS terminal (WinBox → <i>New Terminal</i>, or <code>ssh admin@router</code>) and paste the commands once. Replace <code>&lt;choose-a-long-password&gt;</code> with a password you pick; tikontrol never puts a password in a command, and you type it again in step 4.</p>
      <ul class="small what">
        <li><b>Certificate</b>: the router gets its own certificate so the connection is encrypted and tikontrol can recognise it.</li>
        <li><b>Plain API off</b>: the unencrypted API (port 8728) sends passwords in clear text. Skip that line if something else of yours still uses it.</li>
        <li><b>Read-only user</b>: a separate RouterOS user that can look but not touch, accepted only from tikontrol's address.</li>
      </ul>
      <label class="inline">tikontrol's IP address, so the router only accepts it from there <input type="text" bind:value={tikIp} placeholder="e.g. 192.168.88.2" inputmode="decimal" /></label>
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
      <p class="muted small">tikontrol reads the router's certificate and shows its fingerprint. You compare it with what the router itself says, once. From then on tikontrol refuses any other certificate, so nobody can pretend to be your router.</p>
      <div class="grid2">
        <label>Router address<input type="text" bind:value={host} required placeholder="192.168.88.1" /></label>
        <label>API-SSL port<input type="number" bind:value={port} min="1" max="65535" required /></label>
      </div>
      <button type="button" class="btn" onclick={probe} disabled={busy || !host}>Read certificate</button>
      {#if pin}
        <div class="pin">
          <p class="small">Run <code>:put [/certificate/get tikontrol-api fingerprint]</code> in the router terminal. The result must be exactly this:</p>
          <code class="fp">{pretty(pin)}</code>
          <label class="check"><input type="checkbox" bind:checked={pinOk} /> This is my router's certificate</label>
        </div>
      {/if}

      <h2>4. Credentials</h2>
      <p class="muted small">The user and password from step 2. tikontrol logs in once, reads the user's real permissions, and only saves if they are right. The password is stored encrypted on this machine.</p>
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
          <label class="mode" class:on={writeAs === 'me'}><input type="radio" name="writeAs" checked={writeAs === 'me'} onchange={() => (writeAs = 'me')} /> <b>The admin who is logged in</b> (recommended)<span class="muted small">Sign in with your MikroTik account; changes run as you. Your password stays in memory for your session only, so tikontrol stores no write password.</span></label>
          <label class="mode" class:on={writeAs === 'stored'}><input type="radio" name="writeAs" checked={writeAs === 'stored'} onchange={() => (writeAs = 'stored')} /> <b>A stored writer user</b><span class="muted small">Lets local tikontrol accounts make changes. Its password is stored encrypted and can change the whole router.</span></label>
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
  .steps { margin: 0; padding-left: 20px; display: grid; gap: 4px; color: var(--muted); }
  .steps b { color: var(--text); }
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
