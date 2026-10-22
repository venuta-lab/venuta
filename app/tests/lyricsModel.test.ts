// node --test tests/   (Node runs the TypeScript as is)
// The engine's lyrics (start and end, words, second lines) as the views draw them.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { shownLines } from '../src/lib/lyricsModel.ts'
import type { FoundLine } from '../src/lib/engine.ts'

const lines: FoundLine[] = [
  {
    start_ms: 1000,
    end_ms: 4000,
    text: '無敵の笑顔で',
    script: 'Jpan',
    words: [
      { start_ms: 1000, end_ms: 1500, text: '無敵の' },
      { start_ms: 1500, end_ms: 2000, text: '笑顔で' },
    ],
    roman: { text: 'muteki no egao de', lang: 'ja-Latn', source: { kind: 'lyrics', by: 'netease' } },
    translation: { text: '露出无敌笑容', lang: 'zh-Hans', source: { kind: 'lyrics', by: 'netease' } },
  },
  { start_ms: 4000, end_ms: 9000, text: 'You are my savior', script: 'Latn' },
]

test('lines, words and second lines stay in step', () => {
  const s = shownLines(lines)
  assert.deepEqual(s.lines, [
    [1000, '無敵の笑顔で'],
    [4000, 'You are my savior'],
  ])
  assert.deepEqual(s.words, [
    [
      [1000, 1500, '無敵の'],
      [1500, 2000, '笑顔で'],
    ],
    [],
  ])
  assert.deepEqual(s.translations, ['露出无敌笑容', null])
  assert.deepEqual(s.romanized, ['muteki no egao de', null])
})

test('your offset moves every time, never before the start', () => {
  const s = shownLines(lines, -1500)
  assert.deepEqual(
    s.lines.map(([ms]) => ms),
    [0, 2500],
  )
  assert.deepEqual(s.words?.[0][1], [0, 500, '笑顔で'])
})

test('lines with nothing beside them carry nothing beside them', () => {
  const s = shownLines([{ start_ms: 0, end_ms: 5000, text: 'Plain', script: 'Latn' }])
  assert.equal(s.words, undefined)
  assert.equal(s.translations, undefined)
  assert.equal(s.romanized, undefined)
})
