<script lang="ts">
  // "Show in RouterOS": the exact commands behind a card (PLAN §0.6 rule 9)
  import { prefs } from './live.svelte'
  let { cmd }: { cmd: string } = $props()
</script>

{#if prefs.showRos}
  <pre class="ros"><span>RouterOS</span>{cmd.split(';').map((s) => s.trim()).join('\n')}</pre>
{/if}

<style>
  .ros { margin: 12px 0 0; padding: 8px 10px; background: var(--code-bg); border: 1px solid var(--line); border-radius: 6px; font: 12px/1.5 var(--mono); color: var(--text); white-space: pre-wrap; }
  span { display: block; font: 600 10px var(--font); letter-spacing: .06em; text-transform: uppercase; color: var(--muted); margin-bottom: 2px; }
</style>
