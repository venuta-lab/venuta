import { Channel, invoke } from '@tauri-apps/api/core'
import { useEffect, useState, useSyncExternalStore } from 'react'
import { trackAt } from './catalog'
import {
  editTags,
  inApp,
  type Availability,
  type CatalogTrack,
  type Gain,
  type OnlineEntry,
  type OnlineTrack,
  type Playable,
  type PlayableId,
  type PlaylistItem,
} from './engine'
import { notify } from './notify'
import { isAwake, onAwake } from './motion'

/*
  The player, app-wide. Screens call play / toggle / next…; the engine (in the backend) owns
  the real state and streams it back here, so every screen shows the same thing.
*/

export type Repeat = 'off' | 'all' | 'one'

export type { Availability, PlayableId }

/** A song in the queue: what the bar and Now Playing need to show it.
 *
 *  Hybrid — a local row carries `path`; a remote row carries `online` (a stable id) and
 *  metadata only (no stream URL). The remaining fields are optional for remote rows. */
export interface QueueItem {
  path: string
  dir: string
  title: string
  artists: string[]
  album: string
  album_artist: string
  duration_ms: number
  format: string
  bit_depth: number
  sample_rate: number
  lyrics: boolean
  stars: number
  rg: Gain
  /** Set on remote rows; stable provider id, never a stream URL. */
  online?: PlayableId
  /** Cover art address for remote rows. */
  artwork_url?: string
  /** Filled in by the backend at play time. */
  availability?: Availability
}

/** The sound tools (all off by default: bit-perfect). dB values. */
export interface Sound {
  eq: boolean
  bands: number[]
  preset: string
  preamp_db: number
  bass_db: number
  treble_db: number
  leveling: 'off' | 'track' | 'album'
  leveling_preamp_db: number
  limiter: boolean
  /** Seconds of blend between songs (0: off). Never within one album. */
  crossfade_s: number
}

export const SOUND_OFF: Sound = {
  eq: false,
  bands: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  preset: 'Flat',
  preamp_db: 0,
  bass_db: 0,
  treble_db: 0,
  leveling: 'off',
  leveling_preamp_db: 0,
  limiter: true,
  crossfade_s: 0,
}

/** Where the sound goes and what happens on the way (for the badge and the signal path). */
export interface Signal {
  device: string
  /** "exclusive": straight to the device, at the song's own format. "shared": via Windows' mixer. */
  mode: 'exclusive' | 'shared' | 'memory'
  source: { rate: number; channels: number; bits: number; lossless: boolean; codec: string }
  /** About the file: size, average bitrate, the decoder. */
  file: { size_bytes: number; bitrate_kbps: number; decoder: string }
  /** Decoded ahead, waiting to play. */
  buffer_ms: number
  /** Held by the device before you hear it. */
  latency_ms: number
  output: { rate: number; channels: number; container_bits: number; valid_bits: number; kind: 'int' | 'float' }
  /** Nothing changes the samples on the way. */
  bit_perfect: boolean
  /** What does change them, in words ("Volume at 60%"). */
  steps: string[]
}

export interface PlayerState {
  queue: QueueItem[]
  index: number
  playing: boolean
  /** Current song still opening (remote body downloading). */
  buffering: boolean
  position_ms: number
  /** The playing song's length, as the file says (0 until known). */
  duration_ms: number
  shuffle: boolean
  repeat: Repeat
  /** 0–1. */
  volume: number
  signal: Signal | null
  /** Where the queue came from: a `From` as JSON (see `readFrom`), or plain words from before. */
  from: string
  /** The last problem, in words (a file that couldn't be opened…). */
  error: string
  sound: Sound
  stop_after_current: boolean
  /** Seconds until the sleep timer pauses. */
  sleep_in_s: number | null
  /** The folder the queue's songs are in. */
  root?: string
  /** Band levels of what's playing (0-255, bass to treble) while a spectrum display asks. */
  bands?: number[] | null
}

const idle: PlayerState = {
  queue: [],
  index: -1,
  playing: false,
  buffering: false,
  position_ms: 0,
  duration_ms: 0,
  shuffle: false,
  repeat: 'off',
  volume: 1,
  signal: null,
  from: '',
  error: '',
  sound: SOUND_OFF,
  stop_after_current: false,
  sleep_in_s: null,
}

let state: PlayerState = idle
/** When the last report came (performance.now()), to tell the position between reports. */
let reportedAt = 0
const listeners = new Set<() => void>()
const tell = () => listeners.forEach((l) => l())

