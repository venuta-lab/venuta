// node --test tests/   (Node runs the TypeScript as is)
// YouTube Music history in Stats (lib/ytmHistory.ts): a play taken out leaves the list at once,
// and comes back by itself if YouTube Music doesn't take it out, whatever else went meanwhile.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { playKey, visibleDays } from '../src/lib/ytmHistory.ts'

const days = [
  { title: 'Today', tracks: [{ track_id: 'a' }, { track_id: 'b' }] },
  { title: 'October 2026', tracks: [{ track_id: 'c' }] },
]
const ids = (ds: ReturnType<typeof visibleDays<{ track_id: string }>>) =>
  ds.map((d) => [d.title, d.plays.map((p) => p.track.track_id)])

test('nothing taken out: every day and play, in order', () => {
  assert.deepEqual(ids(visibleDays(days, new Set())), [
    ['Today', ['a', 'b']],
    ['October 2026', ['c']],
  ])
})

test('a play taken out leaves its day; a day left empty goes too', () => {
  const gone = new Set([playKey(0, 1), playKey(1, 0)])
  assert.deepEqual(ids(visibleDays(days, gone)), [['Today', ['a']]])
})

test('each play keeps its own key, so bringing one back brings back only that one', () => {
  const gone = new Set([playKey(0, 0), playKey(0, 1)])
  gone.delete(playKey(0, 0))
  const shown = visibleDays(days, gone)
  assert.deepEqual(ids(shown), [
    ['Today', ['a']],
    ['October 2026', ['c']],
  ])
  assert.equal(shown[0].plays[0].key, playKey(0, 0))
})
