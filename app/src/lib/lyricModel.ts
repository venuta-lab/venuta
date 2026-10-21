// The lyrics model every package builds against (docs/design/lyrics-second-line.md, "The
// contract"): each line as sung, with up to two second lines under it, pronunciation and
// translation, each saying where it came from. The engine (Rust) works them out once per song;
// until it sends them, today's lyrics are read into the same shape here (`fromLegacy`), Roman
// letters for Indian scripts made by rule once per song. Pure, so tests run it as is.

import { toLatin } from './textsearch.ts'
import type { Lyrics } from './lyrics'

/** A word and when it is sung. */
export interface Word {
  start_ms: number
  end_ms: number
  text: string
}

/**
 * Where a second line came from, as serde writes the engine's enum: the lyrics themselves (and
 * which provider), another copy of the lyrics paired line by line, made on this device, a
 * translation engine (and which), or corrected by hand.
 */
export type SecondSource = { Lyrics: string } | { Paired: string } | 'Made' | { Translated: string } | 'Corrected'

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
  /** Detected: Deva, Guru, Arab, Hira, Hang, Hani, Cyrl, Latn, ... (ISO 15924). */
  script: string
  /** Pronunciation. */
  roman: Second | null
  /** Meaning. */
  translation: Second | null
}

/** The contract's `Lyrics { lines, lang, version }` (named so beside today's `Lyrics`). */
export interface LyricsModel {
  lines: LyricLine[]
  lang: string | null
  version: number
}

// --- Reading what the engine sends ----------------------------------------------------------

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
const num = (v: unknown, or = 0) => (typeof v === 'number' && Number.isFinite(v) ? v : or)
const str = (v: unknown, or = '') => (typeof v === 'string' ? v : or)

function readWords(v: unknown): Word[] {
  if (!Array.isArray(v)) return []
  return v.filter(isObj).map((w) => ({ start_ms: num(w.start_ms ?? w.ms), end_ms: num(w.end_ms), text: str(w.text) }))
}

function readSecond(v: unknown): Second | null {
  if (!isObj(v) || typeof v.text !== 'string') return null
  return { text: v.text, words: readWords(v.words), lang: str(v.lang, 'und'), source: (v.source ?? 'Made') as SecondSource }
}

/** The engine's answer as the model: an Option left out (skip_serializing_if) reads as none. */
export function readModel(v: unknown): LyricsModel {
  const o = isObj(v) ? v : {}
  const lines = Array.isArray(o.lines) ? o.lines.filter(isObj) : []
  return {
    lines: lines.map((l) => ({
      start_ms: num(l.start_ms ?? l.ms),
      end_ms: num(l.end_ms),
      text: str(l.text),
      words: readWords(l.words),
      script: str(l.script) || scriptOf(str(l.text)),
      roman: readSecond(l.roman),
      translation: readSecond(l.translation),
    })),
    lang: typeof o.lang === 'string' && o.lang ? o.lang : null,
    version: num(o.version, 1),
  }
}

// --- Where a second line came from ---------------------------------------------------------

export type SourceKind = 'Lyrics' | 'Paired' | 'Made' | 'Translated' | 'Corrected'
const KINDS: SourceKind[] = ['Lyrics', 'Paired', 'Made', 'Translated', 'Corrected']
const kindOf = (s: string): SourceKind | undefined => KINDS.find((k) => k.toLowerCase() === s.toLowerCase())

/** A source in one shape, however it was written: `"Made"`, `{ "Lyrics": "netease" }` (serde's
 *  own), or `{ kind, provider | engine }` (a tagged enum). Anything else is the lyrics'. */
export function sourceKind(source: SecondSource): { kind: SourceKind; by: string | null } {
  const s: unknown = source
  if (typeof s === 'string') return { kind: kindOf(s) ?? 'Lyrics', by: null }
  if (isObj(s)) {
    const tagged = typeof s.kind === 'string' ? kindOf(s.kind) : undefined
    if (tagged) {
      const by = s.provider ?? s.engine ?? s.by ?? null
      return { kind: tagged, by: typeof by === 'string' ? by : null }
    }
    const [key, value] = Object.entries(s)[0] ?? []
    const kind = key ? kindOf(key) : undefined
    if (kind) return { kind, by: typeof value === 'string' ? value : null }
  }
  return { kind: 'Lyrics', by: null }
}

/** Engines that run with the listener's own key (L7), by their names. */
const KEYED: Record<string, string> = { deepl: 'DeepL', microsoft: 'Microsoft', google: 'Google', llm: 'a language model' }

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

/** The scripts told apart, each with the language a song in it most likely is. */
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
  ['Hira', /[\p{Script=Hiragana}\p{Script=Katakana}]/u, 'ja'],
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
  if (counts.get('Hira') && counts.get('Hani')) {
    counts.set('Hira', (counts.get('Hira') ?? 0) + (counts.get('Hani') ?? 0))
    counts.delete('Hani')
  }
  let best = 'Zyyy'
  let most = 0
  for (const [code, n] of counts) if (n > most) [best, most] = [code, n]
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

/** The song's language: as the lyrics say, else the likely one for its script (none for songs in
 *  Latin letters, which could be any). */
export function songLang(m: LyricsModel | null): string | null {
  if (!m) return null
  if (m.lang) return langBase(m.lang)
  const counts = new Map<string, number>()
  for (const l of m.lines) countScripts(l.text, counts)
  const code = dominant(counts)
  return SCRIPTS.find(([c]) => c === code)?.[2] ?? null
}

// --- Today's lyrics in the contract's shape ------------------------------------------------

const INDIC = /[ऀ-ൿ]/
/** How long the last line is held when nothing comes after it. */
const LAST_LINE_MS = 5000

const once = new WeakMap<Lyrics, LyricsModel>()

/**
 * Today's lyrics (parallel arrays: lines, words, translations) as the contract: Roman letters for
 * Indian-script lines made by rule (timed by their words, which the rule keeps one for one), a
 * source's translation with its language read from its letters. Worked out once per song; lyrics
 * that already carry the engine's model pass through as they are.
 */
export function fromLegacy(l: Lyrics): LyricsModel {
  if (l.model) return l.model
  const known = once.get(l)
  if (known) return known
  const provider = l.found?.source ?? 'file'
  const lines: LyricLine[] = l.lines.map(([ms, text], i) => {
    const words = (l.words?.[i] ?? []).map(([s, e, t]) => ({ start_ms: s, end_ms: e, text: t }))
    const script = scriptOf(text)
    const roman: Second | null =
      text && INDIC.test(text)
        ? {
            text: toLatin(text),
            words: words.map((w) => ({ ...w, text: toLatin(w.text) })),
            lang: `${SCRIPTS.find(([c]) => c === script)?.[2] ?? 'hi'}-Latn`,
            source: 'Made',
          }
        : null
    const t = l.translations?.[i]
    const tScript = t ? scriptOf(t) : ''
    const translation: Second | null = t ? { text: t, words: [], lang: SCRIPTS.find(([c]) => c === tScript)?.[2] ?? 'und', source: { Lyrics: provider } } : null
    return { start_ms: ms, end_ms: l.lines[i + 1]?.[0] ?? ms + LAST_LINE_MS, text, words, script, roman, translation }
  })
  const model: LyricsModel = { lines, lang: null, version: 0 }
  model.lang = songLang(model)
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
