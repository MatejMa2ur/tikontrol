// Run from the repo root: node --test tests/coverage.test.ts
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { atLeast, best, cellFor, crosses, fitN, fspl, levelOf, NONE, packWalls, pathWallLoss, predict, rssi, scaleFrom, shares, upsample, type Wall } from '../src/ui/src/lib/coverage.ts'

const near = (a: number, b: number, eps = 0.01) => assert.ok(Math.abs(a - b) < eps, `${a} ≈ ${b}`)

test('free-space path loss matches the textbook values', () => {
  near(fspl(1, 2437), 40.19)
  near(fspl(1, 5500), 47.26)
  near(fspl(1, 6525), 48.74)
  near(fspl(10, 2437) - fspl(1, 2437), 20) // 20 dB per decade in free space
  near(fspl(1000, 2400), 100.05) // 1 km at 2.4 GHz ≈ 100 dB
})

test('segment crossing: proper crossings only', () => {
  assert.equal(crosses(0, 0, 10, 0, 5, -1, 5, 1), true)
  assert.equal(crosses(0, 0, 10, 0, 5, 1, 5, 3), false, 'wall beside the path')
  assert.equal(crosses(0, 0, 10, 0, 12, -1, 12, 1), false, 'wall beyond the end')
  assert.equal(crosses(0, 0, 10, 0, 10, -1, 10, 1), false, 'touching the end point does not count')
  assert.equal(crosses(0, 0, 10, 0, 2, 0, 8, 0), false, 'running along the wall does not count')
  assert.equal(crosses(0, 0, 10, 10, 0, 10, 10, 0), true, 'diagonals')
})

test('wall losses add up per crossing and per band', () => {
  // plan 20 m wide (mpu 0.2), square image; three parallel walls across a horizontal path
  const walls: Wall[] = [
    { x1: 25, y1: 0, x2: 25, y2: 100, m: 'drywall' },
    { x1: 50, y1: 0, x2: 50, y2: 100, m: 'brick' },
    { x1: 75, y1: 0, x2: 75, y2: 40, m: 'metal' }, // does not reach y = 50 %
  ]
  const w24 = packWalls(walls, '2.4', 0.2, 1)
  near(pathWallLoss(1, 10, 19, 10, w24), 3 + 8)
  near(pathWallLoss(1, 2, 19, 2, w24), 3 + 8 + 20)
  near(pathWallLoss(1, 10, 19, 10, packWalls(walls, '5', 0.2, 1)), 4 + 10)
  near(pathWallLoss(1, 10, 4, 10, w24), 0)
})

test('a known geometry gives the expected dBm', () => {
  // AP at (0, 0) m, 20 dBm, 0 dBi; client 10 m away behind one concrete wall; n = 2 (free space)
  const w = packWalls([{ x1: 50, y1: -10, x2: 50, y2: 10, m: 'concrete' }], '2.4', 0.1, 1) // wall at x = 5 m
  const ap = { x: 0, y: 0, tx: 20, gain: 0 }
  near(rssi(ap, 10, 0, w, { band: '2.4', n: 2 }), 20 - 40.19 - 20 - 12)
  near(rssi(ap, 10, 0, w, { band: '2.4', n: 3 }), 20 - 40.19 - 30 - 12)
  near(rssi(ap, 0.3, 0, w, { band: '2.4', n: 2 }), 20 - 40.19) // inside 1 m: the 1 m value
  // 5 GHz loses more: 7 dB more free-space loss at 1 m, 3 dB more in concrete
  const w5 = packWalls([{ x1: 50, y1: -10, x2: 50, y2: 10, m: 'concrete' }], '5', 0.1, 1)
  near(rssi({ ...ap, tx: 23, gain: 2 }, 10, 0, w5, { band: '5', n: 2 }), 25 - 47.26 - 20 - 15)
})

test('the strongest AP wins per point', () => {
  const w = new Float64Array()
  const aps = [{ x: 0, y: 0, tx: 20, gain: 2 }, { x: 20, y: 0, tx: 20, gain: 2 }]
  assert.equal(best(aps, 3, 0, w, { band: '5', n: 2.5 }).ap, 0)
  assert.equal(best(aps, 17, 0, w, { band: '5', n: 2.5 }).ap, 1)
  assert.equal(best([], 1, 1, w, { band: '5', n: 2.5 }).ap, -1)
})

test('scale from a reference line respects the image aspect', () => {
  near(scaleFrom({ x1: 10, y1: 50, x2: 60, y2: 50 }, 10, 0.5)!, 0.2) // 50 units = 10 m
  near(scaleFrom({ x1: 10, y1: 0, x2: 10, y2: 100 }, 10, 0.5)!, 0.2) // full height = 50 units when h = w / 2
  assert.equal(scaleFrom({ x1: 1, y1: 1, x2: 1, y2: 1 }, 5, 1), null)
})

test('grid prediction: size cap, levels and summary', () => {
  assert.equal(cellFor(10, 10, 0.5), 0.5)
  near(cellFor(400, 300, 0.25, 30_000), 2)
  const g = predict({ mpu: 0.2, aspect: 0.5, band: '5', n: 2, cell: 0.5, sources: [{ x: 10, y: 5, tx: 23, gain: 2 }], walls: [] })
  assert.equal(g.cols, 40); assert.equal(g.rows, 20)
  assert.equal(g.dbm.length, 800)
  near(g.dbm[10 * 40 + 20], 25 - 47.26) // cell centred at (10.25, 5.25): inside 1 m
  assert.ok(g.ap.every((a) => a === 0))
  const s = shares(g)
  near(s.reduce((a, b) => a + b, 0), 1, 1e-9)
  assert.equal(atLeast(g, -200), 1)
  assert.ok(atLeast(g, -67) > 0.5 && atLeast(g, -67) <= 1)
  const big = predict({ mpu: 4, aspect: 0.75, band: '2.4', n: 2, cell: 0.25, sources: [], walls: [], maxCells: 30_000 })
  assert.ok(big.cols * big.rows <= 30_400, 'capped')
  assert.equal(levelOf(big.dbm[0]), NONE, 'no AP: no coverage')
  assert.equal(levelOf(-50), 0); assert.equal(levelOf(-67), 1); assert.equal(levelOf(-80), 3); assert.equal(levelOf(-90), NONE)
})

test('upsampling interpolates between cell centres for smooth drawing', () => {
  const u = upsample({ cols: 2, rows: 1, dbm: new Float32Array([-50, -70]) }, 4)
  assert.equal(u.cols, 8); assert.equal(u.rows, 4)
  near(u.dbm[0], -50); near(u.dbm[7], -70) // edges hold the edge cell's value
  near(u.dbm[3], -57.5); near(u.dbm[4], -62.5) // between the two centres
  assert.equal(upsample({ cols: 1, rows: 1, dbm: new Float32Array([-Infinity]) }, 2).dbm[0], -200)
})

test('fitting n recovers the exponent behind synthetic measurements', () => {
  const w = new Float64Array()
  const ap = { x: 0, y: 0, tx: 20, gain: 2 }
  const samples = [3, 6, 9, 14].map((d) => ({ x: d, y: 0, w, band: '5' as const, sources: [ap], dbm: rssi(ap, d, 0, w, { band: '5', n: 2.8 }) }))
  const f = fitN(samples)!
  near(f.n, 2.8, 0.001); near(f.rmse, 0, 0.001)
  assert.equal(fitN([]), null)
})
