/*
  The room panel v2's logic, pure (no React, no app calls), so every decision the panel makes is
  tested against the fixture of every state (tests/roomPanel.test.ts): which screen it opens on,
  the status strip, the chip's colour, who may do what, where a press goes, the join choice, the
  output notes, the people's rows, a remote's progress, and today's view projected from v2 for
  the rest of the window. Lines about a person's state come from the app (Rust's view::line);
  this only places them. Design: docs/design/listen-together.md, WP12 notes.
*/
import type { Hearing, Member, Role, RoomView, TogetherView } from '../../lib/together'
import type {
  CopyView,
  EntryView,
  ListenMode,
  MemberView,
  RequestView,
  RoomViewV2,
  TogetherViewV2,
  Transition,
} from '../../lib/togetherView'

/** What the room panel v2 can ask for (lib/together.ts's `liveActions` in the app; the preview's
 *  are no-ops). */
export interface RoomActions {
  start: () => void
  join: (code: string) => void
  readInvite: (code: string) => Promise<InvitePreview>
  joinNearby: (id: number) => void
  resume: (yes: boolean) => void
  saveQueue: () => void
  setName: (name: string) => void
  /** Stop joining, or stop waiting at the door. */
  cancel: () => void
  leave: () => void
  end: () => void
  handOver: (id: number) => void
  toggle: () => void
  next: () => void
  previous: () => void
  seek: (ms: number) => void
  answer: (request: number, yes: boolean) => void
  admit: (id: number) => void
  deny: (id: number) => void
  remove: (id: number, block: boolean) => void
  setRole: (id: number, role: Role) => void
  setNewRole: (role: Role | 'auto') => void
  setTransition: (t: Transition) => void
  setGuestsVolume: (on: boolean) => void
  setLetIn: (on: boolean) => void
  setVisible: (on: boolean) => void
  setVolume: (percent: number) => void
  setMode: (mode: ListenMode) => void
  startWithNext: () => void
  stepOut: (out: boolean) => void
  setDelay: (ms: number) => void
  lineUp: (on: boolean) => void
  takeHostCopy: () => void
  lighter: (id: number) => void
  nextSongFor: (id: number) => void
  copyInvite: () => void
  undo: () => void
}

/** Today's view or v2: only v2 carries `output`. */
export const isV2 = (v: unknown): v is TogetherViewV2 => typeof v === 'object' && v !== null && 'output' in v

export type Screen = 'start' | 'door' | 'joining' | 'room'

export function screenOf(v: TogetherViewV2): Screen {
  if (v.state === 'waiting') return 'door'
  if (v.state === 'joining') return 'joining'
  if ((v.state === 'hosting' || v.state === 'in' || v.state === 'reconnecting') && v.room) return 'room'
  return 'start'
}

export const meOf = (v: TogetherViewV2): MemberView | null => v.room?.members.find((m) => m.id === v.me) ?? null

/** The host's name, wherever the view has it. */
export function hostName(v: TogetherViewV2): string | null {
  return v.room?.host_name || v.invite?.host || (v.resume?.kind === 'rejoin' ? v.resume.host : null) || null
}

const theHost = (v: TogetherViewV2) => hostName(v) ?? 'the host'
const possessive = (name: string) => `${name}'s`

/** "Ali", "Ali and Dev", "Ali, Dev and Ira", "Ali, Dev and 2 more". */
export function names(list: string[]): string {
  if (list.length <= 1) return list[0] ?? ''
  if (list.length === 2) return `${list[0]} and ${list[1]}`
  if (list.length === 3) return `${list[0]}, ${list[1]} and ${list[2]}`
  return `${list[0]}, ${list[1]} and ${list.length - 2} more`
}

// --- Who may do what -------------------------------------------------------------------------

