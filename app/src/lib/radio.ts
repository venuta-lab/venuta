import { useSyncExternalStore } from 'react'
import { catalogNow } from './catalog'
import {
  inApp,
  isSearchCancelled,
  onlineRadio,
  onlineRadioAt,
  onlineRadioMore,
  onlineRelated,
  onlineRelatedPage,
  onlineSearch,
  type OnlineRadio,
  type OnlineRelatedShelf,
  type OnlineTrack,
  type RadioEndpoint,
} from './engine'
import { undoOfDislike, upcomingCopies } from './dislikes'
import { listening, loadListening, updateListening } from './listening'
import { isLiked, setLiked } from './likes'
import { notify } from './notify'
import { onlineProblem } from './online'
import { ownedIndex } from './owned'
import { rateOnAccount } from './ytmAccount'
import { isLocalOnly } from './localOnly'
import { roomRoute } from './roomRoute'
import { addToRoom, togetherView } from './together'
import {
  appendItems,
  getPlayerState,
  isOnline,
  next,
  onPlayer,
  playItems,
  queueKey,
  readFrom,
  removeAt,
  replaceUpcoming,
  toItem,
  toRemoteItem,
  type From,
  type QueueItem,
} from './player'
import {
  afterChip,
  afterContinue,
  afterMarker,
  afterPage,
  afterRelated,
  ended,
  failed,
  hybridRows,
  listSession,
  loadSession,
  matchMemory,
  nextStep,
  radioSession,
  relatedAsk,
  saveSession,
  sessionShelf,
  type RadioSession,
  type Step,
} from './radioSession'

/*
  What plays next is YouTube Music's, the way its own app does it (lib/radioSession.ts has the
  rules):

  - One song played (search, Home, a link, history, the palette…), or Start radio on any song:
    that song, then its radio. A file of yours finds its YouTube Music song first (once, and only
    when it is the same song) and plays that song's radio.
  - The radio is one session, kept with the queue: more comes as the next page of that same
    session, added as YouTube sent it. Venuta drops nothing but songs you disliked and artists
    you blocked (the engine leaves those out).
  - An album or playlist that ends goes on, with Autoplay on, with what YouTube Music says it
    goes on with, then that radio's pages.
  - Play my copy when I have it (Settings): a song in the queue that is in your library as the
    same song plays from your file, in its place. Nothing is moved.
  - In a room the host's queue goes on the same way.
*/

// --- The session, kept, and told to whoever shows it ----------------------------------------

/** The session of the player's queue (keyed by its `from`), and the host's room's. */
let session: RadioSession | null = null
let roomSession: RadioSession | null = null
let roomCount = 0
/** A play of ours is setting its queue up: what comes next waits for it. */
let starting = false
/** A step is being asked for. */
let running = false
let version = 0
const listeners = new Set<() => void>()

const store = () => {
  try {
    return window.localStorage
  } catch {
    return null
  }
}
/** Files you started a radio from (or that stood in for a song), and their catalog song. */
const matches = matchMemory(store)
/** The sessions of queues something else replaced: Undo puts the queue back, and its radio
 *  goes on where it was. */
const replaced = sessionShelf()

function put(s: RadioSession | null, room = false) {
  if (room) roomSession = s
  else {
    if (session && session.key !== s?.key) replaced.keep(session)
    session = s
    saveSession(store, s)
  }
  version += 1
  listeners.forEach((l) => l())
}

const sessionOf = (room: boolean) => (room ? roomSession : session)

/** What the queue shows of the radio playing (for "Playing from", the chips, Autoplay's end). */
export interface RadioView {
  /** The queue's source in words ("Kitni Baatein radio", an album's name). */
  playingFrom: string
  /** YouTube Music's own name for it, when it gave one ("Kitni Baatein Radio"). */
  title: string | null
  /** The tuning chips, in YouTube's order; `selected` is the one in effect. Pick with `chooseChip`. */
  chips: { text: string; selected: boolean }[]
  /** Something is being asked for (more songs, a chip's). */
  loading: boolean
  /** Nothing more is coming, in words; null while more can come. */
  exhausted: string | null
  /** A radio (pages keep coming), not a list that goes on only at its end. */
  radio: boolean
  /** The host's room's queue. */
  room: boolean
}

let viewCache: { version: number; room: boolean; view: RadioView | null } | null = null

