// node --test tests/   (Node runs the TypeScript as is)
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { STYLES, styleInfo } from '../src/lib/themes.ts'

test('a theme Venuta does not know is Lumen, never another theme', () => {
  for (const id of ['', 'midnight-sakura', 'VENUTA', 'undefined']) assert.equal(styleInfo(id).id, 'venuta')
  assert.equal(STYLES[0].id, 'venuta')
})

test('every theme says what it does, so no code has to ask its name', () => {
  for (const s of STYLES) {
    assert.ok(['yours', 'cover', 'blocks', 'own'].includes(s.accent), s.id)
    assert.ok(['glass', 'themed'].includes(s.miniPlayer), s.id)
    assert.equal(typeof s.surfaces, 'boolean', s.id)
    assert.equal(typeof s.albumBackground, 'boolean', s.id)
    assert.equal(typeof s.lcd, 'boolean', s.id)
    assert.ok(s.seekBar && s.fx, s.id)
    // The waveform and glass mini player belong with the surfaces Lumen offers.
    assert.equal(s.seekBar === 'wave', s.miniPlayer === 'glass', s.id)
    if (s.modeNames) assert.ok(s.modes, `${s.id} names modes it doesn't have`)
  }
  assert.equal(new Set(STYLES.map((s) => s.id)).size, STYLES.length)
})
