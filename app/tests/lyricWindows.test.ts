// node --test tests/   (Node runs the TypeScript as is)
// Each lyrics window keeps its own settings; the desktop lyrics keep where they were and their lock.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { isBackground, readWindow, WINDOW_DEFAULTS } from '../src/lib/lyricWindowSettings.ts'

const from = (kept: Record<string, string>) => (key: string) => kept[key] ?? null

test('nothing kept: each window as it always was', () => {
  assert.deepEqual(readWindow('stage', from({})), WINDOW_DEFAULTS.stage)
  assert.deepEqual(readWindow('overlay', from({})), WINDOW_DEFAULTS.overlay)
  assert.equal(WINDOW_DEFAULTS.stage.background, 'cover')
  assert.equal(WINDOW_DEFAULTS.overlay.background, 'clear')
})

test('the desktop lyrics keep the place and lock they had before', () => {
  const kept = from({ 'desktop-lyrics-bounds': '[100,800,900,160]', 'desktop-lyrics-locked': 'false' })
  const o = readWindow('overlay', kept)
  assert.deepEqual(o.bounds, [100, 800, 900, 160])
  assert.equal(o.locked, false)
  // Not the big screen's.
  assert.equal(readWindow('stage', kept).bounds, null)
})

test('a window keeps its own settings, over what it had before', () => {
  const kept = from({
    'desktop-lyrics-bounds': '[1,2,300,100]',
    'lyric-window-overlay': JSON.stringify({ background: 'style:neo-tokyo', bounds: [5, 6, 700, 200], locked: true, onTop: false }),
    'lyric-window-stage': JSON.stringify({ background: 'pack:my-theme', full: false, monitor: 'DELL' }),
  })
  const o = readWindow('overlay', kept)
  assert.equal(o.background, 'style:neo-tokyo')
  assert.deepEqual(o.bounds, [5, 6, 700, 200])
  assert.equal(o.onTop, false)
  const s = readWindow('stage', kept)
  assert.equal(s.background, 'pack:my-theme')
  assert.equal(s.full, false)
  assert.equal(s.monitor, 'DELL')
})

test('what is not of its kind is left out, field by field', () => {
  const o = readWindow('overlay', from({ 'lyric-window-overlay': JSON.stringify({ background: 'style:../x', bounds: [1, 2, 'x', 4], full: 'yes', onTop: false }) }))
  assert.equal(o.background, 'clear')
  assert.equal(o.bounds, null)
  assert.equal(o.full, false)
  assert.equal(o.onTop, false, 'what was fine is kept')
  assert.deepEqual(readWindow('stage', from({ 'lyric-window-stage': '{not json' })), WINDOW_DEFAULTS.stage)
})

test('backgrounds: clear, cover, Venuta, a theme or one of yours', () => {
  for (const b of ['clear', 'cover', 'venuta', 'style:layer-zero', 'pack:Rainy night']) assert.equal(isBackground(b), true, b)
  for (const b of ['', 'theme', 'style:', 'style:Bad Name', 'pack:', 3, null]) assert.equal(isBackground(b), false, String(b))
})
