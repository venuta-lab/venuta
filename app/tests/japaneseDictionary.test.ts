// node --test tests/   (Node runs the TypeScript as is)
// The Japanese dictionary on its way: said quietly, and nothing once it is ready.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { dictionaryWords } from '../src/lib/japaneseDictionary.ts'

test('what the views say while the Japanese dictionary comes', () => {
  assert.equal(dictionaryWords(null), null)
  assert.equal(dictionaryWords({ state: 'downloading', done: 25, total: 100 }), 'Getting the Japanese dictionary for pronunciation, 25%')
  assert.equal(dictionaryWords({ state: 'downloading', done: 25, total: 0 }), 'Getting the Japanese dictionary for pronunciation', 'no size known: no share')
  assert.equal(dictionaryWords({ state: 'unpacking', done: 0, total: 0 }), 'Setting up the Japanese dictionary for pronunciation')
  assert.equal(dictionaryWords({ state: 'checking', done: 0, total: 0 }), 'Setting up the Japanese dictionary for pronunciation')
  assert.equal(dictionaryWords({ state: 'ready', done: 0, total: 0 }), null)
  assert.match(dictionaryWords({ state: 'failed', done: 0, total: 0, error: 'offline' }) ?? '', /tried again with the next Japanese song/)
})
