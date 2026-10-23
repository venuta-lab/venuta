import { invoke } from '@tauri-apps/api/core'
import { useSyncExternalStore } from 'react'
import { onlineSearch, type OnlineTrack } from './engine'
import { isLocalOnly } from './localOnly'
import { notify } from './notify'
import type { QueueItem } from './player'
import { shareMatch } from './shareMatch'

/*
  Sharing: a song, album, artist or playlist by its YouTube Music link. One press copies the link
  and says so, with "More ways" for Windows' share panel (WhatsApp, Mail, Phone Link, Nearby
  Share) and a QR code. A song of yours is shared by its YouTube Music version: found by one
  search when you share it (nothing is asked before), chosen only when it is surely the same
  recording (lib/shareMatch.ts), and kept for that file. Otherwise the closest few are offered.
*/

/** The catalog's own page for an online song. */
export const songLink = (trackId: string) => `https://music.youtube.com/watch?v=${encodeURIComponent(trackId)}`

/** What the share panel shows. */
export interface ShareRequest {
  /** What is shared ("Tum Hi Ho"). */
  name: string
  /** Its link; none while one is chosen from `offered`. */
  url?: string
  /** A line under the title ("Shares the YouTube Music version."). */
  note?: string
  /** A song of yours with no sure match: the closest online songs, to choose from. */
  offered?: OnlineTrack[]
  /** The file those are offered for (a choice is kept for it). */
  path?: string
}

let request: ShareRequest | null = null
const listeners = new Set<() => void>()
const emit = () => listeners.forEach((l) => l())

/** Show the share panel. */
export function openShare(r: ShareRequest) {
  request = r
  emit()
}
export function closeShare() {
  request = null
  emit()
}
export function useShareRequest(): ShareRequest | null {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => {
        listeners.delete(l)
      }
    },
    () => request,
  )
}

const VERSION_NOTE = 'Shares the YouTube Music version of your song.'

/** Copy a link and say so, with the share panel one press away. */
export function shareLink(url: string, name: string, note?: string) {
  navigator.clipboard
    .writeText(url)
    .then(() => notify(`Link to “${name}” copied.`, { label: 'More ways', run: () => openShare({ name, url, note }) }))
    .catch(() => openShare({ name, url, note }))
}

// The online song each file of yours was shared as, kept on this computer.
const KEY = 'venuta.shareMatches'
type Kept = Record<string, { id: string; title: string }>
function kept(): Kept {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? '{}') as Kept
  } catch {
    return {}
  }
}
function keep(path: string, t: Pick<OnlineTrack, 'track_id' | 'title'>) {
  try {
    localStorage.setItem(KEY, JSON.stringify({ ...kept(), [path]: { id: t.track_id, title: t.title } }))
  } catch {
    /* a convenience only: the next share asks again */
  }
}

/** Share a song: an online one by its link, one of yours by its YouTube Music version. */
export async function shareSong(song: Pick<QueueItem, 'path' | 'title' | 'artists' | 'duration_ms' | 'online'>) {
  if (song.online?.kind === 'online') return shareLink(songLink(song.online.track_id), song.title)
  const known = kept()[song.path]
  if (known) return shareLink(songLink(known.id), song.title, VERSION_NOTE)
  if (isLocalOnly()) {
    notify('Turn off Local only to share a song of yours: it is shared by its YouTube Music link.')
    return
  }
  notify(`Finding “${song.title}” on YouTube Music…`)
  let tracks: OnlineTrack[] = []
  try {
    const query = [song.title, song.artists[0] ?? ''].join(' ').trim()
    tracks = (await onlineSearch(query, 'pick'))?.tracks ?? []
  } catch {
    notify("Couldn't reach YouTube Music to find its link. Try again in a moment.")
    return
  }
  const m = shareMatch(song, tracks)
  if (m.pick) {
    keep(song.path, m.pick)
    return shareLink(songLink(m.pick.track_id), song.title, VERSION_NOTE)
  }
  openShare({
    name: song.title,
    path: song.path,
    offered: m.offered,
    note: m.offered.length ? 'Not on YouTube Music as this same recording. Is it one of these?' : 'Not found on YouTube Music, so it has no link to share.',
  })
}

/** One of the offered songs is yours: kept for the file, then shared. */
export function chooseShared(path: string, name: string, t: OnlineTrack) {
  keep(path, t)
  closeShare()
  shareLink(songLink(t.track_id), name, VERSION_NOTE)
}

/** Windows' share panel, with this link. */
export const shareToApp = (name: string, url: string) => invoke('share_to_app', { title: name, url })

/** The link as a QR code (SVG). */
export const shareQr = (url: string) => invoke<string>('share_qr', { text: url })
