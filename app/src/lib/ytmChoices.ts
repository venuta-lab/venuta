import { listen } from '@tauri-apps/api/event'
import { useEffect, useSyncExternalStore } from 'react'
import { inApp, ytmChoices as readChoices, ytmSetChoices, type YtmChoicesView } from './engine'
import { SAFE, settle, type YtmChoices } from './ytmChoiceWords'
import { useYtmStatus } from './ytmLibrary'

/*
  Your YouTube Music choices for the account and channel in use (app ytm_choices.rs), read once
  it is signed in and again whenever the account or channel changes. Until they are read, and
  for an account never asked, the safe ones hold: nothing is written to an account on a guess.
*/

let now: YtmChoicesView | null = null
let reading: Promise<YtmChoicesView | null> | null = null
let version = 0
const listeners = new Set<() => void>()
const changed = () => {
  version++
  listeners.forEach((l) => l())
}

let pending = false
let asked = 0

/** Read them (once; again with `fresh`, unless a read is on its way). Null signed out or in
 *  Local only. Only the latest read counts: one for an account left behind is dropped. */
export function loadYtmChoices(fresh = false): Promise<YtmChoicesView | null> {
  if (!inApp()) return Promise.resolve(null)
  if (reading && (pending || !fresh)) return reading
  const mine = ++asked
  pending = true
  reading = readChoices()
    .then((v) => {
      if (mine !== asked) return now
      now = v
      changed()
      return v
    })
    .catch(() => now)
    .finally(() => {
      if (mine === asked) pending = false
    })
  return reading
}

if (inApp()) {
  void loadYtmChoices()
  void listen('ytm-account-changed', () => {
    // Another account or channel (or none): its own choices, never the last one's.
    now = null
    pending = false
    changed()
    void loadYtmChoices(true)
  })
}

/** Asks the choices screen (YtmWelcome) to open for the account in use. */
export const YTM_CHOICES_EVENT = 'venuta:ytm-choices'
export const openYtmChoices = () => window.dispatchEvent(new Event(YTM_CHOICES_EVENT))

/** The choices of the account in use as last read (null: none read, or signed out). */
export const ytmChoicesNow = () => now

/** One choice as it holds now: the account's, or the safe one while none is known. */
export const ytmChoice = (key: keyof YtmChoices): boolean => (now?.choices ?? SAFE)[key]

/** Change some of the account in use's choices (or all of `account`'s); saved at once. */
export async function saveYtmChoices(change: Partial<YtmChoices>, account = now?.account): Promise<YtmChoicesView> {
  if (!account) throw new Error('Sign in to YouTube Music first.')
  const mine = now?.account === account
  const next = settle({ ...(mine && now ? now.choices : SAFE), ...change })
  if (mine && now) {
    now = { ...now, asked: true, choices: next }
    changed()
  }
  try {
    const saved = await ytmSetChoices(account, next)
    if (now?.account === account) {
      now = saved
      changed()
    }
    return saved
  } catch (e) {
    // Not kept: show what is stored again.
    void loadYtmChoices(true)
    throw e
  }
}

/** The choices of the account in use, kept current (null signed out or in Local only). */
export function useYtmChoices(): YtmChoicesView | null {
  const { signed_in } = useYtmStatus()
  useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => {
        listeners.delete(l)
      }
    },
    () => version,
  )
  useEffect(() => {
    // Signed in again, or Local only turned off: read them for the account now in use.
    if (signed_in && !now) void loadYtmChoices(true)
  }, [signed_in])
  return signed_in ? now : null
}

/** Your YouTube Music library shows in Venuta: signed in, with "My library in Venuta" on. */
export function useYtmLibraryShown(): boolean {
  const { signed_in } = useYtmStatus()
  const v = useYtmChoices()
  return signed_in && (v?.choices ?? SAFE).library
}
