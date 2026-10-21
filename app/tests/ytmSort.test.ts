// node --test tests/   (Node runs the TypeScript as is)
// Sorting your YouTube Music library lists (lib/ytmSort.ts), as its own app sorts them.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { SORTS, sortedBy } from '../src/lib/ytmSort.ts'

const items = ['banjo', 'Ápple', 'cherry', 'apple pie']

test('recently added is the order YouTube Music gave', () => {
  assert.deepEqual(sortedBy(items, 'added', (s) => s), items)
})

test('A to Z and Z to A go by the name, accents and case aside', () => {
  assert.deepEqual(sortedBy(items, 'az', (s) => s), ['Ápple', 'apple pie', 'banjo', 'cherry'])
  assert.deepEqual(sortedBy(items, 'za', (s) => s), ['cherry', 'banjo', 'apple pie', 'Ápple'])
})

test('sorting never changes the list it is given', () => {
  const copy = [...items]
  sortedBy(items, 'az', (s) => s)
  assert.deepEqual(items, copy)
})

test('the choices are named plainly', () => {
  assert.deepEqual(SORTS.map(([, l]) => l), ['Recently added', 'A to Z', 'Z to A'])
})
