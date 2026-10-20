// node --test tests/   (Node runs the TypeScript as is)
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { canPlayNext, chipsShown, learnLength, onlineAlbumOf, placeOf, queueName, queueNote, relatedParts, rowLabel, rowLength, saveName } from '../src/lib/upNext.ts'
import type { RadioView } from '../src/lib/radio.ts'

const view = (more: Partial<RadioView> = {}): RadioView => ({
  playingFrom: 'Kitni Baatein radio',
  title: 'Kitni Baatein Radio',
  chips: [
    { text: 'All', selected: true },
    { text: 'Familiar', selected: false },
    { text: 'Deep cuts', selected: false },
  ],
  loading: false,
  exhausted: null,
  radio: true,
  room: false,
  ...more,
})

test('the queue is named as YouTube Music names its radio, else as it was started', () => {
  assert.equal(queueName(view()), 'Kitni Baatein Radio')
  assert.equal(queueName(view({ title: null })), 'Kitni Baatein radio')
  assert.equal(queueName(view({ title: '  ', playingFrom: 'Taal' })), 'Taal')
  // An album or playlist says where it came from (its own link), not a radio name.
  assert.equal(queueName(view({ radio: false, title: 'Something' })), null)
  assert.equal(queueName(null), null)
})

test('the tuning chips show in YouTube’s order, not in a room and not when there are none', () => {
  assert.deepEqual(
    chipsShown(view()).map((c) => c.text),
    ['All', 'Familiar', 'Deep cuts'],
  )
  assert.deepEqual(chipsShown(view({ room: true })), [])
  assert.deepEqual(chipsShown(view({ chips: [] })), [])
  assert.deepEqual(chipsShown(null), [])
})

test('under the queue: loading while more comes, the end in words when nothing will', () => {
  assert.equal(queueNote(view()), null)
  assert.equal(queueNote(view({ loading: true })), 'Getting more from YouTube Music…')
  assert.equal(queueNote(view({ exhausted: "That's all YouTube Music has for this radio." })), "That's all YouTube Music has for this radio.")
  assert.equal(queueNote(null), null)
})

test('each row is played, playing or to come', () => {
  assert.deepEqual(
    [0, 1, 2, 3].map((at) => placeOf(at, 1)),
    ['played', 'current', 'next', 'next'],
  )
  // Play next is for a song that isn't playing or next already.
  assert.deepEqual(
    [0, 1, 2, 3].map((at) => canPlayNext(at, 1)),
    [true, false, false, true],
  )
})

test('a row says what it is to a screen reader', () => {
  assert.equal(rowLabel({ title: 'Maand', artists: ['Bayaan'], length: '3:45', where: 'Online', place: 'next' }), 'Maand, Bayaan, 3:45, Online')
  assert.equal(rowLabel({ title: 'Ravi', artists: [], length: '', where: 'Local', place: 'current' }), 'Ravi, Local, playing now')
  assert.equal(rowLabel({ title: 'Bewajah', artists: ['A', 'B'], length: '4:01', where: null, place: 'played' }), 'Bewajah, A, B, 4:01, played')
})

test('a row’s length: as listed, else as the song said when it opened, else nothing (never a dash)', () => {
  // Listed with the song (a radio's lengthText, a playlist's column).
  assert.equal(rowLength(225_000, undefined, 0), '3:45')
  // Listed without one: the length it had when it was opened, then the player's while it plays.
  assert.equal(rowLength(0, 241_000, 0), '4:01')
  assert.equal(rowLength(0, undefined, 199_000), '3:19')
  // Truly not known: nothing at all.
  assert.equal(rowLength(0, undefined, 0), '')
})

test('a song opened says its length for its row from then on', () => {
  const lengths = new Map<string, number>()
  learnLength(lengths, 'online:remote_catalog:aaaaaaaaaaa', 225_000)
  assert.equal(lengths.get('online:remote_catalog:aaaaaaaaaaa'), 225_000)
  // Nothing known yet, or no song: nothing kept.
  learnLength(lengths, 'online:remote_catalog:bbbbbbbbbbb', 0)
  learnLength(lengths, null, 180_000)
  assert.equal(lengths.size, 1)
  // Only so many are kept: the oldest go first.
  for (let i = 0; i < 3000; i++) learnLength(lengths, `k${i}`, 1000)
  assert.ok(lengths.size <= 2000)
  assert.equal(lengths.get('k2999'), 1000)
})

test('saving the queue as a playlist suggests the radio’s name', () => {
  assert.equal(saveName(view(), 'Kitni Baatein radio'), 'Kitni Baatein Radio')
  assert.equal(saveName(view({ title: null }), 'Kitni Baatein radio'), 'Kitni Baatein radio')
  assert.equal(saveName(null, 'Online: Heartbeat Diaries'), 'Heartbeat Diaries')
  assert.equal(saveName(null, ''), '')
})

test('an online song in the queue goes to its online album, when YouTube named one', () => {
  const online = { path: '', title: 'Maand', artists: ['Bayaan'], album: 'Maand', album_id: 'MPREmaand', artwork_url: 'https://lh3.googleusercontent.com/x', online: { kind: 'online' as const, provider: 'remote_catalog', track_id: 'aaaaaaaaaaa' } }
  assert.deepEqual(onlineAlbumOf(online), {
    provider: 'remote_catalog',
    album_id: 'MPREmaand',
    title: 'Maand',
    artists: ['Bayaan'],
    track_count: null,
    release_date: null,
    artwork_url: 'https://lh3.googleusercontent.com/x',
  })
  // No album named: nothing to go to. A file of yours goes to its folder instead.
  assert.equal(onlineAlbumOf({ ...online, album_id: undefined }), null)
  assert.equal(onlineAlbumOf({ ...online, album_id: '  ' }), null)
  assert.equal(onlineAlbumOf({ ...online, path: 'Bayaan/Maand.flac' }), null)
  // A song a room's host plays from their file has no album here.
  assert.equal(onlineAlbumOf({ ...online, online: { kind: 'online', provider: 'venuta_room', track_id: 'x' } }), null)
  // The album's name, else the song's, for the page while it loads.
  assert.equal(onlineAlbumOf({ ...online, album: '' })?.title, 'Maand')
})

test('Related shows each shelf as what it holds, in YouTube’s order', () => {
  const track = { provider: 'remote_catalog', track_id: 'a', title: 'A', artists: [], album: null, album_id: null, duration_ms: null, artwork_url: null, isrc: null }
  const artist = { kind: 'artist' as const, artist: { provider: 'remote_catalog', artist_id: 'UC1', name: 'One', artwork_url: null }, subtitle: null }
  const parts = relatedParts([
    { title: 'You might also like', tracks: [track], items: [], about: null },
    { title: 'Similar artists', tracks: [], items: [artist], about: null },
    { title: 'About the artist', tracks: [], items: [], about: '  Words.  ' },
    { title: 'Empty', tracks: [], items: [], about: '  ' },
  ])
  assert.deepEqual(
    parts.map((p) => [p.kind, p.title]),
    [
      ['songs', 'You might also like'],
      ['cards', 'Similar artists'],
      ['about', 'About the artist'],
    ],
  )
  assert.equal(parts[2].kind === 'about' && parts[2].about, 'Words.')
})
