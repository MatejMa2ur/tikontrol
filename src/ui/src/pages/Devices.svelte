<script lang="ts">
  import { router, internet, clients, radios, raw, bytes, dur, hw } from '../lib/data'
  import { live } from '../lib/live.svelte'
  import { review } from '../lib/apply.svelte'
  import Icon from '../lib/Icon.svelte'
  import Tabs from '../lib/ui/Tabs.svelte'
  import Section from '../lib/ui/Section.svelte'
  import Field from '../lib/ui/Field.svelte'
  import Toggle from '../lib/ui/Toggle.svelte'
  import Modal from '../lib/ui/Modal.svelte'
  import Ports from './Ports.svelte'
  import Radios from './Radios.svelte'

  let { tab = 'list' }: { tab?: string } = $props()
  const neighbors: any[] = raw.neighbors ?? []
  const memPct = Math.round((1 - router.memFree / router.memTotal) * 100)

  let drawer = $state(false)
  let dtab = $state<'overview' | 'ports' | 'settings'>('overview')
  let identity = $state(router.identity)
  let led = $state(true)
  let ssh = $state(true)
  const firstPort: string = (raw.ethernet ?? [])[0]?.name ?? 'ether1'
  let blinkPort = $state(firstPort)
  const q = (v: string) => `"${v.replace(/["\\$]/g, (c) => '\\' + c)}"`
  let adoptOpen = $state(false)

  // fleet upgrade rule (CMR /cmr/upgrade)
  let rule = $state({ channel: 'stable', time: '03:00', strategy: 'sequential', fail: 'stop', auto: false })
  let baseline = $state<string | null>(null)
</script>

