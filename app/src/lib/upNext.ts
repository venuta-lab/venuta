/*
  What the queue shows, as YouTube Music's player shows Up next: the queue's name ("Playing from
  Kitni Baatein Radio"), its tuning chips, a word while more is coming or when nothing more will,
  and every song in it, played ones above the one playing and the ones to come below.

  Pure (no window, no engine), so tests/upNext.test.ts reads it; components/player/UpNext.tsx
  draws it.
*/

import type { OnlineAlbum, OnlineRelatedShelf, OnlineSuggestion, OnlineTrack } from './engine.ts'
import { knownLength } from './onlineSong.ts'
import type { QueueItem } from './player.ts'
import type { RadioView } from './radio.ts'

/** Where a row is: played already, playing now, or to come. */
export type Place = 'played' | 'current' | 'next'

/** The queue's name for "Playing from": YouTube Music's own name for its radio, else the words
 *  it was started with. Null for a list (an album, a playlist), which says where it came from as
 *  a link (components/player/PlayingFrom.tsx). */
export function queueName(view: RadioView | null): string | null {
  if (!view?.radio) return null
  return view.title?.trim() || view.playingFrom.trim() || null
}

/** The tuning chips to show, in YouTube's order: none in a room (they re-tune your own queue). */
export const chipsShown = (view: RadioView | null) => (view && !view.room ? view.chips : [])

/** Under the queue: a word while a page or a chip is coming, the end in words when nothing more
 *  will, else nothing. */
export function queueNote(view: RadioView | null): string | null {
  if (!view) return null
  if (view.loading) return 'Getting more from YouTube Music…'
  return view.exhausted
}

export const placeOf = (at: number, index: number): Place => (at < index ? 'played' : at === index ? 'current' : 'next')

/** Play next is offered for a song that isn't playing or coming next already. */
export const canPlayNext = (at: number, index: number) => at !== index && at !== index + 1

/** A row as a screen reader says it: the song, who, how long, where it plays from, and where it
 *  is in the queue when not to come. */
export function rowLabel(r: { title: string; artists: string[]; length: string; where: string | null; place: Place }) {
  const where = r.place === 'current' ? 'playing now' : r.place === 'played' ? 'played' : ''
  return [r.title, ...r.artists, r.length, r.where ?? '', where].filter(Boolean).join(', ')
}

/** A row's length: as the song was listed, else as it said when it was opened (`opened`), else
 *  the player's while it plays (`playing`, 0 for another row); nothing while none is known. */
export const rowLength = (listed: number, opened: number | undefined, playing: number) => knownLength(listed || opened || playing)

/** Lengths of songs opened, by `queueKey`, kept for their rows: at most this many. */
const LENGTHS_KEPT = 2000

/** Keep the length a song said when it opened, for its row (a song listed without one). */
export function learnLength(lengths: Map<string, number>, key: string | null, ms: number) {
  if (!key || !(ms > 0) || lengths.get(key) === ms) return
  lengths.delete(key)
  lengths.set(key, ms)
  for (const old of lengths.keys()) {
    if (lengths.size <= LENGTHS_KEPT) break
    lengths.delete(old)
  }
}

/** The name suggested for the queue saved as a playlist: the radio's, else what it plays from. */
export function saveName(view: RadioView | null, fromLabel: string): string {
  return view?.title?.trim() || fromLabel.replace(/^Online: /, '').trim()
}

/** What a queued row needs for Go to album online. */
type QueuedSong = Pick<QueueItem, 'path' | 'title' | 'artists' | 'album' | 'album_id' | 'artwork_url' | 'online'>

/** The online album a queued online song is from, for Go to album: the one YouTube named with
 *  the song. Null for a file of yours (it goes to its folder), a song a room's host plays from
 *  their file, and an online song YouTube named no album for. */
export function onlineAlbumOf(q: QueuedSong): OnlineAlbum | null {
  const id = q.album_id?.trim()
  if (q.path || q.online?.kind !== 'online' || q.online.provider === 'venuta_room' || !id) return null
  return {
    provider: q.online.provider,
    album_id: id,
    title: q.album || q.title,
    artists: q.artists,
    track_count: null,
    release_date: null,
    artwork_url: q.artwork_url ?? null,
  }
}

/** One shelf of Related as the tab draws it: a list of songs, a row of cards (artists, albums,
 *  playlists), or the words about the artist. */
export type RelatedPart =
  | { kind: 'songs'; title: string; tracks: OnlineTrack[] }
  | { kind: 'cards'; title: string; items: OnlineSuggestion[] }
  | { kind: 'about'; title: string; about: string }

/** Related's shelves in YouTube's order, each as what it holds; one with nothing is left out. */
export function relatedParts(shelves: OnlineRelatedShelf[]): RelatedPart[] {
  return shelves.flatMap((s): RelatedPart[] => {
    const title = s.title.trim()
    if (s.tracks.length) return [{ kind: 'songs', title, tracks: s.tracks }]
    if (s.items.length) return [{ kind: 'cards', title, items: s.items }]
    const about = s.about?.trim()
    return about ? [{ kind: 'about', title, about }] : []
  })
}
