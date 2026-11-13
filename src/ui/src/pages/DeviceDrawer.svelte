<script lang="ts">
  // The router's side panel (#/device/router): opens over whatever page linked to it, like ClientDrawer.
  import { onMount } from 'svelte'
  import { keep, kept } from '../lib/keep'
  import { router, internet, raw, bytes, dur, hw, isBackend } from '../lib/data'
  import { api } from '../lib/backend'
  import { live } from '../lib/live.svelte'
  import { review, type Intent } from '../lib/apply.svelte'
  import { cmpVersion } from '../lib/security'
  import Icon from '../lib/Icon.svelte'
  import DeviceFace from '../lib/DeviceFace.svelte'
  import Field from '../lib/ui/Field.svelte'
  import Toggle from '../lib/ui/Toggle.svelte'
  import Modal from '../lib/ui/Modal.svelte'
  import Ports from './Ports.svelte'

  let { onclose: close }: { onclose: () => void } = $props()

  let panel: HTMLElement
  onMount(() => {
    const back = document.activeElement as HTMLElement | null
    panel.focus()
    return () => { if (back?.isConnected) back.focus() }
  })

  const memPct = $derived(live.mem ?? Math.round((1 - router.memFree / router.memTotal) * 100))
  // /system/health is feature-detected: missing menu or no sensor = no temperature (same rule as the server's live.rs)
  const health: { name: string; value: unknown }[] = Array.isArray(raw.health) ? raw.health : []
  const sensor = (want: (n: string) => boolean) => health.find((h) => want(String(h.name)))?.value
  const snapTemp = Number(sensor((n) => n === 'cpu-temperature') ?? sensor((n) => n === 'temperature') ?? sensor((n) => n.includes('temperature')) ?? NaN)
  const temp = $derived(live.temp ?? (Number.isFinite(snapTemp) ? snapTemp : null))

  // 24 h CPU / memory / temperature from the server's history (15-minute averages)
  type Hist = { t: number[]; cpu?: (number | null)[]; mem?: (number | null)[]; temp?: (number | null)[] }
  let hist = $state<Hist | null>(null)
  if (isBackend) api<Hist>('/history?range=1D').then((h) => (hist = h)).catch(() => {})
  function spark(v: (number | null)[] | undefined, max = 100) {
    if (!v?.some((x) => x != null)) return ''
    const hi = Math.max(max, ...v.map((x) => x ?? 0))
    return v.map((x, i) => (x == null ? null : `${((i / Math.max(v.length - 1, 1)) * 200).toFixed(1)},${(30 - (x / hi) * 28).toFixed(1)}`)).filter(Boolean).join(' ')
  }
  const last = (v?: (number | null)[]) => v?.filter((x) => x != null).at(-1)

  // Problems first, each with what to do about it
  const update = raw.update ?? {}
  const issues = $derived([
    ...(update['latest-version'] && update['installed-version'] && cmpVersion(update['latest-version'], update['installed-version']) > 0
      ? [{ level: 'warn', text: `RouterOS ${update['latest-version']} is available (running ${update['installed-version']}).`, href: '#/devices/updates', act: 'Updates' }] : []),
    ...(router.firmware && router.firmwareUpgrade && router.firmware !== router.firmwareUpgrade
      ? [{ level: 'warn', text: `RouterBOOT ${router.firmwareUpgrade} is waiting: it installs on the next restart after an upgrade.`, href: '#/devices/updates', act: 'Updates' }] : []),
    ...(memPct >= 90 ? [{ level: 'bad', text: `Memory is ${memPct}% used. Containers, big address lists or many connections are the usual cause.`, href: '#/events/alarms', act: 'Alarms' }] : []),
    ...(router.hddTotal && router.hddFree / router.hddTotal < 0.1 ? [{ level: 'warn', text: `Storage is almost full (${bytes(router.hddFree)} free). Old backups and package files take space.`, href: '#/settings/system', act: 'Backups' }] : []),
    ...(temp != null && temp >= 75 ? [{ level: 'warn', text: `${temp} °C is hot. Check airflow around the router.`, href: '', act: '' }] : []),
  ])

  type Tab = 'overview' | 'ports' | 'settings'
  let dtab = $state<Tab>(kept('devices.tab', 'overview'))
  $effect(() => { keep('devices.tab', dtab) })

  // Settings is one form: Cancel / Review & apply show once something changed, and apply as one batch
  const q = (v: string) => `"${v.replace(/["\\$]/g, (c) => '\\' + c)}"`
  const sshOn = !(raw.services ?? []).filter((s: any) => s.name === 'ssh').every((s: any) => String(s.disabled) === 'true')
  const initial = { identity: router.identity, led: true, ssh: sshOn }
  let form = $state({ ...initial })
  const parts = $derived([
    ...(form.identity.trim() !== initial.identity ? [{ cli: `/system/identity/set name=${q(form.identity.trim())}`, intent: { kind: 'site-settings', identity: form.identity.trim() } }] : []),
    ...(form.led !== initial.led ? [{ cli: `/system/leds/settings/set all-leds-off=${form.led ? 'never' : 'immediate'}`, intent: { kind: 'set-leds', all_leds_off: form.led ? 'never' : 'immediate' } }] : []),
    ...(form.ssh !== initial.ssh ? [{ cli: `/ip/service/set ssh disabled=${form.ssh ? 'no' : 'yes'}`, intent: { kind: 'set-service', name: 'ssh', enabled: form.ssh } }] : []),
  ])
  const invalid = $derived(!form.identity.trim() ? 'The name can\'t be empty.' : '')
  function applyForm() {
    const title = `${initial.identity} settings`
    const intents: Intent[] = parts.map((p) => p.intent)
    review(title, parts.map((p) => p.cli), intents.length === 1 ? intents[0] : { kind: 'batch', title, intents })
  }

  const firstPort: string = (raw.ethernet ?? [])[0]?.name ?? 'ether1'
  let blinkPort = $state(firstPort)

  // switch mode (server: intents/switchmode.rs): management stays on an address the bridge already has
  const bridges: string[] = (raw.bridges ?? []).map((b: any) => b.name)
  const inSwitchMode = (raw.filter ?? []).some((r: any) => r.comment === 'tik:switch')
  let sw = $state<{ bridge: string; address: string; gateway: string; dns: string; wifiOff: boolean } | null>(null)
  const bridgeAddrs = (b: string): string[] => (raw.addresses ?? []).filter((a: any) => a.interface === b && !a.dynamic).map((a: any) => a.address)
  function openSwitch() {
    const bridge = bridges[0] ?? 'bridge'
    sw = { bridge, address: bridgeAddrs(bridge)[0] ?? '', gateway: '', dns: '', wifiOff: true }
  }
  function reviewSwitch() {
    const s = sw!
    sw = null
    review('Use as a switch', [`# all ports into ${s.bridge}, managed at ${s.address}, default route via ${s.gateway}`], { kind: 'switch-mode', bridge: s.bridge, address: s.address, gateway: s.gateway, dns: s.dns || s.gateway, wifi: s.wifiOff ? 'off' : 'on' })
  }
