// Pure security logic for the Security page and the dashboard: the audit and the zone matrix.
// Works on the raw snapshot shape (src/server/src/ros/queries.rs); no imports, so `node --test` can load it.
// Tested in tests/security.test.ts.

/** A suggested change the UI can send through Safe apply (the server validates it again). */
export type Fix = { label: string; title: string; intent: { kind: string; [k: string]: unknown } }
export type Insight = { level: 'ok' | 'warn' | 'bad'; title: string; detail: string; ros: string; href?: string; fix?: Fix }

const on = (v: unknown) => v === true || v === 'true' || v === 'yes'
const off = (r: any) => on(r?.disabled)
const str = (v: unknown) => (typeof v === 'string' ? v : v == null ? '' : String(v))

/** -1, 0 or 1. Compares the numeric parts (7.9 < 7.20); a tag like "rc1" sorts before the release. */
export function cmpVersion(a: string, b: string): number {
  const parse = (v: string) => { const [, nums = '', tag = ''] = v.trim().match(/^([\d.]+)(.*)$/) ?? []; return { n: nums.split('.').filter(Boolean).map(Number), tag } }
  const x = parse(a), y = parse(b)
  for (let i = 0; i < Math.max(x.n.length, y.n.length); i++) {
    const d = (x.n[i] ?? 0) - (y.n[i] ?? 0)
    if (d) return Math.sign(d)
  }
  if (x.tag === y.tag) return 0
  return !x.tag ? 1 : !y.tag ? -1 : x.tag < y.tag ? -1 : 1
}

// Matchers that narrow a rule to some traffic. A rule with any of them is not a "whole zone" rule.
const NARROW = ['protocol', 'dst-port', 'src-address', 'src-address-list', 'dst-address', 'dst-address-list', 'src-mac-address', 'tls-host', 'time']
const states = (r: any): string[] => (Array.isArray(r['connection-state']) ? r['connection-state'] : str(r['connection-state']).split(',').filter(Boolean))
/** `list` (or `!other`) on a rule matches zone `z`; an empty matcher matches everything. */
const listMatches = (v: unknown, z: string) => { const s = str(v); return !s || (s.startsWith('!') ? s.slice(1) !== z : s === z) }
const isDrop = (r: any) => r.action === 'drop' || r.action === 'reject'

/** True when an enabled input rule drops everything new from the WAN side (defconf: "drop all not coming from LAN"). */
export function wanInputDropped(filter: any[]): boolean {
  return filter.some((r) => r.chain === 'input' && !off(r) && isDrop(r) && ['WAN', '!LAN'].includes(str(r['in-interface-list']))
    && NARROW.every((k) => !str(r[k])) && states(r).every((s) => s === 'new'))
}

/** True when an enabled input rule drops DNS (port 53) from the WAN side, or everything from it. */
export function wanDnsDropped(filter: any[]): boolean {
  if (wanInputDropped(filter)) return true
  return filter.some((r) => r.chain === 'input' && !off(r) && isDrop(r) && ['WAN', '!LAN'].includes(str(r['in-interface-list']))
    && ['', 'udp', 'tcp'].includes(str(r.protocol)) && str(r['dst-port']).split(',').some((p) => p.trim() === '53' || (/^\d+-\d+$/.test(p.trim()) && +p.split('-')[0] <= 53 && +p.split('-')[1] >= 53)))
}

// Management services: plain-text ones and the ones worth limiting with an `address` restriction.
const PLAIN = ['telnet', 'ftp', 'www']
const MGMT = ['api', 'winbox', 'ssh', 'api-ssl', 'www-ssl']
// what the server's disable-services intent accepts (src/server/src/apply.rs)
const DISABLABLE = ['telnet', 'ftp', 'www', 'api']

