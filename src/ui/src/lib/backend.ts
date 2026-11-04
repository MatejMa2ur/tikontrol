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
}

export type Boot =
  | { mode: 'mock' }
  | { mode: 'setup' }
  | { mode: 'login' }
  | { mode: 'router'; me: Me }
  | { mode: 'app'; me: Me; snapshot: any }

export const session = { csrf: '', me: null as Me | null }
/** The first snapshot, set by main.ts before the app (and data.ts) load. */
export const preload = { snapshot: null as any }

export class ApiError extends Error {
  constructor(public status: number, message: string) { super(message) }
}

export async function api<T = any>(path: string, opts: { method?: string; body?: unknown } = {}): Promise<T> {
  const method = opts.method ?? 'GET'
  const headers: Record<string, string> = {}
  if (opts.body !== undefined) headers['content-type'] = 'application/json'
  if (method !== 'GET') headers['x-csrf-token'] = session.csrf
  const r = await fetch(`/api/v1${path}`, { method, headers, body: opts.body === undefined ? undefined : JSON.stringify(opts.body), credentials: 'same-origin' })
  const text = await r.text()
  let data: any = null
  try { data = text ? JSON.parse(text) : null } catch { data = text }
  if (!r.ok) throw new ApiError(r.status, (data && data.error) || r.statusText)
  return data as T
}

export async function boot(): Promise<Boot> {
  let health: { needsSetup: boolean } | null = null
  try {
    const r = await fetch('/api/v1/health')
    if (r.ok && r.headers.get('content-type')?.includes('json')) health = await r.json()
  } catch { /* no server: mockup mode */ }
  if (!health) return { mode: 'mock' }
  if (health.needsSetup) return { mode: 'setup' }
  try {
    const me = await api<Me>('/me')
    session.csrf = me.csrf
    session.me = me
    if (!me.router) return { mode: 'router', me }
    const snapshot = await api('/snapshot').catch(() => null)
    if (!snapshot) return { mode: 'router', me }
    return { mode: 'app', me, snapshot }
  } catch (e) {
    if (e instanceof ApiError && e.status === 401) return { mode: 'login' }
    throw e
  }
}

export async function logout() {
  await api('/logout', { method: 'POST', body: {} }).catch(() => {})
  location.reload()
}
