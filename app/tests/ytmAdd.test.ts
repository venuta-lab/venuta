// node --test tests/   (Node runs the TypeScript as is)
// "Add to playlist" on YouTube Music (lib/ytmAdd.ts): only to a playlist you can change, and a
// song already there is left out instead of failing the whole add.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { addPlan, canAddTo } from '../src/lib/ytmAdd.ts'

const there = (owned: boolean, collaborative: boolean, ids: string[], complete = true) => ({
  title: 'Road',
  owned,
  collaborative,
  complete,
  tracks: ids.map((track_id) => ({ track_id })),
})

test('only a playlist you made or collaborate on takes songs', () => {
  assert.equal(canAddTo(there(true, false, [])), true)
  assert.equal(canAddTo(there(false, true, [])), true)
  assert.equal(canAddTo(there(false, false, [])), false)
})

test('someone else’s playlist is refused in words, and nothing is sent', () => {
  const p = addPlan(['a'], there(false, false, []))
  assert.equal(p.send.length, 0)
  assert.match(p.words, /someone else’s playlist/)
})

test('songs already there are left out, and said', () => {
  const p = addPlan(['a', 'b', 'c', 'b'], there(true, false, ['b']))
  assert.deepEqual(p.send, ['a', 'c'])
  assert.equal(p.words, 'Added 2 songs to “Road” on YouTube Music. 1 was there already.')
})

test('all of them there already sends nothing', () => {
  const p = addPlan(['a', 'b'], there(true, false, ['a', 'b']))
  assert.deepEqual(p.send, [])
  assert.equal(p.words, 'Both are in “Road” on YouTube Music already.')
  assert.equal(addPlan(['a'], there(true, false, ['a'])).words, 'It’s in “Road” on YouTube Music already.')
})

test('one new song is named', () => {
  const p = addPlan(['a'], there(true, false, []), '“Tum Hi Ho”')
  assert.deepEqual(p.send, ['a'])
  assert.equal(p.words, 'Added “Tum Hi Ho” to “Road” on YouTube Music.')
})
