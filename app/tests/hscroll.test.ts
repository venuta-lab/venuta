// node --test tests/   (Node runs the TypeScript as is)
// Rows that scroll sideways: which wheel turns a row takes, how far it goes, and where its ends are.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  GLIDE_MS,
  LINE_PX,
  glideAt,
  inClicks,
  overflows,
  pageBy,
  revealLeft,
  rowEnds,
  rowWheel,
  scrollTarget,
  stepItem,
  wheelPixels,
  type WheelLike,
} from '../src/lib/hscroll.ts'

const wheel = (w: Partial<WheelLike>): WheelLike => ({ deltaX: 0, deltaY: 0, deltaMode: 0, shiftKey: false, ctrlKey: false, ...w })
/** A row 1000 wide showing 400, scrolled to `left`. */
const row = (left = 0, width = 1000, client = 400) => ({ left, width, client })
/** Where a row of cards 168 wide, 20 apart, can stop. */
const STOPS = [0, 188, 376, 564]

test('a click of the tilt wheel glides the row by it, either way', () => {
  assert.equal(rowWheel(wheel({ deltaX: 100, wheelDeltaX: -120 }), row()), 100)
  assert.equal(rowWheel(wheel({ deltaX: -100, wheelDeltaX: 120 }), row(300)), -100)
  // Two clicks at once, and a wheel that counts in lines.
  assert.equal(rowWheel(wheel({ deltaX: 200, wheelDeltaX: -240 }), row()), 200)
  assert.equal(rowWheel(wheel({ deltaX: 3, deltaMode: 1 }), row()), 3 * LINE_PX)
})

test('a touchpad sideways (and a free-spinning wheel) is the browser’s to scroll the row with', () => {
  assert.equal(rowWheel(wheel({ deltaX: 4.5, wheelDeltaX: -5 }), row()), null)
  assert.equal(rowWheel(wheel({ deltaX: 40, wheelDeltaX: -48 }), row()), null)
  // A diagonal swipe: neither a click nor the row's.
  assert.equal(rowWheel(wheel({ deltaX: -12, deltaY: 3, wheelDeltaX: 14, wheelDeltaY: -4 }), row(200)), null)
  // Nothing says it came in clicks.
  assert.equal(rowWheel(wheel({ deltaX: 100 }), row()), null)
})

test('up and down over a row stays the page’s', () => {
  assert.equal(rowWheel(wheel({ deltaY: 100, wheelDeltaY: -120 }), row()), null)
  assert.equal(rowWheel(wheel({ deltaY: -100, wheelDeltaY: 120 }), row(300)), null)
})

test('Shift and the wheel scrolls the row, sideways already or as up and down', () => {
  // As Windows sends it: turned sideways.
  assert.equal(rowWheel(wheel({ deltaX: 100, wheelDeltaX: -120, shiftKey: true }), row()), 100)
  // As up and down with Shift held.
  assert.equal(rowWheel(wheel({ deltaY: 100, wheelDeltaY: -120, shiftKey: true }), row()), 100)
  assert.equal(rowWheel(wheel({ deltaY: -100, wheelDeltaY: 120, shiftKey: true }), row(300)), -100)
  assert.equal(rowWheel(wheel({ deltaY: 1, deltaMode: 2, shiftKey: true }), row()), 400)
})

test('a row with nothing more to show takes no wheel at all', () => {
  assert.equal(rowWheel(wheel({ deltaX: 100, wheelDeltaX: -120 }), row(0, 400, 400)), null)
  assert.equal(overflows(row(0, 401, 400)), false)
  assert.equal(overflows(row(0, 402, 400)), true)
})

test('Ctrl and the wheel is zoom, never the row', () => {
  assert.equal(rowWheel(wheel({ deltaY: 100, wheelDeltaY: -120, shiftKey: true, ctrlKey: true }), row()), null)
  assert.equal(rowWheel(wheel({ deltaX: 100, wheelDeltaX: -120, ctrlKey: true }), row()), null)
})

test('clicks are whole 120s, or lines and pages', () => {
  assert.equal(inClicks(0, -120), true)
  assert.equal(inClicks(0, 360), true)
  assert.equal(inClicks(0, -48), false)
  assert.equal(inClicks(0, 0), false)
  assert.equal(inClicks(0, undefined), false)
  assert.equal(inClicks(1, undefined), true)
})

test('wheel deltas in lines or pages become pixels', () => {
  assert.equal(wheelPixels(2, 0, 500), 2)
  assert.equal(wheelPixels(2, 1, 500), 2 * LINE_PX)
  assert.equal(wheelPixels(-1, 2, 500), -500)
})

