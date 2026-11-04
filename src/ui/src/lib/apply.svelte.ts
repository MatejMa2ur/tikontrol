// Safe apply (PLAN §3), mocked: review commands → "apply" → 5-minute keep/revert countdown.
// ponytail: nothing is sent to the router yet; the backend will run these through the scheduler-armed revert.
export const apply = $state({ open: false, title: '', cmds: [] as string[], armed: false, deadline: 0, done: [] as { title: string; at: string }[] })

export function review(title: string, cmds: string | string[]) {
  apply.title = title
  apply.cmds = Array.isArray(cmds) ? cmds : [cmds] // one entry per command; RouterOS scripts may contain ';' themselves
  apply.open = true
}
export function confirm() {
  apply.open = false
  apply.armed = true
  apply.deadline = Date.now() + 5 * 60_000
}
export function keep() {
  apply.armed = false
  apply.done = [{ title: apply.title, at: new Date().toLocaleTimeString() }, ...apply.done]
}
export function revert() { apply.armed = false }
