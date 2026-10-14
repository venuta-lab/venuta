import { ensureCatalog } from '../../lib/catalog'
import { playlistQueue, smartSave, type Availability, type Playable, type PlaylistEntry, type PlaylistSummary, type SmartRule } from '../../lib/engine'
import { enqueueItems, fromPlayable, playAllItems, type From, type QueueItem } from '../../lib/player'

/** "Spotify 2026-10-13" → "Spotify". */
export const sourceName = (source: string | null | undefined) => (source ?? '').replace(/\s+\d{4}-\d{2}-\d{2}$/, '').trim()

/** Where a playlist came from, to follow "From": "Spotify", "another player". */
export const fromWhere = (source: string | null | undefined) => {
  const s = sourceName(source)
  return s === 'Playlist file' ? 'another player' : s
}

/** "Spotify 2026-10-13" → "13 Oct 2026". */
export function sourceDate(source: string | null | undefined): string {
  const m = /(\d{4})-(\d{2})-(\d{2})$/.exec(source ?? '')
  if (!m) return ''
  return new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3])).toLocaleDateString('en-IN', { day: 'numeric', month: 'short', year: 'numeric' })
}

/** "2 hr 14 min", "38 min". */
export function length(ms: number): string {
  const min = Math.round(ms / 60000)
  if (min < 60) return `${min} min`
  return `${Math.floor(min / 60)} hr ${min % 60} min`
}

export const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString('en-IN')} ${n === 1 ? one : many}`

/**
 * A playlist's songs ready to play, in order, as the engine picks them (`playables`): your file
 * when it's here, else the online song; songs still to get are left out, so playing on goes
 * from one to the next whichever kind it is. A smart playlist without its file yet (a library
 * with no playlists folder) is written first, which is what opening it means.
 */
export async function playlistSongs(root: string, name: string, rule?: SmartRule): Promise<QueueItem[]> {
  let found: Playable[]
  try {
    found = await playlistQueue(root, name)
  } catch (e) {
    if (!rule) throw e
    await smartSave(root, rule)
    found = await playlistQueue(root, name)
  }
  await ensureCatalog(root)
  return queueOf(found)
}

/** The engine's playable songs as queue rows (a library song the catalog doesn't know yet waits). */
export const queueOf = (found: Playable[]): QueueItem[] => found.map(fromPlayable).filter((q): q is QueueItem => q !== null)

/** Where a playlist entry plays from: your file, the online song, or (an online song you have)
 *  your file first. None for a song still to get. */
export const entryAvailability = (e: PlaylistEntry): Availability | undefined =>
  e.online ? (e.missing ? 'online' : 'local_preferred') : e.missing ? undefined : 'local'

/** Where a queue from a playlist came from (for "Playing from", and to follow its edits). */
export const playlistFrom = (name: string): From => ({ kind: 'playlist', ref: name, label: name })

export type PlaylistAction = 'play' | 'shuffle' | 'next' | 'queue'

/** Play a whole playlist, shuffle it, or line it up. */
export async function playPlaylist(root: string, p: Pick<PlaylistSummary, 'name'>, how: PlaylistAction, rule?: SmartRule) {
  const songs = await playlistSongs(root, p.name, rule)
  if (!songs.length) return 0
  if (how === 'play' || how === 'shuffle') await playAllItems(root, songs, playlistFrom(p.name), how === 'play' ? 'order' : 'shuffle')
  else await enqueueItems(root, songs, how === 'next')
  return songs.length
}
