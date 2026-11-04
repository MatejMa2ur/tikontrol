<script lang="ts">
  import { raw, router } from '../../lib/data'
  import { review } from '../../lib/apply.svelte'
  import Icon from '../../lib/Icon.svelte'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'

  const users: any[] = raw.users ?? []
  const groups: any[] = raw.userGroups ?? []
  const backups: any[] = raw.backups ?? []
  let u = $state({ name: '', group: 'read' })
  let cmd = $state('')
  let keys = $state<{ name: string; key: string }[]>([])
  const newKey = () => (keys = [...keys, { name: `key-${keys.length + 1}`, key: Array.from(crypto.getRandomValues(new Uint8Array(24)), (b) => b.toString(16).padStart(2, '0')).join('') }])
</script>

<Section title="Admins" desc="Who can log in to the router." ros="/user/print">
  <table><thead><tr><th>Name</th><th>Role</th><th>Last login</th><th></th></tr></thead>
    <tbody>{#each users as x}<tr class="nohover"><td><b>{x.name}</b> <span class="muted small">{x.comment ?? ''}</span></td><td><span class="pill">{x.group}</span></td><td>{x['last-logged-in'] ?? '—'}</td><td class="r">{#if x.name === 'admin'}<span class="muted small">Rename the default admin</span>{/if}</td></tr>{/each}</tbody></table>
  <div class="add"><input type="text" bind:value={u.name} placeholder="Username" /><select bind:value={u.group}>{#each groups as g}<option>{g.name}</option>{/each}</select>
    <button class="btn primary" disabled={!u.name} onclick={() => review(`Admin ${u.name}`, [`/user/add name=${u.name} group=${u.group} password="••••" comment="tik:admin"`])}><Icon name="plus" size={14} /> Invite</button></div>
  <Field label="Single sign-on" na="RouterOS has no SSO; use RADIUS for central logins"><span></span></Field>
</Section>

<Section title="Backups" desc="Binary backup plus a readable export, kept on the router and in tikontrol." ros="/file/print where type=backup">
  {#snippet actions()}<button class="btn primary" onclick={() => review('Back up now', [`/system/backup/save name=tik-${new Date().toISOString().slice(0, 10)} dont-encrypt=no`, `/export show-sensitive=no file=tik-${new Date().toISOString().slice(0, 10)}`])}>Back up now</button>{/snippet}
  <table><thead><tr><th>File</th><th>Date</th><th class="r">Size</th><th></th></tr></thead>
    <tbody>{#each backups as b}<tr class="nohover"><td class="mono small">{b.name}</td><td>{b['last-modified']}</td><td class="r">{(b.size / 1024).toFixed(1)} KB</td><td class="r"><button class="btn small" onclick={() => review(`Restore ${b.name}`, [`/system/backup/load name=${b.name}`])}>Restore</button></td></tr>{/each}</tbody></table>
  <div class="row muted small sched">Automatic: <button class="btn small" onclick={() => review('Weekly backups', ['/system/scheduler/add name=tik-backup interval=7d on-event="/system/backup/save name=tik-auto"'])}>Weekly backups</button></div>
</Section>

<Section title="Updates" desc="RouterOS version and channel." ros="/system/package/update/print">
  <Field label="Version"><span>{router.version}</span></Field>
  <a class="btn" href="#/devices/updates">Manage updates…</a>
</Section>

<Section title="API keys" desc="For Home Assistant and scripts. Keys belong to tikontrol, not the router.">
  {#snippet actions()}<button class="btn" onclick={newKey}><Icon name="key" size={14} /> New key</button>{/snippet}
  {#each keys as k}<div class="row small"><b>{k.name}</b><span class="mono">{k.key}</span></div>{:else}<div class="muted small">No API keys.</div>{/each}
</Section>

<Section title="Console" desc="Run a RouterOS command. Goes through Safe apply.">
  <div class="add"><span class="mono muted">[admin@{router.identity}] &gt;</span><input type="text" class="mono" bind:value={cmd} placeholder="/ip/address/print" style="flex:1" onkeydown={(e) => e.key === 'Enter' && cmd && review('Console', [cmd])} />
    <button class="btn primary" disabled={!cmd} onclick={() => review('Console', [cmd])}><Icon name="terminal" size={14} /> Run</button></div>
</Section>

<style>
  .add { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; margin-top: 10px; }
  .sched { gap: 8px; margin-top: 8px; }
  .row { gap: 10px; padding: 3px 0; }
  .r { text-align: right; }
  .small { font-size: 12px; }
  .btn.small { padding: 3px 9px; }
</style>
