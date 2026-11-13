<script lang="ts">
  import { apply, confirm, keep, revert } from '../apply.svelte'
  import { isBackend } from '../data'
  import Modal from './Modal.svelte'
  import Icon from '../Icon.svelte'
  let now = $state(Date.now())
  // with the server the router reverts by itself; locally we only stop showing the banner
  $effect(() => { const i = setInterval(() => { now = Date.now(); if (apply.armed && now > apply.deadline) apply.armed = false }, 1000); return () => clearInterval(i) })
  const left = $derived(Math.max(0, Math.round((apply.deadline - now) / 1000)))
  const p = $derived(apply.preview)
  const manual = $derived(isBackend && !apply.intent)
</script>

<!-- closing the tab while a change waits for Keep: the router undoes it at the deadline, so warn first -->
<svelte:window onbeforeunload={(e) => { if (apply.armed) e.preventDefault() }} />

{#if apply.open}
  <Modal title="Review changes · {apply.title}" onclose={() => (apply.open = false)} wide>
    {#if manual}
      <p class="muted small">tikontrol can't apply this change itself yet. These are the RouterOS commands to run in a terminal (WinBox → New Terminal); check them first.</p>
    {:else}
      <p class="muted small">These RouterOS commands will run. A revert is armed first: if you don't confirm within 5 minutes (for example because you lost access), the router restores the previous config by itself.</p>
    {/if}
    {#if p?.risk === 'high'}<p class="pill bad" role="alert">High risk</p>{/if}
    {#each p?.warnings ?? [] as w}<p class="warn small" role="note"><Icon name="alert" size={13} /> {w}</p>{/each}
    <pre class="cmds">{#each apply.cmds as c, i}<span class="n">{i + 1}</span>{c}{'\n'}{/each}</pre>
    {#if p?.undo?.length}<details><summary class="muted small">Undo commands (run by the router if you don't confirm)</summary><pre class="cmds">{p.undo.join('\n')}</pre></details>{/if}
    {#if apply.error}<p class="err" role="alert">{apply.error}</p>{/if}
    {#if p && !p.canApply}<p class="muted small" role="note"><Icon name="lock" size={13} /> {p.reason}</p>{/if}
    {#if !isBackend}<p class="muted tiny">Mockup: nothing is sent to the router.</p>{/if}
    {#snippet footer()}
      <button class="btn" onclick={() => (apply.open = false)}>{manual || (p && !p.canApply) ? 'Close' : 'Cancel'}</button>
      {#if manual}
        <button class="btn" onclick={() => navigator.clipboard?.writeText(apply.cmds.join('\n'))}><Icon name="code" size={14} /> Copy commands</button>
      {:else}
        <button class="btn primary" onclick={confirm} disabled={apply.busy || (isBackend && (!p || !p.canApply))} title={p && !p.canApply ? (p.reason ?? '') : ''}><Icon name="check" size={14} /> {p && !p.revertible ? 'Apply' : 'Apply with auto-revert'}</button>
      {/if}
    {/snippet}
  </Modal>
{/if}

{#if apply.armed}
  <div class="banner row" role="status">
    <Icon name="alert" size={16} />
    <span><b>{apply.title}</b> applied. Reverting in {Math.floor(left / 60)}:{String(left % 60).padStart(2, '0')} unless you keep it.</span>
    {#if apply.error}<span class="err">{apply.error}</span>{/if}
    {#if apply.notice}<span class="note" role="alert">{apply.notice}</span>{/if}
    <button class="btn" onclick={revert}>Revert now</button>
    <button class="btn primary" onclick={keep}>Keep changes</button>
  </div>
{/if}

<style>
  .cmds { background: var(--code-bg); border: 1px solid var(--line); border-radius: 8px; padding: 10px 12px; font: 12px/1.7 var(--mono); white-space: pre-wrap; max-height: 50vh; overflow: auto; }
  .n { display: inline-block; width: 22px; color: var(--muted); user-select: none; }
  .tiny { font-size: 11.5px; }
  .warn { color: var(--warn); margin: 4px 0; display: flex; gap: 6px; align-items: flex-start; }
  .err { color: var(--bad); }
  .note { color: var(--warn); font-weight: 600; }
  .banner { position: fixed; bottom: 18px; left: 50%; transform: translateX(-50%); z-index: 45; background: var(--panel); border: 1px solid var(--warn); box-shadow: var(--shadow); border-radius: 10px; padding: 10px 14px; gap: 12px; color: var(--text); }
  .banner :global(svg) { color: var(--warn); }
</style>
