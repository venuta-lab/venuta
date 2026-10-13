import { Channel, invoke } from '@tauri-apps/api/core'
import { useEffect, useState, useSyncExternalStore } from 'react'
import { editTags, inApp, type CatalogTrack, type Gain } from './engine'

/*
  The player, app-wide. Screens call play / toggle / next…; the engine (in the backend) owns
  the real state and streams it back here, so every screen shows the same thing.
*/

export type Repeat = 'off' | 'all' | 'one'

/** A song in the queue: what the bar and Now Playing need to show it. */
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
const listeners = new Set<() => void>()
const set = (s: PlayerState) => {
  state = s
  listeners.forEach((l) => l())
}

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
let lastSet: { from: string; paths: string[] } | null = null

/** Play these songs from `index`, replacing the queue. Shuffle stays as it is (a song picked
 *  from a list while shuffling plays, then the rest shuffled). */
export function play(root: string, tracks: CatalogTrack[], index = 0, from: From | string = '') {
  queueRoot = root
  const items = tracks.map(toItem)
  const f = fromText(from)
  lastSet = { from: f, paths: tracks.map((t) => t.path) }
  // Show it at once; the engine confirms within a moment.
  set({ ...state, queue: items, index, playing: true, position_ms: 0, from: f, error: '' })
  return call('player_play', { root, items, index, from: f })
}

/** The Play and Shuffle buttons: all of it in order (shuffle off), or shuffled from a random
 *  song (shuffle on, so the bar shows it and turning it off goes back to the order). */
export async function playAll(root: string, tracks: CatalogTrack[], from: From | string, how: 'order' | 'shuffle' = 'order') {
  if (!tracks.length) return
  const shuffle = how === 'shuffle'
  if (state.shuffle !== shuffle) {
    set({ ...state, shuffle })
    await call('player_options', { shuffle, repeat: state.repeat })
  }
  return play(root, tracks, shuffle ? Math.floor(Math.random() * tracks.length) : 0, from)
}

/**
 * A source changed while it plays (a playlist edited): the song playing plays on, and what
 * comes after follows the new list. Only when the queue is still the one the source made
 * (nothing added or taken out by hand); otherwise the queue is yours and is left alone.
 */
export function follow(root: string, tracks: CatalogTrack[], from: From) {
  const f = fromText(from)
  if (!lastSet || lastSet.from !== f || !fromIs(state.from, from.kind, from.ref)) return
  const cur = state.index >= 0 ? state.queue[state.index] : undefined
  if (!cur) return
  const queued = state.queue.map((q) => q.path).sort()
  const made = [...lastSet.paths].sort()
  if (queued.length !== made.length || queued.some((p, i) => p !== made[i])) return
  const paths = tracks.map((t) => t.path)
  if (paths.length === lastSet.paths.length && paths.every((p, i) => p === lastSet!.paths[i])) return
  const items = tracks.map(toItem)
  let index = paths.indexOf(cur.path)
  if (index < 0) {
    // The playing song left the list: it plays on, where it was among the songs still there.
    const was = lastSet.paths.indexOf(cur.path)
    const before = new Set(lastSet.paths.slice(0, Math.max(0, was)))
    index = paths.filter((p) => before.has(p)).length
    items.splice(index, 0, cur)
  }
  lastSet = { from: f, paths: items.map((i) => i.path) }
  return call('player_follow', { root, items, index })
}
export const toggle = () => call('player_toggle')
export const next = () => call('player_next')
export const previous = () => call('player_previous')
export const seek = (ms: number) => {
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
export const enqueue = (root: string, tracks: CatalogTrack[], nextUp: boolean) =>
  call('player_enqueue', { root, items: tracks.map(toItem), next: nextUp })
export const removeAt = (i: number) => call('player_remove', { index: i })
export const move = (from: number, to: number) => call('player_move', { from, to })
export const jumpTo = (i: number) => call('player_jump', { index: i })

export const getPlayer = () => state
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
  set({ ...state, queue: state.queue.map((q) => (q.path === path ? { ...q, stars } : q)) })
  await editTags(queueRoot, [{ file: path, set: [['RATING', value]] }])
}
