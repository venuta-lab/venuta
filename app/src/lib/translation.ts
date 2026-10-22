// Translation, when the lyrics carry none in your language:
// - on this device: the models (one per language pair, downloaded only when the listener clicks
//   Download, from Mozilla, the models' publisher, and run on this PC; nothing is sent anywhere),
//   offered only for the song languages it translated well in the tests (Spanish, French);
// - with your own key (DeepL, Microsoft, Google, or AI: Claude): the key kept in the system's
//   secret store, a song sent only when you press Translate, or for every song once you turn on
//   "Translate automatically with my key" (off at first).
//
// The engine's commands (crates/lyrics-translate, through engine::translation):
//   translation_models                     -> TranslationModel[]
//   translation_model_download { id }      -> ()  (returns at once; progress through
//                                                 translation_models, a failure in `error`)
//   translation_model_remove { id }        -> ()  (also stops a download)
//   lyrics_translate { key, from, into, engine?, model? }
//                                          -> ()  (then the song's lyrics are read again, with
//                                                 translation lines Translated("bergamot"),
//                                                 Translated("deepl"), ...; asks for the same song
//                                                 at once share one run; an error is a sentence)
//   translation_keys                       -> KeyState[] (whether a key is kept, never the key)
//   translation_key_save { engine, key, region? } / translation_key_forget { engine }
//   translation_key_test { engine, key?, region?, model?, into } -> [line, what came back]

import { invoke } from '@tauri-apps/api/core'
import { useEffect, useState } from 'react'
import { langBase } from './lyricsModel.ts'
import { ENGINES, isKeyed, type EngineChoice, type EngineId, type KeyedId } from './translationEngine.ts'

export interface TranslationModel {
  id: string
  /** The languages it translates from and into (BCP 47). */
  from: string
  to: string
  /** "Hindi to English". */
  name: string
  size_bytes: number
  installed: boolean
  /** 0 to 1 while downloading, else null. */
  progress: number | null
  /** Why its last download failed (null: it didn't). */
  error?: string | null
  /** Whether it is offered for songs: only where it kept the meaning of lyrics in the tests. */
  offered?: boolean
}

/** A key-based engine's key, as the settings see it: kept or not, never the key itself. */
export interface KeyState {
  engine: KeyedId
  name: string
  kept: boolean
  region?: string | null
  /** DeepL: "Free" or "Pro". */
  plan?: string | null
}

/** What the page talks to: the engine's commands, or a stand-in (UI previews, tests). */
export interface TranslationBackend {
  models(): Promise<TranslationModel[]>
  download(id: string): Promise<void>
  remove(id: string): Promise<void>
  translate(key: string, from: string, into: string, engine?: { engine: string; model?: string }): Promise<void>
  keys(): Promise<KeyState[]>
  saveKey(engine: KeyedId, key: string, region?: string | null): Promise<void>
  forgetKey(engine: KeyedId): Promise<void>
  testKey(engine: KeyedId, a: { key?: string; region?: string | null; model?: string; into: string }): Promise<[string, string]>
}

const inApp = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

/** The engine's commands; outside the app, no models and no keys. */
const engine: TranslationBackend = {
  models: () => (inApp() ? invoke<TranslationModel[]>('translation_models').catch(() => []) : Promise.resolve([])),
  download: (id) => invoke('translation_model_download', { id }),
  remove: (id) => invoke('translation_model_remove', { id }),
  translate: (key, from, into, e) => invoke('lyrics_translate', { key, from, into, engine: e?.engine ?? null, model: e?.model ?? null }),
  keys: () => (inApp() ? invoke<KeyState[]>('translation_keys').catch(() => []) : Promise.resolve([])),
  saveKey: (engine, key, region) => invoke('translation_key_save', { engine, key, region: region ?? null }),
  forgetKey: (engine) => invoke('translation_key_forget', { engine }),
  testKey: (engine, a) => invoke<[string, string]>('translation_key_test', { engine, key: a.key ?? null, region: a.region ?? null, model: a.model ?? null, into: a.into }),
}

