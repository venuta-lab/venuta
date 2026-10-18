// node --test tests/   (Node runs the TypeScript as is)
// What you pin to the top of Home: once each, newest first, at most twelve.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { pinned, shortcutKey, togglePin, type Shortcut } from '../src/lib/shortcutList.ts'

const album = (dir: string): Shortcut => ({ kind: 'album', dir, title: dir })
const online = (id: string): Shortcut => ({ kind: 'online-album', album: { provider: 'p', album_id: id, title: id, artists: [], track_count: null, release_date: null, artwork_url: null } })

test('pinning puts it first; pinning it again takes it off', () => {
  let l: Shortcut[] = []
  l = togglePin(l, album('A'))
  l = togglePin(l, online('x'))
  assert.deepEqual(l.map(shortcutKey), ['online-album:x', 'album:A'])
  assert.ok(pinned(l, album('A')))
  l = togglePin(l, album('A'))
  assert.deepEqual(l.map(shortcutKey), ['online-album:x'])
})

test('at most twelve: the oldest goes', () => {
  let l: Shortcut[] = []
  for (let i = 0; i < 14; i++) l = togglePin(l, album(String(i)))
  assert.equal(l.length, 12)
  assert.equal(shortcutKey(l[0]), 'album:13')
  assert.ok(!pinned(l, album('0')))
})

test('what was kept but is not a shortcut is left out', () => {
  assert.deepEqual(togglePin([{ kind: 'nonsense' } as unknown as Shortcut], album('A')).map(shortcutKey), ['album:A'])
})