/** Everything in a report but where the song is (and the sound's levels and path, which move with it). */
const settled = (s: PlayerState) =>
  `${s.index}|${s.playing}|${s.buffering}|${s.duration_ms}|${s.shuffle}|${s.repeat}|${s.volume}|${s.from}|${s.error}|${s.stop_after_current}|${s.sleep_in_s}|${JSON.stringify(s.sound)}`

// A window that isn't on screen (lib/motion.ts) isn't drawn: while it is away, a report that only
// moves the position is kept (getPlayerState has it) but not told; a new song, play or pause, the
// queue or any setting is told at once. Coming back, it catches up in one go.
let held = false
const set = (s: PlayerState) => {
  const was = state
  state = s
  reportedAt = performance.now()
  if (!isAwake() && s.queue === was.queue && settled(s) === settled(was)) {
    held = true
    return
  }
  held = false
  tell()
}
onAwake(() => {
  if (held && isAwake()) {
    held = false
    tell()
  }
})

/** The player's state now, outside React. */
export const getPlayerState = () => state

/** The song's position to the millisecond: the last report plus the time since (while playing). */
export const positionNow = () =>
  state.playing && !state.buffering
    ? state.position_ms + Math.min(250, performance.now() - reportedAt)
    : state.position_ms

let subscribed = false
/** Start listening to the engine (once). */
export function connectPlayer() {
  if (subscribed || !inApp()) return
  subscribed = true
  // The queue comes only when it changed; everything else every time.
  const channel = new Channel<Omit<PlayerState, 'queue'> & { queue: QueueItem[] | null }>()
  channel.onmessage = (m) => set({ ...m, queue: m.queue ?? state.queue })
  invoke('player_subscribe', { onState: channel }).catch(() => {
    subscribed = false
  })
}

export const toItem = (t: CatalogTrack): QueueItem => ({
  path: t.path,
  dir: t.dir,
  title: t.title,
  artists: t.artists,
  album: t.album,
  album_artist: t.album_artist,
  duration_ms: t.duration_ms,
  format: t.format,
  bit_depth: t.bit_depth,
  sample_rate: t.sample_rate,
  lyrics: t.lyrics,
  stars: t.stars,
  rg: t.rg,
})

/** A remote-catalog track to enqueue: stable id + display metadata, no stream URL. */
export interface RemoteTrack {
  provider: string
  track_id: string
  title: string
  artists?: string[]
  album?: string | null
  duration_ms?: number | null
  artwork_url?: string | null
}

