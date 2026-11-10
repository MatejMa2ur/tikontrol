// Run from the repo root: node --test tests/themes.test.ts
// WCAG 2.2 AA: 4.5:1 for text (1.4.3), 3:1 for UI components and graphics (1.4.11), for every palette in both modes.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { cleanTweaks, designs, designVars, palettes, type Colours } from '../src/ui/src/lib/themes.ts'

const rgb = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16))
const lum = (hex: string) => {
  const [r, g, b] = rgb(hex).map((v) => { const c = v / 255; return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4 })
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}
export const contrast = (a: string, b: string) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05) }
// color-mix(in srgb, c p%, base): pills draw status text on a 14% tint of their own colour
const mix = (c: string, base: string, p: number) => '#' + rgb(c).map((v, i) => Math.round(v * p + rgb(base)[i] * (1 - p)).toString(16).padStart(2, '0')).join('')

test('contrast helper matches known values', () => {
  assert.equal(contrast('#000000', '#ffffff').toFixed(1), '21.0')
  assert.equal(contrast('#777777', '#ffffff').toFixed(2), '4.48')
})

for (const [name, p] of Object.entries(palettes)) for (const mode of ['dark', 'light'] as const) {
  const c: Colours = p[mode]
  const check = (fg: keyof Colours, bg: keyof Colours | string, min: number) => {
    const back = bg in c ? c[bg as keyof Colours] : bg
    const r = contrast(c[fg], back)
    assert.ok(r >= min, `${name}/${mode}: ${fg} on ${bg} is ${r.toFixed(2)}:1, needs ${min}:1`)
  }
  test(`${name} ${mode}: colours are 6-digit hex`, () => {
    for (const [k, v] of Object.entries(c)) assert.match(v, /^#[0-9a-f]{6}$/, `${name}/${mode}.${k}`)
  })
  test(`${name} ${mode}: text, muted and accent text meet 4.5:1`, () => {
    for (const fg of ['text', 'muted', 'accent'] as const) for (const bg of ['bg', 'panel', 'panel2'] as const) check(fg, bg, 4.5)
  })
  test(`${name} ${mode}: white on filled accent meets 4.5:1 and the fill stands out 3:1`, () => {
    check('accentBg', '#ffffff', 4.5)
    for (const bg of ['bg', 'panel'] as const) check('accentBg', bg, 3)
    for (const bg of ['bg', 'panel', 'panel2'] as const) check('accent', bg, 3) // focus ring, selected borders
  })
  test(`${name} ${mode}: status text meets 4.5:1, also on its pill tint`, () => {
    for (const s of ['ok', 'warn', 'bad'] as const) {
      check(s, 'panel', 4.5)
      check(s, mix(c[s], c.panel, 0.14), 4.5)
    }
  })
  test(`${name} ${mode}: chart lines meet 3:1 on panels`, () => {
    for (const s of ['down', 'up', 'accent2'] as const) check(s, 'panel', 3)
  })
}

test('every palette defines the same variables in both modes', () => {
  const keys = Object.keys(palettes.blue.dark).sort()
  for (const [name, p] of Object.entries(palettes)) for (const mode of ['dark', 'light'] as const) assert.deepEqual(Object.keys(p[mode]).sort(), keys, `${name}/${mode}`)
})

test('designs define every shape variable with safe CSS values', () => {
  const keys = Object.keys(designs.default).sort()
  for (const [name, d] of Object.entries(designs)) {
    assert.deepEqual(Object.keys(d).sort(), keys, name)
    for (const k of ['radius', 'radiusSm', 'pad', 'gap', 'cell', 'cardBorder'] as const) assert.match(d[k], /^\d+px$/, `${name}.${k}`)
    assert.match(d.iconStroke, /^\d(\.\d)?$/, `${name}.iconStroke`)
  }
})

test('design tweaks override only their own variables and ignore junk from storage', () => {
  assert.deepEqual(cleanTweaks({ corners: 'round', density: 'huge', font: 1, evil: 'x' }), { corners: 'round' })
  assert.deepEqual(cleanTweaks('nonsense'), {})
  const v = designVars('classic', { corners: 'round', icons: 'bold' })
  assert.equal(v.radius, '16px')
  assert.equal(v.iconStroke, '2.4')
  assert.equal(v.pad, designs.classic.pad, 'untouched parts keep the design')
  assert.deepEqual(designVars('no-such-design'), designs.default)
})
