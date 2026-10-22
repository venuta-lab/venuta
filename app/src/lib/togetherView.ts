/*
  Listen together v2: the shape of the view the app sends the window (contract 5.9 of
  docs/design/listen-together.md), as crates/together/src/view.rs serialises it. Nothing uses it
  yet; the room panel moves to it with protocol 3. tests/fixtures/together-views.json holds every
  state the panel draws, made from the Rust types, so the window is built and tested against it.
*/

import type { Activity, Role, RoomSong } from './together'

export type SessionState = 'off' | 'starting' | 'joining' | 'hosting' | 'waiting' | 'in' | 'reconnecting' | 'ended'

/** How a person listens: a remote on the host's speaker, every device heard together, or apart. */
export type ListenMode = 'one_speaker' | 'every_device' | 'apart'

export type Suspension = 'buffering' | 'output_lost' | 'interrupted' | 'sleeping'

/** A person's player in the room. */
export type SubState =
  | { state: 'opening' | 'ready' | 'in_step' | 'correcting' | 'realigning' | 'stepped_out' | 'cannot_play' | 'remote' }
  | { state: 'suspended'; why: Suspension }

export type Transition = { kind: 'gapless' } | { kind: 'crossfade'; ms: number } | { kind: 'cut' }

export type CopyKind = 'own_file' | 'catalog' | 'host_stream' | 'close_match'
export type CopyClass = 'exact' | 'measured' | 'approximate'

export interface CopyView {
  kind: CopyKind
  class: CopyClass
  /** Another version: how much longer (positive) or shorter, in seconds. */
  length_diff_s: number | null
}

export interface Anchor {
  entry: number
  content_us: number
  host_us: number
  playing: boolean
  seq: number
}

export interface Next {
  entry: number
  boundary_host_us: number
  transition: Transition
  seq: number
}

export interface MemberView {
  id: number
  name: string
  host: boolean
  role: Role
  mode: ListenMode
  /** Null while away. */
  sub_state: SubState | null
  away: boolean
  /** One short line in plain words ("In step, about ±15 ms"), made by the app. */
  line: string
  copy: CopyView | null
  error_ms: number | null
  bound_ms: number | null
}

export interface EntryView {
  id: number
  song: RoomSong
}

export interface RoomViewV2 {
  host_name: string
  members: MemberView[]
  waiting: [number, string][]
  queue: EntryView[]
  anchor: Anchor | null
  next: Next | null
  transition: Transition
  activity: Activity[]
  new_role: Role
  /** A start getting ready: who it still waits for (empty: about to go). */
  starting: string[] | null
  /** The host is reconnecting: room controls are greyed. */
  host_away: boolean
}

export interface You {
  mode: ListenMode
  sub_state: SubState
  line: string
  /** A press on its way to the room ("Pausing...", "Moving to 1:35..."), until it is committed. */
  pressing: string | null
  joining_pct: number | null
}

export interface OutputView {
  name: string
  delay_ms: number
  bluetooth: boolean
  delay_known: boolean
}

export interface TogetherViewV2 {
  state: SessionState
  host: boolean
  me: number | null
  room: RoomViewV2 | null
  invite: { link: string; code: string; qr_svg: string; host: string; until: number } | null
  path: { direct: boolean; relay: boolean; rtt_ms: number | null } | null
  message: string | null
  name: string
  local_only: boolean
  let_in: boolean
  you: You | null
  output: OutputView
}
