// "Set up this device": the steps that bring a MikroTik found by /ip/neighbor under tikontrol.
// Pure, so `node --test tests/guide.test.ts` can load it. The API-SSL commands come from the server
// (policy::api_ssl_commands via /router/setup-commands?host=), never from here.
import { cmpVersion } from './security.ts'

export type Step = { title: string; why: string; cmds: string[]; add?: string }

/** A LAN or loopback IPv4 address: the only kind the guide links to the Routers form. */
export function privateIp(s: string): boolean {
  const m = s.match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/)
  if (!m) return false
  const [a, b, c, d] = m.slice(1).map(Number)
  if ([a, b, c, d].some((x) => x > 255)) return false
  return a === 10 || a === 127 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168)
}

/** RouterOS major version below 7 (RouterOS 6, or SwOS which reports 2.x): tikontrol cannot manage it. */
export const tooOld = (ver: string) => !!ver && cmpVersion(ver, '7') < 0

export function setupSteps(n: { version?: string; address?: string }, apiSsl: string[], self = ''): Step[] {
  const ver = (n.version ?? '').split(' ')[0]
  if (tooOld(ver)) return []
  const steps: Step[] = []
  if (!ver || cmpVersion(ver, '7.18') < 0) {
    steps.push(
      { title: 'Update RouterOS', why: `tikontrol reads routers over the API from RouterOS 7.18${ver ? ` and this one runs ${ver}` : ''}. The device downloads the update and reboots by itself (about 2 minutes).`,
        cmds: ['/system/package/update/check-for-updates', '/system/package/update/install'] },
      { title: 'Update the firmware', why: 'After it is back: brings the RouterBOARD firmware to the new version. It applies on the reboot (about 2 more minutes). A CHR has no RouterBOARD: skip this.',
        cmds: ['/system/routerboard/upgrade', '/system/reboot'] },
    )
  }
  steps.push({ title: 'Give API-SSL a certificate', why: `tikontrol talks to routers only over TLS (port 8729). This creates a certificate for API-SSL, allows it only from tikontrol${self ? ` (${self})` : ''} and turns off the plain-text API.`, cmds: apiSsl })
  const add = privateIp(n.address ?? '') ? n.address : undefined
  steps.push({ title: 'Add it as a router', why: 'Name it under Routers; its connect page opens with this address filled in. There you create a read-only user, check the certificate fingerprint and connect.', cmds: [], add })
  return steps
}
