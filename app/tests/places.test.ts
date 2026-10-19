// node --test tests/   (Node runs the TypeScript as is)
// The rail's places: four groups as agreed, the keys that reach them, and how the three music
// places and Stats share one screen.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  RAIL_GROUPS,
  RAIL_BOTTOM,
  PLACE_LABELS,
  PLACE_KEYS,
  placeForKey,
  isMusicPlace,
  musicTopOf,
  placeOfMusicTop,
  screenKey,
  swipeFrom,
} from '../src/lib/places.ts'

test('the rail: listening, then your library at a glance, then managing it, Settings at the bottom', () => {
  assert.deepEqual(RAIL_GROUPS, [
    ['home', 'search', 'library'],
    ['stats', 'health'],
    ['organize', 'devices'],
  ])
  assert.deepEqual(RAIL_BOTTOM, ['settings'])
  assert.equal(PLACE_LABELS.organize, 'Organize')
  assert.equal(PLACE_LABELS.stats, 'Stats')
})

test('Metadata and Activity are not on the rail any more', () => {
  const onRail = [...RAIL_GROUPS.flat(), ...RAIL_BOTTOM]
  assert.ok(!onRail.includes('activity' as never))
  assert.ok(!onRail.some((p) => (p as string) === 'music' || (p as string) === 'metadata'))
})

test('Ctrl and a number goes to the rail places in order; Activity and Settings keep their keys', () => {
  assert.equal(placeForKey(1), 'home')
  assert.equal(placeForKey(3), 'library')
  assert.equal(placeForKey(4), 'stats')
  assert.equal(placeForKey(7), 'devices')
  assert.equal(placeForKey(8), undefined)
  assert.equal(placeForKey(0), undefined)
  assert.equal(PLACE_KEYS.home, 'Ctrl+1')
  assert.equal(PLACE_KEYS.devices, 'Ctrl+7')
  assert.equal(PLACE_KEYS.activity, 'Ctrl+H')
  assert.equal(PLACE_KEYS.settings, 'Ctrl+,')
})

test('Home, Search, Library and Stats are one screen, so moving between them keeps it', () => {
  for (const p of ['home', 'search', 'library', 'stats'] as const) {
    assert.ok(isMusicPlace(p))
    assert.equal(screenKey(p), 'music')
  }
  assert.ok(!isMusicPlace('health'))
  assert.equal(screenKey('health'), 'health')
})

test('Stats is the music screen’s Listening view, both ways', () => {
  assert.equal(musicTopOf('stats'), 'listening')
  assert.equal(musicTopOf('library'), 'library')
  assert.equal(placeOfMusicTop('listening'), 'stats')
  assert.equal(placeOfMusicTop('search'), 'search')
})

test('a sideways swipe moves along the rail and stops at its ends', () => {
  assert.equal(swipeFrom('home', 1), 'search')
  assert.equal(swipeFrom('library', 1), 'stats')
  assert.equal(swipeFrom('home', -1), undefined)
  assert.equal(swipeFrom('devices', 1), undefined)
  assert.equal(swipeFrom('settings', 1), undefined)
})
