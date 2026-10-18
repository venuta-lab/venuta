import { invoke } from '@tauri-apps/api/core'
import { useEffect, useSyncExternalStore } from 'react'
import { inApp } from './engine'
import { notify } from './notify'
import { isOnline, songItemOf, type QueueItem } from './player'
import { rateOnAccount } from './ytmAccount'

/*
  Liked songs: one playlist in your library, marked as the liked one (crates/engine playlists:
  `like`), holding your files and online songs alike. Here is what the screens need of it: is
  this song liked, and liking or unliking it from any heart or menu, everywhere at once. A song
  is liked as your file (by path) or online (by its id); the engine tells both when you have
  the song you liked online.
*/

interface LikedSet {
  name: string | null
  paths: string[]
  online: string[]
}

let root = ''
let name: string | null = null
let paths = new Set<string>()
let online = new Set<string>()
let version = 0
const listeners = new Set<() => void>()

function publish(r: string, s: LikedSet) {
  root = r
  name = s.name
  paths = new Set(s.paths)
  online = new Set(s.online)
  version++
  listeners.forEach((l) => l())
}

const subscribe = (l: () => void) => {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}

const onlineKey = (q: QueueItem) => (q.online?.kind === 'online' ? `${q.online.provider}:${q.online.track_id}` : '')

/** Whether this song is liked (as your file or online). */
export const isLiked = (q: QueueItem): boolean => (isOnline(q) ? online.has(onlineKey(q)) : !!q.path && paths.has(q.path))

/** What your Liked songs list is called (null until something is liked). */
export const likedListName = () => name

/** Read what is liked for this library (again when `fresh`: the library was read again). */
export async function loadLikes(r: string, fresh = false) {
  if (!r || !inApp() || (!fresh && r === root)) return
  try {
    publish(r, await invoke<LikedSet>('liked_songs', { root: r }))
  } catch {
    // Without the list, nothing shows liked; liking still works and reads it again.
  }
}

/** Keep what is liked current for this library while a screen with hearts shows. */
export function useLikes(r: string, stamp?: unknown) {
  useEffect(() => {
    void loadLikes(r, stamp !== undefined)
  }, [r, stamp])
  return useSyncExternalStore(subscribe, () => version)
}

/** Whether this song is liked, kept current. */
export function useLiked(q: QueueItem | null | undefined): boolean {
  useSyncExternalStore(subscribe, () => version)
  return !!q && isLiked(q)
}

/** Like or unlike a song, everywhere at once; says so, and says when it could not. */
export async function setLiked(r: string, q: QueueItem, liked: boolean) {
  if (!r || !inApp()) return
  // Shown at once; put right from the answer.
  const key = isOnline(q) ? onlineKey(q) : q.path
  const set = isOnline(q) ? online : paths
  if (liked) set.add(key)
  else set.delete(key)
  version++
  listeners.forEach((l) => l())
  try {
    const now = await invoke<LikedSet>('like_song', { root: r, item: songItemOf(q), liked })
    publish(r, now)
    notify(liked ? `Added to ${now.name ?? 'Liked songs'}.` : `Removed from ${name ?? 'Liked songs'}.`)
    await rateOnAccount(q, liked ? 'like' : 'none')
  } catch (e) {
    notify(`Couldn't ${liked ? 'like' : 'unlike'} “${q.title}”: ${String(e)}`)
    root = ''
    void loadLikes(r)
  }
}

/** Like several songs at once (the ones picked on a page); says so once. */
export async function likeMany(r: string, items: QueueItem[]) {
  if (!r || !inApp() || !items.length) return
  let last: LikedSet | null = null
  try {
    // One at a time: each goes on top, so the first picked ends up first.
    for (const q of [...items].reverse()) last = await invoke<LikedSet>('like_song', { root: r, item: songItemOf(q), liked: true })
    if (last) publish(r, last)
    notify(`Added ${items.length === 1 ? `“${items[0].title}”` : `${items.length} songs`} to ${last?.name ?? 'Liked songs'}.`)
    for (const q of items) await rateOnAccount(q, 'like')
  } catch (e) {
    notify(`Couldn't like them all: ${String(e)}`)
    root = ''
    void loadLikes(r)
  }
}

/** Like it if it isn't, unlike it if it is. */
export const toggleLiked = (r: string, q: QueueItem) => setLiked(r, q, !isLiked(q))
