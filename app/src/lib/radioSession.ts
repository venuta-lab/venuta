/*
  One radio session, kept, never restarted: what YouTube Music's own player keeps while a queue
  plays, and the rules for what comes next. Venuta chooses nothing here; it only keeps asking the
  same session for more, the way YouTube Music's app does:

  - A radio: at five songs left, the next page of the same session (its continuation), added as
    it came. Three failed tries and paging stops.
  - When nothing more comes (no continuation), with Autoplay on: what the session says it goes on
    with (the automix endpoint), else YouTube's Related songs for the last song, else the end,
    said in words.
  - An album or playlist: once its last song plays, with Autoplay on, what the list goes on with
    (its own answer names it), then that radio's pages.
  - Repeat on: nothing more.
  - A tuning chip: its endpoint, whose songs replace what is coming up.

  Pure (no window, no engine), so tests/radioSession.test.ts reads it; lib/radio.ts runs it.
*/

import type { OnlineRadio, OnlineTrack, RadioChip, RadioEndpoint } from './engine.ts'

/** Songs left after the playing one when a radio's next page is asked for. */
export const PAGE_AT = 5
/** Failed tries of one step before it is given up. */
export const TRIES = 3
/** How long after a failed try the next one may go. */
export const RETRY_MS = 10_000

export interface RadioSession {
  /** The queue it is for: the player's `from` as it keeps it. */
  key: string
  /** A radio pages by its continuation; a list (an album, a playlist) only goes on at its end. */
  kind: 'radio' | 'list'
  /** What the current pages answer: asked again, with `continuation`, for the next. */
  endpoint: RadioEndpoint | null
  continuation: string | null
  /** YouTube Music's own name for it ("Kitni Baatein Radio"). */
  title: string | null
  chips: RadioChip[]
  /** What Autoplay goes on with when nothing more comes. */
  continueWith: RadioEndpoint | null
  /** A list's own id in the catalog (`OLAK5uy_…`, `PL…`), to ask what it goes on with. */
  list: string | null
  /** The list's own answer was read for what it goes on with. */
  markerRead: boolean
  /** The catalog's Related page for the song the session is about. */
  relatedId: string | null
  relatedUsed: boolean
  /** Failed tries in a row of the step now due, and when the last one failed (ms). */
  failures: number
  failedAt: number
  /** Something is being asked for now (not kept across a restart). */
  loading: boolean
  /** Nothing more is coming, in words. */
  exhausted: string | null
}

/** What to ask for next. */
export type Step =
  | { kind: 'page'; endpoint: RadioEndpoint; continuation: string }
  | { kind: 'continue'; endpoint: RadioEndpoint }
  /** A list's own answer (asked with its last song), for what it goes on with. */
  | { kind: 'marker' }
  /** YouTube's Related songs for the last song. */
  | { kind: 'related' }
  /** Nothing more: say so. */
  | { kind: 'end' }

export interface Situation {
  /** Songs after the one playing. */
  left: number
  autoplay: boolean
  repeat: 'off' | 'all' | 'one'
  /** Now (ms). */
  now: number
}

/** Said when a radio has given everything it has. */
export const RADIO_END = "That's all YouTube Music has for this radio."
/** Said when a list ends and YouTube Music has nothing to go on with. */
export const LIST_END = 'YouTube Music has nothing more to play after this.'

const blank = (key: string): RadioSession => ({
  key,
  kind: 'radio',
  endpoint: null,
  continuation: null,
  title: null,
  chips: [],
  continueWith: null,
  list: null,
  markerRead: false,
  relatedId: null,
  relatedUsed: false,
  failures: 0,
  failedAt: 0,
  loading: false,
  exhausted: null,
})

const sameEndpoint = (a: RadioEndpoint | null | undefined, b: RadioEndpoint | null | undefined) =>
  !!a && !!b && (a.video_id ?? null) === (b.video_id ?? null) && (a.playlist_id ?? null) === (b.playlist_id ?? null) && (a.params ?? null) === (b.params ?? null)

/** A radio's session from its first answer. */
export function radioSession(key: string, radio: OnlineRadio, asked: RadioEndpoint | null = null): RadioSession {
  return {
    ...blank(key),
    endpoint: radio.endpoint ?? asked,
    continuation: radio.continuation ?? null,
    title: radio.title ?? null,
    chips: radio.chips ?? [],
    continueWith: radio.continue_with ?? null,
    relatedId: radio.related_id ?? null,
  }
}

