// The two switches under every lyrics view, Pronunciation and Translation: one choice for Now
// Playing, the big screen, the desktop lyrics and the mini player, remembered per song language
// (Hindi songs can show pronunciation while Spanish songs show translation), starting from the
// defaults in Settings > Lyrics; and the language translations are into. Kept on this computer;
// every window hears a change. The per-window "Roman letters too" switch from before is where the
// Indian languages it covered begin.

import { useEffect, useState } from 'react'
import { langBase } from './lyricsModel.ts'
import type { SecondKind, SecondSwitches } from './secondLines.ts'

export interface SecondChoices {
  /** For a language never switched. */
  defaults: SecondSwitches
  /** Per song language (its base: "hi", "es"), only the switches touched. */
  byLang: Record<string, Partial<SecondSwitches>>
  /** The language translations are into: a language tag, or "system" (the computer's). */
  into: string
}

export const CHOICES_KEY = 'lyric-second-lines'
const OFF: SecondSwitches = { roman: false, translation: false }
const DEFAULT: SecondChoices = { defaults: OFF, byLang: {}, into: 'en' }
/** What "Roman letters too" covered: lines in U+0900 to U+0D7F, Devanagari to Malayalam. */
const INDIC_LANGS = ['hi', 'mr', 'ne', 'sa', 'bn', 'gu', 'pa', 'or', 'ta', 'te', 'kn', 'ml']
const OLD_LOOKS = ['lyric-look-stage', 'lyric-look-overlay', 'lyric-look']

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
function switches(v: unknown): Partial<SecondSwitches> {
  const out: Partial<SecondSwitches> = {}
  if (isObj(v)) for (const k of ['roman', 'translation'] as const) if (typeof v[k] === 'boolean') out[k] = v[k] as boolean
  return out
}

/** The choices from what's kept (`get` reads a key), field by field; with none kept yet, the old
 *  "Roman letters too" switch (either window's) for the Indian languages. */
export function readChoices(get: (key: string) => string | null): SecondChoices {
  let raw: string | null = null
  try {
    raw = get(CHOICES_KEY)
  } catch {
    raw = null
  }
  if (raw !== null) {
    try {
      const o = JSON.parse(raw) as unknown
      if (!isObj(o)) return DEFAULT
      const byLang: SecondChoices['byLang'] = {}
      if (isObj(o.byLang)) for (const [lang, v] of Object.entries(o.byLang)) byLang[lang] = switches(v)
      return { defaults: { ...OFF, ...switches(o.defaults) }, byLang, into: typeof o.into === 'string' && o.into ? o.into : 'en' }
    } catch {
      return DEFAULT
    }
  }
  const latin = OLD_LOOKS.some((key) => {
    try {
      const v = JSON.parse(get(key) ?? 'null') as unknown
      return isObj(v) && v.latin === true
    } catch {
      return false
    }
  })
  return latin ? { ...DEFAULT, byLang: Object.fromEntries(INDIC_LANGS.map((l) => [l, { roman: true }])) } : DEFAULT
}

/** The switches for a song in `lang` (null: a song whose language isn't known). */
export function choiceFor(c: SecondChoices, lang: string | null): SecondSwitches {
  return { ...c.defaults, ...(c.byLang[langBase(lang ?? 'und')] ?? {}) }
}

export function withChoice(c: SecondChoices, lang: string | null, kind: SecondKind, on: boolean): SecondChoices {
  const key = langBase(lang ?? 'und')
  return { ...c, byLang: { ...c.byLang, [key]: { ...c.byLang[key], [kind]: on } } }
}

export const withDefaults = (c: SecondChoices, defaults: SecondSwitches): SecondChoices => ({ ...c, defaults })

/** The language you read: the one chosen, or the computer's (English when it says none). */
export function readerLang(c: SecondChoices, system: string): string {
  if (c.into !== 'system') return langBase(c.into) || 'en'
  return langBase(system) || 'en'
}

/** A key's switch: T for pronunciation (as it was for Roman letters), Shift+T for translation. */
export function keyAction(e: Pick<KeyboardEvent, 'key' | 'shiftKey' | 'ctrlKey' | 'altKey' | 'metaKey'>): SecondKind | null {
  if (e.ctrlKey || e.altKey || e.metaKey || e.key.toLowerCase() !== 't') return null
  return e.shiftKey ? 'translation' : 'roman'
}

// --- Kept, and heard in every window -------------------------------------------------------

const stored = (key: string) => {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

let current: SecondChoices = typeof window === 'undefined' ? DEFAULT : readChoices(stored)
const listeners = new Set<() => void>()

export const secondChoices = () => current

export function setSecondChoices(next: SecondChoices) {
  current = next
  try {
    localStorage.setItem(CHOICES_KEY, JSON.stringify(next))
  } catch {
    // Not kept (private storage): it applies until the window closes.
  }
  listeners.forEach((l) => l())
}

/** Switch one row on or off for songs in `lang`. */
export const setSecondLine = (lang: string | null, kind: SecondKind, on: boolean) => setSecondChoices(withChoice(current, lang, kind, on))

if (typeof window !== 'undefined')
  window.addEventListener('storage', (e) => {
    if (e.key !== CHOICES_KEY) return
    current = readChoices(stored)
    listeners.forEach((l) => l())
  })

export function useSecondChoices(): SecondChoices {
  const [, tick] = useState(0)
  useEffect(() => {
    const l = () => tick((n) => n + 1)
    listeners.add(l)
    return () => {
      listeners.delete(l)
    }
  }, [])
  return current
}

/** The computer's language (the webview's). */
export const systemLang = () => (typeof navigator === 'undefined' ? '' : navigator.language || '')
