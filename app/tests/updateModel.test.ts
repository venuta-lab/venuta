// node --test tests/   (Node runs the TypeScript as is)
// What the update check shows and when it runs: a found update stays found, a manual check
// always ends in an answer, and quiet checks come back on their own.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  AFTER_FAILURE,
  EVERY,
  afterCheck,
  ago,
  knownUpdate,
  nextCheckIn,
  shouldAnnounce,
  statusLine,
  titleButton,
  type UpdateInfo,
  type UpdateState,
} from '../src/lib/updateModel.ts'

const info: UpdateInfo = { version: '0.4.0', current: '0.3.1', notes: 'New things', date: null }
const net = { kind: 'network' as const, message: "Couldn't reach GitHub to check for updates." }
const found: UpdateState = { at: 'found', info }

test('a check that finds a version offers it; one that does not says up to date', () => {
  assert.deepEqual(afterCheck({ at: 'idle' }, { ok: { status: 'found', info } }, false), found)
  assert.deepEqual(afterCheck({ at: 'idle' }, { ok: { status: 'current', current: '0.3.1' } }, true), { at: 'current', version: '0.3.1' })
})

test('a quiet check that fails keeps an update already found', () => {
  assert.deepEqual(afterCheck({ at: 'checking', manual: false, info }, { err: net }, false), found)
})

test('a manual check that fails says why, and still knows the update', () => {
  const s = afterCheck({ at: 'checking', manual: true, info }, { err: net }, true)
  assert.deepEqual(s, { at: 'failed', problem: net, info })
  assert.deepEqual(knownUpdate(s), info)
  const fresh = afterCheck({ at: 'checking', manual: true }, { err: net }, true)
  assert.deepEqual(fresh, { at: 'failed', problem: net, info: undefined })
})

test('a quiet check that fails with nothing known is a failure Settings can show', () => {
  assert.equal(afterCheck({ at: 'checking', manual: false }, { err: net }, false).at, 'failed')
})

test('Local only is its own answer, never "up to date"', () => {
  const s = afterCheck({ at: 'checking', manual: true }, { err: { kind: 'local_only', message: 'Local only is on.' } }, true)
  assert.equal(s.at, 'local_only')
  assert.match(statusLine(s, 0, 0, '0.3.1'), /Local only/)
})

test('the title bar shows the update until it is installed', () => {
  assert.deepEqual(titleButton(found), { label: 'Update to 0.4.0', busy: false, failed: false })
  assert.equal(titleButton({ at: 'checking', manual: false, info })?.label, 'Update to 0.4.0')
  assert.equal(titleButton({ at: 'downloading', info, done: 50, total: 200 })?.label, 'Updating 25%')
  assert.equal(titleButton({ at: 'downloading', info, done: 50, total: null })?.label, 'Updating…')
  assert.equal(titleButton({ at: 'installing', info })?.busy, true)
  assert.deepEqual(titleButton({ at: 'failed', problem: net, info }), { label: 'Update failed', busy: false, failed: true })
  for (const s of [{ at: 'idle' }, { at: 'current', version: '0.3.1' }, { at: 'failed', problem: net }, { at: 'local_only' }] as UpdateState[])
    assert.equal(titleButton(s), null, s.at)
})

test('checks come back: every few hours, sooner after a failure, never during an install', () => {
  assert.equal(nextCheckIn(found), EVERY)
  assert.equal(nextCheckIn({ at: 'current', version: '0.3.1' }), EVERY)
  assert.equal(nextCheckIn({ at: 'failed', problem: net }), AFTER_FAILURE)
  assert.equal(nextCheckIn({ at: 'downloading', info, done: 0, total: null }), null)
  assert.equal(nextCheckIn({ at: 'installing', info }), null)
  assert.ok(EVERY <= 6 * 3600_000 && AFTER_FAILURE < EVERY)
})

test('the notice is shown once per version', () => {
  assert.equal(shouldAnnounce(info, undefined), true)
  assert.equal(shouldAnnounce(info, '0.3.9'), true)
  assert.equal(shouldAnnounce(info, '0.4.0'), false)
})

test('Settings says the state, when it last checked, and what went wrong', () => {
  const now = 10 * 3600_000
  assert.equal(statusLine({ at: 'current', version: '0.3.1' }, now - 5 * 60_000, now, '0.3.1'), 'Venuta 0.3.1 is up to date. Last checked 5 minutes ago.')
  assert.equal(statusLine(found, now, now, '0.3.1'), 'Venuta 0.4.0 is available (you have 0.3.1).')
  assert.equal(statusLine({ at: 'failed', problem: net }, now - 2 * 3600_000, now, '0.3.1'), "Couldn't reach GitHub to check for updates. Last checked 2 hours ago.")
  assert.equal(statusLine({ at: 'failed', problem: net }, 0, now, '0.3.1'), "Couldn't reach GitHub to check for updates.")
  assert.match(statusLine({ at: 'downloading', info, done: 1, total: 4 }, 0, now, '0.3.1'), /Downloading Venuta 0\.4\.0.*25%/)
  assert.match(statusLine({ at: 'checking', manual: true }, 0, now, '0.3.1'), /Checking/)
  for (const s of [found, { at: 'idle' }, { at: 'installing', info }] as UpdateState[]) assert.doesNotMatch(statusLine(s, now, now, '0.3.1'), /—/)
})

test('times read plainly', () => {
  assert.equal(ago(20_000), 'just now')
  assert.equal(ago(60_000), '1 minute ago')
  assert.equal(ago(45 * 60_000), '45 minutes ago')
  assert.equal(ago(3600_000), '1 hour ago')
  assert.equal(ago(30 * 3600_000), 'yesterday')
  assert.equal(ago(4 * 24 * 3600_000), '4 days ago')
})
