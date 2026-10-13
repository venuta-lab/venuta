/*
  The themes and accents on offer, and how an album's colour becomes a safe accent.
  Tokens live in src/themes.css; the reasoning in docs/research/06-themes-and-design.md.
*/

export type Style = 'venuta' | 'neo-tokyo' | 'thermoptic' | 'sync-ratio' | 'layer-zero' | 'bounty' | 'skin99'

export interface StyleInfo {
  id: Style
  name: string
  /** One line about it, and what it is after. */
  line: string
  /** Follows the light/dark switch (the others are one look). */
  modes: boolean
  /** Takes its accent from the album; the retro themes keep their own palette. */
  albumAccent: boolean
}

export const STYLES: StyleInfo[] = [
  { id: 'venuta', name: 'Venuta', line: 'Glass over your album art, in the colour of what you play.', modes: true, albumAccent: true },
  { id: 'neo-tokyo', name: 'Neo-Tokyo', line: 'Red light-trails through a neon night city. After Akira (1988).', modes: false, albumAccent: false },
  { id: 'thermoptic', name: 'Thermoptic', line: 'Rain-damp teal city and terminal-green data. After Ghost in the Shell.', modes: false, albumAccent: false },
  { id: 'sync-ratio', name: 'Sync Ratio', line: 'Emergency orange, cockpit purple, serif title cards. After Evangelion.', modes: false, albumAccent: false },
  { id: 'layer-zero', name: 'Layer Zero', line: 'Blue-grey stillness and shadows of red static. After Serial Experiments Lain.', modes: false, albumAccent: false },
  { id: 'bounty', name: 'Bounty Session', line: 'Ink on paper and colour blocks, cut to a horn section. After Cowboy Bebop.', modes: true, albumAccent: false },
  { id: 'skin99', name: "Skin '99", line: 'Brushed gunmetal, bevels and a green LCD. After the skinned players of 1999.', modes: false, albumAccent: false },
]

export const styleInfo = (id: string) => STYLES.find((s) => s.id === id) ?? STYLES[0]

/** Fixed accents: a dark-theme and a light-theme version of each (research §2.3). */
export const ACCENTS: { name: string; dark: string; light: string }[] = [
  { name: 'Azure', dark: '#5aa8ff', light: '#1f66d1' },
  { name: 'Iris', dark: '#8c8cff', light: '#4b49d6' },
  { name: 'Violet', dark: '#c084fc', light: '#8b3fd9' },
  { name: 'Rose', dark: '#ff6fae', light: '#d02f7a' },
  { name: 'Ember', dark: '#ff6b4a', light: '#d9432a' },
  { name: 'Amber', dark: '#ffb547', light: '#b86e00' },
  { name: 'Lime', dark: '#b5e655', light: '#5e8c12' },
  { name: 'Mint', dark: '#3ddc97', light: '#0f8a5f' },
  { name: 'Lagoon', dark: '#3ccfe0', light: '#0a7f91' },
  { name: 'Graphite', dark: '#d6d9e0', light: '#3a3f4a' },
]

export const accentByName = (name: string) => ACCENTS.find((a) => a.name === name) ?? ACCENTS[0]

// --- OKLCH ------------------------------------------------------------------------------------

type Lch = { l: number; c: number; h: number }

const toLinear = (v: number) => (v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4))
const toGamma = (v: number) => (v <= 0.0031308 ? 12.92 * v : 1.055 * Math.pow(v, 1 / 2.4) - 0.055)

function hexToRgb(hex: string): [number, number, number] | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim())
  if (!m) return null
  const n = parseInt(m[1], 16)
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => v / 255) as [number, number, number]
}

export function hexToOklch(hex: string): Lch | null {
  const rgb = hexToRgb(hex)
  if (!rgb) return null
  const [r, g, b] = rgb.map(toLinear)
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s
  return { l: L, c: Math.hypot(A, B), h: ((Math.atan2(B, A) * 180) / Math.PI + 360) % 360 }
}

function oklchToRgb({ l: L, c, h }: Lch): [number, number, number] {
  const A = c * Math.cos((h * Math.PI) / 180)
  const B = c * Math.sin((h * Math.PI) / 180)
  const l = (L + 0.3963377774 * A + 0.2158037573 * B) ** 3
  const m = (L - 0.1055613458 * A - 0.0638541728 * B) ** 3
  const s = (L - 0.0894841775 * A - 1.291485548 * B) ** 3
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ]
}

const inGamut = (rgb: number[]) => rgb.every((v) => v >= -0.0005 && v <= 1.0005)

export function oklchToHex(lch: Lch): string {
  // Into sRGB by lowering chroma only (keeps lightness and hue).
  let c = lch.c
  let rgb = oklchToRgb({ ...lch, c })
  while (!inGamut(rgb) && c > 0) {
    c = Math.max(0, c - 0.005)
    rgb = oklchToRgb({ ...lch, c })
  }
  return (
    '#' +
    rgb
      .map((v) =>
        Math.round(Math.min(1, Math.max(0, toGamma(Math.min(1, Math.max(0, v))))) * 255)
          .toString(16)
          .padStart(2, '0'),
      )
      .join('')
  )
}

/**
 * An album's colour, made into an accent that reads on the theme (research §2.4): lightness and
 * chroma clamped in OKLCH, hue kept. Greys (no real colour) give null: keep the fixed accent.
 */
export function tuneAccent(hex: string, light: boolean): string | null {
  const lch = hexToOklch(hex)
  if (!lch) return null
  if (lch.c < 0.045) return null
  let { l, c } = lch
  const h = lch.h
  if (light) {
    const top = h >= 85 && h <= 125 ? 0.54 : 0.6
    l = Math.min(Math.max(l, 0.48), top)
    c = Math.min(Math.max(c, 0.09), 0.2)
  } else {
    const floor = h >= 250 && h <= 300 ? 0.72 : 0.7
    l = Math.min(Math.max(l, floor), 0.84)
    c = Math.min(Math.max(c, 0.09), 0.19)
  }
  return oklchToHex({ l, c, h })
}

/** Distance between two colours in OKLab (to skip changes too small to notice). */
export function colourDistance(a: string, b: string): number {
  const x = hexToOklch(a)
  const y = hexToOklch(b)
  if (!x || !y) return 1
  const ax = x.c * Math.cos((x.h * Math.PI) / 180)
  const ay = x.c * Math.sin((x.h * Math.PI) / 180)
  const bx = y.c * Math.cos((y.h * Math.PI) / 180)
  const by = y.c * Math.sin((y.h * Math.PI) / 180)
  return Math.hypot(x.l - y.l, ax - bx, ay - by)
}

/** Bounty Session's blocks: an album's hue snaps to the nearest. */
export const BLOCKS = ['#e85a8c', '#2f6db5', '#f2c230', '#3fa66a']
export function nearestBlock(hex: string): string {
  const lch = hexToOklch(hex)
  if (!lch || lch.c < 0.045) return BLOCKS[1]
  let best = BLOCKS[1]
  let bestD = Infinity
  for (const b of BLOCKS) {
    const bh = hexToOklch(b)!.h
    const d = Math.min(Math.abs(bh - lch.h), 360 - Math.abs(bh - lch.h))
    if (d < bestD) {
      bestD = d
      best = b
    }
  }
  return best
}
