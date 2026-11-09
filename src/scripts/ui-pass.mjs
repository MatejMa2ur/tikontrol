// Headless browser pass over every page and settings tab of a running tikontrol (read-only: it clicks no buttons
// that change anything). Reports console errors, failed requests, horizontal overflow, junk text (undefined/NaN)
// and focus visibility at desktop and mobile widths in both themes. Screenshots go to $OUT (default /tmp/tik-ui).
//   PLAYWRIGHT=/path/to/node_modules/playwright [CHROME=/path/to/chrome] [SETUP_PW_FILE=admin.pw] [ONLY=regex] \
//     node src/scripts/ui-pass.mjs https://127.0.0.1:9643
// Playwright is not a project dependency: install it anywhere (npm i --prefix /tmp/pw playwright) and point at it.
// With SETUP_PW_FILE it first runs one-step setup (or signs in) in the browser as `admin` (the password is read, never printed).
import { createRequire } from 'node:module'
import { readFileSync, mkdirSync } from 'node:fs'
const { chromium } = createRequire(import.meta.url)(process.env.PLAYWRIGHT || 'playwright')
const BASE = process.argv[2] || 'https://127.0.0.1:9643'
const OUT = process.env.OUT || '/tmp/tik-ui'
mkdirSync(OUT, { recursive: true })

const ROUTES = ['dashboard', 'topology', 'devices/list', 'devices/ports', 'devices/radios', 'devices/updates', 'devices/supervisor',
  'clients', 'insights/apps', 'insights/clients', 'insights/domains', 'insights/flows', 'insights/countries',
  'events/events', 'events/alarms', 'events/channels', 'hotspot/portal', 'hotspot/vouchers', 'hotspot/guests', 'hotspot/payments',
  ...['wifi', 'networks', 'internet', 'vpn', 'security', 'routing', 'profiles', 'control', 'system'].map((s) => `settings/${s}`)]
const JUNK = /\bundefined\b|\bNaN\b|\[object Object\]|Infinity/

const browser = await chromium.launch(process.env.CHROME ? { executablePath: process.env.CHROME } : {})
const ctx = await browser.newContext({ ignoreHTTPSErrors: true })
const page = await ctx.newPage()
let problems = []
let where = 'start'
page.on('console', (m) => m.type() === 'error' && !/\b(401|503)\b/.test(m.text()) && problems.push(`${where}: console: ${m.text().slice(0, 200)}`))
page.on('pageerror', (e) => problems.push(`${where}: pageerror: ${e.message.slice(0, 200)}`))
// a 401 from /me is how the app learns it must sign in; a 503 snapshot right after start is the first poll running
page.on('response', (r) => r.status() >= 400 && !(r.status() === 401 && r.url().endsWith('/api/v1/me')) && !(r.status() === 503 && where === 'setup') && problems.push(`${where}: HTTP ${r.status()} ${r.request().method()} ${r.url().replace(BASE, '')}`))
page.on('requestfailed', (r) => !r.url().includes('/live') && problems.push(`${where}: failed ${r.url().replace(BASE, '')} ${r.failure()?.errorText}`))

if (process.env.SETUP_PW_FILE) {
  where = 'setup'
  await page.goto(BASE)
  await page.getByLabel('Password').waitFor({ timeout: 60000 })
  await page.getByLabel('Password').fill(readFileSync(process.env.SETUP_PW_FILE, 'utf8').trim())
  await page.screenshot({ path: `${OUT}/setup.png` })
  if (await page.getByLabel('Username').count()) await page.getByLabel('Username').fill('admin')
  await page.getByRole('button', { name: /^(Connect|Sign in)$/ }).click()
  await page.waitForSelector('nav, .rail', { timeout: 180000 })
}

