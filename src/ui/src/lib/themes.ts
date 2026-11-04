// Colour and design themes: the single source for every themed CSS variable. applyTheme() sets them on
// <html>, so what tests/themes.test.ts checks for contrast is exactly what ships. Plain data, no imports,
// so node can load it for the test. Colours are 6-digit hex: charts append an alpha suffix to them.

export type Mode = 'dark' | 'light'
export type Colours = {
  bg: string; panel: string; panel2: string; line: string; text: string; muted: string
  accent: string // links, focus ring, selected state: text contrast on bg/panel
  accentBg: string // filled controls (primary button, switch) with white text on top
  accent2: string; down: string; up: string; cells: string
  ok: string; warn: string; bad: string; codeBg: string
}
export type Palette = { label: string; dark: Colours; light: Colours }
export type Design = {
  label: string
  radius: string; radiusSm: string // cards; buttons and inputs
  pad: string; gap: string; cell: string // card padding, grid gap, table row padding
  cardBorder: string; cardShadow: string // border width, shadow (var(--shadow-sm) is per mode)
  font: string; iconStroke: string
}

const statusDark = { ok: '#3ccf7a', warn: '#f3b33d', bad: '#f5646a' }
const statusLight = { ok: '#107540', warn: '#8f5400', bad: '#b8262c' }

export const palettes: Record<string, Palette> = {
  blue: {
    label: 'Deep blue',
    dark: { bg: '#0f1012', panel: '#141517', panel2: '#1d1f22', line: '#2a2c30', text: '#e9eaec', muted: '#8a8d93', accent: '#4088f7', accentBg: '#2468e6', accent2: '#b26cf5', down: '#2fb4f2', up: '#b26cf5', cells: '#36383c', codeBg: '#0b0c0d', ...statusDark },
    light: { bg: '#f4f5f6', panel: '#ffffff', panel2: '#f2f3f5', line: '#e6e8eb', text: '#212327', muted: '#646970', accent: '#1766e0', accentBg: '#1766e0', accent2: '#9b5cf0', down: '#0e8acb', up: '#9b5cf0', cells: '#e3e6ea', codeBg: '#f6f8fa', ...statusLight },
  },
  teal: {
    label: 'Teal',
    dark: { bg: '#0d1214', panel: '#121a1c', panel2: '#1a2427', line: '#263437', text: '#e6eeee', muted: '#8b9a9c', accent: '#2fbfa8', accentBg: '#0e7a6b', accent2: '#6ca8f5', down: '#36c3d9', up: '#c08cf5', cells: '#2c3a3d', codeBg: '#0a0f10', ...statusDark },
    light: { bg: '#f2f6f6', panel: '#ffffff', panel2: '#eef3f3', line: '#dde6e6', text: '#1c2526', muted: '#5a696b', accent: '#0b7566', accentBg: '#0b7566', accent2: '#3d6fd0', down: '#0e7fa3', up: '#8a4fd8', cells: '#dce6e6', codeBg: '#f3f7f7', ...statusLight },
  },
  graphite: {
    label: 'Graphite',
    dark: { bg: '#111111', panel: '#181818', panel2: '#222222', line: '#303030', text: '#ececec', muted: '#9a9a9a', accent: '#9fb1c8', accentBg: '#566a87', accent2: '#c0a6e0', down: '#8cc4e0', up: '#c0a6e0', cells: '#343434', codeBg: '#0c0c0c', ...statusDark },
    light: { bg: '#f3f3f3', panel: '#ffffff', panel2: '#f0f0f0', line: '#e2e2e2', text: '#1f1f1f', muted: '#626262', accent: '#43546b', accentBg: '#43546b', accent2: '#75589a', down: '#3b7ea3', up: '#75589a', cells: '#e2e2e2', codeBg: '#f6f6f6', ...statusLight },
  },
  contrast: {
    label: 'High contrast',
    dark: { bg: '#000000', panel: '#000000', panel2: '#161616', line: '#8c8c8c', text: '#ffffff', muted: '#d4d4d4', accent: '#7ab8ff', accentBg: '#1a5fd0', accent2: '#e0a3ff', down: '#5cd6ff', up: '#e0a3ff', cells: '#3a3a3a', codeBg: '#0d0d0d', ok: '#5ce69a', warn: '#ffd257', bad: '#ff8080' },
    light: { bg: '#ffffff', panel: '#ffffff', panel2: '#efefef', line: '#6e6e6e', text: '#000000', muted: '#333333', accent: '#0040b0', accentBg: '#0040b0', accent2: '#6a1fb0', down: '#005f8f', up: '#6a1fb0', cells: '#d0d0d0', codeBg: '#f2f2f2', ok: '#00662c', warn: '#704400', bad: '#a8001c' },
  },
  // Our own palette in the spirit of today's router admin tools: slate navy chrome, a calm mid blue.
  router: {
    label: 'Router Blue',
    dark: { bg: '#18202a', panel: '#1e2833', panel2: '#27333f', line: '#34414f', text: '#e4e9ef', muted: '#97a5b4', accent: '#62a6ee', accentBg: '#3375c0', accent2: '#a993ea', down: '#4cb8e8', up: '#b494f0', cells: '#33414f', codeBg: '#141b23', ...statusDark, bad: '#ff7b80' },
    light: { bg: '#e9edf1', panel: '#ffffff', panel2: '#f1f4f7', line: '#d5dce3', text: '#1d2833', muted: '#526070', accent: '#1d5fa3', accentBg: '#1d5fa3', accent2: '#6e4fc4', down: '#1478b3', up: '#7d4fd0', cells: '#dbe2e9', codeBg: '#f3f6f9', ...statusLight },
  },
}

