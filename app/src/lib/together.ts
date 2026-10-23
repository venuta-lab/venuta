import { invoke } from '@tauri-apps/api/core'
import { useEffect, useSyncExternalStore } from 'react'
import { isV2, routePress, toV1, type InvitePreview, type Press, type RoomActions } from '../components/together/model'
import { inApp, libraryHome } from './engine'
import { loadListening } from './listening'
import { notify } from './notify'
import { clock, getPlayerState, type QueueItem } from './player'
import { setRoomRoute } from './roomRoute'
import type { TogetherViewV2 } from './togetherView'

/*
  Listening together (app together/): host a room from this Venuta or join one, and everyone
  hears the same song at the same moment, each from their best copy. The app sends its view as
  the "together" event after every change; here it's kept for the screens, and while you're in a
  room the player's buttons go to the room (lib/roomRoute.ts).

  Two shapes arrive until protocol 3 is the only one (design, WP12 notes): today's view, and
  View v2 (lib/togetherView.ts), told apart by its `output`. A v2 view is kept for the room
  panel v2 and projected to today's shape for everything else that reads the room; presses then
  go out as protocol 3's actions (entry ids, content_us).
*/

/** The one place the feature's name is written, so it can be renamed in one line. */
export const TOGETHER = 'Listen together'

export type Role = 'control' | 'add' | 'listen'
export type Hearing = 'opening' | 'local' | 'online' | 'from_host' | 'cannot' | 'stepped_out'

export interface RoomSong {
  key: string
  title: string
  artists: string[]
  album: string | null
  duration_ms: number | null
  online_id: string | null
  host_file: boolean
  artwork_url: string | null
  added_by: number | null
}

export interface Member {
  id: number
  name: string
  host: boolean
  role: Role
  hearing: Hearing
  error_ms: number | null
  away: boolean
}

export interface Activity {
  who: number
  /** Their name then (still right after they leave). */
  name: string
  what: string
  at_us: number
  undoable: boolean
}

export interface Timeline {
  index: number
  rev: number
  position_ms: number
  at_us: number
  playing: boolean
}

export interface RoomView {
  host_name: string
  members: Member[]
  waiting: [number, string][]
  queue: RoomSong[]
  rev: number
  timeline: Timeline | null
  activity: Activity[]
  new_role: Role
  same_room: boolean
  /** A song getting ready to start: who it still waits for (empty: about to go). */
  starting?: string[] | null
}

export interface TogetherView {
  state: 'off' | 'starting' | 'joining' | 'hosting' | 'waiting' | 'in' | 'reconnecting' | 'ended'
  host: boolean
  me: number | null
  room: RoomView | null
  invite: { link: string; code: string; qr_svg: string; host: string; until: number } | null
  path: { direct: boolean; relay: boolean; rtt_ms: number | null } | null
  message: string | null
  name: string
  delay_ms: number
  local_only: boolean
  /** The host lets people with the link in without asking. */
  let_in: boolean
}

const OFF: TogetherView = { state: 'off', host: false, me: null, room: null, invite: null, path: null, message: null, name: '', delay_ms: 0, local_only: false, let_in: false }

let view: TogetherView = OFF
/** The v2 view, when the app sends one, and when it came (for a remote's progress). */
let v2: TogetherViewV2 | null = null
let v2At = 0
let version = 0
let started = false
const listeners = new Set<() => void>()
let root = ''
/** An invite link Venuta was opened with (a chat, the browser), until the panel takes it. */
let invite: string | null = null

function changed() {
  version++
  listeners.forEach((l) => l())
}

function offerInvite(link: string | null) {
  if (!link) return
  if (inRoom(view)) return notify('Leave this room first to join another.')
  invite = link
  changed()
}

/** The invite link waiting to be joined (shown in the panel), if any. */
export const pendingInvite = () => invite
export function dropInvite() {
  if (invite === null) return
  invite = null
  changed()
}

function publish(sent: TogetherView | TogetherViewV2) {
  const was = view.state
  v2 = isV2(sent) ? sent : null
  v2At = performance.now()
  const v = isV2(sent) ? toV1(sent) : sent
  view = v
  changed()
  setRoomRoute(inRoom(v) ? route : null)
  // A room that ended (removed, the host ended it, lost): why. (The app gives your own queue
  // back, paused, once the room lets go of the player.)
  if (v.state === 'ended' && was !== 'ended' && v.message) notify(v.message)
}

