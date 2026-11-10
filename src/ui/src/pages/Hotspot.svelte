<script lang="ts">
  import { raw, ssids, parseUptime } from '../lib/data'
  import { review } from '../lib/apply.svelte'
  import Icon from '../lib/Icon.svelte'
  import Tabs from '../lib/ui/Tabs.svelte'
  import Section from '../lib/ui/Section.svelte'
  import Field from '../lib/ui/Field.svelte'
  import Toggle from '../lib/ui/Toggle.svelte'

  let { tab = 'portal' }: { tab?: string } = $props()
  const list = (v: unknown): any[] => (Array.isArray(v) ? v : [])
  const hotspots = list(raw.hotspot)
  const guests = list(raw.hotspotActive)
  const mb = (b: unknown) => `${((Number(b) || 0) / 1e6).toFixed(1)} MB`

  // the router's current guest hotspot (set up by tikontrol as tik-guest); the defaults below only when there is none
  const ours = hotspots.find((h) => h.name === 'tik-guest')
  const others = hotspots.filter((h) => h.name !== 'tik-guest')
  const hprof = list(raw.hotspotProfiles).find((p) => p.name === 'tik-guest')
  const uprof = list(raw.hotspotUserProfiles).find((p) => p.name === 'tik-guest')
  const addr = list(raw.addresses).find((a) => a.comment === 'tik:hotspot')
  const branded = hprof?.['html-directory'] === 'tik-guest-html'
  // rate-limit is rx/tx as the router sees it: guest upload / guest download
  const [rUp, rDown] = String(uprof?.['rate-limit'] ?? '').split('/')
  const loginBy = list(hprof?.['login-by'])
  let portal = $state({
    on: !!ours,
    iface: String(ours?.interface ?? 'guest-vlan'),
    address: String(addr?.address ?? '10.10.10.1/24'),
    ssid: 'Guest', title: 'Welcome', text: 'Free Wi-Fi for our guests.', color: '#2672f5',
    auth: hprof && !loginBy.includes('trial') ? 'voucher' : 'terms',
    duration: String(uprof?.['session-timeout'] || hprof?.['trial-uptime-limit'] || '1d'),
    limit: rUp && rDown ? `${rDown}/${rUp}` : ours ? '' : '20M/5M',
  })
  const durations = [...new Set(['1h', '8h', '1d', '7d', portal.duration])]

  // new vouchers are generated in this browser (crypto.getRandomValues) and shown once for printing, then created as
  // /ip/hotspot/user entries; nothing is kept in the browser. Existing ones are read back from the router (never a password).
  type V = { code: string; duration: string; uses: number; created: string }
  let vouchers = $state<V[]>([])
  let gen = $state({ count: 10, duration: '1d', uses: 1 })
  const code = () => Array.from(crypto.getRandomValues(new Uint8Array(8)), (b) => 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789'[b % 32]).join('')
  function generate() {
    const created = new Date().toLocaleString()
    vouchers = [...Array.from({ length: gen.count }, () => ({ code: code(), duration: gen.duration, uses: gen.uses, created })), ...vouchers]
  }
  const saved = (() => {
    const by = new Map<string, any[]>()
    for (const u of list(raw.hotspotUsers)) {
      const c = String(u.comment ?? '')
      if (c.startsWith('tik:voucher:')) by.set(c.slice(12), [...(by.get(c.slice(12)) ?? []), u])
    }
    return [...by].map(([batch, users]) => ({ batch, users: users.map((u) => ({ code: String(u.name), limit: String(u['limit-uptime'] ?? ''), used: (parseUptime(u.uptime) ?? 0) > 0, disabled: !!u.disabled })) }))
  })()
  const savedCount = saved.reduce((n, b) => n + b.users.length, 0)
  const [down, up] = $derived(portal.limit.split('/').map((x) => x.trim()))
  const applyPortal = () => review('Guest portal', [
    `/ip/address/add address=${portal.address} interface=${portal.iface} comment=tik:hotspot`,
    '/ip/pool/add name=tik-guest ranges=… comment=tik:hotspot',
    '/ip/dhcp-server/network/add … comment=tik:hotspot',
    `/ip/dhcp-server/add name=tik-guest interface=${portal.iface} address-pool=tik-guest comment=tik:hotspot`,
    `/ip/hotspot/user/profile/add name=tik-guest session-timeout=${portal.duration}${portal.limit ? ` rate-limit=${up}/${down}` : ''}`,
    `/ip/hotspot/profile/add name=tik-guest login-by=${portal.auth === 'voucher' ? 'http-chap,http-pap' : 'trial'}`,
    `/ip/hotspot/add name=tik-guest interface=${portal.iface} address-pool=tik-guest profile=tik-guest`,
  ], { kind: 'guest-hotspot', interface: portal.iface, address: portal.address, down: down ?? '', up: up ?? '', session: portal.duration, login: portal.auth === 'voucher' ? 'voucher' : 'terms' })
  const removePortal = () => review('Remove guest portal', [
    '/ip/hotspot/remove [find name="tik-guest"]', '/ip/hotspot/profile/remove [find name="tik-guest"]', '/ip/hotspot/user/profile/remove [find name="tik-guest"]',
    '/ip/dhcp-server/remove [find name="tik-guest"]', '/ip/dhcp-server/network/remove [find comment="tik:hotspot"]', '/ip/pool/remove [find name="tik-guest"]', '/ip/address/remove [find comment="tik:hotspot"]',
  ], { kind: 'remove-guest-hotspot' })
  const colourOk = $derived(/^#[0-9a-fA-F]{6}$/.test(portal.color))
  const uploadPage = () => review('Branded login page', ['/file/add name=tik-guest-html type=directory', '/file/add name=tik-guest-html/login.html contents=…', '/ip/hotspot/profile/set [find name="tik-guest"] html-directory=tik-guest-html'], { kind: 'hotspot-page', title: portal.title, message: portal.text, color: portal.color })
  const defaultPage = () => review("Use RouterOS's default login page", ['/ip/hotspot/profile/set [find name="tik-guest"] html-directory=hotspot', '/file/remove [find name="tik-guest-html"]'], { kind: 'remove-hotspot-page' })
  const batch = (vs: V[]) => vs.filter((v) => v.created === vs[0].created)
</script>

<div class="grid">
  <h1>Hotspot</h1>
  <Tabs base="hotspot" active={tab} tabs={[{ id: 'portal', label: 'Guest portal' }, { id: 'vouchers', label: 'Vouchers', count: vouchers.length + savedCount }, { id: 'guests', label: 'Guests', count: guests.length }, { id: 'payments', label: 'Payments' }]} />

  {#if tab === 'portal'}
    <div class="two">
      <Section title="Captive portal" desc="A guest network with a login page: RouterOS Hotspot, optionally with a branded page served from the router." ros="/ip/hotspot/print; /ip/hotspot/profile/print; /ip/hotspot/user/profile/print; /ip/address/print where comment=tik:hotspot">
        <Field label="Enabled" help={ours ? 'Turn off and review to remove the guest portal' : ''}><Toggle bind:checked={portal.on} label="Guest portal" /></Field>
        {#if others.length}<div class="muted small">This router has other hotspots ({others.map((h) => h.name).join(', ')}); tikontrol leaves them alone.</div>{/if}
        <Field label="Guest interface" help="Only guests should use it: a guest VLAN or guest Wi-Fi, never your main LAN"><input type="text" bind:value={portal.iface} disabled={!!ours} /></Field>
        <Field label="Guest network" help="The router's address on it"><input type="text" bind:value={portal.address} disabled={!!ours} /></Field>
        <Field label="Guest SSID" help="Preview only"><input type="text" bind:value={portal.ssid} /></Field>
        <Field label="Authentication">
          <select bind:value={portal.auth} disabled={!!ours}><option value="terms">Accept terms</option><option value="voucher">Voucher</option></select>
        </Field>
        <Field label="Session length"><select bind:value={portal.duration} disabled={!!ours}>{#each durations as d}<option>{d}</option>{/each}</select></Field>
        <Field label="Speed limit per guest" help="Download/upload, e.g. 20M/5M; empty for none"><input type="text" bind:value={portal.limit} disabled={!!ours} /></Field>
        {#if ours}<div class="muted small">These are the router's current settings. To change them, turn the portal off, apply, then set it up again.</div>{/if}
        <div class="r">
          {#if ours && !portal.on}<button class="btn primary" onclick={removePortal}>Review removal</button>
          {:else if !ours}<button class="btn primary" disabled={!portal.on} onclick={applyPortal}>Review & apply</button>{/if}
        </div>
        <h3 class="sub">Branded login page</h3>
        <Field label="Title"><input type="text" bind:value={portal.title} maxlength="60" /></Field>
        <Field label="Message"><input type="text" bind:value={portal.text} maxlength="240" /></Field>
        <Field label="Brand colour"><input type="color" bind:value={portal.color} /></Field>
        <div class="muted small">{#if !ours}Set up the guest portal first; then the page can be written to the router.{:else if branded}The router serves a branded page. Its title, message and colour aren't read back: uploading again replaces it.{:else}The router serves its default login page.{/if}</div>
        <div class="r">
          {#if branded}<button class="btn" onclick={defaultPage}>Use default page</button>{/if}
          <button class="btn primary" disabled={!ours || !portal.on || !colourOk || !portal.title.trim()} onclick={uploadPage}>{branded ? 'Replace page' : 'Upload page'}</button>
        </div>
      </Section>
      <div class="preview">
        <div class="phone">
          <div class="screen" style="--brand:{portal.color}">
            <div class="logo"><Icon name="wifi" size={22} /></div>
            <h2>{portal.title}</h2>
            <p>{portal.text}</p>
            {#if portal.auth === 'voucher'}<input type="text" placeholder="Voucher code" disabled />{/if}
            {#if portal.auth === 'terms'}<label class="terms"><input type="checkbox" checked disabled /> I accept the terms of use</label>{/if}
            <button class="go">Connect</button>
          </div>
        </div>
        <div class="muted small c">Preview · {portal.ssid}</div>
      </div>
    </div>
  {:else if tab === 'vouchers'}
    <Section title="Vouchers" desc="Codes for guests: each code is a hotspot login. New codes are generated in this browser and shown once for printing; the router keeps them." ros="/ip/hotspot/user/print proplist=name,profile,limit-uptime,uptime,comment,disabled where comment~&quot;^tik:voucher:&quot;">
      {#snippet actions()}
        <input type="number" bind:value={gen.count} min="1" max="100" style="width:70px" aria-label="Number of vouchers" />
        <select bind:value={gen.duration} aria-label="Time per voucher"><option>1h</option><option>1d</option><option>7d</option><option>30d</option></select>
        <button class="btn primary" onclick={generate}><Icon name="plus" size={14} /> Create</button>
        <button class="btn" disabled={!vouchers.length} onclick={() => print()}>Print</button>
      {/snippet}
      {#if vouchers.length}{@const b = batch(vouchers)}
        <div class="vgrid">
          {#each vouchers as v}
            <div class="voucher"><span class="muted small">Wi-Fi voucher · {v.duration}</span><b class="mono">{v.code}</b><span class="muted tiny">{ssids[0]?.ssid ?? ''} guest</span></div>
          {/each}
        </div>
        <div class="r"><span class="muted small">Not on the router yet; these codes are gone when you leave this page. </span><button class="btn primary" onclick={() => review(`Create ${b.length} vouchers`, b.map((v) => `/ip/hotspot/user/add name=${v.code} limit-uptime=${v.duration} comment=tik:voucher:${b[0].code}`), { kind: 'hotspot-vouchers', codes: b.map((v) => v.code), uptime: b[0].duration, shared_users: b[0].uses })}>Save latest batch to router</button></div>
      {/if}
      <h3 class="sub">On the router</h3>
      {#each saved as s}{@const used = s.users.filter((u) => u.used).length}
        <div class="row between small batch"><span>Batch <b class="mono">{s.batch}</b> · {s.users.length} voucher{s.users.length === 1 ? '' : 's'} · {used} used · {s.users[0]?.limit || 'no time limit'}</span>
          <button class="btn small" onclick={() => review(`Remove batch ${s.batch}`, [`/ip/hotspot/user/remove [find comment="tik:voucher:${s.batch}"]`], { kind: 'remove-hotspot-vouchers', batch: s.batch })} aria-label={`Remove voucher batch ${s.batch}`}>Remove batch</button></div>
        <div class="vgrid">
          {#each s.users as u}<div class="voucher" class:used={u.used}><b class="mono">{u.code}</b><span class="muted tiny">{u.used ? 'used' : 'unused'}{u.disabled ? ' · disabled' : ''}</span></div>{/each}
        </div>
      {:else}<div class="muted small">No vouchers on the router.</div>{/each}
    </Section>
  {:else if tab === 'guests'}
    <Section title="Guest sessions" desc="Who's logged in to the portal right now." ros="/ip/hotspot/active/print">
      <table><thead><tr><th>Guest</th><th>IP</th><th>Login</th><th>Time left</th><th class="r">Data</th><th></th></tr></thead>
        <tbody>{#each guests as g}<tr class="nohover"><td><b>{g.user ?? '—'}</b> <span class="muted mono small">{g['mac-address'] ?? ''}</span></td><td class="mono">{g.address}</td><td>{g['login-by'] ?? ''} <span class="muted small">{g.uptime ?? ''}</span></td><td>{g['session-time-left'] || 'no limit'}</td><td class="r mono small">{mb(g['bytes-out'])} ↓ / {mb(g['bytes-in'])} ↑</td><td class="muted small">{g.server ?? ''}</td></tr>
          {:else}<tr class="nohover"><td colspan="6" class="muted">{hotspots.length ? 'No active guests.' : 'Guest portal is not set up yet.'}</td></tr>{/each}</tbody></table>
    </Section>
  {:else}
    <Section title="Payments">
      <div class="muted small">Paid Wi-Fi isn't supported: hand out or sell vouchers instead.</div>
    </Section>
  {/if}
</div>

<style>
  .two { display: grid; grid-template-columns: minmax(0, 1fr) 300px; gap: 14px; align-items: start; }
  .r { text-align: right; margin-top: 10px; }
  .sub { font-size: 13px; margin: 18px 0 6px; }
  .batch { margin-top: 10px; padding: 4px 0; gap: 8px; }
  .preview { display: grid; justify-items: center; gap: 8px; position: sticky; top: 62px; }
  .phone { width: 250px; height: 480px; border-radius: 34px; border: 8px solid #222; background: #fff; overflow: hidden; box-shadow: var(--shadow); }
  .screen { height: 100%; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px; padding: 24px; color: #222; text-align: center; background: linear-gradient(180deg, color-mix(in srgb, var(--brand) 18%, #fff), #fff 45%); }
  .logo { width: 48px; height: 48px; border-radius: 14px; display: grid; place-items: center; background: var(--brand); color: #fff; }
  .screen h2 { margin: 6px 0 0; font-size: 20px; }
  .screen p { margin: 0; font-size: 13px; color: #555; }
  .screen input[type='text'] { width: 100%; background: #f3f4f6; color: #222; border: 1px solid #ddd; }
  .terms { font-size: 12px; color: #444; display: flex; gap: 6px; align-items: center; }
  .go { width: 100%; padding: 10px; border: 0; border-radius: 10px; background: var(--brand); color: #fff; font-weight: 650; }
  .c { text-align: center; }
  .vgrid { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 8px; }
  .voucher { display: grid; gap: 2px; padding: 10px 12px; border: 1px dashed var(--line); border-radius: 8px; }
  .voucher b { font-size: 17px; letter-spacing: .08em; }
  .used b { text-decoration: line-through; }
  .used { opacity: .6; }
  .small { font-size: 12px; } .tiny { font-size: 11px; }
  @media (max-width: 900px) { .two { grid-template-columns: minmax(0, 1fr); } .preview { position: static; } }
  @media print { :global(.appbar), :global(.rail), :global(.tabs), :global(header) { display: none !important; } }
</style>
