// `?device=be3`: the sample network as if it ran on a hAP be³ Media, in the shape the `wifi` package reports.
// ponytail: simulated from MikroTik's spec sheet until there's a real unit to snapshot. Clients and traffic stay sample data.
export function simulateBe3(sample: any) {
  const d = JSON.parse(JSON.stringify(sample).replace(/wlan(\d)/g, 'wifi$1'))
  const ssid = d.wifi[0]?.ssid
  const stations = [...d.stations].sort((a, b) => parseInt(b['signal-strength']) - parseInt(a['signal-strength']))
  const close = stations.find((s) => s.interface === 'wifi2' && !s['mac-address'].startsWith('38:42:0B')) // not the Sonos: no 6 GHz radio
  if (close) close.interface = 'wifi3'
  const ether3 = d.etherMon?.find((m: any) => m.name === 'ether3')
  if (ether3) ether3.rate = '2.5Gbps'
  return {
    ...d,
    resource: {
      ...d.resource,
      'board-name': 'hAP be3 Media',
      'architecture-name': 'arm64',
      cpu: 'ARM64',
      'cpu-count': 4,
      'cpu-frequency': 1500,
      'total-memory': 2 * 2 ** 30,
      'free-memory': 1700 * 2 ** 20,
      'total-hdd-space': 512 * 2 ** 20,
      'free-hdd-space': 400 * 2 ** 20,
    },
    routerboard: { model: 'MA53UG+HbeH', 'current-firmware': d.resource.version, 'upgrade-firmware': d.resource.version },
    wifi: 'no such command prefix', // what snapshot.py stores when the legacy menu doesn't exist
    stations: 'no such command prefix',
    radios: 'no such command prefix',
    linkQuality: 'no such command prefix',
    secProfiles: 'no such command prefix',
    accessList: 'no such command prefix',
    wifiNew: [
      { name: 'wifi1', 'channel.band': '2ghz-be', 'channel.width': '20/40mhz', 'configuration.ssid': ssid, running: true, disabled: false },
      { name: 'wifi2', 'channel.band': '5ghz-be', 'channel.width': '20/40/80/160mhz', 'configuration.ssid': ssid, running: true, disabled: false },
      { name: 'wifi3', 'channel.band': '6ghz-be', 'channel.width': '20/40/80/160/320mhz', 'configuration.ssid': ssid, running: true, disabled: false },
    ],
    stationsNew: d.stations.map(({ 'signal-strength': s, ...rest }: any) => ({ ...rest, signal: parseInt(s) })),
  }
}