const inRoom = (v: TogetherView) => v.state === 'hosting' || v.state === 'in' || v.state === 'reconnecting'

/** Listen for the app's view (once). */
export function startTogether() {
  if (started || !inApp()) return
  started = true
  void invoke<TogetherView | TogetherViewV2>('together_view').then(publish).catch(() => {})
  void invoke<string | null>('together_take_invite').then(offerInvite).catch(() => {})
  void import('@tauri-apps/api/event').then(({ listen }) => {
    void listen<TogetherView | TogetherViewV2>('together', (e) => publish(e.payload))
    // Why a button pressed elsewhere (the tray, media keys, the mini player) did nothing.
    void listen<string>('together-note', (e) => notify(e.payload))
    void listen<string>('together-invite', (e) => offerInvite(e.payload))
  })
}

export const togetherView = () => view
/** The v2 view, while the app sends one. */
export const togetherViewV2 = () => v2
/** How long ago the view came, in ms (a remote's progress moves on by it). */
export const viewAge = () => performance.now() - v2At

function useVersion() {
  useEffect(() => startTogether(), [])
  useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => {
        listeners.delete(l)
      }
    },
    () => version,
  )
}

/** The room's view, kept current. */
export function useTogether(): TogetherView {
  useVersion()
  return view
}

/** The v2 view, kept current (null while the app sends today's). */
export function useTogetherV2(): TogetherViewV2 | null {
  useVersion()
  return v2
}

/** You, in the room. */
export const meIn = (v: TogetherView) => v.room?.members.find((m) => m.id === v.me) ?? null
export const memberName = (v: TogetherView, id: number | null | undefined) => v.room?.members.find((m) => m.id === id)?.name ?? ''
/** Whether you may do this (the host may do anything). */
export function can(v: TogetherView, need: Role): boolean {
  if (v.host) return true
  const me = meIn(v)
  if (!me) return false
  if (need === 'listen') return true
  if (need === 'add') return me.role === 'control' || me.role === 'add'
  return me.role === 'control'
}

const hostName = () => view.room?.host_name || view.invite?.host || 'The host'

function explainNo(need: Role) {
  notify(need === 'add' ? `${hostName()} hasn't let you add songs.` : `${hostName()} is in control.`)
  return Promise.resolve()
}

const ask = (action: Record<string, unknown>) => invoke('together_ask', { action }).catch((e) => notify(String(e)))

/**
 * A press in a v2 room: to the room when your role allows it, else a request to whoever
 * controls, or said why not (components/together/model.ts's `routePress`).
 */
function pressV2(room: TogetherViewV2, press: Press, action: Record<string, unknown>, seekMs = 0) {
  const r = routePress(room, press, seekMs)
  if (r.send === 'ask') return ask(action)
  notify(r.said)
  if (r.send === 'request') return invoke('together_request', { action }).catch((e) => notify(String(e)))
  return Promise.resolve()
}

/** The entry id at a place in the v2 room's queue (the queue drawer counts by place). */
const entryAt = (room: TogetherViewV2, index: number) => room.room?.queue[index]?.id

/** The player's buttons, in a room. */
const route = {
  toggle: () => {
    if (v2) {
      const going = v2.room?.phase === 'playing' || v2.room?.starting != null
      return pressV2(v2, going ? 'pause' : 'play', { do: going ? 'pause' : 'play' })
    }
    if (!can(view, 'control')) return explainNo('control')
    // Playing or on its way (the button shows pause for both): pause. Deciding from the room's
    // paused-while-it-starts state sent Play, which took the room back to the song before.
    const going = !!view.room?.timeline?.playing || view.room?.starting != null
    return ask({ do: going ? 'pause' : 'play' })
  },
  next: () => (v2 ? pressV2(v2, 'next', { do: 'next' }) : can(view, 'control') ? ask({ do: 'next' }) : explainNo('control')),
  previous: () => (v2 ? pressV2(v2, 'previous', { do: 'previous' }) : can(view, 'control') ? ask({ do: 'previous' }) : explainNo('control')),
  seek: (ms: number) => {
    const at = Math.max(0, Math.round(ms))
    if (v2) return pressV2(v2, 'seek', { do: 'seek', content_us: at * 1000 }, at)
    return can(view, 'control') ? ask({ do: 'seek', position_ms: at }) : explainNo('control')
  },
  play: async (items: QueueItem[], index: number) => {
    if (!can(view, 'add')) return explainNo('add')
    const picked = items.slice(index, index + 50)
    await addToRoom(picked, true)
    // Played now when you control the room; else it waits its turn.
    if (can(view, 'control') && view.room?.timeline) await ask({ do: 'next' })
  },
  enqueue: (items: QueueItem[], next: boolean) => (can(view, 'add') ? addToRoom(items, next) : explainNo('add')),
}

