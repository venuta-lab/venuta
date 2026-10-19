// node --test tests/   (Node runs the TypeScript as is)
// The sleep timer at a glance: what the player bar's moon says, and what each choice does.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { SLEEP_MINUTES, sleepBadge, sleepChoice } from '../src/lib/sleep.ts'

test('nothing set: the moon says nothing', () => {
  assert.equal(sleepBadge({ sleep_in_s: null, stop_after_current: false }), null)
})

test('a timer: the minutes left, rounded up, and seconds in the last minute', () => {
  assert.equal(sleepBadge({ sleep_in_s: 30 * 60, stop_after_current: false }), '30m')
  assert.equal(sleepBadge({ sleep_in_s: 22 * 60 + 1, stop_after_current: false }), '23m')
  assert.equal(sleepBadge({ sleep_in_s: 61, stop_after_current: false }), '2m')
  assert.equal(sleepBadge({ sleep_in_s: 60, stop_after_current: false }), '1m')
  assert.equal(sleepBadge({ sleep_in_s: 45, stop_after_current: false }), '0:45')
  assert.equal(sleepBadge({ sleep_in_s: 5, stop_after_current: false }), '0:05')
  assert.equal(sleepBadge({ sleep_in_s: 0, stop_after_current: false }), '0:00')
})

test('stopping after this song says so (and the sooner of the two shows)', () => {
  assert.equal(sleepBadge({ sleep_in_s: null, stop_after_current: true }), 'End of song')
  assert.equal(sleepBadge({ sleep_in_s: 90, stop_after_current: true }), 'End of song', 'the song ends first or the timer does; both stop it')
})

test('the choices, and which one is on', () => {
  assert.deepEqual(SLEEP_MINUTES, [15, 30, 45, 60, 90])
  assert.equal(sleepChoice({ sleep_in_s: null, stop_after_current: false }), 'off')
  assert.equal(sleepChoice({ sleep_in_s: null, stop_after_current: true }), 'song')
  assert.equal(sleepChoice({ sleep_in_s: 1200, stop_after_current: false }), 'timer')
})
