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

  const legacy = hw.wifiPackage !== 'wifi'
  const pkg = legacy ? 'wireless' : 'wifi'
  const NEW_PKG = `Needs the new wifi package (hAP ax/be). Not available on ${router.board ?? 'this router'}`
  const NOT_WIRED = 'Not wired in tikontrol yet'

  // real rows: the snapshot's wifi interfaces (normalizeWifi: auth, master band), bridge ports, VLANs, schedules
  const W: any[] = raw.wifi ?? []
  const pvidOf = (ifn: string) => +((raw.bridgePorts ?? []).find((p: any) => p.interface === ifn)?.pvid ?? 1)
  const networks: { id: number; name: string }[] = [{ id: 1, name: 'Default' }, ...(raw.vlans ?? []).map((v: any) => ({ id: +v['vlan-id'], name: String(v.comment ?? '').startsWith('tik:net:') ? v.comment.slice(8) : v.name }))]
  const netName = (id: number) => networks.find((n) => n.id === id)?.name ?? `VLAN ${id}`
  const secOf = (auth?: string[]) => (!auth ? 'wpa2' : !auth.length ? 'open' : auth.includes('wpa3-psk') ? (auth.includes('wpa2-psk') ? 'wpa2-wpa3' : 'wpa3') : 'wpa2')
  const mbit = (n: unknown) => (+(n ?? 0) ? `${+(+n! / 1e6).toFixed(1)}M` : '0')
  const sched = (ifn: string, on: boolean) => String((raw.scheduler ?? []).find((x: any) => x.name === `tik-wifi-${on ? 'on' : 'off'}-${ifn}`)?.['start-time'] ?? '').slice(0, 5)
  // physical radios (virtual APs ride on their master); one checkbox per band
  const phys = radios.filter((r) => !W.find((w) => w.name === r.name)?.['master-interface'])

  type Ssid = { name: string; password: string; security: string; bands: string[]; ifaces: string[]; network: number; hidden: boolean; isolation: boolean; guest: boolean; schedule: boolean; from: string; to: string; limit: string; macFilter: boolean }
  const fromRouter = (s: (typeof ssids)[number]): Ssid => {
    const rows = W.filter((w) => w.ssid === s.ssid), w0 = rows[0] ?? {}
    const lim = rows.length && (+w0['default-ap-tx-limit'] || +w0['default-client-tx-limit']) ? `${mbit(w0['default-ap-tx-limit'])}/${mbit(w0['default-client-tx-limit'])}` : ''
    return { name: s.ssid, password: '', security: secOf(w0.auth), bands: s.radios.map((r) => r.band), ifaces: s.radios.map((r) => r.name), network: w0.name ? pvidOf(w0.name) : 1, hidden: !!w0['hide-ssid'], isolation: w0['default-forwarding'] === false, guest: String(w0.comment ?? '').startsWith('tik:guest'), schedule: !!(w0.name && sched(w0.name, true)), from: (w0.name && sched(w0.name, true)) || '07:00', to: (w0.name && sched(w0.name, false)) || '23:00', limit: lim, macFilter: (raw.accessList ?? []).length > 0 }
  }
  let list = $state(ssids.map(fromRouter))
  let edit = $state<Ssid | null>(null)
  let editIdx = $state(-1)
  let qr = $state<Ssid | null>(null)
  let qrPass = $state('')

  function open(i: number) { editIdx = i; edit = i < 0 ? { ...fromRouter({ ssid: '', radios: phys.map((r) => ({ name: r.name, band: r.band, running: true })), clients: 0 }), name: '' } : structuredClone($state.snapshot(list[i])) }
  // Safe apply intents (server: src/server/src/intents/wifi.rs). One intent per review.
  function intentFor(s: Ssid): Intent | undefined {
    if (editIdx < 0) { const m = phys.find((r) => s.bands.includes(r.band)); return m && s.password.length >= 8 ? { kind: 'wifi-guest', package: pkg, master: m.name, ssid: s.name, passphrase: s.password } : undefined }
    const before = list[editIdx]
    // keep this SSID's own interfaces on ticked bands, add the physical radio of a newly ticked band, switch off unticked ones
    const keep = before.ifaces.filter((i) => s.bands.includes(radios.find((r) => r.name === i)?.band ?? ''))
    const added = phys.filter((r) => s.bands.includes(r.band) && !before.bands.includes(r.band)).map((r) => r.name)
    const ifs = [...keep, ...added]
    if (!ifs.length) return
    const [down, up] = s.limit ? s.limit.split('/') : ['0', '0']
    return {
      kind: 'wifi-network', package: pkg, interfaces: ifs,
      ...(s.name !== before.name || added.length ? { ssid: s.name } : {}),
      ...(s.hidden !== before.hidden ? { hidden: s.hidden } : {}),
      ...(s.isolation !== before.isolation ? { isolated: s.isolation } : {}),
      ...(s.security !== before.security && s.security !== 'open' ? { security: s.security } : {}),
      ...(s.password ? { passphrase: s.password } : {}),
      ...(before.ifaces.some((i) => !keep.includes(i)) ? { off: before.ifaces.filter((i) => !keep.includes(i)) } : {}),
      ...(s.network !== before.network ? { vlan: s.network } : {}),
      ...(legacy && s.limit !== before.limit ? { limit_down: down || '0', limit_up: up || down || '0' } : {}),
    }
  }
  // offline mockup only: with the server the review shows the commands it compiled
  const cmds = (i?: Intent) => (i ? [`# ${i.kind}: ${JSON.stringify({ ...i, kind: undefined, passphrase: i.passphrase ? '********' : undefined })}`] : ['# pick at least one band (and a password of 8+ characters for a new network)'])
  function save() {
    const i = intentFor(edit!)
    review(editIdx < 0 ? `Create SSID ${edit!.name}` : `Update SSID ${edit!.name}`, cmds(i), i)
    edit = null
  }
  function reviewSchedule(s: Ssid) {
    const r = s.ifaces[0]
    if (r) { const i: Intent = { kind: 'wifi-schedule', package: pkg, interface: r, on: s.from, off: s.to }; review(`Wi-Fi schedule for ${s.name}`, cmds(i), i) }
  }
  // WIFI: QR payload; \ ; , : " are escaped (zxing spec). The password is typed here and never leaves the browser.
  const esc = (v: string) => v.replace(/([\\;,:"])/g, '\\$1')
  const wifiQr = (s: Ssid) => `WIFI:T:${s.security === 'open' ? 'nopass' : 'WPA'};S:${esc(s.name)};${s.security === 'open' ? '' : `P:${esc(qrPass)};`}${s.hidden ? 'H:true;' : ''};`

  // per-radio channel / power
  let rf = $state(phys.map((r) => ({ name: r.name, band: r.band, widths: r.widths.filter((w) => w <= r.maxWidth), channel: 'auto', width: r.width ?? r.maxWidth, power: 'default', on: r.running })))
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
          <td>{netName(s.network)}</td>
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
    <Field label="Bands" help={editIdx < 0 ? 'A new network starts on one radio' : 'Unticking a band turns that radio off; ticking one renames the SSID that radio broadcasts'}>{#each phys as r}<label class="chk"><input type="checkbox" checked={edit.bands.includes(r.band)} onchange={() => (edit!.bands = edit!.bands.includes(r.band) ? edit!.bands.filter((b) => b !== r.band) : [...edit!.bands, r.band])} /> {r.band}</label>{/each}{#if !hw.bands.includes('6 GHz')}<label class="chk na" title="No 6 GHz radio on this router"><input type="checkbox" disabled /> 6 GHz</label>{/if}</Field>
    <Field label="Network" help={editIdx < 0 ? 'Set it after creating the network' : 'Bridge VLAN (PVID) for this SSID'}><select bind:value={edit.network} disabled={editIdx < 0}>{#each networks as n}<option value={n.id}>{n.name}{n.id > 1 ? ` (VLAN ${n.id})` : ''}</option>{/each}</select></Field>
    <Field label="Broadcasting APs" na="Only this router: more APs need MikroTik CMR/CAPsMAN"><span class="muted small">{router.identity}</span></Field>
    <Field label="Guest network" help="Isolated, internet only, optional portal"><Toggle bind:checked={edit.guest} disabled={editIdx >= 0} label="Guest" /></Field>
    <Field label="Client isolation" help="Clients can't see each other"><Toggle bind:checked={edit.isolation} label="Isolation" /></Field>
    <Field label="Hide SSID"><Toggle bind:checked={edit.hidden} label="Hide" /></Field>
    <details><summary class="small">Advanced</summary>
      <Field label="WiFi schedule"><Toggle bind:checked={edit.schedule} label="Schedule" /></Field>
      {#if edit.schedule}<Field label="On between" help="Applies to one radio"><input type="time" bind:value={edit.from} /><input type="time" bind:value={edit.to} /><button class="btn small" onclick={() => reviewSchedule(edit!)}>Review schedule</button></Field>{/if}
      <Field label="Speed limit per client" help="Download/upload, e.g. 50M/10M" na={legacy ? undefined : 'Not supported on the wifi package by tikontrol'}><input type="text" bind:value={edit.limit} placeholder="Unlimited" disabled={!legacy || editIdx < 0} /></Field>
      <Field label="MAC address filter" help="Access-list entries exist; manage them per device on the Clients page"><Toggle bind:checked={edit.macFilter} disabled label="MAC filter" /></Field>
      <Field label="Private PSK" na={legacy ? NEW_PKG : NOT_WIRED} help="Different password per device"><Toggle checked={false} disabled label="PPSK" /></Field>
      <Field label="Band steering / fast roaming (802.11r/k/v)" na={legacy ? NEW_PKG : NOT_WIRED}><Toggle checked={false} disabled label="Band steering" /></Field>
      <Field label="MLO (WiFi 7)" na={hw.wifiStandard === 'WiFi 7' ? 'RouterOS support for MLO on this board is unconfirmed' : 'Needs WiFi 7 hardware (e.g. hAP be³)'}><Toggle checked={false} disabled label="MLO" /></Field>
      <Field label="Hotspot 2.0 / Passpoint" na={legacy ? NEW_PKG : NOT_WIRED}><Toggle checked={false} disabled label="Hotspot 2.0" /></Field>
    </details>
    {#snippet footer()}<button class="btn" onclick={() => (edit = null)}>Cancel</button><button class="btn primary" disabled={!edit?.name} onclick={save}>Review & apply</button>{/snippet}
  </Modal>
{/if}

{#if qr}
  <Modal title="Join {qr.name}" onclose={() => { qr = null; qrPass = '' }}>
    <p class="muted small">tikontrol never reads Wi-Fi passwords from the router. Type it to make the code; it stays in this browser.</p>
    {#if qr.security !== 'open'}<Field label="Password"><input type="password" bind:value={qrPass} autocomplete="off" /></Field>{/if}
    <div class="qrbox">{#if qr.security === 'open' || qrPass.length >= 8}<Qr text={wifiQr(qr)} />{:else}<span class="muted small">Type the password to show the code</span>{/if}<span class="muted small">Point a phone camera at it</span></div>
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
