// Runs the coverage grid off the main thread (lib/coverage.ts), so dragging an AP never freezes the page.
import { predict, type Input } from './coverage.ts'

self.onmessage = (e: MessageEvent<{ id: number; input: Input }>) => {
  const g = predict(e.data.input)
  ;(self as unknown as Worker).postMessage({ id: e.data.id, grid: g }, [g.dbm.buffer, g.ap.buffer])
}
