/*
  New versions, as the app shows them: what a check's answer does to what is shown, when the next
  quiet check runs, and the words for the title bar and Settings. No Tauri here, so tests run it
  as it is (updates.ts does the asking).
*/

export interface UpdateInfo {
  version: string
  current: string
  notes: string | null
  date: string | null
}

export type ProblemKind =
  | 'local_only'
  | 'up_to_date'
  | 'network'
  | 'timeout'
  | 'busy'
  | 'not_published'
  | 'broken'
  | 'no_windows'
  | 'wrong_key'
  | 'stalled'
  | 'signature'
  | 'folder'
  | 'installer'

/** Why there is no update to offer or install, in words from the app. */
export interface Problem {
  kind: ProblemKind
  message: string
}

export type Checked = { status: 'current'; current: string } | { status: 'found'; info: UpdateInfo }

export type UpdateState =
  | { at: 'idle' }
  /** `info`: an update found before stays offered while checking again. */
  | { at: 'checking'; manual: boolean; info?: UpdateInfo }
  | { at: 'current'; version: string }
  | { at: 'found'; info: UpdateInfo }
  | { at: 'downloading'; info: UpdateInfo; done: number; total: number | null }
  | { at: 'installing'; info: UpdateInfo }
  /** `info`: the update that failed to install, or one still known. */
  | { at: 'failed'; problem: Problem; info?: UpdateInfo }
  | { at: 'local_only' }

/** The first quiet check, after start. */
export const START_DELAY = 10_000
/** Then every so often while Venuta runs. */
export const EVERY = 4 * 3600_000
/** Sooner after a check that failed. */
export const AFTER_FAILURE = 30 * 60_000

/** The update this state knows of, if any. */
export function knownUpdate(s: UpdateState): UpdateInfo | undefined {
  switch (s.at) {
    case 'found':
    case 'downloading':
    case 'installing':
      return s.info
    case 'checking':
    case 'failed':
      return s.info
    default:
      return undefined
  }
}

/** What a check's answer shows. A quiet check that fails keeps an update already found. */
export function afterCheck(prev: UpdateState, r: { ok: Checked } | { err: Problem }, manual: boolean): UpdateState {
  const known = knownUpdate(prev)
  if ('ok' in r) return r.ok.status === 'found' ? { at: 'found', info: r.ok.info } : { at: 'current', version: r.ok.current }
  const problem = r.err
  if (problem.kind === 'local_only') return known && !manual ? { at: 'found', info: known } : { at: 'local_only' }
  if (problem.kind === 'up_to_date') return { at: 'current', version: known?.current ?? '' }
  if (known && !manual) return { at: 'found', info: known }
  return { at: 'failed', problem, info: known }
}

/** When the next quiet check runs, or null while an update installs. */
export function nextCheckIn(s: UpdateState): number | null {
  if (s.at === 'downloading' || s.at === 'installing') return null
  if (s.at === 'failed' && !s.info) return AFTER_FAILURE
  return EVERY
}

/** Whether version `a` comes after `b` (1.2.10 after 1.2.9; a pre-release before its release). */
export function isNewer(a: string, b: string): boolean {
  const parse = (v: string) => {
    const [main, pre] = v.trim().replace(/^v/, '').split('-', 2)
    return { nums: main.split('.').map((n) => Number.parseInt(n, 10) || 0), pre: pre ?? '' }
  }
  const x = parse(a)
  const y = parse(b)
  for (let i = 0; i < Math.max(x.nums.length, y.nums.length); i++) {
    const d = (x.nums[i] ?? 0) - (y.nums[i] ?? 0)
    if (d) return d > 0
  }
  if (x.pre === y.pre) return false
  if (!x.pre) return true
  if (!y.pre) return false
  return x.pre.localeCompare(y.pre, undefined, { numeric: true }) > 0
}

/** An update found before, offered again at start while the running version is older than it. */
export function rememberedUpdate(saved: UpdateInfo | undefined, running: string): UpdateInfo | undefined {
  if (!saved || !isNewer(saved.version, running)) return undefined
  return { ...saved, current: running }
}

/** The notice comes once for each version. */
export function shouldAnnounce(info: UpdateInfo, announced: string | undefined): boolean {
  return info.version !== announced
}

const percent = (done: number, total: number | null) => (total ? Math.min(100, Math.round((done / total) * 100)) : null)

/** The title bar's button: shown from the moment an update is found until it is installed. */
export function titleButton(s: UpdateState): { label: string; busy: boolean; failed: boolean } | null {
  switch (s.at) {
    case 'found':
      return { label: `Update to ${s.info.version}`, busy: false, failed: false }
    case 'checking':
      return s.info ? { label: `Update to ${s.info.version}`, busy: false, failed: false } : null
    case 'downloading': {
      const p = percent(s.done, s.total)
      return { label: p === null ? 'Updating…' : `Updating ${p}%`, busy: true, failed: false }
    }
    case 'installing':
      return { label: 'Installing…', busy: true, failed: false }
    case 'failed':
      return s.info ? { label: 'Update failed', busy: false, failed: true } : null
    default:
      return null
  }
}

/** How long ago, in words. */
export function ago(ms: number): string {
  const min = Math.floor(ms / 60_000)
  if (min < 1) return 'just now'
  if (min < 60) return `${min} minute${min === 1 ? '' : 's'} ago`
  const h = Math.floor(min / 60)
  if (h < 24) return `${h} hour${h === 1 ? '' : 's'} ago`
  const d = Math.floor(h / 24)
  return d === 1 ? 'yesterday' : `${d} days ago`
}

/** Settings' line: the state, when Venuta last heard from GitHub, and what went wrong. */
export function statusLine(s: UpdateState, lastChecked: number, now: number, version: string): string {
  const when = lastChecked ? ` Last checked ${ago(now - lastChecked)}.` : ''
  switch (s.at) {
    case 'idle':
      return lastChecked ? `Venuta ${version}.${when}` : 'New versions are signed, and installed only when you choose.'
    case 'checking':
      return 'Checking for updates…'
    case 'current':
      return `Venuta ${s.version || version} is up to date.${when}`
    case 'found':
      return `Venuta ${s.info.version} is available (you have ${s.info.current}).`
    case 'downloading': {
      const p = percent(s.done, s.total)
      return `Downloading Venuta ${s.info.version}…${p === null ? '' : ` ${p}%`}`
    }
    case 'installing':
      return `Installing Venuta ${s.info.version}. Venuta starts again when it is done.`
    case 'failed':
      return `${s.problem.message}${when}`
    case 'local_only':
      return "Local only is on, so Venuta doesn't go online to check for updates."
  }
}
