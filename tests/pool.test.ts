// Run from the repo root: node --test tests/pool.test.ts
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { ip4, span, poolUsage, overlap, suggest } from '../src/ui/src/lib/pool.ts'

test('ip4 and span parse addresses, ranges and prefixes', () => {
  assert.equal(ip4('10.0.0.1'), 0x0a000001)
  assert.equal(ip4('255.255.255.255'), 0xffffffff)
  assert.equal(ip4('256.1.1.1'), undefined)
  assert.equal(ip4('1.2.3'), undefined)
  assert.deepEqual(span('10.0.0.10-10.0.0.20'), [0x0a00000a, 0x0a000014])
  assert.equal(span('10.0.0.20-10.0.0.10'), undefined)
  assert.deepEqual(span('10.0.0.5'), [0x0a000005, 0x0a000005])
  assert.deepEqual(span('10.0.0.1/24'), [0x0a000000, 0x0a0000ff])
  assert.equal(span('10.0.0.1/33'), undefined)
})

test('poolUsage counts live leases inside the pool only', () => {
  const leases = [
    { address: '10.0.0.10', status: 'bound' },
    { address: '10.0.0.11', status: 'waiting' },
    { address: '10.0.0.12', status: 'expired' },
    { address: '10.0.0.13', status: 'bound', disabled: 'true' },
    { address: '10.0.0.200', status: 'bound' }, // static, outside the pool
    { status: 'bound' },
  ]
  assert.deepEqual(poolUsage(['10.0.0.10-10.0.0.19'], leases), { size: 10, used: 2, pct: 20 })
  assert.deepEqual(poolUsage(['10.0.0.10-10.0.0.14', '10.0.0.200'], leases), { size: 6, used: 3, pct: 50 })
  assert.deepEqual(poolUsage(['bad'], leases), { size: 0, used: 0, pct: 0 })
})

test('overlap finds a clashing router address', () => {
  const addrs = ['192.168.88.1/24', '10.10.0.1/16']
  assert.equal(overlap('192.168.88.0/24', addrs), '192.168.88.1/24')
  assert.equal(overlap('10.10.5.0/24', addrs), '10.10.0.1/16')
  assert.equal(overlap('192.168.30.0/24', addrs), undefined)
  assert.equal(overlap('nonsense', addrs), undefined)
})

test('suggest skips used VLAN IDs and subnets', () => {
  assert.deepEqual(suggest([], []), { vlan: 10, subnet: '192.168.10.0/24' })
  assert.deepEqual(suggest([10], ['192.168.20.1/24']), { vlan: 30, subnet: '192.168.30.0/24' })
  const all = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100]
  assert.deepEqual(suggest(all, ['192.168.2.1/24']), { vlan: 3, subnet: '192.168.3.0/24' })
})
