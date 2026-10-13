import { Channel, invoke } from '@tauri-apps/api/core'
import { useSyncExternalStore } from 'react'
import { inApp } from './engine'

/*
  New versions of Venuta: one store for the whole app, so the quiet check at start, the notice
  and Settings › About all agree on what was found and what is happening.
*/
export interface UpdateInfo {
  version: string
  current: string
  notes: string | null
  date: string | null
}

export type UpdateState =
  | { at: 'idle' }
  | { at: 'checking' }
  | { at: 'current'; checked: number }
  | { at: 'found'; info: UpdateInfo }
  | { at: 'downloading'; info: UpdateInfo; done: number; total: number | null }
  | { at: 'failed'; message: string }

let state: UpdateState = { at: 'idle' }
const listeners = new Set<() => void>()
const set = (s: UpdateState) => {
  state = s
  listeners.forEach((l) => l())
}

export function useUpdates(): UpdateState {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => {
        listeners.delete(l)
      }
    },
    () => state,
  )
}

/** Ask GitHub for a newer version. */
export async function checkForUpdate(): Promise<UpdateInfo | null> {
  if (!inApp()) return null
  set({ at: 'checking' })
  try {
    const info = await invoke<UpdateInfo | null>('update_check')
    set(info ? { at: 'found', info } : { at: 'current', checked: Date.now() })
    remember({ last: Date.now() })
    return info
  } catch (e) {
    set({ at: 'failed', message: friendly(String(e)) })
    return null
  }
}

/** Download (signature checked), install and restart. Venuta closes while it installs. */
export async function installUpdate() {
  const s = state
  if (s.at !== 'found') return
  const progress = new Channel<{ done: number; total: number | null }>()
  progress.onmessage = (p) => set({ at: 'downloading', info: s.info, done: p.done, total: p.total })
  set({ at: 'downloading', info: s.info, done: 0, total: null })
  try {
    await invoke('update_install', { onProgress: progress })
    // A test copy stops after the checked download; a real one has restarted by now.
    set({ at: 'found', info: s.info })
  } catch (e) {
    set({ at: 'failed', message: friendly(String(e)) })
  }
}

function friendly(e: string): string {
  if (/signature|signed for version|tampered/i.test(e)) return "The download didn't match Venuta's signature for this version, so it was not installed."
  if (/(404|not found|status code)/i.test(e)) return 'No published version was found on GitHub yet.'
  if (/(dns|connect|timed out|timeout|network|resolve)/i.test(e)) return "Couldn't reach GitHub. Check the internet connection and try again."
  return e.replace(/^Error: /, '')
}

// The quiet check when Venuta starts: on unless switched off, at most once a day.
const KEY = 'venuta.updates'
type Saved = { auto: boolean; last: number }
export function savedUpdates(): Saved {
  try {
    return { auto: true, last: 0, ...JSON.parse(localStorage.getItem(KEY) ?? '{}') }
  } catch {
    return { auto: true, last: 0 }
  }
}
export function remember(change: Partial<Saved>) {
  try {
    localStorage.setItem(KEY, JSON.stringify({ ...savedUpdates(), ...change }))
  } catch {
    /* a convenience only */
  }
}

/** At start: check if it's on and a day has passed. Resolves to what was found, if anything. */
export async function checkAtStart(): Promise<UpdateInfo | null> {
  const s = savedUpdates()
  if (!s.auto || Date.now() - s.last < 24 * 3600 * 1000) return null
  const info = await checkForUpdate()
  // A failed quiet check stays quiet: Settings shows it only when you look.
  if (!info && state.at === 'failed') set({ at: 'idle' })
  return info
}
