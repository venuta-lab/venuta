import { useCallback, useEffect, useSyncExternalStore } from 'react'
import { inApp, ytmLink, ytmLinks, ytmLinkSync, ytmLinkUndo, type YtmLinkView, type YtmPause, type YtmSyncReport } from './engine'
import { notify } from './notify'
import { onlineProblem } from './online'
import { ytmLibraryChanged, ytmReady, ytmStatusRefresh } from './ytmLibrary'
import { due, pauseWords, syncWords } from './ytmLinkWords'

/*
  "Also on YouTube Music": each linked playlist's state for its page, and when it syncs. A sync
  runs when you press it, when the playlist opens (at most once a minute), a few seconds after
  you change it, once soon after start, and every half hour at most. Never in a loop, never
  signed out or in Local only. What a sync did is said once, with Undo when yours changed.
*/

interface LinkState {
  /** undefined: not read yet; null: not linked. */
  link: YtmLinkView | null | undefined
  syncing: boolean
  paused: YtmPause | null
  problem: string
}

const states = new Map<string, LinkState>()
const listeners = new Set<() => void>()
let version = 0
const changed = () => {
  version++
  listeners.forEach((l) => l())
}
const blank: LinkState = { link: undefined, syncing: false, paused: null, problem: '' }
const stateOf = (name: string) => states.get(name) ?? blank
const set = (name: string, patch: Partial<LinkState>) => {
  states.set(name, { ...stateOf(name), ...patch })
  changed()
}

/** Read a playlist's link again (after linking, choosing, unlinking). */
export async function reloadLink(root: string, name: string) {
  if (!inApp()) return set(name, { link: null })
  try {
    set(name, { link: await ytmLink(root, name) })
  } catch {
    set(name, { link: null })
  }
}

const nowSecs = () => Math.floor(Date.now() / 1000)

/** Sync one linked playlist now. `quiet`: a sync nobody pressed says only what changed. `force`
 *  answers the pause shown, and only that one (`gone`: the side "Sync anyway" was asked about). */
export async function syncLink(
  root: string,
  name: string,
  opts: { force?: boolean; gone?: 'venuta' | 'youtube_music'; quiet?: boolean } = {},
) {
  if (!ytmReady() || stateOf(name).syncing) return
  set(name, { syncing: true, problem: '' })
  let report: YtmSyncReport | null = null
  try {
    report = await ytmLinkSync(root, name, !!opts.force, opts.gone ?? null)
  } catch (e) {
    set(name, { syncing: false, problem: onlineProblem(e) })
    void ytmStatusRefresh()
    return
  }
  set(name, { syncing: false, paused: report.paused })
  if (report.busy) return
  await reloadLink(root, name)
  const n = report.note
  if (report.upload_problems.length) notify(`Couldn't upload ${report.upload_problems.join('; ')}`)
  if (n.problem) notify(`“${name}” synced part way: ${onlineProblem(n.problem)} The rest goes the next time.`)
  const words = syncWords(name, n)
  if (n.undo) {
    window.dispatchEvent(new Event('venuta:playlists-changed'))
    const copy = n.undo
    notify(words, {
      label: 'Undo',
      run: () => {
        void ytmLinkUndo(root, name, copy)
          .then(() => {
            window.dispatchEvent(new Event('venuta:playlists-changed'))
            return reloadLink(root, name)
          })
          .then(() => set(name, { paused: { kind: 'held' } }))
          .catch((e) => notify(`Couldn't put it back: ${String(e)}`))
      },
    })
  } else if (words && !opts.quiet) notify(words)
  else if (!words && !report.paused && !opts.quiet) notify(`“${name}” is the same on YouTube Music.`)
  if (n.pushed || n.removed || n.moved) ytmLibraryChanged(['playlists'])
  if (report.paused && !opts.quiet) notify(pauseWords(name, report.paused))
}

/** The playlist's page opened: its link is read, and synced when due. */
export async function linkOpened(root: string, name: string) {
  await reloadLink(root, name)
  const link = stateOf(name).link
  if (link && due(link.last_sync, nowSecs(), 'open')) void syncLink(root, name, { quiet: true })
}

// A change of yours goes over a few seconds after the last one (a drag, then another).
const EDIT_WAIT_MS = 5_000
const editTimers = new Map<string, ReturnType<typeof setTimeout>>()

/** You changed a linked playlist: it syncs a few seconds after the last change. */
export function linkEdited(root: string, name: string) {
  if (!stateOf(name).link) return
  clearTimeout(editTimers.get(name))
  editTimers.set(
    name,
    setTimeout(() => {
      editTimers.delete(name)
      void syncLink(root, name, { quiet: true })
    }, EDIT_WAIT_MS),
  )
}

// Soon after start (once the sign-in is back), then every half hour at most.
const START_WAIT_MS = 20_000
const TICK_MS = 30 * 60_000
let scheduledFor = ''

async function syncDue(root: string) {
  if (!ytmReady()) return
  const links = await ytmLinks(root).catch(() => [])
  for (const l of links) {
    if (due(l.last_sync, nowSecs(), 'timer')) await syncLink(root, l.playlist, { quiet: true })
  }
}

/** Keep the library's links in step (called once the library is known). */
export function scheduleLinks(root: string) {
  if (!inApp() || scheduledFor === root) return
  scheduledFor = root
  setTimeout(() => void syncDue(root), START_WAIT_MS)
  setInterval(() => scheduledFor === root && void syncDue(root), TICK_MS)
}

/** A playlist's link and what it is doing, for its page. */
export function useYtmLinkState(root: string, name: string, on: boolean) {
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
    if (on) void linkOpened(root, name)
  }, [on, root, name])
  const reload = useCallback(() => reloadLink(root, name), [root, name])
  return { ...stateOf(name), reload }
}

/** After a pause was answered (or the link changed), forget it. */
export const clearPause = (name: string) => set(name, { paused: null })
