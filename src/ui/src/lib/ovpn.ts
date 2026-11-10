// OpenVPN client profile (.ovpn) for tikontrol's OpenVPN server, built in the browser.
// The profile holds no secret: the client asks for the VPN user's name and password (auth-user-pass),
// and the only certificate in it is the public certificate of tikontrol's CA (tik-ovpn-ca), which the
// server exports with /certificate/export-certificate (no passphrase, so never the key).
// Every value written into the file is validated first: a newline in a host name would add directives.

export const CA_FILE = 'cert_export_tik-ovpn-ca.crt'
export const CA_CERT = 'tik-ovpn-ca'
export const SERVER_CERT = 'tik-ovpn-server'

/** Same rule as the server's common_name(): 1-64 letters, digits, . and -, no dot or dash at the ends. */
export const CN_RE = /^[A-Za-z0-9](?:[A-Za-z0-9.-]{0,62}[A-Za-z0-9])?$/

const str = (v: unknown) => (typeof v === 'string' ? v : '')
const list = (v: unknown): string[] => (Array.isArray(v) ? v.map(str) : str(v).split(',')).map((s) => s.trim()).filter(Boolean)

/** A DNS name or an IPv4 address, nothing else (no spaces, no ports, no line breaks). */
export function validHost(h: string): boolean {
  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(h)) return h.split('.').every((x) => +x <= 255)
  return h.length > 0 && h.length <= 253 && h.split('.').every((l) => /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/.test(l))
}

/** The router's name on the internet from /ip/cloud: the DDNS name when DDNS is on, else the public address. */
export function remoteHost(cloud: any): string {
  const dns = str(cloud?.['dns-name']).trim()
  const ddns = cloud?.['ddns-enabled'] === true || cloud?.['ddns-enabled'] === 'yes' || cloud?.['ddns-enabled'] === 'true'
  const ip = str(cloud?.['public-address']).trim()
  for (const h of ddns ? [dns, ip] : [ip, dns]) if (h && validHost(h)) return h
  return ''
}

/**
 * The PEM certificates in `text`, normalised, or null when it holds anything else: a private key, an
 * encrypted key, or text outside the certificate blocks.
 */
export function publicPem(text: string): string | null {
  const t = text.replace(/\r\n?/g, '\n').trim()
  const blocks = [...t.matchAll(/-----BEGIN CERTIFICATE-----\n([A-Za-z0-9+/=\n]+?)\n?-----END CERTIFICATE-----/g)]
  if (!blocks.length) return null
  const rest = blocks.reduce((s, b) => s.replace(b[0], ''), t)
  if (rest.trim()) return null
  return blocks.map((b) => `-----BEGIN CERTIFICATE-----\n${b[1].split('\n').filter(Boolean).join('\n')}\n-----END CERTIFICATE-----`).join('\n')
}

// RouterOS cipher names → OpenVPN's, strongest first. blowfish128 and null are left out on purpose.
const CIPHERS: [string, string][] = [['aes256-gcm', 'AES-256-GCM'], ['aes192-gcm', 'AES-192-GCM'], ['aes128-gcm', 'AES-128-GCM'], ['aes256-cbc', 'AES-256-CBC'], ['aes192-cbc', 'AES-192-CBC'], ['aes128-cbc', 'AES-128-CBC']]
const AUTHS: [string, string][] = [['sha512', 'SHA512'], ['sha256', 'SHA256'], ['sha1', 'SHA1'], ['md5', 'MD5']]

export type OvpnServer = { port?: unknown; protocol?: unknown; auth?: unknown; cipher?: unknown; mode?: unknown }

/** The client settings that match a RouterOS OpenVPN server row (raw.ovpn). */
export function clientSettings(s: OvpnServer) {
  const port = Number(s.port)
  const ciphers = list(s.cipher)
  const auths = list(s.auth)
  const data = CIPHERS.filter(([r]) => ciphers.includes(r)).map(([, o]) => o)
  return {
    port: Number.isInteger(port) && port >= 1 && port <= 65535 ? port : 1194,
    proto: str(s.protocol) === 'udp' ? 'udp' : 'tcp-client',
    dev: str(s.mode) === 'ethernet' ? 'tap' : 'tun',
    // RouterOS defaults when the router didn't say
    ciphers: data.length ? data : ['AES-256-GCM', 'AES-128-GCM', 'AES-256-CBC', 'AES-128-CBC'],
    auth: AUTHS.find(([r]) => auths.includes(r))?.[1] ?? '',
  }
}

/** The .ovpn text. Throws on an invalid host or a CA file that isn't plain public certificates. */
export function ovpnProfile(o: { remote: string; server: OvpnServer; caPem: string }): string {
  const remote = o.remote.trim()
  if (!validHost(remote)) throw new Error('The server address must be a host name or an IPv4 address.')
  const ca = publicPem(o.caPem)
  if (!ca) throw new Error('The CA file is not a plain public certificate, so no profile was made.')
  const s = clientSettings(o.server)
  return [
    `# OpenVPN client profile from tikontrol for ${remote}`,
    '# Log in with a VPN user (OpenVPN). This file holds no password or private key.',
    'client',
    `dev ${s.dev}`,
    `proto ${s.proto}`,
    `remote ${remote} ${s.port}`,
    'resolv-retry infinite',
    'nobind',
    'persist-key',
    'persist-tun',
    'remote-cert-tls server',
    'auth-user-pass',
    'auth-nocache',
    ...(s.auth ? [`auth ${s.auth}`] : []),
    `data-ciphers ${s.ciphers.join(':')}`,
    `data-ciphers-fallback ${s.ciphers[0]}`,
    'verb 3',
    '<ca>',
    ca,
    '</ca>',
    '',
  ].join('\n')
}

/** A safe download name for the profile. */
export const profileName = (remote: string) => `tikontrol-${validHost(remote.trim()) ? remote.trim() : 'vpn'}.ovpn`
