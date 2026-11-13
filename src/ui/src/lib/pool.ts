// IPv4 helpers for networks: DHCP pool usage, overlap checks, and the next free VLAN and subnet to suggest.
// Inputs are router config (/ip/pool ranges, /ip/address, /ip/dhcp-server/lease, /interface/vlan); only parsed.

export function ip4(s: string): number | undefined {
  const m = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(s.trim())
  if (!m) return undefined
  const o = m.slice(1).map(Number)
  return o.some((x) => x > 255) ? undefined : ((o[0] << 24) >>> 0) + (o[1] << 16) + (o[2] << 8) + o[3]
}

/** [first, last] of a pool range: "a-b", a single address, or "a/nn". */
export function span(r: string): [number, number] | undefined {
  const [a, b] = r.split('-')
  if (b !== undefined) {
    const x = ip4(a), y = ip4(b)
    return x !== undefined && y !== undefined && x <= y ? [x, y] : undefined
  }
  const [addr, len] = r.split('/')
  const x = ip4(addr)
  if (x === undefined) return undefined
  if (len === undefined) return [x, x]
  const n = Number(len)
  if (!Number.isInteger(n) || n < 0 || n > 32) return undefined
  const size = 2 ** (32 - n)
  const first = x - (x % size)
  return [first, first + size - 1]
}

/** How many addresses a pool has and how many leases (bound or waiting) sit in it. Static leases outside it don't count. */
export function poolUsage(ranges: string[], leases: { address?: string; status?: string; disabled?: unknown }[]) {
  const spans = ranges.map(span).filter((s): s is [number, number] => !!s)
  const size = spans.reduce((n, [a, b]) => n + (b - a + 1), 0)
  const used = leases.filter((l) => String(l.disabled) !== 'true' && l.status !== 'expired').map((l) => ip4(l.address ?? '')).filter((x) => x !== undefined && spans.some(([a, b]) => x >= a && x <= b)).length
  return { size, used, pct: size ? Math.round((used / size) * 100) : 0 }
}

/** The subnet ("a.b.c.d/nn") overlaps one of the router's addresses; returns the clashing one. */
export function overlap(subnet: string, addresses: string[]): string | undefined {
  const s = span(subnet)
  if (!s) return undefined
  return addresses.find((a) => {
    const t = span(a)
    return t && s[0] <= t[1] && t[0] <= s[1]
  })
}

/** The lowest VLAN ID from 10 up (in steps of 10, then any) whose 192.168.<id>.0/24 is free too. */
export function suggest(vlans: number[], addresses: string[]): { vlan: number; subnet: string } {
  const taken = new Set(vlans)
  const free = (v: number) => !taken.has(v) && v <= 254 && !overlap(`192.168.${v}.0/24`, addresses)
  const v = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100].find(free) ?? Array.from({ length: 253 }, (_, i) => i + 2).find(free) ?? 2
  return { vlan: v, subnet: `192.168.${v}.0/24` }
}
