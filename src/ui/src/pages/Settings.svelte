<script lang="ts">
  import Icon from '../lib/Icon.svelte'
  import Wifi from './settings/Wifi.svelte'
  import Networks from './settings/Networks.svelte'
  import Internet from './settings/Internet.svelte'
  import Vpn from './settings/Vpn.svelte'
  import Security from './settings/Security.svelte'
  import Routing from './settings/Routing.svelte'
  import Profiles from './settings/Profiles.svelte'
  import ControlPlane from './settings/ControlPlane.svelte'
  import System from './settings/System.svelte'

  let { section = 'wifi' }: { section?: string } = $props()
  const nav = [
    { id: 'wifi', label: 'WiFi', icon: 'wifi', c: Wifi },
    { id: 'networks', label: 'Networks', icon: 'topology', c: Networks },
    { id: 'internet', label: 'Internet', icon: 'globe', c: Internet },
    { id: 'vpn', label: 'VPN', icon: 'vpn', c: Vpn },
    { id: 'security', label: 'Security', icon: 'shield', c: Security },
    { id: 'routing', label: 'Routing', icon: 'route', c: Routing },
    { id: 'profiles', label: 'Profiles', icon: 'gauge', c: Profiles },
    { id: 'control', label: 'Control Plane', icon: 'key', c: ControlPlane },
    { id: 'system', label: 'System', icon: 'settings', c: System },
  ]
  const cur = $derived(nav.find((n) => n.id === section) ?? nav[0])
</script>

<div class="settings">
  <nav aria-label="Settings">
    <h1>Settings</h1>
    {#each nav as n}<a href="#/settings/{n.id}" class:on={n.id === cur.id}><Icon name={n.icon} size={15} /> {n.label}</a>{/each}
  </nav>
  <div class="content grid">
    <h2 class="h">{cur.label}</h2>
    <cur.c />
  </div>
</div>

<style>
  .settings { display: grid; grid-template-columns: 200px minmax(0, 1fr); gap: 24px; max-width: 1180px; }
  nav { display: grid; gap: 2px; align-content: start; position: sticky; top: 62px; }
  h1 { margin: 0 0 12px 8px; }
  nav a { display: flex; gap: 9px; align-items: center; padding: 8px 10px; border-radius: 8px; color: var(--muted); font-weight: 550; }
  nav a:hover { color: var(--text); background: var(--panel2); }
  nav a.on { color: var(--text); background: color-mix(in srgb, var(--accent) 14%, transparent); }
  nav a.on :global(svg) { color: var(--accent); }
  .h { font-size: 18px; color: var(--text); margin: 4px 0 0; }
  @media (max-width: 800px) { .settings { grid-template-columns: minmax(0, 1fr); } nav { position: static; grid-auto-flow: column; overflow-x: auto; } h1 { display: none; } }
</style>
