// Event list helpers: which client an event is about, and folding repeats into one row.
// Event text comes from the router log and other devices (hostnames): it is only matched, never run or rendered as HTML.

export type Ev = { time: string; cat: string; level: string; text: string; mac?: string }
export type Folded<E extends Ev> = E & { n: number; first: string }

const MAC = /\b[0-9A-F]{2}(?::[0-9A-F]{2}){5}\b/i

/** The client an event is about: the server's own field, else a MAC in the text, else a client name (3+ chars). */
export function macOf(e: Ev, clients: { mac: string; name?: string }[]): string | undefined {
  if (e.mac) return e.mac.toUpperCase()
  const m = e.text.match(MAC)?.[0]?.toUpperCase()
  if (m) return clients.some((c) => c.mac.toUpperCase() === m) ? m : undefined
  return clients.find((c) => c.name && c.name.length >= 3 && e.text.includes(c.name))?.mac
}

/** Fold runs of the same event (same category, level and text) in a newest-first list into one row with a count. */
export function fold<E extends Ev>(rows: E[]): Folded<E>[] {
  const out: Folded<E>[] = []
  for (const e of rows) {
    const prev = out.at(-1)
    if (prev && prev.text === e.text && prev.cat === e.cat && prev.level === e.level) {
      prev.n++
      prev.first = e.time // the list is newest first, so this is older
    } else out.push({ ...e, n: 1, first: e.time })
  }
  return out
}