export interface May {
  /** Play, pause, skip, seek, jump, and change the queue. */
  control: boolean
  add: boolean
  /** Accept or decline requests. */
  answer: boolean
  /** The host's speaker's volume. */
  volume: boolean
  /** The room's settings, roles, the door, removing, ending (the host's). */
  settings: boolean
  /** Changing how you listen. */
  modes: boolean
}

const NOTHING: May = { control: false, add: false, answer: false, volume: false, settings: false, modes: false }

export function may(v: TogetherViewV2): May {
  const room = v.room
  if (!room || screenOf(v) !== 'room') return NOTHING
  if (v.host) return { control: true, add: true, answer: true, volume: true, settings: true, modes: true }
  // While the host is reconnecting, nothing that goes through the host is offered.
  if (room.host_away) return NOTHING
  const me = meOf(v)
  const role: Role = me?.role ?? 'listen'
  const control = role === 'control'
  return {
    control,
    add: control || role === 'add',
    answer: control,
    volume: room.guests_volume && me?.mode === 'one_speaker',
    settings: false,
    modes: true,
  }
}

export type Press = 'play' | 'pause' | 'next' | 'previous' | 'seek' | 'jump' | 'remove' | 'move' | 'add'
export type PressRoute = { send: 'ask' } | { send: 'request'; said: string } | { send: 'refuse'; said: string }

const clockOf = (ms: number) => {
  const s = Math.floor(ms / 1000)
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
}

/**
 * Where a press goes: to the room when your role allows it (and while you reconnect: the
 * session holds it), else a request to whoever controls (B13). Changing the queue's order and
 * adding are never requests: they are explained.
 */
export function routePress(v: TogetherViewV2, press: Press, seekMs = 0): PressRoute {
  const host = theHost(v)
  const role: Role | null = v.host ? 'control' : (meOf(v)?.role ?? null)
  if (role === 'control') return { send: 'ask' }
  if (press === 'add') return role === 'add' ? { send: 'ask' } : { send: 'refuse', said: `${host} hasn't let you add songs.` }
  if (press === 'remove' || press === 'move') return { send: 'refuse', said: `${host} is in control of the queue.` }
  const what = {
    play: 'play',
    pause: 'pause',
    next: 'skip',
    previous: 'go back',
    seek: `move to ${clockOf(seekMs)}`,
    jump: 'play that song',
  }[press]
  return { send: 'request', said: `Asked the room to ${what}` }
}

export const requestsShown = (v: TogetherViewV2): RequestView[] => (may(v).answer ? (v.room?.requests ?? []) : [])

// --- The chip and the status strip ------------------------------------------------------------

export type Tone = 'accent' | 'warning' | 'faded'

/** The title bar chip's ring (4.15 "For you"). */
export function chipTone(v: TogetherViewV2): Tone {
  if (v.state === 'reconnecting' || v.room?.host_away) return 'faded'
  if (screenOf(v) !== 'room' || !v.you) return 'accent'
  const s = v.you.sub_state.state
  if (s === 'stepped_out' || s === 'cannot_play') return 'faded'
  if (s === 'correcting' || s === 'realigning' || s === 'suspended' || s === 'opening') return 'warning'
  return 'accent'
}

export interface Status {
  text: string
  kind: 'away' | 'press' | 'phase'
}

/** The room's status strip, top to bottom: only the lines that hold. */
export function statusLines(v: TogetherViewV2): Status[] {
  const room = v.room
  if (!room || screenOf(v) !== 'room') return []
  const out: Status[] = []
  if (v.state === 'reconnecting') out.push({ text: 'Reconnecting... the music goes on', kind: 'away' })
  if (room.host_away) out.push({ text: `${room.host_name} is reconnecting. The music goes on.`, kind: 'away' })
  const pressing = v.you?.pressing ?? null
  if (room.starting != null) {
    const head = pressing ?? 'Starting...'
    out.push({ text: room.starting.length ? `${head} waiting for ${names(room.starting)}` : head, kind: 'press' })
  } else if (pressing) {
    out.push({ text: pressing, kind: 'press' })
  } else if (room.phase === 'held') {
    out.push({ text: 'Paused', kind: 'phase' })
  } else if (room.phase === 'resting') {
    out.push({ text: may(v).add ? 'Add songs to start' : 'Nothing is playing yet', kind: 'phase' })
  } else if (room.phase === 'queue_ended') {
    out.push({ text: "The room's queue has ended", kind: 'phase' })
  }
  return out
}

