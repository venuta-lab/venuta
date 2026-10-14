/*
  The themes and accents on offer, and how an album's colour becomes a safe accent.
  Tokens live in src/themes.css; the reasoning in docs/research/06-themes-and-design.md.
*/

export type Style =
  | 'venuta'
  | 'green-room'
  | 'orchard'
  | 'redline'
  | 'glasshouse'
  | 'neo-tokyo'
  | 'thermoptic'
  | 'sync-ratio'
  | 'layer-zero'
  | 'bounty'
  | 'skin99'

/** The seek bar's design: Lumen's waveform, or one of the themes' own tracks. */
export type SeekBar = 'wave' | 'pulse' | 'slim' | 'red' | 'segments' | 'cursor' | 'hud' | 'stamp' | 'bevel' | 'trail'

/** The scene ThemeFx draws behind and over the app, named after the theme it was made for. */
export type Fx = 'none' | 'neo-tokyo' | 'thermoptic' | 'sync-ratio' | 'layer-zero' | 'bounty' | 'skin99'

/**
 * A theme and what it does. Code asks these, never a theme's name, so a theme made from another
 * (a theme package) behaves like the one it extends, and an unknown one like Lumen.
 */
export interface StyleInfo {
  id: Style
  name: string
  /** One line about it, and what it is after. */
  line: string
  /** Follows the light/dark switch (the others are one look). */
  modes: boolean
  /**
   * Where its accent comes from: the colour you choose (or the album's, or neutral glass), always
   * the cover's, the nearest of its colour blocks to the cover's, or its own palette.
   */
  accent: 'yours' | 'cover' | 'blocks' | 'own'
  seekBar: SeekBar
  /** The mini player as glass with the cover's light, or in the theme's own player bar. */
  miniPlayer: 'glass' | 'themed'
  /** The surface choice (glass, frosted, solid, AMOLED) applies to it. */
  surfaces: boolean
  /** The album background can be switched on behind it. */
  albumBackground: boolean
  /** An LCD spectrum beside the player's controls. */
  lcd: boolean
  /** Its own words for the light/dark choice. */
  modeNames?: { title: string; light: string; dark: string; hint: string }
  fx: Fx
}

const THEMED = { miniPlayer: 'themed', surfaces: false, lcd: false } as const

export const STYLES: StyleInfo[] = [
  {
    id: 'venuta', name: 'Lumen', line: 'Light through glass, over your album art, in the colour of what you play.',
    modes: true, accent: 'yours', seekBar: 'wave', miniPlayer: 'glass', surfaces: true, albumBackground: true, lcd: false, fx: 'none',
  },
  // The players people know (research 08): their own names here, what they are after in the line.
  {
    id: 'green-room', name: 'Green Room', line: 'Black panels, a green Play and big bold titles. After Spotify.',
    modes: false, accent: 'own', seekBar: 'pulse', ...THEMED, albumBackground: false, fx: 'none',
  },
  {
    id: 'orchard', name: 'Orchard', line: 'Quiet greys, a red-pink key and a floating player. After Apple Music.',
    modes: true, accent: 'own', seekBar: 'slim', ...THEMED, albumBackground: false, fx: 'none',
  },
  {
    id: 'redline', name: 'Redline', line: 'Pitch black, white pills and a red line. After YouTube Music.',
    modes: false, accent: 'own', seekBar: 'red', ...THEMED, albumBackground: false, fx: 'none',
  },
  {
    id: 'glasshouse', name: 'Glasshouse', line: 'Dark glass, an inset stage tinted by each cover. After Cider.',
    modes: false, accent: 'cover', seekBar: 'slim', ...THEMED, albumBackground: false, fx: 'none',
  },
  {
    id: 'neo-tokyo', name: 'Neo-Tokyo', line: 'Red light-trails through a neon night city. After Akira (1988).',
    modes: false, accent: 'own', seekBar: 'trail', ...THEMED, albumBackground: true, fx: 'neo-tokyo',
  },
  {
    id: 'thermoptic', name: 'Thermoptic', line: 'Rain-damp teal city and terminal-green data. After Ghost in the Shell.',
    modes: false, accent: 'own', seekBar: 'hud', ...THEMED, albumBackground: true, fx: 'thermoptic',
  },
  {
    id: 'sync-ratio', name: 'Sync Ratio', line: 'Emergency orange, cockpit purple, serif title cards. After Evangelion.',
    modes: false, accent: 'own', seekBar: 'segments', ...THEMED, albumBackground: true, fx: 'sync-ratio',
  },
  {
    id: 'layer-zero', name: 'Layer Zero', line: 'Blue-grey stillness and shadows of red static. After Serial Experiments Lain.',
    modes: false, accent: 'own', seekBar: 'cursor', ...THEMED, albumBackground: true, fx: 'layer-zero',
  },
  {
    id: 'bounty', name: 'Bounty Session', line: 'Ink on paper and colour blocks, cut to a horn section. After Cowboy Bebop.',
    modes: true, accent: 'blocks', seekBar: 'stamp', ...THEMED, albumBackground: false, fx: 'bounty',
    modeNames: { title: 'Session', light: 'Day', dark: 'Night', hint: 'Day is ink on paper; night is paper on ink.' },
  },
  {
    id: 'skin99', name: "Skin '99", line: 'Brushed gunmetal, bevels and a green LCD. After the skinned players of 1999.',
    modes: false, accent: 'own', seekBar: 'bevel', ...THEMED, lcd: true, albumBackground: false, fx: 'skin99',
  },
]

