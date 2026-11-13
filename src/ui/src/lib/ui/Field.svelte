<script lang="ts">
  import type { Snippet } from 'svelte'
  let { label, help, children, na }: { label: string; help?: string; children: Snippet; na?: string } = $props()
  // The label names every control in the field that has no name of its own (WCAG 1.3.1 / 4.1.2), also ones
  // that appear later (a select shown once a toggle is on). The help text describes them.
  const id = `fld-${Math.random().toString(36).slice(2, 9)}`
  function name(ctl: HTMLElement) {
    const tag = () => ctl.querySelectorAll<HTMLElement>('input, select, textarea').forEach((el) => {
      if (!el.hasAttribute('aria-label') && !el.hasAttribute('aria-labelledby') && !(el as HTMLInputElement).labels?.length) el.setAttribute('aria-labelledby', id)
      if ((help || na) && !el.hasAttribute('aria-describedby')) el.setAttribute('aria-describedby', `${id}-h`)
    })
    tag()
    const o = new MutationObserver(tag)
    o.observe(ctl, { childList: true, subtree: true })
    return () => o.disconnect()
  }
</script>

<div class="field" class:na>
  <div><div class="lbl" {id}>{label}</div>{#if help || na}<div class="muted tiny" id="{id}-h">{na ?? help}</div>{/if}</div>
  <div class="ctl" {@attach name}>{@render children()}</div>
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
