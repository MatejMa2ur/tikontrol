// Small UI choices (filters, tabs, search) kept in sessionStorage: the page is rebuilt when new router data
// arrives, and these must survive that. Per browser tab, gone when it closes.
export const kept = <T>(k: string, d: T): T => { try { const v = sessionStorage.getItem('tik:' + k); return v == null ? d : JSON.parse(v) } catch { return d } }
export const keep = (k: string, v: unknown) => { try { sessionStorage.setItem('tik:' + k, JSON.stringify(v)) } catch { /* storage full or off */ } }
