// node --test tests/   (Node runs the TypeScript as is)
// A typed playlist name is shown as the engine will save it (the same table the engine tests).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { nameNote, savedName } from '../src/lib/playlistName.ts'

const { cases } = JSON.parse(readFileSync(new URL('./fixtures/playlist-names.json', import.meta.url), 'utf8')) as {
  cases: [string, string][]
}

test('every name is saved as the engine saves it', () => {
  for (const [typed, saved] of cases) assert.equal(savedName(typed), saved, JSON.stringify(typed))
})

test('a name saved as typed needs no note', () => {
  assert.equal(nameNote('Road trip'), null)
  assert.equal(nameNote('  Road trip  '), null)
})

test('a changed name says what it is saved as, and why', () => {
  assert.equal(nameNote('Road/trip: best* of?'), '“Road trip best of”'.replace(/^/, 'Saved as ') + ": / : * ? can't be in a name.")
  assert.match(nameNote('L'.repeat(150)) ?? '', /at most 100 letters/)
  assert.match(nameNote('CON') ?? '', /Windows keeps that name/)
  assert.equal(nameNote('***'), 'A name needs at least one letter or number.')
})
