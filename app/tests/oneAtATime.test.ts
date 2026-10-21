// node --test tests/   (Node runs the TypeScript as is)
// A lyrics window's button pressed again while the last press is still being carried out does
// nothing more: one press, one window (the engine makes it one at a time as well).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { oneAtATime } from '../src/lib/oneAtATime.ts'

test('a press while the last one is still going is the same press', async () => {
  const once = oneAtATime<string>()
  let runs = 0
  let finish: () => void = () => {}
  const slow = () =>
    new Promise<void>((r) => {
      runs++
      finish = r
    })
  const a = once('overlay', slow)
  const b = once('overlay', slow)
  assert.equal(runs, 1)
  assert.equal(a, b)
  finish()
  await a
  // Done: the next press is a press of its own.
  const c = once('overlay', slow)
  assert.equal(runs, 2)
  finish()
  await c
})

test('each window has its own presses', async () => {
  const once = oneAtATime<string>()
  let runs = 0
  const quick = async () => {
    runs++
  }
  await Promise.all([once('stage', quick), once('overlay', quick)])
  assert.equal(runs, 2)
})

test('a press that fails lets the next one through', async () => {
  const once = oneAtATime<string>()
  await assert.rejects(once('stage', async () => Promise.reject(new Error('no'))))
  let ran = false
  await once('stage', async () => {
    ran = true
  })
  assert.ok(ran)
})
