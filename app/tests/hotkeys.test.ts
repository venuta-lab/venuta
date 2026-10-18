// node --test tests/   (Node runs the TypeScript as is)
// Shortcuts from anywhere: what a key press makes, and how it reads.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { keysOf, prettyKeys } from '../src/lib/hotkeyKeys.ts'

const press = (code: string, mods: { ctrl?: boolean; alt?: boolean; shift?: boolean; meta?: boolean } = {}) => ({
  code,
  ctrlKey: !!mods.ctrl,
  altKey: !!mods.alt,
  shiftKey: !!mods.shift,
  metaKey: !!mods.meta,
})

test('a key press is written the way Windows registers it', () => {
  assert.equal(keysOf(press('KeyP', { ctrl: true, alt: true })), 'Ctrl+Alt+KeyP')
  assert.equal(keysOf(press('Space', { ctrl: true, shift: true })), 'Ctrl+Shift+Space')
  assert.equal(keysOf(press('ArrowRight', { alt: true, meta: true })), 'Alt+Super+ArrowRight')
  assert.equal(keysOf(press('ControlLeft', { ctrl: true })), null, 'only a modifier so far')
  assert.equal(keysOf(press('ShiftRight', { ctrl: true, shift: true })), null)
})

test('keys read plainly in Settings', () => {
  assert.equal(prettyKeys('Ctrl+Alt+KeyP'), 'Ctrl Alt P')
  assert.equal(prettyKeys('Ctrl+Shift+Digit1'), 'Ctrl Shift 1')
  assert.equal(prettyKeys('Alt+Super+ArrowRight'), 'Alt Win →')
  assert.equal(prettyKeys('MediaPlayPause'), 'Play/Pause')
  assert.equal(prettyKeys(''), 'Off')
})
