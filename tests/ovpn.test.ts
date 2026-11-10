// Run from the repo root: node --test tests/ovpn.test.ts
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { ovpnProfile, publicPem, remoteHost, validHost, clientSettings, profileName, CN_RE } from '../src/ui/src/lib/ovpn.ts'

// fictional, not a real certificate
const CERT = '-----BEGIN CERTIFICATE-----\nMIIBszCCAVmgAwIBAgIUZm9vYmFy\nZm9vYmFyYmF6+/==\n-----END CERTIFICATE-----'
const KEY = '-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA\n-----END RSA PRIVATE KEY-----'
const server = { port: 1194, protocol: 'tcp', auth: ['sha1', 'sha256'], cipher: ['aes128-cbc', 'aes256-gcm'], mode: 'ip' }

test('a profile logs in with a VPN user and embeds only the CA certificate', () => {
  const p = ovpnProfile({ remote: 'vpn.example.net', server, caPem: CERT + '\r\n' })
  const lines = p.split('\n')
  for (const l of ['client', 'dev tun', 'proto tcp-client', 'remote vpn.example.net 1194', 'remote-cert-tls server', 'auth-user-pass', 'auth-nocache', 'auth SHA256', 'data-ciphers AES-256-GCM:AES-128-CBC', 'data-ciphers-fallback AES-256-GCM']) {
    assert.ok(lines.includes(l), `missing ${l}\n${p}`)
  }
  assert.ok(p.includes(`<ca>\n${CERT}\n</ca>\n`))
  assert.ok(!/PRIVATE|password|<key>|<cert>|auth-user-pass[ \t]+\S/.test(p.replace('This file holds no password or private key.', '')))
})

test('server settings shape the profile', () => {
  assert.deepEqual(clientSettings({ port: '1195', protocol: 'udp', mode: 'ethernet', cipher: 'aes256-cbc,blowfish128', auth: 'sha1' }), { port: 1195, proto: 'udp', dev: 'tap', ciphers: ['AES-256-CBC'], auth: 'SHA1' })
  // nothing known: RouterOS defaults, no auth line
  const d = clientSettings({})
  assert.equal(d.port, 1194)
  assert.equal(d.auth, '')
  assert.ok(!ovpnProfile({ remote: '203.0.113.7', server: {}, caPem: CERT }).includes('\nauth '))
  assert.equal(clientSettings({ port: 70000 }).port, 1194)
})

test('the remote comes from DDNS or the public address, and is validated', () => {
  assert.equal(remoteHost({ 'dns-name': 'abc123.sn.example.net', 'ddns-enabled': true, 'public-address': '203.0.113.7' }), 'abc123.sn.example.net')
  assert.equal(remoteHost({ 'dns-name': 'abc123.sn.example.net', 'ddns-enabled': false, 'public-address': '203.0.113.7' }), '203.0.113.7')
  assert.equal(remoteHost({ 'dns-name': 'abc123.sn.example.net', 'ddns-enabled': false }), 'abc123.sn.example.net')
  assert.equal(remoteHost({ 'dns-name': null, 'public-address': null }), '')
  assert.equal(remoteHost(undefined), '')
  assert.equal(remoteHost({ 'public-address': 'x\nremote evil.example 1' }), '')
  for (const bad of ['', 'vpn example', 'vpn.example.net\nup /bin/sh', 'vpn.example.net 443', 'vpn.example.net:443', '-a.example', '300.1.1.1', 'a'.repeat(254)]) {
    assert.equal(validHost(bad), false, bad)
    assert.throws(() => ovpnProfile({ remote: bad, server, caPem: CERT }))
  }
  assert.equal(profileName('vpn.example.net'), 'tikontrol-vpn.example.net.ovpn')
  assert.equal(profileName('../x y'), 'tikontrol-vpn.ovpn')
})

test('only public certificates are embedded', () => {
  assert.equal(publicPem(CERT), CERT)
  assert.equal(publicPem(`${CERT}\n${CERT}\n`), `${CERT}\n${CERT}`)
  for (const bad of [KEY, `${CERT}\n${KEY}`, `${KEY}\n${CERT}`, `${CERT}\n</ca>\nup /bin/sh`, '-----BEGIN CERTIFICATE-----\n<x>\n-----END CERTIFICATE-----', 'hello', '']) {
    assert.equal(publicPem(bad), null, bad)
    assert.throws(() => ovpnProfile({ remote: 'vpn.example.net', server, caPem: bad }))
  }
})

test('common names follow the server rule', () => {
  for (const ok of ['vpn.example.net', '203.0.113.7', 'Home-CA', 'a']) assert.ok(CN_RE.test(ok), ok)
  for (const bad of ['', 'a b', '-a', 'a.', 'a_b', 'a"b', 'a'.repeat(65)]) assert.ok(!CN_RE.test(bad), bad)
})
