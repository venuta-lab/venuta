// node --test tests/   (Node runs the TypeScript as is)
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { keepPlayable, planEnqueue } from '../src/lib/queueRules.ts'

const q = ['a', 'b', 'c', 'd']

test('a song coming up already is not added again', () => {
  assert.deepEqual(planEnqueue(q, 0, ['c', 'e'], false), { add: ['e'], move: null, already: 1 })
})

test('a song already played can come round again', () => {
  assert.deepEqual(planEnqueue(q, 2, ['a'], false).add, ['a'])
})

test('Play next on a song coming up later moves it up instead', () => {
  assert.deepEqual(planEnqueue(q, 0, ['d'], true), { add: [], move: { from: 3, to: 1 }, already: 0 })
  // It is next already: nothing to do.
  assert.deepEqual(planEnqueue(q, 0, ['b'], true), { add: [], move: null, already: 1 })
})

test('the same song twice in what is added goes in once', () => {
  assert.deepEqual(planEnqueue([], -1, ['x', 'x', 'y'], false).add, ['x', 'y'])
})

test('local only plays your songs, from the one you chose', () => {
  const songs = ['mine-1', 'online-1', 'mine-2', 'online-2', 'mine-3']
  const mine = (s: string) => s.startsWith('mine')
  assert.deepEqual(keepPlayable(songs, 2, mine), { items: ['mine-1', 'mine-2', 'mine-3'], index: 1 })
  assert.deepEqual(keepPlayable(songs, 0, mine), { items: ['mine-1', 'mine-2', 'mine-3'], index: 0 })
  assert.equal(keepPlayable(songs, 1, mine), null, 'an online song chosen cannot play')
  assert.equal(keepPlayable([], 0, mine), null)
})
