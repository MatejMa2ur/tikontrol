// Topology grouping: network devices seen by MNDP/LLDP/CDP (`/ip/neighbor`) become nodes between
// this router and the clients behind them. A neighbor is reached through the port it was heard on
// (RouterOS 7 writes "ether2,bridge": the first part is the port), and every wired client the
// bridge host table puts on that same port sits behind it. Pure: tested in tests/topology.test.ts.

export type NeighborRow = { interface?: string; address?: string; 'mac-address'?: string; identity?: string; platform?: string; board?: string; version?: string; uptime?: string; 'interface-name'?: string }
export type Placeable = { mac: string; port?: string; wired: boolean }

export type Downstream<C> = {
  id: string
  label: string
  sub: string
  /** `#/device/<MAC>` side panel, only for a well-formed MAC (neighbour data is untrusted) */
  href?: string
  port: string
  board?: string
  version?: string
  /** further network devices heard on the same port (daisy-chained switches or APs) */
  devices: Downstream<C>[]
  clients: C[]
}

const portOf = (i?: string) => (i ?? '').split(',')[0].trim()
const norm = (m?: string) => (m ?? '').toUpperCase()

export function groupByNeighbor<C extends Placeable>(clients: C[], neighbors: NeighborRow[]): { devices: Downstream<C>[]; direct: C[] } {
  const seen = new Set<string>()
  const byPort = new Map<string, Downstream<C>[]>()
  for (const n of neighbors) {
    const port = portOf(n.interface)
    const mac = norm(n['mac-address'])
    // a neighbor heard on the WAN, a tunnel or nowhere is not a downstream device
    if (!port || !mac || seen.has(mac)) continue
    seen.add(mac)
    const d: Downstream<C> = {
      id: `nb:${mac}`, label: n.identity || n.board || n.platform || mac, sub: n.address ?? '', href: neighborHref(n),
      port, board: n.board, version: n.version, devices: [], clients: [],
    }
    byPort.set(port, [...(byPort.get(port) ?? []), d])
  }
  const direct: C[] = []
  for (const c of clients) {
    if (seen.has(norm(c.mac))) continue // the device itself is drawn as a neighbor node
    const at = c.wired && c.port ? byPort.get(c.port) : undefined
    if (at) at[0].clients.push(c)
    else direct.push(c)
  }
  const devices: Downstream<C>[] = []
  for (const list of byPort.values()) {
    const [head, ...rest] = list
    head.devices = rest
    devices.push(head)
  }
  return { devices, direct }
}

// A neighbour's side panel is keyed by its MAC: the hash is user-editable and the MAC comes from another device,
// so both are checked against the strict form before use.
const MAC = /^[0-9A-F]{2}(:[0-9A-F]{2}){5}$/i
export const neighborHref = (n: NeighborRow) => (MAC.test(n['mac-address'] ?? '') ? `#/device/${norm(n['mac-address'])}` : undefined)
export const findNeighbor = <N extends NeighborRow>(list: N[], id: string): N | undefined =>
  MAC.test(id) ? list.find((n) => norm(n['mac-address']) === id.toUpperCase()) : undefined

/** https WebFig link for a neighbour's announced IPv4 address; anything else gets no link */
export function webfigUrl(address?: string): string | undefined {
  const m = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(address ?? '')
  return m && m.slice(1).every((o) => +o <= 255) ? `https://${m.slice(1).map(Number).join('.')}/` : undefined
}

/** IPv4 `ip` inside `cidr` ("192.0.2.1/24") */
export function inSubnet(ip: string, cidr: string): boolean {
  const n = (v: string) => v.split('.').reduce((a, o) => a * 256 + (+o), 0)
  const [net, bits] = cidr.split('/')
  const size = 2 ** (32 - Math.min(32, +bits || 32))
  return Math.floor(n(ip) / size) === Math.floor(n(net) / size)
}
