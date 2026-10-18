// The big screen (lyrics on a whole screen, for a TV or a second monitor, or in a window) and
// the desktop lyrics (the lines alone, floating over everything), each its own window. Both are
// the same kind of window, each with its own settings, kept on this computer: what's behind the
// words, full screen or a window and where, on top or not, locked (clicks go through) or not.

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'
import { inApp } from './engine'
import type { Ground, LyricWindow } from './lyricLook'
import { keyOf, readWindow, type LyricBackground, type LyricWindowSettings } from './lyricWindowSettings'
import { usePacks } from './packs'
import { getPrefs, resolvedTheme, themeInfo, usePrefs } from './prefs'
import { STYLES, type Style } from './themes'

export type { LyricWindow } from './lyricLook'
export { WINDOW_DEFAULTS, isBackground, readWindow, type Bounds, type LyricBackground, type LyricWindowSettings } from './lyricWindowSettings'

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

/** The theme a background shows over Venuta's own (none: Venuta's, or no theme at all). */
export function themeOf(bg: LyricBackground): { style: Style } | { pack: string } | null {
  if (bg.startsWith('style:')) return { style: bg.slice('style:'.length) as Style }
  if (bg.startsWith('pack:')) return { pack: bg.slice('pack:'.length) }
  return null
}

/** Whether a background is light behind the words: a theme in its light mode (Venuta's mode
 *  choice decides, as in the app). Clear and the cover's light are always dark. */
export function groundOf(bg: LyricBackground): Ground {
  if (bg === 'clear' || bg === 'cover') return 'dark'
  const t = themeOf(bg)
  const p = getPrefs()
  const shown = t === null ? p : 'pack' in t ? { ...p, pack: t.pack } : { ...p, style: t.style, pack: '' }
  return resolvedTheme(shown)
}

// --- Showing them --------------------------------------------------------------------------

/** Heard in this window when a lyrics window comes or goes, so its buttons light up at once. */
const SHOWN = 'venuta:lyrics-window-shown'
const told = <T,>(p: Promise<T>) => p.finally(() => window.dispatchEvent(new Event(SHOWN)))

const placeOf = (s: LyricWindowSettings) => ({ bounds: s.bounds, full: s.full, onTop: s.onTop, locked: s.locked, monitor: s.monitor })

/** Show a lyrics window as it was left (or bring it forward). */
export const openLyricWindow = (w: LyricWindow) => told(invoke<void>('lyrics_window', { kind: w, open: true, place: placeOf(current[w]) }))
export const closeLyricWindow = (w: LyricWindow) => told(invoke<void>('lyrics_window', { kind: w, open: false, place: null }))
export const lyricWindowOpen = (w: LyricWindow) => (inApp() ? invoke<boolean>('lyrics_window_open', { kind: w }) : Promise.resolve(false))

/** Change a window's settings and, when it's showing, the window: full screen or back to its
 *  place, on top or not, locked or not. */
export async function setLyricWindow(w: LyricWindow, change: Partial<LyricWindowSettings>) {
  keepWindowSettings(w, change)
  const s = current[w]
  if (!inApp()) return
  await invoke<void>('lyrics_window_set', {
    kind: w,
    full: change.full ?? null,
    onTop: change.onTop ?? null,
    locked: change.locked ?? null,
    bounds: s.bounds,
  })
}

/** Lyrics on a whole screen (the one Venuta is on, unless another is named). Esc goes back to a
 *  window; Esc again closes it. */
export async function openBigScreen(monitor?: string) {
  keepWindowSettings('stage', { monitor: monitor ?? null, full: true })
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

/** Every background there is: only the words, the cover's light, Venuta's theme, each theme,
 *  and each theme of yours. */
export function useBackgrounds(): { id: LyricBackground; name: string; line: string }[] {
  const packs = usePacks()
  const p = usePrefs()
  const venuta = themeInfo({ ...p }).name
  return [
    { id: 'clear', name: 'Clear', line: 'Only the words: nothing else on screen' },
    { id: 'cover', name: 'Cover', line: "The playing cover's light" },
    { id: 'venuta', name: 'Venuta', line: `As Venuta looks now (${venuta})` },
    ...STYLES.map((t) => ({ id: `style:${t.id}` as LyricBackground, name: t.name, line: t.line })),
    ...packs.filter((i) => i.pack).map((i) => ({ id: `pack:${i.id}` as LyricBackground, name: i.pack!.name, line: 'A theme of yours' })),
  ]
}