// --- Modes -------------------------------------------------------------------------------------

/** A person's mode in a word or two. */
export function modeWord(mode: ListenMode, host: string): string {
  return mode === 'one_speaker' ? `${possessive(host)} speaker` : mode === 'every_device' ? 'Every device' : 'Apart'
}

export interface JoinChoice {
  speaker: string
  speakerSays: string
  here: string
  hereSays: string
  hereMode: ListenMode
  hereWhyNot: string | null
  selected: 'speaker' | 'here'
}

/** While joining on the host's network: one speaker first, this device beside it (A16). */
export function joinChoice(v: TogetherViewV2): JoinChoice | null {
  const y = v.you
  if (v.state !== 'joining' || !y || y.choices[0]?.mode !== 'one_speaker') return null
  const here = y.choices.find((c) => c.mode !== 'one_speaker')
  if (!here) return null
  const host = theHost(v)
  return {
    speaker: `Play on ${possessive(host)} speaker`,
    speakerSays: `Only ${possessive(host)} speaker plays. You see the song and the queue.`,
    here: 'Play on this device too',
    hereSays: here.mode === 'every_device' ? `Your speaker plays in step with ${possessive(host)}.` : 'You hear it on this device, with the room.',
    hereMode: here.mode,
    hereWhyNot: here.why_not,
    selected: y.mode === 'one_speaker' ? 'speaker' : 'here',
  }
}

export interface ModeSwitch {
  label: string
  mode: ListenMode
  why_not: string | null
}

/**
 * In the room, the other ways you may listen, as buttons: a remote's "Play here too" (the
 * first way of playing offered), a player's "Listen on Asha's speaker", and between every
 * device and apart. Greyed with why when it can't be had now.
 */
export function modeSwitches(v: TogetherViewV2): ModeSwitch[] {
  const y = v.you
  if (!y || screenOf(v) !== 'room') return []
  const host = theHost(v)
  const away = v.room?.host_away ? `${host} is reconnecting` : null
  const others = y.choices.filter((c) => c.mode !== y.mode)
  let out: ModeSwitch[]
  if (y.mode === 'one_speaker') {
    const play = others.find((c) => c.mode !== 'one_speaker' && !c.why_not) ?? others.find((c) => c.mode !== 'one_speaker')
    out = play ? [{ label: 'Play here too', mode: play.mode, why_not: play.why_not }] : []
  } else {
    out = others.map((c) => ({
      label: c.mode === 'one_speaker' ? `Listen on ${possessive(host)} speaker` : c.mode === 'every_device' ? "Play in step with the room's speakers" : 'Play apart',
      mode: c.mode,
      why_not: c.why_not,
    }))
  }
  return away ? out.map((s) => ({ ...s, why_not: away })) : out
}

// --- Your output -------------------------------------------------------------------------------

export const delayWords = (ms: number) => (ms === 0 ? 'none' : `${ms > 0 ? '+' : '-'}${Math.abs(ms)} ms`)

/** The typical Bluetooth delay (SBC, measured by RTINGS), offered, never applied by itself. */
export const BLUETOOTH_GUESS_MS = 170

export interface OutputPanel {
  show: boolean
  name: string
  bluetooth: boolean
  delay: string
  note: { text: string; warn: boolean } | null
  suggest: { ms: number; says: string } | null
  /** "Line up by ear" (every device, a guest: the host's device and this one click in turn). */
  lineUp: boolean
}

