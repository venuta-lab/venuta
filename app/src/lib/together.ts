import { invoke } from '@tauri-apps/api/core'
import { useEffect, useSyncExternalStore } from 'react'
import { inApp } from './engine'
import { loadListening } from './listening'
import { notify } from './notify'
import { clock, getPlayerState, keepQueue, restoreQueue, type QueueItem } from './player'
import { setRoomRoute } from './roomRoute'

/*
  Listening together (app together/): host a room from this Venuta or join one, and everyone
  hears the same song at the same moment, each from their best copy. The app sends its view as
  the "together" event after every change; here it's kept for the screens, and while you're in a
  room the player's buttons go to the room (lib/roomRoute.ts).
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
let version = 0
let started = false
const listeners = new Set<() => void>()
/** Your own queue, kept aside while you're in a room (given back when you leave). */
let yours: ReturnType<typeof keepQueue> = null
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

function publish(v: TogetherView) {
  const was = view.state
  view = v
  changed()
  setRoomRoute(inRoom(v) ? route : null)
  // A room that ended (removed, the host ended it, lost): your own music back.
  if (v.state === 'ended' && was !== 'ended') {
    if (v.message) notify(v.message)
    giveBack()
  }
}

const inRoom = (v: TogetherView) => v.state === 'hosting' || v.state === 'in' || v.state === 'reconnecting'

/** Listen for the app's view (once). */
export function startTogether() {
  if (started || !inApp()) return
  started = true
  void invoke<TogetherView>('together_view').then(publish).catch(() => {})
  void invoke<string | null>('together_take_invite').then(offerInvite).catch(() => {})
  void import('@tauri-apps/api/event').then(({ listen }) => {
    void listen<TogetherView>('together', (e) => publish(e.payload))
    // Why a button pressed elsewhere (the tray, media keys, the mini player) did nothing.
    void listen<string>('together-note', (e) => notify(e.payload))
    void listen<string>('together-invite', (e) => offerInvite(e.payload))
  })
}

export const togetherView = () => view

/** The room's view, kept current. */
export function useTogether(): TogetherView {
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
  return view
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

/** The player's buttons, in a room. */
const route = {
  toggle: () => {
    if (!can(view, 'control')) return explainNo('control')
    return ask({ do: view.room?.timeline?.playing ? 'pause' : 'play' })
  },
  next: () => (can(view, 'control') ? ask({ do: 'next' }) : explainNo('control')),
  previous: () => (can(view, 'control') ? ask({ do: 'previous' }) : explainNo('control')),
  seek: (ms: number) => (can(view, 'control') ? ask({ do: 'seek', position_ms: Math.max(0, Math.round(ms)) }) : explainNo('control')),
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

function giveBack() {
  const k = yours
  yours = null
  if (k) void restoreQueue(k)
}

/** Open a room with what's playing (it goes on, now for everyone). */
export async function startRoom(libraryRoot: string) {
  root = libraryRoot
  const s = getPlayerState()
  yours = keepQueue()
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
      yours = null
    }
  } catch (e) {
    yours = null
    notify(String(e))
  }
}

/** What an invite says before joining: whose room, and whether it still works. */
export const readInvite = (code: string) => invoke<{ host: string; open: boolean; until: number }>('together_read_invite', { code })

export async function joinRoom(libraryRoot: string, code: string) {
  dropInvite()
  root = libraryRoot
  yours = keepQueue()
  try {
    const v = await invoke<TogetherView>('together_join', { root: libraryRoot, code })
    publish(v)
    if (v.message && v.state !== 'waiting' && v.state !== 'in' && v.state !== 'joining') {
      notify(v.message)
      giveBack()
    }
  } catch (e) {
    notify(String(e))
    yours = null
  }
}

/** Leave (the host: end it for everyone). Your own queue comes back. */
export async function leaveRoom() {
  await invoke('together_leave').catch(() => {})
  giveBack()
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
export const jumpInRoom = (index: number) => (can(view, 'control') ? ask({ do: 'jump', index }) : explainNo('control'))
export const removeFromRoom = (index: number) => (can(view, 'control') ? ask({ do: 'remove', index }) : explainNo('control'))
export const moveInRoom = (from: number, to: number) => (can(view, 'control') ? ask({ do: 'move', from, to }) : explainNo('control'))

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
