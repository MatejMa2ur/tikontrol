<script lang="ts">
  import { ports, clients, raw, bytes, router } from '../lib/data'
  import { displayName } from '../lib/live.svelte'
  import { review, type Intent } from '../lib/apply.svelte'
  import Icon from '../lib/Icon.svelte'
  import Ros from '../lib/Ros.svelte'
  import Modal from '../lib/ui/Modal.svelte'
  import Field from '../lib/ui/Field.svelte'
  import Toggle from '../lib/ui/Toggle.svelte'

  let { compact = false }: { compact?: boolean } = $props()
  const mon: any[] = raw.etherMon ?? []
  const stats: any[] = raw.etherStats ?? []
  const bports: any[] = raw.bridgePorts ?? []
  const bridge = (raw.bridges ?? [])[0] ?? {}
  const name = (mac: string) => { const c = clients.find((x) => x.mac === mac); return c ? displayName(c) : mac }
  const m = (p: string) => mon.find((x) => x.name === p) ?? {}
  const st = (p: string) => stats.find((x) => x.name === p) ?? {}

  // port editor
  let edit = $state<string | null>(null)
  let f = $state({ enabled: true, profile: 'all', pvid: 1, tagged: '', speed: 'auto', poe: false })
  function open(p: string) {
    const bp = bports.find((b) => b.interface === p)
    f = { enabled: true, profile: bp ? (bp['frame-types'] === 'admit-only-vlan-tagged' ? 'trunk' : 'all') : 'wan', pvid: bp?.pvid ?? 1, tagged: '', speed: 'auto', poe: false }
    const others = ports.map((x) => x.name).filter((n) => n !== p)
    a = { mirrorTo: sw['mirror-target'] && sw['mirror-target'] !== 'none' ? sw['mirror-target'] : others.at(-1) ?? '', bondWith: others[0] ?? '', bondMode: '802.3ad', edge: bp?.edge ?? 'auto', bpduGuard: !!bp?.['bpdu-guard'], storm: swp(p)?.['storm-rate'] ?? 100 }
    edit = p
  }
  function save() {
    const p = edit!
    const cmds = [`/interface/ethernet/set ${p} disabled=${!f.enabled}${f.speed !== 'auto' ? ` auto-negotiation=no speed=${f.speed}` : ''}`]
    if (f.profile === 'access') cmds.push(`/interface/bridge/port/set [find interface=${p}] pvid=${f.pvid} frame-types=admit-only-untagged-and-priority-tagged`)
    if (f.profile === 'trunk') cmds.push(`/interface/bridge/port/set [find interface=${p}] frame-types=admit-only-vlan-tagged`, `/interface/bridge/vlan/add bridge=bridge vlan-ids=${f.tagged || '10,20'} tagged=bridge,${p}`)
    // one change the server can compile → send it as an intent; mixed changes stay commands to copy
    const bp = bports.find((b) => b.interface === p)
    const extras = f.speed !== 'auto'
    const enabledChanged = f.enabled === false
    let intent: Intent | undefined
    if (!extras && enabledChanged && f.profile === 'all') intent = { kind: 'set-port-enabled', interface: p, enabled: false }
    else if (!extras && !enabledChanged && f.profile === 'access' && bp) intent = { kind: 'set-bridge-port', interface: p, pvid: f.pvid, frame_types: 'admit-only-untagged-and-priority-tagged' }
    else if (!extras && !enabledChanged && f.profile === 'trunk' && bp) intent = { kind: 'port-profile', interface: p, pvid: 1, tagged: (f.tagged || '').split(',').map((v) => Number(v.trim())).filter((v) => v > 0) }
    review(`Port ${p}`, cmds, intent)
    edit = null
  }

  // advanced switching: each control is its own Safe apply intent, shown only when the router reports the menu
  const list = (v: unknown): any[] => (Array.isArray(v) ? v : [])
  const sw: any = list(raw.switches)[0] ?? {}
  const mirrorOk = 'mirror-source' in sw
  const mirroring = mirrorOk && sw['mirror-source'] && sw['mirror-source'] !== 'none'
  const swp = (p: string) => list(raw.switchPorts).find((x) => x.name === p && 'storm-rate' in x)
  const bonds = list(raw.bonds)
  const dot1x = list(raw.dot1x)
  const radiusDot1x = list(raw.radius).some((r) => list(r.service).includes('dot1x') && !r.disabled)
  const disc: any = raw.discovery && typeof raw.discovery === 'object' ? raw.discovery : null
  const UPLINK = 'High risk if this is the port tikontrol or your browser reaches the router through: a mistake cuts you off until the change reverts (5 min).'
  let a = $state({ mirrorTo: '', bondWith: '', bondMode: '802.3ad', edge: 'auto', bpduGuard: false, storm: 100 })
  const go = (title: string, cmd: string, intent: Intent) => { review(title, cmd, intent); edit = null }
