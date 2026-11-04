import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig, type Plugin } from 'vite'

// A local snapshot.json holds real router data. It may feed `npm run dev`, but it must never end up
// in a build: the built UI is embedded in the tikontrol binary and container image.
const noSnapshot: Plugin = {
  name: 'tik-no-snapshot',
  apply: 'build',
  load(id) {
    // valid JSON: Vite's built-in JSON plugin still parses what we return
    if (id.endsWith('/lib/snapshot.json')) return 'null'
  },
}

export default defineConfig({
  plugins: [svelte(), noSnapshot],
  build: { target: 'es2022', reportCompressedSize: false },
})
