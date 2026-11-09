<script lang="ts">
  // Internet Activity: client count as a stepped "cell" area (left axis) + down/up throughput lines (right axis)
  import uPlot from 'uplot'
  import 'uplot/dist/uPlot.min.css'
  import { untrack } from 'svelte'
  import { bps } from './data'

  type D = { t: number[]; clients: number[]; rx: number[]; tx: number[] }
  let { data, mode = 'all', height = 420 }: { data: D; mode?: 'all' | 'down' | 'up'; height?: number } = $props()
  let el: HTMLDivElement
  let plot: uPlot | undefined
  const css = (v: string) => getComputedStyle(document.documentElement).getPropertyValue(v).trim()
  const CELL = 13 // px; size of the grid cells drawn through the client area

  $effect(() => {
    const font = '11px system-ui, sans-serif'
    const line = (label: string, color: string): uPlot.Series => ({ label, scale: 'b', stroke: color, width: 1.4, fill: color + '30', points: { show: false } })
    plot = new uPlot(
      {
        width: el.clientWidth, height,
        legend: { show: false },
        cursor: { y: false, points: { show: false } },
        scales: { x: { time: true }, c: { range: (_u, _min, max) => [0, Math.max(5, Math.ceil(max * 1.6 / 5) * 5)] }, b: { range: (_u, _min, max) => [0, Math.max(max * 1.1, 1000)] } },
        axes: [
          { stroke: css('--muted'), grid: { show: false }, ticks: { show: false }, font },
          { scale: 'c', stroke: css('--muted'), grid: { stroke: css('--line'), width: 1 }, ticks: { show: false }, font, size: 40, label: 'Clients', labelSize: 16, labelFont: font },
          { scale: 'b', side: 1, stroke: css('--muted'), grid: { show: false }, ticks: { show: false }, font, size: 70, values: (_u, v) => v.map(bps) },
        ],
        series: [
          {},
          { label: 'Clients', scale: 'c', stroke: 'transparent', fill: css('--cells'), paths: uPlot.paths.stepped!({ align: 1 }), points: { show: false } },
          line('Download', css('--down')),
          line('Upload', css('--up')),
        ],
        hooks: {
          // Cut the client area into cells by drawing background-coloured grid lines right after it.
          drawSeries: [(u, i) => {
            if (i !== 1) return
            const { ctx, bbox } = u, s = CELL * devicePixelRatio
            ctx.save()
            ctx.strokeStyle = css('--panel'); ctx.lineWidth = devicePixelRatio
            ctx.beginPath()
            for (let x = bbox.left; x < bbox.left + bbox.width; x += s) { ctx.moveTo(x, bbox.top); ctx.lineTo(x, bbox.top + bbox.height) }
            for (let y = bbox.top + bbox.height; y > bbox.top; y -= s) { ctx.moveTo(bbox.left, y); ctx.lineTo(bbox.left + bbox.width, y) }
            ctx.stroke(); ctx.restore()
          }],
        },
      },
      untrack(() => [data.t, data.clients, data.rx, data.tx]),
      el,
    )
    const ro = new ResizeObserver(() => plot?.setSize({ width: el.clientWidth, height }))
    ro.observe(el)
    return () => { ro.disconnect(); plot?.destroy() }
  })
  $effect(() => { plot?.setData([data.t, data.clients, data.rx, data.tx]) })
  $effect(() => { plot?.setSeries(2, { show: mode !== 'up' }); plot?.setSeries(3, { show: mode !== 'down' }) })
</script>

<div bind:this={el}></div>
