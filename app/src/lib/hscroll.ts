/*
  Rows that scroll sideways (shelves of cards, rows of chips): what a wheel, a key or an arrow
  button does over one. The rows themselves are components/ScrollRow.tsx; this is the part that
  decides, kept apart so it can be tested.

  Who scrolls what:
  - A sideways wheel scrolls the row under the pointer and nothing else: no place changes on a
    sideways wheel anywhere.
  - A wheel that turns in clicks (a mouse's tilt wheel, Shift and the wheel) glides the row by
    each click, on a row of cards a card or more a click. The row glides itself: the browser's
    own scroll animation follows Windows' "Animation effects" alone, and Venuta's Motion setting
    decides here (with less motion, the row moves at once).
  - A touchpad's two fingers sideways (and a free-spinning wheel) are the browser's: it already
    follows the fingers and glides on after them, better than a row could.
  - Up and down on the wheel over a row scrolls the page, as anywhere else: a row never takes it.
  - Ctrl and the wheel is zoom: never the row's.
*/

export interface WheelLike {
  deltaX: number
  deltaY: number
  /** 0 pixels, 1 lines, 2 pages (WheelEvent.DOM_DELTA_*). */
  deltaMode: number
  /** Chromium's older measure of a turn: 120 a click of a wheel, less for a touchpad's steps. */
  wheelDeltaX?: number
  wheelDeltaY?: number
  shiftKey: boolean
  ctrlKey: boolean
}

/** Where a row is: scrollLeft, scrollWidth and clientWidth. */
export interface RowBox {
  left: number
  width: number
  client: number
}

/** A line of a wheel that counts in lines, in pixels (what browsers use for one). */
export const LINE_PX = 40
/** One click of a wheel, in Chromium's older measure. */
const CLICK = 120
/** How close to an end counts as there (a fraction of a pixel or two of rounding). */
const END_SLACK = 4
/** An arrow button moves by most of what shows, so the card at the edge stays in sight. */
const PAGE = 0.85
/** How long a glide takes. */
export const GLIDE_MS = 260

/** A wheel delta in pixels, whatever it counted in. */
export function wheelPixels(delta: number, mode: number, page: number): number {
  return mode === 1 ? delta * LINE_PX : mode === 2 ? delta * page : delta
}

/** Whether a wheel turned in whole clicks (a mouse's wheel), not a touchpad's fine steps. */
export function inClicks(mode: number, legacy: number | undefined): boolean {
  if (mode !== 0) return true
  return legacy !== undefined && legacy !== 0 && legacy % CLICK === 0
}

/** Whether the row has more than it shows. */
export const overflows = (b: RowBox) => b.width - b.client > 1

/** Which ends of the row are in view: the arrows and the edge fades show where there is more. */
export function rowEnds(b: RowBox): { start: boolean; end: boolean } {
  return { start: b.left <= END_SLACK, end: b.left + b.client >= b.width - END_SLACK }
}

/** How far an arrow button moves the row: most of its visible width. */
export const pageBy = (client: number) => Math.max(1, Math.round(client * PAGE))

/**
 * What the row does with a wheel turn over it: `null` leaves it to the browser (a touchpad
 * sideways, up and down for the page, Ctrl for zoom), a number is how far the row glides itself
 * (a sideways click: the tilt wheel, or Shift and the wheel, whichever way it arrives).
 */
export function rowWheel(e: WheelLike, b: RowBox): number | null {
  if (e.ctrlKey || !overflows(b)) return null
  const dx = wheelPixels(e.deltaX, e.deltaMode, b.client)
  const dy = wheelPixels(e.deltaY, e.deltaMode, b.client)
  if (dx !== 0 && dy === 0) return inClicks(e.deltaMode, e.wheelDeltaX) ? dx : null
  // Shift and the wheel, as up and down (Windows usually turns it sideways first, as above).
  if (dx === 0 && dy !== 0 && e.shiftKey) return inClicks(e.deltaMode, e.wheelDeltaY) ? dy : null
  return null
}

/**
 * Where the row heads when moved `by`: on from `base` (where the last glide was heading, while it
 * is under way, so quick clicks add up instead of starting over mid-glide), never past its ends.
 * A row that snaps stops at one of its `stops` (each card's start), the next one on that way:
 * a click of the wheel moves a whole card at least.
 */
export function scrollTarget(b: RowBox, base: number, by: number, stops: number[] = []): number {
  const max = Math.max(0, b.width - b.client)
  const raw = Math.min(Math.max(0, base + by), max)
  if (!stops.length || by === 0) return raw
  const next = by > 0 ? Math.min(...stops.filter((s) => s >= raw - 0.5)) : Math.max(...stops.filter((s) => s <= raw + 0.5))
  return Number.isFinite(next) ? Math.min(Math.max(0, next), max) : raw
}

/**
 * Where the row scrolls to so an item (its `start` and `end` within the row's content) shows
 * whole, clear of the `pad` at each edge (a fade, the row's scroll padding); `null` when it
 * already does. In a row that snaps, `stops` are its stopping places: going right, it stops at
 * the first that shows the item whole.
 */
export function revealLeft(b: RowBox, item: { start: number; end: number }, pad: number, stops: number[] = []): number | null {
  const max = Math.max(0, b.width - b.client)
  const clamp = (x: number) => Math.min(Math.max(0, x), max)
  if (item.start - pad >= b.left - 0.5 && item.end + pad <= b.left + b.client + 0.5) return null
  if (item.start - pad < b.left) return clamp(item.start - pad)
  const need = item.end + pad - b.client
  const stop = stops.filter((s) => s >= need - 0.5).sort((x, y) => x - y)[0]
  return clamp(stop ?? need)
}

/** Where a glide from `from` to `to` is after `t` of `ms`: quick at first, settling gently. */
export function glideAt(from: number, to: number, t: number, ms = GLIDE_MS): number {
  const p = Math.min(1, Math.max(0, t / ms))
  return from + (to - from) * (1 - (1 - p) ** 3)
}

/** The item a Left or Right key moves to in a row of `count`, or none at its ends. */
export function stepItem(at: number, key: string, count: number): number | null {
  const dir = key === 'ArrowRight' ? 1 : key === 'ArrowLeft' ? -1 : 0
  const to = at + dir
  return dir && at >= 0 && to >= 0 && to < count ? to : null
}
