// Wi-Fi coverage prediction for the floor plan on the Radios page. A deliberately simple, explainable model:
//
//   RSSI = Ptx + Gant − (FSPL(1 m, f) + 10·n·log10(d)) − Σ wall losses
//
// log-distance path loss with a free-space reference at 1 m, the exponent n set by the user (open plan ≈ 2.0,
// office/home ≈ 2.5, dense ≈ 3.0) and a fixed loss for every wall the straight line from the AP crosses.
// It predicts the downlink signal a client with a 0 dBi antenna would see. No reflections, no floors above or
// below, no furniture or people: calibrate n against a few measured points (fitN) before trusting it.
// Plain data and maths, no imports, so node can load it for tests/coverage.test.ts and a Web Worker can run it.

export type Band = '2.4' | '5' | '6'
export const BANDS: Band[] = ['2.4', '5', '6']
/** Centre frequency used per band, MHz (channel 6, channel 100, 6 GHz UNII-5/7 middle). */
export const BAND_MHZ: Record<Band, number> = { '2.4': 2437, '5': 5500, '6': 6525 }
export const DEFAULT_TX: Record<Band, number> = { '2.4': 20, '5': 23, '6': 23 }
export const DEFAULT_GAIN = 2 // dBi, typical internal antenna of a home router or ceiling AP
export const DEFAULT_N = 2.5
export const DEFAULT_CELL = 0.5 // metres
export const PRESETS = [
  { n: 2.0, label: 'Open plan' },
  { n: 2.5, label: 'Office / home' },
  { n: 3.0, label: 'Dense (many rooms)' },
]

export type Material = 'drywall' | 'glass' | 'wood' | 'brick' | 'concrete' | 'metal'
/**
 * Loss per wall crossing, dB. Approximate typical planning values for a single wall at normal incidence, in
 * line with published measurement summaries (e.g. ITU-R P.2040 material data and common vendor planning
 * tables); real walls vary widely (thickness, moisture, rebar, coated "low-e" glass can reach 25+ dB).
 * Slightly higher at 5 and 6 GHz. Adjust with measurements rather than trusting these numbers.
 */
export const MATERIALS: Record<Material, { label: string; loss: Record<Band, number> }> = {
  drywall: { label: 'Drywall', loss: { '2.4': 3, '5': 4, '6': 4 } },
  glass: { label: 'Glass', loss: { '2.4': 2, '5': 3, '6': 3 } },
  wood: { label: 'Wood', loss: { '2.4': 4, '5': 5, '6': 6 } },
  brick: { label: 'Brick', loss: { '2.4': 8, '5': 10, '6': 12 } },
  concrete: { label: 'Concrete', loss: { '2.4': 12, '5': 15, '6': 17 } },
  metal: { label: 'Metal', loss: { '2.4': 20, '5': 24, '6': 26 } },
}
export const MATERIAL_KEYS = Object.keys(MATERIALS) as Material[]

/** Signal levels for the legend and summary, strongest first. Below the last one: no usable coverage. */
export const LEVELS = [
  { min: -55, key: 'excellent', label: 'Excellent' },
  { min: -67, key: 'good', label: 'Good' },
  { min: -75, key: 'fair', label: 'Fair' },
  { min: -85, key: 'poor', label: 'Poor' },
] as const
export const NONE = LEVELS.length // index of "below the last level"
export function levelOf(dbm: number): number {
  const i = LEVELS.findIndex((l) => dbm >= l.min)
  return i < 0 ? NONE : i
}

// ---------- geometry. Plans store points in percent (x of the width, y of the height). ----------
// One "plan unit" is 1 % of the image width; y in units is y% × aspect (height/width), so distances are
// isotropic and metres = units × mpu (metres per plan unit, from the reference line).
export type Pt = { x: number; y: number }
export type Wall = { x1: number; y1: number; x2: number; y2: number; m: Material }