/** Add songs to the room (quiet: autoplay's, said in the room's activity only). */
export async function addToRoom(items: QueueItem[], next: boolean, quiet = false) {
  try {
    await invoke('together_add', { root: root || getPlayerState().root, items, next })
    if (!quiet)
      notify(items.length === 1 ? `Added “${items[0].title}” to the room.` : `Added ${items.length} songs to the room.`)
  } catch (e) {
    notify(String(e))
  }
}

/** Open a room with what's playing (it goes on, now for everyone). */
export async function startRoom(libraryRoot: string) {
  root = libraryRoot
  const s = getPlayerState()
  try {
    const v = await invoke<TogetherView>('together_host', {
      root: libraryRoot,
      items: s.queue,
      index: Math.max(0, s.index),
      positionMs: Math.round(s.position_ms),
      playing: s.playing,
    })
    publish(v)
    if (v.invite) {
      await navigator.clipboard?.writeText(v.invite.link).catch(() => {})
      notify('Your room is open. The invite link is copied: send it to friends.')
    } else if (v.message) {
      notify(v.message)
    }
  } catch (e) {
    notify(String(e))
  }
}

/** What an invite says before joining: whose room, and whether it still works. */
export const readInvite = (code: string) => invoke<{ host: string; open: boolean; until: number }>('together_read_invite', { code })

export async function joinRoom(libraryRoot: string, code: string) {
  dropInvite()
  root = libraryRoot
  try {
    const v = await invoke<TogetherView>('together_join', { root: libraryRoot, code })
    publish(v)
    if (v.message && v.state !== 'waiting' && v.state !== 'in' && v.state !== 'joining') notify(v.message)
  } catch (e) {
    notify(String(e))
  }
}

/** Leave (the host: end it for everyone). The app gives your own queue back, paused. */
export async function leaveRoom() {
  await invoke('together_leave').catch(() => {})
}

export const admit = (id: number) => invoke('together_admit', { id }).catch((e) => notify(String(e)))
export const deny = (id: number) => invoke('together_deny', { id }).catch((e) => notify(String(e)))
export const removeMember = (id: number, block: boolean) => invoke('together_remove', { id, block }).catch((e) => notify(String(e)))
export const setRole = (id: number, role: Role) => invoke('together_role', { id, roleName: role }).catch((e) => notify(String(e)))
export const setNewRole = (role: Role) => invoke('together_new_role', { roleName: role }).catch((e) => notify(String(e)))
export const setSameRoom = (on: boolean) => invoke('together_same_room', { on }).catch((e) => notify(String(e)))
export const setLetIn = (on: boolean) => invoke('together_let_in_without_asking', { on }).catch((e) => notify(String(e)))
export const undoRoom = () => invoke('together_undo').catch((e) => notify(String(e)))
export const stepOut = (out: boolean) => invoke('together_step_out', { out }).catch((e) => notify(String(e)))
// Saved by the app: your listening choices here are read again, so a later change there
// doesn't put back the old ones.
export const setMyName = (name: string) =>
  invoke('together_set_name', { name })
    .then(() => loadListening(true))
    .catch((e) => notify(String(e)))
export const setMyDelay = (ms: number) =>
  invoke('together_set_delay', { ms: Math.round(ms) })
    .then(() => loadListening(true))
    .catch((e) => notify(String(e)))
