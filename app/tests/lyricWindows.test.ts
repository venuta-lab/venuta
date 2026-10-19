// node --test tests/   (Node runs the TypeScript as is)
// Each lyrics window keeps its own background, theme and place; what was kept before carries over.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { isBackground, isTheme, readWindow, seeThrough, WINDOW_DEFAULTS } from '../src/lib/lyricWindowSettings.ts'

const from = (kept: Record<string, string>) => (key: string) => kept[key] ?? null

test('nothing kept: both windows look like Venuta (its theme and its own scene)', () => {
  assert.deepEqual(readWindow('stage', from({})), WINDOW_DEFAULTS.stage)
  assert.deepEqual(readWindow('overlay', from({})), WINDOW_DEFAULTS.overlay)
  assert.equal(WINDOW_DEFAULTS.stage.background, 'theme')
  assert.equal(WINDOW_DEFAULTS.overlay.background, 'theme')
  assert.equal(WINDOW_DEFAULTS.stage.theme, 'venuta')
  assert.equal(WINDOW_DEFAULTS.overlay.theme, 'venuta')
})

test('the desktop lyrics keep the lock they had before; a place kept by each screen\'s own scale starts fresh', () => {
  const kept = from({ 'desktop-lyrics-bounds': '[100,800,900,160]', 'desktop-lyrics-locked': 'false' })
  const o = readWindow('overlay', kept)
  assert.equal(o.px, null, 'such a place lands somewhere else on a screen of another scale')
  assert.equal(o.locked, false)
  assert.equal(readWindow('stage', kept).px, null, 'not the big screen\'s')
  const old = readWindow('overlay', from({ 'lyric-window-overlay': JSON.stringify({ bounds: [5, 6, 700, 200] }) }))
  assert.equal(old.px, null)
})

test('a window keeps its own background, theme and place', () => {
  const kept = from({
    'lyric-window-overlay': JSON.stringify({ background: 'frosted', theme: 'style:neo-tokyo', px: [-1900, 6, 1150, 250], locked: true, onTop: false }),
    'lyric-window-stage': JSON.stringify({ background: 'glass', theme: 'pack:my-theme', monitor: 'DELL' }),
  })
  const o = readWindow('overlay', kept)
  assert.equal(o.background, 'frosted')
  assert.equal(o.theme, 'style:neo-tokyo')
  assert.deepEqual(o.px, [-1900, 6, 1150, 250], 'screen pixels, a screen left of the main one too')
  assert.equal(o.onTop, false)
  const s = readWindow('stage', kept)
  assert.equal(s.background, 'glass')
  assert.equal(s.theme, 'pack:my-theme')
  assert.equal(s.monitor, 'DELL')
})

test('a theme kept as the background (when they were one choice) becomes that theme\'s scene', () => {
  const s = readWindow('stage', from({ 'lyric-window-stage': JSON.stringify({ background: 'style:layer-zero' }) }))
  assert.equal(s.background, 'theme')
  assert.equal(s.theme, 'style:layer-zero')
  const v = readWindow('overlay', from({ 'lyric-window-overlay': JSON.stringify({ background: 'venuta' }) }))
  assert.deepEqual([v.background, v.theme], ['theme', 'venuta'])
})

test('the big screen is never locked', () => {
  assert.equal(readWindow('stage', from({ 'lyric-window-stage': JSON.stringify({ locked: true }) })).locked, false)
})

test('what is not of its kind is left out, field by field', () => {
  const o = readWindow('overlay', from({ 'lyric-window-overlay': JSON.stringify({ background: 'smoke', theme: 'style:../x', px: [1, 2, 'x', 4], onTop: false }) }))
  assert.equal(o.background, 'theme')
  assert.equal(o.theme, 'venuta')
  assert.equal(o.px, null)
  const px = (p: unknown) => readWindow('overlay', from({ 'lyric-window-overlay': JSON.stringify({ px: p }) })).px
  assert.equal(px([1.5, 2, 300, 100]), null, 'whole pixels')
  assert.equal(px([1, 2, 0, 100]), null, 'some size')
  assert.equal(o.onTop, false, 'what was fine is kept')
  assert.deepEqual(readWindow('stage', from({ 'lyric-window-stage': '{not json' })), WINDOW_DEFAULTS.stage)
})

test('backgrounds and themes', () => {
  for (const b of ['clear', 'glass', 'frosted', 'cover', 'theme']) assert.equal(isBackground(b), true, b)
  for (const b of ['', 'venuta', 'style:x', 3]) assert.equal(isBackground(b), false, String(b))
  for (const t of ['venuta', 'style:layer-zero', 'pack:Rainy night']) assert.equal(isTheme(t), true, t)
  for (const t of ['', 'clear', 'style:', 'style:Bad Name', 'pack:', null]) assert.equal(isTheme(t), false, String(t))
})

test('a theme ground that lets what is behind show (and needs the window blur)', () => {
  for (const c of ['rgb(11 12 16 / 0.35)', 'rgba(1, 2, 3, 0.4)', 'rgb(0 0 0 / 40%)', '#0000', '#11223380', 'transparent']) assert.equal(seeThrough(c), true, c)
  for (const c of ['#202020', '#f3f3f3', 'rgb(1 2 3)', 'rgba(1, 2, 3, 1)', 'rgb(0 0 0 / 100%)', '#112233ff', '']) assert.equal(seeThrough(c), false, c)
})
