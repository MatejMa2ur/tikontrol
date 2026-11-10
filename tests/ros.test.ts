// Run from the repo root: node --test tests/ros.test.ts
// "Show in RouterOS": every Section's `ros` value is a read-only command a user may paste into a terminal,
// so none may change the router or print a secret.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

const PAGES = join(import.meta.dirname, '../src/ui/src/pages')
const svelteFiles = (dir: string): string[] => readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? svelteFiles(join(dir, e.name)) : e.name.endsWith('.svelte') ? [join(dir, e.name)] : []))

/** the end of a `{...}` expression starting at i, skipping braces inside JS strings */
function braceEnd(s: string, i: number): number {
  let depth = 0, q = ''
  for (let j = i; j < s.length; j++) {
    const c = s[j]
    if (q) { if (c === '\\') j++; else if (c === q) q = ''; continue }
    if (c === "'" || c === '"' || c === '`') q = c
    else if (c === '{') depth++
    else if (c === '}' && --depth === 0) return j
  }
  throw new Error(`unclosed { at ${i}`)
}

/** every ros="..." / ros='...' / ros={...} attribute value in a file */
export function rosValues(src: string): string[] {
  const out: string[] = []
  for (const m of src.matchAll(/\bros=(["'{])/g)) {
    const at = m.index! + 4
    out.push(m[1] === '{' ? src.slice(at, braceEnd(src, at) + 1) : src.slice(at + 1, src.indexOf(m[1], at + 1)))
  }
  return out
}

/** the concrete commands a value can show: `{c ? 'a' : 'b'}` gives both branches, any other `{expr}` a placeholder */
export function expand(v: string): string[] {
  const i = v.indexOf('{')
  if (i < 0) return [v.replaceAll('&quot;', '"')]
  const end = braceEnd(v, i)
  const expr = v.slice(i + 1, end)
  const t = expr.match(/^.*?\?\s*'([^']*)'\s*:\s*'([^']*)'\s*$/s)
  const fill = t ? [t[1], t[2]] : ['x']
  return fill.flatMap((f) => expand(v.slice(0, i) + f + v.slice(end + 1)))
}

const WRITE = new Set(['add', 'set', 'unset', 'edit', 'remove', 'enable', 'disable', 'reset', 'reset-counters', 'reset-counters-all', 'reboot', 'shutdown', 'import', 'export', 'run', 'execute', 'load', 'save', 'upgrade', 'install', 'downgrade', 'check-for-updates', 'download', 'flush', 'move', 'comment', 'make-static', 'pair', 'unpair', 'cancel', 'fetch', 'send', 'generate-key', 'version-check'])
// menus whose rows hold keys, passwords or shared secrets: only with an explicit field list
const SECRET_MENUS = ['/ip/cloud/print', '/ip/cloud/back-to-home-users/print', '/ppp/secret/print', '/radius/print', '/interface/wireguard/print', '/interface/wireguard/peers/print', '/interface/l2tp-server/server/print', '/ip/hotspot/user/print', '/interface/wireless/security-profiles/print', '/interface/wireless/access-list/print', '/interface/wifi/print', '/interface/wifi/access-list/print', '/interface/wifi/security/print', '/user/ssh-keys/print', '/snmp/community/print']
const SECRET_FIELDS = /private|preshared|pre-shared|secret|password|passphrase|^wpa/

const all = svelteFiles(PAGES).flatMap((f) => rosValues(readFileSync(f, 'utf8')).flatMap((v) => expand(v).map((cmd) => ({ f: f.slice(PAGES.length + 1), cmd }))))

test('the parser finds the pages\' ros values and expands both branches of a ternary', () => {
  assert.deepEqual(rosValues(`<Section ros="/a/print; /b/print where c~&quot;x&quot;"> <S ros={p ? '/c/print' : '/d/print where n~"^t"'}> <S ros='/e/print'>`), ['/a/print; /b/print where c~&quot;x&quot;', `{p ? '/c/print' : '/d/print where n~"^t"'}`, '/e/print'])
  assert.deepEqual(expand(`{p === 'w' ? '/c/print' : '/d/print'}`), ['/c/print', '/d/print'])
  assert.deepEqual(expand('/interface/{r.pkg}/monitor {r.name} once'), ['/interface/x/monitor x once'])
  assert.ok(all.length > 60, `only ${all.length} ros values found`)
})

for (const { f, cmd } of all) {
  test(`${f}: ${cmd}`, () => {
    assert.doesNotMatch(cmd, /show-sensitive/)
    for (const part of cmd.split(';').map((s) => s.trim())) {
      assert.match(part, /^\//, `"${part}" is not a RouterOS path`)
      const [path] = part.split(/\s+/)
      const verb = path.split('/').at(-1)!
      assert.ok(!WRITE.has(verb), `"${part}" uses the write verb ${verb}`)
      if (SECRET_MENUS.includes(path)) {
        const props = part.match(/\bproplist=(\S+)/)?.[1]
        assert.ok(props, `"${part}" can show secrets: name its fields with proplist=`)
        for (const p of props.split(',')) assert.doesNotMatch(p, SECRET_FIELDS, `"${part}" asks for ${p}`)
      }
    }
  })
}