export function outputPanel(v: TogetherViewV2): OutputPanel {
  const o = v.output
  const mode = v.you?.mode
  const show = screenOf(v) === 'room' && !!v.you && mode !== 'one_speaker'
  let note: OutputPanel['note'] = null
  if (mode === 'every_device' && !o.delay_known)
    note = { text: o.bluetooth ? 'Bluetooth delay not measured: you may hear an echo' : "This output's delay isn't measured: you may hear an echo", warn: true }
  else if (mode === 'apart' && o.bluetooth && o.delay_ms > 0) note = { text: `Bluetooth: about ${o.delay_ms} ms behind`, warn: false }
  return {
    show,
    name: o.name,
    bluetooth: o.bluetooth,
    delay: delayWords(o.delay_ms),
    note,
    suggest: o.bluetooth && !o.delay_known && o.delay_ms === 0 ? { ms: BLUETOOTH_GUESS_MS, says: `Bluetooth is usually about ${BLUETOOTH_GUESS_MS} ms late` } : null,
    lineUp: show && !v.host && mode === 'every_device',
  }
}

// --- The people ------------------------------------------------------------------------------

/**
 * A copy in words, as Rust's `CopyView::words` says it to the host ("your file, streamed");
 * to a guest, the host's file is the host's (`host`: the host's name, for a guest's view).
 */
export function copyWords(c: CopyView, host: string | null = null): string {
  const kind = { own_file: 'own file', catalog: 'YouTube Music', host_stream: host ? `${possessive(host)} file, streamed` : 'your file, streamed', close_match: 'a close match' }[c.kind]
  const d = c.length_diff_s
  const how = c.class !== 'approximate' ? 'matched' : d ? `another version (${d > 0 ? '+' : ''}${d} s)` : 'another version'
  return `${kind}, ${how}`
}

const msWords = (x: number) => (Math.round(x) < 1 ? 'under 1 ms' : `${Math.round(x)} ms`)

/** The numbers behind a line: "Off by 4 ms, the clock within 9 ms". */
export function numbersWords(m: MemberView): string | null {
  if (m.error_ms == null || m.bound_ms == null || m.away) return null
  return `Off by ${msWords(Math.abs(m.error_ms))}, the clock within ${Math.max(1, Math.ceil(m.bound_ms))} ms`
}

export function pathWords(p: TogetherViewV2['path']): string | null {
  if (!p) return null
  const how = p.direct ? 'Direct' : p.relay ? 'Through a relay' : 'Connected'
  return p.rtt_ms != null ? `${how}, ${p.rtt_ms} ms round trip` : how
}

export interface Row {
  m: MemberView
  you: boolean
  mode: string
  line: string
  /** The copy they hear (the host sees everyone's; a guest only their own, D14). */
  copy: string | null
  numbers: string | null
  /** Your connection to the host (a guest's own row). */
  path: string | null
  /** The host's note on a guest who missed three starts. */
  slow: string | null
  /** The host's role menu and Remove. */
  manage: boolean
}

export function rows(v: TogetherViewV2): Row[] {
  const room = v.room
  if (!room) return []
  return room.members.map((m) => {
    const you = m.id === v.me
    return {
      m,
      you,
      mode: modeWord(m.mode, room.host_name),
      line: you && v.you ? v.you.line : m.line,
      copy: (v.host || you) && m.copy ? copyWords(m.copy, v.host ? null : room.host_name) : null,
      numbers: numbersWords(m),
      path: you && !v.host ? pathWords(v.path) : null,
      slow: v.host && m.slow ? `${possessive(m.name)} connection is slow` : null,
      manage: v.host && !m.host,
    }
  })
}

/** On another version over 2 s longer or shorter: one tap to the host's copy (E10). */
export function versionNote(v: TogetherViewV2): string | null {
  if (v.host) return null
  const c = meOf(v)?.copy
  const d = c?.length_diff_s
  if (!c || c.class !== 'approximate' || d == null || Math.abs(d) <= 2) return null
  return `Different version (${d > 0 ? '+' : ''}${d} s): use ${possessive(theHost(v))} copy`
}

