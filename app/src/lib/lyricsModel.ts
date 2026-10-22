// Lyrics as the engine sends them (crates/engine/src/lyrics.rs, the contract in
// docs/design/lyrics-second-line.md): each line as sung, with up to two second lines under it,
// pronunciation and translation, each saying where it came from. Turned here into what the lyrics
// views draw: the lines beside their words and second lines, in step (`shownLines`), and the
// model the second-line views read (`modelOf`, `fromLegacy`). Pure, so tests run it as is.

import type { FoundLine, LyricWord, SecondSource as EngineSource } from './engine'
import type { Lyrics } from './lyrics'
import { toLatin } from './textsearch.ts'

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

// --- The model the second-line views read ---------------------------------------------------
// The engine's shape with every field filled in (words always a list, a second line or null),
// so the views never ask what is missing.

export type Word = LyricWord

/** Where a second line came from: `{ kind: "lyrics", by: "netease" }`, `{ kind: "made" }`, ... */
export type SecondSource = EngineSource

/** A second line: pronunciation or translation. */
export interface Second {
  text: string
  /** Pronunciation only: timed by the original's words. */
  words: Word[]
  /** BCP 47: "hi-Latn" for pronunciation, "en" for translation. */
  lang: string
  source: SecondSource
}

export interface LyricLine {
  start_ms: number
  end_ms: number
  /** The original, as sung. */
  text: string
  words: Word[]
  /** ISO 15924, as the engine has it: Latn, Deva, Jpan, Hang, Hani, Cyrl, ...; Zyyy for none. */
  script: string
  /** Pronunciation. */
  roman: Second | null
  /** Meaning. */
  translation: Second | null
}

/** The contract's `Lyrics { lines, lang, version }` (named so beside the views' `Lyrics`). */
export interface LyricsModel {
  lines: LyricLine[]
  lang: string | null
  version: number
}

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
const num = (v: unknown, or = 0) => (typeof v === 'number' && Number.isFinite(v) ? v : or)
const str = (v: unknown, or = '') => (typeof v === 'string' ? v : or)

function readWords(v: unknown, at: (ms: number) => number): Word[] {
  if (!Array.isArray(v)) return []
  return v.filter(isObj).map((w) => ({ start_ms: at(num(w.start_ms ?? w.ms)), end_ms: at(num(w.end_ms)), text: str(w.text) }))
}

function readSecond(v: unknown, at: (ms: number) => number): Second | null {
  if (!isObj(v) || typeof v.text !== 'string' || !v.text) return null
  return { text: v.text, words: readWords(v.words, at), lang: str(v.lang, 'und'), source: (v.source ?? { kind: 'made' }) as SecondSource }
}

function readLines(lines: unknown[], at: (ms: number) => number): LyricLine[] {
  return lines.filter(isObj).map((l) => ({
    start_ms: at(num(l.start_ms ?? l.ms)),
    end_ms: at(num(l.end_ms)),
    text: str(l.text),
    words: readWords(l.words, at),
    script: str(l.script) || scriptOf(str(l.text)),
    roman: readSecond(l.roman, at),
    translation: readSecond(l.translation, at),
  }))
}

/** The engine's lines as the model, every time moved by `offsetMs` (never before 0). */
export function modelOf(found: FoundLine[], lang: string | null | undefined, offsetMs = 0, version = 1): LyricsModel {
  const at = (ms: number) => Math.max(0, ms + offsetMs)
  return { lines: readLines(found as unknown[], at), lang: lang || null, version }
}

/** A whole answer as the model (fixtures, tests): fields left out read as none. */
export function readModel(v: unknown): LyricsModel {
  const o = isObj(v) ? v : {}
  return { lines: readLines(Array.isArray(o.lines) ? o.lines : [], (ms) => ms), lang: typeof o.lang === 'string' && o.lang ? o.lang : null, version: num(o.version, 1) }
}

// --- Where a second line came from ---------------------------------------------------------

export type SourceKind = 'Lyrics' | 'Paired' | 'Made' | 'Translated' | 'Corrected'
const KINDS: SourceKind[] = ['Lyrics', 'Paired', 'Made', 'Translated', 'Corrected']
const kindOf = (s: string): SourceKind | undefined => KINDS.find((k) => k.toLowerCase() === s.toLowerCase())

