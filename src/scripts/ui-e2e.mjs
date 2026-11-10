// End-to-end browser test of tikontrol-only features that need no router: client rename, icon and notes, and the
// floor-plan coverage map (upload, keyboard move, save, reload in a fresh browser, remove). Works on the demo server:
//   (cd src/server && TIK_DEMO=../ui/src/lib/sample.json TIK_LISTEN=off TIK_HTTP=127.0.0.1:8099 cargo run)
//   SETUP_PW_FILE=admin.pw node src/scripts/ui-e2e.mjs http://127.0.0.1:8099
// It changes tikontrol's own prefs and floor plan (not the router); run it on a test instance.
import { createRequire } from 'node:module'
import fs from 'node:fs'
import assert from 'node:assert/strict'
const { chromium } = createRequire(import.meta.url)(process.env.PLAYWRIGHT || 'playwright')
const BASE = process.argv[2] || 'http://127.0.0.1:8099'
const PW = fs.readFileSync(process.env.SETUP_PW_FILE, 'utf8').trim()
async function signin(b) { const p = await (await b.newContext({ viewport: { width: 1300, height: 900 }, ignoreHTTPSErrors: true })).newPage();
  p.on('pageerror', e => console.log('pageerror:', e.message));
  await p.goto(BASE); await p.getByLabel('Password').waitFor();
  if (await p.getByLabel('Username').count()) await p.getByLabel('Username').fill('admin');
  await p.getByLabel('Password').fill(PW); await p.getByRole('button', { name: /^(Connect|Sign in)$/ }).click();
  await p.waitForSelector('nav, .rail'); return p }
let href;
(async () => { const b = await chromium.launch();
  let p = await signin(b)
  // rename, icon, notes
  await p.goto(BASE + '/#/clients'); await p.waitForTimeout(1500)
  href = await p.locator('a.name').first().getAttribute('href'); console.log('client', href ? 'found' : 'none')
  await p.goto(BASE + '/' + href); await p.waitForTimeout(1200)
  await p.getByTitle('Rename').click(); await p.keyboard.press('Control+A'); await p.keyboard.type('Test NAS'); await p.keyboard.press('Enter')
  await p.getByRole('tab', { name: /Settings/ }).click().catch(async () => { await p.getByText('Settings', { exact: true }).last().click() })
  await p.getByRole('button', { name: 'server', exact: true }).click()
  await p.getByPlaceholder('e.g. Living room TV').fill('rack shelf 2')
  await p.waitForTimeout(1500) // prefs are saved 600 ms after the last change
  // floor plan
  await p.goto(BASE + '/#/devices/radios'); await p.waitForTimeout(1200)
  // a simple drawn plan, made in the page so the test needs no image file
  const png = await p.evaluate(() => { const c = document.createElement('canvas'); c.width = 900; c.height = 600; const g = c.getContext('2d'); g.fillStyle = '#fff'; g.fillRect(0, 0, 900, 600); g.lineWidth = 6; g.strokeRect(20, 20, 860, 560); g.beginPath(); g.moveTo(450, 20); g.lineTo(450, 580); g.stroke(); return c.toDataURL('image/png').split(',')[1] })
  await p.locator('input[type=file]').setInputFiles({ name: 'plan.png', mimeType: 'image/png', buffer: Buffer.from(png, 'base64') }); await p.waitForTimeout(800)
  const pin = p.locator('button.ap').first(); await pin.focus(); for (let i = 0; i < 5; i++) await p.keyboard.press('ArrowRight')
  await p.getByRole('button', { name: 'Save' }).click(); await p.waitForTimeout(800)
  assert.equal(await p.getByRole('status').filter({ hasText: 'Saved' }).count(), 1, 'plan saved')
  await p.context().close()
  // a fresh browser: everything comes back from the server
  p = await signin(b)
  await p.goto(BASE + '/#/clients'); await p.waitForTimeout(1500)
  assert.ok(await p.getByText('Test NAS').count() > 0, 'alias persisted on the server')
  await p.goto(BASE + '/' + href); await p.waitForTimeout(1200)
  assert.ok(await p.getByText('rack shelf 2').count() > 0, 'note persisted')
  await p.goto(BASE + '/#/devices/radios'); await p.waitForTimeout(1500)
  assert.equal(await p.locator('img[alt="Floor plan"]').count(), 1, 'plan persisted')
  const left = await p.locator('button.ap').first().evaluate(e => e.style.left)
  assert.equal(left, '55%', 'pin moved with the keyboard and saved: ' + left)
  await p.getByRole('button', { name: 'Remove plan' }).click(); await p.waitForTimeout(500)
  await p.reload(); await p.waitForTimeout(1500)
  assert.equal(await p.locator('img[alt="Floor plan"]').count(), 0, 'plan removed')
  console.log('E2E OK'); await b.close() })().catch(e => { console.error('FAIL', e.message); process.exit(1) })
