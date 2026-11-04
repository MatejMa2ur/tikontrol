// Pre-gzips the built UI so the server embeds and sends it compressed, with no compression code of its own.
// Run after `vite build` (see package.json). Keeps only the .gz copy of each text file.
import { readdirSync, readFileSync, statSync, unlinkSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { gzipSync, constants } from 'node:zlib'

const walk = (d) => readdirSync(d).flatMap((f) => (statSync(join(d, f)).isDirectory() ? walk(join(d, f)) : [join(d, f)]))
let before = 0, after = 0
for (const f of walk(new URL('../dist', import.meta.url).pathname)) {
  if (!/\.(html|js|css|svg|json|webmanifest)$/.test(f)) continue
  const raw = readFileSync(f)
  const gz = gzipSync(raw, { level: constants.Z_BEST_COMPRESSION })
  writeFileSync(`${f}.gz`, gz)
  unlinkSync(f)
  before += raw.length; after += gz.length
}
console.log(`gzip: ${(before / 1024).toFixed(0)} KB -> ${(after / 1024).toFixed(0)} KB`)
