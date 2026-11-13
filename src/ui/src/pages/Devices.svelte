<script lang="ts">
  import { router, internet, clients, radios, raw, dur, isBackend, neighbors } from '../lib/data'
  import { api, switchSite, type Site } from '../lib/backend'
  import { live } from '../lib/live.svelte'
  import { review } from '../lib/apply.svelte'
  import { cmr, loadCmr, cmrOk, type CmrDevice } from '../lib/cmr.svelte'
  import { cmpVersion } from '../lib/security'
  import { privateIp, setupSteps, tooOld } from '../lib/guide'
  import { neighborHref } from '../lib/topology'
  import Icon from '../lib/Icon.svelte'
  import Tabs from '../lib/ui/Tabs.svelte'
  import Section from '../lib/ui/Section.svelte'
  import Field from '../lib/ui/Field.svelte'
  import Modal from '../lib/ui/Modal.svelte'
  import Ports from './Ports.svelte'
  import Radios from './Radios.svelte'

  let { tab = 'list' }: { tab?: string } = $props()
  const memPct = $derived(live.mem ?? Math.round((1 - router.memFree / router.memTotal) * 100))
  const q = (v: string) => `"${v.replace(/["\\$]/g, (c) => '\\' + c)}"`
  let adoptOpen = $state(false)

  // MikroTik CMR (src/server/src/cmr.rs): managed devices, adoption, upgrade rules, Device Supervisor
  $effect(() => { loadCmr() })
  // when the router itself is the CMR server, its own CMR entry is the first row already
  const fleet = $derived((cmr.data?.devices ?? []).filter((d) => !(d.controller && cmr.data?.source === 'router')))
  // routers this tikontrol already manages (Settings → Control Plane → Routers): a neighbour among them gets Open, not Set up
  let sites = $state<Site[]>([])
  if (isBackend) api<Site[]>('/sites').then((r) => (sites = r)).catch(() => {})
  // match on address: two MikroTiks can share a name (both "hAP")
  const same = (d: any, n: any) => (d.address && n.address ? String(d.address).split('/')[0] === n.address : d.identity === n.identity)
  const strangers = $derived(neighbors.filter((n) => !fleet.some((d) => same(d, n))))
  const pending = $derived(fleet.filter((d) => !d.paired))
  let adopt = $state<{ d: CmrDevice; user: string; pass: string } | null>(null)
  let labelEdit = $state<{ d: CmrDevice; text: string } | null>(null)
  const labelList = (t: string) => t.split(',').map((x) => x.trim()).filter(Boolean)
  const DAYS = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun']
  let rule = $state({ name: 'nightly', labels: '', channel: 'stable', time: '03:00', days: ['sun'] as string[], strategy: 'sequential', fail: 'stop' })
  const ruleIntent = () => ({ kind: 'cmr-upgrade-rule', name: rule.name, labels: labelList(rule.labels), channel: rule.channel, time: rule.time, days: rule.days, strategy: rule.strategy, fail_policy: rule.fail })
  let alert = $state({ name: 'CPU high', metric: 'cpu', threshold: 90, severity: 'high', labels: '' })
  const tik = (c?: string) => !!c?.startsWith('tik:')

  // "Set up this device" for a not-adopted neighbour (lib/guide.ts); API-SSL commands come from the server
  let guide = $state.raw<{ n: any; ssl: string[]; self: string } | null>(null)
  function openGuide(n: any) {
    guide = { n, ssl: [], self: '' }
    if (isBackend && privateIp(n.address ?? '')) api<{ commands: string[]; self: string }>(`/router/setup-commands?host=${encodeURIComponent(n.address)}`).then((r) => { if (guide?.n === n) guide = { n, ssl: r.commands, self: r.self } }).catch(() => {})
  }
  const steps = $derived(guide ? setupSteps(guide.n, guide.ssl, guide.self) : [])
  const copy = (cmds: string[]) => navigator.clipboard?.writeText(cmds.filter((c) => !c.startsWith('#')).join('\n'))

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
          <tr onclick={() => (location.hash = '#/device/router')}>
            <td><span class="row"><span class="icon-box"><Icon name="devices" /></span><span><a class="plain" href="#/device/router" onclick={(e) => e.stopPropagation()}><b>{router.identity}</b></a><div class="muted small">{internet.behind ? 'Switch' : 'Gateway'}{(raw.wifi ?? []).some((w: any) => String(w.disabled) !== 'true') ? ' · AP' : ''}</div></span></span></td>
            <td>{router.board}<div class="muted small mono">{router.model}</div></td>
            <td class="mono">{internet.gatewayIp}</td>
            <td>{router.version}{#if router.firmware !== router.firmwareUpgrade}<div><span class="pill warn">firmware upgrade</span></div>{/if}</td>
            <td>{dur(router.uptimeS)}</td>
            <td>{live.cpu}%</td>
            <td><span class={memPct > 75 ? 'warnc' : ''}>{memPct}%</span></td>
            <td>{clients.filter((c) => c.online).length}</td>
            <td><span class="pill ok"><span class="dot ok"></span>Online</span></td>
          </tr>
          {#each fleet as d}
            <tr>
              <td><b>{d.identity}</b><div class="muted small">{d.controller ? 'CMR server' : (d.labels ?? []).join(', ') || 'CMR device'}</div></td>
              <td>{d.board}</td><td class="mono">{d.address ?? '—'}</td><td>{d.version}</td><td>{d.uptime ?? '—'}</td><td>—</td><td>—</td><td>—</td>
              <td>
                {#if !d.paired}<span class="pill warn" title={d.state}>Pending</span> <button class="btn primary small" onclick={() => (adopt = { d, user: 'admin', pass: '' })}>Adopt</button>
                {:else}<span class="pill {d.online ? 'ok' : 'bad'}"><span class="dot {d.online ? 'ok' : 'bad'}"></span>{d.online ? 'Online' : 'Offline'}</span>
                  {#if !d.controller}<button class="btn small" onclick={() => (labelEdit = { d, text: (d.labels ?? []).join(', ') })}>Labels</button>{/if}{/if}
              </td>
            </tr>
          {/each}
          {#each strangers as n}
            {@const ver = (n.version ?? '').split(' ')[0]}
            {@const known = sites.find((x) => x.host === n.address)}
            {@const href = neighborHref(n)}
            <tr onclick={() => (href ? (location.hash = href) : known ? switchSite(known.id) : openGuide(n))}><td>{#if href}<a class="plain" {href} onclick={(e) => e.stopPropagation()}><b>{n.identity}</b></a>{:else}<b>{n.identity}</b>{/if}<div class="muted small">Not managed here</div></td><td>{n.board ?? n.platform}</td><td class="mono">{n.address}</td>
              <td>{ver}{#if tooOld(ver)}<br /><span class="pill bad" title="tikontrol needs RouterOS 7">RouterOS 7 needed</span>{:else if ver && cmpVersion(ver, '7.18') < 0}<br /><span class="pill warn" title="tikontrol reads routers over the API from RouterOS 7.18">update to 7.18+</span>{/if}</td>
              <td>—</td><td>—</td><td>—</td><td>—</td>
              <td>{#if known}<span class="pill ok">In tikontrol</span> <button class="btn small" onclick={(e) => { e.stopPropagation(); switchSite(known.id) }} aria-label="Open {n.identity}">Open</button>
                {:else}<span class="pill">Not adopted</span> <button class="btn small" onclick={(e) => { e.stopPropagation(); openGuide(n) }} aria-label="Set up {n.identity}">Set up</button>{/if}</td></tr>
          {/each}
        </tbody>
      </table>
      {#if cmr.data?.available === false}
        <div class="empty muted small">No CMR server: {cmr.data.error}. Other devices' stats and adoption need the cmr package (7.26+) on this router or a controller set in Settings → Control plane.</div>
      {/if}
      {#if !neighbors.length && !fleet.length}
        <div class="empty muted small">No other MikroTik devices discovered on the LAN (MNDP/LLDP). Plug in a MikroTik AP or switch and it shows up here, ready to adopt through CMR.</div>
      {/if}
    </section>
  {:else if tab === 'ports'}
    <Ports />
  {:else if tab === 'radios'}
    <Radios />
  {:else if tab === 'updates'}
    <Section title="Firmware" desc="RouterOS packages and RouterBOOT firmware for every managed device." ros="/system/package/update/print; /system/routerboard/print">
      <table>
        <thead><tr><th>Device</th><th>RouterOS</th><th>Channel</th><th>RouterBOOT</th><th></th></tr></thead>
        <tbody><tr>
          <td><b>{router.identity}</b></td><td>{router.version}</td><td>{router.channel}</td>
          <td>{router.firmware}{#if router.firmware !== router.firmwareUpgrade} → <b>{router.firmwareUpgrade}</b>{/if}</td>
          <td class="r"><button class="btn small" onclick={() => review('Check for updates', '/system/package/update/check-for-updates', { kind: 'check-updates' })}>Check</button>
            {#if router.firmware !== router.firmwareUpgrade}<button class="btn primary small" onclick={() => review('Upgrade RouterBOOT', ['/system/routerboard/upgrade'], { kind: 'routerboot-upgrade' })}>Upgrade firmware</button>{/if}</td>
        </tr></tbody>
      </table>
    </Section>
    {#if cmrOk()}
      <Section title="Fleet versions" desc="Every device the CMR server manages, with the upgrade rule that claims it." ros="/cmr/device/print">
        <table>
          <thead><tr><th>Device</th><th>RouterOS</th><th>Channel</th><th>Rule</th><th></th></tr></thead>
          <tbody>{#each fleet.filter((d) => d.paired) as d}<tr>
            <td><b>{d.identity}</b></td><td>{d.version}</td><td>{d.channel ?? '—'}</td><td>{d['upgrade-rule'] ?? '—'}</td>
            <td class="r">{#if d['upgrade-available']}<button class="btn primary small" onclick={() => review(`Upgrade ${d.identity} now`, [`/cmr/device/upgrade [find ids="${d.ids}"]`], { kind: 'cmr-upgrade-device', device: d.ids })}>Upgrade now</button>{:else}<span class="muted small">Up to date</span>{/if}</td>
          </tr>{/each}</tbody>
        </table>
      </Section>
    {/if}
    <Section title="Scheduled upgrades" desc="Rolling upgrades across the fleet in an update window (CMR upgrade rules). Rules are checked in order; the first that matches a device claims it." ros="/cmr/upgrade/print; /cmr/upgrade/job/print">
      {#if cmrOk()}
        <table>
          <thead><tr><th>Rule</th><th>Devices</th><th>Channel</th><th>When</th><th>Strategy</th><th></th></tr></thead>
          <tbody>{#each cmr.data!.upgrades as u}<tr>
            <td><b>{u.name}</b>{#if u.dynamic}<div class="muted small">default</div>{/if}</td><td>{u.labels ?? 'all'}</td><td>{u.channel}</td><td class="mono small">{u['schedule-time'] ?? 'on demand'}</td><td>{u.strategy}{u['fail-policy'] ? ` · ${u['fail-policy']} on failure` : ''}</td>
            <td class="r">{#if tik(u.comment)}<button class="btn small" onclick={() => review(`Remove upgrade rule ${u.name}`, [`/cmr/upgrade/remove [find comment="${u.comment}"]`], { kind: 'cmr-remove-upgrade-rule', name: u.name })}>Remove</button>{/if}</td>
          </tr>{/each}</tbody>
        </table>
        {#each cmr.data!.jobs as j}<div class="small muted">Job: {j.labels ?? 'all'} → {j.channel}, {j.state}{j['starts-in'] ? ` (starts in ${j['starts-in']})` : ''}{j.success ? `, ${j.success} done` : ''}</div>{/each}
        <h3 class="small">New rule</h3>
        <Field label="Name"><input type="text" bind:value={rule.name} /></Field>
        <Field label="Devices" help="CMR labels, comma-separated: office,+ap = office AND ap, -core = not core. Empty = all"><input type="text" bind:value={rule.labels} placeholder="all" /></Field>
        <Field label="Channel" help="Or a pinned version, e.g. 7.24.4"><input type="text" list="channels" bind:value={rule.channel} /><datalist id="channels"><option>stable</option><option>long-term</option><option>testing</option><option>development</option></datalist></Field>
        <Field label="Window" help="No day = only when triggered"><input type="time" bind:value={rule.time} aria-label="Time of day" />{#each DAYS as d}<label class="chk"><input type="checkbox" checked={rule.days.includes(d)} onchange={() => (rule.days = rule.days.includes(d) ? rule.days.filter((x) => x !== d) : [...rule.days, d])} /> {d}</label>{/each}</Field>
        <Field label="Strategy" help="Sequential upgrades one device at a time"><div class="seg"><button class:on={rule.strategy === 'sequential'} onclick={() => (rule.strategy = 'sequential')}>Sequential</button><button class:on={rule.strategy === 'parallel'} onclick={() => (rule.strategy = 'parallel')}>Parallel</button></div></Field>
        <Field label="If a device fails"><select bind:value={rule.fail}><option value="stop">Stop the rollout</option><option value="continue">Continue</option></select></Field>
        <div class="r"><button class="btn primary" onclick={() => review(`Upgrade rule ${rule.name}`, [`/cmr/upgrade/add name=${q(rule.name)} channel=${rule.channel} strategy=${rule.strategy}`], ruleIntent())}>Review rule</button></div>
      {:else}
        <div class="muted small">Needs a CMR server (the cmr package on RouterOS 7.26+, here or on a controller set in Settings → Control plane).</div>
      {/if}
    </Section>
  {:else if tab === 'supervisor'}
    <Section title="Device Supervisor" desc="Health anomalies across devices: offline, high CPU/memory, failed upgrades. Needs the CMR server; for this router, CPU and memory alerts are in Logs & Alarms." ros="/system/resource/print; /cmr/alert/print">
      <div class="row sup"><span class="dot ok"></span> {router.identity}: online, CPU {live.cpu}%, memory <span class={memPct > 75 ? 'warnc' : ''}>{memPct}%</span>{#if memPct > 75}<span class="pill warn">memory high</span>{/if}</div>
      {#if cmrOk()}
        {#each cmr.data!.devices.filter((d) => d.paired && !d.online) as d}<div class="row sup"><span class="dot bad"></span> {d.identity}: offline</div>{/each}
        {#each cmr.data!.alerts.filter((a) => a.active?.length) as a}<div class="row sup"><span class="pill {a.severity === 'critical' || a.severity === 'high' ? 'bad' : 'warn'}">{a.severity}</span> <b>{a.name}</b>: {a.active!.join(', ')}</div>{/each}
        <h3 class="small">Alert rules (on the CMR server)</h3>
        <table>
          <thead><tr><th>Rule</th><th>Severity</th><th>Devices</th><th>Active</th><th></th></tr></thead>
          <tbody>{#each cmr.data!.alerts as a}<tr>
            <td><b>{a.name}</b>{#if a.category}<div class="muted small">{a.category.join(', ')}</div>{/if}</td><td>{a.severity}</td><td>{a.devices ?? 0}</td><td>{a['devices-on'] ?? 0}</td>
            <td class="r"><button class="btn small" onclick={() => review(`${a.disabled ? 'Enable' : 'Disable'} alert ${a.name}`, [`/cmr/alert/set [find name=${q(a.name)}] disabled=${a.disabled ? 'no' : 'yes'}`], { kind: 'cmr-alert-enabled', name: a.name, enabled: !!a.disabled })}>{a.disabled ? 'Enable' : 'Disable'}</button>
              {#if tik(a.comment)}<button class="btn small" onclick={() => review(`Remove alert ${a.name}`, [`/cmr/alert/remove [find comment="${a.comment}"]`], { kind: 'cmr-remove-alert', name: a.name })}>Remove</button>{/if}</td>
          </tr>{/each}</tbody>
        </table>
        <h3 class="small">New alert</h3>
        <Field label="Name"><input type="text" bind:value={alert.name} /></Field>
        <Field label="When" help="CPU, memory and disk in %, offline in minutes"><select bind:value={alert.metric}><option value="cpu">CPU above</option><option value="memory">Memory above</option><option value="disk">Disk above</option><option value="offline">Offline longer than</option><option value="update">An update is available</option></select>{#if alert.metric !== 'update'}<input type="number" min="1" max="1440" bind:value={alert.threshold} aria-label="Threshold" />{/if}</Field>
        <Field label="Severity"><select bind:value={alert.severity}><option>critical</option><option>high</option><option>medium</option><option>low</option></select></Field>
        <Field label="Devices" help="CMR labels; empty = all"><input type="text" bind:value={alert.labels} placeholder="all" /></Field>
        <div class="r"><button class="btn primary" onclick={() => review(`Alert ${alert.name}`, [`/cmr/alert/add name=${q(alert.name)}`], { kind: 'cmr-alert', name: alert.name, metric: alert.metric, threshold: alert.metric === 'update' ? 0 : alert.threshold, severity: alert.severity, labels: labelList(alert.labels) })}>Review alert</button></div>
      {/if}
    </Section>
    <Section title="Config drift" desc="Compare the configuration with a stored baseline (Blueprints / Drift Inspector).">
      <div class="muted small">Not built yet. tikontrol's reader account has no <code>sensitive</code> policy, and reading <code>/export</code> over the API with only <code>read</code> is not verified yet (a 7.21 pre-release leaked keys in a hide-sensitive export). Until that is tested on CHR, no baseline is taken.</div>
    </Section>
  {/if}
</div>

{#if adoptOpen}
  <Modal title="Find and adopt devices" onclose={() => (adoptOpen = false)}>
    <p class="small">tikontrol looks for MikroTik devices via MNDP/LLDP and CMR pairing requests.</p>
    <ol class="small steps">
      <li>On the new device: <code>/cmr/client set enabled=yes</code> (or plug it in; it finds the CMR server via MNDP).</li>
      <li>It appears here as <b>Pending</b>. Click <b>Adopt</b> to pair it (<code>/cmr/device/pair</code>).</li>
      <li>Wi-Fi, VLANs and upgrades are then pushed by labels.</li>
    </ol>
    {#each pending as d}
      <div class="row between"><span><b>{d.identity}</b> <span class="muted small">{d.board} · {d.address ?? ''} · {d.state ?? 'pending'}</span></span><button class="btn primary small" onclick={() => { adoptOpen = false; adopt = { d, user: 'admin', pass: '' } }}>Adopt</button></div>
    {:else}
      <div class="empty muted small">{cmrOk() ? 'No device is waiting to be adopted.' : `${neighbors.length} found by MNDP/LLDP. Adopting needs a CMR server (the cmr package on RouterOS 7.26+, or a controller in Settings → Control plane).`}</div>
    {/each}
  </Modal>
{/if}

{#if adopt}
  <Modal title="Adopt {adopt.d.identity}" onclose={() => (adopt = null)}>
    <p class="small">The CMR server pairs with the device. By default a device asks for one of its own RouterOS logins: it is passed to the device once and never stored.</p>
    <Field label="Device user"><input type="text" bind:value={adopt.user} autocomplete="off" /></Field>
    <Field label="Device password" help="Leave empty if the device pairs without one"><input type="password" bind:value={adopt.pass} autocomplete="off" /></Field>
    <div class="r"><button class="btn primary" onclick={() => { const a = adopt!; adopt = null; review(`Adopt ${a.d.identity}`, [`/cmr/device/pair [find ids="${a.d.ids}"]`], { kind: 'cmr-pair', device: a.d.ids, username: a.pass ? a.user : '', password: a.pass }) }}>Review</button></div>
  </Modal>
{/if}

{#if guide}
  {@const n = guide.n}
  <Modal title="Set up {n.identity}" onclose={() => (guide = null)}>
    <p class="small"><b>{n.board ?? n.platform ?? 'MikroTik'}</b> · {n.identity} · <span class="mono">{n.address ?? 'no address'}</span> · {n.version ?? 'version unknown'}</p>
    {#if tooOld((n.version ?? '').split(' ')[0])}
      <p class="small" role="alert"><b>RouterOS 7 needed.</b> tikontrol manages RouterOS 7.18 and newer over API-SSL. This device runs {n.version} (RouterOS 6 or SwOS), which tikontrol cannot manage. Install RouterOS 7 on it first if the model supports it (see MikroTik's upgrade guide); otherwise it stays visible here as a neighbour.</p>
    {:else}
      <p class="small">Paste each block into the device's terminal: WinBox → <b>New Terminal</b>, or WebFig at <span class="mono">http://{n.address ?? '<its address>'}/</span> → <b>Terminal</b>. Sign in there with its admin login; tikontrol never sees it.</p>
      <ol class="small steps">
        {#each steps as s}
          <li><b>{s.title}</b><div class="muted">{s.why}</div>
            {#if s.cmds.length}<div class="cmd"><pre class="mono">{s.cmds.join('\n')}</pre><button class="btn small" onclick={() => copy(s.cmds)} aria-label="Copy: {s.title}">Copy</button></div>
            {:else if !s.title.startsWith('Add')}<div class="muted">{isBackend ? 'Loading the commands…' : 'The commands come from the tikontrol server (they include its own address).'}</div>{/if}
            {#if s.add}<div><a class="btn small primary" href="#/settings/control?add={s.add}" onclick={() => (guide = null)}>Add as router</a></div>
            {:else if s.title.startsWith('Add')}<div class="muted">Its address is not a LAN address: type the one tikontrol reaches it on under Settings → Control Plane → Routers.</div>{/if}
          </li>
        {/each}
      </ol>
    {/if}
  </Modal>
{/if}

{#if labelEdit}
  <Modal title="Labels of {labelEdit.d.identity}" onclose={() => (labelEdit = null)}>
    <p class="small">CMR pushes Wi-Fi networks, VLANs and upgrade rules to devices by label. Automatic labels: <span class="mono">{(labelEdit.d['auto-labels'] ?? []).join(', ')}</span></p>
    <Field label="Labels" help="Comma-separated: letters, digits, - _ ."><input type="text" bind:value={labelEdit.text} /></Field>
    <div class="r"><button class="btn primary" onclick={() => { const l = labelEdit!; labelEdit = null; review(`Labels of ${l.d.identity}`, [`/cmr/device/set [find ids="${l.d.ids}"] labels=${labelList(l.text).join(',')}`], { kind: 'cmr-device-labels', device: l.d.ids, labels: labelList(l.text) }) }}>Review</button></div>
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
  h3 { margin: 18px 0 6px; }
  .steps { line-height: 1.8; padding-left: 18px; }
  .steps li + li { margin-top: 10px; }
  .cmd { display: flex; gap: 8px; align-items: flex-start; margin: 4px 0; }
  .cmd pre { flex: 1; margin: 0; padding: 6px 8px; background: var(--bg); border: 1px solid var(--line); border-radius: 6px; overflow-x: auto; line-height: 1.5; white-space: pre; }
  .chk { display: inline-flex; gap: 3px; align-items: center; font-size: 12px; }
  code { font: 11.5px var(--mono); background: var(--panel2); padding: 1px 5px; border-radius: 4px; }
</style>
