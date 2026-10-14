// The big screen (lyrics filling a whole screen, for a TV or a second monitor) and the desktop
// lyrics (the lines alone, floating over everything), each its own window.

import { invoke } from '@tauri-apps/api/core'
import { useEffect, useState } from 'react'
import { inApp } from './engine'

export interface Monitor {
  name: string
  width: number
  height: number
  primary: boolean
  /** The screen Venuta's window is on. */
  current: boolean
}

export const monitors = () => (inApp() ? invoke<Monitor[]>('monitors') : Promise.resolve([] as Monitor[]))

/** Lyrics on a whole screen (the one Venuta is on, unless another is named). Esc closes it. */
export const openBigScreen = (monitor?: string) => invoke<void>('lyrics_stage', { open: true, monitor: monitor ?? null })
export const closeBigScreen = () => invoke<void>('lyrics_stage', { open: false, monitor: null })

const BOUNDS = 'desktop-lyrics-bounds'
const LOCKED = 'desktop-lyrics-locked'

/** Where you last left the desktop lyrics (logical pixels), if you moved them. */
export function savedBounds(): [number, number, number, number] | null {
  try {
    const b = JSON.parse(localStorage.getItem(BOUNDS) ?? 'null') as [number, number, number, number] | null
    return Array.isArray(b) && b.length === 4 && b.every(Number.isFinite) ? b : null
  } catch {
    return null
  }
}

export function saveBounds(b: [number, number, number, number]) {
  try {
    localStorage.setItem(BOUNDS, JSON.stringify(b.map(Math.round)))
  } catch {
    // Not kept: it opens in its usual place next time.
  }
}

export function lockedSaved(): boolean {
  try {
    return localStorage.getItem(LOCKED) !== 'false'
  } catch {
    return true
  }
}

/** Heard in this window when the desktop lyrics come or go, so their buttons light up at once. */
const SHOWN = 'venuta:desktop-lyrics-shown'
const told = <T,>(p: Promise<T>) => p.finally(() => window.dispatchEvent(new Event(SHOWN)))

export const openDesktopLyrics = () =>
  told(invoke<void>('desktop_lyrics', { open: true, bounds: savedBounds(), locked: lockedSaved() }))
export const closeDesktopLyrics = () => told(invoke<void>('desktop_lyrics', { open: false, bounds: null, locked: false }))
export const desktopLyricsOpen = () => (inApp() ? invoke<boolean>('desktop_lyrics_open') : Promise.resolve(false))

/**
 * Whether the desktop lyrics are showing: asked when this mounts, after a toggle here, when
 * the window comes back to the front, and every couple of seconds (the tray, the shortcut and
 * the lyrics' own Close change it too).
 */
export function useDesktopLyricsShown(): boolean {
  const [shown, setShown] = useState(false)
  useEffect(() => {
    let live = true
    const ask = () => void desktopLyricsOpen().then((v) => live && setShown(v), () => {})
    ask()
    const every = window.setInterval(ask, 2000)
    window.addEventListener(SHOWN, ask)
    window.addEventListener('focus', ask)
    return () => {
      live = false
      window.clearInterval(every)
      window.removeEventListener(SHOWN, ask)
      window.removeEventListener('focus', ask)
    }
  }, [])
  return shown
}

export async function toggleDesktopLyrics() {
  if (await desktopLyricsOpen()) await closeDesktopLyrics()
  else await openDesktopLyrics()
}

/** Locked: clicks pass through to the windows under it. Unlocked: drag it, size it, change it. */
export async function lockDesktopLyrics(lock: boolean) {
  try {
    localStorage.setItem(LOCKED, String(lock))
  } catch {
    // Applies now, even if it can't be kept.
  }
  await invoke<void>('desktop_lyrics_lock', { lock })
}
