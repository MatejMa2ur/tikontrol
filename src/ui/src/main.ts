import { mount } from 'svelte'
import './app.css'
import { boot, preload } from './lib/backend'
import { applyTheme } from './lib/themes'

// Decide what to show before the app (and data.ts, which reads the first snapshot) is loaded.
const target = document.getElementById('app')!
const stored = (k: string, d: string) => { try { return String(JSON.parse(localStorage.getItem(k) ?? '') ?? d) } catch { return d } }
const q = new URLSearchParams(location.search)
const mode = q.get('theme') ?? stored('tik.theme', 'dark')
applyTheme(document.documentElement, mode === 'light' ? 'light' : 'dark', q.get('palette') ?? stored('tik.palette', ''), q.get('design') ?? stored('tik.design', ''))

const b = await boot().catch((e) => ({ mode: 'error' as const, error: String(e) }))
if (b.mode === 'login') mount((await import('./pages/Login.svelte')).default, { target })
else if (b.mode === 'setup') mount((await import('./pages/Setup.svelte')).default, { target, props: { step: 'admin' } })
else if (b.mode === 'router') mount((await import('./pages/Setup.svelte')).default, { target, props: { step: 'router', me: b.me } })
else if (b.mode === 'error') target.textContent = `tikontrol: ${b.error}`
else {
  if (b.mode === 'app') preload.snapshot = b.snapshot
  mount((await import('./App.svelte')).default, { target })
}
