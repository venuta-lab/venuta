// node --test tests/   (Node runs the TypeScript as is)
// How tall a song's lines are on the desktop strip, and how the strip grows to fit the rows that
// are switched on: kept for the song (not the line), so the words never change size as the song
// moves, and the strip grows or shrinks only when a switch changes or a new song comes.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { grownStrip, stripRows, ROW } from '../src/lib/lyricRows.ts'

test('rows: the line, the next under it, then pronunciation and translation', () => {
  const none = { roman: false, translation: false }
  assert.equal(stripRows(1, none), ROW.line)
  assert.equal(stripRows(2, none), ROW.line + ROW.next)
  assert.equal(stripRows(1, { roman: true, translation: false }), ROW.line + ROW.roman)
  assert.equal(stripRows(2, { roman: true, translation: true }), ROW.line + ROW.next + ROW.roman + ROW.translation)
  // As it was before for a line and its Roman letters (1.18 + 0.89 + 0.83).
  assert.ok(Math.abs(stripRows(2, { roman: true, translation: false }) - 2.9) < 0.01)
  assert.ok(ROW.translation < ROW.roman, 'the translation row is the smaller')
})

const SCREEN = { x: 0, y: 0, w: 1920, h: 1040 }

test('a row switched on grows the strip so the words keep their size', () => {
  const before = stripRows(2, { roman: false, translation: false })
  const after = stripRows(2, { roman: true, translation: true })
  // Along the bottom of the screen: it grows upward, its bottom edge stays.
  const place = { x: 500, y: 800, w: 920, h: 200 }
  const band = 46
  const g = grownStrip(place, band, before, after, SCREEN)
  const room = (h: number) => h - band - 24
  assert.ok(Math.abs(room(g.h) / after - room(place.h) / before) < 0.6, 'the same size for a row')
  assert.equal(g.y + g.h, place.y + place.h, 'the bottom edge stays')
  assert.equal(g.x, place.x)
  assert.equal(g.w, place.w)
})

test('switched off, it shrinks back to where it was', () => {
  const place = { x: 500, y: 800, w: 920, h: 200 }
  const a = stripRows(2, { roman: false, translation: false })
  const b = stripRows(2, { roman: true, translation: true })
  const back = grownStrip(grownStrip(place, 46, a, b, SCREEN), 46, b, a, SCREEN)
  assert.ok(Math.abs(back.h - place.h) <= 1)
  assert.ok(Math.abs(back.y - place.y) <= 1)
})

test('near the top of the screen it grows downward', () => {
  const place = { x: 0, y: 20, w: 920, h: 200 }
  const g = grownStrip(place, 46, 2.07, 3.6, SCREEN)
  assert.equal(g.y, 20)
  assert.ok(g.h > 200)
})

test('never taller than the screen, never off it', () => {
  const place = { x: 0, y: 900, w: 920, h: 130 }
  const g = grownStrip(place, 46, 1.18, 40, SCREEN)
  assert.ok(g.h <= SCREEN.h)
  assert.ok(g.y >= SCREEN.y)
  assert.ok(g.y + g.h <= SCREEN.y + SCREEN.h)
})

test('the same rows: nothing moves', () => {
  const place = { x: 1, y: 2, w: 3, h: 200 }
  assert.deepEqual(grownStrip(place, 46, 2.9, 2.9, SCREEN), place)
})
