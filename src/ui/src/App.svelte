<script lang="ts">
  import { tick as settled } from 'svelte'
  import { router, isSample, simulated, isBackend, load } from './lib/data'
  import { api, logout, session } from './lib/backend'
  import { apply } from './lib/apply.svelte'
  import { prefs } from './lib/live.svelte'
  import Icon from './lib/Icon.svelte'
  import Palette from './lib/Palette.svelte'
  import Dashboard from './pages/Dashboard.svelte'
  import Clients from './pages/Clients.svelte'
  import Topology from './pages/Topology.svelte'
  import Devices from './pages/Devices.svelte'
  import SafeApply from './lib/ui/SafeApply.svelte'
  import Events from './pages/Events.svelte'
  import Insights from './pages/Insights.svelte'
  import Hotspot from './pages/Hotspot.svelte'
  import Settings from './pages/Settings.svelte'

  // ≤ 8 top-level items (PLAN §0.6 rule 5). `sections` = FEATURES.md headings for unbuilt pages.
  const pages = [
    { id: 'dashboard', label: 'Dashboard', icon: 'dashboard' },
    { id: 'topology', label: 'Topology', icon: 'topology' },
    { id: 'devices', label: 'Devices', icon: 'devices' },
    { id: 'clients', label: 'Clients', icon: 'clients' },
    { id: 'insights', label: 'Insights', icon: 'traffic' },
    { id: 'events', label: 'Logs & Alarms', icon: 'events' },
    { id: 'hotspot', label: 'Hotspot', icon: 'ticket' },
    { id: 'settings', label: 'Settings', icon: 'settings' },
  ]

  let hash = $state(location.hash)
  $effect(() => {
    const on = () => (hash = location.hash)
    addEventListener('hashchange', on)
    return () => removeEventListener('hashchange', on)
  })
  const route = $derived(hash.replace(/^#\/?/, '').split('/'))
  // old/short links: #/ports and #/radios live under Devices
  const alias: Record<string, string[]> = { ports: ['devices', 'ports'], radios: ['devices', 'radios'] }
  const r = $derived(alias[route[0]] ?? route)
  const page = $derived(pages.find((p) => p.id === r[0]) ?? pages[0])

  // With the server: fetch a fresh snapshot every 30 s and re-mount the page with it, unless the user is
  // typing or reviewing a change (live numbers stream separately and never need this).
  let version = $state(0)
  let me = $state(session.me)
  let stale = $state('')
  $effect(() => {
    if (!isBackend) return
    let last = ''
    const tick = async () => {
      try {
        const s = await api('/snapshot')
        const json = JSON.stringify(s)
        stale = ''
        // don't rebuild the page under someone typing, reviewing a change or using a dialog
        const busy = apply.open || apply.armed || !!document.querySelector('[role="dialog"], details[open]') || document.activeElement?.matches('input, textarea, select')
        if (json !== last && last && !busy) {
          const y = scrollY
          load(s); version++
          await settled(); scrollTo(0, y)
        }
        if (!busy) last = json
      } catch (e) { stale = (e as Error).message }
      me = await api('/me').catch(() => me)
    }
    tick()
    const i = setInterval(tick, 30_000)
    return () => clearInterval(i)
  })
  const modeLabel = $derived(me?.readOnly ? 'Read-only' : 'Read & write')
  const modeTitle = $derived(me?.policies ? `Router user ${me.policies.user} (group ${me.policies.group}): ${me.policies.policy.join(', ')}` : '')
</script>

<!-- Layout follows UniFi Network: top app bar, icon-only rail, full-bleed content -->
<header class="appbar">
  <a class="logo" href="#/dashboard" aria-label="tikontrol home">tik</a>
  <nav class="apps">
    <a class="app on" href="#/dashboard"><span class="ring"></span>Network</a>
    <a class="app" href="#/devices" title="Devices"><Icon name="devices" size={16} /></a>
    <a class="app" href="#/clients" title="Clients"><Icon name="clients" size={16} /></a>
  </nav>
  {#if isBackend}
    <span class="pill hide-sm mock" class:ok={me?.readOnly} class:warn={!me?.readOnly} title={modeTitle}>{modeLabel}</span>
    {#if stale || me?.error}<span class="pill bad hide-sm" role="status" title={me?.error ?? stale}>Router unreachable · showing last data</span>{/if}
  {:else}
    <span class="pill hide-sm mock" title="No tikontrol server: live numbers are simulated around a snapshot">{simulated ? 'Simulated hAP be³ Media · ' : isSample ? 'Sample data · ' : ''}Mockup</span>
  {/if}
  <div class="right row">
    <Palette {pages} />
    <button class="btn ghost" class:on={prefs.showRos} onclick={() => (prefs.showRos = !prefs.showRos)} title="Show the RouterOS commands behind every card"><Icon name="code" size={15} /></button>
    <button class="btn ghost" onclick={() => (prefs.theme = prefs.theme === 'dark' ? 'light' : 'dark')} aria-label="Toggle theme"><Icon name={prefs.theme === 'dark' ? 'sun' : 'moon'} size={15} /></button>
    <span class="site hide-sm"><span class="dot ok"></span><span class="led"></span>{router.board?.startsWith(router.identity) ? router.board : `${router.identity} ${router.board}`}<Icon name="down" size={11} /></span>
    {#if isBackend}
      <button class="avatar" title="Signed in as {me?.user} ({me?.role}). Click to sign out." aria-label="Sign out {me?.user}" onclick={logout}>{(me?.user ?? 'a').slice(0, 1).toUpperCase()}</button>
    {:else}<span class="avatar" title="admin">A</span>{/if}
  </div>
</header>

<SafeApply />
<div class="shell">
  <nav class="rail" aria-label="Main">
    {#each pages as p, i}
      {#if i === pages.length - 1}<span class="grow"></span>{/if}
      <a href="#/{p.id}" class:on={page.id === p.id} aria-current={page.id === p.id ? 'page' : undefined} title={p.label} aria-label={p.label}><Icon name={p.icon} size={18} /></a>
    {/each}
  </nav>
  <main class:bleed={page.id === 'dashboard'}>
    {#key version}
    {#if page.id === 'dashboard'}<Dashboard />
    {:else if page.id === 'topology'}<Topology />
    {:else if page.id === 'devices'}<Devices tab={r[1] || 'list'} />
    {:else if page.id === 'clients'}<Clients selected={r[1]} />
    {:else if page.id === 'events'}<Events tab={r[1] || 'events'} />
    {:else if page.id === 'insights'}<Insights tab={r[1] || 'apps'} />
    {:else if page.id === 'hotspot'}<Hotspot tab={r[1] || 'portal'} />
    {:else if page.id === 'settings'}<Settings section={r[1] || 'wifi'} />{/if}
    {/key}
  </main>
</div>

<style>
  .appbar { position: sticky; top: 0; z-index: 15; height: 46px; display: flex; align-items: center; gap: 10px; background: var(--panel); border-bottom: 1px solid var(--line); padding-right: 12px; }
  .logo { width: 46px; height: 46px; display: grid; place-items: center; font: 800 13px var(--font); color: var(--text); letter-spacing: -.03em; border-right: 1px solid var(--line); }
  .apps { display: flex; height: 100%; }
  .app { display: flex; align-items: center; gap: 7px; padding: 0 14px; color: var(--muted); font-weight: 550; border-right: 1px solid var(--line); }
  .app:hover { color: var(--text); }
  .app.on { color: var(--accent); background: var(--bg); }
  .ring { width: 14px; height: 14px; border-radius: 50%; border: 3px solid var(--accent); box-sizing: border-box; }
  .mock { margin-left: 4px; }
  .right { margin-left: auto; gap: 6px; }
  .btn.on { color: var(--accent); }
  .site { display: inline-flex; align-items: center; gap: 8px; padding: 5px 12px; border-radius: 99px; background: var(--bg); border: 1px solid var(--line); font-size: 12px; font-weight: 550; }
  .led { width: 16px; height: 4px; border-radius: 2px; background: var(--muted); }
  button.avatar { border: 0; cursor: pointer; }
  .avatar { width: 28px; height: 28px; border-radius: 50%; display: grid; place-items: center; background: linear-gradient(135deg, var(--accent), var(--accent2)); color: #fff; font-weight: 700; font-size: 12px; }
  .shell { display: grid; grid-template-columns: 46px minmax(0, 1fr); min-height: calc(100vh - 46px); }
  .rail { position: sticky; top: 46px; height: calc(100vh - 46px); display: flex; flex-direction: column; align-items: center; gap: 4px; padding: 10px 0; border-right: 1px solid var(--line); background: var(--panel); }
  .rail a { width: 34px; height: 34px; display: grid; place-items: center; border-radius: 8px; color: var(--muted); }
  .rail a:hover { color: var(--text); background: var(--panel2); }
  .rail a.on { color: var(--accent); }
  .grow { flex: 1; }
  main { padding: 16px 24px 40px; min-width: 0; }
  main.bleed { padding: 0; }
  @media (max-width: 760px) {
    .shell { grid-template-columns: minmax(0, 1fr); }
    .rail { position: fixed; bottom: 0; top: auto; height: auto; width: 100%; flex-direction: row; justify-content: space-around; border-right: 0; border-top: 1px solid var(--line); z-index: 10; }
    .grow { display: none; }
    main { padding: 12px 12px 80px; }
    main.bleed { padding-bottom: 60px; }
    .app:not(.on) { display: none; }
  }
</style>
