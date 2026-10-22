// Translation on this device: the models (one per language pair, downloaded only when asked,
// from the model's own publisher, run on this PC; nothing is sent anywhere) and the way to ask
// for a song's translation. The engine's side is the translation package's (L5); until its
// commands exist there are no models, and Translation shows only what the lyrics carry.
//
// The commands it fills (crates/lyrics-translate, through the Tauri app):
//   translation_models                     -> TranslationModel[]
//   translation_model_download { id }      -> ()  (progress through translation_models)
//   translation_model_remove { id }        -> ()
//   lyrics_translate { key, from, into }   -> ()  (then the song's lyrics are read again, with
//                                                 translation lines Translated(engine))

import { invoke } from '@tauri-apps/api/core'
import { useEffect, useState } from 'react'
import { langBase } from './lyricsModel.ts'

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
}

/** What the page talks to: the engine's commands, or a stand-in (UI previews, tests). */
export interface TranslationBackend {
  models(): Promise<TranslationModel[]>
  download(id: string): Promise<void>
  remove(id: string): Promise<void>
  translate(key: string, from: string, into: string): Promise<void>
}

const inApp = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

/** The engine's commands; none there yet (or outside the app), no models. */
const engine: TranslationBackend = {
  models: () => (inApp() ? invoke<TranslationModel[]>('translation_models').catch(() => []) : Promise.resolve([])),
  download: (id) => invoke('translation_model_download', { id }),
  remove: (id) => invoke('translation_model_remove', { id }),
  translate: (key, from, into) => invoke('lyrics_translate', { key, from, into }),
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
export const translateSong = (key: string, from: string, into: string) => backend.translate(key, from, into)

/** The models, read again after a download or removal (and every second while one downloads). */
export function useTranslationModels(): TranslationModel[] {
  const [models, setModels] = useState<TranslationModel[]>([])
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
    void translationModels().then((m) => live && setModels(m))
    return () => {
      live = false
    }
  }, [version])
  const downloading = models.some((m) => m.progress !== null)
  useEffect(() => {
    if (!downloading) return
    const t = window.setInterval(() => setVersion((v) => v + 1), 1000)
    return () => window.clearInterval(t)
  }, [downloading])
  return models
}

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
  | { kind: 'translating'; model: TranslationModel }
  | { kind: 'none' }

/** What Translation can do for a song: `have` is whether its lines carry one in your language. */
export function translationStatus(a: { have: boolean; on: boolean; from: string | null; to: string; models: TranslationModel[] }): TranslationStatus {
  if (!a.on) return { kind: 'off' }
  if (a.have) return { kind: 'ready' }
  const m = modelFor(a.models, a.from, a.to)
  if (!m) return { kind: 'none' }
  if (m.progress !== null) return { kind: 'downloading', model: m }
  return m.installed ? { kind: 'translating', model: m } : { kind: 'needs-model', model: m }
}

/** Language names in English, for the few places a language is named. */
export function languageName(tag: string): string {
  try {
    return new Intl.DisplayNames(['en'], { type: 'language' }).of(langBase(tag)) ?? tag
  } catch {
    return tag
  }
}
