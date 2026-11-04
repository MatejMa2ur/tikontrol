<script lang="ts">
  let { checked = $bindable(false), disabled = false, label = '' }: { checked?: boolean; disabled?: boolean; label?: string } = $props()
</script>

<button type="button" role="switch" aria-checked={checked} aria-label={label} class="sw" class:on={checked} {disabled} onclick={() => (checked = !checked)}><i></i></button>

<style>
  .sw { width: 34px; height: 20px; border-radius: 99px; border: 0; background: var(--line); position: relative; cursor: pointer; flex: none; transition: background .15s; }
  .sw i { position: absolute; top: 3px; left: 3px; width: 14px; height: 14px; border-radius: 50%; background: #fff; transition: transform .15s; box-shadow: 0 1px 2px #0004; }
  .sw.on { background: var(--accent-bg); }
  .sw.on i { transform: translateX(14px); }
  .sw:disabled { opacity: .45; cursor: not-allowed; }
</style>
