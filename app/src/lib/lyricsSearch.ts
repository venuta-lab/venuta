// Searching the words of songs: every line of every song's lyrics, folded the way names are
// (lib/textsearch), so a line written in Devanagari is found by typing it in Roman letters and
// the other way round. Read once per library and kept; read again a few minutes later.

import { useEffect, useMemo, useState } from 'react'
import { allLyrics } from './engine'
import { EXACTLY, matcher, searchKey, toLatin } from './textsearch'

interface Line {
  path: string
  text: string
  key: string
}

/** A song whose lyrics hold the words, and the first line that does. */
export interface LyricHit {
  path: string
  line: string
  /** The line in Roman letters, when it is written in another script. */
  latin?: string
}

const FRESH_MS = 5 * 60_000
let kept: { root: string; at: number; lines: Promise<Line[]> } | null = null

/** Fold every line, a slice at a time so typing never waits for it (tens of thousands of lines). */
async function keyed(songs: { path: string; lines: string[] }[]): Promise<Line[]> {
  const out: Line[] = []
  let since = performance.now()
  for (const s of songs) {
    for (const text of s.lines) out.push({ path: s.path, text, key: searchKey(text) })
    if (performance.now() - since > 12) {
      await new Promise((r) => setTimeout(r, 0))
      since = performance.now()
    }
  }
  return out
}

function linesOf(root: string): Promise<Line[]> {
  if (kept && kept.root === root && Date.now() - kept.at < FRESH_MS) return kept.lines
  const lines = allLyrics(root).then(keyed)
  kept = { root, at: Date.now(), lines }
  lines.catch(() => {
    if (kept?.lines === lines) kept = null
  })
  return lines
}

/** Typed queries this short don't search lyrics (every song has an "a" in it). */
const SHORTEST = 3

/**
 * Songs whose lyrics hold every word of the query in one line (exactly, in any script), at
 * most `limit`, in the order of their songs. Nothing until the lyrics are read (the first
 * time takes a moment).
 */
export function useLyricHits(root: string, query: string, limit = 30): LyricHit[] {
  const wanted = searchKey(query).replace(/ /g, '').length >= SHORTEST
  const [lines, setLines] = useState<Line[] | null>(null)
  useEffect(() => {
    if (!wanted || !root) return
    let live = true
    linesOf(root)
      .then((l) => live && setLines(l))
      .catch(() => live && setLines([]))
    return () => {
      live = false
    }
  }, [root, wanted])
  return useMemo(() => {
    if (!wanted || !lines) return []
    const has = matcher(query, { exact: true })
    const seen = new Set<string>()
    const hits: LyricHit[] = []
    for (const l of lines) {
      if (seen.has(l.path) || has(l.key) !== EXACTLY) continue
      seen.add(l.path)
      const latin = /[ऀ-ൿ]/.test(l.text) ? toLatin(l.text) : undefined
      hits.push({ path: l.path, line: l.text, latin })
      if (hits.length >= limit) break
    }
    return hits
  }, [lines, query, wanted, limit])
}
