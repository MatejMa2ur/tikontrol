<script lang="ts">
  import { ports, clients, raw, bytes, router } from '../lib/data'
  import { displayName } from '../lib/live.svelte'
  import { review } from '../lib/apply.svelte'
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
  let f = $state({ enabled: true, profile: 'all', pvid: 1, tagged: '', speed: 'auto', poe: false, mirror: false, lag: false, stp: true, storm: false, dot1x: false, lldp: true })
  function open(p: string) {
    const bp = bports.find((b) => b.interface === p)
    f = { enabled: true, profile: bp ? (bp['frame-types'] === 'admit-only-vlan-tagged' ? 'trunk' : 'all') : 'wan', pvid: bp?.pvid ?? 1, tagged: '', speed: 'auto', poe: false, mirror: false, lag: false, stp: true, storm: false, dot1x: false, lldp: true }
    edit = p
  }
  function save() {
    const p = edit!
    const cmds = [`/interface/ethernet/set ${p} disabled=${!f.enabled}${f.speed !== 'auto' ? ` auto-negotiation=no speed=${f.speed}` : ''}`]
    if (f.profile === 'access') cmds.push(`/interface/bridge/port/set [find interface=${p}] pvid=${f.pvid} frame-types=admit-only-untagged-and-priority-tagged`)
    if (f.profile === 'trunk') cmds.push(`/interface/bridge/port/set [find interface=${p}] frame-types=admit-only-vlan-tagged`, `/interface/bridge/vlan/add bridge=bridge vlan-ids=${f.tagged || '10,20'} tagged=bridge,${p}`)
    if (f.mirror) cmds.push(`/interface/ethernet/switch/set switch1 mirror-source=${p} mirror-target=ether5`)
    if (f.dot1x) cmds.push(`/interface/dot1x/server/add interface=${p}`)
    if (!f.lldp) cmds.push(`/ip/neighbor/discovery-settings/set discover-interface-list=!dynamic`)
    review(`Port ${p}`, cmds)
    edit = null
  }
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
      <Field label="PoE output" na="Not available on {router.board} (PoE-in only)"><Toggle bind:checked={f.poe} disabled label="PoE" /></Field>
      <Field label="Port mirroring" help="Copy this port's traffic to ether5"><Toggle bind:checked={f.mirror} label="Mirroring" /></Field>
      <Field label="Link aggregation" help="Bond with another port (LACP)"><Toggle bind:checked={f.lag} label="LAG" /></Field>
      <Field label="Loop protection (STP)"><Toggle bind:checked={f.stp} label="STP" /></Field>
      <Field label="Storm control" na="Switch-chip dependent; not exposed on this model"><Toggle bind:checked={f.storm} disabled label="Storm control" /></Field>
      <Field label="802.1X authentication"><Toggle bind:checked={f.dot1x} label="802.1X" /></Field>
      <Field label="LLDP / neighbour discovery"><Toggle bind:checked={f.lldp} label="LLDP" /></Field>
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
  summary { cursor: pointer; color: var(--muted); padding: 6px 0; }
</style>
