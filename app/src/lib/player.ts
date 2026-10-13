import { Channel, invoke } from '@tauri-apps/api/core'
import { useSyncExternalStore } from 'react'
import { inApp, type CatalogTrack } from './engine'

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
}

/** Where the sound goes and what happens on the way (for the badge and the signal path). */
export interface Output {
  device: string
  /** "exclusive": straight to the device, at the file's own format. "shared": via Windows mixing. */
  mode: 'exclusive' | 'shared' | 'none'
  sample_rate: number
  bit_depth: number
  /** Nothing changes the samples on the way. */
  bit_perfect: boolean
  /** What does change them, in words ("Resampled to 48 kHz", "Volume levelled"). */
  steps: string[]
}

export interface PlayerState {
  queue: QueueItem[]
  index: number
  playing: boolean
  position_ms: number
  shuffle: boolean
  repeat: Repeat
  /** 0–1. */
  volume: number
  output: Output | null
  /** "Taal", "Songs", "Search: rahman" — where the queue came from. */
  from: string
  /** The last problem, in words (a file that couldn't be opened…). */
  error: string
}

const idle: PlayerState = {
  queue: [],
  index: -1,
  playing: false,
  position_ms: 0,
  shuffle: false,
  repeat: 'off',
  volume: 1,
  output: null,
  from: '',
  error: '',
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
  const channel = new Channel<PlayerState>()
  channel.onmessage = set
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
})

const call = (cmd: string, args: Record<string, unknown> = {}) =>
  inApp() ? invoke(cmd, args).catch((e) => set({ ...state, error: String(e) })) : Promise.resolve()

/** Play these songs from `index`, replacing the queue. */
export function play(root: string, tracks: CatalogTrack[], index = 0, from = '') {
  const items = tracks.map(toItem)
  // Show it at once; the engine confirms within a moment.
  set({ ...state, queue: items, index, playing: true, position_ms: 0, from, error: '' })
  return call('player_play', { root, items, index, from })
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
