<script lang="ts">
  // Tabs are part of the URL (#/page/tab) so they survive reloads and can be linked.
  let { tabs, active, base }: { tabs: { id: string; label: string; count?: number }[]; active: string; base: string } = $props()
</script>

<nav class="tabs">
  {#each tabs as t}<a href="#/{base}/{t.id}" class:on={t.id === active}>{t.label}{#if t.count != null}<span>{t.count}</span>{/if}</a>{/each}
</nav>

<style>
  .tabs { display: flex; gap: 2px; border-bottom: 1px solid var(--line); margin-bottom: 14px; overflow-x: auto; }
  a { padding: 8px 12px; color: var(--muted); font-weight: 550; border-bottom: 2px solid transparent; margin-bottom: -1px; white-space: nowrap; }
  a:hover { color: var(--text); }
  a.on { color: var(--text); border-color: var(--accent); }
  span { margin-left: 6px; font-size: 11px; padding: 0 6px; border-radius: 99px; background: var(--panel2); }
</style>
