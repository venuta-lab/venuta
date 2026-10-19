// "Also on YouTube Music" in words: what a sync did, why one waited, and when one is due.

export interface SyncCounts {
  pulled: number
  dropped: number
  pushed: number
  removed: number
  moved: number
}

export type PauseLike =
  | { kind: 'unread' }
  | { kind: 'held' }
  | { kind: 'other_account'; account: string }
  | { kind: 'many_gone'; side: 'venuta' | 'youtube_music'; gone: number; of: number }

const songs = (n: number) => `${n.toLocaleString('en-IN')} ${n === 1 ? 'song' : 'songs'}`

/** What a sync did, in one line; empty when it did nothing. */
export function syncWords(name: string, n: SyncCounts): string {
  const parts = [
    n.pulled ? `${songs(n.pulled)} came in` : '',
    n.dropped ? `${n.dropped.toLocaleString('en-IN')} went` : '',
    n.pushed ? `${n.pushed.toLocaleString('en-IN')} added there` : '',
    n.removed ? `${n.removed.toLocaleString('en-IN')} taken out there` : '',
  ].filter(Boolean)
  if (!parts.length && n.moved) parts.push('its order there matches yours')
  return parts.length ? `“${name}” is the same on YouTube Music now: ${parts.join(', ')}.` : ''
}

/** Why a sync waited, and what to do about it. */
export function pauseWords(name: string, p: PauseLike): string {
  switch (p.kind) {
    case 'unread':
      return `YouTube Music didn’t send all of “${name}”, so nothing was changed. It tries again the next time.`
    case 'held':
      return `You put “${name}” back as it was before a sync. Nothing is sent to YouTube Music until you sync again. Syncing again brings in what is still there. Nothing there is taken out.`
    case 'other_account':
      return `“${name}” is linked on another YouTube Music account (or another of its channels). Switch to that one, or unlink.`
    case 'many_gone':
      return p.side === 'youtube_music'
        ? `“${name}” on YouTube Music has ${p.gone.toLocaleString('en-IN')} of its ${p.of.toLocaleString('en-IN')} songs gone since the last sync, so nothing was changed. Sync anyway takes them out here too.`
        : `Your “${name}” has ${p.gone.toLocaleString('en-IN')} of its ${p.of.toLocaleString('en-IN')} songs gone since the last sync, so nothing was sent. Sync anyway takes them out on YouTube Music too.`
  }
}

/** Seconds between syncs of one playlist: opened, at most once a minute; otherwise half an hour. */
const OPEN_GAP = 60
const TIMER_GAP = 30 * 60

/** Is a sync due (seconds since 1970)? An edit of yours always is. */
export function due(lastSync: number, now: number, why: 'open' | 'timer' | 'edit' | 'press'): boolean {
  if (why === 'edit' || why === 'press') return true
  return now - lastSync >= (why === 'open' ? OPEN_GAP : TIMER_GAP) || lastSync === 0
}

/** YouTube Music's most songs in one playlist. */
export const YTM_PLAYLIST_LIMIT = 5000

/** How a linked playlist stands: songs there, and lines that stay here only. */
export function standWords(there: number, onlyHere: number): string {
  const n = (x: number) => x.toLocaleString('en-IN')
  const all = there + onlyHere
  const line = !onlyHere
    ? there === 1
      ? 'The 1 song there.'
      : `All ${n(there)} songs there.`
    : there
      ? `${n(there)} of ${n(all)} songs there; ${n(onlyHere)} only here.`
      : `None of its ${n(all)} songs there yet; ${n(onlyHere)} only here.`
  return there >= YTM_PLAYLIST_LIMIT - 100
    ? `${line} YouTube Music holds up to ${n(YTM_PLAYLIST_LIMIT)} songs in a playlist; past that, songs stay here only.`
    : line
}
