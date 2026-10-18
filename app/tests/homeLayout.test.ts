// node --test tests/   (Node runs the TypeScript as is)
// Home's sections in your order, each on or off: what Edit Home keeps.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { arrange, move, toggle, type HomeLayout } from '../src/lib/homeLayout.ts'

const known = ['hero', 'jump', 'quick', 'online']

test('nothing saved: every section, in its own order, on', () => {
  assert.deepEqual(arrange({ order: [], hidden: [] }, known), known.map((id) => ({ id, on: true })))
})

test('your order first; a section added since keeps its place after the one before it', () => {
  const saved: HomeLayout = { order: ['online', 'hero', 'quick'], hidden: ['quick'] }
  assert.deepEqual(arrange(saved, known), [
    { id: 'online', on: true },
    { id: 'hero', on: true },
    { id: 'jump', on: true },
    { id: 'quick', on: false },
  ])
})

test('a section that no longer exists is left out', () => {
  assert.deepEqual(arrange({ order: ['gone', 'jump'], hidden: ['gone'] }, ['jump']), [{ id: 'jump', on: true }])
})

test('moving and switching keep the rest as they are', () => {
  const l: HomeLayout = { order: [], hidden: [] }
  const up = move(l, known, 'quick', -1)
  assert.deepEqual(up.order, ['hero', 'quick', 'jump', 'online'])
  assert.deepEqual(move(up, known, 'hero', -1).order, up.order, 'the first stays first')
  const off = toggle(up, 'jump')
  assert.deepEqual(off.hidden, ['jump'])
  assert.deepEqual(toggle(off, 'jump').hidden, [])
})