/** The radio playing now, as the queue shows it; null when there is no session. */
export function radioSessionNow(): RadioView | null {
  const room = !!roomRoute()
  if (viewCache && viewCache.version === version && viewCache.room === room) return viewCache.view
  const s = sessionOf(room)
  const view: RadioView | null = s
    ? {
        playingFrom: room ? (s.title ?? '') : (readFrom(s.key)?.label ?? s.title ?? ''),
        title: s.title,
        // Re-tuning replaces what's coming up in your own queue; a room's has no such step.
        chips: room ? [] : s.chips.map((c) => ({ text: c.text, selected: c.selected })),
        loading: s.loading,
        exhausted: s.exhausted,
        radio: s.kind === 'radio',
        room,
      }
    : null
  viewCache = { version, room, view }
  return view
}

/** Be told when the radio playing changes. Returns the way to stop. */
export function onRadioSession(l: () => void): () => void {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}

/** The radio playing now, for a component (see `RadioView`). */
export const useRadioSession = () => useSyncExternalStore(onRadioSession, radioSessionNow)

// --- Songs as queue rows -----------------------------------------------------------------

/** The catalog's songs as queue rows, in its order: your file for the same song when Play my
 *  copy is on (remembered as that song, for what comes after it), else the online song. */
function rowsOf(root: string, tracks: OnlineTrack[]): QueueItem[] {
  const mine = listening().play_my_copy ? catalogNow(root) : null
  const owns = mine ? ownedIndex(mine) : () => undefined
  return hybridRows(
    tracks,
    !!mine,
    (t) => {
      const yours = owns(t)
      if (yours && matches.get(yours.path) !== t.track_id) matches.set(yours.path, t.track_id)
      return yours
    },
    toItem,
    toRemoteItem,
  )
}

/** The catalog's id of an online row (a room host's own file is not the catalog's). */
const onlineIdOf = (q: QueueItem) =>
  isOnline(q) && q.online?.kind === 'online' && q.online.provider !== 'venuta_room' ? q.online.track_id : null

/** Songs searched for already this session and not found as the same song. */
const misses = new Set<string>()

interface SongInfo {
  path: string
  title: string
  artists: string[]
  album_artist: string
  duration_ms: number
  online_id?: string | null
}

/** The catalog's id for a song: its own when online, else the same song found online (once,
 *  then remembered). Only the same song (lib/owned.ts): a live version, a remix or a cover is
 *  not it, and nothing is guessed. Null when there is no such song. */
async function catalogIdOf(q: SongInfo): Promise<string | null> {
  if (q.online_id) return q.online_id
  const known = q.path ? matches.get(q.path) : undefined
  if (known) return known
  const artists = q.artists.length ? q.artists : q.album_artist ? [q.album_artist] : []
  const key = q.path || `${q.title}|${artists[0] ?? ''}`
  if (misses.has(key)) return null
  const found = await onlineSearch(`${q.title} ${artists[0] ?? ''}`.trim(), 'radio')
  const same = ownedIndex([{ title: q.title, artists, duration_ms: q.duration_ms }])
  const id = found.tracks.find((t) => same(t))?.track_id ?? null
  if (id && q.path) matches.set(q.path, id)
  if (!id) misses.add(key)
  return id
}

const rowInfo = (q: QueueItem): SongInfo => ({ path: q.path, title: q.title, artists: q.artists, album_artist: q.album_artist, duration_ms: q.duration_ms, online_id: onlineIdOf(q) })

/** Add rows where the session's queue is: the player's, or the host's room. */
const add = (root: string, rows: QueueItem[], room: boolean) => (!rows.length ? Promise.resolve() : room ? addToRoom(rows, false, true) : appendItems(root, rows))

// --- Starting music ----------------------------------------------------------------------

const fromText = (f: From) => JSON.stringify(f)

/** Play `items` from `index` as the queue of a new session: set up before anything asks for
 *  what comes next. `ask`: the radio's first answer, asked once the music is playing (null:
 *  `radio` is that answer already). */
async function begin(root: string, items: QueueItem[], index: number, from: From, radio: OnlineRadio | null, ask: (() => Promise<OnlineRadio>) | null, asked: RadioEndpoint | null) {
  const room = !!roomRoute()
  const key = room ? `room:${++roomCount}` : fromText(from)
  let played: Promise<unknown>
  starting = true
  try {
    played = playItems(root, items, index, from)
    put(radio ? radioSession(key, radio, asked) : { ...radioSession(key, { playlist_id: null, tracks: [] }, asked), loading: true }, room)
  } finally {
    starting = false
  }
  await played
  if (!ask) return
  try {
    const first = await ask()
    if (sessionOf(room)?.key !== key) return
    put(radioSession(key, first, asked), room)
    await add(root, rowsOf(root, first.tracks), room)
  } catch (e) {
    // The song plays on; what comes after it is asked for again once it nears its end.
    if (sessionOf(room)?.key === key) put(listSession(key, null), room)
    if (!isSearchCancelled(e)) notify(onlineProblem(e))
  }
}