/** A source in one shape: the engine's `{ kind, by }`, and also serde's untagged `"Made"` or
 *  `{ "Lyrics": "netease" }`. Anything else is the lyrics'. */
export function sourceKind(source: SecondSource): { kind: SourceKind; by: string | null } {
  const s: unknown = source
  if (typeof s === 'string') return { kind: kindOf(s) ?? 'Lyrics', by: null }
  if (isObj(s)) {
    const tagged = typeof s.kind === 'string' ? kindOf(s.kind) : undefined
    if (tagged) {
      const by = s.by ?? s.provider ?? s.engine ?? null
      return { kind: tagged, by: typeof by === 'string' ? by : null }
    }
    const [key, value] = Object.entries(s)[0] ?? []
    const kind = key ? kindOf(key) : undefined
    if (kind) return { kind, by: typeof value === 'string' ? value : null }
  }
  return { kind: 'Lyrics', by: null }
}

/** Engines that run with the listener's own key (L7), by their names. */
const KEYED: Record<string, string> = { deepl: 'DeepL', microsoft: 'Microsoft', google: 'Google', llm: 'Claude' }

/** Where a second line came from, in a few quiet words. `device`: "PC", later "phone". */
export function sourceWords(s: SecondSource, device = 'PC'): string {
  const { kind, by } = sourceKind(s)
  switch (kind) {
    case 'Lyrics':
      return 'From the lyrics'
    case 'Paired':
      return 'From another copy of the lyrics'
    case 'Made':
      return `Made on this ${device}`
    case 'Translated': {
      const keyed = by ? KEYED[by.toLowerCase()] : undefined
      return keyed ? `Translated by ${keyed}, with your key` : `Translated on this ${device}`
    }
    case 'Corrected':
      return 'Corrected by you'
  }
}

// --- Scripts and languages -----------------------------------------------------------------

/** The scripts told apart (as the engine names them), each with the language a song in it most
 *  likely is. */
const SCRIPTS: [code: string, test: RegExp, lang: string | null][] = [
  ['Latn', /\p{Script=Latin}/u, null],
  ['Deva', /\p{Script=Devanagari}/u, 'hi'],
  ['Beng', /\p{Script=Bengali}/u, 'bn'],
  ['Guru', /\p{Script=Gurmukhi}/u, 'pa'],
  ['Gujr', /\p{Script=Gujarati}/u, 'gu'],
  ['Orya', /\p{Script=Oriya}/u, 'or'],
  ['Taml', /\p{Script=Tamil}/u, 'ta'],
  ['Telu', /\p{Script=Telugu}/u, 'te'],
  ['Knda', /\p{Script=Kannada}/u, 'kn'],
  ['Mlym', /\p{Script=Malayalam}/u, 'ml'],
  ['Arab', /\p{Script=Arabic}/u, 'ur'],
  ['Hebr', /\p{Script=Hebrew}/u, 'he'],
  ['Thai', /\p{Script=Thai}/u, 'th'],
  ['Hang', /\p{Script=Hangul}/u, 'ko'],
  ['Jpan', /[\p{Script=Hiragana}\p{Script=Katakana}]/u, 'ja'],
  ['Hani', /\p{Script=Han}/u, 'zh'],
  ['Cyrl', /\p{Script=Cyrillic}/u, 'ru'],
  ['Grek', /\p{Script=Greek}/u, 'el'],
]

function countScripts(text: string, into: Map<string, number>) {
  for (const ch of text) {
    const hit = SCRIPTS.find(([, re]) => re.test(ch))
    if (hit) into.set(hit[0], (into.get(hit[0]) ?? 0) + 1)
  }
}

function dominant(counts: Map<string, number>): string {
  // Kanji beside kana is Japanese.
  if (counts.get('Jpan') && counts.get('Hani')) {
    counts.set('Jpan', (counts.get('Jpan') ?? 0) + (counts.get('Hani') ?? 0))
    counts.delete('Hani')
  }
  let best = 'Zyyy'
  let most = 0
  // Latin loses a tie (as the engine reads it).
  for (const [code, n] of counts) if (n > most || (n === most && best === 'Latn')) [best, most] = [code, n]
  return best
}

/** The script a line is written in: the one most of its letters are in (Zyyy: none). */
export function scriptOf(text: string): string {
  const counts = new Map<string, number>()
  countScripts(text, counts)
  return dominant(counts)
}

