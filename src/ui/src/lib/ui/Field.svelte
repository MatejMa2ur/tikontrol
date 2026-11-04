<script lang="ts">
  import type { Snippet } from 'svelte'
  let { label, help, children, na }: { label: string; help?: string; children: Snippet; na?: string } = $props()
</script>

<div class="field" class:na>
  <div><div class="lbl">{label}</div>{#if help || na}<div class="muted tiny">{na ?? help}</div>{/if}</div>
  <div class="ctl">{@render children()}</div>
</div>

<style>
  .field { display: grid; grid-template-columns: minmax(160px, 1fr) minmax(0, 1.4fr); gap: 16px; align-items: center; padding: 10px 0; border-top: 1px solid var(--line); }
  .field:first-of-type { border-top: 0; }
  .lbl { font-weight: 550; }
  .tiny { font-size: 11.5px; margin-top: 2px; }
  .ctl { display: flex; gap: 8px; align-items: center; justify-content: flex-end; flex-wrap: wrap; }
  .ctl :global(input[type='text']), .ctl :global(input[type='number']), .ctl :global(input[type='password']), .ctl :global(select) { width: 100%; max-width: 300px; }
  .na { opacity: .55; }
  @media (max-width: 700px) { .field { grid-template-columns: 1fr; gap: 6px; } .ctl { justify-content: flex-start; } }
</style>