</script>

<!-- a dialog opened from the panel closes first; one Esc closes one layer -->
<svelte:window onkeydown={(e) => e.key === 'Escape' && !document.querySelector('[role="dialog"]') && close()} />
<button class="side-scrim" aria-label="Close" onclick={close}></button>
<aside class="side-panel" aria-label="Device details" tabindex="-1" bind:this={panel}>
  <header class="row between">
    <div class="row"><span class="icon-box lg"><Icon name="devices" size={22} /></span><div><b class="title">{router.identity}</b><div class="muted small">{router.board} · RouterOS {router.version}</div></div></div>
    <button class="btn ghost" onclick={close} aria-label="Close"><Icon name="close" /></button>
  </header>
  <div class="seg dt" role="tablist" aria-label="Device sections">
    {#each [['overview', 'Overview'], ['ports', 'Ports'], ['settings', 'Settings']] as [id, label]}
      <button role="tab" aria-selected={dtab === id} class:on={dtab === id} onclick={() => (dtab = id as Tab)}>{label}</button>
    {/each}
  </div>
  {#if dtab === 'overview'}
    {#if issues.length}
      <ul class="issues" aria-label="Needs attention">
        {#each issues as i}<li class={i.level}><Icon name="alert" size={14} /><span>{i.text}</span>{#if i.href}<a class="btn small" href={i.href}>{i.act}</a>{/if}</li>{/each}
      </ul>
    {/if}
    <div class="face"><DeviceFace /></div>
    <dl>
      <dt>Model</dt><dd>{router.board} <span class="muted mono">{router.model}</span></dd>
      <dt>Hardware</dt><dd>{hw.specs}</dd>
      <dt>IP address</dt><dd class="mono">{internet.gatewayIp}</dd>
      <dt>Uptime</dt><dd>{dur(router.uptimeS)}</dd>
      <dt>CPU</dt><dd>{router.cpuCount} {router.cpuCount === 1 ? 'core' : 'cores'} · {live.cpu}%</dd>
      <dt>Memory</dt><dd>{bytes(router.memFree)} free of {bytes(router.memTotal)}</dd>
      <dt>Storage</dt><dd>{bytes(router.hddFree)} free of {bytes(router.hddTotal)}</dd>
      <dt>Temperature</dt>{#if temp != null}<dd>{temp} °C</dd>{:else}<dd class="muted">No sensor on this model</dd>{/if}
      <dt>Firmware</dt><dd>{router.firmware || 'No RouterBOOT (virtual router)'}{router.firmware && router.firmware !== router.firmwareUpgrade ? ` → ${router.firmwareUpgrade} pending` : ''}</dd>
    </dl>
    <h3 class="small">Last 24 hours</h3>
    {#if !isBackend}
      <p class="muted small">History is recorded by the tikontrol server.</p>
    {:else if !hist}
      <p class="muted small">Loading…</p>
    {:else}
      {#each [{ k: 'CPU', v: hist.cpu, unit: '%', max: 100 }, { k: 'Memory', v: hist.mem, unit: '%', max: 100 }, { k: 'Temperature', v: hist.temp, unit: ' °C', max: 60 }] as s}
        {@const pts = spark(s.v, s.max)}
        <div class="hrow"><span class="muted small">{s.k}</span>
          {#if pts}<svg viewBox="0 0 200 30" preserveAspectRatio="none" role="img" aria-label="{s.k} over the last 24 hours"><polyline points={pts} /></svg><span class="small mono">{last(s.v)}{s.unit}</span>
          {:else}<span class="muted small">{s.k === 'Temperature' ? 'No sensor' : 'No samples yet'}</span><span></span>{/if}
        </div>
      {/each}
    {/if}
  {:else if dtab === 'ports'}
    <Ports compact />
  {:else}
    <Field label="Name"><input type="text" bind:value={form.identity} aria-label="Device name" aria-invalid={!!invalid} /></Field>
    <Field label="LEDs"><Toggle bind:checked={form.led} label="LEDs" /></Field>
    <Field label="Device SSH" help="Key-based SSH for admins"><Toggle bind:checked={form.ssh} label="SSH" /></Field>
    {#if parts.length}
      <div class="footer">
        {#if invalid}<span class="small bad" role="alert">{invalid}</span>{:else}<span class="muted small">{parts.length} change{parts.length > 1 ? 's' : ''}</span>{/if}
        <button class="btn" onclick={() => (form = { ...initial })}>Cancel</button>
        <button class="btn primary" disabled={!!invalid} onclick={applyForm}>Review & apply</button>
      </div>
    {/if}

    <h3 class="small">Manage</h3>
    <div class="manage">
      <div><b>Locate</b><div class="muted small">Blinks a port's LED so you can find the router.</div></div>
      <div class="row"><select bind:value={blinkPort} aria-label="Port to blink">{#each (raw.ethernet ?? [{ name: firstPort }]) as e}<option>{e.name}</option>{/each}</select>
        <button class="btn" onclick={() => review('Locate device', [`/interface/ethernet/blink [find name=${q(blinkPort)}]`], { kind: 'locate', interface: blinkPort })}>Locate</button></div>
      <div><b>Restart</b><div class="muted small">Everything on the network drops for about a minute.</div></div>
      <div><button class="btn" onclick={() => review('Restart device', ['/system/reboot'], { kind: 'reboot' })}>Restart…</button></div>
      {#if inSwitchMode}
        <div><b>Leave switch mode</b><div class="muted small">Turns its DHCP, NAT, addresses and WAN port back on.</div></div>
        <div><button class="btn" onclick={() => review('Leave switch mode', ['# turn back on what switch mode turned off'], { kind: 'leave-switch-mode' })}>Leave…</button></div>
      {:else}
        <div><b>Use as a switch</b><div class="muted small">Every port on one bridge behind another router: no DHCP, NAT or routing.</div></div>
        <div><button class="btn" onclick={openSwitch}>Set up…</button></div>
      {/if}
    </div>
  {/if}
</aside>

{#if sw}
  <Modal title="Use {router.identity} as a switch" onclose={() => (sw = null)}>
    <p class="small">Every ethernet port joins the bridge. DHCP servers and clients, NAT, the other addresses and the WAN port are turned off, not deleted. The preview lists each change; it reverts by itself unless you keep it.</p>
    <Field label="Bridge"><select aria-label="Bridge" bind:value={sw.bridge} onchange={() => (sw!.address = bridgeAddrs(sw!.bridge)[0] ?? '')}>{#each bridges as b}<option>{b}</option>{/each}</select></Field>
    <Field label="Management address" help="One the bridge already has: the one you reach this router on">
      <select aria-label="Management address" bind:value={sw.address}>{#each bridgeAddrs(sw.bridge) as a}<option>{a}</option>{:else}<option value="">No address on this bridge</option>{/each}</select></Field>
    <Field label="Gateway" help="The router that serves this network"><input type="text" aria-label="Gateway" bind:value={sw.gateway} placeholder="192.168.88.1" /></Field>
    <Field label="DNS server" help="Empty: the gateway"><input type="text" aria-label="DNS server" bind:value={sw.dns} placeholder={sw.gateway || '192.168.88.1'} /></Field>
    <Field label="Turn Wi-Fi off"><Toggle bind:checked={sw.wifiOff} label="Turn Wi-Fi off" /></Field>
    <div class="r">{#if !sw.address || !sw.gateway}<span class="muted small">Pick a management address and enter the gateway. </span>{/if}<button class="btn primary" disabled={!sw.address || !sw.gateway} onclick={reviewSwitch}>Review</button></div>
  </Modal>
{/if}

<style>
  .r { text-align: right; }
  .small { font-size: 12px; }
  .btn.small { padding: 4px 9px; }
  .bad { color: var(--bad); }
  .lg { width: 44px; height: 44px; }
  .title { font-size: 17px; }
  .dt { margin: 16px 0; }
  .face { margin: 0 0 16px; }
  dl { display: grid; grid-template-columns: 110px 1fr; gap: 8px 12px; }
  dt { color: var(--muted); } dd { margin: 0; }
  h3 { margin: 18px 0 6px; }
  .hrow { display: grid; grid-template-columns: 90px 1fr 60px; gap: 10px; align-items: center; padding: 3px 0; }
  .hrow svg { width: 100%; height: 30px; }
  .hrow polyline { fill: none; stroke: var(--accent); stroke-width: 1.5; vector-effect: non-scaling-stroke; }
  .hrow .mono { text-align: right; }
  .issues { list-style: none; margin: 0 0 14px; padding: 0; display: grid; gap: 6px; }
  .issues li { display: flex; gap: 8px; align-items: center; padding: 8px 10px; border-radius: 8px; border: 1px solid var(--line); border-left: 3px solid var(--warn); font-size: 12.5px; }
  .issues li.bad { border-left-color: var(--bad); }
  .issues li span { flex: 1; }
  .footer { position: sticky; bottom: -18px; display: flex; gap: 8px; align-items: center; justify-content: flex-end; margin: 12px -18px 0; padding: 10px 18px; background: var(--panel); border-top: 1px solid var(--line); }
  .footer span { margin-right: auto; }
  .manage { display: grid; grid-template-columns: 1fr auto; gap: 12px 10px; align-items: center; }
  .manage .row { gap: 6px; }
</style>
