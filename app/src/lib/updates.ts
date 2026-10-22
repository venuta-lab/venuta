import { Channel, invoke } from '@tauri-apps/api/core'
import { useSyncExternalStore } from 'react'
import { inApp } from './engine'
import { notify } from './notify'
import {
  START_DELAY,
  afterCheck,
  knownUpdate,
  nextCheckIn,
  shouldAnnounce,
  type Checked,
  type Problem,
  type UpdateState,
} from './updateModel'

export type { UpdateInfo, UpdateState } from './updateModel'

/*
  New versions of Venuta: one store for the whole app, so the quiet checks, the title bar's
  button, the notice and Settings › System › Updates all agree on what was found and what is
  happening. What each answer means lives in updateModel.ts.
*/

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

// What is kept between starts: the quiet checks' switch, when GitHub last answered, and the
// version the notice was shown for.
const KEY = 'venuta.updates'
type Saved = { auto: boolean; last: number; announced?: string }
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

const asProblem = (e: unknown): Problem =>
  e && typeof e === 'object' && 'kind' in e && 'message' in e ? (e as Problem) : { kind: 'network', message: String(e).replace(/^Error: /, '') }

let running: Promise<void> | null = null

/**
 * Ask GitHub for a newer version. A manual check always ends in an answer (up to date, found,
 * or failed and why); a quiet one keeps an update already found when it fails. `anyway` asks
 * once while Local only is on.
 */
export function checkForUpdate(opts: { manual: boolean; anyway?: boolean } = { manual: true }): Promise<void> {
  if (!inApp() || state.at === 'downloading' || state.at === 'installing') return Promise.resolve()
  if (running) return running
  const before = state
  set({ at: 'checking', manual: opts.manual, info: knownUpdate(before) })
  running = invoke<Checked>('update_check', { anyway: opts.anyway ?? false })
    .then(
      (ok) => {
        remember({ last: Date.now() })
        set(afterCheck(before, { ok }, opts.manual))
      },
      (e) => set(afterCheck(before, { err: asProblem(e) }, opts.manual)),
    )
    .finally(() => {
      running = null
      announce()
    })
  return running
}

/** Download (signature checked), install and restart. Venuta closes while it installs. */
export async function installUpdate() {
  const info = knownUpdate(state)
  if (!info || state.at === 'downloading' || state.at === 'installing') return
  const progress = new Channel<{ done: number; total: number | null }>()
  progress.onmessage = (p) => {
    const complete = p.total !== null && p.done >= p.total
    set(complete ? { at: 'installing', info } : { at: 'downloading', info, done: p.done, total: p.total })
  }
  set({ at: 'downloading', info, done: 0, total: null })
  try {
    await invoke('update_install', { onProgress: progress })
    // A test copy stops after the checked download; a real one has quit for the installer.
    set({ at: 'found', info })
  } catch (e) {
    const problem = asProblem(e)
    set(problem.kind === 'up_to_date' ? { at: 'current', version: info.current } : { at: 'failed', problem, info })
  }
}

/** The latest release's page on GitHub, to download the installer by hand. */
export function openDownloadPage() {
  void invoke('update_download_page').catch(() => {})
}

/** The notice, once per version, when the window can be seen (started in the tray, it waits). */
function announce() {
  if (state.at !== 'found') return
  const info = state.info
  if (!shouldAnnounce(info, savedUpdates().announced) || document.visibilityState !== 'visible') return
  remember({ announced: info.version })
  notify(`Venuta ${info.version} is out.`, { label: 'Update now', run: () => void installUpdate() })
}

/**
 * The quiet checks: soon after start, every few hours while Venuta runs, sooner after a failure
 * and as soon as the network is back. Each one asks only if checking is switched on. Returns a
 * stop function.
 */
export function startUpdateChecks(): () => void {
  if (!inApp()) return () => {}
  let timer = 0
  const schedule = (ms: number | null) => {
    window.clearTimeout(timer)
    timer = window.setTimeout(run, ms ?? 60_000)
  }
  const run = () => {
    if (!savedUpdates().auto) return schedule(nextCheckIn(state))
    void checkForUpdate({ manual: false }).then(() => schedule(nextCheckIn(state)))
  }
  const online = () => {
    if (state.at === 'failed') run()
  }
  window.addEventListener('online', online)
  document.addEventListener('visibilitychange', announce)
  schedule(START_DELAY)
  return () => {
    window.clearTimeout(timer)
    window.removeEventListener('online', online)
    document.removeEventListener('visibilitychange', announce)
  }
}
