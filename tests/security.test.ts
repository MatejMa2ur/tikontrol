// Run from the repo root: node --test tests/security.test.ts
// Fictional router data only.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { cmpVersion, securityAudit, wanDnsDropped, wanInputDropped, zoneCell, zonesOf } from '../src/ui/src/lib/security.ts'

const defconf = [
  { chain: 'input', action: 'accept', comment: 'defconf: accept established,related,untracked', 'connection-state': 'established,related,untracked' },
  { chain: 'input', action: 'drop', comment: 'defconf: drop invalid', 'connection-state': 'invalid' },
  { chain: 'input', action: 'drop', comment: 'defconf: drop all not coming from LAN', 'in-interface-list': '!LAN' },
  { chain: 'forward', action: 'fasttrack-connection', 'connection-state': 'established,related' },
  { chain: 'forward', action: 'drop', comment: 'defconf: drop all from WAN not DSTNATed', 'connection-state': ['new'], 'in-interface-list': 'WAN' },
]
const titles = (raw: any) => securityAudit(raw).map((i) => i.title)

test('versions compare numerically', () => {
  assert.equal(cmpVersion('7.20', '7.9'), 1)
  assert.equal(cmpVersion('7.18.2', '7.18.2'), 0)
  assert.equal(cmpVersion('7.19', '7.19rc1'), 1)
  assert.equal(cmpVersion('7.19rc1', '7.19'), -1)
})

test('a clean router has no findings', () => {
  const raw = {
    filter: defconf,
    services: [{ name: 'telnet', disabled: true }, { name: 'ssh', address: '10.0.0.0/24' }, { name: 'api-ssl', address: '10.0.0.0/24' }, { name: 'winbox', disabled: 'true' }],
    dnsCfg: { 'allow-remote-requests': true },
    users: [{ name: 'me', group: 'full' }, { name: 'admin', disabled: true }],
    upnp: { enabled: false },
    update: { channel: 'stable', 'installed-version': '7.20', 'latest-version': '7.20' },
  }
  assert.deepEqual(securityAudit(raw), [])
  assert.deepEqual(securityAudit({}), [], 'an empty snapshot is not a finding')
})

test('plain-text services are bad and offer the disable-services fix', () => {
  const a = securityAudit({ filter: defconf, services: [{ name: 'telnet' }, { name: 'www', address: '10.0.0.0/24' }, { name: 'ftp', disabled: true }] })
  assert.equal(a[0].level, 'bad')
  assert.equal(a[0].title, 'telnet, www enabled')
  assert.deepEqual(a[0].fix?.intent, { kind: 'disable-services', names: ['telnet', 'www'] })
})

test('management services without an address restriction', () => {
  const services = [{ name: 'ssh', address: '' }, { name: 'ssh' }, { name: 'winbox', address: '10.0.0.0/24' }, { name: 'api-ssl' }, { name: 'detnet' }, { name: 'www-ssl', dynamic: true }]
  let a = securityAudit({ filter: defconf, services })
  assert.deepEqual(a.map((i) => [i.level, i.title]), [['warn', 'ssh, api-ssl open to any address']], 'LAN only: a warning')
  a = securityAudit({ filter: [], services })
  assert.equal(a[0].level, 'bad', 'no WAN drop on input: may be reachable from the internet')
  a = securityAudit({ filter: defconf, services: [{ name: 'api', address: '10.0.0.0/24' }] })
  assert.deepEqual(a.map((i) => i.title), ['Plain API (port 8728) enabled'])
  assert.deepEqual(a[0].fix?.intent, { kind: 'disable-services', names: ['api'] })
})

