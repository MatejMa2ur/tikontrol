<script lang="ts">
  import type { Snippet } from 'svelte'
  import Ros from '../Ros.svelte'
  let { title, desc, actions, children, ros, id }: { title: string; desc?: string; actions?: Snippet; children?: Snippet; ros?: string; id?: string } = $props()
</script>

<section class="card sec" {id}>
  <header class="row between">
    <div><h3>{title}</h3>{#if desc}<p class="muted small">{desc}</p>{/if}</div>
    {#if actions}<div class="row">{@render actions()}</div>{/if}
  </header>
  {@render children?.()}
  {#if ros}<Ros cmd={ros} />{/if}
</section>

<style>
  .sec { padding: 16px 18px; }
  header { margin-bottom: 12px; gap: 12px; align-items: flex-start; }
  h3 { margin: 0; font-size: 13.5px; font-weight: 650; }
  p { margin: 3px 0 0; max-width: 70ch; }
</style>
