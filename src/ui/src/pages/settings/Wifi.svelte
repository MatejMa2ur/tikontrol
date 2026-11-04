<script lang="ts">
  import { ssids, radios, raw, clients, hw, router } from '../../lib/data'
  import { displayName } from '../../lib/live.svelte'
  import { review, type Intent } from '../../lib/apply.svelte'
  import Icon from '../../lib/Icon.svelte'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'
  import Toggle from '../../lib/ui/Toggle.svelte'
  import Modal from '../../lib/ui/Modal.svelte'
  import Qr from '../../lib/ui/Qr.svelte'

  const sec: any = (raw.secProfiles ?? [])[0] ?? {}
  // ponytail: the commands below are still legacy /interface/wireless. Port them before writes reach a `wifi` package router.
  const legacy = hw.wifiPackage !== 'wifi' // the new `wifi` package unlocks the greyed options
  const NEW_PKG = `Needs the new wifi package (hAP ax/be). Not available on ${router.board}`

  type Ssid = { name: string; password: string; security: string; bands: string[]; network: string; hidden: boolean; isolation: boolean; guest: boolean; schedule: boolean; from: string; to: string; limit: string; macFilter: boolean; ppsk: boolean; steering: boolean; fastRoam: boolean; mlo: boolean; hs20: boolean; aps: string }
  const fromRouter = (s: (typeof ssids)[number]): Ssid => ({ name: s.ssid, password: '', security: sec['authentication-types']?.includes('wpa3-psk') || hw.bands.includes('6 GHz') ? 'wpa2-wpa3' : 'wpa2', bands: s.radios.map((r) => r.band), network: 'Default', hidden: false, isolation: false, guest: false, schedule: false, from: '07:00', to: '23:00', limit: '', macFilter: (raw.accessList ?? []).length > 0, ppsk: false, steering: false, fastRoam: false, mlo: false, hs20: false, aps: 'All' })
  let list = $state(ssids.map(fromRouter))
  let edit = $state<Ssid | null>(null)
  let editIdx = $state(-1)
  let qr = $state<Ssid | null>(null)
  let qrPass = $state('')

  function open(i: number) { editIdx = i; edit = i < 0 ? { ...fromRouter({ ssid: 'New network', radios: radios.map((r) => ({ name: r.name, band: r.band, running: true })), clients: 0 }), name: '' } : structuredClone($state.snapshot(list[i])) }
  function cmds(s: Ssid) {
    const radiosFor = radios.filter((r) => s.bands.includes(r.band))
    const prof = `tik-${s.name.replace(/\W/g, '')}`
    const out = [`/interface/wireless/security-profiles/add name=${prof} mode=dynamic-keys authentication-types=${s.security === 'wpa2' ? 'wpa2-psk' : 'wpa2-psk,wpa3-psk'} wpa2-pre-shared-key="••••••" ${s.security !== 'wpa2' ? 'wpa3-pre-shared-key="••••••"' : ''}`.trim()]
    if (editIdx >= 0) radiosFor.forEach((r) => out.push(`/interface/wireless/set ${r.name} ssid="${s.name}" security-profile=${prof} hide-ssid=${s.hidden ? 'yes' : 'no'} default-forwarding=${s.isolation ? 'no' : 'yes'}`))
    else radiosFor.forEach((r) => out.push(`/interface/wireless/add master-interface=${r.name} name=${prof}-${r.band[0]}g ssid="${s.name}" security-profile=${prof} hide-ssid=${s.hidden ? 'yes' : 'no'} default-forwarding=${s.isolation ? 'no' : 'yes'}`))
    if (s.network !== 'Default') out.push(`/interface/wireless/set [find ssid="${s.name}"] vlan-mode=use-tag vlan-id=${s.network}`)
    if (s.limit) out.push(`/interface/wireless/set [find ssid="${s.name}"] default-client-tx-limit=${s.limit.split('/')[0]} default-ap-tx-limit=${s.limit.split('/')[1] ?? s.limit.split('/')[0]}`)
    if (s.schedule) out.push(`/system/scheduler/add name=${prof}-off start-time=${s.to}:00 interval=1d on-event="/interface/wireless/disable [find ssid=\\"${s.name}\\"]"`, `/system/scheduler/add name=${prof}-on start-time=${s.from}:00 interval=1d on-event="/interface/wireless/enable [find ssid=\\"${s.name}\\"]"`)
    if (s.macFilter) out.push(`/interface/wireless/set [find ssid="${s.name}"] default-authentication=no`)
    return out
  }
  const pkg = hw.wifiPackage === 'wifi' ? 'wifi' : 'wireless'
  // Safe apply intents (server: src/server/src/intents/wifi.rs). One intent per review.
  function intentFor(s: Ssid): Intent | undefined {
    const ifs = radios.filter((r) => s.bands.includes(r.band)).map((r) => r.name)
    if (!ifs.length) return
    if (editIdx < 0) return s.password.length >= 8 ? { kind: 'wifi-guest', package: pkg, master: ifs[0], ssid: s.name, passphrase: s.password } : undefined
    const before = list[editIdx]
    return {
      kind: 'wifi-network', package: pkg, interfaces: ifs,
      ...(s.name !== before.name ? { ssid: s.name } : {}),
      ...(s.hidden !== before.hidden ? { hidden: s.hidden } : {}),
      ...(s.isolation !== before.isolation ? { isolated: s.isolation } : {}),
      ...(s.security !== before.security && s.security !== 'open' ? { security: s.security } : {}),
      ...(s.password ? { passphrase: s.password } : {}),
    }
  }
  function save() {
    review(editIdx < 0 ? `Create SSID ${edit!.name}` : `Update SSID ${edit!.name}`, cmds(edit!), intentFor(edit!))
    if (editIdx < 0) list.push(edit!); else list[editIdx] = edit!
    edit = null
  }
  function reviewSchedule(s: Ssid) {
    const r = radios.find((x) => s.bands.includes(x.band))
    if (r) review(`Wi-Fi schedule for ${s.name}`, cmds(s).filter((c) => c.startsWith('/system/scheduler')), { kind: 'wifi-schedule', package: pkg, interface: r.name, on: s.from, off: s.to })
  }
  const wifiQr = (s: Ssid) => `WIFI:T:${s.security === 'open' ? 'nopass' : 'WPA'};S:${s.name};P:${qrPass};H:${s.hidden ? 'true' : 'false'};;`

  // per-radio channel / power
  let rf = $state(radios.map((r) => ({ name: r.name, band: r.band, widths: r.widths.filter((w) => w <= r.maxWidth), channel: 'auto', width: r.width ?? r.maxWidth, power: 'default', on: r.running })))
  type Rf = (typeof rf)[number]
  const pkgPath = pkg === 'wifi' ? '/interface/wifi' : '/interface/wireless'
  const freqOf = (r: Rf) => (r.channel === 'auto' ? 'auto' : String((r.band.startsWith('2') ? 2407 : r.band.startsWith('6') ? 5950 : 5000) + 5 * +r.channel))
  const WIDTHS: Record<string, Record<number, string>> = {
    wifi: { 20: '20mhz', 40: '20/40mhz', 80: '20/40/80mhz', 160: '20/40/80/160mhz', 320: '20/40/80/160/320mhz' },
    wireless: { 20: '20mhz', 40: '20/40mhz-XX', 80: '20/40/80mhz-XXXX', 160: '20/40/80/160mhz-XXXXXXXX' },
  }
  const widthOf = (r: Rf) => WIDTHS[pkg][r.width] ?? '20mhz'
  const powerOf = (r: Rf) => (r.power === 'default' ? 'default' : String(r.power === 'high' ? 20 : r.power === 'medium' ? 14 : 8))
  const radioCli = (r: Rf) => pkg === 'wifi'
    ? `/interface/wifi/set [find name="${r.name}"] channel.frequency=${freqOf(r)} channel.width=${widthOf(r)}${r.power === 'default' ? '' : ` configuration.tx-power=${powerOf(r)}`}`
    : `/interface/wireless/set [find name="${r.name}"] frequency=${freqOf(r)} channel-width=${widthOf(r)}${r.power === 'default' ? ' tx-power-mode=default' : ` tx-power-mode=all-rates-fixed tx-power=${powerOf(r)}`}`
  const radioIntent = (r: Rf): Intent => ({ kind: 'wifi-radio', package: pkg, interface: r.name, frequency: freqOf(r), width: widthOf(r), tx_power: powerOf(r) })
  const channels = (band: string) => (band.startsWith('2') ? ['auto', '1', '6', '11'] : band.startsWith('6') ? ['auto', '5', '37', '69', '101', '133', '165', '197'] : ['auto', '36', '44', '52', '100', '116', '132', '149'])
