<script lang="ts">
  let { parts, value, label, size = 120 }: { parts: { v: number; color: string }[]; value: string; label: string; size?: number } = $props()
  const R = 46, C = 2 * Math.PI * R, GAP = 2.5
  const segs = $derived.by(() => {
    const total = parts.reduce((s, p) => s + p.v, 0) || 1
    let off = 0
    return parts.map((p) => {
      const len = Math.max(0, (p.v / total) * C - (parts.length > 1 ? GAP : 0))
      const s = { len, off, color: p.color }
      off += (p.v / total) * C
      return s
    })
  })
</script>

<div class="donut" style="width:{size}px;height:{size}px">
  <svg viewBox="0 0 110 110" width={size} height={size}>
    <circle cx="55" cy="55" r={R} fill="none" stroke="var(--line)" stroke-width="7" />
    {#each segs as s}
      <circle cx="55" cy="55" r={R} fill="none" stroke={s.color} stroke-width="7" stroke-dasharray="{s.len} {C}" stroke-dashoffset={-s.off} transform="rotate(-90 55 55)" />
    {/each}
  </svg>
  <div class="center"><b>{value}</b><span>{label}</span></div>
</div>

<style>
  .donut { position: relative; flex: none; }
  .center { position: absolute; inset: 0; display: grid; place-content: center; text-align: center; }
  b { font-size: 15px; font-weight: 650; }
  span { font-size: 10.5px; color: var(--muted); }
</style>
