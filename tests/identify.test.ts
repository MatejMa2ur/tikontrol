// Run from the repo root: node --test tests/identify.test.ts
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { catOf, fingerprintKey, fromPrl, guessDevice, guessModel, identOn, isPrivateMac, makerOf } from '../src/ui/src/lib/identify.ts'

test('maker from the MAC prefix, none for private addresses', () => {
  assert.equal(makerOf('00:03:93:12:34:56'), 'Apple')
  assert.equal(makerOf('B8:27:EB:00:00:01'), 'Raspberry Pi')
  assert.equal(makerOf('52:54:00:00:00:01'), 'QEMU / KVM') // local bit set, still known
  assert.equal(isPrivateMac('DA:A1:19:00:00:01'), true)
  assert.equal(makerOf('DA:A1:19:00:00:01'), undefined)
  assert.equal(makerOf('00:00:00:00:00:00'), undefined)
})

test('type and OS guess from hostname, vendor class, maker and neighbours', () => {
  const base = { priv: false, wired: false }
  assert.deepEqual(guessDevice({ ...base, hostname: 'Alexs-iPhone' }), { kind: 'phone', type: 'Phone', os: 'iOS' })
  assert.deepEqual(guessDevice({ ...base, hostname: 'DESKTOP-AB12CD', classId: 'MSFT 5.0' }), { kind: 'computer', type: 'Computer', os: 'Windows' })
  assert.deepEqual(guessDevice({ ...base, classId: 'android-dhcp-14' }), { kind: 'phone', type: 'Phone', os: 'Android 14' })
  assert.equal(guessDevice({ ...base, maker: 'Sonos' })?.kind, 'speaker')
  assert.equal(guessDevice({ ...base, hostname: 'ESP_3A1B2C', maker: 'Espressif' })?.type, 'Smart home')
  assert.deepEqual(guessDevice({ ...base, neighbor: { platform: 'MikroTik', board: 'hAP ax2', version: '7.20' } }), { kind: 'devices', type: 'hAP ax2', os: 'MikroTik 7.20' })
  assert.equal(guessDevice({ priv: true, wired: false })?.kind, 'phone')
  assert.equal(guessDevice({ priv: false, wired: true }), undefined)
})

test('log lines get an events category', () => {
  assert.equal(catOf(['system', 'error', 'critical'], 'login failure for user admin from 192.168.88.9 via ssh'), 'Security')
  assert.equal(catOf(['system', 'info'], 'dhcp lease added by ssh-cmd:admin@192.168.88.9 (*1 = /ip dhcp-server lease add)'), 'Admin')
  assert.equal(catOf(['system', 'info', 'account'], 'user admin logged in from 192.168.88.9 via winbox'), 'Admin')
  assert.equal(catOf(['dhcp', 'info'], 'defconf assigned 192.168.88.20 for 02:00:00:AA:BB:20'), 'Clients')
  assert.equal(catOf(['wireless', 'info'], '02:00:00:AA:BB:20@wlan1: connected'), 'WiFi')
  assert.equal(catOf(['system', 'info'], 'wg-home link up'), 'System')
})

test('identification off: no guesses, neighbour announcements still shown', () => {
  identOn.v = false
  try {
    assert.equal(guessDevice({ priv: false, wired: false, hostname: 'Alexs-iPhone' }), undefined)
    assert.equal(guessDevice({ priv: true, wired: false }), undefined)
    assert.equal(guessDevice({ priv: false, wired: true, neighbor: { board: 'hAP ax2' } })?.type, 'hAP ax2')
  } finally { identOn.v = true }
})

test('model identifiers from mDNS/SSDP map to a kind', () => {
  const kv = (k: string, v: string): [string, string, string][] => [['mdns', k, v]]
  assert.deepEqual(guessModel(kv('model', 'iPhone15,2')), { kind: 'phone', type: 'Phone', os: 'iOS' })
  assert.equal(guessModel(kv('model', 'AudioAccessory5,1'))?.type, 'Speaker')
  assert.equal(guessModel(kv('model', 'Mac14,2'))?.os, 'macOS')
  assert.equal(guessModel(kv('md', 'Chromecast'))?.type, 'TV')
  assert.equal(guessModel([['ssdp', 'deviceType', 'urn:schemas-upnp-org:device:MediaRenderer:1']])?.type, 'Media player')
  assert.equal(guessModel(kv('ty', 'Acme LaserJet 100'))?.type, 'Printer')
  assert.equal(guessModel(kv('service', '_ipp._tcp'))?.type, 'Printer') // service only when no model matched
  assert.equal(guessModel([['mdns', 'service', '_hap._tcp'], ['mdns', 'model', 'AppleTV11,1']])?.type, 'TV')
  assert.equal(guessModel(kv('fn', 'iPhone12')), undefined) // a friendly name is not a model id
  assert.equal(guessModel(kv('model', 'Widget 3000')), undefined)
})

test('fingerprint key: request list and vendor class first, else a host-name pattern', () => {
  assert.equal(fingerprintKey({ hostname: 'ESP_3A1B2C', maker: 'Espressif' }), fingerprintKey({ hostname: 'esp_00ff19', maker: 'Espressif' }))
  assert.notEqual(fingerprintKey({ hostname: 'ESP_3A1B2C', maker: 'Espressif' }), fingerprintKey({ hostname: 'ESP_3A1B2C', maker: 'Acme' }))
  assert.equal(fingerprintKey({ classId: 'udhcp 1.36.1', prl: '1,3,6', hostname: 'cam-1' }), fingerprintKey({ classId: 'udhcp 1.36.1', prl: '1,3,6', hostname: 'other' }))
  assert.equal(fingerprintKey({ hostname: 'cafe-office' }), '|||cafe-office') // words survive, only ids with a digit go
  assert.equal(fingerprintKey({ hostname: '12' }), undefined) // too weak to learn from
  assert.equal(fingerprintKey({ maker: 'Apple' }), undefined)
})

test('DHCP request lists, Apple services and the iMac boundary', () => {
  const base = { priv: true, wired: false }
  assert.equal(fromPrl('1,121,3,6,15,108,114,119,162,252')?.maker, 'Apple')
  assert.equal(fromPrl('1,3,6,15,26,28,51,58,59,43')?.os, 'Android')
  assert.equal(fromPrl('1,2,3'), undefined)
  assert.deepEqual(guessDevice({ ...base, prl: '1,121,3,6,15,108,114,119,252' }), { kind: 'phone', type: 'Phone or laptop', os: 'iOS or macOS' })
  assert.equal(guessDevice({ priv: false, wired: true, hostname: 'zimacube' })?.kind, 'server')
  assert.equal(guessDevice({ priv: false, wired: true, hostname: 'sams-imac' })?.os, 'macOS')
  assert.equal(guessModel([['mdns', 'service', '_apple-mobdev2._tcp']])?.os, 'iOS')
})