test('quick clicks add up from where the row is heading, and stop at its ends', () => {
  assert.equal(scrollTarget(row(100), 100, 100), 200)
  assert.equal(scrollTarget(row(130), 200, 100), 300)
  assert.equal(scrollTarget(row(500), 550, 100), 600)
  assert.equal(scrollTarget(row(50), 50, -100), 0)
  // A row that fits never moves.
  assert.equal(scrollTarget(row(0, 300, 400), 0, 100), 0)
})

test('in a row of cards each click moves a card on, whatever the click’s size', () => {
  assert.equal(scrollTarget(row(0), 0, 100, STOPS), 188)
  assert.equal(scrollTarget(row(120), 188, 100, STOPS), 376)
  assert.equal(scrollTarget(row(376), 376, -100, STOPS), 188)
  assert.equal(scrollTarget(row(0), 0, 33, STOPS), 188)
  // A bigger click can pass more than one card.
  assert.equal(scrollTarget(row(0), 0, 300, STOPS), 376)
  // Left from between cards: the one before.
  assert.equal(scrollTarget(row(250), 250, -40, STOPS), 188)
  // Past the last stop: the row's end.
  assert.equal(scrollTarget(row(564), 564, 100, STOPS), 600)
  assert.equal(scrollTarget(row(0), 0, -100, STOPS), 0)
})

test('an arrow pages by most of a visible width, to the card that was cut off', () => {
  assert.equal(pageBy(1000), 850)
  assert.equal(pageBy(0), 1)
  const wide = { left: 0, width: 4000, client: 1248 }
  const stops = Array.from({ length: 20 }, (_, i) => i * 188)
  assert.equal(scrollTarget(wide, 0, pageBy(1248), stops), 1128)
  assert.equal(scrollTarget({ ...wide, left: 1128 }, 1128, -pageBy(1248), stops), 0)
})

test('a glide starts quick, settles gently, and ends where it was going', () => {
  assert.equal(glideAt(0, 200, 0), 0)
  assert.equal(glideAt(0, 200, GLIDE_MS), 200)
  assert.equal(glideAt(0, 200, GLIDE_MS * 3), 200)
  const half = glideAt(0, 200, GLIDE_MS / 2)
  assert.ok(half > 150 && half < 200, `half way: ${half}`)
  assert.equal(glideAt(300, 100, GLIDE_MS), 100)
})

test('the ends of a row, with a pixel or two of rounding', () => {
  assert.deepEqual(rowEnds(row(0)), { start: true, end: false })
  assert.deepEqual(rowEnds(row(3)), { start: true, end: false })
  assert.deepEqual(rowEnds(row(250)), { start: false, end: false })
  assert.deepEqual(rowEnds(row(597.5)), { start: false, end: true })
  assert.deepEqual(rowEnds(row(0, 400, 400)), { start: true, end: true })
})

test('an item a key moves to comes whole into view, and the row moves only when it must', () => {
  // Cards 100 wide, 20 apart, starting at 12; the row shows 400 of 1000.
  const stops = [0, 120, 240, 360, 480, 600]
  // Already whole: no move.
  assert.equal(revealLeft(row(0), { start: 132, end: 232 }, 12, stops), null)
  // Cut off on the right: the first stop that shows it whole.
  assert.equal(revealLeft(row(0), { start: 372, end: 472 }, 12, stops), 120)
  // Cut off on the left: its start, less the pad.
  assert.equal(revealLeft(row(240), { start: 132, end: 232 }, 12, stops), 120)
  // A row that doesn't snap stops just where it shows whole.
  assert.equal(revealLeft(row(0), { start: 372, end: 472 }, 24), 96)
  // Never past the ends.
  assert.equal(revealLeft(row(500), { start: 980, end: 1000 }, 24), 600)
  assert.equal(revealLeft(row(100), { start: 4, end: 60 }, 24), 0)
})

test('Left and Right move between a row’s items and stop at its ends', () => {
  assert.equal(stepItem(2, 'ArrowRight', 5), 3)
  assert.equal(stepItem(2, 'ArrowLeft', 5), 1)
  assert.equal(stepItem(0, 'ArrowLeft', 5), null)
  assert.equal(stepItem(4, 'ArrowRight', 5), null)
  assert.equal(stepItem(2, 'ArrowDown', 5), null)
  assert.equal(stepItem(-1, 'ArrowRight', 5), null)
})
