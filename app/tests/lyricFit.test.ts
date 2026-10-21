// node --test tests/   (Node runs the TypeScript as is)
// The big screen's lines always fit the screen: a long line (a long Hindi line, a word or a run of
// words with no place to wrap, a wide theme font) is made smaller until it does, and only past the
// smallest size is it broken inside a word.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { MIN_FIT, nextFit, shrinkFor } from '../src/lib/lyricFit.ts'

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
