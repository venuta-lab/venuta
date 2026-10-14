import { useSyncExternalStore } from 'react'

/*
  The online catalog's two plain facts for the window: is this computer online at all, and
  what to say when a catalog call fails. The backend already words its own failures for people
  ("The online catalog can't be reached right now."); anything else becomes one calm line.
*/

export const OFFLINE = 'You’re offline.'
const UNREACHABLE = 'The online catalog can’t be reached right now.'

/** Is the computer connected (as far as the window can tell)? */
export const isNetworkOnline = () => typeof navigator === 'undefined' || navigator.onLine !== false

const subscribe = (on: () => void) => {
  window.addEventListener('online', on)
  window.addEventListener('offline', on)
  return () => {
    window.removeEventListener('online', on)
    window.removeEventListener('offline', on)
  }
}

/** Connected or not, following the system as it changes. */
export const useNetworkOnline = () => useSyncExternalStore(subscribe, isNetworkOnline, () => true)

/** One short, human line for a failed online catalog call. */
export function onlineProblem(e: unknown): string {
  if (!isNetworkOnline()) return OFFLINE
  const raw = (e instanceof Error ? e.message : String(e ?? '')).replace(/^error:\s*/i, '').trim()
  // Technical leftovers (a missing command, a crash, a file path) aren't for people.
  if (!raw || raw.length > 220 || /panicked|invoke|not allowed|command \S+ not found|os error|\.rs:\d|\{|\bnull\b/i.test(raw)) return UNREACHABLE
  const line = raw[0].toUpperCase() + raw.slice(1)
  return /[.!?…]$/.test(line) ? line : `${line}.`
}
