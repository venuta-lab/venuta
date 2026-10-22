// Which engine translates lyrics when the lyrics carry no translation in your language: on this
// PC (offline, offered only for the song languages it translated well), or a service with your
// own key (DeepL, Microsoft, Google, or AI: Claude). A key-based engine runs only when you press
// Translate for a song, or for every song once you turn on "Translate automatically with my key"
// (off at first: it uses your quota). Kept on this computer; every window hears a change. The
// keys themselves are never here: they are kept in the system's secret store by the engine.

import { useEffect, useState } from 'react'

export type EngineId = 'device' | 'deepl' | 'microsoft' | 'google' | 'llm'
export type KeyedId = Exclude<EngineId, 'device'>

export interface EngineChoice {
  engine: EngineId
  /** Translate every song with Translation on, with your key, without pressing Translate. */
  auto: boolean
  /** The language model's id (AI with your key). */
  model: string
}

export const DEFAULT_MODEL = 'claude-sonnet-5-5'
export const ENGINE_KEY = 'lyric-translation-engine'
export const DEFAULT_ENGINE: EngineChoice = { engine: 'device', auto: false, model: DEFAULT_MODEL }

export interface EngineInfo {
  /** In the picker. */
  label: string
  /** In a sentence: "Translating with DeepL". */
  name: string
  /** Who the song's lyrics go to. */
  vendor: string
  /** The free allowance, as the service states it. */
  quota: string
  /** Where a key comes from. */
  keyHelp: string
}

export const ENGINES: Record<EngineId, EngineInfo> = {
  device: { label: 'On this PC (offline)', name: 'this PC', vendor: '', quota: '', keyHelp: '' },
  deepl: {
    label: 'DeepL, with your key',
    name: 'DeepL',
    vendor: 'DeepL',
    quota: 'DeepL API Free includes 500,000 characters a month; a Free key ends in :fx. Pro keys are billed by DeepL.',
    keyHelp: 'Your DeepL API key, from your DeepL account.',
  },
  microsoft: {
    label: 'Microsoft Translator, with your key',
    name: 'Microsoft Translator',
    vendor: 'Microsoft',
    quota: "Azure Translator's free tier includes 2 million characters a month.",
    keyHelp: "A key of your Azure Translator resource, and the resource's region (leave it empty for a global resource).",
  },
  google: {
    label: 'Google Cloud Translation, with your key',
    name: 'Google Cloud Translation',
    vendor: 'Google',
    quota: 'The first 500,000 characters a month are free; Google bills the rest to your Cloud account.',
    keyHelp: 'An API key of a Google Cloud project with the Cloud Translation API turned on.',
  },
  llm: {
    label: 'AI (Claude), with your key',
    name: 'Claude',
    vendor: 'Anthropic',
    quota: 'Anthropic bills each translation to your API account, a small amount per song.',
    keyHelp: 'An API key from your Anthropic Console account.',
  },
}

export const ENGINE_IDS = Object.keys(ENGINES) as EngineId[]
export const isKeyed = (e: EngineId): e is KeyedId => e !== 'device'

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)

/** The choice from what's kept (`get` reads a key), field by field. */
export function readEngine(get: (key: string) => string | null): EngineChoice {
  try {
    const o = JSON.parse(get(ENGINE_KEY) ?? 'null') as unknown
    if (!isObj(o)) return DEFAULT_ENGINE
    const engine = ENGINE_IDS.includes(o.engine as EngineId) ? (o.engine as EngineId) : 'device'
    const model = typeof o.model === 'string' && o.model.trim() ? o.model.trim() : DEFAULT_MODEL
    return { engine, auto: o.auto === true, model }
  } catch {
    return DEFAULT_ENGINE
  }
}

/** What the engine is asked: the engine's name, and the model for AI. */
export function engineArg(c: EngineChoice): { engine: string; model?: string } {
  return c.engine === 'llm' ? { engine: 'llm', model: c.model } : { engine: c.engine }
}

// --- Kept, and heard in every window -------------------------------------------------------

const stored = (key: string) => {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

let current: EngineChoice = typeof window === 'undefined' ? DEFAULT_ENGINE : readEngine(stored)
const listeners = new Set<() => void>()

export const engineChoice = () => current

export function setEngineChoice(next: EngineChoice) {
  current = next
  try {
    localStorage.setItem(ENGINE_KEY, JSON.stringify(next))
  } catch {
    // Not kept (private storage): it applies until the window closes.
  }
  listeners.forEach((l) => l())
}

if (typeof window !== 'undefined')
  window.addEventListener('storage', (e) => {
    if (e.key !== ENGINE_KEY) return
    current = readEngine(stored)
    listeners.forEach((l) => l())
  })

export function useEngineChoice(): EngineChoice {
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
