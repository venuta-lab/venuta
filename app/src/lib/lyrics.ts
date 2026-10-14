// A song's words, and where the song is in them: shared by Now Playing, the mini player, the
// big screen and the desktop lyrics.

import { invoke } from '@tauri-apps/api/core'
import { useEffect, useRef, useState } from 'react'
import { inApp } from './engine'
import { usePlayerValue } from './player'

export interface Lyrics {
  /** [ms, line], in order. Empty when the words have no times. */
  lines: [number, string][]
  /** Words without times, as written. */
  text: string
}

const kept = new Map<string, Lyrics>()

/** Bumped when a song's words change; every window hears it (the storage event crosses them). */
const CHANGED_KEY = 'lyrics-changed'
const changeListeners = new Set<(key: string) => void>()
/** `root|path`, or '*' for every song. */
function heard(key: string) {
  if (key === '*') kept.clear()
  else kept.delete(key)
  changeListeners.forEach((l) => l(key))
}
if (typeof window !== 'undefined')
  window.addEventListener('storage', (e) => {
    if (e.key !== CHANGED_KEY || !e.newValue) return
    heard(e.newValue.slice(e.newValue.indexOf('|') + 1))
  })

/** The words of a song (null while they're read). */
export function useLyrics(root: string, path: string | undefined): Lyrics | null {
  const [lyrics, setLyrics] = useState<{ path: string; value: Lyrics | null }>({ path: '', value: null })
  const [version, setVersion] = useState(0)
  useEffect(() => {
    const on = (key: string) => (key === '*' || key === `${root}|${path}`) && setVersion((v) => v + 1)
    changeListeners.add(on)
    return () => {
      changeListeners.delete(on)
    }
  }, [root, path])
  useEffect(() => {
    if (!path || !inApp()) return
    const known = kept.get(`${root}|${path}`)
    if (known) {
      setLyrics({ path, value: known })
      return
    }
    let live = true
    invoke<Lyrics>('song_lyrics', { root, path })
      .then((value) => {
        kept.set(`${root}|${path}`, value)
        if (live) setLyrics({ path, value })
      })
      .catch(() => live && setLyrics({ path, value: { lines: [], text: '' } }))
    return () => {
      live = false
    }
  }, [root, path, version])
  return lyrics.path === path ? lyrics.value : null
}

/**
 * A song's words changed (timed, or the timing undone): read them again, in every window. With
 * no song, every song's words are read again.
 */
export function lyricsChanged(root?: string, path?: string) {
  const key = root && path ? `${root}|${path}` : '*'
  heard(key)
  try {
    localStorage.setItem(CHANGED_KEY, `${Date.now()}|${key}`)
  } catch {
    // Other windows then show the old words until the song changes.
  }
}

/** Play a song and open its lyrics, ready to tap along and time them (from Health). */
export const timeLyrics = (root: string, path: string) =>
  window.dispatchEvent(new CustomEvent<{ root: string; path: string }>('venuta:time-lyrics', { detail: { root, path } }))

/** Lines light up a moment early, so they're read as they're sung. */
const LEAD_MS = 250

/** The line being sung at `position`; -1 before the first. */
export function lineAt(lyrics: Lyrics | null, position: number): number {
  if (!lyrics?.lines.length) return -1
  let lo = 0
  let hi = lyrics.lines.length - 1
  let found = -1
  const t = position + LEAD_MS
  while (lo <= hi) {
    const mid = (lo + hi) >> 1
    if (lyrics.lines[mid][0] <= t) {
      found = mid
      lo = mid + 1
    } else hi = mid - 1
  }
  return found
}

/**
 * How far through line `at` the song is, 0 to 1: the karaoke fill. Lines are timed, words
 * aren't, so it sweeps evenly to the next line's start (or 5 seconds for the last line).
 */
export function lineProgress(lyrics: Lyrics | null, at: number, position: number): number {
  if (!lyrics || at < 0) return 0
  const start = lyrics.lines[at][0]
  const end = lyrics.lines[at + 1]?.[0] ?? start + 5000
  // The fill finishes a little before the next line comes in.
  const span = Math.max(400, end - start - 300)
  return Math.min(1, Math.max(0, (position + LEAD_MS - start) / span))
}

/**
 * The song's position, moving smoothly between the player's reports (every 100 ms), for
 * lyrics drawn large: the karaoke fill and the big screen. Re-renders each frame while playing.
 */
export function useSmoothPosition(): number {
  const reported = usePlayerValue((s) => s.position_ms)
  const playing = usePlayerValue((s) => s.playing)
  const at = useRef({ ms: reported, t: performance.now() })
  const [now, setNow] = useState(reported)
  useEffect(() => {
    at.current = { ms: reported, t: performance.now() }
    setNow(reported)
  }, [reported])
  useEffect(() => {
    if (!playing) return
    let frame = 0
    const tick = () => {
      // Never more than a report ahead: a stall shows as a stall, not a runaway.
      const ahead = Math.min(250, performance.now() - at.current.t)
      setNow(at.current.ms + ahead)
      frame = requestAnimationFrame(tick)
    }
    frame = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(frame)
  }, [playing])
  return now
}
