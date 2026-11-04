<script lang="ts">
  import { raw, ssids } from '../lib/data'
  import { review } from '../lib/apply.svelte'
  import Icon from '../lib/Icon.svelte'
  import Tabs from '../lib/ui/Tabs.svelte'
  import Section from '../lib/ui/Section.svelte'
  import Field from '../lib/ui/Field.svelte'
  import Toggle from '../lib/ui/Toggle.svelte'

  let { tab = 'portal' }: { tab?: string } = $props()
  const hotspots: any[] = raw.hotspot ?? []
  const guests: any[] = raw.hotspotActive ?? []
  const mb = (b: unknown) => `${((Number(b) || 0) / 1e6).toFixed(1)} MB`

  let portal = $state({ on: hotspots.length > 0, iface: 'guest-vlan', address: '10.10.10.1/24', ssid: 'Guest', title: 'Welcome', text: 'Free Wi-Fi for our guests.', color: '#2672f5', auth: 'terms', password: '', duration: '1d', limit: '20M/5M' })

  // vouchers are generated in this browser (crypto.getRandomValues) and created as /ip/hotspot/user entries
  type V = { code: string; duration: string; uses: number; created: string; used: boolean }
  let vouchers = $state<V[]>([])
  let gen = $state({ count: 10, duration: '1d', uses: 1 })
  const code = () => Array.from(crypto.getRandomValues(new Uint8Array(8)), (b) => 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789'[b % 32]).join('')
  function generate() {
    const created = new Date().toLocaleString()
    vouchers = [...Array.from({ length: gen.count }, () => ({ code: code(), duration: gen.duration, uses: gen.uses, created, used: false })), ...vouchers]
  }
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
  const batch = (vs: V[]) => vs.filter((v) => v.created === vs[0].created)
</script>

<div class="grid">
  <h1>Hotspot</h1>
  <Tabs base="hotspot" active={tab} tabs={[{ id: 'portal', label: 'Guest portal' }, { id: 'vouchers', label: 'Vouchers', count: vouchers.length }, { id: 'guests', label: 'Guests', count: guests.length }, { id: 'payments', label: 'Payments' }]} />

  {#if tab === 'portal'}
    <div class="two">
      <Section title="Captive portal" desc="A separate guest SSID with a login page. RouterOS Hotspot + a custom HTML page served from the router." ros="/ip/hotspot/print; /ip/hotspot/profile/print">
        <Field label="Enabled"><Toggle bind:checked={portal.on} label="Guest portal" /></Field>
        <Field label="Guest interface" help="Only guests should use it: a guest VLAN or guest Wi-Fi, never your main LAN"><input type="text" bind:value={portal.iface} /></Field>
        <Field label="Guest network" help="The router's address on it"><input type="text" bind:value={portal.address} /></Field>
        <Field label="Guest SSID"><input type="text" bind:value={portal.ssid} /></Field>
        <Field label="Authentication">
          <select bind:value={portal.auth}><option value="terms">Accept terms</option><option value="voucher">Voucher</option></select>
        </Field>
        <Field label="Session length"><select bind:value={portal.duration}><option>1h</option><option>8h</option><option>1d</option><option>7d</option></select></Field>
        <Field label="Speed limit per guest" help="Download/upload"><input type="text" bind:value={portal.limit} /></Field>
        <Field label="Title"><input type="text" bind:value={portal.title} /></Field>
        <Field label="Message"><input type="text" bind:value={portal.text} /></Field>
        <Field label="Brand colour"><input type="color" bind:value={portal.color} /></Field>
        <div class="muted small">Title, message and colour are a preview only: the router serves its default login page until custom HTML upload exists.</div>
        <div class="r"><button class="btn primary" onclick={applyPortal}>Review & apply</button></div>
      </Section>
      <div class="preview">
        <div class="phone">
          <div class="screen" style="--brand:{portal.color}">
            <div class="logo"><Icon name="wifi" size={22} /></div>
            <h2>{portal.title}</h2>
            <p>{portal.text}</p>
            {#if portal.auth === 'password'}<input type="text" placeholder="Password" disabled />{/if}
            {#if portal.auth === 'voucher'}<input type="text" placeholder="Voucher code" disabled />{/if}
            {#if portal.auth === 'terms'}<label class="terms"><input type="checkbox" checked disabled /> I accept the terms of use</label>{/if}
            <button class="go">Connect</button>
          </div>
        </div>
        <div class="muted small c">Preview · {portal.ssid}</div>
      </div>
    </div>
  {:else if tab === 'vouchers'}
    <Section title="Vouchers" desc="Codes for guests: each code is a hotspot login. Generated in this browser, kept on the router." ros="/ip/hotspot/user/print">
      {#snippet actions()}
        <input type="number" bind:value={gen.count} min="1" max="100" style="width:70px" />
        <select bind:value={gen.duration}><option>1h</option><option>1d</option><option>7d</option><option>30d</option></select>
        <button class="btn primary" onclick={generate}><Icon name="plus" size={14} /> Create</button>
        <button class="btn" disabled={!vouchers.length} onclick={() => print()}>Print</button>
      {/snippet}
      <div class="vgrid">
        {#each vouchers as v}
          <div class="voucher" class:used={v.used}><span class="muted small">Wi-Fi voucher · {v.duration}</span><b class="mono">{v.code}</b><span class="muted tiny">{ssids[0]?.ssid ?? ''} guest</span></div>
        {:else}<div class="muted small">No vouchers yet.</div>{/each}
      </div>
      {#if vouchers.length}{@const b = batch(vouchers)}<div class="r"><button class="btn primary" onclick={() => review(`Create ${b.length} vouchers`, b.map((v) => `/ip/hotspot/user/add name=${v.code} limit-uptime=${v.duration} shared-users=${v.uses} comment=tik:voucher:${b[0].code}`), { kind: 'hotspot-vouchers', codes: b.map((v) => v.code), uptime: b[0].duration, shared_users: b[0].uses })}>Save latest batch to router</button></div>{/if}
    </Section>
  {:else if tab === 'guests'}
    <Section title="Guest sessions" desc="Who's logged in to the portal right now." ros="/ip/hotspot/active/print">
      <table><thead><tr><th>Guest</th><th>IP</th><th>Login</th><th>Time left</th><th class="r">Data</th><th></th></tr></thead>
        <tbody>{#each guests as g}<tr class="nohover"><td><b>{g.user ?? '—'}</b> <span class="muted mono small">{g['mac-address'] ?? ''}</span></td><td class="mono">{g.address}</td><td>{g['login-by'] ?? ''} <span class="muted small">{g.uptime ?? ''}</span></td><td>{g['session-time-left'] || 'no limit'}</td><td class="r mono small">{mb(g['bytes-out'])} ↓ / {mb(g['bytes-in'])} ↑</td><td class="muted small">{g.server ?? ''}</td></tr>
          {:else}<tr class="nohover"><td colspan="6" class="muted">{hotspots.length ? 'No active guests.' : 'Guest portal is not set up yet.'}</td></tr>{/each}</tbody></table>
    </Section>
  {:else}
    <Section title="Payments" desc="Paid Wi-Fi (Stripe and similar) is out of scope for tikontrol.">
      <div class="muted small">Use vouchers instead: sell or hand out codes.</div>
    </Section>
  {/if}
</div>

<style>
  .two { display: grid; grid-template-columns: minmax(0, 1fr) 300px; gap: 14px; align-items: start; }
  .r { text-align: right; margin-top: 10px; }
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
  .used { opacity: .4; }
  .small { font-size: 12px; } .tiny { font-size: 11px; }
  @media (max-width: 900px) { .two { grid-template-columns: minmax(0, 1fr); } .preview { position: static; } }
  @media print { :global(.appbar), :global(.rail), :global(.tabs), :global(header) { display: none !important; } }
</style>
