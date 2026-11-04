<script lang="ts">
  import { apply, confirm, keep, revert } from '../apply.svelte'
  import Modal from './Modal.svelte'
  import Icon from '../Icon.svelte'
  let now = $state(Date.now())
  $effect(() => { const i = setInterval(() => { now = Date.now(); if (apply.armed && now > apply.deadline) revert() }, 1000); return () => clearInterval(i) })
  const left = $derived(Math.max(0, Math.round((apply.deadline - now) / 1000)))
</script>

{#if apply.open}
  <Modal title="Review changes · {apply.title}" onclose={() => (apply.open = false)} wide>
    <p class="muted small">These RouterOS commands will run. A revert is armed first: if you don't confirm within 5 minutes (for example because you lost access), the router restores the previous config by itself.</p>
    <pre class="cmds">{#each apply.cmds as c, i}<span class="n">{i + 1}</span>{c}{'\n'}{/each}</pre>
    <p class="muted tiny">Mockup: nothing is sent to the router yet.</p>
    {#snippet footer()}
      <button class="btn" onclick={() => (apply.open = false)}>Cancel</button>
      <button class="btn primary" onclick={confirm}><Icon name="check" size={14} /> Apply with auto-revert</button>
    {/snippet}
  </Modal>
{/if}

{#if apply.armed}
  <div class="banner row" role="status">
    <Icon name="alert" size={16} />
    <span><b>{apply.title}</b> applied. Reverting in {Math.floor(left / 60)}:{String(left % 60).padStart(2, '0')} unless you keep it.</span>
    <button class="btn" onclick={revert}>Revert now</button>
    <button class="btn primary" onclick={keep}>Keep changes</button>
  </div>
{/if}

<style>
  .cmds { background: var(--code-bg); border: 1px solid var(--line); border-radius: 8px; padding: 10px 12px; font: 12px/1.7 var(--mono); white-space: pre-wrap; max-height: 50vh; overflow: auto; }
  .n { display: inline-block; width: 22px; color: var(--muted); user-select: none; }
  .tiny { font-size: 11.5px; }
  .banner { position: fixed; bottom: 18px; left: 50%; transform: translateX(-50%); z-index: 45; background: var(--panel); border: 1px solid var(--warn); box-shadow: var(--shadow); border-radius: 10px; padding: 10px 14px; gap: 12px; color: var(--text); }
  .banner :global(svg) { color: var(--warn); }
</style>
