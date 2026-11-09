// Talks to the tikontrol server (src/server). When the UI runs without it (npm run dev, a static
// preview), `boot()` reports 'mock' and the pages fall back to snapshot.json / sample.json.

export type Me = {
  user: string
  role: 'admin' | 'viewer'
  csrf: string
  router: { host: string; mode: 'read-only' | 'read-write' } | null
  readOnly: boolean
  policies: { user: string; group: string; policy: string[] } | null
  error: string | null
  /** the temporary session opened with the setup token */
  setup?: boolean
  /** router address to pre-fill on first setup (TIK_ROUTER or the default gateway) */
  routerHint?: string | null
  /** signed in with a MikroTik account */
  mikrotik?: boolean
}

export type Boot =
  | { mode: 'mock' }
  | { mode: 'setup'; hint: string | null }
  | { mode: 'login' }
  | { mode: 'router'; me: Me }
  | { mode: 'app'; me: Me; snapshot: any }

export const session = { csrf: '', me: null as Me | null }

/** The router this browser looks at ('' = the main one). Every request carries it; the server scopes
 *  live data, history and Safe apply to it. */
export const site = localStorage.getItem('tik.site') ?? ''
export type Site = { id: string; name: string; identity: string | null; host: string | null; mode: string | null; error: string | null }
export function switchSite(id: string) {
  if (id) localStorage.setItem('tik.site', id)
  else localStorage.removeItem('tik.site')
  location.hash = '#/dashboard'
  location.reload()
}
/** The first snapshot, set by main.ts before the app (and data.ts) load. */
export const preload = { snapshot: null as any }

export class ApiError extends Error {
  constructor(public status: number, message: string, public data: any = null) { super(message) }
}

export async function api<T = any>(path: string, opts: { method?: string; body?: unknown } = {}): Promise<T> {
  const method = opts.method ?? 'GET'
  const headers: Record<string, string> = {}
  if (opts.body !== undefined) headers['content-type'] = 'application/json'
  if (method !== 'GET') headers['x-csrf-token'] = session.csrf
  if (site) headers['x-tik-site'] = site
  const r = await fetch(`/api/v1${path}`, { method, headers, body: opts.body === undefined ? undefined : JSON.stringify(opts.body), credentials: 'same-origin' })
  const text = await r.text()
  let data: any = null
  try { data = text ? JSON.parse(text) : null } catch { data = text }
  if (!r.ok) throw new ApiError(r.status, (data && data.error) || r.statusText, data)
  return data as T
}

export async function boot(): Promise<Boot> {
  let health: { needsSetup: boolean; routerHint?: string | null } | null = null
  try {
    const r = await fetch('/api/v1/health')
    if (r.ok && r.headers.get('content-type')?.includes('json')) health = await r.json()
  } catch { /* no server: mockup mode */ }
  if (!health) return { mode: 'mock' }
  try {
    const me = await api<Me>('/me')
    session.csrf = me.csrf
    session.me = me
    // the setup session only connects the router; using tikontrol needs a real sign-in
    if (!me.router || me.setup) return { mode: 'router', me }
    const snapshot = await api('/snapshot').catch(() => null)
    if (!snapshot) return { mode: 'router', me }
    return { mode: 'app', me, snapshot }
  } catch (e) {
    // the chosen router was removed (maybe from another browser): back to the main one
    if (e instanceof ApiError && e.status === 404 && site) { switchSite(''); return new Promise(() => {}) }
    if (e instanceof ApiError && e.status === 401) return health.needsSetup ? { mode: 'setup', hint: health.routerHint ?? null } : { mode: 'login' }
    throw e
  }
}

export async function logout() {
  await api('/logout', { method: 'POST', body: {} }).catch(() => {})
  location.reload()
}
