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

/** Logical pixels: x, y, width, height. */
export type Bounds = [number, number, number, number]

export interface LyricWindowSettings {
  background: LyricBackground
  theme: LyricTheme
  /** The desktop lyrics: over every other window. */
  onTop: boolean
  /** The desktop lyrics: clicks go through them, all but their lock button. */
  locked: boolean
  /** The desktop lyrics: where they were, if moved. */
  bounds: Bounds | null
  /** The big screen: the screen it fills, by name (null: the one Venuta is on). */
  monitor: string | null
}

/** As each window always was: the big screen over the cover's light, the desktop lyrics the
 *  clear lines on top, locked; both in Venuta's own theme. */
export const WINDOW_DEFAULTS: Record<LyricWindow, LyricWindowSettings> = {
  stage: { background: 'cover', theme: 'venuta', onTop: false, locked: false, bounds: null, monitor: null },
  overlay: { background: 'clear', theme: 'venuta', onTop: true, locked: true, bounds: null, monitor: null },
}

export const keyOf = (w: LyricWindow) => `lyric-window-${w}`
/** The desktop lyrics' place and lock from before (kept where they were). */
const OLD_BOUNDS = 'desktop-lyrics-bounds'
const OLD_LOCKED = 'desktop-lyrics-locked'

const isBounds = (b: unknown): b is Bounds => Array.isArray(b) && b.length === 4 && b.every((n) => typeof n === 'number' && Number.isFinite(n))
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
  if (w === 'overlay') {
    const b = parse(OLD_BOUNDS)
    if (isBounds(b)) out.bounds = b
    if (get(OLD_LOCKED) === 'false') out.locked = false
  }
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
    if (o.bounds === null || isBounds(o.bounds)) out.bounds = (o.bounds as Bounds | null) ?? out.bounds
    if (o.monitor === null || typeof o.monitor === 'string') out.monitor = o.monitor as string | null
  }
  // The big screen is never locked: it fills a screen, and Esc closes it.
  if (w === 'stage') out.locked = false
  return out
}
