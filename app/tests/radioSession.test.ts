// node --test tests/   (Node runs the TypeScript as is)
import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  PAGE_AT,
  RETRY_MS,
  TRIES,
  afterChip,
  afterContinue,
  afterMarker,
  afterPage,
  afterRelated,
  failed,
  hybridRows,
  listSession,
  loadSession,
  matchMemory,
  nextStep,
  radioSession,
  saveSession,
  upcomingReplaced,
  type RadioSession,
} from '../src/lib/radioSession.ts'
import type { OnlineRadio } from '../src/lib/engine.ts'

const track = (id: string, title = id) => ({ provider: 'remote_catalog', track_id: id, title, artists: ['Someone'], album: null, album_id: null, duration_ms: 200_000, artwork_url: null, isrc: null })

const first: OnlineRadio = {
  playlist_id: 'RDAMVMseed',
  title: 'Seed Radio',
  tracks: [track('a'), track('b'), track('a')],
  endpoint: { video_id: 'seed', playlist_id: 'RDAMVMseed', params: 'wAEB' },
  continuation: 'TOKEN2',
  chips: [
    { text: 'All', selected: true, endpoint: { playlist_id: 'RDAMVMseed', params: 'X' } },
    { text: 'Familiar', selected: false, endpoint: { playlist_id: 'RDATiYvseed' } },
  ],
  continue_with: null,
  related_id: 'MPTRt_seed',
}

const at = (left: number, more: Partial<{ autoplay: boolean; repeat: 'off' | 'all' | 'one'; now: number }> = {}) => ({
  left,
  autoplay: true,
  repeat: 'off' as const,
  now: 1_000_000,
  ...more,
})

test('a radio keeps its session: endpoint, page token, name, chips', () => {
  const s = radioSession('k', first)
  assert.equal(s.kind, 'radio')
  assert.deepEqual(s.endpoint, first.endpoint)
  assert.equal(s.continuation, 'TOKEN2')
  assert.equal(s.title, 'Seed Radio')
  assert.deepEqual(
    s.chips.map((c) => [c.text, c.selected]),
    [
      ['All', true],
      ['Familiar', false],
    ],
  )
  assert.equal(s.relatedId, 'MPTRt_seed')
})

test('the next page is asked for at five songs left, with the same endpoint and its token', () => {
  const s = radioSession('k', first)
  assert.equal(nextStep(s, at(PAGE_AT + 1)), null)
  assert.deepEqual(nextStep(s, at(PAGE_AT)), { kind: 'page', endpoint: first.endpoint, continuation: 'TOKEN2' })
})

test('a page moves the session to the page after it', () => {
  const s = afterPage(radioSession('k', first), { ...first, tracks: [track('c')], continuation: 'TOKEN3', chips: [] })
  assert.equal(s.continuation, 'TOKEN3')
  // A later page has no chips: the first page's stay.
  assert.equal(s.chips.length, 2)
  assert.deepEqual(s.endpoint, first.endpoint)
})

test('repeat on or nothing playing asks nothing; autoplay off still pages a radio', () => {
  const s = radioSession('k', first)
  assert.equal(nextStep(s, at(0, { repeat: 'all' })), null)
  assert.equal(nextStep(s, at(0, { repeat: 'one' })), null)
  assert.equal(nextStep(s, at(0, { autoplay: false }))?.kind, 'page')
})

test('three failed tries stop the paging, with a wait between tries', () => {
  let s = radioSession('k', first)
  s = failed(s, 1_000_000)
  assert.equal(nextStep(s, at(0, { now: 1_000_000 + RETRY_MS - 1 })), null, 'waits before trying again')
  assert.equal(nextStep(s, at(0, { now: 1_000_000 + RETRY_MS }))?.kind, 'page')
  for (let i = 1; i < TRIES; i++) s = failed(s, 1_000_000)
  assert.equal(s.continuation, null, 'gave up on the pages')
  // Then what a radio does at its end: here YouTube's Related songs.
  assert.equal(nextStep(s, at(0, { now: 1_000_000 + RETRY_MS }))?.kind, 'related')
})

test('a radio that ends goes on with what it names, else Related, else stops and says so', () => {
  const named = { playlist_id: 'RDAMPLmix', params: 'P' }
  let s = afterPage(radioSession('k', first), { ...first, continuation: null, continue_with: named, chips: [] })
  assert.deepEqual(nextStep(s, at(PAGE_AT)), { kind: 'continue', endpoint: named })
  // Only with Autoplay on.
  assert.equal(nextStep(s, at(0, { autoplay: false })), null)
  // The mix it continued into is the session now: its endpoint, its pages, its chips.
  s = afterContinue(s, { ...first, endpoint: named, continuation: 'MIX2', title: 'The Mix', continue_with: named })
  assert.deepEqual(s.endpoint, named)
  assert.equal(s.continuation, 'MIX2')
  assert.equal(s.title, 'The Mix')
  assert.equal(s.continueWith, null, 'never the same one again')
  s = afterPage(s, { ...first, continuation: null, chips: [], continue_with: null })
  assert.equal(nextStep(s, at(0))?.kind, 'related')
  s = afterRelated(s)
  assert.equal(nextStep(s, at(0))?.kind, 'end')
})

