// Run from the repo root: node --test tests/topology.test.ts
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { groupByNeighbor, inSubnet } from '../src/ui/src/lib/topology.ts'

const c = (mac: string, port: string | undefined, wired = true) => ({ mac, port, wired })

test('wired clients behind a neighbor switch hang off that switch', () => {
  const nbs = [{ interface: 'ether3,bridge', 'mac-address': '02:00:00:00:10:01', identity: 'Office switch', address: '192.168.88.2', board: 'CSS610' }]
  const { devices, direct } = groupByNeighbor([c('02:00:00:00:00:01', 'ether3'), c('02:00:00:00:00:02', 'ether2'), c('02:00:00:00:00:03', 'wlan1', false)], nbs)
  assert.equal(devices.length, 1)
  assert.equal(devices[0].label, 'Office switch')
  assert.deepEqual(devices[0].clients.map((x) => x.mac), ['02:00:00:00:00:01'])
  assert.deepEqual(direct.map((x) => x.mac), ['02:00:00:00:00:02', '02:00:00:00:00:03'])
})

test('the neighbor itself is not also a client, chains and junk are handled', () => {
  const nbs = [
    { interface: 'ether4', 'mac-address': '02:00:00:00:20:01', identity: 'AP hall' },
    { interface: 'ether4', 'mac-address': '02:00:00:00:20:02', identity: 'AP attic' },
    { interface: 'ether4', 'mac-address': '02:00:00:00:20:02', identity: 'duplicate' },
    { interface: '', 'mac-address': '02:00:00:00:30:01', identity: 'nowhere' },
    { interface: 'ether5' },
  ]
  const { devices, direct } = groupByNeighbor([c('02:00:00:00:20:01', 'ether4'), c('02:00:00:00:00:09', 'ether4')], nbs)
  assert.equal(devices.length, 1)
  assert.deepEqual(devices[0].devices.map((d) => d.label), ['AP attic'])
  assert.deepEqual(devices[0].clients.map((x) => x.mac), ['02:00:00:00:00:09'])
  assert.deepEqual(direct, [])
})

test('inSubnet: a switch finds its gateway inside its own bridge subnet', () => {
  assert.equal(inSubnet('192.0.2.1', '192.0.2.7/24'), true)
  assert.equal(inSubnet('198.51.100.1', '192.0.2.7/24'), false)
  assert.equal(inSubnet('192.0.2.130', '192.0.2.7/25'), false)
  assert.equal(inSubnet('192.0.2.7', '192.0.2.7/32'), true)
})

test('neighbour panel ids and WebFig links accept only well-formed values', async () => {
  const { neighborHref, findNeighbor, webfigUrl } = await import('../src/ui/src/lib/topology.ts')
  const nbs = [{ 'mac-address': '02:00:00:00:10:0a', identity: 'Switch' }, { 'mac-address': '"><script>', identity: 'Evil' }]
  assert.equal(neighborHref(nbs[0]), '#/device/02:00:00:00:10:0A')
  assert.equal(neighborHref(nbs[1]), undefined)
  assert.equal(findNeighbor(nbs, '02:00:00:00:10:0A')?.identity, 'Switch')
  assert.equal(findNeighbor(nbs, '"><script>'), undefined)
  assert.equal(findNeighbor(nbs, 'router'), undefined)
  assert.equal(webfigUrl('192.168.88.2'), 'https://192.168.88.2/')
  assert.equal(webfigUrl('192.168.088.2'), 'https://192.168.88.2/')
  assert.equal(webfigUrl('300.1.1.1'), undefined)
  assert.equal(webfigUrl('evil.example/x'), undefined)
  assert.equal(webfigUrl(undefined), undefined)
})
