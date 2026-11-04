<script lang="ts">
  import { raw, internet } from '../../lib/data'
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
  let client = $state({ kind: 'wireguard', endpoint: '', all: false })
  let s2s = $state({ remote: '', subnet: '10.0.1.0/24' })
  let policy = $state({ client: '', via: 'vpn-client' })
</script>

<Section title="WireGuard server" desc="Phones and laptops connect home. One QR code per device." ros="/interface/wireguard/print; /interface/wireguard/peers/print">
  {#snippet actions()}<button class="btn primary" onclick={newPeer}><Icon name="plus" size={14} /> Add device</button>{/snippet}
  {#if !srv}<div class="muted small">No tikontrol WireGuard server yet. <button class="btn small" onclick={() => review('WireGuard server wg-tik', ['/interface/wireguard/add name=wg-tik listen-port=13231', '/ip/address/add address=10.66.0.1/24 interface=wg-tik', '/interface/list/member/add list=LAN interface=wg-tik', '/ip/firewall/filter/add chain=input protocol=udp dst-port=13231 action=accept comment=tik:wgsrv:wg-tik'], { kind: 'wireguard-server', name: 'wg-tik', listen_port: 13231, address: '10.66.0.1/24', lan: true })}>Create <span class="mono">wg-tik</span> on port 13231</button></div>{/if}
  <table>
    <thead><tr><th>Device</th><th>Interface</th><th>Allowed IPs</th><th>Last handshake</th><th class="r">Traffic</th></tr></thead>
    <tbody>{#each peers as p}<tr class="nohover"><td><b>{p.name}</b> <span class="muted small">{p.comment ?? ''}</span></td><td class="mono small">{p.interface}</td><td class="mono small">{(p['allowed-address'] ?? []).join(', ')}</td><td>{p['last-handshake'] ?? 'never'}</td><td class="r mono small">{p.rx ?? 0} / {p.tx ?? 0}</td></tr>{/each}</tbody>
  </table>
</Section>

<Section title="Back To Home" desc="MikroTik's built-in relay VPN. Works behind double NAT, managed from the MikroTik app." ros="/ip/cloud/back-to-home-users/print">
  <Field label="Status"><span class="pill" class:ok={bth?.running}>{bth?.running ? 'Running' : 'Off'}</span></Field>
  <Field label="Devices"><span>{peers.filter((p) => p.interface === 'back-to-home-vpn').length}</span></Field>
</Section>

<Section title="OpenVPN & L2TP servers" desc="For devices without WireGuard. L2TP/IPsec is built into every OS." ros="/interface/ovpn-server/server/print; /interface/l2tp-server/server/print">
  <Field label="OpenVPN"><Toggle bind:checked={ovpn.on} label="OpenVPN" /></Field>
  <Field label="L2TP / IPsec"><Toggle bind:checked={l2.on} label="L2TP" /></Field>
  <div class="r">
    <button class="btn" onclick={() => review('OpenVPN server', [`/interface/ovpn-server/server/set ${ovpnName ? `[find name="${ovpnName}"] disabled=${ovpn.on ? 'no' : 'yes'}` : `enabled=${ovpn.on ? 'yes' : 'no'}`}`], { kind: 'vpn-server', server: 'ovpn', enabled: ovpn.on, name: ovpnName })}>Review OpenVPN</button>
    <button class="btn primary" onclick={() => review('L2TP server', [`/interface/l2tp-server/server/set enabled=${l2.on ? 'yes' : 'no'}`], { kind: 'vpn-server', server: 'l2tp', enabled: l2.on })}>Review L2TP</button>
  </div>
</Section>

<Section title="VPN client" desc="Send some or all traffic through a commercial or remote VPN.">
  <Field label="Type"><select bind:value={client.kind}><option value="wireguard">WireGuard</option><option value="ovpn">OpenVPN</option></select></Field>
  <Field label="Endpoint"><input type="text" bind:value={client.endpoint} placeholder="vpn.example.com:51820" /></Field>
  <Field label="Route all traffic"><Toggle bind:checked={client.all} label="All traffic" /></Field>
  <Field label="Only for client" help="Policy-based routing: send one device through the VPN"><input type="text" bind:value={policy.client} placeholder="IP or leave empty" /></Field>
  <div class="r"><button class="btn primary" disabled={!client.endpoint} onclick={() => review('VPN client', [
    `/interface/wireguard/add name=vpn-client`, `/interface/wireguard/peers/add interface=vpn-client endpoint-address=${client.endpoint.split(':')[0]} endpoint-port=${client.endpoint.split(':')[1] ?? 51820} allowed-address=0.0.0.0/0`,
    '/routing/table/add name=via-vpn fib', '/ip/route/add dst-address=0.0.0.0/0 gateway=vpn-client routing-table=via-vpn',
    client.all ? '/ip/firewall/mangle/add chain=prerouting in-interface-list=LAN action=mark-routing new-routing-mark=via-vpn' : `/routing/rule/add src-address=${policy.client || '0.0.0.0/0'} action=lookup table=via-vpn`,
  ])}>Review & apply</button></div>
</Section>

<Section title="Site-to-site" desc="Join another office or home network over WireGuard.">
  <Field label="Remote router"><input type="text" bind:value={s2s.remote} placeholder="vpn.example.com" /></Field>
  <Field label="Remote subnet"><input type="text" bind:value={s2s.subnet} /></Field>
  <div class="r"><button class="btn primary" disabled={!s2s.remote} onclick={() => review('Site-to-site', [`/interface/wireguard/add name=s2s listen-port=13232`, `/interface/wireguard/peers/add interface=s2s endpoint-address=${s2s.remote} endpoint-port=13232 allowed-address=${s2s.subnet}`, `/ip/route/add dst-address=${s2s.subnet} gateway=s2s`])}>Review & apply</button></div>
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
  .small { font-size: 12px; }
  @media (max-width: 800px) { .cols { grid-template-columns: minmax(0, 1fr); } }
</style>
