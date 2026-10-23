/*
  Listen together v2: the shape of the view the app sends the window (contract 5.9 of
  docs/design/listen-together.md), as crates/together/src/view.rs serialises it. Nothing uses it
  yet in the live app: the room panel v2 (components/together/RoomPanel.tsx) is built on it and
  shown once the app sends it (WP9). tests/fixtures/together-views.json holds every
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
  /** Missed three prepares in a row (the host's view). */
  slow: boolean
  /** Null: can't take the room over; else how many people would lose songs only the host has. */
  hand_over: number | null
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
  phase: Phase
  /** What people without the role asked (shown to whoever controls). */
  requests: RequestView[]
  /** The host hasn't chosen the role for new people (control up to 4 people, add above). */
  auto_roles: boolean
  /** Remotes may change the host's speaker's volume. */
  guests_volume: boolean
  /** The host's speaker's volume, 0 to 100 (for the host, and remotes allowed to change it). */
  volume: number | null
  /** The words of the song playing when only the host has it (sent to remotes by the host). */
  lyrics?: RoomLyrics | null
}

/** A song's words for the remote view: timed lines [ms, words], or plain words. */
export interface RoomLyrics {
  key: string
  lines: [number, string][]
  text: string
}

/** Where the room's playback is. */
export type Phase = 'resting' | 'queue_ended' | 'playing' | 'held'

/** Something asked of whoever controls ("Ravi asks to skip"). */
export interface RequestView {
  id: number
  from: number
  words: string
}

export interface You {
  mode: ListenMode
  sub_state: SubState
  line: string
  /** A press on its way to the room ("Pausing...", "Moving to 1:35..."), until it is committed. */
  pressing: string | null
  joining_pct: number | null
  /** The modes you may take now, in the order offered (one speaker first on the host's network). */
  choices: ModeChoice[]
  /** Why you stepped out, while you are. */
  stepped_out: StepOut | null
}

export interface ModeChoice {
  mode: ListenMode
  /** Why it can't be had now, in words; null: it can. */
  why_not: string | null
}

export type StepOut = 'chosen' | 'sleep_timer' | 'unplugged'

/** A room open on this network. */
export interface NearbyView {
  id: number
  host: string
  listening: number
}

/** A room to go back to after a crash or restart. */
export type Resume = { kind: 'rejoin'; host: string } | { kind: 'reopen' }

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
  /** The host's time when this view was made, from your clock line (null before it is ready). */
  host_now_us: number | null
  nearby: NearbyView[]
  /** The host's room can be seen on this network. */
  visible: boolean
  resume: Resume | null
  /** After a room: how many songs its queue had (to save it as a playlist). */
  last_queue: number | null
}