const SANS = "Inter, ui-sans-serif, system-ui, -apple-system, 'Segoe UI', sans-serif"
export const designs: Record<string, Design> = {
  default: { label: 'Standard', radius: '8px', radiusSm: '7px', pad: '16px', gap: '14px', cell: '9px', cardBorder: '1px', cardShadow: 'none', font: SANS, iconStroke: '1.8' },
  compact: { label: 'Compact', radius: '6px', radiusSm: '5px', pad: '12px', gap: '10px', cell: '5px', cardBorder: '1px', cardShadow: 'none', font: SANS, iconStroke: '1.8' },
  soft: { label: 'Soft', radius: '14px', radiusSm: '10px', pad: '18px', gap: '16px', cell: '10px', cardBorder: '0px', cardShadow: 'var(--shadow-sm)', font: "ui-rounded, system-ui, -apple-system, 'Segoe UI', sans-serif", iconStroke: '1.5' },
  sharp: { label: 'Sharp', radius: '2px', radiusSm: '2px', pad: '14px', gap: '12px', cell: '7px', cardBorder: '1px', cardShadow: 'none', font: "'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif", iconStroke: '2.2' },
}

export const DEFAULT_PALETTE = 'blue'
export const DEFAULT_DESIGN = 'default'

// accentBg -> --accent-bg, panel2 -> --panel2
const cssVar = (k: string) => '--' + k.replace(/[A-Z]/g, (c) => '-' + c.toLowerCase())

/** Set the theme's variables on an element (normally <html>). Unknown names fall back to the defaults. */
export function applyTheme(el: HTMLElement, mode: Mode, palette: string, design: string) {
  const m: Mode = mode === 'light' ? 'light' : 'dark'
  const pal = Object.hasOwn(palettes, palette) ? palette : DEFAULT_PALETTE
  const des = Object.hasOwn(designs, design) ? design : DEFAULT_DESIGN
  for (const [k, v] of Object.entries({ ...palettes[pal][m], ...designs[des] })) if (k !== 'label') el.style.setProperty(cssVar(k), v)
  Object.assign(el.dataset, { theme: m, palette: pal, design: des })
}
