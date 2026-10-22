// A song's words, and where the song is in them: shared by Now Playing, the mini player, the
// big screen and the desktop lyrics.

import { invoke } from '@tauri-apps/api/core'
import { useEffect, useRef, useState } from 'react'
import { inApp, lyricsFind, type FoundLine, type LyricsAnswer, type LyricsAsk, type LyricsChoice } from './engine'
import { isOnline, usePlayerValue, type QueueItem } from './player'
import { useAwake } from './motion'
import { modelOf, shownLines, type LyricsModel } from './lyricsModel'
import { watchJapaneseDictionary } from './japaneseDictionary'

export interface Lyrics {
  /** [ms, line], in order. Empty when the words have no times. */
  lines: [number, string][]
  /** Words without times, as written. */
  text: string
  /** Each line's words with their times [start, end, word], when the source times words. */
  words?: [number, number, string][][]
  /** Each line's translation, when the source has one. */
  translations?: (string | null)[]
  /** Each line in Roman letters, when the source has them. */
  romanized?: (string | null)[]
  /** The song's language (BCP 47), when the source says. */
  lang?: string | null
  /** The engine's lines with their second lines (pronunciation, translation, where each came
   *  from), your offset applied: what the second-line views read (lib/lyricsModel.ts). */
  model?: LyricsModel
  /** Found online: the source's id and name, who wrote them down, and what you chose. */
  found?: { key: string; source: string; label: string; credit: string | null; choice: LyricsChoice; instrumental: boolean }
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
  window.dispatchEvent(new Event('venuta:lyrics-changed'))
}
if (typeof window !== 'undefined')
  window.addEventListener('storage', (e) => {
    if (e.key !== CHANGED_KEY || !e.newValue) return
    heard(e.newValue.slice(e.newValue.indexOf('|') + 1))
  })
// The Japanese dictionary has come: this window reads its lyrics again (every window hears the
// engine itself), so their Japanese lines get a pronunciation.
watchJapaneseDictionary(() => heard('*'))

/** What a song is called for the lyrics services: `online:<provider>:<id>`, or its file. */
export function lyricsKey(song: QueueItem | undefined): string {
  if (!song) return ''
  if (isOnline(song) && song.online?.kind === 'online') return `online:${song.online.provider}:${song.online.track_id}`
  return song.path ? `file:${song.path}` : ''
}

/** The song as the lyrics services are asked about it. */
export function lyricsAsk(song: QueueItem): LyricsAsk {
  return {
    key: lyricsKey(song),
    video_id: isOnline(song) && song.online?.kind === 'online' ? song.online.track_id : null,
    title: song.title,
    artists: song.artists.length ? song.artists : song.album_artist ? [song.album_artist] : [],
    album: song.album,
    duration_ms: song.duration_ms,
  }
}

/** A source's answer as lyrics to show: your offset applied to every time. */
export function lyricsOf(key: string, a: LyricsAnswer): Lyrics {
  const f = a.found
  if (!f) return { lines: [], text: '' }
  return {
    ...shownLines(f.lines, a.choice.offset_ms ?? 0),
    text: f.text,
    lang: f.lang ?? null,
    model: modelOf(f.lines, f.lang, a.choice.offset_ms ?? 0, f.version ?? 1),
    found: { key, source: f.source, label: a.label ?? f.source, credit: f.credit, choice: a.choice, instrumental: f.instrumental },
  }
}

/** Ask a song's lyrics again (another source chosen, the offset moved, a new name). */
export function lyricsChoiceChanged(key: string) {
  heard(`found|${key}`)
}

/** Lyrics from the lyrics services for a song with none of its own (online, or a file
 *  without lyrics): your chosen source, else Automatic. Null while loading. */
