// node --test tests/   (Node runs the TypeScript as is)
// An online song's own YouTube Music menu (lib/ytmSongMenu.ts): which token a press sends, how the
// song stands after it, and what the menu offers next, all from what the row came with.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { SongMenus, songMenuItems, togglePress, type TrackMenu } from '../src/lib/ytmSongMenu.ts'

const saved = (): TrackMenu => ({
  library: { on: true, on_token: 'ADD', off_token: 'REMOVE' },
  pin: { on: false, on_token: 'PIN', off_token: 'UNPIN' },
  feedback: [],
})

test('a press sends the way the song does not stand now', () => {
  assert.deepEqual(togglePress({ on: true, on_token: 'ADD', off_token: 'REMOVE' }), { token: 'REMOVE', next: false })
  assert.deepEqual(togglePress({ on: false, on_token: 'ADD', off_token: 'REMOVE' }), { token: 'ADD', next: true })
})

test('a way the row did not carry cannot be pressed', () => {
  assert.equal(togglePress({ on: true, on_token: 'ADD', off_token: null }), null)
  assert.equal(togglePress({ on: false, on_token: null }), null)
})

test('the menu says each press the way it would go', () => {
  const items = songMenuItems(saved())
  assert.deepEqual(
    items.map((i) => [i.press, i.label, i.token]),
    [
      ['library', 'Remove from YouTube Music library', 'REMOVE'],
      ['pin', 'Pin to Listen again', 'PIN'],
    ],
  )
  const other = songMenuItems({ library: { on: false, on_token: 'ADD', off_token: 'REMOVE' }, pin: { on: true, on_token: 'PIN', off_token: 'UNPIN' } })
  assert.deepEqual(
    other.map((i) => i.label),
    ['Save to YouTube Music library', 'Unpin from Listen again'],
  )
})

test('not interested is offered in YouTube Music’s own words', () => {
  const items = songMenuItems({ feedback: [{ icon: 'NOT_INTERESTED', label: 'Not interested', token: 'NI' }] })
  assert.deepEqual(items, [{ press: 'feedback', label: 'Not interested', token: 'NI', next: true }])
})

test('a song with no menu offers nothing', () => {
  assert.deepEqual(songMenuItems(null), [])
  assert.deepEqual(songMenuItems({}), [])
})

test('after a press, the song stands the new way wherever it shows', () => {
  const m = new SongMenus()
  m.remember('k', saved())
  m.pressed('k', 'library', false)
  assert.equal(m.menuOf('k')?.library?.on, false)
  // The same listing again (the page it was pressed on): the press holds.
  assert.equal(m.menuOf('k', saved())?.library?.on, false)
  assert.deepEqual(
    songMenuItems(m.menuOf('k')).map((i) => [i.label, i.token]),
    [
      ['Save to YouTube Music library', 'ADD'],
      ['Pin to Listen again', 'PIN'],
    ],
  )
})

test('a press holds over a listing from before it, with the newest listing’s tokens', () => {
  const m = new SongMenus()
  m.remember('k', { ...saved(), listed_at: 100 })
  m.pressed('k', 'library', false, 200)
  // The same song on another shelf, listed before the press with tokens of its own: either
  // listing's tokens do the same, and sending one twice leaves things as once did.
  const elsewhere = { ...saved(), library: { on: true, on_token: 'ADD-2', off_token: 'REMOVE-2' }, listed_at: 150 }
  assert.equal(m.menuOf('k', elsewhere)?.library?.on, false)
  assert.equal(m.menuOf('k')?.library?.on_token, 'ADD-2')
})

test('a song listed after a press is told as that listing says', () => {
  const m = new SongMenus()
  m.remember('k', { ...saved(), listed_at: 100 })
  m.pressed('k', 'pin', true, 200)
  assert.equal(m.menuOf('k')?.pin?.on, true)
  // Unpinned on the phone since, and listed again here after the press: the listing wins.
  const later = { ...saved(), pin: { on: false, on_token: 'PIN-3', off_token: 'UNPIN-3' }, listed_at: 300 }
  assert.equal(m.menuOf('k', later)?.pin?.on, false)
  assert.equal(m.menuOf('k')?.pin?.on_token, 'PIN-3')
})

test('an older listing shown again never takes the place of a newer one', () => {
  const m = new SongMenus()
  m.remember('k', { ...saved(), pin: { on: true, on_token: 'PIN', off_token: 'UNPIN' }, listed_at: 500 })
  m.remember('k', { ...saved(), listed_at: 100 })
  assert.equal(m.menuOf('k')?.pin?.on, true)
})

test('not interested, once sent, is not offered again for that song', () => {
  const m = new SongMenus()
  m.remember('k', { feedback: [{ icon: 'NOT_INTERESTED', label: 'Not interested', token: 'NI' }] })
  m.sent('k', 'NI')
  assert.deepEqual(songMenuItems(m.menuOf('k')), [])
})

test('a song never listed with a menu has none, and the oldest are let go past the limit', () => {
  const m = new SongMenus(2)
  assert.equal(m.menuOf('nothing'), null)
  m.remember('a', saved())
  m.remember('b', saved())
  m.remember('c', saved())
  assert.equal(m.menuOf('a'), null)
  assert.ok(m.menuOf('c'))
})