/** Play a song and then its radio, as YouTube Music does when you play one song. `quiet`: no
 *  notice that a radio starts (a song tapped in a list, rather than Start radio). */
export async function startRadio(root: string, seed: QueueItem, opts: { quiet?: boolean } = {}) {
  if (isLocalOnly()) {
    if (opts.quiet) return void playItems(root, [seed], 0)
    return notify("Local only is on, so there's no radio from YouTube Music. Switch it off in Settings, Online.")
  }
  // A file of yours played on its own plays at once. With Autoplay on it then goes on with the
  // same song's YouTube Music radio: at once when that song is remembered, else once it is found
  // (Autoplay's next step, `run`'s marker). No same song there: it plays alone, nothing guessed.
  if (opts.quiet && !onlineIdOf(seed)) {
    const known = seed.path ? matches.get(seed.path) : undefined
    if (!known || !listening().autoplay) return void playItems(root, [seed], 0)
  }
  if (!opts.quiet) notify(`Starting a radio of “${seed.title}”…`)
  try {
    const id = onlineIdOf(seed) ?? (await catalogIdOf(rowInfo(seed)))
    // A file of yours with no same song on YouTube Music: tapped, it still plays; asked for its
    // radio, that is said, and nothing is guessed.
    if (!id && opts.quiet) return void playItems(root, [seed], 0)
    if (!id) return notify(`“${seed.title}” isn't on YouTube Music as the same song, so there's no radio of it.`)
    const list = `RDAMVM${id}`
    const from: From = { kind: 'radio', ref: list, label: `${seed.title} radio`, radio: list, seed: seed.title }
    await begin(root, [seed], 0, from, null, () => onlineRadio(id), { video_id: id, playlist_id: list, params: 'wAEB' })
  } catch (e) {
    // Finding a file's song failed (offline): a song tapped still plays.
    if (opts.quiet) return void playItems(root, [seed], 0)
    if (!isSearchCancelled(e)) notify(onlineProblem(e))
  }
}

/** Play one song the way YouTube Music does: it, then its radio. */
export const playWithRadio = (root: string, item: QueueItem) => startRadio(root, item, { quiet: true })

/** Play the queue a button names, as YouTube Music put it on the button (an artist's radio or
 *  shuffle, a mix): its endpoint, sent as it is. */
export async function playRadioAt(root: string, endpoint: RadioEndpoint, label: string) {
  if (isLocalOnly()) return notify("Local only is on, so there's no radio from YouTube Music. Switch it off in Settings, Online.")
  try {
    const radio = await onlineRadioAt(endpoint)
    const rows = rowsOf(root, radio.tracks)
    if (!rows.length) return notify(`YouTube Music has nothing in ${label}.`)
    const list = radio.playlist_id ?? endpoint.playlist_id ?? null
    await begin(root, rows, 0, { kind: 'radio', ref: list ?? label, label, radio: list }, radio, null, radio.endpoint ?? endpoint)
  } catch (e) {
    notify(onlineProblem(e))
  }
}

/** Play a radio the catalog names by its id alone (an artist's radio with no button to replay). */
export async function playRadioList(root: string, playlistId: string, label: string) {
  if (isLocalOnly()) return notify("Local only is on, so there's no radio from YouTube Music. Switch it off in Settings, Online.")
  try {
    const radio = await onlineRadio(null, playlistId)
    const rows = rowsOf(root, radio.tracks)
    if (!rows.length) return notify(`The online catalog has nothing in ${label}.`)
    const list = radio.playlist_id ?? playlistId
    await begin(root, rows, 0, { kind: 'radio', ref: playlistId, label, radio: list }, radio, null, radio.endpoint ?? { playlist_id: playlistId })
  } catch (e) {
    notify(onlineProblem(e))
  }
}

/** YouTube Music's Related songs for a song in the queue (the Related tab): the session's
 *  Related page when it is about that song, else the song's own. A file of yours asks for its
 *  same song's (found once, remembered). Nothing is asked in Local only or outside the app. */
export async function relatedSongs(item: QueueItem): Promise<OnlineTrack[]> {
  if (isLocalOnly() || !inApp()) return []
  const id = onlineIdOf(item) ?? (await catalogIdOf(rowInfo(item)))
  if (!id) return []
  const ask = relatedAsk(sessionOf(!!roomRoute()), id)
  return onlineRelated(ask.trackId, ask.relatedId)
}

