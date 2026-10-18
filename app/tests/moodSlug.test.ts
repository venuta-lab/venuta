// node --test tests/   (Node runs the TypeScript as is)
// A mood or genre tile finds its picture by its name.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { moodSlug } from '../src/lib/moodSlug.ts'

test('a tile name becomes its picture file name', () => {
  assert.equal(moodSlug('Feel good'), 'feel-good')
  assert.equal(moodSlug('R&B & soul'), 'r-and-b-and-soul')
  assert.equal(moodSlug('Bollywood & Indian'), 'bollywood-and-indian')
  assert.equal(moodSlug('Hip-hop'), 'hip-hop')
  assert.equal(moodSlug('  J-Pop '), 'j-pop')
  assert.equal(moodSlug('Música Brasileira'), 'musica-brasileira', 'accents folded')
})
