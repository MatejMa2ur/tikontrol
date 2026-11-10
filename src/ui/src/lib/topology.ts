// Topology grouping: network devices seen by MNDP/LLDP/CDP (`/ip/neighbor`) become nodes between
// this router and the clients behind them. A neighbor is reached through the port it was heard on
// (RouterOS 7 writes "ether2,bridge": the first part is the port), and every wired client the
// bridge host table puts on that same port sits behind it. Pure: tested in tests/topology.test.ts.

export type NeighborRow = { interface?: string; address?: string; 'mac-address'?: string; identity?: string; platform?: string; board?: string; version?: string }
export type Placeable = { mac: string; port?: string; wired: boolean }

export type Downstream<C> = {
  id: string
  label: string
  sub: string
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
      id: `nb:${mac}`, label: n.identity || n.board || n.platform || mac, sub: n.address ?? '',
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