/** Metres per plan unit from a reference line drawn on the plan (percent coordinates) and its real length. */
export function scaleFrom(line: { x1: number; y1: number; x2: number; y2: number }, meters: number, aspect: number): number | null {
  const u = Math.hypot(line.x2 - line.x1, (line.y2 - line.y1) * aspect)
  return u > 0 && meters > 0 ? meters / u : null
}

/** Free-space path loss at distance d (metres) and frequency f (MHz), dB. */
export function fspl(dM: number, mhz: number): number {
  return 20 * Math.log10(dM) + 20 * Math.log10(mhz) - 27.55
}

const orient = (ax: number, ay: number, bx: number, by: number, cx: number, cy: number) => (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
/** True when segment a-b properly crosses segment c-d (touching an end or running along it does not count). */
export function crosses(ax: number, ay: number, bx: number, by: number, cx: number, cy: number, dx: number, dy: number): boolean {
  const d1 = orient(cx, cy, dx, dy, ax, ay), d2 = orient(cx, cy, dx, dy, bx, by)
  if (!((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0))) return false
  const d3 = orient(ax, ay, bx, by, cx, cy), d4 = orient(ax, ay, bx, by, dx, dy)
  return (d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0)
}

/** Walls in metres with their loss for one band, packed for the inner loop: x1 y1 x2 y2 loss per wall. */
export function packWalls(walls: Wall[], band: Band, mpu: number, aspect: number): Float64Array {
  const w = new Float64Array(walls.length * 5)
  walls.forEach((s, i) => w.set([s.x1 * mpu, s.y1 * aspect * mpu, s.x2 * mpu, s.y2 * aspect * mpu, MATERIALS[s.m]?.loss[band] ?? 0], i * 5))
  return w
}

/** Total wall loss on the straight path a-b (metres). */
export function pathWallLoss(ax: number, ay: number, bx: number, by: number, w: Float64Array): number {
  let loss = 0
  for (let i = 0; i < w.length; i += 5) if (crosses(ax, ay, bx, by, w[i], w[i + 1], w[i + 2], w[i + 3])) loss += w[i + 4]
  return loss
}

/** An AP for one band, position in metres. */
export type Source = { x: number; y: number; tx: number; gain: number }
export type Model = { band: Band; n: number }

/** Predicted RSSI (dBm) at (px, py) metres from one source. Inside 1 m the 1 m value is used. */
export function rssi(s: Source, px: number, py: number, w: Float64Array, m: Model): number {
  const d = Math.max(1, Math.hypot(px - s.x, py - s.y))
  return s.tx + s.gain - (fspl(1, BAND_MHZ[m.band]) + 10 * m.n * Math.log10(d)) - pathWallLoss(s.x, s.y, px, py, w)
}

/** Strongest source at a point: { dbm, ap } (ap = index into sources, -1 when there are none). */
export function best(sources: Source[], px: number, py: number, w: Float64Array, m: Model): { dbm: number; ap: number } {
  let dbm = -Infinity, ap = -1
  sources.forEach((s, i) => { const v = rssi(s, px, py, w, m); if (v > dbm) { dbm = v; ap = i } })
  return { dbm, ap }
}

export type Input = {
  mpu: number; aspect: number; band: Band; n: number; cell: number
  sources: Source[] // metres
  walls: Wall[] // percent
  maxCells?: number
}
export type Grid = { cols: number; rows: number; cell: number; widthM: number; heightM: number; dbm: Float32Array; ap: Int8Array }

export const MAX_CELLS = 30_000

/** Cell size actually used: the asked size, made coarser when the plan would need more than maxCells cells. */
export function cellFor(widthM: number, heightM: number, cell: number, maxCells = MAX_CELLS): number {
  return Math.max(cell, Math.sqrt((widthM * heightM) / maxCells))
}

/** Predict the whole plan on a grid; each cell holds the strongest source's RSSI at its centre. */
export function predict(inp: Input): Grid {
  const widthM = 100 * inp.mpu, heightM = 100 * inp.aspect * inp.mpu
  const cell = cellFor(widthM, heightM, inp.cell, inp.maxCells)
  const cols = Math.max(1, Math.ceil(widthM / cell)), rows = Math.max(1, Math.ceil(heightM / cell))
  const w = packWalls(inp.walls, inp.band, inp.mpu, inp.aspect)
  const m = { band: inp.band, n: inp.n }
  const dbm = new Float32Array(cols * rows).fill(-Infinity), ap = new Int8Array(cols * rows).fill(-1)
  for (let r = 0; r < rows; r++) {
    const py = (r + 0.5) * cell
    for (let c = 0; c < cols; c++) {
      const b = best(inp.sources, (c + 0.5) * cell, py, w, m)
      dbm[r * cols + c] = b.dbm; ap[r * cols + c] = b.ap
    }
  }
  return { cols, rows, cell, widthM, heightM, dbm, ap }
}

/**
 * The grid k times finer by bilinear interpolation between cell centres, for drawing: level boundaries become
 * smooth curves instead of cell-sized steps. Cells without any AP (-Infinity) are treated as -200 dBm.
 */
export function upsample(g: Pick<Grid, 'cols' | 'rows' | 'dbm'>, k: number): { cols: number; rows: number; dbm: Float32Array } {
  const cols = g.cols * k, rows = g.rows * k, out = new Float32Array(cols * rows)
  const v = (c: number, r: number) => Math.max(-200, g.dbm[Math.min(g.rows - 1, Math.max(0, r)) * g.cols + Math.min(g.cols - 1, Math.max(0, c))])
  for (let y = 0; y < rows; y++) {
    const fy = (y + 0.5) / k - 0.5, r0 = Math.floor(fy), ty = fy - r0
    for (let x = 0; x < cols; x++) {
      const fx = (x + 0.5) / k - 0.5, c0 = Math.floor(fx), tx = fx - c0
      const top = v(c0, r0) * (1 - tx) + v(c0 + 1, r0) * tx, bottom = v(c0, r0 + 1) * (1 - tx) + v(c0 + 1, r0 + 1) * tx
      out[y * cols + x] = top * (1 - ty) + bottom * ty
    }
  }
  return { cols, rows, dbm: out }
}

/** Share of cells (0-1) in each level, strongest first, plus "none" last. */
export function shares(g: Pick<Grid, 'dbm'>): number[] {
  const n = Array(LEVELS.length + 1).fill(0)
  for (const v of g.dbm) n[levelOf(v)]++
  return n.map((x) => (g.dbm.length ? x / g.dbm.length : 0))
}

/** Share of cells (0-1) at or above a threshold. */
export function atLeast(g: Pick<Grid, 'dbm'>, dbm: number): number {
  let k = 0
  for (const v of g.dbm) if (v >= dbm) k++
  return g.dbm.length ? k / g.dbm.length : 0
}

/** One measured point: where (metres), the band, the signal the client reported, and the sources on that band. */
export type Sample = { x: number; y: number; dbm: number; sources: Source[]; w: Float64Array; band: Band }

/** The exponent n (1.6-4.0, 0.05 steps) that best explains the measurements, with its RMS error in dB. */
export function fitN(samples: Sample[]): { n: number; rmse: number } | null {
  const usable = samples.filter((s) => s.sources.length)
  if (!usable.length) return null
  let bestN = DEFAULT_N, bestE = Infinity
  for (let k = 0; k <= 48; k++) {
    const n = 1.6 + k * 0.05
    let e = 0
    for (const s of usable) e += (best(s.sources, s.x, s.y, s.w, { band: s.band, n }).dbm - s.dbm) ** 2
    if (e < bestE) { bestE = e; bestN = n }
  }
  return { n: Math.round(bestN * 100) / 100, rmse: Math.sqrt(bestE / usable.length) }
}
