// node --test tests/   (Node runs the TypeScript as is)
// One lyrics clock per window. Each lyrics window used to run the frame loop three or four times
// (useNowLyrics in the frame, in useWindowCover, and again in the view), each with its own
// setState per frame. Now one provider per window reads the song and runs the clock, and every
// part of the window reads it from there: guarded here by where the hooks are called.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'

const dir = new URL('../src/components/lyrics/', import.meta.url)
const source = (name: string) => readFileSync(new URL(name, dir), 'utf8')
const files = readdirSync(dir).filter((f) => /\.tsx?$/.test(f))
const calls = (text: string, hook: string) => (text.match(new RegExp(`\\b${hook}\\(`, 'g')) ?? []).length

test('the smooth clock runs in one place: the window provider', () => {
  const where = files.filter((f) => calls(source(f), 'useSmoothPosition') > 0)
  assert.deepEqual(where, ['NowLyrics.tsx'])
  assert.equal(calls(source('NowLyrics.tsx'), 'useSmoothPosition'), 1)
})

test("the song's words are read once per window, by the provider", () => {
  const where = files.filter((f) => calls(source(f), 'useSongLyrics') > 0)
  assert.deepEqual(where, ['NowLyrics.tsx'])
})

test('the cover and the frame read the song from the provider, not a clock of their own', () => {
  for (const f of ['LyricsWindow.tsx', 'useNowLyrics.ts']) assert.equal(calls(source(f), 'useNowLyrics'), 0, f)
})

test('each lyrics window is wrapped in the provider once', () => {
  for (const f of ['BigScreen.tsx', 'DesktopLyrics.tsx']) assert.equal((source(f).match(/<NowLyricsProvider>/g) ?? []).length, 1, f)
})
