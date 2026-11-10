// Run from the repo root: node --test tests/guide.test.ts
// Fictional neighbours only.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { privateIp, setupSteps, tooOld } from '../src/ui/src/lib/guide.ts'

const ssl = ['# 1. cert', '/certificate/add name=tikontrol-api', '/ip/service/set api-ssl address="10.0.0.5/32"']

test('an old RouterOS 7 gets update, firmware, API-SSL and add steps', () => {
  const s = setupSteps({ version: '7.15.3 (stable)', address: '192.168.88.2' }, ssl, '10.0.0.5')
  assert.deepEqual(s.map((x) => x.title), ['Update RouterOS', 'Update the firmware', 'Give API-SSL a certificate', 'Add it as a router'])
  assert.ok(s[0].cmds.includes('/system/package/update/install'))
  assert.ok(s[1].cmds.includes('/system/routerboard/upgrade'))
  assert.equal(s[2].cmds, ssl)
  assert.match(s[2].why, /10\.0\.0\.5/)
  assert.equal(s[3].add, '192.168.88.2')
})

test('7.18+ skips the update; unknown versions keep it', () => {
  assert.equal(setupSteps({ version: '7.18' }, ssl)[0].title, 'Give API-SSL a certificate')
  assert.equal(setupSteps({ version: '7.25.1 (stable)' }, ssl).length, 2)
  assert.equal(setupSteps({}, ssl)[0].title, 'Update RouterOS')
})

test('RouterOS 6 and SwOS get no steps', () => {
  assert.ok(tooOld('6.49.10') && tooOld('2.17') && !tooOld('7.1') && !tooOld(''))
  assert.deepEqual(setupSteps({ version: '6.49.10 (long-term)' }, ssl), [])
})

test('only LAN or loopback IPv4 addresses are linked to the Routers form', () => {
  for (const ok of ['10.1.2.3', '172.16.0.1', '172.31.255.255', '192.168.88.2', '127.0.0.1']) assert.ok(privateIp(ok), ok)
  for (const bad of ['8.8.8.8', '172.32.0.1', '192.169.0.1', '256.1.1.1', '10.0.0', 'fe80::1', '', '10.0.0.1/24', '10.0.0.1&x=1']) assert.ok(!privateIp(bad), bad)
  assert.equal(setupSteps({ version: '7.20', address: 'fe80::1' }, ssl).at(-1)!.add, undefined)
})
