// node --test tests/   (Node runs the TypeScript as is)
// "Also on YouTube Music": what a sync did, and why one waited, said plainly; and when a link
// is due (on open at most once a minute, otherwise every half hour at most).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { syncWords, pauseWords, due } from '../src/lib/ytmLinkWords.ts'

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
