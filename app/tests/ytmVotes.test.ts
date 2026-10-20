// node --test tests/   (Node runs the TypeScript as is)
// Votes on a collaborative YouTube Music playlist (lib/ytmVotes.ts): which token a press sends,
// the vote it leaves, and the count shown for it, all from what the row came with.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { votePress, voteCount } from '../src/lib/ytmVotes.ts'

const votes = (mine: 'up' | 'down' | 'neutral' = 'neutral') => ({
  mine,
  votes: 0,
  count: '0',
  count_up: '1',
  count_down: '-1',
  up: { token: 'UP', undo_token: 'UNDO-UP' },
  down: { token: 'DOWN', undo_token: 'UNDO-DOWN' },
})

test('no vote yet: up sends the up token and leaves the vote up', () => {
  assert.deepEqual(votePress(votes(), 'neutral', 'up'), { token: 'UP', next: 'up' })
  assert.deepEqual(votePress(votes(), 'neutral', 'down'), { token: 'DOWN', next: 'down' })
})

test('pressing the vote already given takes it back with that button’s other token', () => {
  assert.deepEqual(votePress(votes('up'), 'up', 'up'), { token: 'UNDO-UP', next: 'neutral' })
  assert.deepEqual(votePress(votes('down'), 'down', 'down'), { token: 'UNDO-DOWN', next: 'neutral' })
})

test('voted up, pressing down votes down', () => {
  assert.deepEqual(votePress(votes('up'), 'up', 'down'), { token: 'DOWN', next: 'down' })
})

test('a button with nothing to take its vote back by can’t be pressed again', () => {
  const v = { ...votes('up'), up: { token: 'UP', undo_token: null } }
  assert.equal(votePress(v, 'up', 'up'), null)
})

test('the count shown is YouTube’s own words for the vote as it stands', () => {
  assert.equal(voteCount(votes(), 'neutral'), '0')
  assert.equal(voteCount(votes(), 'up'), '1')
  assert.equal(voteCount(votes(), 'down'), '-1')
})

test('without those words, the count moves by the vote from where it came', () => {
  const bare = { ...votes('up'), votes: 3, count: null, count_up: null, count_down: null }
  assert.equal(voteCount(bare, 'up'), '3')
  assert.equal(voteCount(bare, 'neutral'), '2')
  assert.equal(voteCount(bare, 'down'), '1')
})