/** The session of an album or playlist (`list`: its id in the catalog, when it has one). */
export function listSession(key: string, list: string | null): RadioSession {
  return { ...blank(key), kind: 'list', list }
}

/** What to ask for now, if anything. */
export function nextStep(s: RadioSession, at: Situation): Step | null {
  if (s.exhausted || s.loading || at.repeat !== 'off') return null
  if (s.failures > 0 && at.now - s.failedAt < RETRY_MS) return null
  const due = s.kind === 'radio' ? at.left <= PAGE_AT : at.left <= 0
  if (!due) return null
  // A radio's own pages come whatever Autoplay says: the radio is the queue.
  if (s.kind === 'radio' && s.continuation && s.endpoint) return { kind: 'page', endpoint: s.endpoint, continuation: s.continuation }
  // What comes after the session is Autoplay's.
  if (!at.autoplay) return null
  if (s.continueWith) return { kind: 'continue', endpoint: s.continueWith }
  if (s.kind === 'list' && !s.markerRead) return { kind: 'marker' }
  if (!s.relatedUsed) return { kind: 'related' }
  return { kind: 'end' }
}

/** A page came: the session moves to the page after it. A page with no songs counts as a failed
 *  try, so a radio that keeps answering nothing ends. */
export function afterPage(s: RadioSession, page: OnlineRadio, now = Date.now()): RadioSession {
  const next = {
    ...s,
    loading: false,
    continuation: page.continuation ?? null,
    continueWith: page.continue_with ?? s.continueWith,
    relatedId: s.relatedId ?? page.related_id ?? null,
    chips: page.chips?.length ? page.chips : s.chips,
    title: s.title ?? page.title ?? null,
  }
  return page.tracks.length ? { ...next, failures: 0 } : failedTry(next, now)
}

/** What the session went on with answered: that radio is the session now. Nothing in it: on to
 *  the next thing a session's end does. */
export function afterContinue(s: RadioSession, radio: OnlineRadio): RadioSession {
  const used = s.continueWith
  if (!radio.tracks.length) return { ...s, loading: false, failures: 0, continueWith: null, markerRead: true }
  const named = radio.continue_with ?? null
  return {
    ...s,
    kind: 'radio',
    loading: false,
    failures: 0,
    endpoint: radio.endpoint ?? used,
    continuation: radio.continuation ?? null,
    title: radio.title ?? s.title,
    chips: radio.chips ?? [],
    // Never the same one again: an answer naming itself would be asked for ever.
    continueWith: sameEndpoint(named, used) || sameEndpoint(named, radio.endpoint) ? null : named,
    markerRead: true,
    relatedId: radio.related_id ?? s.relatedId,
  }
}

/** A list's own answer came: it names what the list goes on with, or nothing. */
export function afterMarker(s: RadioSession, answer: OnlineRadio): RadioSession {
  return {
    ...s,
    loading: false,
    failures: 0,
    markerRead: true,
    continueWith: answer.continue_with ?? null,
    relatedId: answer.related_id ?? s.relatedId,
  }
}

/** Related songs were added (or there were none): the next end is the end. */
export function afterRelated(s: RadioSession): RadioSession {
  return { ...s, loading: false, failures: 0, relatedUsed: true }
}

/** The end: nothing more comes, and it is said once. */
export function ended(s: RadioSession): RadioSession {
  return { ...s, loading: false, exhausted: s.kind === 'list' && !s.endpoint ? LIST_END : RADIO_END }
}

const failedTry = (s: RadioSession, now: number): RadioSession => {
  const failures = s.failures + 1
  if (failures < TRIES) return { ...s, loading: false, failures, failedAt: now }
  // Given up: on to what comes after it.
  const gave = { ...s, loading: false, failures: 0, failedAt: now }
  if (s.kind === 'radio' && s.continuation) return { ...gave, continuation: null }
  if (s.continueWith) return { ...gave, continueWith: null }
  if (s.kind === 'list' && !s.markerRead) return { ...gave, markerRead: true }
  return { ...gave, relatedUsed: true }
}