/** A language tag's language: "hi-Latn" is "hi", "pt_BR" is "pt". */
export const langBase = (tag: string | null | undefined) => (tag ?? '').split(/[-_]/)[0].toLowerCase()

const OTHER_SCRIPTS: Record<string, string> = { mr: 'Deva', ne: 'Deva', sa: 'Deva', ar: 'Arab', fa: 'Arab', uk: 'Cyrl', bg: 'Cyrl', sr: 'Cyrl' }

/** The script a language is read in (most are in Latin letters). */
export function scriptOfLang(tag: string): string {
  const base = langBase(tag)
  return SCRIPTS.find(([, , lang]) => lang === base)?.[0] ?? OTHER_SCRIPTS[base] ?? 'Latn'
}

/** The song's language: as the lyrics say, else the likely one for its script (none for songs in
 *  Latin letters, which could be any). */
export function songLang(m: LyricsModel | null): string | null {
  if (!m) return null
  if (m.lang && langBase(m.lang) !== 'und') return langBase(m.lang)
  const counts = new Map<string, number>()
  for (const l of m.lines) countScripts(l.text, counts)
  const code = dominant(counts)
  return SCRIPTS.find(([c]) => c === code)?.[2] ?? null
}

// --- Lyrics without the model ---------------------------------------------------------------

const INDIC = /[ऀ-ൿ]/
/** How long the last line is held when nothing comes after it. */
const LAST_LINE_MS = 5000

const once = new WeakMap<Lyrics, LyricsModel>()

/**
 * The model of a song's lyrics: the engine's (`model`, as every reader now sends it), else read
 * from the arrays (lines, words, translations, Roman letters), with Roman letters for
 * Indian-script lines made by rule when nothing carried any. Worked out once per song.
 */
export function fromLegacy(l: Lyrics): LyricsModel {
  if (l.model) return l.model
  const known = once.get(l)
  if (known) return known
  const provider = l.found?.source ?? 'file'
  const lines: LyricLine[] = l.lines.map(([ms, text], i) => {
    const words = (l.words?.[i] ?? []).map(([s, e, t]) => ({ start_ms: s, end_ms: e, text: t }))
    const script = scriptOf(text)
    const carried = l.romanized?.[i]
    const roman: Second | null = carried
      ? { text: carried, words: [], lang: 'und-Latn', source: { kind: 'lyrics', by: provider } }
      : text && INDIC.test(text)
        ? {
            text: toLatin(text),
            words: words.map((w) => ({ ...w, text: toLatin(w.text) })),
            lang: `${SCRIPTS.find(([c]) => c === script)?.[2] ?? 'hi'}-Latn`,
            source: { kind: 'made' },
          }
        : null
    const t = l.translations?.[i]
    const tScript = t ? scriptOf(t) : ''
    const tLang = SCRIPTS.find(([c]) => c === tScript)?.[2] ?? `und-${tScript}`
    const translation: Second | null = t ? { text: t, words: [], lang: tLang, source: { kind: 'lyrics', by: provider } } : null
    return { start_ms: ms, end_ms: l.lines[i + 1]?.[0] ?? ms + LAST_LINE_MS, text, words, script, roman, translation }
  })
  const model: LyricsModel = { lines, lang: l.lang ?? null, version: 0 }
  model.lang = model.lang ?? songLang(model)
  once.set(l, model)
  return model
}

// --- Word by word ---------------------------------------------------------------------------

/** Lines light up a moment early, so they're read as they're sung (as lib/lyrics.ts). */
export const LEAD_MS = 250

/** How far through timed words `position` is, 0 to 1, by letters: the karaoke fill. */
export function wordsProgress(words: { start_ms: number; end_ms: number; text: string }[], position: number): number {
  const t = position + LEAD_MS
  const total = words.reduce((n, w) => n + w.text.replace(/\s+/g, '').length, 0) || 1
  let done = 0
  for (const w of words) {
    const len = w.text.replace(/\s+/g, '').length
    if (t >= w.end_ms) done += len
    else {
      if (t > w.start_ms) done += len * ((t - w.start_ms) / Math.max(1, w.end_ms - w.start_ms))
      break
    }
  }
  return Math.min(1, done / total)
}
