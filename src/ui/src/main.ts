import { mount } from 'svelte'
import './app.css'
import { boot, preload } from './lib/backend'

// Decide what to show before the app (and data.ts, which reads the first snapshot) is loaded.
const target = document.getElementById('app')!
const theme = (() => { try { return JSON.parse(localStorage.getItem('tik.theme') ?? '"dark"') } catch { return 'dark' } })()
document.documentElement.dataset.theme = new URLSearchParams(location.search).get('theme') ?? theme

const b = await boot().catch((e) => ({ mode: 'error' as const, error: String(e) }))
if (b.mode === 'login') mount((await import('./pages/Login.svelte')).default, { target })
else if (b.mode === 'setup') mount((await import('./pages/Setup.svelte')).default, { target, props: { step: 'admin' } })
else if (b.mode === 'router') mount((await import('./pages/Setup.svelte')).default, { target, props: { step: 'router', me: b.me } })
else if (b.mode === 'error') target.textContent = `tikontrol: ${b.error}`
else {
  if (b.mode === 'app') preload.snapshot = b.snapshot
  mount((await import('./App.svelte')).default, { target })
}
