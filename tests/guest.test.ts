// Run from the repo root: node --test tests/guest.test.ts
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { guestTest } from '../src/ui/src/lib/guest.ts'

const home = { mac: 'AA:BB:CC:00:00:01', ip: '192.168.88.20', port: 'wlan1', ssid: 'HomeNet' }

test('nothing is a guest on a router without guest networks', () => {
  const raw = {
    wifi: [{ name: 'wlan1', ssid: 'HomeNet' }],
    addresses: [{ address: '192.168.88.1/24', interface: 'bridge', comment: 'defconf' }],
    dhcpServers: [{ name: 'defconf', interface: 'bridge' }],
  }
  assert.equal(guestTest(raw)(home), false)
  assert.equal(guestTest({})(home), false)
  assert.equal(guestTest(undefined)(home), false)
})

test("tikontrol's guest Wi-Fi interface, by name or comment", () => {
  const raw = { wifi: [{ name: 'wlan1', ssid: 'HomeNet' }, { name: 'tik-guest-friends-wlan1', ssid: 'Friends', comment: 'tik:guest:friends:wlan1' }] }
  assert.equal(guestTest(raw)({ ...home, port: 'tik-guest-friends-wlan1', ssid: 'Friends' }), true)
  assert.equal(guestTest({ wifi: [{ name: 'wifi3', ssid: 'Friends', comment: 'tik:guest:friends:wifi1' }] })({ ...home, port: 'wifi3', ssid: 'Friends' }), true)
  assert.equal(guestTest(raw)(home), false)
})

test('an SSID or interface named guest', () => {
  assert.equal(guestTest({})({ ...home, ssid: 'HomeNet-Guest' }), true)
  assert.equal(guestTest({ interfaces: [{ name: 'vlan30', comment: 'tik:net:guests' }] })({ ...home, port: 'vlan30' }), true)
})

test('an IP in a guest subnet: hotspot address, guest network or a guest DHCP server', () => {
  const hs = { addresses: [{ address: '10.10.10.1/24', interface: 'ether5', comment: 'tik:hotspot' }] }
  assert.equal(guestTest(hs)({ ...home, ip: '10.10.10.57', port: 'ether5' }), true)
  assert.equal(guestTest(hs)({ ...home, ip: '10.10.11.57' }), false)
  // tik:net:guest on the VLAN interface (networks.rs), the address itself has the same comment
  const net = { interfaces: [{ name: 'vlan30', comment: 'tik:net:guest' }], addresses: [{ address: '172.16.30.1/24', interface: 'vlan30', comment: 'tik:net:guest' }] }
  assert.equal(guestTest(net)({ ...home, ip: '172.16.30.9', port: 'ether3' }), true, 'wired guest on a VLAN: matched by IP, not port')
  const dhcp = { dhcpServers: [{ name: 'tik-guest', interface: 'guest-br' }], addresses: [{ address: '10.20.0.1/16', interface: 'guest-br' }] }
  assert.equal(guestTest(dhcp)({ ...home, ip: '10.20.200.3' }), true)
  // a /32 is the router's own address, not a subnet
  assert.equal(guestTest({ addresses: [{ address: '192.168.88.20/32', interface: 'x', comment: 'guest' }] })(home), false)
})

test('hotspot: any active user, and the hotspot interface', () => {
  const raw = { hotspot: [{ name: 'tik-guest', interface: 'ether5', disabled: false }], hotspotActive: [{ 'mac-address': 'aa:bb:cc:00:00:09', address: '10.5.50.9' }] }
  const t = guestTest(raw)
  assert.equal(t({ ...home, mac: 'AA:BB:CC:00:00:09' }), true)
  assert.equal(t({ ...home, ip: '10.5.50.9' }), true)
  assert.equal(t({ ...home, port: 'ether5' }), true)
  assert.equal(t(home), false)
  assert.equal(guestTest({ hotspot: [{ interface: 'ether5', disabled: true }] })({ ...home, port: 'ether5' }), false)
})

test('bad addresses never match', () => {
  const raw = { addresses: [{ address: '10.0.0.300/24', comment: 'guest' }, { address: 'x', comment: 'guest' }, { address: '10.0.0.1/33', comment: 'guest' }] }
  assert.equal(guestTest(raw)({ ...home, ip: '10.0.0.5' }), false)
  assert.equal(guestTest({ addresses: [{ address: '10.0.0.1/24', comment: 'guest' }] })({ ...home, ip: '—' }), false)
})