let backend: TranslationBackend = engine
const listeners = new Set<() => void>()
const changed = () => listeners.forEach((l) => l())

/** Use a stand-in (UI previews). */
export function setTranslationBackend(b: TranslationBackend | null) {
  backend = b ?? engine
  changed()
}

export const translationModels = () => backend.models()
export const downloadModel = (id: string) => backend.download(id).finally(changed)
export const removeModel = (id: string) => backend.remove(id).finally(changed)
export const translateSong = (key: string, from: string, into: string, e?: { engine: string; model?: string }) => backend.translate(key, from, into, e)
export const translationKeys = () => backend.keys()
export const saveKey = (e: KeyedId, key: string, region?: string | null) => backend.saveKey(e, key, region).finally(changed)
export const forgetKey = (e: KeyedId) => backend.forgetKey(e).finally(changed)
export const testKey = (e: KeyedId, a: { key?: string; region?: string | null; model?: string; into: string }) => backend.testKey(e, a)

/** Read again whenever a model or a key changes (and every second while a model downloads). */
function useListened<T>(read: () => Promise<T[]>, busy: (v: T[]) => boolean = () => false): T[] {
  const [items, setItems] = useState<T[]>([])
  const [version, setVersion] = useState(0)
  useEffect(() => {
    const l = () => setVersion((v) => v + 1)
    listeners.add(l)
    return () => {
      listeners.delete(l)
    }
  }, [])
  useEffect(() => {
    let live = true
    void read().then((m) => live && setItems(m))
    return () => {
      live = false
    }
    // `read` is one of the module's own readers, the same every time.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [version])
  const waiting = busy(items)
  useEffect(() => {
    if (!waiting) return
    const t = window.setInterval(() => setVersion((v) => v + 1), 1000)
    return () => window.clearInterval(t)
  }, [waiting])
  return items
}

/** The models. */
export const useTranslationModels = (): TranslationModel[] => useListened(translationModels, (m) => m.some((x) => x.progress !== null))

/** The key-based engines' keys (kept or not). */
export const useTranslationKeys = (): KeyState[] => useListened(translationKeys)

/** The model that translates a song in `from` into `to`, if there is one. */
export function modelFor(models: TranslationModel[], from: string | null, to: string): TranslationModel | null {
  if (!from) return null
  return models.find((m) => langBase(m.from) === langBase(from) && langBase(m.to) === langBase(to)) ?? null
}

/** A size as people read it: "45 MB", "1.5 GB". */
export function sizeWords(bytes: number): string {
  const mb = bytes / 1048576
  if (mb >= 1024) return `${Math.round((mb / 1024) * 10) / 10} GB`
  return `${Math.max(1, Math.round(mb))} MB`
}

export type TranslationStatus =
  | { kind: 'off' }
  | { kind: 'ready' }
  | { kind: 'needs-model'; model: TranslationModel }
  | { kind: 'downloading'; model: TranslationModel }
  | { kind: 'translating'; engine: EngineId; model?: TranslationModel }
  | { kind: 'failed'; engine: EngineId; error: string }
  /** On this device: a model there is, but it got this language's lyrics wrong in the tests. */
  | { kind: 'unreliable' }
  /** On this device: no model for this language. */
  | { kind: 'none' }
  /** With your key: none entered yet. */
  | { kind: 'needs-key'; engine: KeyedId }
  /** With your key: waits for you to press Translate. */
  | { kind: 'ask'; engine: KeyedId }

/**
 * What Translation can do for a song. `have`: its lines carry one in your language. `engine`: the
 * engine chosen in Settings; `keyKept`: a key is kept for it; `asked`: you pressed Translate for
 * this song; `error`: why the last try failed.
 */
export function translationStatus(a: {
  have: boolean
  on: boolean
  from: string | null
  to: string
  models: TranslationModel[]
  engine: EngineChoice
  keyKept: boolean
  asked: boolean
  error: string | null
}): TranslationStatus {
  if (!a.on) return { kind: 'off' }
  if (a.have) return { kind: 'ready' }
  const e = a.engine.engine
  if (isKeyed(e)) {
    if (!a.keyKept) return { kind: 'needs-key', engine: e }
    if (a.error) return { kind: 'failed', engine: e, error: a.error }
    return a.engine.auto || a.asked ? { kind: 'translating', engine: e } : { kind: 'ask', engine: e }
  }
  const m = modelFor(a.models, a.from, a.to)
  if (!m) return { kind: 'none' }
  if (m.offered === false) return { kind: 'unreliable' }
  if (a.error) return { kind: 'failed', engine: 'device', error: a.error }
  if (m.progress !== null) return { kind: 'downloading', model: m }
  return m.installed ? { kind: 'translating', engine: 'device', model: m } : { kind: 'needs-model', model: m }
}

const KEY_ENGINES = 'DeepL, Microsoft, Google or AI with your own key'

/** What a status says to the listener, in a sentence (empty: nothing to say). `device`: "PC". */
export function statusWords(s: TranslationStatus, from: string | null, into: string, device = 'PC'): string {
  const lang = from ? languageName(from) : ''
  switch (s.kind) {
    case 'unreliable':
      return `Translation on this ${device} got ${lang ? `${lang} lyrics` : 'songs in this language'} wrong too often in our tests, so it isn't offered: choose ${KEY_ENGINES} in Settings > Lyrics.`
    case 'none':
      return `No translation on this ${device} for ${lang ? `${lang} songs` : 'songs in this language'}: ${KEY_ENGINES} can translate them (Settings > Lyrics).`
    case 'needs-model':
      return `Translating ${lang || 'this language'} into ${languageName(into)} needs its model on this ${device} (${sizeWords(s.model.size_bytes)}, downloaded once from its publisher; nothing is sent anywhere).`
    case 'downloading':
      return `Downloading the ${lang} model, ${Math.round((s.model.progress ?? 0) * 100)}%`
    case 'translating':
      return isKeyed(s.engine) ? `Translating with ${ENGINES[s.engine].name}…` : `Translating on this ${device}…`
    case 'failed':
      return s.error
    case 'needs-key':
      return `Translating with ${ENGINES[s.engine].name} needs your key: add it in Settings > Lyrics.`
    case 'ask':
      return `Sends this song's lyrics to ${ENGINES[s.engine].vendor} and uses your quota.`
    default:
      return ''
  }
}

/** Language names in English, for the few places a language is named. */
export function languageName(tag: string): string {
  try {
    return new Intl.DisplayNames(['en'], { type: 'language' }).of(langBase(tag)) ?? tag
  } catch {
    return tag
  }
}

// --- A song's runs: pressed, running, failed -----------------------------------------------

/** What has been asked in this window, by `songKey|from|into|engine|model`. */
const pressed = new Set<string>()
const running = new Set<string>()
const done = new Set<string>()
const failures = new Map<string, string>()
const runListeners = new Set<() => void>()
const runsChanged = () => runListeners.forEach((l) => l())

export const runKey = (key: string, from: string, into: string, e: { engine: string; model?: string }) => [key, from, into, e.engine, e.model ?? ''].join('|')

/** You pressed Translate (or Try again) for a song. */
export function pressTranslate(run: string) {
  pressed.add(run)
  failures.delete(run)
  done.delete(run)
  runsChanged()
}

/** Runs a song's translation once (asks again only after a failure was cleared). */
export function startRun(run: string, go: () => Promise<void>, after: () => void) {
  if (running.has(run) || done.has(run) || failures.has(run)) return
  running.add(run)
  void go()
    .then(() => {
      done.add(run)
      after()
    })
    .catch((e: unknown) => failures.set(run, typeof e === 'string' ? e : e instanceof Error ? e.message : 'The translation failed.'))
    .finally(() => {
      running.delete(run)
      runsChanged()
    })
}

/** A run's state for the views: pressed, and its error. */
export function useRun(run: string): { asked: boolean; error: string | null } {
  const [, tick] = useState(0)
  useEffect(() => {
    const l = () => tick((n) => n + 1)
    runListeners.add(l)
    return () => {
      runListeners.delete(l)
    }
  }, [])
  return { asked: pressed.has(run), error: failures.get(run) ?? null }
}