/** The whole Related tab for a song in the queue, shelf by shelf as YouTube Music shows it: its
 *  songs, then the artists, albums and playlists it relates. Found the way `relatedSongs` finds
 *  the page; nothing is asked in Local only or outside the app. */
export async function relatedPage(item: QueueItem): Promise<OnlineRelatedShelf[]> {
  if (isLocalOnly() || !inApp()) return []
  const id = onlineIdOf(item) ?? (await catalogIdOf(rowInfo(item)))
  if (!id) return []
  const ask = relatedAsk(sessionOf(!!roomRoute()), id)
  return onlineRelatedPage(ask.trackId, ask.relatedId)
}

/** A catalog list id as the radio's own (`VL` is a page id's prefix, not the list's). */
export const listIdOf = (id: string | null | undefined) => (id ? id.replace(/^VL/, '') : null)

/** True for one of YouTube Music's own radios and mixes (`RD…`): they play as a radio. */
export const isRadioList = (id: string | null | undefined) => !!listIdOf(id)?.startsWith('RD')

// --- Tuning ------------------------------------------------------------------------------

/** Choose one of the radio's tuning chips (`index` in `RadioView.chips`): its songs replace what
 *  is coming up; the songs played and the one playing stay. */
export async function chooseChip(index: number) {
  const s = session
  const p = getPlayerState()
  if (roomRoute() || !s || s.key !== p.from || running) return
  const chip = s.chips[index]
  if (!chip || chip.selected) return
  const playing = p.queue[p.index]
  const id = playing ? (onlineIdOf(playing) ?? (playing.path ? matches.get(playing.path) : undefined) ?? null) : null
  running = true
  put({ ...s, loading: true })
  try {
    const radio = await onlineRadioAt(chip.endpoint, id)
    const now = session
    if (!now || now.key !== s.key) return
    const root = getPlayerState().root ?? ''
    put(afterChip(now, index, radio))
    await replaceUpcoming(root, rowsOf(root, radio.tracks))
  } catch (e) {
    if (session?.key === s.key) put({ ...session, loading: false })
    notify(onlineProblem(e))
  } finally {
    running = false
  }
}

// --- Dislikes and blocks -----------------------------------------------------------------

/** Dislike a song: radio and autoplay never play it again; it leaves Liked songs and what is
 *  coming up (as YouTube Music's radio does); if it is playing, the next song starts. With Likes
 *  and dislikes on, the account gets one thumbs down. Undo puts back the rating it had: a like
 *  comes back as a like (the songs taken out of the queue stay out). */
