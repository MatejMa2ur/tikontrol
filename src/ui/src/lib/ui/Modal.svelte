<script lang="ts">
  import type { Snippet } from 'svelte'
  import Icon from '../Icon.svelte'
  let { title, onclose, children, footer, wide = false }: { title: string; onclose: () => void; children: Snippet; footer?: Snippet; wide?: boolean } = $props()
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && onclose()} />
<button class="scrim" aria-label="Close" onclick={onclose}></button>
<div class="modal" class:wide role="dialog" aria-modal="true" aria-label={title}>
  <header class="row between"><h3>{title}</h3><button class="btn ghost" onclick={onclose} aria-label="Close"><Icon name="close" /></button></header>
  <div class="body">{@render children()}</div>
  {#if footer}<footer class="row">{@render footer()}</footer>{/if}
</div>

<style>
  .scrim { position: fixed; inset: 0; background: #0008; border: 0; z-index: 40; }
  .modal { position: fixed; top: 50%; left: 50%; transform: translate(-50%, -50%); width: min(560px, 94vw); max-height: 88vh; display: flex; flex-direction: column; background: var(--panel); border: 1px solid var(--line); border-radius: 12px; box-shadow: var(--shadow); z-index: 41; }
  .wide { width: min(860px, 96vw); }
  header { padding: 12px 12px 4px 18px; }
  h3 { margin: 0; font-size: 15px; }
  .body { padding: 6px 18px 16px; overflow-y: auto; }
  footer { padding: 12px 18px; border-top: 1px solid var(--line); justify-content: flex-end; }
</style>
