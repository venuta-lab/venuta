/*
  What the queue shows, as YouTube Music's player shows Up next: the queue's name ("Playing from
  Kitni Baatein Radio"), its tuning chips, a word while more is coming or when nothing more will,
  and every song in it, played ones above the one playing and the ones to come below.

  Pure (no window, no engine), so tests/upNext.test.ts reads it; components/player/UpNext.tsx
  draws it.
*/

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

/** The name suggested for the queue saved as a playlist: the radio's, else what it plays from. */
export function saveName(view: RadioView | null, fromLabel: string): string {
  return view?.title?.trim() || fromLabel.replace(/^Online: /, '').trim()
}
