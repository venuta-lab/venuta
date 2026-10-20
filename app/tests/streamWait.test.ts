// node --test tests/   (Node runs the TypeScript as is)
// While an online song waits for its stream: what the player says, and whether a seek can go.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { canSeekNow, waitingWords } from '../src/lib/streamWait.ts'

test('playing: nothing to say, and seeking goes', () => {
  assert.equal(waitingWords({ buffering: false, stalled: false }), null)
  assert.equal(canSeekNow({ buffering: false, stalled: false }), true)
})

test('still opening: Opening…, and nothing to seek in yet', () => {
  assert.equal(waitingWords({ buffering: true, stalled: false }), 'Opening…')
  assert.equal(canSeekNow({ buffering: true, stalled: false }), false)
})

test('partway, waiting for more of the stream: Buffering…, and seeking still goes', () => {
  assert.equal(waitingWords({ buffering: true, stalled: true }), 'Buffering…')
  assert.equal(canSeekNow({ buffering: true, stalled: true }), true)
})

test('a report from before stalled was said reads as opening', () => {
  assert.equal(waitingWords({ buffering: true }), 'Opening…')
  assert.equal(canSeekNow({ buffering: true }), false)
})
