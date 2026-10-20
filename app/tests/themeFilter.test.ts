// node --test tests/   (Node runs the TypeScript as is)
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { filterThemes, MODE_FILTERS, modesWord, SOURCES, themeHint, type Filterable } from '../src/lib/themeFilter.ts'

const T = (key: string, kind: Filterable['kind'], modes: Filterable['modes'], usable = true): Filterable => ({ key, kind, modes, usable })
const lumen = T('style:venuta', 'built', ['dark', 'light'])
const green = T('style:green-room', 'built', ['dark'])
const orchard = T('style:orchard', 'built', ['dark', 'light'])
const dusk = T('pack:dusk', 'yours', ['dark'])
const paper = T('pack:paper', 'yours', ['light'])
const broken = T('pack:broken', 'yours', ['dark'], false)
const list = [lumen, green, orchard, dusk, paper, broken]
const keys = (l: Filterable[]) => l.map((e) => e.key)

test('the filters are the four sources and the three modes, in that order', () => {
  assert.deepEqual(
    SOURCES.map((s) => s.label),
    ['All', 'Built-in', 'Created', 'Recent'],
  )
  assert.deepEqual(
    MODE_FILTERS.map((m) => m.label),
    ['All', 'Dark', 'Light'],
  )
})

test('All and All is every theme, in the order given (built in first)', () => {
  assert.deepEqual(keys(filterThemes(list, { source: 'all', mode: 'all' }, [])), keys(list))
})

test('Built-in and Created split by where the theme comes from', () => {
  assert.deepEqual(keys(filterThemes(list, { source: 'built', mode: 'all' }, [])), ['style:venuta', 'style:green-room', 'style:orchard'])
  // A theme of yours that cannot load is still listed, so it can be removed or shown.
  assert.deepEqual(keys(filterThemes(list, { source: 'created', mode: 'all' }, [])), ['pack:dusk', 'pack:paper', 'pack:broken'])
})

test('Recent is newest first, and leaves out themes that are gone or cannot be used', () => {
  const recent = ['pack:paper', 'style:venuta', 'pack:deleted', 'pack:broken', 'style:venuta', 'style:green-room']
  assert.deepEqual(keys(filterThemes(list, { source: 'recent', mode: 'all' }, recent)), ['pack:paper', 'style:venuta', 'style:green-room'])
  assert.deepEqual(filterThemes(list, { source: 'recent', mode: 'all' }, []), [])
})

test('Dark and Light keep the themes that have that mode, with any source', () => {
  assert.deepEqual(keys(filterThemes(list, { source: 'all', mode: 'light' }, [])), ['style:venuta', 'style:orchard', 'pack:paper'])
  assert.deepEqual(keys(filterThemes(list, { source: 'all', mode: 'dark' }, [])), ['style:venuta', 'style:green-room', 'style:orchard', 'pack:dusk', 'pack:broken'])
  assert.deepEqual(keys(filterThemes(list, { source: 'created', mode: 'light' }, [])), ['pack:paper'])
  assert.deepEqual(keys(filterThemes(list, { source: 'recent', mode: 'dark' }, ['pack:paper', 'pack:dusk', 'style:venuta'])), ['pack:dusk', 'style:venuta'])
})

test('a theme says its modes in a word', () => {
  assert.equal(modesWord(['dark', 'light']), 'light and dark')
  assert.equal(modesWord(['light', 'dark']), 'light and dark')
  assert.equal(modesWord(['dark']), 'dark')
  assert.equal(modesWord(['light']), 'light')
})

test("a card's hover line: what the theme is, then where it comes from and its modes", () => {
  assert.equal(
    themeHint({ line: 'Light through glass.', kind: 'built', modes: ['dark', 'light'] }),
    'Light through glass.\nBuilt in, light and dark',
  )
  assert.equal(themeHint({ line: 'Night blues.', kind: 'yours', base: 'Lumen', modes: ['dark'] }), 'Night blues.\nYours, built on Lumen, dark')
  assert.equal(themeHint({ line: '', kind: 'venuta', base: 'Orchard', modes: ['light'] }), 'From Venuta, built on Orchard, light')
})