test('a continue-with that brings nothing goes on to Related', () => {
  const named = { playlist_id: 'RDAMPLmix' }
  let s = afterPage(radioSession('k', first), { ...first, continuation: null, continue_with: named, chips: [] })
  s = afterContinue(s, { ...first, tracks: [], continuation: null, continue_with: null, endpoint: named })
  assert.equal(nextStep(s, at(0))?.kind, 'related')
})

test('an album or playlist goes on only once its last song plays, with Autoplay on', () => {
  const s = listSession('k', 'OLAK5uy_album')
  assert.equal(nextStep(s, at(1)), null)
  assert.equal(nextStep(s, at(0, { autoplay: false })), null)
  // Its continue-with isn't known yet: the list's own answer names it.
  assert.deepEqual(nextStep(s, at(0)), { kind: 'marker' })
  const named = { playlist_id: 'RDAMPLOLAK5uy_album', params: 'P' }
  const marked = afterMarker(s, { playlist_id: 'OLAK5uy_album', tracks: [track('x')], continue_with: named, related_id: 'MPTRt_x' })
  assert.deepEqual(nextStep(marked, at(0)), { kind: 'continue', endpoint: named })
  // No marker at all: Related, then the end.
  const bare = afterMarker(s, { playlist_id: 'OLAK5uy_album', tracks: [], continue_with: null })
  assert.equal(nextStep(bare, at(0))?.kind, 'related')
})

test('a list that continued into a radio pages that radio', () => {
  const named = { playlist_id: 'RDAMPLOLAK5uy_album' }
  let s = afterMarker(listSession('k', 'OLAK5uy_album'), { playlist_id: 'OLAK5uy_album', tracks: [], continue_with: named })
  s = afterContinue(s, { ...first, endpoint: named, continuation: 'R2' })
  assert.equal(s.kind, 'radio')
  assert.deepEqual(nextStep(s, at(PAGE_AT)), { kind: 'page', endpoint: named, continuation: 'R2' })
})

test('a chip re-tunes the session: its endpoint, its pages, it marked as chosen', () => {
  const s = afterChip(radioSession('k', first), 1, { ...first, endpoint: first.chips![1].endpoint, continuation: 'F2', chips: [] })
  assert.deepEqual(s.endpoint, { playlist_id: 'RDATiYvseed' })
  assert.equal(s.continuation, 'F2')
  assert.deepEqual(
    s.chips.map((c) => c.selected),
    [false, true],
  )
  // Chips sent with the answer win.
  const sent = afterChip(radioSession('k', first), 1, first)
  assert.deepEqual(
    sent.chips.map((c) => c.selected),
    [true, false],
  )
})

test('choosing a chip keeps the songs played and the one playing, and replaces the rest', () => {
  assert.deepEqual(upcomingReplaced(['p1', 'p2', 'now', 'u1', 'u2'], 2, ['n1', 'n2']), ['p1', 'p2', 'now', 'n1', 'n2'])
  assert.deepEqual(upcomingReplaced(['now'], 0, ['n1']), ['now', 'n1'])
})

test('your copy takes the same song’s place, in the same order, repeats and all', () => {
  const owned = new Map([['b', 'file-b']])
  const rows = hybridRows(
    [track('a'), track('b'), track('c'), track('b')],
    true,
    (t) => owned.get(t.track_id),
    (mine) => `LOCAL:${mine}`,
    (t) => `ONLINE:${t.track_id}`,
  )
  assert.deepEqual(rows, ['ONLINE:a', 'LOCAL:file-b', 'ONLINE:c', 'LOCAL:file-b'])
  // Switched off: everything streams, as YouTube Music's own app plays it.
  const off = hybridRows([track('a'), track('b')], false, (t) => owned.get(t.track_id), (m) => `LOCAL:${m}`, (t) => `ONLINE:${t.track_id}`)
  assert.deepEqual(off, ['ONLINE:a', 'ONLINE:b'])
})

const memoryStore = () => {
  const m = new Map<string, string>()
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v), removeItem: (k: string) => void m.delete(k) }
}

test('the session is kept with the queue it is for, across a restart', () => {
  const store = memoryStore()
  const s: RadioSession = { ...radioSession('{"kind":"radio"}', first), loading: true }
  saveSession(() => store, s)
  const back = loadSession(() => store, '{"kind":"radio"}')
  assert.ok(back)
  assert.equal(back.continuation, 'TOKEN2')
  assert.equal(back.loading, false, 'nothing was being asked after a restart')
  assert.equal(loadSession(() => store, 'another queue'), null)
  assert.equal(loadSession(() => null, '{"kind":"radio"}'), null)
})

test('a file’s match in the catalog is remembered; a miss is not', () => {
  const store = memoryStore()
  const m = matchMemory(() => store)
  assert.equal(m.get('C:/Music/a.flac'), undefined)
  m.set('C:/Music/a.flac', 'vid1')
  assert.equal(matchMemory(() => store).get('C:/Music/a.flac'), 'vid1')
})
