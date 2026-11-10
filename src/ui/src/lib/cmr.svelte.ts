// MikroTik CMR fleet data (GET /api/v1/cmr, src/server/src/cmr.rs): managed devices, Device Supervisor
// alerts, upgrade rules and jobs, Wi-Fi networks with the APs their labels select, and app traffic.
// Without the server (mockup) or without a CMR server, `cmr.data` stays null and pages show how to set it up.
import { api } from './backend'
import { isBackend } from './data'

export type CmrDevice = { '.id': string; identity: string; ids: string; board?: string; version?: string; address?: string; labels?: string[]; 'auto-labels'?: string[]; online: boolean; paired: boolean; controller?: boolean; state?: string; 'upgrade-available'?: boolean; 'upgrade-rule'?: string; channel?: string; uptime?: string }
export type CmrAlert = { '.id': string; name: string; labels?: string[]; severity?: string; category?: string[]; disabled?: boolean; devices?: number; 'devices-on'?: number; fired?: number; comment?: string; active?: string[] }
export type CmrRule = { name: string; labels?: string; channel: string; 'schedule-time'?: string; strategy: string; 'fail-policy'?: string; dynamic?: boolean; comment?: string }
export type CmrJob = { labels?: string; channel: string; strategy: string; 'schedule-time'?: string; 'starts-in'?: string; state: string; success?: string }
export type CmrWifi = { ssid: string; labels?: string[]; disabled?: boolean; aps: string[] }
/** [down, up] bytes per period; null when RouterOS left the column out (80-column console) */
type Pair = [number, number] | null
export type CmrApp = { name: string; category: string | null; minute: Pair; hour: Pair; day: Pair; week: Pair }
export type Cmr = {
  available?: false; error?: string; source: 'router' | 'controller'
  controller: { host: string; port: number; reader: string; writer: string | null; mode: string } | null
  devices: CmrDevice[]; alerts: CmrAlert[]; upgrades: CmrRule[]; jobs: CmrJob[]; wifi: CmrWifi[]; apps: CmrApp[]; appsError?: string | null; apptrafficDevices?: string
}

export const cmr = $state({ data: null as Cmr | null, loading: false })

export function loadCmr() {
  if (!isBackend || cmr.loading) return
  cmr.loading = true
  api<Cmr>('/cmr').then((d) => (cmr.data = d)).catch(() => {}).finally(() => (cmr.loading = false))
}

/** CMR is reachable and answered (not just configured). */
export const cmrOk = () => !!cmr.data && cmr.data.available !== false