export function jumpInRoom(index: number) {
  if (v2) return pressV2(v2, 'jump', { do: 'jump', entry: entryAt(v2, index) })
  return can(view, 'control') ? ask({ do: 'jump', index }) : explainNo('control')
}
export function removeFromRoom(index: number) {
  if (v2) return pressV2(v2, 'remove', { do: 'remove', entry: entryAt(v2, index) })
  return can(view, 'control') ? ask({ do: 'remove', index }) : explainNo('control')
}
export function moveInRoom(from: number, to: number) {
  if (v2) return pressV2(v2, 'move', { do: 'move', entry: entryAt(v2, from), to })
  return can(view, 'control') ? ask({ do: 'move', from, to }) : explainNo('control')
}

// --- The room panel v2's actions (the commands are WP9's; design, WP12 notes) -----------------

const call = (cmd: string, args?: Record<string, unknown>) => invoke(cmd, args).catch((e) => notify(String(e)))
const libraryRoot = async () => (await libraryHome()).library ?? ''

/** What the room panel v2 does, in the app. */
export const liveActions: RoomActions = {
  start: async () => startRoom(await libraryRoot()),
  join: async (code) => joinRoom(await libraryRoot(), code),
  readInvite: (code) => invoke<InvitePreview>('together_read_invite', { code }),
  joinNearby: async (id) => {
    root = await libraryRoot()
    await call('together_join_nearby', { root, id })
  },
  watchNearby: (on) => void invoke('together_nearby', { on }).catch(() => {}),
  resume: (yes) => void call('together_resume', { yes }),
  saveQueue: () => void call('together_save_queue'),
  setName: (name) => void setMyName(name),
  cancel: () => void leaveRoom(),
  leave: () => void leaveRoom(),
  end: () => void call('together_end'),
  handOver: (id) => void call('together_hand_over', { id }),
  toggle: () => void route.toggle(),
  next: () => void route.next(),
  previous: () => void route.previous(),
  seek: (ms) => void route.seek(ms),
  answer: (request, yes) => void ask(yes ? { do: 'accept', request } : { do: 'decline', request }),
  admit: (id) => void admit(id),
  deny: (id) => void deny(id),
  remove: (id, block) => void removeMember(id, block),
  setRole: (id, role) => void setRole(id, role),
  setNewRole: (role) => void call('together_new_role', { roleName: role }),
  setTransition: (transition) => void call('together_transition', { transition }),
  setGuestsVolume: (on) => void call('together_guests_volume', { on }),
  setLetIn: (on) => void setLetIn(on),
  setVisible: (on) => void call('together_visible', { on }),
  setVolume: (percent) => void ask({ do: 'volume', percent: Math.max(0, Math.min(100, Math.round(percent))) }),
  setMode: (mode) => void call('together_mode', { mode }),
  startWithNext: () => void call('together_start_with_next'),
  stepOut: (out) => void stepOut(out),
  setDelay: (ms) => void setMyDelay(ms),
  lineUp: (on) => void call('together_line_up', { on }),
  takeHostCopy: () => void call('together_use_host_copy'),
  lighter: (id) => void call('together_lighter', { id }),
  nextSongFor: (id) => void call('together_next_song_for', { id }),
  copyInvite: async () => {
    const link = v2?.invite?.link
    if (!link) return
    await navigator.clipboard?.writeText(link).catch(() => {})
    notify('Invite link copied. Send it to friends; you let each of them in.')
  },
  undo: () => void undoRoom(),
}

/** What the queue drawer's header says in a room. */
export function roomQueueLine(v: TogetherView): string {
  const q = v.room?.queue ?? []
  const at = v.room?.timeline?.index ?? -1
  const after = q.slice(at + 1)
  if (!after.length) return 'Nothing after this song'
  const left = after.reduce((t, s) => t + (s.duration_ms ?? 0), 0)
  return `${after.length.toLocaleString('en-IN')} up next, ${clock(left)}`
}

/** Whose room you're listening in, with whom (null out of a room). */
export function roomWords(v: TogetherView): string | null {
  if (!inRoom(v) || !v.room) return null
  if (!v.host) return `Listening in ${v.room.host_name}'s room`
  const others = v.room.members.filter((m) => !m.host).map((m) => m.name)
  if (!others.length) return 'Your room is open'
  const shown = others.length > 2 ? `${others.slice(0, 2).join(', ')} and ${others.length - 2} more` : others.join(' and ')
  return `Listening with ${shown}`
}