for (const [w, h, size] of [[1440, 900, 'desktop'], [390, 844, 'mobile']]) {
  await page.setViewportSize({ width: w, height: h })
  for (const theme of ['dark', 'light']) {
    for (const r of ROUTES) {
      where = `${size}/${theme}/${r}`
      if (process.env.ONLY && !new RegExp(process.env.ONLY).test(where)) continue
      await page.goto(`${BASE}/?theme=${theme}#/${r}`)
      await page.waitForSelector('main', { timeout: 30000 })
      await page.waitForTimeout(1200)
      const t = await page.evaluate(() => {
        const text = document.querySelector('main')?.innerText ?? ''
        const over = [...document.querySelectorAll('main *')].filter((e) => e.getBoundingClientRect().right > innerWidth + 1 && getComputedStyle(e).position !== 'fixed' && !e.closest('[class*=scroll], .table-wrap, .tablewrap, pre, svg, .tabs'))
        return { text, docOver: document.documentElement.scrollWidth - innerWidth, over: over.slice(0, 3).map((e) => `${e.tagName.toLowerCase()}.${[...e.classList].join('.')}`) }
      })
      if (t.text.trim().length < 20) problems.push(`${where}: main is (nearly) empty`)
      const junk = t.text.match(JUNK)
      if (junk) problems.push(`${where}: junk text "${t.text.slice(Math.max(0, junk.index - 40), junk.index + 30).replace(/\s+/g, ' ')}"`)
      if (t.docOver > 1) problems.push(`${where}: page scrolls sideways by ${t.docOver}px (${t.over.join(', ')})`)
      if (theme === 'dark') await page.screenshot({ path: `${OUT}/${size}-${r.replace('/', '-')}.png`, fullPage: true })
    }
  }
}

// drawers: this router's device detail (opened by keyboard) and the first DHCP lease's client detail
for (const [name, go] of [
  ['device-drawer', async () => { await page.goto(`${BASE}/#/devices/list`); await page.waitForTimeout(1200); await page.locator('main table button.plain').first().focus(); await page.keyboard.press('Enter') }],
  ['client-drawer', async () => { const mac = await page.evaluate(async () => (await (await fetch('/api/v1/snapshot')).json()).leases?.[0]?.['mac-address']); await page.goto(`${BASE}/#/clients/${mac}`) }],
]) {
  where = name
  await page.setViewportSize({ width: 1440, height: 900 })
  await go()
  await page.waitForTimeout(1500)
  const text = await page.evaluate(() => document.querySelector('[role=dialog], aside, .drawer')?.innerText ?? '')
  if (text.trim().length < 20) problems.push(`${where}: did not open`)
  const junk = text.match(JUNK)
  if (junk) problems.push(`${where}: junk text "${text.slice(Math.max(0, junk.index - 40), junk.index + 30).replace(/\s+/g, ' ')}"`)
  await page.screenshot({ path: `${OUT}/${name}.png` })
}

// read-only: a change opens Review & apply with the reason and a disabled Apply (the router refuses it anyway)
where = 'read-only'
if (await page.getByText('Read-only', { exact: true }).count()) {
  await page.goto(`${BASE}/#/devices/list`)
  await page.waitForTimeout(1200)
  await page.locator('main table button.plain').first().click()
  await page.getByRole('button', { name: 'Restart' }).click()
  await page.waitForTimeout(2500)
  const note = await page.locator('[role=note]').first().innerText().catch(() => '')
  const disabled = await page.getByRole('button', { name: /^Apply/ }).isDisabled().catch(() => false)
  if (!note || !disabled) problems.push(`read-only: Review & apply shows no reason (${note || 'none'}) or Apply is enabled`)
  await page.screenshot({ path: `${OUT}/read-only-review.png` })
}

// keyboard: every focus stop in the first 25 Tabs must show a visible focus style
where = 'focus'
await page.setViewportSize({ width: 1440, height: 900 })
await page.goto(`${BASE}/#/dashboard`)
await page.waitForTimeout(1500)
for (let i = 0; i < 25; i++) {
  await page.keyboard.press('Tab')
  const f = await page.evaluate(() => {
    const e = document.activeElement
    if (!e || e === document.body) return null
    const s = getComputedStyle(e)
    const shown = (s.outlineStyle !== 'none' && parseFloat(s.outlineWidth) > 0) || s.boxShadow !== 'none'
    return { shown, what: `${e.tagName.toLowerCase()} "${(e.getAttribute('aria-label') || e.textContent || '').trim().slice(0, 30)}"` }
  })
  if (f && !f.shown) problems.push(`focus: no visible focus on ${f.what}`)
}

// global search
where = 'palette'
await page.keyboard.press('Meta+k')
await page.keyboard.type('control')
await page.waitForTimeout(300)
const hits = await page.locator('[role=dialog] button.item').count()
if (!hits) problems.push('palette: no results for "control"')

await browser.close()
problems = [...new Set(problems)]
console.log(problems.length ? problems.join('\n') : 'no problems found')
console.log(`screenshots in ${OUT}`)
process.exit(problems.length ? 1 : 0)
