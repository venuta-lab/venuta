// node --test tests/   (Node runs the TypeScript as is)
// "Also on YouTube Music": what a sync did, and why one waited, said plainly; and when a link
// is due (on open at most once a minute, otherwise every half hour at most).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { syncWords, pauseWords, due, standWords } from '../src/lib/ytmLinkWords.ts'

const note = (n: Partial<Record<'pulled' | 'dropped' | 'pushed' | 'removed' | 'moved', number>>) => ({
  at: 0,
  pulled: 0,
  dropped: 0,
  pushed: 0,
  removed: 0,
  moved: 0,
  ...n,
})

test('nothing done says nothing', () => {
  assert.equal(syncWords('Road', note({})), '')
})

test('what came in and went out, both ways, in one line', () => {
  assert.equal(
    syncWords('Road', note({ pulled: 2, dropped: 1, pushed: 3 })),
    '“Road” is the same on YouTube Music now: 2 songs came in, 1 went, 3 added there.',
  )
  assert.equal(syncWords('Road', note({ moved: 4 })), '“Road” is the same on YouTube Music now: its order there matches yours.')
  assert.equal(syncWords('Road', note({ removed: 1 })), '“Road” is the same on YouTube Music now: 1 taken out there.')
})

test('a sync that waits says why and what nothing changed means', () => {
  assert.match(pauseWords('Road', { kind: 'unread' }), /nothing was changed/)
  assert.match(pauseWords('Road', { kind: 'held' }), /until you sync again/)
  // Syncing again after an Undo brings in what is there, and never takes songs out there.
  assert.match(pauseWords('Road', { kind: 'held' }), /Nothing there is taken out\./)
  assert.match(pauseWords('Road', { kind: 'other_account', account: 'x' }), /another YouTube Music account/)
  assert.equal(
    pauseWords('Road', { kind: 'many_gone', side: 'youtube_music', gone: 12, of: 15 }),
    '“Road” on YouTube Music has 12 of its 15 songs gone since the last sync, so nothing was changed. Sync anyway takes them out here too.',
  )
  assert.equal(
    pauseWords('Road', { kind: 'many_gone', side: 'venuta', gone: 15, of: 15 }),
    'Your “Road” has 15 of its 15 songs gone since the last sync, so nothing was sent. Sync anyway takes them out on YouTube Music too.',
  )
})

test('due: opened at most once a minute, otherwise every half hour', () => {
  const now = 10_000
  assert.equal(due(0, now, 'open'), true)
  assert.equal(due(now - 30, now, 'open'), false)
  assert.equal(due(now - 61, now, 'open'), true)
  assert.equal(due(now - 61, now, 'timer'), false)
  assert.equal(due(now - 30 * 60, now, 'timer'), true)
  assert.equal(due(now - 1, now, 'edit'), true)
})

test('how a linked playlist stands, and the 5,000-song limit there', () => {
  assert.equal(standWords(42, 8), '42 of 50 songs there; 8 only here.')
  assert.equal(standWords(3, 0), 'All 3 songs there.')
  assert.equal(standWords(1, 0), 'The 1 song there.')
  assert.equal(standWords(0, 2), 'None of its 2 songs there yet; 2 only here.')
  assert.match(standWords(4950, 10), /holds up to 5,000 songs/)
})
