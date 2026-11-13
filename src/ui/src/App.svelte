<script lang="ts">
  import { tick as settled } from 'svelte'
  import { router, isSample, simulated, isBackend, load, clients } from './lib/data'
  import { api, logout, session, site, switchSite, type Site } from './lib/backend'
  import { apply } from './lib/apply.svelte'
  import { prefs } from './lib/live.svelte'
  import Icon from './lib/Icon.svelte'
  import Palette from './lib/Palette.svelte'
  import Dashboard from './pages/Dashboard.svelte'
  import Clients from './pages/Clients.svelte'
  import ClientDrawer from './pages/ClientDrawer.svelte'
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

  // old/short links: #/ports and #/radios live under Devices
  const alias: Record<string, string[]> = { ports: ['devices', 'ports'], radios: ['devices', 'radios'] }
  const parse = (h: string) => { const x = h.replace(/^#\/?/, '').split('?')[0].split('/'); return alias[x[0]] ?? x } // ?query is for the page itself
  const isClient = (x: string[]) => x[0] === 'clients' && !!x[1] && x[1] !== 'offline' && x[1] !== 'groups'

  // #/clients/<mac> opens a side panel over the page you came from instead of switching to Clients.
  // `under` = that page's hash, `depth` = panel entries pushed since, so ✕ goes back exactly that far.
  // Both live in history.state, so Back, Forward and reload keep the page underneath.
  type Panel = { under: string; depth: number }
  let hash = $state(location.hash)
  let panel = $state<Panel | null>(history.state?.depth ? history.state : null)
  $effect(() => {
    const on = () => {
      const was = hash
      hash = location.hash
      if (!isClient(parse(hash))) { panel = null; return }
      const s = history.state as Panel | null
      if (s?.depth) { panel = s; return }
      panel = isClient(parse(was)) && panel ? { under: panel.under, depth: panel.depth + 1 } : { under: was, depth: 1 }
      history.replaceState($state.snapshot(panel), '')
    }
    addEventListener('hashchange', on)
    return () => removeEventListener('hashchange', on)
  })
  const closePanel = () => (panel ? history.go(-panel.depth) : (location.hash = '#/clients'))
  const route = $derived(parse(hash))
  const mac = $derived(isClient(route) ? route[1] : '')
  const r = $derived(mac ? parse(panel?.under ?? '#/clients') : route) // the page shown (under the panel)
  const page = $derived(pages.find((p) => p.id === r[0]) ?? pages[0])

  // With the server: fetch a fresh snapshot every 30 s and re-mount the page with it, unless the user is
  // typing or reviewing a change (live numbers stream separately and never need this).
  let version = $state(0)
  let me = $state(session.me)
  let stale = $state('')
  $effect(() => {
    if (!isBackend) return
    let last = ''
    let rebuilt = Date.now()
    const tick = async () => {
      try {
        const s = await api('/snapshot')
        // Rebuilding the page is the expensive part. Counters change on every poll and live numbers stream
        // separately, so rebuild at once only when something besides numbers changed, else every 5 minutes.
        const json = JSON.stringify(s).replace(/\d+/g, '0')
        const due = Date.now() - rebuilt > 300_000
        stale = ''
        // don't rebuild the page under someone typing, reviewing a change or using a dialog
        const busy = apply.open || apply.armed || !!document.querySelector('[role="dialog"], details[open]') || document.activeElement?.matches('input, textarea, select')
        if ((json !== last || due) && last && !busy) {
          const y = scrollY
          load(s); version++; rebuilt = Date.now()
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
  let sites = $state<Site[]>([])
  if (isBackend) api<Site[]>('/sites').then((s) => (sites = s)).catch(() => {})
  const modeLabel =$derived(me?.readOnly ? 'Read-only' : 'Read & write')
  const modeTitle = $derived(me?.policies ? `Router user ${me.policies.user} (group ${me.policies.group}): ${me.policies.policy.join(', ')}` : '')
</script>

<!-- Layout: top app bar, icon-only rail, full-bleed content -->
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
    <button class="btn ghost" onclick={() => { prefs.auto = false; prefs.theme = prefs.theme === 'dark' ? 'light' : 'dark' }} aria-label="Toggle theme"><Icon name={prefs.theme === 'dark' ? 'sun' : 'moon'} size={15} /></button>
    {#if sites.length > 1}
      <!-- site switcher: every request then carries this router's id (lib/backend.ts) -->
      <label class="site"><span class="dot ok"></span><span class="sr">Router</span>
        <select value={site} onchange={(e) => switchSite(e.currentTarget.value)} aria-label="Router shown">
          {#each sites as s (s.id)}<option value={s.id}>{s.name}{s.identity ? ` · ${s.identity}` : ''}</option>{/each}
        </select></label>
    {:else}
      <span class="site hide-sm"><span class="dot ok"></span><span class="led"></span>{router.board?.startsWith(router.identity) ? router.board : `${router.identity} ${router.board}`}<Icon name="down" size={11} /></span>
    {/if}
    {#if isBackend}
      <button class="avatar" popovertarget="me-menu" title="Signed in as {me?.user}" aria-label="Account menu for {me?.user}">{(me?.user ?? 'a').slice(0, 1).toUpperCase()}</button>
      <div id="me-menu" class="me-menu" popover>
        <b>{me?.user}</b>
        <span class="muted small">Your role: {me?.role === 'admin' ? 'Admin' : 'Viewer'}{me?.mikrotik ? ' (MikroTik account)' : ''}</span>
        {#if me?.readOnly}<span class="muted small">Changes are off for everyone: tikontrol connects with a read-only router user.</span>{#if me?.role === 'admin'}<a class="small" href="#/settings/control">Allow changes</a>{/if}{/if}
        <button class="btn" onclick={logout}><Icon name="lock" size={14} />Sign out</button>
      </div>
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
    {:else if page.id === 'clients'}<Clients selected={r[1] ?? mac} />
    {:else if page.id === 'events'}<Events tab={r[1] || 'events'} />
    {:else if page.id === 'insights'}<Insights tab={r[1] || 'apps'} />
    {:else if page.id === 'hotspot'}<Hotspot tab={r[1] || 'portal'} />
    {:else if page.id === 'settings'}<Settings section={r[1] || 'wifi'} />{/if}
    {#each clients.filter((c) => c.mac === mac) as c (c.mac)}<ClientDrawer client={c} onclose={closePanel} />{/each}
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
  .site select { background: transparent; border: 0; color: inherit; font: inherit; padding: 0; }
  .sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
  .led { width: 16px; height: 4px; border-radius: 2px; background: var(--muted); }
  button.avatar { border: 0; cursor: pointer; }
  .me-menu { position: fixed; inset: 52px 12px auto auto; margin: 0; min-width: 200px; padding: 12px; display: none; flex-direction: column; gap: 6px; background: var(--panel); color: var(--text); border: 1px solid var(--line); border-radius: var(--radius); box-shadow: 0 8px 24px #0004; }
  .me-menu:popover-open { display: flex; }
  .me-menu .btn { margin-top: 6px; justify-content: center; }
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
