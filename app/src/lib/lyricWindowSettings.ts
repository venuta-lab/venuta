// What each lyrics window keeps (the big screen's and the desktop lyrics'), and how it's read
// back: pure, so tests run it as is. lib/lyricWindows.ts keeps and applies them.
//
// Each window has one job. The big screen fills a screen, for a TV or a second monitor; the
// desktop lyrics are a strip over everything, moved, sized and locked. Both have the same three
// choices: what's behind the words (the background), the theme, and the look (lib/lyricLook.ts).

import type { LyricWindow } from './lyricLook'

/**
 * What's behind a window's words: nothing at all (Clear), see-through glass (what's behind
 * stays sharp), frosted glass (what's behind blurred), the playing cover's light, or the
 * theme's own scene.
 */
export type LyricBackground = 'clear' | 'glass' | 'frosted' | 'cover' | 'theme'
export const BACKGROUNDS: LyricBackground[] = ['clear', 'glass', 'frosted', 'cover', 'theme']

/**
 * The theme a window shows: Venuta's own (and whatever it changes to), a built-in one (`style:`
 * and its id) or one of yours (`pack:` and its id). It colours the words, the controls, and the
 * tint of glass and frost.
 */
export type LyricTheme = 'venuta' | `style:${string}` | `pack:${string}`

/** Screen pixels: x, y, width, height. The same on every screen whatever its scale, so a place
 *  kept on one monitor means the same spot when Venuta opens on another. */
export type Px = [number, number, number, number]

export interface LyricWindowSettings {
  background: LyricBackground
  theme: LyricTheme
  /** The desktop lyrics: over every other window. */
  onTop: boolean
  /** The desktop lyrics: clicks go through them, all but their lock button. */
  locked: boolean
  /** The desktop lyrics: where they were, if moved (screen pixels). */
  px: Px | null
  /** The big screen: the screen it fills, by name (null: the one Venuta is on). */
  monitor: string | null
}

/** Until chosen otherwise, both windows look like Venuta: its theme and its own scene behind
 *  the words. The desktop lyrics start on top, locked. */
export const WINDOW_DEFAULTS: Record<LyricWindow, LyricWindowSettings> = {
  stage: { background: 'theme', theme: 'venuta', onTop: false, locked: false, px: null, monitor: null },
  overlay: { background: 'theme', theme: 'venuta', onTop: true, locked: true, px: null, monitor: null },
}

export const keyOf = (w: LyricWindow) => `lyric-window-${w}`
/** The desktop lyrics' lock from before. (Places kept before, by each screen's own scale, are
 *  left: on a screen of another scale they'd land somewhere else.) */
const OLD_LOCKED = 'desktop-lyrics-locked'

const isPx = (p: unknown): p is Px => Array.isArray(p) && p.length === 4 && p.every((n) => Number.isInteger(n)) && p[2] > 0 && p[3] > 0
export const isBackground = (b: unknown): b is LyricBackground => typeof b === 'string' && (BACKGROUNDS as string[]).includes(b)
export const isTheme = (t: unknown): t is LyricTheme => typeof t === 'string' && (t === 'venuta' || /^style:[a-z0-9-]+$/.test(t) || /^pack:.+$/.test(t))

/** A window's settings from what's kept (`get` reads a key): its own, else what it had before
 *  (the desktop lyrics' place and lock; a theme kept as the background, from when the two were
 *  one choice), else the defaults. Anything unreadable or not of its kind is left out, field by
 *  field. */
export function readWindow(w: LyricWindow, get: (key: string) => string | null): LyricWindowSettings {
  const out: LyricWindowSettings = { ...WINDOW_DEFAULTS[w] }
  const parse = (key: string): unknown => {
    try {
      const raw = get(key)
      return raw === null ? null : JSON.parse(raw)
    } catch {
      return null
    }
  }
  if (w === 'overlay' && get(OLD_LOCKED) === 'false') out.locked = false
  const own = parse(keyOf(w))
  if (own && typeof own === 'object') {
    const o = own as Record<string, unknown>
    if (isBackground(o.background)) out.background = o.background
    // A theme kept as the background: the theme's own scene, in that theme.
    else if (isTheme(o.background)) {
      out.background = 'theme'
      out.theme = o.background
    }
    if (isTheme(o.theme)) out.theme = o.theme
    for (const k of ['onTop', 'locked'] as const) if (typeof o[k] === 'boolean') out[k] = o[k] as boolean
    if (isPx(o.px)) out.px = o.px
    if (o.monitor === null || typeof o.monitor === 'string') out.monitor = o.monitor as string | null
  }
  // The big screen is never locked: it fills a screen, and Esc closes it.
  if (w === 'stage') out.locked = false
  return out
}

/** Whether a CSS colour lets what's behind show (a theme's ground that relies on the window's own
 *  blur, as Lumen's glass does): an alpha under 1, or transparent. */
export function seeThrough(colour: string): boolean {
  const c = colour.trim().toLowerCase()
  if (!c || c === 'transparent') return !!c
  const hex = /^#([0-9a-f]{4}|[0-9a-f]{8})$/.exec(c)
  if (hex) return hex[1].length === 4 ? hex[1][3] !== 'f' : hex[1].slice(6) !== 'ff'
  const alpha = /\/\s*([\d.]+)(%?)\s*\)$/.exec(c) ?? /^rgba?\([^)]*,\s*([\d.]+)(%?)\s*\)$/.exec(c)
  if (!alpha) return false
  const a = parseFloat(alpha[1]) / (alpha[2] ? 100 : 1)
  return a < 1
}
