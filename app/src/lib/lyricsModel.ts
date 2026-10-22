// Lyrics as the engine sends them (crates/engine/src/lyrics.rs, the contract in
// docs/design/lyrics-second-line.md) turned into what the lyrics views draw: lines, and beside
// them each line's words, translation and pronunciation, in step.

import type { FoundLine } from './engine'

export interface ShownLines {
  /** [ms, line], in order. */
  lines: [number, string][]
  /** Each line's words with their times [start, end, word], when any line times words. */
  words?: [number, number, string][][]
  /** Each line's translation, when any line has one. */
  translations?: (string | null)[]
  /** Each line in Roman letters, when any line has them. */
  romanized?: (string | null)[]
}

/** The engine's lines as the views draw them, every time moved by `offsetMs` (never before 0). */
export function shownLines(found: FoundLine[], offsetMs = 0): ShownLines {
  const at = (ms: number) => Math.max(0, ms + offsetMs)
  const out: ShownLines = { lines: found.map((l) => [at(l.start_ms), l.text] as [number, string]) }
  if (found.some((l) => l.words?.length))
    out.words = found.map((l) => (l.words ?? []).map((w) => [at(w.start_ms), at(w.end_ms), w.text] as [number, number, string]))
  if (found.some((l) => l.translation?.text)) out.translations = found.map((l) => l.translation?.text ?? null)
  if (found.some((l) => l.roman?.text)) out.romanized = found.map((l) => l.roman?.text ?? null)
  return out
}