/** Cover for an online id: catalog picture, else YouTube hqdefault when the id is a video id. */
export function onlineCoverUrl(t: { track_id: string; artwork_url?: string | null }): string | undefined {
  const raw = (t.artwork_url ?? '').trim()
  if (raw) {
    let url = raw
    if (url.startsWith('//')) url = `https:${url}`
    else if (url.startsWith('http://')) url = `https://${url.slice(7)}`
    if (/^https:\/\//i.test(url) && !/googlevideo\.com|mime=audio|\.m3u8/i.test(url)) return url
  }
  // Stable YouTube-style id → always have a picture in search / Now Playing / SMTC.
  if (/^[\w-]{11}$/.test(t.track_id)) return `https://i.ytimg.com/vi/${t.track_id}/hqdefault.jpg`
  return undefined
}

/** Build a queue row for an online track. Leaves `path`/`dir` empty so the backend treats it
 *  as remote and resolves its stream at play time. */
export const toRemoteItem = (t: RemoteTrack): QueueItem => ({
  path: '',
  dir: '',
  title: t.title,
  artists: t.artists ?? [],
  album: t.album ?? '',
  album_artist: '',
  duration_ms: t.duration_ms ?? 0,
  format: '',
  bit_depth: 0,
  sample_rate: 0,
  lyrics: false,
  stars: 0,
  rg: { track_db: null, album_db: null, track_peak: null, album_peak: null },
  online: { kind: 'online', provider: t.provider, track_id: t.track_id },
  artwork_url: onlineCoverUrl(t),
  availability: 'online',
})

/** A queue row for a playlist's online song: its stable id only; the stream resolves at play time. */
export const onlineItem = (o: OnlineEntry): QueueItem =>
  toRemoteItem({
    provider: o.provider,
    track_id: o.track_id,
    title: o.title,
    artists: o.artists,
    album: o.album,
    duration_ms: o.duration_ms,
    artwork_url: o.artwork_url,
  })

/** Is this row an online song (no file of yours behind it)? */
export const isOnline = (q: QueueItem) => !q.path && q.online?.kind === 'online'

/** Cover picture for a queue row: online artwork URL when set, else nothing (local uses covers). */
export const remoteArtUrl = (q: QueueItem | null | undefined): string | undefined =>
  q && isOnline(q) ? q.artwork_url : undefined

/** What tells queue rows apart: the library path, or the online song's stable id. */
export const queueKey = (q: QueueItem) => (isOnline(q) && q.online?.kind === 'online' ? `online:${q.online.provider}:${q.online.track_id}` : q.path)

/** A song ready to play (from `playlistQueue`) as a queue row: your file with all the library
 *  knows about it, or the online song. Null for a library song the catalog doesn't have yet. */
export function fromPlayable(p: Playable): QueueItem | null {
  if (p.id.kind === 'online')
    return toRemoteItem({ ...p.id, title: p.title, artists: p.artists, album: p.album, duration_ms: p.duration_ms, artwork_url: p.artwork_url })
  const t = trackAt(p.id.path)
  return t ? { ...toItem(t), availability: p.availability ?? 'local' } : null
}

/** The online song a row is, as a playlist keeps it (stable id and what to show). */
export function onlineEntryOf(q: QueueItem): OnlineEntry | null {
  if (!isOnline(q) || q.online?.kind !== 'online') return null
  return {
    provider: q.online.provider,
    track_id: q.online.track_id,
    title: q.title,
    artists: q.artists,
    album: q.album || undefined,
    duration_ms: q.duration_ms || undefined,
    artwork_url: q.artwork_url,
  }
}

/** An online catalog song as a playlist keeps it (cover picture may stay; never a stream URL). */
export const onlineEntryFrom = (t: OnlineTrack): OnlineEntry => ({
  provider: t.provider,
  track_id: t.track_id,
  title: t.title,
  artists: t.artists,
  album: t.album ?? undefined,
  duration_ms: t.duration_ms ?? undefined,
  isrc: t.isrc ?? undefined,
  artwork_url: onlineCoverUrl(t),
})

/** A queue row as a playlist line: the online song by its id, else the library file. */
export const songItemOf = (q: QueueItem): PlaylistItem => {
  const o = onlineEntryOf(q)
  return o ? { online: o } : { path: q.path }
}

const call = (cmd: string, args: Record<string, unknown> = {}) =>
  inApp() ? invoke(cmd, args).catch((e) => set({ ...state, error: String(e) })) : Promise.resolve()

/** The folder the queue's songs are in. */
let queueRoot = ''

/** Where a queue came from: "Playing from Hindi", and the way back to it. */
export interface From {
  kind: 'playlist' | 'album' | 'artist' | 'songs' | 'search'
  /** The playlist's name, the album's folder, the artist's name, the words searched. */
  ref: string
  /** In words: "Hindi", "Taal", "Search: rahman". */
  label: string
}
const fromText = (f: From | string) => (typeof f === 'string' ? f : JSON.stringify(f))
/** What the player's `from` says (older saves are plain words: a label only). */
export function readFrom(s: string): From | null {
  if (!s) return null
  if (s.startsWith('{'))
    try {
      const v = JSON.parse(s) as From
      if (v && typeof v.label === 'string' && typeof v.kind === 'string') return v
    } catch {
      /* plain words */
    }
  return { kind: 'songs', ref: '', label: s }
}
/** Is the queue from this playlist (or album…)? */
export const fromIs = (s: string, kind: From['kind'], ref: string) => {
  const f = readFrom(s)
  return !!f && f.kind === kind && f.ref === ref
}

/** What the queue was last set to, to follow its source's changes (see `follow`). */
let lastSet: { from: string; keys: string[] } | null = null

/** Play these songs from `index`, replacing the queue. Shuffle stays as it is (a song picked
 *  from a list while shuffling plays, then the rest shuffled). */
export const play = (root: string, tracks: CatalogTrack[], index = 0, from: From | string = '') =>
  playItems(root, tracks.map(toItem), index, from)

/** `play` for queue rows of any kind: library songs and online ones, mixed. */
export function playItems(root: string, items: QueueItem[], index = 0, from: From | string = '') {
  queueRoot = root
  const f = fromText(from)
  const row = items[index]
  // Same queue + same song again: rewind without a full reopen (uses in-memory resolve cache too).
  if (
    row &&
    state.queue.length === items.length &&
    state.index === index &&
    items.every((it, i) => queueKey(it) === queueKey(state.queue[i]!)) &&
    queueKey(row) === queueKey(state.queue[index]!)
  ) {
    set({ ...state, playing: true, position_ms: 0, from: f, error: '' })
    return call('player_jump', { index }).then(() => call('player_seek', { ms: 0 }))
  }
  lastSet = { from: f, keys: items.map(queueKey) }
  const opening = !!row && isOnline(row)
  // Show the queue at once. Online rows stay in Opening… until the engine has real samples.
  set({
    ...state,
    queue: items,
    index,
    playing: true,
    buffering: opening,
    position_ms: 0,
    duration_ms: row?.duration_ms || 0,
    from: f,
    error: '',
  })
  return call('player_play', { root, items, index, from: f })
}

/** The Play and Shuffle buttons: all of it in order (shuffle off), or shuffled from a random
 *  song (shuffle on, so the bar shows it and turning it off goes back to the order). */
export const playAll = (root: string, tracks: CatalogTrack[], from: From | string, how: 'order' | 'shuffle' = 'order') =>
  playAllItems(root, tracks.map(toItem), from, how)

/** `playAll` for queue rows of any kind. */
export async function playAllItems(root: string, items: QueueItem[], from: From | string, how: 'order' | 'shuffle' = 'order') {
  if (!items.length) return
  const shuffle = how === 'shuffle'
  if (state.shuffle !== shuffle) {
    set({ ...state, shuffle })
    await call('player_options', { shuffle, repeat: state.repeat })
  }
  return playItems(root, items, shuffle ? Math.floor(Math.random() * items.length) : 0, from)
}

/**
 * A source changed while it plays (a playlist edited): the song playing plays on, and what
 * comes after follows the new list. Only when the queue is still the one the source made
 * (nothing added or taken out by hand); otherwise the queue is yours and is left alone.
 */
export const follow = (root: string, tracks: CatalogTrack[], from: From) => followItems(root, tracks.map(toItem), from)

/** `follow` for queue rows of any kind (songs are told apart by `queueKey`). */
export function followItems(root: string, next: QueueItem[], from: From) {
  const f = fromText(from)
  if (!lastSet || lastSet.from !== f || !fromIs(state.from, from.kind, from.ref)) return
  const cur = state.index >= 0 ? state.queue[state.index] : undefined
  if (!cur) return
  const queued = state.queue.map(queueKey).sort()
  const made = [...lastSet.keys].sort()
  if (queued.length !== made.length || queued.some((p, i) => p !== made[i])) return
  const keys = next.map(queueKey)
  if (keys.length === lastSet.keys.length && keys.every((p, i) => p === lastSet!.keys[i])) return
  const items = next.slice()
  const curKey = queueKey(cur)
  let index = keys.indexOf(curKey)
  if (index < 0) {
    // The playing song left the list: it plays on, where it was among the songs still there.
    const was = lastSet.keys.indexOf(curKey)
    const before = new Set(lastSet.keys.slice(0, Math.max(0, was)))
    index = keys.filter((p) => before.has(p)).length
    items.splice(index, 0, cur)
  }
  lastSet = { from: f, keys: items.map(queueKey) }
  return call('player_follow', { root, items, index })
}
export const toggle = () => call('player_toggle')
export const next = () => call('player_next')
export const previous = () => call('player_previous')
export const seek = (ms: number) => {
  // An online song still opening has nothing to seek in yet: every seek (bar, keys, lyrics,
  // mini player) waits until it plays.
  if (state.buffering) return Promise.resolve()
  set({ ...state, position_ms: ms })
  return call('player_seek', { ms: Math.max(0, Math.round(ms)) })
}
export const setVolume = (v: number) => {
  const volume = Math.min(1, Math.max(0, v))
  set({ ...state, volume })
  return call('player_volume', { volume })
}
export const setShuffle = (shuffle: boolean) => call('player_options', { shuffle, repeat: state.repeat })
export const setRepeat = (repeat: Repeat) => call('player_options', { shuffle: state.shuffle, repeat })
/** After the current song (next) or at the end (not next). */
export const enqueue = (root: string, tracks: CatalogTrack[], nextUp: boolean) => enqueueItems(root, tracks.map(toItem), nextUp)
/** `enqueue` for queue rows of any kind. */
export const enqueueItems = (root: string, items: QueueItem[], nextUp: boolean) => call('player_enqueue', { root, items, next: nextUp })
export const removeAt = (i: number) => call('player_remove', { index: i })
export const move = (from: number, to: number) => call('player_move', { from, to })
export const jumpTo = (i: number) => call('player_jump', { index: i })

// For checking playback from the inspector (development only).
if (import.meta.env.DEV) (window as unknown as { __venutaPlayer: () => PlayerState }).__venutaPlayer = () => state

export function usePlayer(): PlayerState {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => {
        listeners.delete(l)
      }
    },
    () => state,
  )
}