</script>

<Section title="WiFi networks" desc="Each SSID can broadcast on one or more bands." ros="/interface/wireless/print; /interface/wireless/security-profiles/print">
  {#snippet actions()}<button class="btn primary" onclick={() => open(-1)}><Icon name="plus" size={14} /> Create New</button>{/snippet}
  <table>
    <thead><tr><th>Name</th><th>Security</th><th>Bands</th><th>Network</th><th>Clients</th><th></th></tr></thead>
    <tbody>
      {#each list as s, i}
        <tr onclick={() => open(i)}>
          <td><span class="row"><Icon name="wifi" size={15} /><b>{s.name}</b>{#if s.hidden}<span class="pill">hidden</span>{/if}{#if s.guest}<span class="pill">guest</span>{/if}</span></td>
          <td>{s.security === 'wpa2' ? 'WPA2' : s.security === 'wpa3' ? 'WPA3' : s.security === 'open' ? 'Open' : 'WPA2/WPA3'}</td>
          <td>{s.bands.join(' + ')}</td>
          <td>{s.network}</td>
          <td>{clients.filter((c) => c.ssid === s.name).length}</td>
          <td class="r"><button class="btn small" onclick={(e) => { e.stopPropagation(); qr = s; qrPass = '' }}>QR code</button></td>
        </tr>
      {/each}
    </tbody>
  </table>
</Section>

<Section title="Radios" desc="Channel, width and transmit power per band. Auto picks the quietest channel at boot." ros="/interface/wireless/set wlan2 frequency=… channel-width=… tx-power=…">
  {#each rf as r}
    <Field label="{r.band} · {r.name}">
      <select bind:value={r.channel}>{#each channels(r.band) as c}<option value={c}>{c === 'auto' ? 'Auto channel' : `Channel ${c}`}</option>{/each}</select>
      <select bind:value={r.width}>{#each r.widths as w}<option value={w}>{w} MHz</option>{/each}</select>
      <select bind:value={r.power}><option value="default">Auto power</option><option value="high">High</option><option value="medium">Medium</option><option value="low">Low</option></select>
      <button class="btn small" onclick={() => review(`Radio settings for ${r.name}`, [radioCli(r)], radioIntent(r))}>Apply</button>
      <button class="btn small" onclick={() => review(`${r.on ? 'Turn off' : 'Turn on'} ${r.name}`, [`${pkgPath}/${r.on ? 'disable' : 'enable'} [find name="${r.name}"]`], { kind: 'wifi-radio-enabled', package: pkg, interface: r.name, enabled: !r.on })}>{r.on ? 'Turn off' : 'Turn on'}</button>
    </Field>
  {/each}
  <div class="r"><button class="btn" onclick={() => review('Radio settings', rf.map(radioCli))}>Show all as commands</button></div>
</Section>

{#if edit}
  <Modal title={editIdx < 0 ? 'Create WiFi network' : `Edit ${edit.name}`} onclose={() => (edit = null)} wide>
    <Field label="Name (SSID)"><input type="text" bind:value={edit.name} /></Field>
    <Field label="Password" help="Never read back from the router"><input type="password" bind:value={edit.password} placeholder="unchanged" /></Field>
    <Field label="Security"><select bind:value={edit.security}><option value="wpa2">WPA2</option><option value="wpa2-wpa3">WPA2/WPA3</option><option value="wpa3">WPA3</option><option value="open">Open</option></select></Field>
    <Field label="Bands">{#each radios as r}<label class="chk"><input type="checkbox" checked={edit.bands.includes(r.band)} onchange={() => (edit!.bands = edit!.bands.includes(r.band) ? edit!.bands.filter((b) => b !== r.band) : [...edit!.bands, r.band])} /> {r.band}</label>{/each}<label class="chk na"><input type="checkbox" disabled /> 6 GHz</label></Field>
    <Field label="Network" help="VLAN for this SSID"><select bind:value={edit.network}><option>Default</option><option value="10">VLAN 10</option><option value="20">VLAN 20</option><option value="30">VLAN 30</option></select></Field>
    <Field label="Broadcasting APs"><select bind:value={edit.aps}><option>All</option><option>{radios[0] ? 'hAP' : ''}</option></select></Field>
    <Field label="Guest network" help="Isolated, internet only, optional portal"><Toggle bind:checked={edit.guest} label="Guest" /></Field>
    <Field label="Client isolation" help="Clients can't see each other"><Toggle bind:checked={edit.isolation} label="Isolation" /></Field>
    <Field label="Hide SSID"><Toggle bind:checked={edit.hidden} label="Hide" /></Field>
    <details><summary class="small">Advanced</summary>
      <Field label="WiFi schedule"><Toggle bind:checked={edit.schedule} label="Schedule" /></Field>
      {#if edit.schedule}<Field label="On between" help="Applies to one radio"><input type="time" bind:value={edit.from} /><input type="time" bind:value={edit.to} /><button class="btn small" onclick={() => reviewSchedule(edit!)}>Review schedule</button></Field>{/if}
      <Field label="Speed limit per client" help="e.g. 50M/10M"><input type="text" bind:value={edit.limit} placeholder="Unlimited" /></Field>
      <Field label="MAC address filter" help="Only allow listed devices"><Toggle bind:checked={edit.macFilter} label="MAC filter" /></Field>
      <Field label="Private PSK" na={legacy ? NEW_PKG : undefined} help="Different password per device"><Toggle bind:checked={edit.ppsk} disabled={legacy} label="PPSK" /></Field>
      <Field label="Band steering" na={legacy ? NEW_PKG : undefined}><Toggle bind:checked={edit.steering} disabled={legacy} label="Band steering" /></Field>
      <Field label="Fast roaming (802.11r/k/v)" na={legacy ? NEW_PKG : undefined}><Toggle bind:checked={edit.fastRoam} disabled={legacy} label="Fast roaming" /></Field>
      <Field label="MLO (WiFi 7)" na={hw.wifiStandard === 'WiFi 7' ? 'RouterOS support for MLO on this board is unconfirmed' : 'Needs WiFi 7 hardware (e.g. hAP be³)'}><Toggle bind:checked={edit.mlo} disabled label="MLO" /></Field>
      <Field label="Hotspot 2.0 / Passpoint" na={legacy ? NEW_PKG : undefined}><Toggle bind:checked={edit.hs20} disabled={legacy} label="Hotspot 2.0" /></Field>
    </details>
    {#snippet footer()}<button class="btn" onclick={() => (edit = null)}>Cancel</button><button class="btn primary" disabled={!edit?.name} onclick={save}>Review & apply</button>{/snippet}
  </Modal>
{/if}

{#if qr}
  <Modal title="Join {qr.name}" onclose={() => (qr = null)}>
    <p class="muted small">tikontrol never reads Wi-Fi passwords from the router. Type it to make the code; it stays in this browser.</p>
    <Field label="Password"><input type="password" bind:value={qrPass} /></Field>
    <div class="qrbox"><Qr text={wifiQr(qr)} /><span class="muted small">Point a phone camera at it</span></div>
  </Modal>
{/if}

<style>
  .r { text-align: right; margin-top: 10px; }
  .btn.small { padding: 3px 9px; }
  .chk { display: inline-flex; gap: 5px; align-items: center; margin-left: 10px; }
  .chk.na { opacity: .45; }
  details { margin-top: 6px; }
  summary { cursor: pointer; color: var(--muted); padding: 6px 0; }
  .small { font-size: 12px; }
  .qrbox { display: grid; justify-items: center; gap: 8px; margin-top: 12px; }
  .pill { margin-left: 6px; }
</style>