/** Security findings from a snapshot. Only problems, worst first; an empty list means nothing found. */
export function securityAudit(raw: any): Insight[] {
  const out: Insight[] = []
  const filter: any[] = Array.isArray(raw?.filter) ? raw.filter : []
  const svc: any[] = (Array.isArray(raw?.services) ? raw.services : []).filter((s: any) => !off(s) && !on(s.dynamic))
  const names = (list: string[]) => [...new Set(svc.filter((s) => list.includes(s.name)).map((s) => s.name as string))]
  const wanSafe = wanInputDropped(filter)

  const plain = names(PLAIN)
  if (plain.length) out.push({
    level: 'bad', title: `${plain.join(', ')} enabled`,
    detail: 'These services send your router password unencrypted. Disable them or limit them to the LAN.',
    ros: `/ip/service/disable ${plain.join(',')}`,
    fix: { label: 'Disable', title: `Disable ${plain.join(', ')}`, intent: { kind: 'disable-services', names: plain } },
  })
  if (names(['api']).length) out.push({
    level: 'warn', title: 'Plain API (port 8728) enabled',
    detail: 'The unencrypted API sends passwords in clear text. tikontrol uses api-ssl (8729); disable api unless another tool needs it.',
    ros: '/ip/service/disable api',
    fix: { label: 'Disable', title: 'Disable api', intent: { kind: 'disable-services', names: ['api'] } },
  })
  // the `address` field lists who may connect; empty = anyone who can reach the router
  const open = names(MGMT).filter((n) => svc.some((s) => s.name === n && !str(s.address).trim()))
  if (open.length) out.push({
    level: wanSafe ? 'warn' : 'bad', title: `${open.join(', ')} open to any address`,
    detail: wanSafe
      ? 'These services have no "Available from" restriction. The input firewall drops them from the internet, but every device on your LAN can try to log in. Limit them to your admin network.'
      : 'These services have no "Available from" restriction and no input rule drops traffic from the WAN: they may be reachable from the internet. Limit them to your LAN.',
    ros: `/ip/service/set [find name~"${open.join('|')}"] address=<your admin network>`,
  })
  if (on(raw?.dnsCfg?.['allow-remote-requests']) && !wanDnsDropped(filter)) out.push({
    level: 'bad', title: 'DNS server open to the internet',
    detail: 'The router answers DNS for other devices, and no input rule drops DNS (port 53) from the WAN. Open resolvers get abused for DDoS amplification.',
    ros: '/ip/firewall/filter/add chain=input in-interface-list=WAN protocol=udp dst-port=53 action=drop',
  })
  const users: any[] = Array.isArray(raw?.users) ? raw.users : []
  if (users.some((u) => u.name === 'admin' && !off(u))) out.push({
    level: 'warn', title: 'Default admin user still present',
    detail: 'Everyone knows the name "admin", so it is what password guessing tries first. Create your own full-access user, then disable or remove admin.',
    ros: '/user/print',
  })
  if (on(raw?.upnp?.enabled)) out.push({
    level: 'warn', title: 'UPnP is on',
    detail: 'Any device on the LAN, including a compromised one, can open ports to the internet without asking.',
    ros: '/ip/upnp/set enabled=no',
    fix: { label: 'Turn off', title: 'Disable UPnP', intent: { kind: 'set-upnp', enabled: false } },
  })
  const u = raw?.update ?? {}
  const latest = str(u['latest-version']), installed = str(u['installed-version'])
  if (latest && installed && cmpVersion(latest, installed) > 0) out.push({
    level: 'warn', title: `RouterOS ${latest} is available`,
    detail: `You run ${installed} on the ${str(u.channel) || 'stable'} channel. Updates fix security holes: read the changelog and take a backup before upgrading.`,
    ros: '/system/package/update/check-for-updates',
  })
  const rank = { bad: 0, warn: 1, ok: 2 }
  return out.sort((a, b) => rank[a.level] - rank[b.level])
}

// ---------- zones ----------
export type Policy = 'allow' | 'block' | 'default'
export type Cell = {
  /** what tikontrol set for this pair (forward only); 'default' = no tik:zone rule */
  policy: Policy
  /** what the rules make of new connections from → to */
  effect: 'Allow' | 'Block'
  /** where the effect comes from: 'tikontrol', a rule comment, or 'no rule' */
  why: string
  disabled?: boolean
}

/** Zones = interface lists that have members, LAN and WAN first. */
export function zonesOf(members: any[]): string[] {
  const lists = [...new Set(members.map((m) => str(m.list)).filter(Boolean))]
  return [...lists.filter((l) => l === 'LAN'), ...lists.filter((l) => l === 'WAN'), ...lists.filter((l) => l !== 'LAN' && l !== 'WAN').sort()]
}

/**
 * The policy for new connections from zone `from` to zone `to` (or 'Router' = the input chain),
 * read from the real filter rules: tikontrol's own `tik:zone:<from>:<to>` rule first, otherwise the
 * first enabled whole-zone rule in the chain (no ports, addresses or protocols), otherwise accept,
 * which is RouterOS's chain default. Approximate: an interface can be in more than one list.
 */
export function zoneCell(filter: any[], from: string, to: string): Cell {
  const input = to === 'Router'
  const chain = filter.filter((r) => r.chain === (input ? 'input' : 'forward'))
  const tag = !input && chain.find((r) => str(r.comment) === `tik:zone:${from}:${to}`)
  if (tag) return { policy: isDrop(tag) ? 'block' : 'allow', effect: off(tag) ? 'Allow' : isDrop(tag) ? 'Block' : 'Allow', why: off(tag) ? 'tikontrol (rule disabled)' : 'tikontrol', disabled: off(tag) }
  const hit = chain.find((r) => !off(r) && (r.action === 'accept' || isDrop(r)) && str(r['in-interface-list']) && listMatches(r['in-interface-list'], from)
    && (input || listMatches(r['out-interface-list'], to)) && NARROW.every((k) => !str(r[k])) && states(r).every((s) => s === 'new'))
  if (hit) return { policy: 'default', effect: isDrop(hit) ? 'Block' : 'Allow', why: str(hit.comment) || `${hit.chain} rule` }
  return { policy: 'default', effect: 'Allow', why: 'no rule' }
}