export const styleInfo = (id: string) => STYLES.find((s) => s.id === id) ?? STYLES[0]

/**
 * Colours to keep (One colour), designed for Lumen as seeds: a hue and how colourful. Each is
 * made into the theme's colours the same way an album's is (accentRoles), so it reads in light
 * and dark alike. The names of older swatches still find their nearest.
 */
export const ACCENTS: { name: string; seed: string }[] = [
  { name: 'Azure', seed: '#3d8bff' },
  { name: 'Iris', seed: '#6f6cf6' },
  { name: 'Orchid', seed: '#b45be6' },
  { name: 'Rose', seed: '#ec4f8c' },
  { name: 'Coral', seed: '#f0624a' },
  { name: 'Amber', seed: '#f2a33a' },
  { name: 'Citrine', seed: '#d2c23a' },
  { name: 'Jade', seed: '#2fbf7f' },
  { name: 'Lagoon', seed: '#20b5c4' },
  { name: 'Slate', seed: '#7d8ba6' },
]
const RENAMED: Record<string, string> = { Violet: 'Orchid', Ember: 'Coral', Lime: 'Citrine', Mint: 'Jade', Graphite: 'Slate' }

export const accentByName = (name: string) => ACCENTS.find((a) => a.name === (RENAMED[name] ?? name)) ?? ACCENTS[0]

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


// --- An accent's colours, solved for contrast (research 07) -----------------------------------

export type Intensity = 'subtle' | 'balanced' | 'vivid'

/** What an accent becomes on a theme: the colour, type on it, and text in it. */
export interface AccentRoles {
  /** Fills: the main button, progress, the playing row, selection. 3:1 against the page. */
  accent: string
  /** Type on the accent: white or near-black, whichever reads (4.5:1 or better). */
  onAccent: string
  /** Text in the accent's colour on the page: 4.5:1. */
  ink: string
}

/** The pages accents sit on (Lumen's glass is a little lighter or darker; these are the worst case). */
const SURFACE = { dark: '#1d1e23', light: '#f3f3f5' }
/** How colourful an accent may be, per intensity: at least, at most (OKLCH chroma). */
const CHROMA: Record<Intensity, [number, number]> = { subtle: [0.05, 0.09], balanced: [0.095, 0.15], vivid: [0.12, 0.22] }

function luminance(hex: string): number {
  const rgb = hexToRgb(hex)
  if (!rgb) return 0
  const [r, g, b] = rgb.map(toLinear)
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

/** WCAG 2 contrast ratio of two colours (1 to 21). */
export function contrast(a: string, b: string): number {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p)
  return (x + 0.05) / (y + 0.05)
}

/** The accent's colours on a dark or light theme. Greys (no real colour) give null. */
export function accentRoles(seed: string, light: boolean, intensity: Intensity = 'balanced'): AccentRoles | null {
  const lch = hexToOklch(seed)
  if (!lch || lch.c < 0.03) return null
  const [lo, hi] = CHROMA[intensity]
  const c = Math.min(Math.max(lch.c, lo), hi)
  const h = lch.h
  const surface = light ? SURFACE.light : SURFACE.dark
  // Lightness that keeps the cover's character, moved only as far as contrast needs.
  let l = light ? Math.min(Math.max(lch.l, 0.46), 0.62) : Math.min(Math.max(lch.l, 0.66), 0.84)
  let accent = oklchToHex({ l, c, h })
  for (let i = 0; i < 60 && contrast(accent, surface) < 3.2; i++) {
    l += light ? -0.01 : 0.01
    accent = oklchToHex({ l, c, h })
  }
  const onAccent = contrast(accent, '#ffffff') >= contrast(accent, '#0b0c10') ? '#ffffff' : '#0b0c10'
  let li = light ? Math.min(l, 0.54) : Math.max(l, 0.74)
  const ci = Math.min(c, 0.16)
  let ink = oklchToHex({ l: li, c: ci, h })
  for (let i = 0; i < 60 && contrast(ink, surface) < 4.6; i++) {
    li += light ? -0.01 : 0.01
    ink = oklchToHex({ l: li, c: ci, h })
  }
  return { accent, onAccent, ink }
}

/** No colour at all: white controls on dark, graphite on light (Neutral). */
export const neutralRoles = (light: boolean): AccentRoles =>
  light ? { accent: '#26272c', onAccent: '#ffffff', ink: '#1d1d1f' } : { accent: '#e6e7ea', onAccent: '#0b0c10', ink: '#f2f3f5' }

/**
 * An album's colour as a tint for a page (a playlist or album header, the stage): deep enough
 * that white type reads over it, coloured enough to be its cover's. Greys stay a neutral grey.
 */
export function artTone(hex: string): string {
  const lch = hexToOklch(hex)
  if (!lch) return '#535353'
  if (lch.c < 0.03) return oklchToHex({ l: 0.42, c: 0, h: 0 })
  return oklchToHex({ l: Math.min(Math.max(lch.l, 0.4), 0.5), c: Math.min(Math.max(lch.c, 0.07), 0.15), h: lch.h })
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
