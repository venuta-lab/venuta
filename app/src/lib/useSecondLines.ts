// A song's second lines as a view draws them: the model (your corrections in place), which
// switches it offers, which are on (remembered per song language), the rows kept for every line,
// where they came from, and what Translation can do when the lyrics carry none in your language.
// Worked out once per song (memoised), never per frame.

import { useEffect, useMemo } from 'react'
import { dictionaryWords, useJapaneseDictionary } from './japaneseDictionary'
import { applyCorrections, useCorrections } from './lyricCorrections'
import { lyricsChanged, lyricsChoiceChanged, lyricsKey, type Lyrics } from './lyrics'
import { attributionOf, fromLegacy, songLang, type LyricsModel } from './lyricsModel'
import type { QueueItem } from './player'
import { choiceFor, readerLang, setSecondLine, systemLang, useSecondChoices } from './secondChoice'
import { noteWords, offered, reservedRows, rowsFor, secondProgress, sourcesOf, type SecondKind, type SecondSwitches } from './secondLines'
import type { SecondRowView } from '../components/lyrics/LyricText'
import { pressTranslate, runKey, startRun, translateSong, translationStatus, useRun, useTranslationKeys, useTranslationModels, type TranslationStatus } from './translation'
import { MAKE_TRANSLATIONS, engineArg, isKeyed, useEngineChoice } from './translationEngine'

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
  /** Translate this song now with your key (or try again after a failure). */
  translate: () => void
  /** The Japanese dictionary on its way, for a song that waits for it (null: nothing to say). */
  dictionary: string | null
}

const NONE: SecondSwitches = { roman: false, translation: false }

export function useSecondLines(song: QueueItem | null | undefined, lyrics: Lyrics | null): SecondLinesView {
  const key = lyricsKey(song ?? undefined)
  const choices = useSecondChoices()
  const corrections = useCorrections(key)
  const models = useTranslationModels()
  const keys = useTranslationKeys()
  const engine = useEngineChoice()
  const dict = useJapaneseDictionary()
  const reader = readerLang(choices, systemLang())
  const base = useMemo(() => (lyrics?.lines.length ? fromLegacy(lyrics) : null), [lyrics])
  const model = useMemo(() => (base ? applyCorrections(base, corrections, reader) : null), [base, corrections, reader])
  const lang = songLang(model)
  const choice = choiceFor(choices, lang)
  const arg = engineArg(engine)
  const run = runKey(key, lang ?? '', reader, arg)
  const { asked, error } = useRun(run)
  const keyKept = isKeyed(engine.engine) && keys.some((k) => k.engine === engine.engine && k.kept)
  const view = useMemo((): SecondLinesView => {
    const can = offered(model, reader)
    const carried = reservedRows(model, { roman: true, translation: true }, reader).translation
    // Translation is offered when the lyrics carry one in your language; while Venuta makes
    // translations itself, for every song in another language (when nothing can translate it
    // here, the note under the switches says why and what can).
    const offers = { roman: can.roman, translation: carried || (MAKE_TRANSLATIONS && can.translation) }
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
      translation: translationStatus({ have: carried, on: on.translation, from: lang, to: reader, models, engine, keyKept, asked, error }),
      translate: () => pressTranslate(run),
      // Said only while some Japanese line still waits for it.
      dictionary: model?.lines.some((l) => !l.roman && (l.script === 'Jpan' || (lang === 'ja' && l.script === 'Hani'))) ? dictionaryWords(dict) : null,
    }
  }, [key, model, lang, reader, choice.roman, choice.translation, models, dict, engine, keyKept, asked, error, run])

  // Translation on and nothing in your language yet: translate the song once, on this PC when
  // its model is here, or with your key when you pressed Translate (or turned on translating
  // automatically), then read its lyrics again with the translation in them. A failure stays,
  // said in words, until you try again.
  const translating = view.translation.kind === 'translating'
  useEffect(() => {
    if (!translating || !key || !lang) return
    startRun(
      run,
      () => translateSong(key, lang, reader, arg.engine === 'device' ? undefined : arg),
      () => (key.startsWith('file:') ? lyricsChanged() : lyricsChoiceChanged(key)),
    )
    // `arg` is part of `run`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [translating, run, key, lang, reader])
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
    attribution: attributionOf(r.second?.source) ?? undefined,
    progress: fill && line && r.kind === 'roman' && r.second ? secondProgress(line, r.second, fill.position, fill.progress) : undefined,
  }))
}
