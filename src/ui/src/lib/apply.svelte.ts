// Safe apply (PLAN §3). With the tikontrol server and an intent, the server compiles and validates the
// change, arms a revert job on the router, applies it, and waits for "Keep" from this browser; without a
// confirmation the router reverts by itself. Without an intent (or without the server) the commands are
// shown for the user to run in RouterOS, nothing is sent.
import { api } from './backend'
import { isBackend } from './data'

export type Intent = { kind: string; [k: string]: unknown }
type Preview = { id: string; title: string; risk: 'low' | 'medium' | 'high'; warnings: string[]; commands: string[]; undo: string[]; revertible: boolean; window: number; canApply: boolean; reason: string | null }

export const apply = $state({
  open: false, title: '', cmds: [] as string[], armed: false, deadline: 0, done: [] as { title: string; at: string }[],
  intent: null as Intent | null, preview: null as Preview | null, busy: false, error: '', id: '',
})

export function review(title: string, cmds: string | string[], intent?: Intent) {
  apply.title = title
  apply.cmds = Array.isArray(cmds) ? cmds : [cmds] // one entry per command; RouterOS scripts may contain ';' themselves
  apply.intent = intent ?? null
  apply.preview = null
  apply.error = ''
  apply.open = true
  if (isBackend && intent) {
    apply.busy = true
    api<Preview>('/apply/preview', { method: 'POST', body: { intent } })
      .then((p) => { apply.preview = p; apply.title = p.title; apply.cmds = p.commands })
      .catch((e) => (apply.error = e.message))
      .finally(() => (apply.busy = false))
  }
}

export async function confirm() {
  if (isBackend && apply.preview) {
    apply.busy = true
    apply.error = ''
    try {
      const r = await api<{ state: string; deadline: number | null }>('/apply/start', { method: 'POST', body: { id: apply.preview.id } })
      apply.id = apply.preview.id
      apply.open = false
      if (r.state === 'armed' && r.deadline) { apply.armed = true; apply.deadline = r.deadline * 1000 }
      else apply.done = [{ title: apply.title, at: new Date().toLocaleTimeString() }, ...apply.done]
    } catch (e) { apply.error = (e as Error).message } finally { apply.busy = false }
    return
  }
  apply.open = false
  apply.armed = true
  apply.deadline = Date.now() + 5 * 60_000
}

export async function keep() {
  if (isBackend && apply.id) {
    try { await api('/apply/keep', { method: 'POST', body: { id: apply.id } }) } catch (e) { apply.error = (e as Error).message; return }
  }
  apply.armed = false
  apply.done = [{ title: apply.title, at: new Date().toLocaleTimeString() }, ...apply.done]
}

export async function revert() {
  if (isBackend && apply.id && apply.armed) await api('/apply/revert', { method: 'POST', body: { id: apply.id } }).catch(() => {})
  apply.armed = false
}
