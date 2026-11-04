<script lang="ts">
  import { raw, internet, clients } from '../../lib/data'
  import { review } from '../../lib/apply.svelte'
  import Icon from '../../lib/Icon.svelte'
  import Section from '../../lib/ui/Section.svelte'
  import Field from '../../lib/ui/Field.svelte'
  import Toggle from '../../lib/ui/Toggle.svelte'
  import Modal from '../../lib/ui/Modal.svelte'
  import Qr from '../../lib/ui/Qr.svelte'

  const wg: any[] = raw.wireguard ?? []
  const peers: any[] = raw.wgPeers ?? []
  const l2tp: any = raw.l2tp ?? {}
  const bth = wg.find((w) => w.name === 'back-to-home-vpn')

  // keys are generated in the browser and never leave it except inside the peer's config/QR
  const b64 = (b: ArrayBuffer) => btoa(String.fromCharCode(...new Uint8Array(b)))
  async function keypair() {
    const k = (await crypto.subtle.generateKey({ name: 'X25519' }, true, ['deriveBits'])) as CryptoKeyPair
    const pkcs8 = new Uint8Array(await crypto.subtle.exportKey('pkcs8', k.privateKey))
    return { priv: b64(pkcs8.slice(-32).buffer), pub: b64(await crypto.subtle.exportKey('raw', k.publicKey)) }
  }

  let add = $state<null | { name: string; ip: string; full: boolean; priv: string; pub: string; err?: string }>(null)
  async function newPeer() {
    const n = peers.length + 10
    add = { name: 'Phone', ip: `10.66.0.${n}/32`, full: true, priv: '', pub: '' }
    try { Object.assign(add, await keypair()) } catch { add.err = 'This browser has no X25519 in WebCrypto; the backend will generate keys instead.' }
  }
  const srv = wg.find((w) => w.name === 'wg-tik')
  const conf = $derived(add ? `[Interface]\nPrivateKey = ${add.priv}\nAddress = ${add.ip}\nDNS = ${internet.gatewayIp}\n\n[Peer]\nPublicKey = ${srv?.['public-key'] ?? '<router public key>'}\nEndpoint = ${raw.cloud?.['dns-name'] ?? internet.publicIp}:13231\nAllowedIPs = ${add.full ? '0.0.0.0/0' : '192.168.88.0/24'}\nPersistentKeepalive = 25` : '')

  const ovpnName: string = (raw.ovpn ?? [])[0]?.name ?? ''
  let ovpn = $state({ on: (raw.ovpn ?? []).some((o: any) => !o.disabled) })
  let l2 = $state({ on: !!l2tp.enabled })
  const tagged = (k: string) => wg.filter((w) => String(w.comment ?? '').startsWith(`tik:${k}:`))
  const vpnClients = tagged('vpnc')
  const tunnels = tagged('s2s')
  const rules: any[] = raw.routingRules ?? []
  const bthUsers: any[] = raw.bthUsers ?? []
  const peerOf = (n: string) => peers.find((p) => p.interface === n)
  const sourcesOf = (n: string) => rules.filter((r) => r.comment === `tik:vpnc:${n}` && r.table === `tik-vpn-${n}`).map((r) => r['src-address'])
  const lans = [...new Set<string>((raw.dhcpNetworks ?? []).map((n: any) => n.address as string).filter(Boolean))]
  const splitEp = (s: string): [string, number] => { const [h, p] = s.trim().split(':'); return [h ?? '', +(p || 51820)] }

  // provider's private key is pasted, generated here, or left empty for the router to generate one
  let client = $state({ name: 'vpn1', address: '', publicKey: '', endpoint: '', privateKey: '', myPub: '', all: false, only: [] as string[], err: '' })
  async function genClientKey() {
    try { const k = await keypair(); client.privateKey = k.priv; client.myPub = k.pub } catch { client.err = 'This browser has no X25519 in WebCrypto: leave the key empty and the router generates one.' }
  }
  const clientSources = $derived(client.all ? lans : client.only)
  function clientReview() {
    const [ep, port] = splitEp(client.endpoint)
    review(`VPN client ${client.name}`, [`/interface/wireguard/add name=${client.name}${client.privateKey ? ' private-key="********"' : ''}`, `/interface/wireguard/peers/add interface=${client.name} endpoint-address=${ep} endpoint-port=${port} allowed-address=0.0.0.0/0`, ...clientSources.map((s) => `/routing/rule/add src-address=${s} action=lookup table=tik-vpn-${client.name}`)],
      { kind: 'wireguard-client', name: client.name, private_key: client.privateKey, address: client.address, public_key: client.publicKey.trim(), endpoint: ep, endpoint_port: port, sources: clientSources })
  }

  let s2s = $state({ name: 's2s1', port: 13232, address: '10.255.0.1/30', publicKey: '', endpoint: '', remote: '10.0.1.0/24' })
  function s2sReview() {
    const [ep, port] = s2s.endpoint ? splitEp(s2s.endpoint) : ['', 0]
    const remote = s2s.remote.split(',').map((x) => x.trim()).filter(Boolean)
    review(`Site-to-site ${s2s.name}`, [`/interface/wireguard/add name=${s2s.name} listen-port=${s2s.port}`, `/interface/wireguard/peers/add interface=${s2s.name} allowed-address=${remote.join(',')}`, ...remote.map((r) => `/ip/route/add dst-address=${r} gateway=${s2s.name}`)],
      { kind: 'site-to-site', name: s2s.name, listen_port: +s2s.port, address: s2s.address, public_key: s2s.publicKey.trim(), endpoint: ep, endpoint_port: port || +s2s.port, remote_subnets: remote })
  }
