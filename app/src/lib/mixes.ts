/*
  YouTube Music's own mixes on Home and in Mixed for you (My Supermix, My Mix, Discover, Replay,
  any `RD…` list): playing one plays what its own play button plays. Pure, so tests/mixes.test.ts
  reads it; screens/listen/OnlineCards.tsx plays it (lib/radio.ts `playRadioAt`).
*/

import type { OnlineCollection, RadioEndpoint } from './engine.ts'

/** The radio a playlist card plays: a mix's own play button's endpoint (its `params` and all),
 *  else the radio its list id names. Null for a playlist or a podcast, which play their songs. */
export function radioOfCard(c: Pick<OnlineCollection, 'kind' | 'collection_id' | 'play'>): RadioEndpoint | null {
  if (c.kind === 'podcast') return null
  if (c.play?.playlist_id?.startsWith('RD')) return c.play
  const list = c.collection_id.replace(/^VL/, '')
  return list.startsWith('RD') ? { playlist_id: list } : null
}
