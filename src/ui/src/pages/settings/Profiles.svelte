<script lang="ts">
  import { raw } from '../../lib/data'
  import { review } from '../../lib/apply.svelte'
  import Icon from '../../lib/Icon.svelte'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'

  const queues: any[] = raw.queues ?? []
  let sp = $state({ name: '', down: 50, up: 10 })
  let port = $state({ name: '', native: 1, tagged: '' })
  let radius = $state({ host: '', secret: '' })
  let qos = $state({ voip: true, gaming: false })
</script>

<Section title="Speed limit profiles" desc="Reusable limits for clients and groups." ros="/queue/simple/print">
  {#each queues as q}<div class="mono small">{q.name}: {q['max-limit']}</div>{:else}<div class="muted small">No limits yet.</div>{/each}
  <div class="add"><input type="text" bind:value={sp.name} placeholder="Name" /><input type="number" bind:value={sp.down} title="Down Mbps" style="width:80px" /><input type="number" bind:value={sp.up} title="Up Mbps" style="width:80px" /><span class="muted small">Mbps ↓/↑</span>
    <button class="btn primary" disabled={!sp.name} onclick={() => review(`Speed profile ${sp.name}`, [`/queue/type/add name=tik-${sp.name} kind=pcq pcq-rate=${sp.down}M pcq-classifier=dst-address`])}><Icon name="plus" size={14} /> Add</button></div>
</Section>

<Section title="Port profiles" desc="Which VLANs a switch port carries." ros="/interface/bridge/vlan/print">
  <div class="add"><input type="text" bind:value={port.name} placeholder="Name" /><input type="number" bind:value={port.native} title="Native VLAN" style="width:90px" /><input type="text" bind:value={port.tagged} placeholder="Tagged VLANs: 20,30" />
    <button class="btn primary" disabled={!port.name} onclick={() => review(`Port profile ${port.name}`, [`# stored in tikontrol; applied per port as /interface/bridge/port/set pvid=${port.native} + /interface/bridge/vlan tagged=`])}><Icon name="plus" size={14} /> Add</button></div>
</Section>

<Section title="RADIUS" desc="For WPA-Enterprise Wi-Fi and 802.1X ports." ros="/radius/print">
  <Field label="Server"><input type="text" bind:value={radius.host} placeholder="10.0.0.50" /></Field>
  <Field label="Secret"><input type="password" bind:value={radius.secret} /></Field>
  <div class="r"><button class="btn primary" disabled={!radius.host} onclick={() => review('RADIUS', [`/radius/add address=${radius.host} secret="••••" service=wireless,dot1x`])}>Review & apply</button></div>
</Section>

<Section title="QoS" desc="Prioritise latency-sensitive traffic.">
  <label class="row small"><input type="checkbox" bind:checked={qos.voip} /> Calls (SIP, Teams, Zoom)</label>
  <label class="row small"><input type="checkbox" bind:checked={qos.gaming} /> Gaming</label>
  <div class="r"><button class="btn primary" onclick={() => review('QoS', [
    ...(qos.voip ? ['/ip/firewall/mangle/add chain=postrouting protocol=udp dst-port=5060,3478-3481,8801-8810 action=set-priority new-priority=6 comment="tik:qos voip"'] : []),
    ...(qos.gaming ? ['/ip/firewall/mangle/add chain=postrouting protocol=udp dst-port=3074,27015-27030 action=set-priority new-priority=5 comment="tik:qos gaming"'] : []),
  ])}>Review & apply</button></div>
</Section>

<style>
  .add { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; margin-top: 10px; }
  .r { text-align: right; margin-top: 10px; }
  .row { gap: 8px; padding: 4px 0; }
  .small { font-size: 12px; }
</style>
