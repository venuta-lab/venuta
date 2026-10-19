// node --test tests/   (Node runs the TypeScript as is)
// A thumbs down: the song leaves what is coming up, and Undo puts back exactly the rating it had.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { undoOfDislike, upcomingCopies } from '../src/lib/dislikes.ts'

const q = ['a', 'b', 'c', 'b', 'd', 'b']

test('a dislike takes the song out of what is coming up, from the last', () => {
  // Playing the first: both later copies go, the last first so each place stays right.
  assert.deepEqual(upcomingCopies(q, 0, 'b'), [5, 3, 1])
  // Playing the second b: it is skipped (not here), the later ones go.
  assert.deepEqual(upcomingCopies(q, 1, 'b'), [5, 3])
  // Songs already played stay where they are.
  assert.deepEqual(upcomingCopies(q, 4, 'b'), [5])
  assert.deepEqual(upcomingCopies(q, 0, 'e'), [])
  // Nothing playing: everything is still to come.
  assert.deepEqual(upcomingCopies(q, -1, 'a'), [0])
})

test('undo of a dislike puts back a like it replaced', () => {
  assert.deepEqual(undoOfDislike({ liked: true, disliked: false }), { undislike: true, relike: true, account: 'like' })
})

test('undo of a dislike of a song with no rating clears it', () => {
  assert.deepEqual(undoOfDislike({ liked: false, disliked: false }), { undislike: true, relike: false, account: 'none' })
})

test('undo of a dislike of a song disliked already changes nothing', () => {
  assert.deepEqual(undoOfDislike({ liked: false, disliked: true }), { undislike: false, relike: false, account: null })
})
