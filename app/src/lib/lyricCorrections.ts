// Second lines corrected by hand: a pronunciation spelled the way you say it, a translation put
// the way you'd put it. Kept per song (by the line's own words, so the same line sung again is
// corrected too, and a retimed song keeps them), shown in place of what was made or found, and
// said to be yours. An empty correction takes it away. Pure where it can be; kept on this
// computer and heard in every window.

import { useEffect, useMemo, useState } from 'react'
import { langBase, type LyricLine, type LyricsModel, type Second } from './lyricsModel.ts'
import type { SecondKind } from './secondLines.ts'

/** A song's corrections: per original line (its words, trimmed), the rows corrected. */
export type Corrections = Record<string, Partial<Record<SecondKind, string>>>

const lineKey = (text: string) => text.trim()

export function withCorrection(c: Corrections, lineText: string, kind: SecondKind, text: string | null): Corrections {
  const key = lineKey(lineText)
  const row = { ...c[key] }
  const value = text?.trim()
  if (value) row[kind] = value
  else delete row[kind]
  const out = { ...c }
  if (Object.keys(row).length) out[key] = row
  else delete out[key]
  return out
}

function corrected(line: LyricLine, kind: SecondKind, text: string, reader: string): Second {
  if (kind === 'translation') return { text, words: [], lang: langBase(reader) || 'en', source: { kind: 'corrected' } }
  // Timed by the original words when it has as many.
  const mine = text.split(/\s+/).filter(Boolean)
  const words = line.words.length === mine.length ? line.words.map((w, i) => ({ ...w, text: mine[i] })) : []
  return { text, words, lang: line.roman?.lang ?? 'und-Latn', source: { kind: 'corrected' } }
}

/** The song with your corrections in place (the same song when there are none). */
export function applyCorrections(m: LyricsModel, c: Corrections, reader: string): LyricsModel {
  if (!Object.keys(c).length) return m
  let changed = false
  const lines = m.lines.map((l) => {
    const row = c[lineKey(l.text)]
    if (!row) return l
    changed = true
    return {
      ...l,
      roman: row.roman ? corrected(l, 'roman', row.roman, reader) : l.roman,
      translation: row.translation ? corrected(l, 'translation', row.translation, reader) : l.translation,
    }
  })
  return changed ? { ...m, lines } : m
}

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)

/** One song's corrections from what's kept (every song's, as JSON), anything not of its kind left out. */
export function readCorrections(raw: string | null, song: string): Corrections {
  if (!raw) return {}
  try {
    const all = JSON.parse(raw) as unknown
    const mine = isObj(all) ? all[song] : null
    if (!isObj(mine)) return {}
    const out: Corrections = {}
    for (const [line, v] of Object.entries(mine)) {
      if (!isObj(v)) continue
      const row: Partial<Record<SecondKind, string>> = {}
      for (const k of ['roman', 'translation'] as const) if (typeof v[k] === 'string' && v[k]) row[k] = v[k] as string
      if (Object.keys(row).length) out[line] = row
    }
    return out
  } catch {
    return {}
  }
}

// --- Kept on this computer ------------------------------------------------------------------
// One key for every song's corrections. (The engine may keep them with the lyrics later; this is
// the one place that would change.)

export const CORRECTIONS_KEY = 'lyric-corrections'
const listeners = new Set<() => void>()
const stored = () => {
  try {
    return localStorage.getItem(CORRECTIONS_KEY)
  } catch {
    return null
  }
}

/** Correct one row of a line of a song (`song`: its lyrics key), or take the correction away (''). */
export function correctLine(song: string, lineText: string, kind: SecondKind, text: string) {
  if (!song) return
  let all: Record<string, unknown> = {}
  try {
    const parsed = JSON.parse(stored() ?? '{}') as unknown
    if (isObj(parsed)) all = parsed
  } catch {
    // Unreadable: started again.
  }
  const next = withCorrection(readCorrections(JSON.stringify(all), song), lineText, kind, text)
  if (Object.keys(next).length) all[song] = next
  else delete all[song]
  try {
    localStorage.setItem(CORRECTIONS_KEY, JSON.stringify(all))
  } catch {
    // Not kept: it shows until the window closes.
  }
  listeners.forEach((l) => l())
}

if (typeof window !== 'undefined')
  window.addEventListener('storage', (e) => {
    if (e.key === CORRECTIONS_KEY) listeners.forEach((l) => l())
  })

/** A song's corrections, kept up to date (in every window). */
export function useCorrections(song: string): Corrections {
  const [version, setVersion] = useState(0)
  useEffect(() => {
    const l = () => setVersion((v) => v + 1)
    listeners.add(l)
    return () => {
      listeners.delete(l)
    }
  }, [])
  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useMemo(() => readCorrections(stored(), song), [song, version])
}
