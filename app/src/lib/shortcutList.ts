/*
  What you pin to the top of Home ("Add to Home"): your albums, artists and playlists, and online
  albums, artists and playlists. Once each, newest first, at most twelve. Pure (types only), so
  tests/shortcuts.test.ts reads it; lib/shortcuts.ts keeps them with your settings.
*/

import type { OnlineAlbum, OnlineCollection } from './engine'

export type Shortcut =
  | { kind: 'album'; dir: string; title: string }
  | { kind: 'artist'; name: string }
  | { kind: 'playlist'; name: string }
  | { kind: 'online-album'; album: OnlineAlbum }
  | { kind: 'online-artist'; artist: { artist_id: string; name: string; artwork_url?: string | null } }
  | { kind: 'online-collection'; collection: OnlineCollection }

const MOST = 12

/** What tells one shortcut from another. Empty for something that isn't one. */
export function shortcutKey(s: Shortcut): string {
  switch (s?.kind) {
    case 'album':
      return `album:${s.dir}`
    case 'artist':
      return `artist:${s.name.toLowerCase()}`
    case 'playlist':
      return `playlist:${s.name}`
    case 'online-album':
      return `online-album:${s.album.album_id}`
    case 'online-artist':
      return `online-artist:${s.artist.artist_id}`
    case 'online-collection':
      return `online-collection:${s.collection.collection_id}`
    default:
      return ''
  }
}

export const pinned = (list: Shortcut[], s: Shortcut) => list.some((x) => shortcutKey(x) === shortcutKey(s))

/** Pin it first, or take it off when pinned. */
export function togglePin(list: Shortcut[], s: Shortcut): Shortcut[] {
  const key = shortcutKey(s)
  const kept = list.filter((x) => shortcutKey(x))
  if (kept.some((x) => shortcutKey(x) === key)) return kept.filter((x) => shortcutKey(x) !== key)
  return [s, ...kept].slice(0, MOST)
}