/** Step out, or back in ("Rejoin" when your headphones went). A remote plays nothing. */
export function stepButton(v: TogetherViewV2): { label: string; out: boolean } | null {
  const y = v.you
  if (!y || screenOf(v) !== 'room' || y.mode === 'one_speaker') return null
  if (y.sub_state.state !== 'stepped_out') return { label: 'Step out', out: false }
  return { label: y.stepped_out === 'unplugged' ? 'Rejoin' : 'Catch up', out: true }
}

// --- A remote's view ------------------------------------------------------------------------

export interface Progress {
  entry: EntryView
  ms: number
  length_ms: number | null
  playing: boolean
}

/**
 * Where the room is in its song, from the anchor at the host's time when the view was made,
 * moved on by `sinceMs` (as `Anchor::content_at`: a playing anchor set ahead waits at its
 * place). Null without an anchor or a clock line.
 */
export function progress(v: TogetherViewV2, sinceMs: number): Progress | null {
  const room = v.room
  const a = room?.anchor
  if (!room || !a || v.host_now_us == null) return null
  const entry = room.queue.find((e) => e.id === a.entry)
  if (!entry) return null
  const t = v.host_now_us + sinceMs * 1000
  const content = a.playing && t > a.host_us ? a.content_us + (t - a.host_us) : a.content_us
  const length = entry.song.duration_ms
  const ms = Math.max(0, Math.round(content / 1000))
  return { entry, ms: length != null ? Math.min(ms, length) : ms, length_ms: length, playing: a.playing && room.phase === 'playing' }
}

// --- Leaving, ending, handing over --------------------------------------------------------------

export const END_ASK = "End the room for everyone? Everyone's own music comes back, paused."

export interface HandOverChoice {
  id: number
  name: string
  warn: string | null
}

export function handOverChoices(v: TogetherViewV2): HandOverChoice[] {
  if (!v.host || !v.room) return []
  return v.room.members
    .filter((m) => !m.host && m.hand_over != null)
    .map((m) => {
      const n = m.hand_over ?? 0
      return { id: m.id, name: m.name, warn: n > 0 ? `${n} ${n === 1 ? 'person' : 'people'} will lose songs only you have` : null }
    })
}

// --- The host's settings ------------------------------------------------------------------------

export function transitionWords(t: Transition): string {
  if (t.kind === 'gapless') return 'Gapless'
  if (t.kind === 'cut') return 'Cut'
  return `Crossfade ${Number((t.ms / 1000).toFixed(1))} s`
}

export const transitionKey = (t: Transition) => (t.kind === 'crossfade' ? `crossfade:${t.ms}` : t.kind)

/** Gapless, Crossfade 4 s, Cut, and the room's own value when it is another. */
export function transitionOptions(current: Transition): { key: string; label: string; t: Transition }[] {
  const list: Transition[] = [{ kind: 'gapless' }, { kind: 'crossfade', ms: 4000 }, { kind: 'cut' }]
  if (!list.some((t) => transitionKey(t) === transitionKey(current))) list.splice(2, 0, current)
  return list.map((t) => ({ key: transitionKey(t), label: transitionWords(t), t }))
}

/** What "New people can" shows: Automatic while the host hasn't chosen (D20). */
export const newRoleValue = (room: RoomViewV2): Role | 'auto' => (room.auto_roles ? 'auto' : room.new_role)

// --- Out of a room, at the door, joining -------------------------------------------------------------

export interface InvitePreview {
  host: string
  open: boolean
  until: number
  listening?: number
  now_playing?: string
}

export function inviteWords(p: InvitePreview | null): string {
  if (!p) return "That isn't a Venuta invite."
  if (!p.open) return `${possessive(p.host)} invite has ended. Ask for a new one.`
  let s = `${possessive(p.host)} room`
  if (p.listening) s += `, ${p.listening} listening`
  if (p.now_playing) s += `, now playing ${p.now_playing}`
  return s
}

