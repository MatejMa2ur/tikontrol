// Works out what we're running on from what the router reports, so no model needs configuring.
// Pure on purpose: tests/hardware.test.ts runs it under plain node.

// snapshot.py stores an error string when a menu doesn't exist on this router
const list = (v: unknown): any[] => (Array.isArray(v) ? v : [])

// RouterOS has two WiFi packages: legacy `wireless` (hAP ac²) and `wifi` (hAP ax/be).
// Map `wifi` onto the legacy shape so the rest of the UI only knows one.
export function normalizeWifi(raw: any) {
  const legacy = list(raw.wifi)
  return {
    ...raw,
    wifi: legacy.length ? legacy : list(raw.wifiNew).map((w) => ({
      pkg: 'wifi',
      name: w.name,
      ssid: w['configuration.ssid'],
      band: w['channel.band'] ?? '',
      'channel-width': w['channel.width'],
      running: w.running,
      disabled: w.disabled,
    })),
    stations: legacy.length ? list(raw.stations) : list(raw.stationsNew).map((s) => ({ ...s, 'signal-strength': String(s.signal) })),
    radios: list(raw.radios),
    linkQuality: list(raw.linkQuality),
    secProfiles: list(raw.secProfiles),
    accessList: list(raw.accessList),
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
