// node --test tests/   (Node runs the TypeScript as is)
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { RECENT_KEEP, RECENT_KEY, recentSearchStore, type KeyValueStore } from '../src/lib/recentSearches.ts'

function memory(): KeyValueStore & { data: Map<string, string> } {
  const data = new Map<string, string>()
  return { data, getItem: (k) => data.get(k) ?? null, setItem: (k, v) => void data.set(k, v), removeItem: (k) => void data.delete(k) }
}

test('newest first, once each (case and spacing ignored), kept in the store', () => {
  const store = memory()
  let t = 0
  const r = recentSearchStore(() => store, () => ++t)
  r.push('wonderwall')
  r.push('  Kesariya  ')
  r.push('WONDERWALL')
  assert.deepEqual(r.list().map((x) => x.q), ['WONDERWALL', 'Kesariya'])
  assert.ok(store.data.get(RECENT_KEY)?.includes('Kesariya'))
  // A fresh reader sees the same (it lives in the store, nowhere else).
  assert.deepEqual(recentSearchStore(() => store).list().map((x) => x.q), ['WONDERWALL', 'Kesariya'])
  // One letter, or nothing, is no search.
  r.push('a')
  r.push('   ')
  assert.equal(r.list().length, 2)
})

test('at most the limit, the oldest dropped', () => {
  const r = recentSearchStore(() => memory())
  for (let i = 0; i < RECENT_KEEP + 5; i++) r.push(`song ${i}`)
  const list = r.list()
  assert.equal(list.length, RECENT_KEEP)
  assert.equal(list[0].q, `song ${RECENT_KEEP + 4}`)
  assert.equal(list.at(-1)?.q, 'song 5')
})

test('remove one, clear all; listeners hear each change; a broken store is survived', () => {
  const store = memory()
  const r = recentSearchStore(() => store)
  let heard = 0
  const stop = r.subscribe(() => heard++)
  r.push('wonderwall')
  r.push('kesariya')
  r.remove('Wonderwall')
  assert.deepEqual(r.list().map((x) => x.q), ['kesariya'])
  r.clear()
  assert.deepEqual(r.list(), [])
  assert.equal(store.data.has(RECENT_KEY), false)
  assert.equal(heard, 4)
  stop()
  r.push('tum hi ho')
  assert.equal(heard, 4)

  // The list is the same array until it changes (React's store hook relies on it).
  assert.equal(r.list(), r.list())

  const junk = memory()
  junk.setItem(RECENT_KEY, '{not json')
  assert.deepEqual(recentSearchStore(() => junk).list(), [])
  const broken = recentSearchStore(() => ({
    getItem: () => null,
    setItem: () => {
      throw new Error('quota')
    },
    removeItem: () => {},
  }))
  broken.push('still here')
  assert.equal(broken.list()[0].q, 'still here')
})
