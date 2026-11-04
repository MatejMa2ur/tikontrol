// Works out what we're running on from what the router reports, so no model needs configuring.
// Pure on purpose: tests/hardware.test.ts runs it under plain node.

// snapshot.py stores an error string when a menu doesn't exist on this router
const list = (v: unknown): any[] => (Array.isArray(v) ? v : [])

// RouterOS has two WiFi packages: legacy `wireless` (hAP ac²) and `wifi` (hAP ax/be).
// Map `wifi` onto the legacy shape so the rest of the UI only knows one.
// Each row also gets `auth` (authentication types; [] = open) and a virtual AP inherits its master's band.
export function normalizeWifi(raw: any) {
  const legacy = list(raw.wifi)
  const profiles = list(raw.secProfiles)
  const rows = legacy.length
    ? legacy.map((w) => { const p = profiles.find((x) => x.name === w['security-profile']); return { ...w, auth: p ? (p.mode === 'none' ? [] : list(p['authentication-types'])) : undefined } })
    : list(raw.wifiNew).map((w) => ({
      pkg: 'wifi',
      name: w.name,
      ssid: w['configuration.ssid'],
      band: w['channel.band'] ?? '',
      'channel-width': w['channel.width'],
      running: w.running,
      disabled: w.disabled,
      'master-interface': w['master-interface'],
      'hide-ssid': w['configuration.hide-ssid'],
      'default-forwarding': w['datapath.client-isolation'] == null ? undefined : !w['datapath.client-isolation'],
      auth: w['security.authentication-types'] == null ? undefined : list(w['security.authentication-types']),
      comment: w.comment,
    }))
  return {
    ...raw,
    wifi: rows.map((w) => ({ ...w, band: w.band || rows.find((m) => m.name === w['master-interface'])?.band || '' })),
    stations: legacy.length ? list(raw.stations) : list(raw.stationsNew).map((s) => ({ ...s, 'signal-strength': String(s.signal) })),
    // ponytail: the wifi monitor's field names are from the manual, unverified on hardware
    radios: legacy.length ? list(raw.radios) : list(raw.radiosNew),
    linkQuality: list(raw.linkQuality),
    secProfiles: list(raw.secProfiles),
    accessList: list(raw.accessList),
  }
}

// Monitor channel string -> numbers. Legacy `wireless`: "5640/20-eCee/ac/DP(24dBm)" (extension letters give the
// width, 1 letter = 20 MHz; flag D = DFS). The `wifi` package: "5180/ax/Ceee". Unknown parts stay undefined.
export function parseChannel(s: unknown) {
  const parts = String(s ?? '').split('/')
  const freq = parseInt(parts[0]) || undefined
  const ext = parts.map((p) => p.match(/^(?:\d+-)?([Ce]+)$/)?.[1]).find((x) => x?.includes('C'))
  const dbm = String(s ?? '').match(/\((\d+)dBm\)/)?.[1]
  return {
    freq,
    channel: freq ? (freq < 3000 ? (freq - 2407) / 5 : freq > 5900 ? (freq - 5950) / 5 : (freq - 5000) / 5) : undefined,
    width: ext ? 20 * ext.length : freq && parts[1] === '20' ? 20 : undefined,
    // 5260-5720 MHz (channels 52-144) need radar detection everywhere
    dfs: freq ? parts.some((p) => /^[A-Z]*D[A-Z]*(\(|$)/.test(p)) || (freq >= 5260 && freq <= 5720) : undefined,
    txPower: dbm ? +dbm : undefined,
  }
}

export const bandLabel = (band = '') => (band.startsWith('2') ? '2.4 GHz' : band.startsWith('6') ? '6 GHz' : '5 GHz')
export const standardOf = (band = '') => (/be/.test(band) ? 'WiFi 7' : /ax/.test(band) ? 'WiFi 6' : /ac/.test(band) ? 'WiFi 5' : 'WiFi 4')

const memory = (b: number) => (b >= 2 ** 30 ? `${Math.round(b / 2 ** 30)} GiB` : `${Math.round(b / 2 ** 20)} MiB`)

// Expects normalizeWifi() output
export function detectHardware(raw: any) {
  const res = raw.resource ?? {}
  const wifi = list(raw.wifi)
  const make: string = res.platform ?? 'MikroTik'
  const model: string | undefined = raw.routerboard?.model
  const arch: string | undefined = res['architecture-name']
  const cpuCount: number | undefined = res['cpu-count']
  const cpuMhz: number | undefined = res['cpu-frequency']
  const memTotal: number = res['total-memory'] ?? 0
  const standards = wifi.map((w) => standardOf(w.band)).sort()
  const wifiStandard = standards.at(-1)
  return {
    make, model, arch, cpuCount, cpuMhz, memTotal, wifiStandard,
    board: res['board-name'] as string | undefined,
    ports: list(raw.ethernet).length,
    bands: [...new Set(wifi.map((w) => bandLabel(w.band)))],
    wifiPackage: wifi.some((w) => w.pkg === 'wifi') ? 'wifi' : wifi.length ? 'wireless' : 'none',
    // ponytail: 32 MiB free is a guess at the cmr package size, measure it on real hardware
    canHostCmr: ['arm', 'arm64', 'x86', 'x86_64'].includes(arch ?? '') && (res['free-hdd-space'] ?? 0) >= 32 * 2 ** 20,
    specs: [
      [make, model].filter(Boolean).join(' '),
      arch,
      cpuCount && cpuMhz ? `${cpuCount}× ${cpuMhz} MHz` : undefined,
      memTotal ? memory(memTotal) : undefined,
      wifiStandard,
    ].filter(Boolean).join(' · '),
  }
}