export interface StartWords {
  opening: boolean
  message: string | null
  saveQueue: string | null
  resume: { text: string; button: string } | null
  nearby: { id: number; text: string; sub: string }[]
}

export function startWords(v: TogetherViewV2): StartWords {
  const r = v.resume
  return {
    opening: v.state === 'starting',
    message: v.message,
    saveQueue: v.state === 'ended' && v.last_queue ? "Save the room's queue as a playlist" : null,
    resume: !r
      ? null
      : r.kind === 'rejoin'
        ? { text: `Rejoin ${possessive(r.host)} room?`, button: 'Rejoin' }
        : { text: 'Reopen your room? Friends still on the invite can come back.', button: 'Reopen' },
    nearby: v.nearby.map((n) => ({ id: n.id, text: `${possessive(n.host)} room`, sub: `${n.listening} listening` })),
  }
}

export const doorWords = (v: TogetherViewV2) => `Waiting for ${theHost(v)} to let you in...`

export function joiningWords(v: TogetherViewV2): { title: string; line: string | null; pct: number | null } {
  const host = hostName(v)
  return { title: host ? `Joining ${possessive(host)} room` : 'Joining the room', line: v.you?.line ?? null, pct: v.you?.joining_pct ?? null }
}

// --- Today's view, for the rest of the window ------------------------------------------------------

function hearingOf(m: MemberView): Hearing {
  const s = m.sub_state?.state
  if (s === 'opening' || s === undefined) return 'opening'
  if (s === 'stepped_out') return 'stepped_out'
  if (s === 'cannot_play') return 'cannot'
  if (s === 'remote') return 'from_host'
  const k = m.copy?.kind
  return k === 'own_file' ? 'local' : k === 'host_stream' ? 'from_host' : k ? 'online' : 'opening'
}

/**
 * A v2 view as today's `TogetherView`, for what still reads it (the queue drawer, Now Playing,
 * radio's top-up, the availability badge) until WP13 moves them to v2.
 */
export function toV1(v: TogetherViewV2): TogetherView {
  const r = v.room
  let room: RoomView | null = null
  if (r) {
    const index = r.anchor ? r.queue.findIndex((e) => e.id === r.anchor!.entry) : -1
    const members: Member[] = r.members.map((m) => ({
      id: m.id,
      name: m.name,
      host: m.host,
      role: m.role,
      hearing: hearingOf(m),
      error_ms: m.error_ms == null ? null : Math.round(m.error_ms),
      away: m.away,
    }))
    room = {
      host_name: r.host_name,
      members,
      waiting: r.waiting,
      queue: r.queue.map((e) => e.song),
      rev: r.anchor?.seq ?? 0,
      timeline:
        r.anchor && index >= 0
          ? { index, rev: r.anchor.seq, position_ms: Math.round(r.anchor.content_us / 1000), at_us: r.anchor.host_us, playing: r.anchor.playing }
          : null,
      activity: r.activity,
      new_role: r.new_role,
      same_room: v.you?.mode === 'every_device',
      starting: r.starting,
    }
  }
  return {
    state: v.state,
    host: v.host,
    me: v.me,
    room,
    invite: v.invite,
    path: v.path,
    message: v.message,
    name: v.name,
    delay_ms: v.output.delay_ms,
    local_only: v.local_only,
    let_in: v.let_in,
  }
}

/**
 * The words the host sent for a remote's song playing (a song only the host has, WP18), as
 * the lyrics display takes them; null when there are none for it (a catalog song's words come
 * from the lyrics services instead).
 */
export function hostWords(v: TogetherViewV2): { lines: [number, string][]; text: string } | null {
  const sent = v.room?.lyrics
  const song = v.you?.mode === 'one_speaker' ? progress(v, 0)?.entry.song : undefined
  return sent && song && sent.key === song.key ? { lines: sent.lines, text: sent.text } : null
}