</script>

<div class="grid">
  {#if !compact}
    <section class="card face">
      {#each ports as p, i}
        <button class="port" class:up={p.up} title="{p.name}: {p.up ? m(p.name).rate : 'no link'}" onclick={() => open(p.name)}>
          <span class="jack"><span></span></span>
          <span class="small">{i + 1}{p.wan ? ' · WAN' : ''}</span>
          <span class="tiny muted">{p.up ? m(p.name).rate : ''}</span>
        </button>
      {/each}
    </section>
  {/if}

  <section class="card flush">
    <table>
      <thead><tr><th>Port</th><th>Status</th><th>Speed</th>{#if !compact}<th>Role</th><th>Profile</th><th class="r">Traffic ↓ / ↑</th><th class="r">Errors</th>{/if}<th>Connected</th></tr></thead>
      <tbody>
        {#each ports as p}
          {@const bp = bports.find((b) => b.interface === p.name)}
          <tr onclick={() => open(p.name)}>
            <td class="row"><Icon name="ports" size={14} /> <b>{p.name}</b></td>
            <td><span class="pill {p.up ? 'ok' : ''}">{p.up ? 'Link up' : 'No link'}</span></td>
            <td>{p.up ? `${m(p.name).rate}${m(p.name)['full-duplex'] ? ' FD' : ' HD'}` : '—'}</td>
            {#if !compact}
              <td class="muted">{p.wan ? 'Internet (WAN)' : 'LAN (bridge)'}</td>
              <td class="muted">{bp ? `All · native VLAN ${bp.pvid}` : 'WAN'}</td>
              <td class="r mono">{bytes(st(p.name)['tx-byte'] ?? 0)} / {bytes(st(p.name)['rx-byte'] ?? 0)}</td>
              <td class="r {(st(p.name)['rx-error'] ?? 0) + (st(p.name)['tx-error'] ?? 0) ? 'bad' : 'muted'}">{(st(p.name)['rx-error'] ?? 0) + (st(p.name)['tx-error'] ?? 0)}</td>
            {/if}
            <td>
              {#if p.clients.length}{#each p.clients as mac}<a class="chip" href="#/clients/{mac}" onclick={(e) => e.stopPropagation()}>{name(mac)}</a>{/each}
              {:else if p.wan}<span class="muted">Upstream router</span>
              {:else}<span class="muted">—</span>{/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>
  {#if !compact}
    <section class="card small muted">Bridge <b>{bridge.name}</b> · STP mode <b>{bridge['protocol-mode']}</b> · VLAN filtering <b>{bridge['vlan-filtering'] ? 'on' : 'off'}</b> · IGMP snooping <b>{bridge['igmp-snooping'] ? 'on' : 'off'}</b> · PoE out: <b>not available on this model</b></section>
    <Ros cmd="/interface/ethernet/monitor [find] once; /interface/print stats where type=ether; /interface/bridge/port/print; /interface/bridge/host/print where !local" />
  {/if}
</div>

{#if edit}
  <Modal title="Port {edit}" onclose={() => (edit = null)} wide>
    <Field label="Enabled"><Toggle bind:checked={f.enabled} label="Enabled" /></Field>
    <Field label="Port profile" help="Access = one untagged VLAN · Trunk = tagged VLANs">
      <div class="seg">{#each [['all', 'Allow all'], ['access', 'Access'], ['trunk', 'Trunk']] as [v, l]}<button class:on={f.profile === v} onclick={() => (f.profile = v)}>{l}</button>{/each}</div>
    </Field>
    {#if f.profile === 'access'}<Field label="VLAN ID"><input type="number" bind:value={f.pvid} min="1" max="4094" /></Field>{/if}
    {#if f.profile === 'trunk'}<Field label="Tagged VLANs" help="Comma separated"><input type="text" bind:value={f.tagged} placeholder="10,20,30" /></Field>{/if}
    <Field label="Link speed"><select bind:value={f.speed}><option value="auto">Auto</option><option>1G-baseT-full</option><option>100M-baseT-full</option><option>10M-baseT-full</option></select></Field>
    <details><summary class="small">Advanced</summary>
      <p class="tiny warn">{UPLINK}</p>
      <Field label="PoE output" na="Not available on {router.board} (PoE-in only)"><Toggle bind:checked={f.poe} disabled label="PoE" /></Field>
      {#if mirrorOk}
        <Field label="Port mirroring" help={mirroring ? `Now: ${sw['mirror-source']} → ${sw['mirror-target']}` : `Copy ${edit}'s traffic to a capture port (switch chip ${sw.type ?? ''})`}>
          <select bind:value={a.mirrorTo} aria-label="Mirror target">{#each ports.filter((x) => x.name !== edit) as o}<option>{o.name}</option>{/each}</select>
          <button class="btn" onclick={() => go(`Mirror ${edit}`, `/interface/ethernet/switch/set ${sw.name} mirror-source=${edit} mirror-target=${a.mirrorTo}`, { kind: 'set-port-mirror', source: edit, target: a.mirrorTo })}>Review</button>
          {#if mirroring}<button class="btn" onclick={() => go('Stop mirroring', `/interface/ethernet/switch/set ${sw.name} mirror-source=none mirror-target=none`, { kind: 'remove-port-mirror' })}>Stop</button>{/if}
        </Field>
      {:else}<Field label="Port mirroring" na="This router's switch chip reports no mirroring. /tool/sniffer can capture on the CPU instead."><span></span></Field>{/if}
      <Field label="Link aggregation" help={bonds.length ? `Bonds: ${bonds.map((b) => `${b.name} (${list(b.slaves).join('+')})`).join(', ')}` : 'Bond with another port; the other end must match (LACP)'}>
        <select bind:value={a.bondWith} aria-label="Bond with">{#each ports.filter((x) => x.name !== edit && !x.wan) as o}<option>{o.name}</option>{/each}</select>
        <select bind:value={a.bondMode} aria-label="Bond mode"><option value="802.3ad">LACP</option><option>active-backup</option><option>balance-xor</option></select>
        <button class="btn" onclick={() => go(`Bond ${edit}`, `/interface/bonding/add name=bond1 slaves=${edit},${a.bondWith} mode=${a.bondMode}`, { kind: 'create-bond', name: 'bond1', slaves: [edit, a.bondWith], mode: a.bondMode })}>Review</button>
        {#each bonds.filter((b) => String(b.comment ?? '').startsWith('tik:bond:')) as b}<button class="btn" onclick={() => go(`Remove ${b.name}`, `/interface/bonding/remove ${b.name}`, { kind: 'remove-bond', name: b.name })}>Remove {b.name}</button>{/each}
      </Field>
      {#if bports.some((b) => b.interface === edit)}
        <Field label="Loop protection (STP)" help="Bridge mode {bridge['protocol-mode'] ?? '?'} · edge = end-device port · BPDU guard shuts the port if a switch appears">
          <select bind:value={a.edge} aria-label="Edge"><option>auto</option><option>yes</option><option>no</option></select>
          <Toggle bind:checked={a.bpduGuard} label="BPDU guard" />
          <button class="btn" onclick={() => go(`STP on ${edit}`, `/interface/bridge/port/set [find interface=${edit}] edge=${a.edge} bpdu-guard=${a.bpduGuard ? 'yes' : 'no'}`, { kind: 'set-port-stp', interface: edit, edge: a.edge, bpdu_guard: a.bpduGuard })}>Review</button>
          {#if bridge['protocol-mode'] !== 'rstp'}<button class="btn" onclick={() => go('Bridge RSTP', `/interface/bridge/set ${bridge.name} protocol-mode=rstp`, { kind: 'set-bridge-stp', bridge: bridge.name, protocol_mode: 'rstp' })}>Use RSTP</button>{/if}
        </Field>
      {/if}
      {#if edit && swp(edit)}
        <Field label="Storm control" help="Max broadcast / unknown-unicast, % of link speed">
          <input type="number" min="0" max="100" bind:value={a.storm} aria-label="Storm rate" />
          <button class="btn" onclick={() => go(`Storm control on ${edit}`, `/interface/ethernet/switch/port/set ${edit} storm-rate=${a.storm}`, { kind: 'set-storm-rate', interface: edit, percent: a.storm })}>Review</button>
        </Field>
      {:else}<Field label="Storm control" na="This router's switch chip reports no storm-rate (CRS3xx switches have it). STP still stops loops."><span></span></Field>{/if}
      {#if dot1x.some((d) => d.interface === edit)}
        <Field label="802.1X authentication" help="On: devices log in through RADIUS">
          {#if dot1x.some((d) => d.interface === edit && String(d.comment ?? '').startsWith('tik:dot1x:'))}<button class="btn" onclick={() => go(`Stop 802.1X on ${edit}`, `/interface/dot1x/server/remove [find interface=${edit}]`, { kind: 'remove-dot1x-port', interface: edit })}>Turn off</button>{:else}<span class="muted small">Set up outside tikontrol</span>{/if}
        </Field>
      {:else if radiusDot1x}
        <Field label="802.1X authentication" help="Devices get no network until they log in through RADIUS">
          <button class="btn" onclick={() => go(`802.1X on ${edit}`, `/interface/dot1x/server/add interface=${edit} auth-types=dot1x`, { kind: 'add-dot1x-port', interface: edit })}>Review</button>
        </Field>
      {:else}<Field label="802.1X authentication" na="Needs a RADIUS server with service dot1x under /radius first (tikontrol never handles RADIUS secrets)."><span></span></Field>{/if}
      {#if disc}
        <Field label="LLDP / neighbour discovery" help="Router-wide: discovery on {disc['discover-interface-list'] ?? '?'}{disc.protocol ? ` (${list(disc.protocol).join(', ')})` : ''}">
          <button class="btn" onclick={() => go('Discovery on LAN only', '/ip/neighbor/discovery-settings/set discover-interface-list=LAN', { kind: 'set-discovery', interface_list: 'LAN' })}>LAN only</button>
          {#if 'protocol' in disc}<button class="btn" onclick={() => go('LLDP only', '/ip/neighbor/discovery-settings/set discover-interface-list=LAN protocol=lldp', { kind: 'set-discovery', interface_list: 'LAN', protocols: ['lldp'] })}>LLDP only</button>{/if}
        </Field>
      {/if}
    </details>
    {#snippet footer()}<button class="btn" onclick={() => (edit = null)}>Cancel</button><button class="btn primary" onclick={save}>Review & apply</button>{/snippet}
  </Modal>
{/if}

<style>
  .face { display: flex; gap: 10px; justify-content: center; padding: 22px; background: linear-gradient(var(--panel2), var(--panel)); }
  .port { display: grid; justify-items: center; gap: 4px; color: var(--muted); background: none; border: 0; cursor: pointer; }
  .jack { width: 46px; height: 38px; border-radius: 5px; background: var(--code-bg); border: 2px solid var(--line); display: grid; place-items: end center; padding-bottom: 5px; }
  .jack span { width: 22px; height: 8px; border-radius: 2px; background: var(--line); }
  .port.up .jack { border-color: var(--ok); }
  .port.up .jack span { background: var(--ok); box-shadow: 0 0 8px var(--ok); }
  .port:first-child .jack { border-style: dashed; }
  .tiny { font-size: 10.5px; }
  .flush { padding: 4px 6px; overflow-x: auto; }
  .r { text-align: right; }
  .bad { color: var(--bad); }
  .chip { display: inline-block; padding: 2px 8px; margin-right: 6px; border-radius: 6px; background: var(--panel2); color: var(--text); font-size: 12px; }
  details { margin-top: 8px; }
  .warn { color: var(--warn); }
  summary { cursor: pointer; color: var(--muted); padding: 6px 0; }
</style>