test('open DNS resolver needs an input drop from WAN', () => {
  assert.deepEqual(titles({ filter: [], dnsCfg: { 'allow-remote-requests': true } }), ['DNS server open to the internet'])
  assert.deepEqual(titles({ filter: [], dnsCfg: { 'allow-remote-requests': false } }), [])
  assert.deepEqual(titles({ filter: defconf, dnsCfg: { 'allow-remote-requests': 'true' } }), [])
  const dns53 = [{ chain: 'input', action: 'drop', 'in-interface-list': 'WAN', protocol: 'udp', 'dst-port': '53' }]
  assert.ok(wanDnsDropped(dns53))
  assert.ok(wanDnsDropped([{ chain: 'input', action: 'drop', 'in-interface-list': 'WAN', 'dst-port': '1-1024' }]))
  assert.ok(!wanDnsDropped([{ chain: 'input', action: 'drop', 'in-interface-list': 'WAN', 'dst-port': '22' }]))
  assert.ok(!wanDnsDropped([{ ...dns53[0], disabled: true }]), 'a disabled rule does not count')
  assert.ok(!wanDnsDropped([{ ...dns53[0], chain: 'forward' }]))
  assert.ok(!wanInputDropped([{ chain: 'input', action: 'drop', 'in-interface-list': 'WAN', protocol: 'tcp' }]), 'a tcp-only drop leaves udp open')
})

test('admin user, UPnP and updates', () => {
  assert.deepEqual(titles({ users: [{ name: 'admin', group: 'full' }] }), ['Default admin user still present'])
  const up = securityAudit({ upnp: { enabled: true } })
  assert.deepEqual(up.map((i) => i.title), ['UPnP is on'])
  assert.deepEqual(up[0].fix?.intent, { kind: 'set-upnp', enabled: false })
  assert.deepEqual(titles({ update: { 'installed-version': '7.19.4', 'latest-version': '7.20.1' } }), ['RouterOS 7.20.1 is available'])
  assert.deepEqual(titles({ update: { 'installed-version': '7.21beta2', 'latest-version': '7.20.1' } }), [], 'newer than the channel is not out of date')
  assert.deepEqual(titles({ update: { 'installed-version': '7.20' } }), [], 'not checked yet')
})

test('findings come worst first', () => {
  const a = securityAudit({ filter: [], users: [{ name: 'admin' }], services: [{ name: 'telnet' }], dnsCfg: { 'allow-remote-requests': true } })
  assert.deepEqual(a.map((i) => i.level), ['bad', 'bad', 'warn'])
})

test('zones are the interface lists with members, LAN and WAN first', () => {
  assert.deepEqual(zonesOf([{ list: 'IoT', interface: 'vlan30' }, { list: 'WAN', interface: 'ether1' }, { list: 'LAN', interface: 'bridge' }, { list: 'LAN', interface: 'wg0' }, { list: 'Guest' }]), ['LAN', 'WAN', 'Guest', 'IoT'])
})

test('zone matrix follows the real rules', () => {
  assert.deepEqual(zoneCell(defconf, 'WAN', 'LAN'), { policy: 'default', effect: 'Block', why: 'defconf: drop all from WAN not DSTNATed' })
  assert.deepEqual(zoneCell(defconf, 'LAN', 'WAN'), { policy: 'default', effect: 'Allow', why: 'no rule' })
  assert.equal(zoneCell(defconf, 'WAN', 'Router').effect, 'Block', '!LAN matches WAN')
  assert.equal(zoneCell(defconf, 'LAN', 'Router').effect, 'Allow')
  assert.equal(zoneCell(defconf, 'IoT', 'Router').effect, 'Block')

  const tik = [
    { chain: 'forward', action: 'drop', 'in-interface-list': 'IoT', 'out-interface-list': 'LAN', 'connection-state': 'new', comment: 'tik:zone:IoT:LAN' },
    { chain: 'forward', action: 'accept', 'in-interface-list': 'Guest', 'out-interface-list': 'IoT', comment: 'tik:zone:Guest:IoT', disabled: 'true' },
    { chain: 'forward', action: 'drop', 'in-interface-list': 'Guest', protocol: 'tcp', 'dst-port': '445' },
    ...defconf,
  ]
  assert.deepEqual(zoneCell(tik, 'IoT', 'LAN'), { policy: 'block', effect: 'Block', why: 'tikontrol', disabled: false })
  assert.equal(zoneCell(tik, 'LAN', 'IoT').policy, 'default', 'direction matters')
  assert.deepEqual(zoneCell(tik, 'Guest', 'IoT'), { policy: 'allow', effect: 'Allow', why: 'tikontrol (rule disabled)', disabled: true })
  assert.equal(zoneCell(tik, 'Guest', 'LAN').effect, 'Allow', 'a port-only rule is not a zone policy')
})
