<script lang="ts">
  // Our own drawing of the router's port side (not a vendor image): body colour, ports, USB and power from
  // faceOf(hw), link LEDs from the router's live port state. Unknown models get a generic box with their port count.
  import { hw, ports, raw } from './data'
  import { faceOf } from './hardware'

  let { onselect }: { onselect?: (port: string) => void } = $props()
  const f = faceOf(hw)
  const rate = (n: string) => (raw.etherMon ?? []).find((m: any) => m.name === n)?.rate
  const ink = '#6a6d72' // printed labels on the case
  // plug colour by link speed: slow links stand out, multi-gig gets the accent (CHR's virtual NICs report no rate)
  const plug = (n: string) => {
    const r = String(rate(n) ?? '')
    return /^(10|100)Mbps/.test(r) ? 'var(--warn)' : /^(2\.5|5|10|25|40|100)Gbps/.test(r) ? 'var(--accent)' : 'var(--ok)'
  }

  // left to right: DC jack, reset pinhole, USB, microSD, then ports 1…n, then the power LED
  type Part = { kind: 'dc' | 'reset' | 'A' | 'C' | 'sd'; x: number; w: number }
  const parts: Part[] = []
  let x = 16
  for (const kind of ['dc', 'reset', ...f.usb, ...(f.sd ? ['sd'] : [])] as Part['kind'][]) {
    const w = { dc: 14, reset: 10, A: 20, C: 16, sd: 16 }[kind]
    parts.push({ kind, x, w })
    x += w + 11
  }
  const P0 = x + 6, STEP = 32
  const W = P0 + Math.max(f.ports, ports.length) * STEP + 34
  const label = (p: (typeof ports)[number], i: number) =>
    `Port ${i + 1} (${p.name}): ${p.up ? `link up${rate(p.name) ? `, ${rate(p.name)}` : ''}` : 'no link'}${p.wan ? ', internet (WAN)' : ''}${i === 0 && f.poeIn ? ', PoE in' : ''}`
  const key = (e: KeyboardEvent, n: string) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onselect?.(n) } }
</script>

<svg class="face" viewBox="0 0 {W} 66" role={onselect ? 'group' : 'img'} aria-label="{hw.board ?? 'Router'}, port side{f.known ? '' : ' (generic drawing)'}">
  <!-- declared inside <svg> so its elements get the SVG namespace -->
  {#snippet jack(p: (typeof ports)[number], i: number, px: number)}
    <title>{label(p, i)}</title>
    <!-- RJ45 socket: dark opening with the clip notch at the bottom, link LED top-left, second LED top-right -->
    <path class="jack" d="M{px} 19h26v17h-8v3h-10v-3h-8z" fill="#1f2124" />
    <rect x={px + 2} y="20.5" width="5" height="2.5" rx="0.5" fill={p.up ? 'var(--ok)' : '#4a4d52'} />
    <rect x={px + 19} y="20.5" width="5" height="2.5" rx="0.5" fill="#4a4d52" />
    {#if p.up}
      <!-- plugged in: the plug's boot fills the opening, its cable runs down out of the socket -->
      <g class="plug" fill={plug(p.name)}>
        <rect x={px + 2} y="24.5" width="22" height="10" rx="2" />
        <rect x={px + 5} y="34" width="16" height="3.5" rx="1" opacity="0.85" />
        <rect x={px + 9.5} y="37" width="7" height="5" rx="1.5" />
        <rect x={px + 6} y="27" width="14" height="1.5" rx="0.75" fill="#00000033" />
      </g>
    {/if}
    <text x={px + 13} y="48" fill={p.wan ? 'var(--accent)' : ink} font-weight={p.wan ? 700 : 500}>{i + 1}{p.wan ? ' WAN' : ''}</text>
    {#if i === 0 && f.poeIn}<text x={px + 13} y="56" fill={ink}>PoE in</text>{/if}
  {/snippet}
  <rect x="2" y="6" width={W - 4} height="56" rx="12" fill={f.body} stroke="var(--line)" />
  <rect x="8" y="55" width={W - 16} height="4" rx="2" fill="#0000000d" />
  {#each parts as p}
    {#if p.kind === 'dc'}<circle cx={p.x + 7} cy="29" r="7" fill="#2a2c30" /><circle cx={p.x + 7} cy="29" r="2" fill="#8a8d92" /><text x={p.x + 7} y="48" fill={ink}>DC</text>
    {:else if p.kind === 'reset'}<circle cx={p.x + 5} cy="29" r="2" fill="#2a2c30" /><text x={p.x + 5} y="48" fill={ink}>RES</text>
    {:else if p.kind === 'A'}<rect x={p.x} y="24" width="20" height="10" rx="1.5" fill="#2a2c30" /><rect x={p.x + 3} y="26" width="14" height="3" fill="#3d7bd9" /><text x={p.x + 10} y="48" fill={ink}>USB</text>
    {:else if p.kind === 'C'}<rect x={p.x + 1} y="26" width="14" height="6" rx="3" fill="#2a2c30" /><text x={p.x + 8} y="48" fill={ink}>USB-C</text>
    {:else}<rect x={p.x} y="28" width="16" height="3" rx="1" fill="#2a2c30" /><text x={p.x + 8} y="48" fill={ink}>SD</text>{/if}
  {/each}
  {#if f.speed}<text x={P0 + 13} y="15" fill={ink}>{f.speed}</text>{/if}
  {#each ports as p, i}
    {@const px = P0 + i * STEP}
    {#if onselect}
      <g class="port" role="button" tabindex="0" aria-label={label(p, i)} onclick={() => onselect(p.name)} onkeydown={(e) => key(e, p.name)}>{@render jack(p, i, px)}</g>
    {:else}
      <g class="port">{@render jack(p, i, px)}</g>
    {/if}
  {/each}
  <circle cx={W - 20} cy="29" r="2.5" fill="var(--ok)" /><text x={W - 20} y="48" fill={ink}>PWR</text>
</svg>

<style>
  .face { width: 100%; height: auto; display: block; }
  text { font: 500 6.5px system-ui, sans-serif; text-anchor: middle; }
  .port { outline: none; }
  .port[role='button'] { cursor: pointer; }
  .port:hover .jack, .port:focus-visible .jack { stroke: var(--accent); stroke-width: 2; }
</style>
