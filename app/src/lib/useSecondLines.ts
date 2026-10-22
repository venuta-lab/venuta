// A song's second lines as a view draws them: the model (your corrections in place), which
// switches it offers, which are on (remembered per song language), the rows kept for every line,
// where they came from, and what Translation can do when the lyrics carry none in your language.
// Worked out once per song (memoised), never per frame.

import { useEffect, useMemo } from 'react'
import { dictionaryWords, useJapaneseDictionary } from './japaneseDictionary'
import { applyCorrections, useCorrections } from './lyricCorrections'
import { lyricsChanged, lyricsChoiceChanged, lyricsKey, type Lyrics } from './lyrics'
import { fromLegacy, songLang, type LyricsModel } from './lyricsModel'
import type { QueueItem } from './player'
import { choiceFor, readerLang, setSecondLine, systemLang, useSecondChoices } from './secondChoice'
import { noteWords, offered, reservedRows, rowsFor, secondProgress, sourcesOf, type SecondKind, type SecondSwitches } from './secondLines'
import type { SecondRowView } from '../components/lyrics/LyricText'
import { modelFor, translateSong, translationStatus, useTranslationModels, type TranslationStatus } from './translation'

export interface SecondLinesView {
  /** The song's lyrics key (corrections are kept by it). */
  key: string
  model: LyricsModel | null
  /** The song's language (null: not known). */
  lang: string | null
  /** The language you read. */
  reader: string
  /** The switches this song offers. */
  offered: SecondSwitches
  /** The switches as they are for this song's language (only those offered). */
  on: SecondSwitches
  /** The rows every line keeps. */
  rows: SecondSwitches
  toggle: (kind: SecondKind) => void
  /** Where the rows came from, in a line each (null: not shown). */
  notes: Record<SecondKind, string | null>
  translation: TranslationStatus
  /** The Japanese dictionary on its way, for a song that waits for it (null: nothing to say). */
  dictionary: string | null
}

const NONE: SecondSwitches = { roman: false, translation: false }
/** Songs asked to be translated in this window (the engine keeps one answer per song). */
const asked = new Set<string>()

export function useSecondLines(song: QueueItem | null | undefined, lyrics: Lyrics | null): SecondLinesView {
  const key = lyricsKey(song ?? undefined)
  const choices = useSecondChoices()
  const corrections = useCorrections(key)
  const models = useTranslationModels()
  const dict = useJapaneseDictionary()
  const reader = readerLang(choices, systemLang())
  const base = useMemo(() => (lyrics?.lines.length ? fromLegacy(lyrics) : null), [lyrics])
  const model = useMemo(() => (base ? applyCorrections(base, corrections, reader) : null), [base, corrections, reader])
  const lang = songLang(model)
  const choice = choiceFor(choices, lang)
  const view = useMemo((): SecondLinesView => {
    const can = offered(model, reader)
    const carried = reservedRows(model, { roman: true, translation: true }, reader).translation
    const offers = { roman: can.roman, translation: carried || (can.translation && !!modelFor(models, lang, reader)) }
    const on = { roman: offers.roman && choice.roman, translation: offers.translation && choice.translation }
    const rows = model ? reservedRows(model, on, reader) : NONE
    const src = sourcesOf(model, rows, reader)
    return {
      key,
      model,
      lang,
      reader,
      offered: offers,
      on,
      rows,
      toggle: (kind) => setSecondLine(lang, kind, !on[kind]),
      notes: { roman: src.roman && noteWords('roman', src.roman), translation: src.translation && noteWords('translation', src.translation) },
      translation: translationStatus({ have: carried, on: on.translation, from: lang, to: reader, models }),
      // Said only while some Japanese line still waits for it.
      dictionary: model?.lines.some((l) => !l.roman && (l.script === 'Jpan' || (lang === 'ja' && l.script === 'Hani'))) ? dictionaryWords(dict) : null,
    }
  }, [key, model, lang, reader, choice.roman, choice.translation, models, dict])

  // Translation on, its model on this PC, and nothing in your language yet: translate the song
  // here (once), then read its lyrics again with the translation in them.
  const translating = view.translation.kind === 'translating'
  useEffect(() => {
    if (!translating || !key || !lang) return
    const ask = `${key}|${lang}|${reader}`
    if (asked.has(ask)) return
    asked.add(ask)
    void translateSong(key, lang, reader)
      .then(() => (key.startsWith('file:') ? lyricsChanged() : lyricsChoiceChanged(key)))
      .catch(() => asked.delete(ask))
  }, [translating, key, lang, reader])
  return view
}

/**
 * Line `i`'s rows as LyricText draws them, in the view's language tags; with `fill` (the clock),
 * the pronunciation's own karaoke fill. Before the first line (or past the last), the rows are
 * kept, empty, so nothing moves.
 */
export function rowViewsAt(v: SecondLinesView, i: number, fill?: { position: number; progress: number }): SecondRowView[] | undefined {
  if (!v.rows.roman && !v.rows.translation) return undefined
  const line = v.model?.lines[i]
  return rowsFor(line, v.rows, v.reader).map((r) => ({
    kind: r.kind,
    text: r.text,
    lang: r.second?.lang,
    progress: fill && line && r.kind === 'roman' && r.second ? secondProgress(line, r.second, fill.position, fill.progress) : undefined,
  }))
}
