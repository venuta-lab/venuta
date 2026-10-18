// node --test tests/   (Node runs the TypeScript as is)
// Telling when an online song is one you have (lib/owned.ts): it then shows once, as yours.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { ownedIndex, titleCore } from '../src/lib/owned.ts'

const mine = [
  { path: 'a/tum.flac', title: 'Tum Hi Ho', artists: ['Arijit Singh'], duration_ms: 262_000 },
  { path: 'b/kesariya.flac', title: 'केसरिया', artists: ['अरिजीत सिंह'], duration_ms: 268_000 },
  { path: 'c/so-what.flac', title: 'So What (Remastered 2009)', artists: ['Miles Davis'], duration_ms: 545_000 },
]

test('a title core leaves out film credits, features and remaster notes', () => {
  assert.equal(titleCore('Kesariya (From "Brahmastra")'), titleCore('Kesariya'))
  assert.equal(titleCore('Shape of You (feat. Someone)'), titleCore('Shape of You'))
  assert.equal(titleCore('So What - Remastered 2009'), titleCore('So What'))
})

test('the same song online is found in your library, by title, artist and length', () => {
  const owns = ownedIndex(mine)
  assert.equal(owns({ title: 'Tum Hi Ho', artists: ['Mithoon', 'Arijit Singh'], duration_ms: 261_000 })?.path, 'a/tum.flac')
  assert.equal(owns({ title: 'So What', artists: ['Miles Davis'], duration_ms: 546_000 })?.path, 'c/so-what.flac')
})

test('across scripts: a Roman title finds a Devanagari one', () => {
  const owns = ownedIndex(mine)
  assert.equal(owns({ title: 'Kesariya (From "Brahmastra")', artists: ['Arijit Singh'], duration_ms: 268_500 })?.path, 'b/kesariya.flac')
})

test('another artist, or a version of another length, is not yours', () => {
  const owns = ownedIndex(mine)
  assert.equal(owns({ title: 'Tum Hi Ho', artists: ['Some Cover Band'], duration_ms: 262_000 }), undefined)
  assert.equal(owns({ title: 'Tum Hi Ho', artists: ['Arijit Singh'], duration_ms: 190_000 }), undefined, 'a shorter edit')
  assert.equal(owns({ title: 'Tum Hi Ho', artists: ['Arijit Singh'], duration_ms: null })?.path, 'a/tum.flac', 'no length: title and artist decide')
})