export async function dislike(root: string, q: QueueItem) {
  if (!(isOnline(q) && q.online?.kind === 'online')) return
  const key = `${q.online.provider}:${q.online.track_id}`
  await loadListening()
  const before = { liked: isLiked(q), disliked: listening().disliked.includes(key) }
  if (!before.disliked) await updateListening({ disliked: [...listening().disliked, key] })
  // The thumbs down replaces the like on the account: one rating, not an unlike and then it.
  if (before.liked) await setLiked(root, q, false, { account: false })
  void rateOnAccount(q, 'dislike')
  const s = getPlayerState()
  const playing = s.queue[s.index]
  if (!roomRoute()) for (const i of upcomingCopies(s.queue.map(queueKey), s.index, queueKey(q))) await removeAt(i)
  if (playing && queueKey(playing) === queueKey(q)) void next()
  notify(`Radio and autoplay won't play “${q.title}” again.`, {
    label: 'Undo',
    run: () => {
      const undo = undoOfDislike(before)
      if (undo.undislike) void updateListening({ disliked: listening().disliked.filter((d) => d !== key) })
      // Liked again here, and so (with the switch on) on the account too.
      if (undo.relike) void setLiked(root, q, true)
      else if (undo.account) void rateOnAccount(q, undo.account)
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

// --- Keeping it going --------------------------------------------------------------------

/** The session of a queue that has none kept: an album's or playlist's (by its catalog list,
 *  when it says one), or a radio from before sessions were kept, which goes on from its list. */
function sessionFor(key: string): RadioSession {
  const f = readFrom(key)
  return loadSession(store, key) ?? replaced.take(key) ?? listSession(key, listIdOf(f?.list ?? (f?.kind === 'radio' ? f.radio : null)))
}

/** Ask for what the step names and put it in the queue (`room`: the host's room's). */
async function run(step: Step, room: boolean, root: string, key: string, last: SongInfo | null) {
  const live = () => {
    const s = sessionOf(room)
    return s && s.key === key ? s : null
  }
  const start = live()
  if (!start) return
  put({ ...start, loading: true }, room)
  const lastId = async () => (last ? catalogIdOf(last) : null)
  try {
    switch (step.kind) {
      case 'page': {
        const page = await onlineRadioMore(step.endpoint, step.continuation)
        const s = live()
        if (!s) return
        put(afterPage(s, page), room)
        await add(root, rowsOf(root, page.tracks), room)
        return
      }
      case 'continue': {
        const radio = await onlineRadioAt(step.endpoint, await lastId())
        const s = live()
        if (!s) return
        put(afterContinue(s, radio), room)
        await add(root, rowsOf(root, radio.tracks), room)
        return
      }
      case 'marker': {
        const id = await lastId()
        const s = live()
        if (!s) return
        // A file with no song of its own on YouTube Music: nothing to ask what comes after it. It
        // ends quietly (the queue says nothing comes next); a notice as it starts would only puzzle.
        if (!id) return put(ended({ ...s, markerRead: true, relatedUsed: true }), room)
        // A list with no list of its own there (your files, a song played on its own): it goes on
        // with its last song's radio, as Start radio on that song would play it.
        if (!s.list) {
          const radio = await onlineRadio(id)
          const now = live()
          if (!now) return
          const songRadio: RadioEndpoint = { video_id: id, playlist_id: `RDAMVM${id}`, params: 'wAEB' }
          put(afterContinue({ ...now, continueWith: songRadio }, radio), room)
          await add(root, rowsOf(root, radio.tracks), room)
          return
        }
        const asked: RadioEndpoint = { video_id: id, playlist_id: s.list }
        const answer = await onlineRadioAt(asked, id)
        const now = live()
        if (!now) return
        // Answered with a radio already (a song on its own goes straight on into its mix, and so
        // does a radio list): that radio is what comes next.
        if ((answer.endpoint?.playlist_id ?? '').startsWith('RD') && answer.tracks.length) {
          put(afterContinue({ ...now, continueWith: answer.endpoint ?? null }, answer), room)
          await add(root, rowsOf(root, answer.tracks), room)
        } else put(afterMarker(now, answer), room)
        return
      }
      case 'related': {
        const id = await lastId()
        const songs = id ? await onlineRelated(id) : []
        const s = live()
        if (!s) return
        put(afterRelated(s), room)
        await add(root, rowsOf(root, songs), room)
        return
      }
      case 'end': {
        const s = ended(start)
        put(s, room)
        if (s.exhausted) notify(s.exhausted)
        return
      }
    }
  } catch {
    // Offline or refused: tried again in a while, three times, then the next thing.
    const s = live()
    if (s) put(failed(s, Date.now()), room)
  }
}

async function topUp() {
  if (starting || running || isLocalOnly()) return
  if (roomRoute()) return roomTopUp()
  if (roomSession) put(null, true)
  const s = getPlayerState()
  if (s.index < 0 || !s.queue.length) return
  const key = s.from
  if (!session || session.key !== key) put(sessionFor(key))
  const now = session
  if (!now) return
  const step = nextStep(now, { left: s.queue.length - 1 - s.index, autoplay: listening().autoplay, repeat: s.repeat, now: Date.now() })
  if (!step) return
  const last = s.queue[s.queue.length - 1]
  running = true
  try {
    await run(step, false, s.root ?? '', key, last ? rowInfo(last) : null)
  } finally {
    running = false
  }
}

/** In a room: the host's queue goes on the same way (only the host's, so a room never gets it
 *  twice). */
async function roomTopUp() {
  const v = togetherView()
  const q = v.room?.queue ?? []
  const t = v.room?.timeline
  if (!v.host || !t || !q.length) return
  if (!roomSession) put(listSession(`room:${++roomCount}`, null), true)
  const now = roomSession
  if (!now) return
  const step = nextStep(now, { left: q.length - 1 - t.index, autoplay: listening().autoplay, repeat: 'off', now: Date.now() })
  if (!step) return
  const last = q[q.length - 1]
  running = true
  try {
    await run(step, true, getPlayerState().root ?? '', now.key, {
      path: '',
      title: last.title,
      artists: last.artists,
      album_artist: '',
      duration_ms: last.duration_ms ?? 0,
      online_id: last.online_id,
    })
  } finally {
    running = false
  }
}

let started = false
/** Keep radios going and autoplay on from now (once, when the window starts). */
export function keepRadioGoing() {
  if (started) return
  started = true
  void loadListening()
  // Something new plays: its queue starts without a session (ours sets its own right after).
  window.addEventListener('venuta:played', () => {
    if (!roomRoute()) put(null)
  })
  onPlayer(() => void topUp())
}
