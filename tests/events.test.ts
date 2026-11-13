// Run from the repo root: node --test tests/events.test.ts
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { fold, macOf } from '../src/ui/src/lib/events.ts'

const clients = [{ mac: 'AA:BB:CC:00:00:01', name: 'Laptop' }, { mac: 'AA:BB:CC:00:00:02', name: 'TV' }]
const ev = (text: string, time = '2026-01-01 10:00:00', extra = {}) => ({ time, cat: 'WiFi', level: 'info', text, ...extra })

test('macOf: server field first, then a known MAC in the text, then a client name', () => {
  assert.equal(macOf(ev('x', undefined, { mac: 'aa:bb:cc:00:00:09' }), clients), 'AA:BB:CC:00:00:09')
  assert.equal(macOf(ev('aa:bb:cc:00:00:02@wlan1 connected'), clients), 'AA:BB:CC:00:00:02')
  assert.equal(macOf(ev('11:22:33:44:55:66@wlan1 connected'), clients), undefined) // not a client we know
  assert.equal(macOf(ev('Laptop came online (5 GHz)'), clients), 'AA:BB:CC:00:00:01')
  assert.equal(macOf(ev('TV is short, too easy to match by accident'), clients), undefined)
  assert.equal(macOf(ev('nothing here'), []), undefined)
})

test('fold: runs of the same event become one row with a count and its time span', () => {
  const rows = [ev('a', '10:03'), ev('a', '10:02'), ev('a', '10:01'), ev('b', '10:00'), ev('a', '09:59')]
  const f = fold(rows)
  assert.deepEqual(f.map((r) => [r.text, r.n, r.time, r.first]), [['a', 3, '10:03', '10:01'], ['b', 1, '10:00', '10:00'], ['a', 1, '09:59', '09:59']])
  assert.equal(rows[0].text, 'a') // input untouched
  assert.equal((rows[0] as any).n, undefined)
})

test('fold: a different level or category breaks a run', () => {
  const f = fold([ev('a'), { ...ev('a'), level: 'warning' }, { ...ev('a'), cat: 'System' }])
  assert.equal(f.length, 3)
  assert.deepEqual(fold([]), [])
})