/** Just one value from the player: re-renders only when that value changes. */
export function usePlayerValue<T>(pick: (s: PlayerState) => T): T {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => {
        listeners.delete(l)
      }
    },
    () => pick(state),
  )
}

/** The song playing (or paused) now. */
export const current = (s: PlayerState) => (s.index >= 0 ? s.queue[s.index] : undefined)

/** 3:05, 1:02:10. */
export function clock(ms: number): string {
  const t = Math.max(0, Math.floor(ms / 1000))
  const h = Math.floor(t / 3600)
  const m = Math.floor((t % 3600) / 60)
  const s = String(t % 60).padStart(2, '0')
  return h ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`
}

export interface OutputDevice {
  id: string
  name: string
  default: boolean
}

export interface OutputChoices {
  devices: OutputDevice[]
  /** The chosen device; null follows the Windows default. */
  device: string | null
  exclusive: boolean
}

export const outputChoices = () =>
  inApp()
    ? invoke<OutputChoices>('player_devices')
    : Promise.resolve<OutputChoices>({ devices: [], device: null, exclusive: true })
export const setOutput = (device: string | null, exclusive: boolean) => call('player_output', { device, exclusive })

/** "FLAC 24/96" for the badge. */
export function sourceLabel(s: Signal['source']): string {
  const khz = s.rate % 1000 ? (s.rate / 1000).toFixed(1) : String(s.rate / 1000)
  return s.lossless ? `${s.codec} ${s.bits}/${khz}` : `${s.codec} ${khz} kHz`
}

let resumedFor = ''
/** Pick up where you left off in this folder (once per folder): the saved queue, paused. */
export function resumePlayer(root: string) {
  if (!inApp() || !root || resumedFor === root || state.queue.length) return
  resumedFor = root
  queueRoot = root
  invoke('player_resume', { root }).catch(() => {})
}

export interface Verification {
  bit_perfect: boolean
  bits: number
  rate: number
  frames: number
  file_md5: string
  output_md5: string
}

/** Check that the song playing now comes out bit-perfect (FLAC: against its own fingerprint). */
export function verifyCurrent(): Promise<Verification> {
  const song = current(state)
  if (!song) return Promise.reject(new Error('Play a song first.'))
  return invoke<Verification>('player_verify', { root: queueRoot, path: song.path })
}

const waves = new Map<string, number[]>()
/** The playing song's shape for the seek bar (600 peaks, 0–255), once it's been read. */
export function useWaveform(path: string | undefined): number[] | null {
  const [, tick] = useState(0)
  // The engine says which folder the queue is in (after a restart nothing was played here yet).
  const root = usePlayerValue((s) => s.root || queueRoot)
  useEffect(() => {
    if (!path || !root || waves.has(path) || !inApp()) return
    let live = true
    invoke<number[]>('song_waveform', { root, path })
      .then((w) => {
        waves.set(path, w)
        if (live) tick((n) => n + 1)
      })
      .catch(() => {})
    return () => {
      live = false
    }
  }, [path, root])
  return path ? (waves.get(path) ?? null) : null
}

export const setSound = (sound: Sound) => {
  set({ ...state, sound })
  return call('player_sound', { sound })
}
/** Band levels for a spectrum display: asks the engine to measure while mounted. */
export function useSpectrum(): number[] | null {
  useEffect(() => {
    if (!inApp()) return
    void invoke('player_spectrum', { on: true }).catch(() => {})
    return () => {
      void invoke('player_spectrum', { on: false }).catch(() => {})
    }
  }, [])
  return usePlayerValue((s) => s.bands ?? null)
}

/** The mini player: open it (the main window steps aside) or close it and come back. */
export const miniPlayer = (open: boolean) => call('mini_player', { open })

export const setStopAfter = (on: boolean) => call('player_stop_after', { on })
/** Pause after `minutes` (null cancels). */
export const setSleep = (minutes: number | null) => call('player_sleep', { minutes })

/** Your rating for a song (0–5), written into the file as RATING (journalled, undoable). */
export async function rate(path: string, stars: number) {
  const value = stars > 0 ? [String(Math.min(5, stars) * 20)] : []
  const was = state.queue.find((q) => q.path === path)?.stars ?? 0
  const show = (n: number) => set({ ...state, queue: state.queue.map((q) => (q.path === path ? { ...q, stars: n } : q)) })
  show(stars)
  try {
    const r = await editTags(state.root || queueRoot, [{ file: path, set: [['RATING', value]] }])
    if (r.errors.length) throw new Error(r.errors[0][1])
  } catch (e) {
    show(was)
    notify(`Couldn't save the rating: ${e instanceof Error ? e.message : String(e)}`)
  }
}
