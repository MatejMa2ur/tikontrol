// Run from the repo root: node --test tests/install.test.ts
// Static checks on the RouterOS installer; it cannot run here, see docs/INSTALL.md.
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

const read = (p: string) => readFileSync(new URL(`../${p}`, import.meta.url), 'utf8')
const install = read('deploy/install.rsc')
const uninstall = read('deploy/uninstall.rsc')
// The app YAML without its comment header.
const appYaml = read('deploy/routeros-app.yaml').split('\n').filter((l) => !l.startsWith('#')).join('\n')

test('install.rsc embeds exactly deploy/routeros-app.yaml', () => {
  const block = install.split('# BEGIN app yaml')[1].split('# END app yaml')[0]
  const embedded = [...block.matchAll(/^:set yaml \(\$yaml \. "((?:[^"\\]|\\.)*)"\)$/gm)]
    .map((m) => m[1].replace(/\\(.)/g, (_, c: string) => (c === 'n' ? '\n' : c)))
    .join('')
  assert.equal(embedded, appYaml)
})

test('the app store entry is the same app', () => {
  const store = read('deploy/app-store.yaml').split('\n').filter((l) => !l.startsWith('#'))
  assert.ok(store[0].startsWith('- '))
  assert.equal(store.map((l) => l.slice(2)).join('\n'), appYaml)
})

test('installer scripts never run dangerous commands', () => {
  for (const [name, src] of [['install', install], ['uninstall', uninstall]]) {
    for (const line of src.split('\n')) {
      const code = line.trim()
      if (code.startsWith('#')) continue
      if (code.includes('/disk/format')) assert.match(code, /^:put "[^"]*\/disk\/format[^"]*"$/, `${name}: ${code}`)
      for (const bad of ['/user/remove', '/user/set', '/ip/firewall', '/ip/service/set api ', 'name=api ', 'name="api"'])
        assert.ok(!code.includes(bad), `${name} must not contain ${bad}: ${code}`)
    }
  }
  // Every object the installer adds carries a tik: comment (the app is found by its name).
  for (const add of install.match(/^\s*\/(?:certificate|ip\/dns\/static)\/add .*$/gm) ?? []) assert.match(add, /comment="tik: /)
})
