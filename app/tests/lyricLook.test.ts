// node --test tests/   (Node runs the TypeScript as is)
// Each lyrics window keeps its own look; the look they shared before is where both begin.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readLook } from '../src/lib/lyricLook.ts'

const from = (kept: Record<string, string>) => (key: string) => kept[key] ?? null

test('each window keeps its own look', () => {
  const kept = from({
    'lyric-look-stage': JSON.stringify({ look: 'glow', size: 'xl' }),
    'lyric-look-overlay': JSON.stringify({ look: 'outline', size: 's', lines: 1 }),
  })
  assert.equal(readLook('stage', kept).look, 'glow')
  assert.equal(readLook('stage', kept).size, 'xl')
  assert.equal(readLook('overlay', kept).look, 'outline')
  assert.equal(readLook('overlay', kept).lines, 1)
})

test('the look they shared before is where both begin', () => {
  const kept = from({ 'lyric-look': JSON.stringify({ look: 'serif', colour: 'white' }) })
  for (const w of ['stage', 'overlay'] as const) {
    assert.equal(readLook(w, kept).look, 'serif')
    assert.equal(readLook(w, kept).colour, 'white')
    assert.equal(readLook(w, kept).size, 'm', 'the rest from the default')
  }
})

test('nothing kept, or something unreadable: the default', () => {
  assert.equal(readLook('stage', from({})).look, 'karaoke')
  assert.equal(readLook('overlay', from({ 'lyric-look-overlay': '{nope', 'lyric-look': JSON.stringify({ look: 'glow' }) })).look, 'glow')
})
