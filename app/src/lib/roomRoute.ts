/*
  While you're in a room (listening together), the player's own buttons go to the room: play,
  pause, skip and seek ask the room, and playing or queueing songs adds them to it. lib/together.ts
  sets this; lib/player.ts asks it first. Kept apart so neither imports the other.
*/
import type { QueueItem } from './player'

export interface RoomRoute {
  toggle: () => Promise<unknown>
  next: () => Promise<unknown>
  previous: () => Promise<unknown>
  seek: (ms: number) => Promise<unknown>
  /** Play these, from this one: added after the room's song, then played. */
  play: (items: QueueItem[], index: number) => Promise<unknown>
  enqueue: (items: QueueItem[], next: boolean) => Promise<unknown>
}

let route: RoomRoute | null = null

export const setRoomRoute = (r: RoomRoute | null) => {
  route = r
}

export const roomRoute = () => route
