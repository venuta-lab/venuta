import { catalogNow } from './catalog'
import { isSearchCancelled, onlineRadio, onlineSearch, type OnlineTrack } from './engine'
import { listening, loadListening, updateListening } from './listening'
import { isLiked, setLiked } from './likes'
import { notify } from './notify'
import { onlineProblem } from './online'
import { ownedIndex } from './owned'
import { rateOnAccount } from './ytmAccount'
import {
  enqueueItems,
  getPlayerState,
  isOnline,
  next,
  onPlayer,
  playItems,
  queueKey,
  readFrom,
  toItem,
  toRemoteItem,
  type From,
  type QueueItem,
} from './player'

/*
  Radio and autoplay, the way a music app does them:

  - Start radio (any song, yours or online; an artist): the song, then songs like it from the
    online catalog. A song you have plays from your file. "Playing from <song> radio".
  - A radio never ends: when two songs are left, more come, never one already in the queue,
    never a song you disliked or an artist you blocked (the engine leaves those out).
  - Autoplay: any queue that runs out (an album, a playlist) goes on with a radio of its last
    song, when autoplay is on (Settings, and the queue's switch).
*/

/** Songs left in the queue (after the playing one) when more are asked for. */
const LEFT = 2

/** The catalog's id for a song: its own when online, else the same song found online. */
async function catalogIdOf(q: QueueItem): Promise<string | null> {
  if (isOnline(q) && q.online?.kind === 'online') return q.online.track_id
  const found = await onlineSearch(`${q.title} ${q.artists[0] ?? q.album_artist ?? ''}`.trim(), 'radio')
  const same = ownedIndex([{ title: q.title, artists: q.artists, duration_ms: q.duration_ms }])
  return (found.tracks.find((t) => same(t)) ?? found.tracks[0])?.track_id ?? null
}

/** Radio songs as queue rows: your file when you have the song, else online; each once. */
function rowsOf(root: string, tracks: OnlineTrack[], skip: Set<string>): QueueItem[] {
  const mine = catalogNow(root)
  const owns = mine ? ownedIndex(mine) : () => undefined
  const out: QueueItem[] = []
  for (const t of tracks) {
    const yours = owns(t)
    const row = yours ? toItem(yours) : toRemoteItem(t)
    const key = queueKey(row)
    if (skip.has(key)) continue
    skip.add(key)
    out.push(row)
  }
  return out
}

/** Play a radio of this song: it, then songs like it. */
export async function startRadio(root: string, seed: QueueItem) {
  notify(`Starting a radio of “${seed.title}”…`)
  try {
    const id = await catalogIdOf(seed)
    if (!id) return notify(`The online catalog has no radio for “${seed.title}”.`)
    const radio = await onlineRadio(id)
    const rows = rowsOf(root, radio.tracks, new Set([queueKey(seed)]))
    if (!rows.length) return notify(`The online catalog has no radio for “${seed.title}”.`)
    const from: From = { kind: 'radio', ref: radio.playlist_id ?? id, label: `${seed.title} radio`, radio: radio.playlist_id, seed: seed.title }
    await playItems(root, [seed, ...rows], 0, from)
  } catch (e) {
    if (!isSearchCancelled(e)) notify(onlineProblem(e))
  }
}

/** Play a radio the catalog names (an artist's radio, a mix). */
export async function playRadioList(root: string, playlistId: string, label: string) {
  try {
    const radio = await onlineRadio(null, playlistId)
    const rows = rowsOf(root, radio.tracks, new Set())
    if (!rows.length) return notify(`The online catalog has nothing in ${label}.`)
    await playItems(root, rows, 0, { kind: 'radio', ref: playlistId, label, radio: radio.playlist_id ?? playlistId })
  } catch (e) {
    notify(onlineProblem(e))
  }
}

/** Dislike a song: radio and autoplay never play it again; it leaves Liked songs; if it is
 *  playing, the next song starts. */
export async function dislike(root: string, q: QueueItem) {
  if (!(isOnline(q) && q.online?.kind === 'online')) return
  const key = `${q.online.provider}:${q.online.track_id}`
  await loadListening()
  if (!listening().disliked.includes(key)) await updateListening({ disliked: [...listening().disliked, key] })
  if (isLiked(q)) await setLiked(root, q, false)
  void rateOnAccount(q, 'dislike')
  const s = getPlayerState()
  const playing = s.queue[s.index]
  if (playing && queueKey(playing) === queueKey(q)) void next()
  notify(`Radio and autoplay won't play “${q.title}” again.`, {
    label: 'Undo',
    run: () => {
      void updateListening({ disliked: listening().disliked.filter((d) => d !== key) })
      void rateOnAccount(q, 'none')
    },
  })
}

/** Block an artist: radio, autoplay and suggestions never offer them. */
export async function blockArtist(name: string, artistId: string | null = null) {
  await loadListening()
  const had = listening().blocked_artists
  if (had.some((b) => b.name.toLowerCase() === name.toLowerCase())) return notify(`${name} is blocked already.`)
  await updateListening({ blocked_artists: [...had, { name, artist_id: artistId }] })
  notify(`Radio, autoplay and suggestions won't offer ${name}.`, {
    label: 'Undo',
    run: () => void updateListening({ blocked_artists: listening().blocked_artists.filter((b) => b.name !== name) }),
  })
}

// --- Keeping a radio going, and autoplay ---------------------------------------------------

let asking = false
/** What more was last asked after (so a failed answer isn't asked again at once). */
let askedAfter = ''

async function topUp() {
  const s = getPlayerState()
  if (asking || s.index < 0 || !s.queue.length || s.repeat !== 'off') return
  const left = s.queue.length - 1 - s.index
  const from = readFrom(s.from)
  const radio = from?.kind === 'radio' && from.radio ? from.radio : null
  // A radio tops up early; any other queue only once its last song is playing.
  if (radio ? left > LEFT : left > 0) return
  if (!radio) {
    await loadListening()
    if (!listening().autoplay) return
  }
  const last = s.queue[s.queue.length - 1]
  const lastKey = queueKey(last)
  if (askedAfter === lastKey) return
  asking = true
  askedAfter = lastKey
  try {
    const root = s.root ?? ''
    const id = await catalogIdOf(last)
    if (!id && !radio) return
    const more = await onlineRadio(id, radio)
    const rows = rowsOf(root, more.tracks, new Set(getPlayerState().queue.map(queueKey)))
    if (rows.length) await enqueueItems(root, rows, false)
  } catch {
    // Offline or refused: the queue ends as it would have; the next song played asks again.
  } finally {
    asking = false
  }
}

let started = false
/** Keep radios going and autoplay on from now (once, when the window starts). */
export function keepRadioGoing() {
  if (started) return
  started = true
  void loadListening()
  onPlayer(() => void topUp())
}
