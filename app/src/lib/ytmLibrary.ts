import { listen } from '@tauri-apps/api/event'
import { useEffect, useSyncExternalStore } from 'react'
import { inApp, ytmLibrary, ytmLibrarySongs, ytmStatus, type OnlineSuggestion, type YtmShelf, type YtmStatus, type YtmTrack } from './engine'
import { onlineProblem } from './online'
import { isLocalOnly, useLocalOnly } from './localOnly'

/*
  The signed-in YouTube Music account's library, as Library shows it beside your own music: each
  part read once when it is first shown, kept until the account changes or something is changed
  on it, then read again. Signed out, or with Local only on, there is nothing to read.
*/

export type YtmPart = YtmShelf | 'songs' | 'upload_songs'
type Value = OnlineSuggestion[] | YtmTrack[]
interface Entry {
  value: Value | null
  problem: string
  loading: boolean
}

const parts = new Map<YtmPart, Entry>()
const listeners = new Set<() => void>()
let version = 0
const changed = () => {
  version++
  listeners.forEach((l) => l())
}
const subscribe = (l: () => void) => {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}

function load(part: YtmPart) {
  const was = parts.get(part)
  if (was?.loading) return
  parts.set(part, { value: was?.value ?? null, problem: '', loading: true })
  changed()
  const ask: Promise<Value> = part === 'songs' ? ytmLibrarySongs(false) : part === 'upload_songs' ? ytmLibrarySongs(true) : ytmLibrary(part)
  ask
    .then((value) => parts.set(part, { value, problem: '', loading: false }))
    .catch((e) => parts.set(part, { value: was?.value ?? [], problem: onlineProblem(e), loading: false }))
    .finally(() => {
      changed()
      void refreshStatus()
    })
}

/** Something changed on the account (a save, a follow, a playlist made): these parts are read again. */
export function ytmLibraryChanged(which: YtmPart[]) {
  for (const p of which) {
    if (parts.has(p)) load(p)
  }
}

// --- Signed in, and whether YouTube still takes it ----------------------------------------------

let status: YtmStatus = { signed_in: false, expired: false }
const statusListeners = new Set<() => void>()
async function refreshStatus() {
  if (!inApp()) return
  const next = await ytmStatus().catch(() => status)
  if (next.signed_in !== status.signed_in || next.expired !== status.expired) {
    status = next
    statusListeners.forEach((l) => l())
  }
}
if (inApp()) {
  void refreshStatus()
  void listen('ytm-account-changed', () => {
    // Another account (or none): everything read for the last one is forgotten.
    parts.clear()
    changed()
    void refreshStatus()
  })
}

/** Signed in to YouTube Music (and not in Local only), and whether the sign-in still works. */
export function useYtmStatus(): YtmStatus {
  const localOnly = useLocalOnly()
  const s = useSyncExternalStore(
    (l) => {
      statusListeners.add(l)
      return () => {
        statusListeners.delete(l)
      }
    },
    () => status,
  )
  return localOnly ? { signed_in: false, expired: false } : s
}

/** A part of the account's library: read the first time it shows (signed in only). */
export function useYtmPart<T extends Value>(part: YtmPart, wanted = true): { value: T | null; problem: string; loading: boolean } {
  const { signed_in } = useYtmStatus()
  useSyncExternalStore(subscribe, () => version)
  const on = wanted && signed_in
  useEffect(() => {
    if (on && !parts.has(part)) load(part)
  }, [on, part])
  const e = on ? parts.get(part) : undefined
  return { value: (e?.value as T | null) ?? null, problem: e?.problem ?? '', loading: !!e?.loading }
}

/** How many a part holds once read (for a chip's count), or null before. */
export function useYtmCount(part: YtmPart, wanted = true): number | null {
  const { value } = useYtmPart(part, wanted)
  return value ? value.length : null
}

/** Signed in, the sign-in still taken, and not in Local only: account work can go ahead. */
export function ytmReady(): boolean {
  return status.signed_in && !status.expired && !isLocalOnly()
}

/** Ask again whether the sign-in still works (after a request said it doesn't). */
export const ytmStatusRefresh = () => refreshStatus()
