// Run from the repo root: node --test tests/identify.test.ts
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { catOf, guessDevice, isPrivateMac, makerOf } from '../src/ui/src/lib/identify.ts'

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
