// What every lyrics view draws under a line, from the model (lib/lyricsModel.ts) and the two
// switches (lib/secondChoice.ts): one rule for every view. The original line, then pronunciation,
// then translation, smaller and dimmer. The rows are kept for the song, not the line, so a song
// keeps the same number of rows for every line (a line with nothing for a row keeps it empty) and
// nothing changes size as the song moves. Pure, so tests run it as is.

import { langBase, scriptOfLang, songLang, sourceKind, sourceWords, wordsProgress, type LyricLine, type LyricsModel, type Second } from './lyricsModel.ts'

export type SecondKind = 'roman' | 'translation'

/** The song without the translations a machine made for it (on a PC, or by a key's service),
 *  kept from before: only those the lyrics brought, and your own corrections, are left. */
export function withoutMadeTranslations(m: LyricsModel): LyricsModel {
  if (!m.lines.some((l) => l.translation && sourceKind(l.translation.source).kind === 'Translated')) return m
  return { ...m, lines: m.lines.map((l) => (l.translation && sourceKind(l.translation.source).kind === 'Translated' ? { ...l, translation: null } : l)) }
}
export interface SecondSwitches {
  roman: boolean
  translation: boolean
}

/** A translation shows only in the language you read. One whose language the lyrics didn't say
 *  ("und-Latn": known only by its letters) shows when it is in the letters you read. */
function readable(s: Second | null, reader: string): s is Second {
  if (!s?.text) return false
  const base = langBase(s.lang)
  if (base === langBase(reader)) return true
  const script = s.lang.split(/[-_]/)[1]
  return base === 'und' && !!script && script === scriptOfLang(reader)
}

/**
 * The switches a song offers. Pronunciation: some line has one (songs in Latin letters have none).
 * Translation: the song is in another language than the one you read (a line may carry one, or
 * one can be made on this device).
 */
export function offered(m: LyricsModel | null, reader: string): SecondSwitches {
  if (!m?.lines.length) return { roman: false, translation: false }
  const lang = songLang(m)
  const roman = m.lines.some((l) => !!l.roman?.text)
  const carried = m.lines.some((l) => readable(l.translation, reader))
  const foreign = !!lang && lang !== langBase(reader)
  return { roman, translation: carried || foreign }
}

/** The rows kept for every line of the song: what is switched on and there for some line. */
export function reservedRows(m: LyricsModel | null, on: SecondSwitches, reader: string): SecondSwitches {
  if (!m) return { roman: false, translation: false }
  return {
    roman: on.roman && m.lines.some((l) => !!l.roman?.text),
    translation: on.translation && m.lines.some((l) => readable(l.translation, reader)),
  }
}

/** A row under a line: its words ('' keeps the row, empty), and where they came from. */
export interface SecondRow {
  kind: SecondKind
  text: string
  second: Second | null
}

/** The rows under one line, in order: pronunciation, then translation. */
export function rowsFor(line: LyricLine | undefined, rows: SecondSwitches, reader: string): SecondRow[] {
  const out: SecondRow[] = []
  if (rows.roman) {
    const r = line?.roman?.text ? line.roman : null
    out.push({ kind: 'roman', text: r?.text ?? '', second: r })
  }
  if (rows.translation) {
    const t = line && readable(line.translation, reader) ? line.translation : null
    out.push({ kind: 'translation', text: t?.text ?? '', second: t })
  }
  return out
}

/**
 * The karaoke fill on the pronunciation (0 to 1, by letters). By its own word times; else the
 * original's, word for word, when they count the same; else the line's fill (`lineProgress`).
 * A translation never fills: its words are in another order.
 */
export function secondProgress(line: LyricLine, second: Second, position: number, lineProgress: number): number {
  if (second.words.length) return wordsProgress(second.words, position)
  const mine = second.text.split(/\s+/).filter(Boolean)
  if (line.words.length && line.words.length === mine.length) return wordsProgress(line.words.map((w, i) => ({ ...w, text: mine[i] })), position)
  return lineProgress
}

/** Where a song's rows came from, for the quiet note under them: the most used source, and how
 *  many lines you corrected. */
export interface SourceNote {
  words: string
  corrected: number
}

export function sourcesOf(m: LyricsModel | null, rows: SecondSwitches, reader: string, device = 'PC'): { roman: SourceNote | null; translation: SourceNote | null } {
  const note = (pick: (l: LyricLine) => Second | null): SourceNote | null => {
    if (!m) return null
    const counts = new Map<string, number>()
    let corrected = 0
    for (const l of m.lines) {
      const s = pick(l)
      if (!s) continue
      if (sourceKind(s.source).kind === 'Corrected') corrected++
      else {
        const w = sourceWords(s.source, device)
        counts.set(w, (counts.get(w) ?? 0) + 1)
      }
    }
    if (!counts.size && !corrected) return null
    const words = [...counts].sort((a, b) => b[1] - a[1])[0]?.[0] ?? sourceWords({ kind: 'corrected' })
    return { words, corrected }
  }
  return {
    roman: rows.roman ? note((l) => (l.roman?.text ? l.roman : null)) : null,
    translation: rows.translation ? note((l) => (readable(l.translation, reader) ? l.translation : null)) : null,
  }
}

/** The note as one line: "Pronunciation made on this PC, 2 lines corrected by you". */
export function noteWords(kind: SecondKind, n: SourceNote): string {
  const what = kind === 'roman' ? 'Pronunciation' : 'Translation'
  const first = n.words.charAt(0).toLowerCase() + n.words.slice(1)
  const own = n.corrected ? `, ${n.corrected} ${n.corrected === 1 ? 'line' : 'lines'} corrected by you` : ''
  if (n.words === 'Corrected by you') return `${what} corrected by you`
  // "Translated on this PC" says what it is already.
  if (kind === 'translation' && n.words.startsWith('Translated')) return `${n.words}${own}`
  return `${what} ${first}${own}`
}
