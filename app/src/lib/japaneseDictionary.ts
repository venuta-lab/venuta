// The Japanese dictionary (kanji readings for the pronunciation line): the engine fetches it in
// the background the first time a song's lyrics wait for it, and tells every window how it goes
// (`venuta://japanese-dictionary`: {state, done, total, error}; app/src-tauri/src/
// japanese_dictionary.rs). The lyrics views say so quietly while it comes, and once it is ready
// each window reads its lyrics again, so their Japanese lines get a pronunciation.

import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'

export interface DictionaryStatus {
  state: 'downloading' | 'checking' | 'unpacking' | 'ready' | 'failed'
  done: number
  total: number
  error?: string
}

/** What the views say about it, or nothing (not asked for, or ready). */
export function dictionaryWords(s: DictionaryStatus | null): string | null {
  if (!s) return null
  switch (s.state) {
    case 'downloading': {
      const pct = s.total > 0 ? `, ${Math.min(100, Math.round((s.done / s.total) * 100))}%` : ''
      return `Getting the Japanese dictionary for pronunciation${pct}`
    }
    case 'checking':
    case 'unpacking':
      return 'Setting up the Japanese dictionary for pronunciation'
    case 'failed':
      return "The Japanese dictionary for pronunciation couldn't be fetched. It is tried again with the next Japanese song."
    case 'ready':
      return null
  }
}

const EVENT = 'venuta://japanese-dictionary'
let status: DictionaryStatus | null = null
const listeners = new Set<() => void>()
let watching = false

/** Follow the engine's word on it, once per window; `ready` is told when it has come. */
export function watchJapaneseDictionary(ready: () => void) {
  if (watching || typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return
  watching = true
  void listen<DictionaryStatus>(EVENT, (e) => {
    status = e.payload
    if (status.state === 'ready') ready()
    listeners.forEach((l) => l())
  })
}

/** Use a stand-in status (UI previews). */
export function setDictionaryStatus(s: DictionaryStatus | null) {
  status = s
  listeners.forEach((l) => l())
}

/** What the dictionary is doing now (null: nothing to say). */
export function useJapaneseDictionary(): DictionaryStatus | null {
  const [, tick] = useState(0)
  useEffect(() => {
    const l = () => tick((n) => n + 1)
    listeners.add(l)
    return () => {
      listeners.delete(l)
    }
  }, [])
  return status
}
