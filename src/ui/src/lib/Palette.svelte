<script lang="ts">
  // ⌘K / Ctrl+K: jump to any page, settings page, client, device or SSID
  import { clients, ssids, router, neighbors } from './data'
  import { displayName } from './live.svelte'
  import Icon from './Icon.svelte'

  let { pages }: { pages: { id: string; label: string; icon: string }[] } = $props()
  let open = $state(false)
  let q = $state('')
  let idx = $state(0)

  // ponytail: mirrors the tabs in pages/Settings.svelte; keep in sync when a settings page is added
  const settings = [['wifi', 'WiFi', 'wifi'], ['networks', 'Networks', 'topology'], ['internet', 'Internet', 'globe'], ['vpn', 'VPN', 'vpn'], ['security', 'Security', 'shield'], ['routing', 'Routing', 'route'], ['profiles', 'Profiles', 'gauge'], ['control', 'Control Plane', 'key'], ['system', 'System', 'settings']]
  const items = $derived(
    [
      ...pages.map((p) => ({ label: p.label, hint: 'Page', icon: p.icon, href: `#/${p.id}`, extra: '' })),
      ...settings.map(([id, label, icon]) => ({ label: `Settings › ${label}`, hint: 'Settings', icon, href: `#/settings/${id}`, extra: '' })),
      ...clients.map((c) => ({ label: displayName(c), hint: c.ip, icon: c.kind, href: `#/clients/${c.mac}`, extra: `${c.mac} ${c.name}` })),
      { label: router.identity, hint: `Device · ${router.board}`, icon: 'devices', href: '#/device/router', extra: router.model },
      ...neighbors.map((n) => ({ label: String(n.identity ?? n['mac-address'] ?? ''), hint: `Device · ${n.address ?? ''}`, icon: 'devices', href: '#/devices/list', extra: `${n.board ?? ''} ${n['mac-address'] ?? ''}` })),
      ...ssids.map((s) => ({ label: s.ssid, hint: `SSID · ${s.clients} clients`, icon: 'wifi', href: '#/settings/wifi', extra: '' })),
    ].filter((i) => !q || `${i.label} ${i.hint} ${i.extra}`.toLowerCase().includes(q.toLowerCase())).slice(0, 9),
  )
  const go = (href: string) => { location.hash = href; open = false; q = '' }
  function key(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === 'k') { e.preventDefault(); open = !open; idx = 0 }
    if (!open) return
    if (e.key === 'Escape') open = false
    if (e.key === 'ArrowDown') { e.preventDefault(); idx = Math.min(idx + 1, items.length - 1) }
    if (e.key === 'ArrowUp') { e.preventDefault(); idx = Math.max(idx - 1, 0) }
    if (e.key === 'Enter' && items[idx]) go(items[idx].href)
  }
</script>

<svelte:window onkeydown={key} />
<button class="btn ghost trigger" onclick={() => (open = true)}><Icon name="search" size={14} /> <span class="hide-sm">Search <kbd>⌘K</kbd></span></button>

{#if open}
  <button class="scrim" aria-label="Close search" onclick={() => (open = false)}></button>
  <div class="pal" role="dialog" aria-label="Search">
    <!-- svelte-ignore a11y_autofocus -->
    <input type="text" placeholder="Search clients, devices, SSIDs, settings…" bind:value={q} oninput={() => (idx = 0)} autofocus />
    {#each items as it, i}
      <button class="item row" class:on={i === idx} onmouseenter={() => (idx = i)} onclick={() => go(it.href)}>
        <Icon name={it.icon} size={15} /> <span>{it.label}</span> <span class="muted small hint">{it.hint}</span>
      </button>
    {/each}
  </div>
{/if}

<style>
  .trigger { border: 1px solid var(--line); }
  kbd { font: 11px var(--mono); padding: 0 5px; border: 1px solid var(--line); border-radius: 4px; }
  .scrim { position: fixed; inset: 0; background: #0007; border: 0; z-index: 30; }
  .pal { position: fixed; top: 14vh; left: 50%; transform: translateX(-50%); width: min(520px, 92vw); background: var(--panel); border: 1px solid var(--line); border-radius: 12px; box-shadow: var(--shadow); z-index: 31; padding: 8px; }
  .pal input { width: 100%; font-size: 15px; padding: 10px 12px; margin-bottom: 6px; }
  .item { width: 100%; text-align: left; background: none; border: 0; padding: 8px 10px; border-radius: 7px; cursor: pointer; }
  .item.on { background: var(--panel2); }
  .hint { margin-left: auto; }
</style>
