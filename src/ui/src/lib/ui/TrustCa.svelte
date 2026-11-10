<script lang="ts">
  // "Trust this tikontrol": download tikontrol's local CA (public certificate only) and compare its SHA-256.
  let fp = $state('')
  // only HTTPS has a local CA (and crypto.subtle needs a secure context); plain HTTP is a local dev server
  const https = location.protocol === 'https:'
  let missing = $state(!https)
  if (https) fetch('/api/v1/ca.crt', { credentials: 'same-origin' })
    .then(async (r) => {
      if (!r.ok) { missing = true; return }
      const b64 = (await r.text()).replace(/-----[^-]+-----|\s/g, '')
      const der = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0))
      const h = new Uint8Array(await crypto.subtle.digest('SHA-256', der))
      fp = [...h].map((x) => x.toString(16).padStart(2, '0').toUpperCase()).join(':')
    })
    .catch(() => {})
</script>

{#if missing}
  <p class="muted small">tikontrol uses your own certificate (TIK_TLS_CERT), so there is no local CA to install.</p>
{:else}
  <div class="trust">
    <p class="small">Install tikontrol's own certificate authority once per device and the browser stops warning. Before trusting it, check that the SHA-256 fingerprint your device shows matches this one, which is also printed in tikontrol's log at start.</p>
    <div class="row"><a class="btn primary" href="/api/v1/ca.crt" download="tikontrol-ca.crt">Download tikontrol-ca.crt</a></div>
    {#if fp}<p class="small">CA SHA-256 <code class="fp">{fp}</code></p>{/if}
    <ul class="small">
      <li><b>macOS:</b> open the file, add it to the <i>System</i> keychain in Keychain Access, open it, and under Trust set <i>When using this certificate</i> to <i>Always Trust</i>. Or: <code>sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain tikontrol-ca.crt</code></li>
      <li><b>iPhone, iPad:</b> open the download, install the profile in Settings, then turn it on in Settings → General → About → Certificate Trust Settings.</li>
      <li><b>Windows:</b> open the file → Install Certificate → Local Machine → put it in <i>Trusted Root Certification Authorities</i>.</li>
      <li><b>Android:</b> Settings → Security → Encryption &amp; credentials → Install a certificate → CA certificate.</li>
      <li><b>Firefox</b> has its own store: Settings → Privacy &amp; Security → Certificates → View Certificates → Authorities → Import, tick "trust this CA to identify websites".</li>
    </ul>
    <p class="small muted"><b>The risk:</b> anyone who gets the CA key (<code>ca-key.pem</code> in tikontrol's data directory) could impersonate websites to devices that trust it. tikontrol limits the CA with name constraints to local names (tikontrol, localhost, *.lan, *.home.arpa, *.internal, *.local) and private IPv4 addresses, so it can't vouch for public sites like your bank. Keep the data directory private, and remove the CA from your devices if you stop using tikontrol.</p>
  </div>
{/if}

<style>
  .trust { display: grid; gap: 8px; }
  .trust p, .trust ul { margin: 0; max-width: 75ch; }
  .trust ul { padding-left: 18px; display: grid; gap: 4px; }
  .fp { word-break: break-all; }
</style>
