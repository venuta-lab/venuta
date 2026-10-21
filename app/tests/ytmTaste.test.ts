// node --test tests/   (Node runs the TypeScript as is)
// Choose artists you like (lib/ytmTaste.ts): what YouTube Music is told on Done.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { firstChosen, tasteAnswer, toggleChoice } from '../src/lib/ytmTaste.ts'

const artists = [
  { name: 'Arijit Singh', selection_value: 'S1', impression_value: 'I1', picture: null, selected: false },
  { name: 'Shreya Ghoshal', selection_value: 'S2', impression_value: 'I2', picture: null, selected: true },
  { name: 'A.R. Rahman', selection_value: 'S3', impression_value: 'I3', picture: null, selected: false },
]

test('every artist shown is told as seen, the chosen ones as chosen, in their order', () => {
  assert.deepEqual(tasteAnswer(artists, new Set(['S3', 'S1'])), { impressions: ['I1', 'I2', 'I3'], selected: ['S1', 'S3'] })
})

test('the ones YouTube Music says are chosen start chosen', () => {
  assert.deepEqual([...firstChosen(artists)], ['S2'])
})

test('a press chooses an artist, and another takes it back', () => {
  const once = toggleChoice(new Set<string>(), 'S1')
  assert.deepEqual([...once], ['S1'])
  assert.deepEqual([...toggleChoice(once, 'S1')], [])
})

test('nothing chosen is nothing to tell', () => {
  assert.deepEqual(tasteAnswer(artists, new Set()).selected, [])
})
