// The big screen (lyrics filling a screen, for a TV or a second monitor) and the desktop lyrics
// (the lines alone, in a strip over everything, moved, sized and locked), each its own window
// with its own settings, kept on this computer: what's behind the words, the theme, and (the
// desktop lyrics) where they are, on top or not, locked or not.

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'
import { inApp } from './engine'
import type { Ground, LyricWindow } from './lyricLook'
import { keyOf, readWindow, type LyricBackground, type LyricTheme, type LyricWindowSettings } from './lyricWindowSettings'
import { usePacks } from './packs'
import { resolvedTheme, themeInfo, usePrefs, venutaPrefs } from './prefs'
import { STYLES, type Style } from './themes'

export type { LyricWindow } from './lyricLook'
export { BACKGROUNDS, WINDOW_DEFAULTS, isBackground, isTheme, readWindow, type Bounds, type LyricBackground, type LyricTheme, type LyricWindowSettings } from './lyricWindowSettings'

export interface Monitor {
  name: string
  width: number
  height: number
  primary: boolean
  /** The screen Venuta's window is on. */
  current: boolean
}

export const monitors = () => (inApp() ? invoke<Monitor[]>('monitors') : Promise.resolve([] as Monitor[]))

const stored = (key: string) => {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

const current: Record<LyricWindow, LyricWindowSettings> = { stage: readWindow('stage', stored), overlay: readWindow('overlay', stored) }
const listeners = new Set<() => void>()

export const windowSettings = (w: LyricWindow) => current[w]

/** Change a window's settings (kept; every window hears it). The window itself is changed by
 *  `setLyricWindow`; this only keeps them. */
export function keepWindowSettings(w: LyricWindow, change: Partial<LyricWindowSettings>) {
  current[w] = { ...current[w], ...change }
  try {
    localStorage.setItem(keyOf(w), JSON.stringify(current[w]))
  } catch {
    // Not kept: it applies until the window closes.
  }
  listeners.forEach((l) => l())
}

// Another window changed them.
if (typeof window !== 'undefined')
  window.addEventListener('storage', (e) => {
    const w = (['stage', 'overlay'] as const).find((x) => e.key === keyOf(x))
    if (!w) return
    current[w] = readWindow(w, stored)
    listeners.forEach((l) => l())
  })

export function useLyricWindow(w: LyricWindow): LyricWindowSettings {
  const [, tick] = useState(0)
  useEffect(() => {
    const l = () => tick((n) => n + 1)
    listeners.add(l)
    return () => {
      listeners.delete(l)
    }
  }, [])
  return current[w]
}

/** The theme shown over Venuta's own (none: Venuta's). */
export function themeOf(t: LyricTheme): { style: Style } | { pack: string } | null {
  if (t.startsWith('style:')) return { style: t.slice('style:'.length) as Style }
  if (t.startsWith('pack:')) return { pack: t.slice('pack:'.length) }
  return null
}

/** A theme's light or dark (Venuta's mode choice decides, as in the app). */
export function modeOf(t: LyricTheme): Ground {
  const o = themeOf(t)
  const p = venutaPrefs()
  return resolvedTheme(o === null ? p : 'pack' in o ? { ...p, pack: o.pack } : { ...p, style: o.style, pack: '' })
}

/** Whether what's behind the words is light: glass, frost and the theme's scene in a light
 *  theme. Clear (anything may be behind) and the cover's light are read as dark: the words
 *  carry their own shadow. */
export function groundOf(s: Pick<LyricWindowSettings, 'background' | 'theme'>): Ground {
  return s.background === 'clear' || s.background === 'cover' ? 'dark' : modeOf(s.theme)
}

// --- Showing them --------------------------------------------------------------------------

/** Heard in this window when a lyrics window comes or goes, so its buttons light up at once. */
const SHOWN = 'venuta:lyrics-window-shown'
const told = <T,>(p: Promise<T>) => p.finally(() => window.dispatchEvent(new Event(SHOWN)))

// The big screen fills a screen; the desktop lyrics are a window of their own.
const placeOf = (w: LyricWindow, s: LyricWindowSettings) => ({ bounds: s.bounds, full: w === 'stage', onTop: s.onTop, locked: s.locked, monitor: s.monitor })

/** Show a lyrics window as it was left (or bring it forward). */
export const openLyricWindow = (w: LyricWindow) => told(invoke<void>('lyrics_window', { kind: w, open: true, place: placeOf(w, current[w]) }))
export const closeLyricWindow = (w: LyricWindow) => told(invoke<void>('lyrics_window', { kind: w, open: false, place: null }))
export const lyricWindowOpen = (w: LyricWindow) => (inApp() ? invoke<boolean>('lyrics_window_open', { kind: w }) : Promise.resolve(false))

/** Change a window's settings and, when it's showing, the window: on top or not, locked or not
 *  (the rest its page draws). */
export async function setLyricWindow(w: LyricWindow, change: Partial<LyricWindowSettings>) {
  keepWindowSettings(w, change)
  if (!inApp() || (change.onTop === undefined && change.locked === undefined)) return
  await invoke<void>('lyrics_window_set', {
    kind: w,
    full: null,
    onTop: change.onTop ?? null,
    locked: change.locked ?? null,
    bounds: null,
  })
}

/** Lyrics filling a screen (the one Venuta is on, unless another is named). Esc closes it. */
export async function openBigScreen(monitor?: string) {
  keepWindowSettings('stage', { monitor: monitor ?? null })
  await openLyricWindow('stage')
}
export const closeBigScreen = () => closeLyricWindow('stage')

export const openDesktopLyrics = () => openLyricWindow('overlay')
export const closeDesktopLyrics = () => closeLyricWindow('overlay')
export const desktopLyricsOpen = () => lyricWindowOpen('overlay')

/**
 * Whether a lyrics window is showing: asked when this mounts, after a change here and when the
 * window comes back to the front, and told by the engine whenever one comes or goes (the tray,
 * the shortcut and the windows' own Close change it too).
 */
export function useLyricWindowShown(w: LyricWindow): boolean {
  const [shown, setShown] = useState(false)
  useEffect(() => {
    let live = true
    const ask = () => void lyricWindowOpen(w).then((v) => live && setShown(v), () => {})
    ask()
    const heard = inApp()
      ? listen<{ kind: LyricWindow; open: boolean }>('venuta://lyrics-window', (e) => live && e.payload.kind === w && setShown(e.payload.open))
      : null
    window.addEventListener(SHOWN, ask)
    window.addEventListener('focus', ask)
    return () => {
      live = false
      void heard?.then((off) => off())
      window.removeEventListener(SHOWN, ask)
      window.removeEventListener('focus', ask)
    }
  }, [w])
  return shown
}

export const useDesktopLyricsShown = () => useLyricWindowShown('overlay')

export async function toggleDesktopLyrics() {
  if (await desktopLyricsOpen()) await closeDesktopLyrics()
  else await openDesktopLyrics()
}

/** Locked: clicks pass through to the windows under it (all but its lock button). Unlocked:
 *  drag it, size it, change it. */
export const lockDesktopLyrics = (lock: boolean) => setLyricWindow('overlay', { locked: lock })

/** What's behind the words, in words. */
export const BACKGROUND_NAMES: Record<LyricBackground, { name: string; line: string }> = {
  clear: { name: 'Clear', line: 'Nothing but the words' },
  glass: { name: 'Glass', line: 'See-through: what is behind stays sharp' },
  frosted: { name: 'Frosted', line: 'What is behind, blurred' },
  cover: { name: 'Cover', line: "The playing cover's light" },
  theme: { name: 'Theme', line: "The theme's own scene" },
}

/** Every theme a window can show: Venuta's own (the default, following it), each built-in
 *  theme, and each theme of yours. */
export function useThemes(): { id: LyricTheme; name: string; line: string }[] {
  const packs = usePacks()
  // Listened to, so the name follows when Venuta changes.
  usePrefs()
  return [
    { id: 'venuta', name: 'Same as Venuta', line: `Follows Venuta (now ${themeInfo(venutaPrefs()).name})` },
    ...STYLES.map((t) => ({ id: `style:${t.id}` as LyricTheme, name: t.name, line: t.line })),
    ...packs.filter((i) => i.pack).map((i) => ({ id: `pack:${i.id}` as LyricTheme, name: i.pack!.name, line: 'A theme of yours' })),
  ]
}
