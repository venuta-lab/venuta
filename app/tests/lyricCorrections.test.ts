// node --test tests/   (Node runs the TypeScript as is)
// A second line corrected by hand: kept per song, shown in place of what was made or found, and
// said to be yours. Taken away, the line goes back to where it came from.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { readModel, sourceKind, type LyricsModel } from '../src/lib/lyricsModel.ts'
import { applyCorrections, readCorrections, withCorrection } from '../src/lib/lyricCorrections.ts'

const fixture = (name: string): LyricsModel => readModel(JSON.parse(readFileSync(new URL(`./fixtures/lyrics-second/${name}.json`, import.meta.url), 'utf8')))
const hindi = fixture('hindi')
const mixed = fixture('mixed')

test('a corrected pronunciation shows in place, timed by the original words', () => {
  const c = withCorrection({}, hindi.lines[0].text, 'roman', 'Aaj phir se chand nikla hai')
  const m = applyCorrections(hindi, c, 'en')
  const r = m.lines[0].roman!
  assert.equal(r.text, 'Aaj phir se chand nikla hai')
  assert.deepEqual(sourceKind(r.source), { kind: 'Corrected', by: null })
  assert.equal(r.lang, 'hi-Latn')
  assert.deepEqual(
    r.words.map((w) => [w.start_ms, w.text]),
    hindi.lines[0].words.map((w, i) => [w.start_ms, r.text.split(' ')[i]]),
  )
  assert.equal(m.lines[1], hindi.lines[1], 'other lines as they were')
  assert.equal(hindi.lines[0].roman?.text, 'Aaj phir se chaand nikla hai', 'the song itself is not changed')
})

test('a correction with a different number of words is not word-timed (the fill follows the line)', () => {
  const m = applyCorrections(hindi, withCorrection({}, hindi.lines[0].text, 'roman', 'Aaj phir'), 'en')
  assert.deepEqual(m.lines[0].roman?.words, [])
})

test('a corrected translation is in the language you read, and a line with none can get one', () => {
  const c = withCorrection({}, mixed.lines[1].text, 'translation', 'Hold me close tonight (as it is)')
  const m = applyCorrections(mixed, c, 'en')
  assert.equal(m.lines[1].translation?.text, 'Hold me close tonight (as it is)')
  assert.equal(m.lines[1].translation?.lang, 'en')
  assert.deepEqual(sourceKind(m.lines[1].translation!.source), { kind: 'Corrected', by: null })
})

test('the same line sung again is corrected too', () => {
  const song: LyricsModel = { ...hindi, lines: [hindi.lines[0], hindi.lines[1], hindi.lines[0]] }
  const m = applyCorrections(song, withCorrection({}, hindi.lines[0].text, 'translation', 'The moon is out again tonight'), 'en')
  assert.equal(m.lines[0].translation?.text, 'The moon is out again tonight')
  assert.equal(m.lines[2].translation?.text, 'The moon is out again tonight')
})

test('taken away (empty), the line is back as it came', () => {
  let c = withCorrection({}, hindi.lines[0].text, 'roman', 'Something else')
  c = withCorrection(c, hindi.lines[0].text, 'roman', '  ')
  assert.deepEqual(c, {})
  assert.equal(applyCorrections(hindi, c, 'en'), hindi, 'nothing corrected: the song as it is')
})

test('read back from what is kept, anything not of its kind left out', () => {
  const kept = JSON.stringify({ 'file:a.flac': { 'आज': { roman: 'Aaj', translation: 7 } }, bad: 3 })
  assert.deepEqual(readCorrections(kept, 'file:a.flac'), { 'आज': { roman: 'Aaj' } })
  assert.deepEqual(readCorrections(kept, 'file:b.flac'), {})
  assert.deepEqual(readCorrections('{nope', 'file:a.flac'), {})
  assert.deepEqual(readCorrections(null, 'file:a.flac'), {})
})
