// Telling when an online song is one you already have, so search shows it once, as yours, and
// it plays from your file. The same song is: the same title once film credits, features and
// remaster notes are left out; at least one artist in common; and lengths within a few seconds
// when both are known. Titles and names are compared by their search keys, so a Roman spelling
// finds a Devanagari one (lib/textsearch.ts).

import { searchKey } from './textsearch.ts'

/** Lengths further apart than this are different versions (a radio edit, a film cut). */
const SAME_LENGTH_MS = 4_000

/** Where a title stops meaning the song: what follows is credits and notes. */
const CUTS = [' (from', ' [from', ' - from', ' (feat', ' [feat', ' feat.', ' (ft.', ' ft.', ' (remaster', ' [remaster', ' - remaster', ' (live', ' - live']

/** The words that name the song, as a search key. */
export function titleCore(title: string): string {
  const lower = title.toLowerCase()
  const at = Math.min(...CUTS.map((c) => lower.indexOf(c)).filter((i) => i > 0), lower.length)
  return searchKey(title.slice(0, at))
}

interface Song {
  title: string
  artists: string[]
  duration_ms?: number | null
}

/**
 * Two artist names are the same person when every word of the shorter one matches a word of
 * the other, a word matching when it equals it or begins it ("sin" for "sing": सिंह and Singh
 * spell the same name a letter apart once read in Roman letters). Title and length still have to
 * agree, so the looseness here never makes two songs one.
 */
export function sameName(a: string, b: string): boolean {
  if (a === b) return true
  const [short, long] = a.length <= b.length ? [a.split(' '), b.split(' ')] : [b.split(' '), a.split(' ')]
  const fits = (w: string, v: string) => w === v || (w.length >= 3 && v.startsWith(w)) || (v.length >= 3 && w.startsWith(v))
  return short.length > 0 && short.every((w) => long.some((v) => fits(w, v)))
}

/** A lookup over your songs: the one of yours an online song is, if any. */
export function ownedIndex<T extends Song>(mine: T[]): (online: Song) => T | undefined {
  const byTitle = new Map<string, T[]>()
  for (const t of mine) {
    const key = titleCore(t.title)
    if (!key) continue
    const list = byTitle.get(key)
    if (list) list.push(t)
    else byTitle.set(key, [t])
  }
  return (online) => {
    const candidates = byTitle.get(titleCore(online.title))
    if (!candidates) return undefined
    const theirs = online.artists.map(searchKey).filter(Boolean)
    return candidates.find((t) => {
      const shared = t.artists.some((a) => {
        const key = searchKey(a)
        return !!key && theirs.some((b) => sameName(key, b))
      })
      if (!shared) return false
      const a = t.duration_ms ?? 0
      const b = online.duration_ms ?? 0
      return !a || !b || Math.abs(a - b) <= SAME_LENGTH_MS
    })
  }
}
