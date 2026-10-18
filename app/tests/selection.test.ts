// node --test tests/   (Node runs the TypeScript as is)
// Picking songs in a list to act on them together: a click picks one more (or lets it go), Shift
// picks everything from the last one picked, and the picks keep the list's order.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { pick, pickAll, picked, type Picks } from '../src/lib/selection.ts'

const none: Picks = { keys: [], anchor: null }
const list = ['a', 'b', 'c', 'd', 'e']

test('a click picks a song, and a second click lets it go', () => {
  const one = pick(none, list, 'b')
  assert.deepEqual(one.keys, ['b'])
  assert.deepEqual(pick(one, list, 'b').keys, [])
})

test('Shift picks every song from the last one picked to this one, either way', () => {
  const from = pick(none, list, 'b')
  assert.deepEqual(pick(from, list, 'd', { range: true }).keys, ['b', 'c', 'd'])
  assert.deepEqual(pick(pick(none, list, 'd'), list, 'b', { range: true }).keys, ['b', 'c', 'd'])
})

test('Shift with nothing picked yet picks just that song', () => {
  assert.deepEqual(pick(none, list, 'c', { range: true }).keys, ['c'])
})

test('picks come back in the order of the list, whatever order they were picked in', () => {
  let p = pick(none, list, 'e')
  p = pick(p, list, 'a')
  p = pick(p, list, 'c')
  assert.deepEqual(picked(p, list), ['a', 'c', 'e'])
})

test('a pick no longer in the list (it was filtered out) is not acted on', () => {
  const p = pick(pick(none, list, 'a'), list, 'z')
  assert.deepEqual(picked(p, list), ['a'])
})

test('Select all picks the whole list; again lets all go', () => {
  const all = pickAll(none, list)
  assert.deepEqual(all.keys, list)
  assert.deepEqual(pickAll(all, list).keys, [])
})