export function useOnlineLyrics(song: QueueItem | undefined, enabled: boolean): Lyrics | null {
  const key = enabled ? lyricsKey(song) : ''
  const [version, setVersion] = useState(0)
  useEffect(() => {
    const on = (k: string) => (k === '*' || k === `found|${key}`) && setVersion((v) => v + 1)
    changeListeners.add(on)
    return () => {
      changeListeners.delete(on)
    }
  }, [key])
  const [lyrics, setLyrics] = useState<{ key: string; value: Lyrics | null }>({ key: '', value: null })
  useEffect(() => {
    if (!key || !song || !inApp()) {
      setLyrics({ key: '', value: null })
      return
    }
    const known = kept.get(`found|${key}`)
    if (known) {
      setLyrics({ key, value: known })
      return
    }
    let live = true
    setLyrics({ key, value: null })
    lyricsFind(lyricsAsk(song))
      .then((a) => {
        const v = lyricsOf(key, a)
        kept.set(`found|${key}`, v)
        if (live) setLyrics({ key, value: v })
      })
      .catch(() => live && setLyrics({ key, value: { lines: [], text: '' } }))
    return () => {
      live = false
    }
  }, [key, version, song?.title, song?.album, song?.duration_ms, song?.artists?.join(',')]) // eslint-disable-line react-hooks/exhaustive-deps
  return lyrics.key === key ? lyrics.value : null
}

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
    // Your file's lines come in the engine's shape, word times and translations and all.
    invoke<{ lines: FoundLine[]; text: string; lang?: string | null }>('song_lyrics', { root, path })
      .then((read) => {
        const lines = read.lines ?? []
        const value: Lyrics = { ...shownLines(lines), text: read.text ?? '', lang: read.lang ?? null, model: modelOf(lines, read.lang) }
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

/** Words a song really has (timed lines or text). */
const hasWords = (l: Lyrics | null) => !!l && (l.lines.length > 0 || !!l.text.trim())

/**
 * A song's words, wherever they come from: your file's (its .lrc or tags) when it has them,
 * else the lyrics services' (online songs, and files without lyrics; shown only, never
 * written to the file). Null while they're read.
 */
export function useSongLyrics(root: string, song: QueueItem | undefined, enabled = true): Lyrics | null {
  const local = !!song && !isOnline(song) && !!song.path
  const file = useLyrics(root, enabled && local ? song?.path : undefined)
  // Online songs, and files that turned out to have no words of their own.
  const fileless = !local || (file !== null && !hasWords(file))
  const found = useOnlineLyrics(song, enabled && !!song && fileless)
  if (!enabled || !song) return null
  if (previewWords && !inApp()) return previewWords(song)
  if (local && file === null) return null
  if (local && hasWords(file)) return file
  return found
}

/** UI previews outside the app (dev only: lyrics-preview.html): the words a song shows. */
let previewWords: ((song: QueueItem) => Lyrics | null) | null = null
export function setPreviewLyrics(words: ((song: QueueItem) => Lyrics | null) | null) {
  if (import.meta.env.DEV) previewWords = words
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
  // Timed word by word: the fill follows the singing, each word as it is sung.
  const words = lyrics.words?.[at]
  if (words?.length) {
    const t = position + LEAD_MS
    const total = words.reduce((n, w) => n + w[2].length, 0) || 1
    let done = 0
    for (const [s, e, w] of words) {
      if (t >= e) done += w.length
      else {
        if (t > s) done += w.length * ((t - s) / Math.max(1, e - s))
        break
      }
    }
    return Math.min(1, done / total)
  }
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
  // Each frame only while playing and on screen (lib/motion.ts).
  const awake = useAwake()
  useEffect(() => {
    if (!playing || !awake) return
    let frame = 0
    const tick = () => {
      // Never more than a report ahead: a stall shows as a stall, not a runaway.
      const ahead = Math.min(250, performance.now() - at.current.t)
      setNow(at.current.ms + ahead)
      frame = requestAnimationFrame(tick)
    }
    frame = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(frame)
  }, [playing, awake])
  return now
}
