<script lang="ts">
  import { raw } from '../../lib/data'
  import { review } from '../../lib/apply.svelte'
  import { prefs } from '../../lib/live.svelte'
  import Icon from '../../lib/Icon.svelte'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'

  const queues: any[] = raw.queues ?? []
  let sp = $state({ name: '', down: 50, up: 10 })
  const slug = (n: string) => n.trim().replace(/[^A-Za-z0-9_-]+/g, '-').slice(0, 32)
  const QOS = {
    voip: { protocol: 'udp', ports: '5060,3478-3481,8801-8810', dscp: 46, priority: 1 },
    gaming: { protocol: 'udp', ports: '3074,27015-27030', dscp: 34, priority: 2 },
  }
  const qosRule = (name: 'voip' | 'gaming') => {
    const q = QOS[name]
    return review(`QoS ${name}`, [
      `/ip/firewall/mangle/add chain=postrouting protocol=${q.protocol} dst-port=${q.ports} action=change-dscp new-dscp=${q.dscp} passthrough=yes comment=tik:qos:${name}`,
      `/ip/firewall/mangle/add chain=postrouting protocol=${q.protocol} dst-port=${q.ports} action=mark-packet new-packet-mark=tik-qos-${name} passthrough=no comment=tik:qos:${name}`,
      `/queue/tree/add name=tik-qos-${name} parent=global packet-mark=tik-qos-${name} priority=${q.priority} comment=tik:qos:${name}`,
    ], { kind: 'qos-rule', name, ...q })
  }
  let port = $state({ name: '', native: 1, tagged: '' })
  let radius = $state({ host: '', secret: '' })
</script>

<Section title="Speed limit profiles" desc="A per-device limit for every member of a client group (PCQ queues on the group's address list). Set members in Clients → Groups." ros="/queue/tree/print; /queue/type/print where kind=pcq">
  {#each queues as q}<div class="mono small">{q.name}: {q['max-limit']}</div>{:else}<div class="muted small">No limits yet.</div>{/each}
  <div class="add"><select bind:value={sp.name} aria-label="Group"><option value="">Group…</option>{#each prefs.groups as g}<option value={slug(g.name)}>{g.name}</option>{/each}</select><input type="number" bind:value={sp.down} title="Down Mbps" aria-label="Download Mbps" style="width:80px" /><input type="number" bind:value={sp.up} title="Up Mbps" aria-label="Upload Mbps" style="width:80px" /><span class="muted small">Mbps ↓/↑ per device</span>
    <button class="btn primary" disabled={!sp.name || !(sp.down > 0 && sp.up > 0)} onclick={() => review(`Speed profile ${sp.name}`, [
      `/queue/type/add name=tik-${sp.name}-down kind=pcq pcq-rate=${sp.down}M pcq-classifier=dst-address`, `/queue/type/add name=tik-${sp.name}-up kind=pcq pcq-rate=${sp.up}M pcq-classifier=src-address`,
      `/ip/firewall/mangle/add chain=forward dst-address-list=tik-${sp.name} action=mark-packet new-packet-mark=tik-${sp.name}-down passthrough=no comment=tik:profile:${sp.name}`,
      `/ip/firewall/mangle/add chain=forward src-address-list=tik-${sp.name} action=mark-packet new-packet-mark=tik-${sp.name}-up passthrough=no comment=tik:profile:${sp.name}`,
      `/queue/tree/add name=tik-${sp.name}-down parent=global packet-mark=tik-${sp.name}-down queue=tik-${sp.name}-down comment=tik:profile:${sp.name}`,
      `/queue/tree/add name=tik-${sp.name}-up parent=global packet-mark=tik-${sp.name}-up queue=tik-${sp.name}-up comment=tik:profile:${sp.name}`,
    ], { kind: 'group-limit', group: sp.name, down: `${sp.down}M`, up: `${sp.up}M` })}><Icon name="plus" size={14} /> Add</button>
    <button class="btn" disabled={!sp.name} onclick={() => review(`Remove speed profile ${sp.name}`, [`/queue/tree/remove [find comment="tik:profile:${sp.name}"]`, `/ip/firewall/mangle/remove [find comment="tik:profile:${sp.name}"]`], { kind: 'remove-group-limit', group: sp.name })}>Remove</button></div>
</Section>

<Section title="Port profiles" desc="Named VLAN templates. Pick one when you edit a port under Devices · Ports: it fills in the native and tagged VLANs." ros="/interface/bridge/vlan/print">
  {#each prefs.portProfiles as pp, i}<div class="row between small"><span><b>{pp.name}</b> · native {pp.native}{pp.tagged.length ? ` · tagged ${pp.tagged.join(', ')}` : ''}</span><button class="btn small ghost" onclick={() => prefs.portProfiles.splice(i, 1)} aria-label={`Delete ${pp.name}`}>Delete</button></div>
  {:else}<div class="muted small">No port profiles yet.</div>{/each}
  <div class="add"><input type="text" bind:value={port.name} placeholder="Name" aria-label="Profile name" /><input type="number" min="1" max="4094" bind:value={port.native} title="Native VLAN" aria-label="Native VLAN" style="width:90px" /><input type="text" bind:value={port.tagged} placeholder="Tagged VLANs: 20,30" aria-label="Tagged VLANs" />
    <button class="btn primary" disabled={!port.name.trim() || !(port.native >= 1 && port.native <= 4094) || prefs.portProfiles.some((x) => x.name === port.name.trim())} onclick={() => { prefs.portProfiles.push({ name: port.name.trim(), native: port.native, tagged: port.tagged.split(',').map((v) => Number(v.trim())).filter((v) => v >= 1 && v <= 4094) }); port = { name: '', native: 1, tagged: '' } }}><Icon name="plus" size={14} /> Add</button></div>
</Section>

<Section title="RADIUS" desc="For WPA-Enterprise Wi-Fi and 802.1X ports." ros="/radius/print">
  <Field label="Server"><input type="text" bind:value={radius.host} placeholder="10.0.0.50" /></Field>
  <Field label="Secret" help="Sent to the router only, never stored or shown"><input type="password" bind:value={radius.secret} autocomplete="new-password" /></Field>
  <div class="r"><button class="btn primary" disabled={!radius.host || !radius.secret} onclick={() => review('RADIUS', [`/radius/add address=${radius.host} secret="********" service=wireless,dot1x comment=tik:radius:${radius.host}`], { kind: 'radius-server', address: radius.host, secret: radius.secret, services: ['wireless', 'dot1x'] })}>Review & apply</button></div>
</Section>

<Section title="QoS" desc="Prioritise latency-sensitive traffic: DSCP marks plus a queue priority. Only matters when the line is full (use Smart Queues).">
  <div class="row between small"><span>Calls (SIP, Teams, Zoom)</span><button class="btn" onclick={() => qosRule('voip')}>Review & apply</button></div>
  <div class="row between small"><span>Gaming</span><button class="btn" onclick={() => qosRule('gaming')}>Review & apply</button></div>
</Section>

<style>
  .add { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; margin-top: 10px; }
  .r { text-align: right; margin-top: 10px; }
  .row { gap: 8px; padding: 4px 0; }
  .small { font-size: 12px; }
</style>