<div class="grid">
  <div class="row between wrap"><h1>Devices</h1>
    <div class="row"><button class="btn" onclick={() => (adoptOpen = true)}><Icon name="search" size={14} /> Find devices</button></div>
  </div>
  <Tabs base="devices" active={tab} tabs={[{ id: 'list', label: 'MikroTik Devices', count: 1 + neighbors.length }, { id: 'ports', label: 'Ports' }, { id: 'radios', label: 'Radios', count: radios.length }, { id: 'updates', label: 'Updates' }, { id: 'supervisor', label: 'Supervisor' }]} />

  {#if tab === 'list'}
    <section class="card flush">
      <table>
        <thead><tr><th>Name</th><th>Model</th><th>IP address</th><th>Version</th><th>Uptime</th><th>CPU</th><th>Memory</th><th>Clients</th><th>Status</th></tr></thead>
        <tbody>
          <tr onclick={() => { drawer = true; dtab = 'overview' }}>
            <td><span class="row"><span class="icon-box"><Icon name="devices" /></span><span><b>{identity}</b><div class="muted small">Gateway · AP</div></span></span></td>
            <td>{router.board}<div class="muted small mono">{router.model}</div></td>
            <td class="mono">{internet.gatewayIp}</td>
            <td>{router.version}{#if router.firmware !== router.firmwareUpgrade}<div><span class="pill warn">firmware upgrade</span></div>{/if}</td>
            <td>{dur(router.uptimeS)}</td>
            <td>{live.cpu}%</td>
            <td><span class={memPct > 75 ? 'warnc' : ''}>{memPct}%</span></td>
            <td>{clients.filter((c) => c.online).length}</td>
            <td><span class="pill ok"><span class="dot ok"></span>Online</span></td>
          </tr>
          {#each neighbors as n}
            <tr><td><b>{n.identity}</b></td><td>{n.board ?? n.platform}</td><td class="mono">{n.address}</td><td>{n.version}</td><td>—</td><td>—</td><td>—</td><td>—</td><td><span class="pill">Not adopted</span></td></tr>
          {/each}
        </tbody>
      </table>
      {#if !neighbors.length}
        <div class="empty muted small">No other MikroTik devices discovered on the LAN (MNDP/LLDP). Plug in a MikroTik AP or switch and it shows up here, ready to adopt through CMR.</div>
      {/if}
    </section>
  {:else if tab === 'ports'}
    <Ports />
  {:else if tab === 'radios'}
    <Radios />
  {:else if tab === 'updates'}
    <Section title="Firmware" desc="RouterOS packages and RouterBOOT firmware for every managed device." ros="/system/package/update/check-for-updates; /system/routerboard/print">
      <table>
        <thead><tr><th>Device</th><th>RouterOS</th><th>Channel</th><th>RouterBOOT</th><th></th></tr></thead>
        <tbody><tr>
          <td><b>{identity}</b></td><td>{router.version}</td><td>{router.channel}</td>
          <td>{router.firmware}{#if router.firmware !== router.firmwareUpgrade} → <b>{router.firmwareUpgrade}</b>{/if}</td>
          <td class="r"><button class="btn small" onclick={() => review('Check for updates', '/system/package/update/check-for-updates')}>Check</button>
            {#if router.firmware !== router.firmwareUpgrade}<button class="btn primary small" onclick={() => review('Upgrade RouterBOOT', ['/system/routerboard/upgrade'], { kind: 'routerboot-upgrade' })}>Upgrade firmware</button>{/if}</td>
        </tr></tbody>
      </table>
    </Section>
    <Section title="Scheduled upgrades" desc="Rolling upgrades across the fleet in an auto-update window. Runs through a CMR upgrade rule." ros="/cmr/upgrade/add">
      <Field label="Automatic upgrades"><Toggle bind:checked={rule.auto} label="Automatic upgrades" /></Field>
      <Field label="Channel"><select bind:value={rule.channel}><option>stable</option><option>long-term</option><option>testing</option><option>development</option></select></Field>
      <Field label="Time of day"><input type="text" bind:value={rule.time} /></Field>
      <Field label="Strategy" help="Sequential upgrades one device at a time"><div class="seg"><button class:on={rule.strategy === 'sequential'} onclick={() => (rule.strategy = 'sequential')}>Sequential</button><button class:on={rule.strategy === 'parallel'} onclick={() => (rule.strategy = 'parallel')}>Parallel</button></div></Field>
      <Field label="If a device fails"><select bind:value={rule.fail}><option value="stop">Stop the rollout</option><option value="continue">Continue</option></select></Field>
      <div class="r"><button class="btn primary" onclick={() => review('Upgrade rule', `/cmr/upgrade/add name=nightly labels=all channel=${rule.channel} schedule-time="${rule.time}:00" strategy=${rule.strategy} fail-policy=${rule.fail}`)}>Save rule</button></div>
    </Section>
  {:else if tab === 'supervisor'}
    <Section title="Device Supervisor" desc="Health anomalies across devices: offline, high CPU/memory, failed upgrades. Backed by CMR state alerts." ros="/cmr/alert/print">
      <div class="row sup"><span class="dot ok"></span> {identity}: online, CPU {live.cpu}%, memory <span class={memPct > 75 ? 'warnc' : ''}>{memPct}%</span>{#if memPct > 75}<span class="pill warn">memory high</span>{/if}</div>
    </Section>
    <Section title="Config drift" desc="Take a baseline of the configuration, then see what changed since (Blueprints / Drift Inspector)." ros="/export terse">
      {#if baseline}
        <div class="row small"><Icon name="check" size={14} /> Baseline taken {baseline}. No drift detected.</div>
      {:else}<div class="muted small">No baseline yet.</div>{/if}
      <div class="r"><button class="btn" onclick={() => (baseline = new Date().toLocaleString())}>Take baseline</button></div>
    </Section>
  {/if}
</div>

{#if drawer}
  <button class="scrim" aria-label="Close" onclick={() => (drawer = false)}></button>
  <aside class="drawer" aria-label="Device details">
    <header class="row between">
      <div class="row"><span class="icon-box lg"><Icon name="devices" size={22} /></span><div><b class="title">{identity}</b><div class="muted small">{router.board} · RouterOS {router.version}</div></div></div>
      <button class="btn ghost" onclick={() => (drawer = false)} aria-label="Close"><Icon name="close" /></button>
    </header>
    <div class="seg dt">
      <button class:on={dtab === 'overview'} onclick={() => (dtab = 'overview')}>Overview</button>
      <button class:on={dtab === 'ports'} onclick={() => (dtab = 'ports')}>Ports</button>
      <button class:on={dtab === 'settings'} onclick={() => (dtab = 'settings')}>Settings</button>
    </div>
    {#if dtab === 'overview'}
      <dl>
        <dt>Model</dt><dd>{router.board} <span class="muted mono">{router.model}</span></dd>
        <dt>Hardware</dt><dd>{hw.specs}</dd>
        <dt>IP address</dt><dd class="mono">{internet.gatewayIp}</dd>
        <dt>Uptime</dt><dd>{dur(router.uptimeS)}</dd>
        <dt>CPU</dt><dd>{router.cpuCount} cores · {live.cpu}%</dd>
        <dt>Memory</dt><dd>{bytes(router.memFree)} free of {bytes(router.memTotal)}</dd>
        <dt>Storage</dt><dd>{bytes(router.hddFree)} free of {bytes(router.hddTotal)}</dd>
        <dt>Temperature</dt><dd class="muted">No sensor on this model</dd>
        <dt>Firmware</dt><dd>{router.firmware}{router.firmware !== router.firmwareUpgrade ? ` → ${router.firmwareUpgrade} pending` : ''}</dd>
      </dl>
      <div class="actions">
        <select bind:value={blinkPort} aria-label="Port to blink">{#each (raw.ethernet ?? [{ name: firstPort }]) as e}<option>{e.name}</option>{/each}</select>
        <button class="btn" onclick={() => review('Locate device', [`/interface/ethernet/blink [find name=${q(blinkPort)}] duration=10s`], { kind: 'locate', interface: blinkPort })}>Locate (blink)</button>
        <button class="btn" onclick={() => review('Restart device', ['/system/reboot'], { kind: 'reboot' })}>Restart</button>
      </div>
    {:else if dtab === 'ports'}
      <Ports compact />
    {:else}
      <Field label="Name"><input type="text" bind:value={identity} aria-label="Device name" /><button class="btn small" onclick={() => review('Device name', [`/system/identity/set name=${q(identity)}`], { kind: 'site-settings', identity })}>Save</button></Field>
      <Field label="LEDs"><Toggle bind:checked={led} label="LEDs" /><button class="btn small" onclick={() => review(led ? 'Turn the LEDs on' : 'Turn the LEDs off', [`/system/leds/settings/set all-leds-off=${led ? 'never' : 'immediate'}`], { kind: 'set-leds', all_leds_off: led ? 'never' : 'immediate' })}>Save</button></Field>
      <Field label="Device SSH" help="Key-based SSH for admins"><Toggle bind:checked={ssh} label="SSH" /></Field>
      <div class="r"><button class="btn" onclick={() => review('Device SSH', [`/ip/service/set ssh disabled=${ssh ? 'no' : 'yes'}`])}>Show SSH command</button></div>
    {/if}
  </aside>
{/if}

{#if adoptOpen}
  <Modal title="Find and adopt devices" onclose={() => (adoptOpen = false)}>
    <p class="small">tikontrol looks for MikroTik devices via MNDP/LLDP and CMR pairing requests.</p>
    <ol class="small steps">
      <li>On the new device: <code>/cmr/client set enabled=yes</code> (or plug it in; it finds the CMR server via MNDP).</li>
      <li>It appears here as <b>Pending</b>. Click <b>Adopt</b> to pair it (<code>/cmr/device/pair</code>).</li>
      <li>Wi-Fi, VLANs and upgrades are then pushed by labels.</li>
    </ol>
    <div class="empty muted small">{neighbors.length ? `${neighbors.length} found` : 'Nothing found yet. CMR server is not running (needs the cmr package on a CHR or larger router).'}</div>
  </Modal>
{/if}

<style>
  .flush { padding: 4px 6px; overflow-x: auto; }
  .empty { padding: 14px 10px; }
  .r { text-align: right; }
  .small { font-size: 12px; }
  .btn.small { padding: 4px 9px; }
  .warnc { color: var(--warn); }
  .sup { gap: 8px; }
  .scrim { position: fixed; inset: 0; background: #0006; border: 0; z-index: 20; }
  .drawer { position: fixed; top: 0; right: 0; bottom: 0; width: min(460px, 100vw); background: var(--panel); border-left: 1px solid var(--line); z-index: 21; padding: 18px; overflow-y: auto; box-shadow: var(--shadow); }
  .lg { width: 44px; height: 44px; }
  .title { font-size: 17px; }
  .dt { margin: 16px 0; }
  dl { display: grid; grid-template-columns: 110px 1fr; gap: 8px 12px; }
  dt { color: var(--muted); } dd { margin: 0; }
  .actions { display: flex; gap: 8px; margin-top: 16px; }
  .steps { line-height: 1.8; padding-left: 18px; }
  code { font: 11.5px var(--mono); background: var(--panel2); padding: 1px 5px; border-radius: 4px; }
</style>
