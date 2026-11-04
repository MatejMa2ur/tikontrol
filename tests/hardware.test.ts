// Run from the repo root: node --test tests/hardware.test.ts
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { simulateBe3 } from '../src/ui/src/lib/be3.ts'
import { bandLabel, detectHardware, normalizeWifi, parseChannel, standardOf } from '../src/ui/src/lib/hardware.ts'

const sample = JSON.parse(readFileSync(new URL('../src/ui/src/lib/sample.json', import.meta.url), 'utf8'))

test('recognises the hAP ac² from its own data', () => {
  const hw = detectHardware(normalizeWifi(sample))
  assert.equal(hw.make, 'MikroTik')
  assert.equal(hw.board, 'hAP ac^2')
  assert.equal(hw.model, 'RBD52G-5HacD2HnD')
  assert.equal(hw.arch, 'arm')
  assert.equal(hw.ports, 5)
  assert.equal(hw.wifiPackage, 'wireless')
  assert.equal(hw.wifiStandard, 'WiFi 5')
  assert.deepEqual(hw.bands, ['2.4 GHz', '5 GHz'])
  assert.equal(hw.canHostCmr, false) // too little free flash
  assert.equal(hw.specs, 'MikroTik RBD52G-5HacD2HnD · arm · 4× 672 MHz · 128 MiB · WiFi 5')
})

test('recognises a hAP be³ Media from the wifi package, with legacy menus missing', () => {
  const raw = normalizeWifi(simulateBe3(sample))
  const hw = detectHardware(raw)
  assert.equal(hw.model, 'MA53UG+HbeH')
  assert.equal(hw.arch, 'arm64')
  assert.equal(hw.wifiPackage, 'wifi')
  assert.equal(hw.wifiStandard, 'WiFi 7')
  assert.deepEqual(hw.bands, ['2.4 GHz', '5 GHz', '6 GHz'])
  assert.equal(hw.canHostCmr, true)
  assert.equal(hw.specs, 'MikroTik MA53UG+HbeH · arm64 · 4× 1500 MHz · 2 GiB · WiFi 7')
  assert.equal(raw.stations.length, sample.stations.length)
  assert.equal(raw.stations[0]['signal-strength'], String(parseInt(sample.stations[0]['signal-strength'])))
  assert.equal(raw.stations.filter((s: any) => s.interface === 'wifi3').length, 1)
  for (const k of ['radios', 'linkQuality', 'secProfiles', 'accessList']) assert.deepEqual(raw[k], [], k)
})

test('survives a router that reports nothing', () => {
  const hw = detectHardware(normalizeWifi({}))
  assert.equal(hw.wifiPackage, 'none')
  assert.equal(hw.canHostCmr, false)
  assert.equal(hw.specs, 'MikroTik')
})

test('the committed sample is fictional', () => {
  const text = JSON.stringify(sample)
  assert.equal(sample.cloud['public-address'], null)
  assert.equal(sample.cloud['dns-name'], null)
  assert.equal(sample.clock['time-zone-name'], 'Etc/UTC')
  assert.doesNotMatch(text, /192\.168\./)
  assert.doesNotMatch(text, /\b\d+\.\d+\.\d+\.\d+\.\d+\b/) // a broken address, not a fake one
  assert.doesNotMatch(text, /"(host-name|comment)":\s*"(?!Speaker|dev-vm|home-server|desktop-pc|smart-hub|defconf|back-to-home-vpn|special dummy|system default)/)
})

test('maps RouterOS band names', () => {
  assert.deepEqual(['2ghz-b/g/n', '5ghz-a/n/ac', '5ghz-ax', '6ghz-be'].map(standardOf), ['WiFi 4', 'WiFi 5', 'WiFi 6', 'WiFi 7'])
  assert.deepEqual(['2ghz-be', '5ghz-be', '6ghz-be'].map(bandLabel), ['2.4 GHz', '5 GHz', '6 GHz'])
})

test('parses monitor channel strings', () => {
  assert.deepEqual(parseChannel('5640/20-eCee/ac/DP(24dBm)'), { freq: 5640, channel: 128, width: 80, dfs: true, txPower: 24 })
  assert.deepEqual(parseChannel('5180/20-Ceee/ac(23dBm)'), { freq: 5180, channel: 36, width: 80, dfs: false, txPower: 23 })
  assert.deepEqual(parseChannel('2437/20/gn(17dBm)'), { freq: 2437, channel: 6, width: 20, dfs: false, txPower: 17 })
  assert.deepEqual(parseChannel('5500/ax/eC'), { freq: 5500, channel: 100, width: 40, dfs: true, txPower: undefined })
  assert.equal(parseChannel(undefined).freq, undefined)
})
