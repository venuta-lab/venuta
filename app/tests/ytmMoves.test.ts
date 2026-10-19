// node --test tests/   (Node runs the TypeScript as is)
// A YouTube Music playlist reordered in Venuta: the fewest moves (each "this item before that one")
// that turn the old order into the new, so a drag sends one change, not the whole list.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { movesBetween, applyMoves } from '../src/lib/ytmMoves.ts'

test('nothing moved: no moves', () => {
  assert.deepEqual(movesBetween(['a', 'b', 'c'], ['a', 'b', 'c']), [])
})

test('one song dragged up: one move, before the song it now precedes', () => {
  assert.deepEqual(movesBetween(['a', 'b', 'c', 'd'], ['a', 'd', 'b', 'c']), [{ set_video_id: 'd', before: 'b' }])
})

test('one song dragged to the end: one move, before nothing', () => {
  assert.deepEqual(movesBetween(['a', 'b', 'c'], ['b', 'c', 'a']), [{ set_video_id: 'a', before: null }])
})

test('any reshuffle: the moves, applied in order, give exactly the new order', () => {
  const cases: [string[], string[]][] = [
    [['a', 'b', 'c', 'd', 'e'], ['e', 'd', 'c', 'b', 'a']],
    [['a', 'b', 'c', 'd', 'e', 'f'], ['b', 'a', 'd', 'c', 'f', 'e']],
    [['a', 'b', 'c', 'd'], ['c', 'a', 'd', 'b']],
  ]
  for (const [from, to] of cases) {
    const moves = movesBetween(from, to)
    assert.deepEqual(applyMoves(from, moves), to, `${from.join('')} -> ${to.join('')}`)
    // Never more moves than songs that really changed place.
    assert.ok(moves.length < from.length)
  }
})