/** The step now due failed (offline, refused). Three in a row and it is given up. */
export const failed = (s: RadioSession, now: number) => failedTry(s, now)

/** A chip was chosen (`index` in the session's chips) and its endpoint answered: the session is
 *  that re-tuned radio now, with the chip marked as the one in effect. */
export function afterChip(s: RadioSession, index: number, radio: OnlineRadio): RadioSession {
  const chips = radio.chips?.length ? radio.chips : s.chips.map((c, i) => ({ ...c, selected: i === index }))
  return {
    ...s,
    kind: 'radio',
    loading: false,
    failures: 0,
    endpoint: radio.endpoint ?? s.chips[index]?.endpoint ?? s.endpoint,
    continuation: radio.continuation ?? null,
    chips,
    title: radio.title ?? s.title,
    continueWith: radio.continue_with ?? null,
    relatedId: radio.related_id ?? s.relatedId,
    relatedUsed: false,
    exhausted: null,
  }
}

/** The queue after a chip: the songs played and the one playing stay, the new ones follow. */
export function upcomingReplaced<T>(queue: T[], index: number, fresh: T[]): T[] {
  return [...queue.slice(0, index + 1), ...fresh]
}

/** The catalog's songs as queue rows, in its order, repeats and all: with `playMyCopy`, a song
 *  you have (`owns` finds it: the same song, lib/owned.ts) plays from your file in that place;
 *  otherwise, and when it is off, it streams. Nothing moves, nothing is left out. */
export function hybridRows<M, R>(
  tracks: OnlineTrack[],
  playMyCopy: boolean,
  owns: (t: OnlineTrack) => M | undefined,
  mine: (m: M) => R,
  online: (t: OnlineTrack) => R,
): R[] {
  return tracks.map((t) => {
    const yours = playMyCopy ? owns(t) : undefined
    return yours !== undefined ? mine(yours) : online(t)
  })
}

/** The little a store needs (localStorage, or a stand-in in tests). */
export interface KeyValueStore {
  getItem(key: string): string | null
  setItem(key: string, value: string): void
  removeItem(key: string): void
}

/** Where the session of the queue playing is kept, so a restart carries on with it. */
export const SESSION_KEY = 'radio-session'

/** Keep the session (the one of the queue playing; one at a time). */
export function saveSession(store: () => KeyValueStore | null, s: RadioSession | null) {
  try {
    const kv = store()
    if (!s) kv?.removeItem(SESSION_KEY)
    else kv?.setItem(SESSION_KEY, JSON.stringify({ ...s, loading: false }))
  } catch {
    // Not kept this time: the session still runs until the window closes.
  }
}

/** The kept session, when it is the one of this queue (`key`: the player's `from`). */
export function loadSession(store: () => KeyValueStore | null, key: string): RadioSession | null {
  try {
    const v = JSON.parse(store()?.getItem(SESSION_KEY) ?? 'null') as RadioSession | null
    if (!v || typeof v !== 'object' || v.key !== key || (v.kind !== 'radio' && v.kind !== 'list')) return null
    return { ...blank(key), ...v, chips: Array.isArray(v.chips) ? v.chips : [], loading: false }
  } catch {
    return null
  }
}

/** Where a file's song in the catalog is remembered (by the file's path), and how many. */
export const MATCHES_KEY = 'radio-matches'
const MATCHES_KEEP = 500

/** Files you started a radio from, and the catalog's same song for each: found once, asked
 *  again never. Only a confident match is kept. */
export function matchMemory(store: () => KeyValueStore | null) {
  const read = (): Record<string, string> => {
    try {
      const v = JSON.parse(store()?.getItem(MATCHES_KEY) ?? '{}') as unknown
      return v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, string>) : {}
    } catch {
      return {}
    }
  }
  return {
    get: (path: string): string | undefined => {
      const id = read()[path]
      return typeof id === 'string' && id ? id : undefined
    },
    set: (path: string, id: string) => {
      const all = read()
      delete all[path]
      all[path] = id
      const keys = Object.keys(all)
      for (const k of keys.slice(0, Math.max(0, keys.length - MATCHES_KEEP))) delete all[k]
      try {
        store()?.setItem(MATCHES_KEY, JSON.stringify(all))
      } catch {
        // Not kept: found again next time.
      }
    },
  }
}
