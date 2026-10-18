// What each lyrics window keeps (the big screen's and the desktop lyrics'), and how it's read
// back: pure, so tests run it as is. lib/lyricWindows.ts keeps and applies them.

import type { LyricWindow } from './lyricLook'

/**
 * What's behind a window's words: nothing at all (Clear: only the lyrics on screen), the
 * playing cover's light, Venuta's own theme (and whatever it changes to), or one theme: a
 * built-in one (`style:` and its id) or one of yours (`pack:` and its id).
 */
export type LyricBackground = 'clear' | 'cover' | 'venuta' | `style:${string}` | `pack:${string}`

/** Logical pixels: x, y, width, height. */
export type Bounds = [number, number, number, number]

export interface LyricWindowSettings {
  background: LyricBackground
  /** Filling a screen, or a window of its own size. */
  full: boolean
  /** Over every other window. */
  onTop: boolean
  /** Clicks go through it, all but its lock button. */
  locked: boolean
  /** Where it was as a window, if it has been one. */
  bounds: Bounds | null
  /** The screen it fills, by name (null: the one Venuta is on). */
  monitor: string | null
}

/** As each window was before it had settings of its own: the big screen fills a screen over the
 *  cover's light; the desktop lyrics are the clear lines on top, locked. */
export const WINDOW_DEFAULTS: Record<LyricWindow, LyricWindowSettings> = {
  stage: { background: 'cover', full: true, onTop: false, locked: false, bounds: null, monitor: null },
  overlay: { background: 'clear', full: false, onTop: true, locked: true, bounds: null, monitor: null },
}

export const keyOf = (w: LyricWindow) => `lyric-window-${w}`
/** The desktop lyrics' place and lock from before (kept where they were). */
const OLD_BOUNDS = 'desktop-lyrics-bounds'
const OLD_LOCKED = 'desktop-lyrics-locked'

const isBounds = (b: unknown): b is Bounds => Array.isArray(b) && b.length === 4 && b.every((n) => typeof n === 'number' && Number.isFinite(n))
export const isBackground = (b: unknown): b is LyricBackground =>
  typeof b === 'string' && (b === 'clear' || b === 'cover' || b === 'venuta' || /^style:[a-z0-9-]+$/.test(b) || /^pack:.+$/.test(b))

/** A window's settings from what's kept (`get` reads a key): its own, else what it had before
 *  it had settings (the desktop lyrics' place and lock), else the defaults. Anything unreadable
 *  or not of its kind is left out, field by field. */
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
    for (const k of ['full', 'onTop', 'locked'] as const) if (typeof o[k] === 'boolean') out[k] = o[k] as boolean
    if (o.bounds === null || isBounds(o.bounds)) out.bounds = (o.bounds as Bounds | null) ?? out.bounds
    if (o.monitor === null || typeof o.monitor === 'string') out.monitor = o.monitor as string | null
  }
  return out
}

