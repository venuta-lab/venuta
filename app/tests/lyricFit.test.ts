// node --test tests/   (Node runs the TypeScript as is)
// The big screen's lines always fit the screen: a long line (a long Hindi line, a word or a run of
// words with no place to wrap, a wide theme font) is made smaller until it does, and only past the
// smallest size is it broken inside a word.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { MIN_FIT, nextFit, shrinkFor, stageRoom } from '../src/lib/lyricFit.ts'

const row = (scroll: number, client: number) => ({ scroll, client })

test('lines that fit stay as they are', () => {
  assert.equal(shrinkFor({ rows: [row(1200, 1200), row(800, 1200)], height: 500, room: 700 }), 1)
  assert.equal(nextFit(1, 1), 1)
  // Rounding: a pixel over is not overflow.
  assert.equal(shrinkFor({ rows: [row(1201, 1200)], height: 701, room: 700 }), 1)
})

test('a line wider than the screen (nowhere to wrap) shrinks until it is as wide as the screen', () => {
  const need = shrinkFor({ rows: [row(1200, 1200), row(2400, 1200)], height: 300, room: 700 })
  assert.equal(need, 0.5)
  const fit = nextFit(1, need)
  assert.ok(fit < 0.5 && fit > 0.45, `a little under, so it lands inside: ${fit}`)
})

test('lines taller than the room shrink to its height, whichever is tighter wins', () => {
  assert.equal(shrinkFor({ rows: [row(1000, 1200)], height: 1400, room: 700 }), 0.5)
  assert.equal(shrinkFor({ rows: [row(1500, 1200)], height: 1400, room: 700 }), 0.5)
  assert.equal(shrinkFor({ rows: [row(4800, 1200)], height: 1400, room: 700 }), 0.25)
})

test('never smaller than the smallest size: past it, the line breaks inside its words instead', () => {
  assert.equal(nextFit(1, 0.01), MIN_FIT)
  assert.equal(nextFit(MIN_FIT, 0.5), MIN_FIT)
  assert.ok(MIN_FIT > 0.2 && MIN_FIT < 0.5)
})

test('nothing measured yet (no room, no width) asks nothing', () => {
  assert.equal(shrinkFor({ rows: [row(0, 0)], height: 0, room: 0 }), 1)
  assert.equal(shrinkFor({ rows: [], height: 10, room: 0 }), 1)
})

test('the desktop strip: a line with nowhere to wrap shrinks across before it is cut, and breaks only at the smallest size', () => {
  // A strip 496 px across (520 less its padding); the line one word, 1100 px wide at the chosen size.
  const strip = 496
  const word = 1100
  let fit = 1
  for (let i = 0; i < 10; i++) {
    const step = nextFit(fit, shrinkFor({ rows: [row(word * fit, strip)], height: 60 * fit, room: 248 }))
    if (step === fit) break
    fit = step
  }
  assert.ok(word * fit <= strip && fit > MIN_FIT, `fits across at ${fit}`)
  // Four times wider than the strip: it stops at the smallest size (where its word may break).
  fit = 1
  for (let i = 0; i < 10; i++) fit = nextFit(fit, shrinkFor({ rows: [row(4 * strip * fit, strip)], height: 60 * fit, room: 248 }))
  assert.equal(fit, MIN_FIT)
})

test("the big screen's lines have the room above the song bar, however tall the bar grows", () => {
  // No song bar (Clear, or nothing playing): the whole screen, with a margin top and bottom.
  assert.deepEqual(stageRoom(720, null), { top: 36, bottom: 36, room: 648 })
  // The song bar's top 580 px down a 720 px screen: the lines end a gap above it, and keep a
  // margin under the window's buttons at the top.
  const r = stageRoom(720, 580)
  assert.ok(Math.abs(r.top + r.room - (580 - 720 * 0.03)) < 1e-9, 'they end a gap above the bar')
  assert.equal(r.bottom, 720 - 580 + 720 * 0.03)
  assert.equal(r.top + r.room + r.bottom, 720)
  // The bar grows (where the rows came from, the translation note with its button): less room.
  const taller = stageRoom(720, 540)
  assert.equal(r.room - taller.room, 40)
  // A bar reaching the top of the screen leaves no room, never less.
  assert.equal(stageRoom(720, 30).room, 0)
})
