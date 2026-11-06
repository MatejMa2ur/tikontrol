<script lang="ts">
  import { api } from '../lib/backend'
  let name = $state('')
  let password = $state('')
  let remember = $state(localStorage.getItem('tik.remember') !== 'false')
  let error = $state('')
  let busy = $state(false)
  async function submit(e: SubmitEvent) {
    e.preventDefault()
    busy = true
    error = ''
    try {
      await api('/login', { method: 'POST', body: { name, password, remember } })
      localStorage.setItem('tik.remember', String(remember))
      location.reload()
    } catch (err) {
      error = (err as Error).message
      password = ''
    } finally {
      busy = false
    }
  }
</script>

<main class="auth">
  <form class="card" onsubmit={submit}>
    <div class="logo" aria-hidden="true">tik</div>
    <h1>Sign in to tikontrol</h1>
    <p class="muted small">Sign in with your <b>MikroTik username and password</b>, the same as in WinBox. Your router group decides what you can do. If the router is unreachable, use tikontrol's local account instead.</p>
    <label>Username<input type="text" autocomplete="username" bind:value={name} required maxlength="64" /></label>
    <label>Password<input type="password" autocomplete="current-password" bind:value={password} required maxlength="256" /></label>
    {#if error}<p class="err" role="alert">{error}</p>{/if}
    <label class="keep"><input type="checkbox" bind:checked={remember} /> Keep me signed in on this browser for 30 days <span class="muted small">(also across updates; Sign out ends it)</span></label>
    <button class="btn primary" disabled={busy}>{busy ? 'Signing in…' : 'Sign in'}</button>
  </form>
</main>

<style>
  .auth { min-height: 100vh; display: grid; place-items: center; padding: 16px; }
  form { width: min(360px, 100%); display: grid; gap: 12px; padding: 24px; }
  .logo { font: 800 18px var(--font); letter-spacing: -.03em; }
  h1 { font-size: 16px; margin: 0 0 4px; }
  label { display: grid; gap: 4px; font-weight: 550; }
  label input { width: 100%; box-sizing: border-box; }
  .err { color: var(--bad); margin: 0; }
  button { justify-content: center; }
  .keep { display: flex; gap: 8px; align-items: baseline; font-weight: 500; }
</style>
