<script lang="ts">
  import { raw, internet, clients, parseUptime, dur } from '../../lib/data'
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

  // WireGuard peers tikontrol can pause or remove: everything but Back To Home (managed by /ip/cloud)
  const ownPeer = (p: any) => p.interface !== 'back-to-home-vpn' && !!p['public-key']
  const peerName = (p: any) => p.name || String(p['public-key']).slice(0, 8)
  const removePeer = (p: any) => review(`Remove WireGuard peer ${peerName(p)}`, [`/interface/wireguard/peers/remove [find public-key="${p['public-key']}"]`], { kind: 'remove-wireguard-peer', public_key: p['public-key'] })
  const togglePeer = (p: any) => review(`${p.disabled ? 'Enable' : 'Disable'} WireGuard peer ${peerName(p)}`, [`/interface/wireguard/peers/set [find public-key="${p['public-key']}"] disabled=${p.disabled ? 'no' : 'yes'}`], { kind: 'wireguard-peer-enabled', public_key: p['public-key'], enabled: !!p.disabled })

  // Connected now: L2TP/OpenVPN sessions plus WireGuard peers with a handshake in the last 3 minutes
  // (WireGuard re-handshakes every 2 minutes while a peer is up). Names come from other devices: text only.
  const HANDSHAKE_S = 180
  const connected = [
    ...(raw.pppActive ?? []).map((a: any) => ({ who: a.name, type: a.service === 'ovpn' ? 'OpenVPN' : a.service === 'l2tp' ? 'L2TP/IPsec' : String(a.service ?? ''), from: a['caller-id'] ?? '', address: a.address ?? '', since: `up ${dur(parseUptime(a.uptime))}`, enc: a.encoding || '—' })),
    ...peers.filter((p) => { const s = parseUptime(p['last-handshake']); return s != null && s <= HANDSHAKE_S && !p.disabled })
      .map((p) => ({ who: peerName(p), type: p.interface === 'back-to-home-vpn' ? 'Back To Home' : 'WireGuard', from: p['endpoint-address'] || '', address: (p['allowed-address'] ?? [])[0] ?? '', since: `handshake ${p['last-handshake']} ago`, enc: 'WireGuard' })),
  ]

  // OpenVPN: 7.17+ lists several servers (disabled=), older versions have one (enabled=)
  const ovpns: any[] = raw.ovpn ?? []
  const ovpnName: string = ovpns[0]?.name ?? ''
  const ovpnOn = (o: any) => (o.name ? !o.disabled : !!o.enabled)
  const noCert = (o: any) => !o.certificate || o.certificate === 'none'
  let ovpn = $state({ on: ovpns.some(ovpnOn) })
  const ovpnMissing = $derived(ovpns.length === 0 ? 'There is no OpenVPN server on this router yet.' : ovpns.every(noCert) ? 'The OpenVPN server has no certificate, so it can\'t accept connections. It needs a certificate authority (CA) and a server certificate signed by it.' : '')

  // L2TP always runs over IPsec: enabling sets use-ipsec=required with a pre-shared secret.
  // Secrets are typed or generated here, sent once in the intent, and masked in every preview.
  const ipsecRequired = l2tp['use-ipsec'] === 'required'
  let l2 = $state({ on: !!l2tp.enabled, secret: '', show: false })
  const SECRET_RE = /^[\x21-\x7e]{12,64}$/
  function randomSecret(n = 20) {
    const abc = 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789'
    const v = crypto.getRandomValues(new Uint32Array(n))
    return Array.from(v, (x) => abc[x % abc.length]).join('')
  }
  const l2Ok = $derived(!l2.on || (l2.secret ? SECRET_RE.test(l2.secret) : ipsecRequired))
  function l2Review() {
    const set = `/interface/l2tp-server/server/set enabled=${l2.on ? 'yes use-ipsec=required' : 'no'}${l2.on && l2.secret ? ' ipsec-secret="********"' : ''}`
    review('L2TP server', [set], { kind: 'vpn-server', server: 'l2tp', enabled: l2.on, ...(l2.on && l2.secret ? { ipsec_secret: l2.secret } : {}) })
    l2.secret = ''
    l2.show = false
  }

  // VPN users (/ppp/secret): only the ones tikontrol created (tik:vpn-user:) can be removed here
  const pppSecrets: any[] = raw.pppSecrets ?? []
  const profiles: string[] = (raw.pppProfiles ?? []).map((p: any) => p.name).filter(Boolean)
  const USER_RE = /^[A-Za-z0-9][A-Za-z0-9._-]{0,31}$/
  let user = $state({ name: '', password: '', service: 'l2tp', profile: profiles.includes('default-encryption') || !profiles.length ? 'default-encryption' : profiles[0], show: false })
  const userErr = $derived(!user.name ? '' : !USER_RE.test(user.name) ? 'Name: 1-32 letters, digits, . _ -' : user.password && !SECRET_RE.test(user.password) ? 'Password: 12-64 characters, no spaces' : user.password.toLowerCase().includes(user.name.toLowerCase()) ? 'The password must not contain the name' : '')
  function userReview() {
    review(`VPN user ${user.name}`, [`/ppp/secret/add name=${user.name} password="********" service=${user.service} profile=${user.profile} comment=tik:vpn-user:${user.name}`],
      { kind: 'vpn-user', name: user.name, password: user.password, service: user.service, profile: user.profile })
    user.password = ''
    user.show = false
  }
  const removeUser = (u: any) => review(`Remove VPN user ${u.name}`, [`/ppp/secret/remove [find comment="tik:vpn-user:${u.name}"]`, `/ppp/active/remove [find name="${u.name}"]`], { kind: 'remove-vpn-user', name: u.name })
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
    <thead><tr><th>Device</th><th>Interface</th><th>Allowed IPs</th><th>Last handshake</th><th class="r">Traffic</th><th><span class="sr">Actions</span></th></tr></thead>
    <tbody>{#each peers as p}<tr class="nohover"><td><b>{p.name}</b> <span class="muted small">{p.comment ?? ''}</span>{#if p.disabled} <span class="pill">Disabled</span>{/if}</td><td class="mono small">{p.interface}</td><td class="mono small">{(p['allowed-address'] ?? []).join(', ')}</td><td>{p['last-handshake'] ?? 'never'}</td><td class="r mono small">{p.rx ?? 0} / {p.tx ?? 0}</td>
      <td class="r nowrap">{#if ownPeer(p)}<button class="btn small" onclick={() => togglePeer(p)} aria-label={`${p.disabled ? 'Enable' : 'Disable'} ${peerName(p)}`}>{p.disabled ? 'Enable' : 'Disable'}</button> <button class="btn small" onclick={() => removePeer(p)} aria-label={`Remove ${peerName(p)}`}>Remove</button>{:else if p.interface === 'back-to-home-vpn'}<span class="muted small">MikroTik app</span>{/if}</td></tr>{/each}</tbody>
  </table>
</Section>

<Section title="Connected now" desc="VPN users and devices online at the moment: L2TP and OpenVPN sessions, and WireGuard peers with a handshake in the last 3 minutes." ros="/ppp/active/print; /interface/wireguard/peers/print">
  {#if connected.length}
    <table><thead><tr><th>Who</th><th>VPN</th><th>From</th><th>Address</th><th>Connected</th><th>Encryption</th></tr></thead>
      <tbody>{#each connected as c}<tr class="nohover"><td><b>{c.who}</b></td><td>{c.type}</td><td class="mono small">{c.from || '—'}</td><td class="mono small">{c.address || '—'}</td><td class="small">{c.since}</td><td class="small">{c.enc}</td></tr>{/each}</tbody></table>
  {:else}<div class="muted small">Nobody is connected over VPN right now.</div>{/if}
</Section>

<Section title="Back To Home" desc="MikroTik's built-in relay VPN (Teleport). Works behind double NAT; devices are added in the MikroTik app, so this is read-only." ros="/ip/cloud/print; /ip/cloud/back-to-home-users/print">
  <Field label="Status"><span class="pill" class:ok={internet.bth && bth?.running}>{internet.bth ? (bth?.running ? 'Running' : 'Enabled, not running') : (raw.cloud?.['back-to-home-vpn'] ?? 'Not available')}</span></Field>
  {#if bthUsers.length}
    <table><thead><tr><th>User</th><th>LAN access</th><th>Expires</th><th>State</th></tr></thead>
      <tbody>{#each bthUsers as u}<tr class="nohover"><td><b>{u.name}</b> <span class="muted small">{u.comment ?? ''}</span></td><td>{u['allow-lan'] ? 'Yes' : 'Internet only'}</td><td>{u.expires || 'never'}</td><td><span class="pill">{u.disabled ? 'Disabled' : 'Active'}</span></td></tr>{/each}</tbody></table>
  {:else}<Field label="Devices"><span>{peers.filter((p) => p.interface === 'back-to-home-vpn').length}</span></Field>{/if}
</Section>

<Section title="L2TP/IPsec server" desc="For devices without WireGuard: L2TP/IPsec is built into every OS. Each device needs the shared IPsec secret and a VPN user." ros="/interface/l2tp-server/server/print">
  <Field label="L2TP / IPsec"><Toggle bind:checked={l2.on} label="L2TP" /></Field>
  <Field label="Encryption"><span class="pill" class:ok={ipsecRequired}>{ipsecRequired ? 'IPsec required' : `IPsec ${l2tp['use-ipsec'] ?? 'unknown'}: not encrypted`}</span></Field>
  {#if l2.on}
    <Field label="IPsec secret" help={ipsecRequired ? 'Leave empty to keep the current secret, or set a new one (every device must then use it).' : 'Required: 12-64 characters, no spaces. Sent to the router once, never stored or shown again by tikontrol: copy it now.'}>
      <input type={l2.show ? 'text' : 'password'} class="mono" bind:value={l2.secret} autocomplete="new-password" aria-label="IPsec secret" />
      <button class="btn small" onclick={() => { l2.secret = randomSecret(); l2.show = true }}>Generate</button>
      <button class="btn small" onclick={() => (l2.show = !l2.show)} aria-pressed={l2.show}>{l2.show ? 'Hide' : 'Show'}</button>
    </Field>
  {/if}
  <div class="r"><button class="btn primary" disabled={!l2Ok} onclick={l2Review}>Review L2TP</button></div>
</Section>

<Section title="OpenVPN server" desc="For devices with an OpenVPN app. It needs a certificate on the router before it can accept connections." ros="/interface/ovpn-server/server/print">
  {#each ovpns as o}
    <Field label={o.name ? `Server ${o.name}` : 'Server'}><span class="pill" class:ok={ovpnOn(o) && !noCert(o)}>{ovpnOn(o) ? 'On' : 'Off'}</span> <span class="muted small">{o.protocol ?? 'tcp'} {o.port ?? 1194} · certificate: {noCert(o) ? 'none' : o.certificate}</span></Field>
  {/each}
  {#if ovpnMissing}
    <div class="note" role="note">
      <b>What's missing:</b> {ovpnMissing}
      tikontrol doesn't create certificates yet. In RouterOS: add a CA certificate and a server certificate under System → Certificates, sign the CA, then sign the server certificate with it, and pick the server certificate on the OpenVPN server. Devices then need the CA certificate (export it) and a VPN user below.
      <details><summary>Example commands</summary><pre class="cmds">/certificate/add name=vpn-ca common-name=vpn-ca key-usage=key-cert-sign,crl-sign days-valid=3650
/certificate/sign vpn-ca
/certificate/add name=vpn-server common-name=vpn.example.com key-usage=digital-signature,key-encipherment,tls-server days-valid=3650
/certificate/sign vpn-server ca=vpn-ca
/interface/ovpn-server/server/set certificate=vpn-server require-client-certificate=no</pre></details>
    </div>
  {/if}
  {#if ovpns.length}
    <Field label="OpenVPN"><Toggle bind:checked={ovpn.on} label="OpenVPN" /></Field>
    <div class="r"><button class="btn" disabled={ovpn.on && ovpns.every(noCert)} title={ovpn.on && ovpns.every(noCert) ? 'Add a certificate first' : undefined} onclick={() => review('OpenVPN server', [`/interface/ovpn-server/server/set ${ovpnName ? `[find name="${ovpnName}"] disabled=${ovpn.on ? 'no' : 'yes'}` : `enabled=${ovpn.on ? 'yes' : 'no'}`}`], { kind: 'vpn-server', server: 'ovpn', enabled: ovpn.on, name: ovpnName })}>Review OpenVPN</button></div>
  {/if}
</Section>

<Section title="VPN users" desc="Logins for L2TP/IPsec and OpenVPN. Passwords are sent to the router once and never stored or shown by tikontrol." ros="/ppp/secret/print">
  {#if pppSecrets.length}
    <table><thead><tr><th>User</th><th>VPN</th><th>Profile</th><th>State</th><th><span class="sr">Actions</span></th></tr></thead>
      <tbody>{#each pppSecrets as u}<tr class="nohover"><td><b>{u.name}</b></td><td>{u.service === 'ovpn' ? 'OpenVPN' : u.service === 'l2tp' ? 'L2TP' : (u.service ?? 'any')}</td><td class="small">{u.profile ?? ''}</td><td><span class="pill">{u.disabled ? 'Disabled' : 'Active'}</span></td>
        <td class="r">{#if u.comment === `tik:vpn-user:${u.name}`}<button class="btn small" onclick={() => removeUser(u)} aria-label={`Remove VPN user ${u.name}`}>Remove</button>{:else}<span class="muted small">not from tikontrol</span>{/if}</td></tr>{/each}</tbody></table>
  {/if}
  <Field label="Name" help="Letters, digits, . _ -"><input type="text" bind:value={user.name} autocomplete="off" aria-label="VPN user name" /></Field>
  <Field label="Password" help="12-64 characters, no spaces. Copy it now: it can't be shown again.">
    <input type={user.show ? 'text' : 'password'} class="mono" bind:value={user.password} autocomplete="new-password" aria-label="VPN user password" />
    <button class="btn small" onclick={() => { user.password = randomSecret(); user.show = true }}>Generate</button>
    <button class="btn small" onclick={() => (user.show = !user.show)} aria-pressed={user.show}>{user.show ? 'Hide' : 'Show'}</button>
  </Field>
  <Field label="VPN"><select bind:value={user.service} aria-label="VPN type"><option value="l2tp">L2TP/IPsec</option><option value="ovpn">OpenVPN</option></select></Field>
  <Field label="Profile" help="default-encryption requires encryption"><select bind:value={user.profile} aria-label="PPP profile">{#each profiles.length ? profiles : ['default-encryption'] as pr}<option value={pr}>{pr}</option>{/each}</select></Field>
  {#if userErr}<div class="muted small" role="alert">{userErr}</div>{/if}
  <div class="r"><button class="btn primary" disabled={!user.name || !user.password || !!userErr} onclick={userReview}>Review & apply</button></div>
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
  .nowrap { white-space: nowrap; }
  .note { border: 1px solid var(--line); border-radius: 8px; padding: 10px; font-size: 13px; line-height: 1.5; margin: 8px 0; }
  .note summary { cursor: pointer; margin-top: 6px; }
  .sr { position: absolute; left: 0; top: 0; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
  @media (max-width: 800px) { .cols { grid-template-columns: minmax(0, 1fr); } }
</style>