</script>

<Section title="WireGuard server" desc="Phones and laptops connect home. One QR code per device." ros="/interface/wireguard/print; /interface/wireguard/peers/print">
  {#snippet actions()}<button class="btn primary" onclick={newPeer}><Icon name="plus" size={14} /> Add device</button>{/snippet}
  {#if !srv}<div class="muted small">No tikontrol WireGuard server yet. <button class="btn small" onclick={() => review('WireGuard server wg-tik', ['/interface/wireguard/add name=wg-tik listen-port=13231', '/ip/address/add address=10.66.0.1/24 interface=wg-tik', '/interface/list/member/add list=LAN interface=wg-tik', '/ip/firewall/filter/add chain=input protocol=udp dst-port=13231 action=accept comment=tik:wgsrv:wg-tik'], { kind: 'wireguard-server', name: 'wg-tik', listen_port: 13231, address: '10.66.0.1/24', lan: true })}>Create <span class="mono">wg-tik</span> on port 13231</button></div>{/if}
  <table>
    <thead><tr><th>Device</th><th>Interface</th><th>Allowed IPs</th><th>Last handshake</th><th class="r">Traffic</th></tr></thead>
    <tbody>{#each peers as p}<tr class="nohover"><td><b>{p.name}</b> <span class="muted small">{p.comment ?? ''}</span></td><td class="mono small">{p.interface}</td><td class="mono small">{(p['allowed-address'] ?? []).join(', ')}</td><td>{p['last-handshake'] ?? 'never'}</td><td class="r mono small">{p.rx ?? 0} / {p.tx ?? 0}</td></tr>{/each}</tbody>
  </table>
</Section>

<Section title="Back To Home" desc="MikroTik's built-in relay VPN (Teleport). Works behind double NAT; devices are added in the MikroTik app, so this is read-only." ros="/ip/cloud/print; /ip/cloud/back-to-home-users/print">
  <Field label="Status"><span class="pill" class:ok={internet.bth && bth?.running}>{internet.bth ? (bth?.running ? 'Running' : 'Enabled, not running') : (raw.cloud?.['back-to-home-vpn'] ?? 'Not available')}</span></Field>
  {#if bthUsers.length}
    <table><thead><tr><th>User</th><th>LAN access</th><th>Expires</th><th>State</th></tr></thead>
      <tbody>{#each bthUsers as u}<tr class="nohover"><td><b>{u.name}</b> <span class="muted small">{u.comment ?? ''}</span></td><td>{u['allow-lan'] ? 'Yes' : 'Internet only'}</td><td>{u.expires || 'never'}</td><td><span class="pill">{u.disabled ? 'Disabled' : 'Active'}</span></td></tr>{/each}</tbody></table>
  {:else}<Field label="Devices"><span>{peers.filter((p) => p.interface === 'back-to-home-vpn').length}</span></Field>{/if}
</Section>

<Section title="OpenVPN & L2TP servers" desc="For devices without WireGuard. L2TP/IPsec is built into every OS." ros="/interface/ovpn-server/server/print; /interface/l2tp-server/server/print">
  <Field label="OpenVPN"><Toggle bind:checked={ovpn.on} label="OpenVPN" /></Field>
  <Field label="L2TP / IPsec"><Toggle bind:checked={l2.on} label="L2TP" /></Field>
  <div class="r">
    <button class="btn" onclick={() => review('OpenVPN server', [`/interface/ovpn-server/server/set ${ovpnName ? `[find name="${ovpnName}"] disabled=${ovpn.on ? 'no' : 'yes'}` : `enabled=${ovpn.on ? 'yes' : 'no'}`}`], { kind: 'vpn-server', server: 'ovpn', enabled: ovpn.on, name: ovpnName })}>Review OpenVPN</button>
    <button class="btn primary" onclick={() => review('L2TP server', [`/interface/l2tp-server/server/set enabled=${l2.on ? 'yes' : 'no'}`], { kind: 'vpn-server', server: 'l2tp', enabled: l2.on })}>Review L2TP</button>
  </div>
</Section>

<Section title="VPN client" desc="WireGuard tunnel to a VPN provider. Pick which devices or networks go through it (policy-based routing); the rest keeps your normal connection." ros="/interface/wireguard/print; /routing/rule/print">
  {#if vpnClients.length}
    <table><thead><tr><th>Tunnel</th><th>Provider</th><th>Routed through it</th><th>State</th></tr></thead>
      <tbody>{#each vpnClients as v}{@const p = peerOf(v.name)}<tr class="nohover"><td><b>{v.name}</b>{#if v['public-key']}<div class="mono tiny muted">{v['public-key']}</div>{/if}</td><td class="mono small">{p ? `${p['endpoint-address']}:${p['endpoint-port']}` : '—'}</td><td class="mono small">{sourcesOf(v.name).join(', ') || 'nothing'}</td><td><span class="pill" class:ok={v.running}>{v.running ? 'Up' : 'Down'}</span>{#if p?.['last-handshake']} <span class="muted small">{p['last-handshake']}</span>{/if}</td></tr>{/each}</tbody></table>
  {/if}
  <Field label="Name"><input type="text" bind:value={client.name} aria-label="Tunnel name" /></Field>
  <Field label="Tunnel address" help="From the provider's config, e.g. 10.2.0.2/32"><input type="text" bind:value={client.address} aria-label="Tunnel address" /></Field>
  <Field label="Provider public key"><input type="text" class="mono" bind:value={client.publicKey} aria-label="Provider public key" /></Field>
  <Field label="Endpoint" help="host:port"><input type="text" bind:value={client.endpoint} placeholder="vpn.example.com:51820" aria-label="Endpoint" /></Field>
  <Field label="Private key" help="Paste the provider's, generate one here, or leave empty for the router to make one. Sent to the router once, never stored by tikontrol.">
    <input type="password" class="mono" bind:value={client.privateKey} autocomplete="off" aria-label="Private key" /><button class="btn small" onclick={genClientKey}>Generate</button></Field>
  {#if client.myPub}<Field label="Your public key" help="Register this with the provider"><span class="mono small">{client.myPub}</span></Field>{/if}
  {#if client.err}<div class="muted small">{client.err}</div>{/if}
  <Field label="Route all LAN devices" help={lans.join(', ') || 'No DHCP networks found'}><Toggle bind:checked={client.all} label="All LAN devices" /></Field>
  {#if !client.all}<Field label="Only these devices" help="Policy-based routing per device"><select multiple bind:value={client.only} aria-label="Devices routed through the VPN" size="4">{#each clients.filter((c) => c.ip && c.ip !== '—') as c}<option value={`${c.ip}/32`}>{c.name} ({c.ip})</option>{/each}</select></Field>{/if}
  <div class="r"><button class="btn primary" disabled={!client.name || !client.address || !client.publicKey || !client.endpoint} onclick={clientReview}>Review & apply</button></div>
</Section>

<Section title="Site-to-site" desc="Join another office or home network over WireGuard. Both sides need each other's public key." ros="/interface/wireguard/print; /ip/route/print">
  {#if tunnels.length}
    <table><thead><tr><th>Tunnel</th><th>This router's public key</th><th>Remote subnets</th><th>State</th></tr></thead>
      <tbody>{#each tunnels as t}{@const p = peerOf(t.name)}<tr class="nohover"><td><b>{t.name}</b> <span class="muted small">:{t['listen-port']}</span></td><td class="mono tiny">{t['public-key'] ?? '—'}</td><td class="mono small">{(p?.['allowed-address'] ?? []).slice(1).join(', ')}</td><td><span class="pill" class:ok={t.running}>{t.running ? 'Up' : 'Down'}</span>{#if p?.['last-handshake']} <span class="muted small">{p['last-handshake']}</span>{/if}</td></tr>{/each}</tbody></table>
  {/if}
  <Field label="Name"><input type="text" bind:value={s2s.name} aria-label="Tunnel name" /></Field>
  <Field label="Listen port"><input type="number" bind:value={s2s.port} aria-label="Listen port" /></Field>
  <Field label="Tunnel address" help="This side, e.g. 10.255.0.1/30; the other side takes .2"><input type="text" bind:value={s2s.address} aria-label="Tunnel address" /></Field>
  <Field label="Remote public key"><input type="text" class="mono" bind:value={s2s.publicKey} aria-label="Remote public key" /></Field>
  <Field label="Remote router" help="host:port, empty if only the other side connects"><input type="text" bind:value={s2s.endpoint} placeholder="vpn.example.com:13232" aria-label="Remote router" /></Field>
  <Field label="Remote subnets" help="Comma separated"><input type="text" bind:value={s2s.remote} aria-label="Remote subnets" /></Field>
  <div class="r"><button class="btn primary" disabled={!s2s.name || !s2s.publicKey || !s2s.remote} onclick={s2sReview}>Review & apply</button></div>
</Section>

{#if add}
  <Modal title="Add VPN device" onclose={() => (add = null)} wide>
    <div class="cols">
      <div>
        <Field label="Name"><input type="text" bind:value={add.name} /></Field>
        <Field label="Address"><input type="text" bind:value={add.ip} /></Field>
        <Field label="Send all traffic home"><Toggle bind:checked={add.full} label="Full tunnel" /></Field>
        {#if add.err}<div class="muted small">{add.err}</div>{/if}
        <pre class="cmds">{conf}</pre>
      </div>
      <div class="qr">{#if add.priv}<Qr text={conf} size={220} /><span class="muted small">Scan with the WireGuard app. The private key is shown only now.</span>{/if}</div>
    </div>
    {#snippet footer()}<button class="btn" onclick={() => (add = null)}>Cancel</button><button class="btn primary" disabled={!srv || !add?.pub} title={srv ? undefined : 'Create the WireGuard server first'} onclick={() => { review(`VPN device ${add!.name}`, [
      ...(wg.some((w) => w.name === 'wg-tik') ? [] : ['/interface/wireguard/add name=wg-tik listen-port=13231', '/ip/address/add address=10.66.0.1/24 interface=wg-tik', '/interface/list/member/add list=LAN interface=wg-tik', '/ip/firewall/filter/add chain=input protocol=udp dst-port=13231 action=accept place-before=0 comment="tik:wireguard"']),
      `/interface/wireguard/peers/add interface=wg-tik name="${add!.name}" public-key="${add!.pub}" allowed-address=${add!.ip} comment="tik:wg:${add!.pub}"`,
    ], { kind: 'wireguard-peer', interface: 'wg-tik', name: add!.name, public_key: add!.pub, allowed_address: add!.ip }); add = null }}>Review & apply</button>{/snippet}
  </Modal>
{/if}

<style>
  .cols { display: grid; grid-template-columns: minmax(0, 1fr) 260px; gap: 18px; }
  .qr { display: grid; justify-items: center; align-content: start; gap: 8px; text-align: center; }
  .cmds { background: var(--code-bg); border: 1px solid var(--line); border-radius: 8px; padding: 10px; font: 11px/1.6 var(--mono); white-space: pre-wrap; word-break: break-all; }
  .r { text-align: right; margin-top: 10px; }
  .small { font-size: 12px; } .tiny { font-size: 11px; word-break: break-all; }
  @media (max-width: 800px) { .cols { grid-template-columns: minmax(0, 1fr); } }
</style>
