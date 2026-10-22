// node --test tests/   (Node runs the TypeScript as is)
// The second lines under each lyric line: pronunciation and translation, built against the
// contract in docs/design/lyrics-second-line.md with the fixture songs in
// tests/fixtures/lyrics-second (a Hindi song with both, a Japanese one whose translation is in
// Chinese only, a Spanish one with a translation only, a mixed Hindi and English one, an English
// one with nothing extra).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { attributionOf, fromLegacy, langBase, modelOf, readModel, scriptOf, songLang, sourceKind, sourceWords, type LyricsModel } from '../src/lib/lyricsModel.ts'
import { noteWords, offered, reservedRows, rowsFor, secondProgress, sourcesOf } from '../src/lib/secondLines.ts'

const fixture = (name: string): LyricsModel => readModel(JSON.parse(readFileSync(new URL(`./fixtures/lyrics-second/${name}.json`, import.meta.url), 'utf8')))
const hindi = fixture('hindi')
const japanese = fixture('japanese')
const spanish = fixture('spanish')
const mixed = fixture('mixed')
const plain = fixture('plain')
const ALL = { roman: true, translation: true }
const NONE = { roman: false, translation: false }

test('the fixtures read as the contract has them', () => {
  assert.equal(hindi.lang, 'hi')
  assert.equal(hindi.lines[0].roman?.text, 'Aaj phir se chaand nikla hai')
  assert.equal(hindi.lines[0].roman?.words.length, hindi.lines[0].words.length, 'pronunciation timed by the original words')
  assert.deepEqual(sourceKind(hindi.lines[0].roman!.source), { kind: 'Paired', by: 'lyricsplus' })
  assert.deepEqual(sourceKind(mixed.lines[0].roman!.source), { kind: 'Made', by: null })
  assert.equal(plain.lines[0].roman, null)
  // Left out (skip_serializing_if) reads as none.
  const short = readModel({ lines: [{ start_ms: 0, end_ms: 10, text: 'Hi', script: 'Latn' }], version: 1 })
  assert.equal(short.lines[0].roman, null)
  assert.equal(short.lines[0].translation, null)
  assert.deepEqual(short.lines[0].words, [])
  assert.equal(short.lang, null)
})

test('a source reads the way serde writes it, and the other usual ways', () => {
  assert.deepEqual(sourceKind('Made'), { kind: 'Made', by: null })
  assert.deepEqual(sourceKind('Corrected'), { kind: 'Corrected', by: null })
  assert.deepEqual(sourceKind({ Lyrics: 'netease' }), { kind: 'Lyrics', by: 'netease' })
  assert.deepEqual(sourceKind({ Translated: 'opus-mt' }), { kind: 'Translated', by: 'opus-mt' })
  assert.deepEqual(sourceKind({ kind: 'Paired', provider: 'lyricsplus' } as never), { kind: 'Paired', by: 'lyricsplus' })
  assert.deepEqual(sourceKind({ kind: 'translated', engine: 'deepl' } as never), { kind: 'Translated', by: 'deepl' })
  assert.deepEqual(sourceKind(42 as never), { kind: 'Lyrics', by: null }, 'anything else is the lyrics')
})

test('where each line came from, said quietly and plainly', () => {
  assert.equal(sourceWords({ Lyrics: 'netease' }), 'From the lyrics')
  assert.equal(sourceWords({ Paired: 'lyricsplus' }), 'From another copy of the lyrics')
  assert.equal(sourceWords('Made'), 'Made on this PC')
  assert.equal(sourceWords({ Translated: 'opus-mt' }), 'Translated on this PC')
  assert.equal(sourceWords({ Translated: 'firefox-translations' }), 'Translated on this PC')
  assert.equal(sourceWords({ Translated: 'deepl' }), 'Translated by DeepL, with your key')
  assert.equal(sourceWords('Corrected'), 'Corrected by you')
  assert.equal(sourceWords('Made', 'phone'), 'Made on this phone')
  for (const s of ['Made', 'Corrected', { Lyrics: 'x' }, { Translated: 'opus-mt' }] as const) assert.doesNotMatch(sourceWords(s), /—/, 'no em dashes')
})

test('scripts, by the letters a line is written in', () => {
  assert.equal(scriptOf('आज फिर से चाँद निकला है'), 'Deva')
  assert.equal(scriptOf('ਤੇਰੇ ਬਿਨਾ'), 'Guru')
  assert.equal(scriptOf('கண்ணே கலைமானே'), 'Taml')
  assert.equal(scriptOf('夜空に星が光る'), 'Jpan', 'kana with kanji is Japanese (as the engine names it)')
  assert.equal(scriptOf('ab अब'), 'Deva', 'Latin loses a tie')
  assert.equal(scriptOf('夜空中星星闪耀'), 'Hani')
  assert.equal(scriptOf('사랑해요'), 'Hang')
  assert.equal(scriptOf('Я тебя люблю'), 'Cyrl')
  assert.equal(scriptOf('دل دیاں گلاں'), 'Arab')
  assert.equal(scriptOf('Bajo la luna'), 'Latn')
  assert.equal(scriptOf('दिल ये मेरा, baby please stay'), 'Latn', 'the letters most used win')
  assert.equal(scriptOf('दिल ये मेरा, baby'), 'Deva')
  assert.equal(scriptOf('♪ ... 123'), 'Zyyy')
})

test("a song's language: as the lyrics say, else from its script", () => {
  assert.equal(langBase('hi-Latn'), 'hi')
  assert.equal(langBase('pt_BR'), 'pt')
  assert.equal(songLang(hindi), 'hi')
  assert.equal(songLang({ ...hindi, lang: null }), 'hi')
  assert.equal(songLang({ ...japanese, lang: null }), 'ja')
  assert.equal(songLang({ ...plain, lang: null }), null, 'Latin letters alone say nothing of the language')
  assert.equal(songLang({ ...hindi, lang: 'mr' }), 'mr', 'the lyrics know better than the script')
})

test('pronunciation is offered for songs not in Latin letters; translation for songs in another language', () => {
  assert.deepEqual(offered(hindi, 'en'), { roman: true, translation: true })
  assert.deepEqual(offered(japanese, 'en'), { roman: true, translation: true }, 'not here yet, but it can be made on this PC')
  assert.deepEqual(offered(spanish, 'en'), { roman: false, translation: true })
  assert.deepEqual(offered(mixed, 'en'), { roman: true, translation: true })
  assert.deepEqual(offered(plain, 'en'), { roman: false, translation: false })
  assert.deepEqual(offered(spanish, 'es'), { roman: false, translation: false }, 'a song in the language you read needs no translation')
  assert.deepEqual(offered(null, 'en'), { roman: false, translation: false })
})

test('rows are kept for the song, not the line: only what is switched on and there', () => {
  assert.deepEqual(reservedRows(hindi, ALL, 'en'), ALL)
  assert.deepEqual(reservedRows(hindi, NONE, 'en'), NONE)
  assert.deepEqual(reservedRows(hindi, { roman: true, translation: false }, 'en'), { roman: true, translation: false })
  assert.deepEqual(reservedRows(spanish, ALL, 'en'), { roman: false, translation: true })
  assert.deepEqual(reservedRows(plain, ALL, 'en'), NONE)
  // A translation shows only in the language you read: the Japanese song's is Chinese.
  assert.deepEqual(reservedRows(japanese, ALL, 'en'), { roman: true, translation: false })
  assert.deepEqual(reservedRows(japanese, ALL, 'zh'), { roman: true, translation: true })
  assert.deepEqual(reservedRows(japanese, ALL, 'zh-Hans'), { roman: true, translation: true })
})

test('one layout rule: the original, then pronunciation, then translation, the same rows for every line', () => {
  for (const song of [hindi, japanese, spanish, mixed, plain]) {
    const rows = reservedRows(song, ALL, 'en')
    const counts = new Set(song.lines.map((l) => rowsFor(l, rows, 'en').length))
    assert.equal(counts.size, 1, `every line of ${song.lang} keeps the same rows`)
  }
  const [first] = rowsFor(mixed.lines[0], reservedRows(mixed, ALL, 'en'), 'en').map((r) => r.kind)
  assert.equal(first, 'roman')
  assert.deepEqual(
    rowsFor(mixed.lines[0], reservedRows(mixed, ALL, 'en'), 'en').map((r) => r.kind),
    ['roman', 'translation'],
  )
  // An English line in the mixed song keeps its rows, empty, so nothing moves.
  const english = rowsFor(mixed.lines[1], reservedRows(mixed, ALL, 'en'), 'en')
  assert.deepEqual(
    english.map((r) => [r.kind, r.text]),
    [
      ['roman', ''],
      ['translation', ''],
    ],
  )
  assert.deepEqual(rowsFor(spanish.lines[2], reservedRows(spanish, ALL, 'en'), 'en')[0].text, 'And even if the night is long, I am not afraid to get lost with you')
  // A translation in another language is never shown, even if a row is kept for one.
  assert.equal(rowsFor(japanese.lines[0], { roman: true, translation: true }, 'en')[1].text, '')
})

test('the karaoke fill runs on the pronunciation by its own word timing', () => {
  const line = hindi.lines[0]
  const roman = line.roman!
  // Halfway through the third word (से, se), with the lyrics' lead of 250 ms taken away.
  const mid = line.words[2].start_ms + (line.words[2].end_ms - line.words[2].start_ms) / 2 - 250
  const p = secondProgress(line, roman, mid, 0)
  const letters = roman.text.replace(/\s+/g, '').length
  const expected = ('Aaj'.length + 'phir'.length + 'se'.length / 2) / letters
  assert.ok(Math.abs(p - expected) < 0.01, `${p} ~ ${expected}`)
  assert.equal(secondProgress(line, roman, line.start_ms - 1000, 0), 0)
  assert.equal(secondProgress(line, roman, line.end_ms + 1000, 0), 1)
})

test('pronunciation without its own word times borrows the original words, word for word', () => {
  const line = hindi.lines[1]
  const bare = { ...line.roman!, words: [] }
  const t = line.words[1].end_ms - 250
  const p = secondProgress(line, bare, t, 0.123)
  const letters = bare.text.replace(/\s+/g, '').length
  assert.ok(Math.abs(p - ('Teri'.length + 'baaton'.length) / letters) < 0.01)
})

test('no word timing at all: the pronunciation fills with the line', () => {
  const line = mixed.lines[0]
  assert.equal(secondProgress(line, line.roman!, 99999, 0.4), 0.4)
})

test("where a song's second lines came from, for the quiet note", () => {
  assert.deepEqual(sourcesOf(hindi, reservedRows(hindi, ALL, 'en'), 'en'), {
    roman: { words: 'From another copy of the lyrics', corrected: 0 },
    translation: { words: 'Translated on this PC', corrected: 0 },
  })
  assert.deepEqual(sourcesOf(mixed, reservedRows(mixed, ALL, 'en'), 'en'), {
    roman: { words: 'Made on this PC', corrected: 0 },
    translation: { words: 'Translated on this PC', corrected: 1 },
  })
  assert.deepEqual(sourcesOf(plain, reservedRows(plain, ALL, 'en'), 'en'), { roman: null, translation: null })
  assert.equal(noteWords('roman', { words: 'Made on this PC', corrected: 0 }), 'Pronunciation made on this PC')
  assert.equal(noteWords('roman', { words: 'From the lyrics', corrected: 2 }), 'Pronunciation from the lyrics, 2 lines corrected by you')
  assert.equal(noteWords('translation', { words: 'Translated on this PC', corrected: 1 }), 'Translated on this PC, 1 line corrected by you')
  assert.equal(noteWords('translation', { words: 'From the lyrics', corrected: 0 }), 'Translation from the lyrics')
  assert.equal(noteWords('translation', { words: 'Corrected by you', corrected: 3 }), 'Translation corrected by you')
})

test("today's lyrics (parallel arrays) become the contract, Roman letters made once per song", () => {
  const legacy = {
    lines: [
      [1000, 'तेरी बातों में'],
      [4000, 'Hold me close'],
      [7000, '夜空に星が光る'],
    ] as [number, string][],
    text: '',
    words: [
      [
        [1000, 1500, 'तेरी'],
        [1500, 2000, 'बातों'],
        [2000, 2600, 'में'],
      ],
      [],
      [],
    ] as [number, number, string][][],
    translations: [null, null, '夜空中星星闪耀'],
    found: { key: 'k', source: 'netease', label: 'Netease', credit: null, choice: { offset_ms: 0 }, instrumental: false },
  }
  const m = fromLegacy(legacy)
  assert.equal(m.lines.length, 3)
  assert.equal(m.lines[0].start_ms, 1000)
  assert.equal(m.lines[0].end_ms, 4000)
  assert.equal(m.lines[2].end_ms, 12000, 'the last line runs five seconds')
  assert.equal(m.lines[0].script, 'Deva')
  assert.equal(m.lines[0].roman?.text, 'teri baaton mein')
  assert.deepEqual(sourceKind(m.lines[0].roman!.source), { kind: 'Made', by: null })
  assert.deepEqual(
    m.lines[0].roman?.words.map((w) => [w.start_ms, w.text]),
    [
      [1000, 'teri'],
      [1500, 'baaton'],
      [2000, 'mein'],
    ],
  )
  assert.equal(m.lines[1].roman, null, 'Latin letters need none')
  assert.equal(m.lines[2].roman, null, 'only Indian scripts are read by rule here')
  assert.equal(m.lines[2].translation?.text, '夜空中星星闪耀')
  assert.equal(m.lines[2].translation?.lang, 'zh', "a source's translation in Chinese letters is Chinese")
  assert.deepEqual(sourceKind(m.lines[2].translation!.source), { kind: 'Lyrics', by: 'netease' })
  assert.equal(fromLegacy(legacy), fromLegacy(legacy), 'worked out once per song')
})

test('contract lines from the engine pass through as they are', () => {
  const legacy = { lines: [[0, 'x']] as [number, string][], text: '', model: hindi }
  assert.equal(fromLegacy(legacy), hindi)
})

test("the engine's sources, as it writes them", () => {
  assert.equal(sourceWords({ kind: 'lyrics', by: 'netease' }), 'From the lyrics')
  assert.equal(sourceWords({ kind: 'paired', by: 'lyricsplus' }), 'From another copy of the lyrics')
  assert.equal(sourceWords({ kind: 'made' }), 'Made on this PC')
  assert.equal(sourceWords({ kind: 'translated', by: 'opus-mt' }), 'Translated on this PC')
  assert.equal(sourceWords({ kind: 'corrected' }), 'Corrected by you')
})

test("the engine's lines become the model, your offset moving every time", () => {
  const raw = JSON.parse(readFileSync(new URL('./fixtures/lyrics-second/hindi.json', import.meta.url), 'utf8'))
  const m = modelOf(raw.lines, raw.lang, -1500)
  assert.equal(m.lang, 'hi')
  assert.equal(m.lines[0].start_ms, 0, 'never before the start')
  assert.equal(m.lines[1].start_ms, 3500)
  assert.equal(m.lines[1].roman?.words[0].start_ms, 3500, 'the pronunciation is timed with the line')
  assert.equal(m.lines[5].roman, null, 'a line with nothing under it')
})

test('a translation whose language the lyrics did not say shows when it is in the letters you read', () => {
  const line = { ...plain.lines[0], translation: { text: 'Hello', words: [], lang: 'und-Latn', source: { kind: 'lyrics', by: 'file' } } } as const
  const song = { lines: [line], lang: 'ja', version: 1 }
  assert.deepEqual(reservedRows(song, ALL, 'en'), { roman: false, translation: true })
  assert.deepEqual(reservedRows(song, ALL, 'hi'), { roman: false, translation: false })
  assert.deepEqual(reservedRows(japanese, ALL, 'en'), { roman: true, translation: false }, 'zh-Hans is not English')
})

test("Google's attribution goes beside the lines Google translated, and only those", () => {
  // Google asks apps that show its Cloud Translation API's translations to show "Powered by
  // Google Translate" beside them (docs/research/lyrics-translation-test.md).
  const google = fixture('japanese-google')
  for (const line of google.lines) {
    const rows = rowsFor(line, { roman: true, translation: true }, 'en')
    assert.deepEqual(
      rows.map((r) => [r.kind, attributionOf(r.second?.source)]),
      [
        ['roman', null],
        ['translation', 'Powered by Google Translate'],
      ],
    )
  }
  for (const s of [{ kind: 'translated', by: 'google' }, { Translated: 'google' }, { kind: 'translated', by: 'Google' }, 'translated'])
    assert.equal(attributionOf(s as never), s === 'translated' ? null : 'Powered by Google Translate', JSON.stringify(s))
  // Other engines, the lyrics' own lines, and a Google line corrected by hand: nothing.
  for (const s of [{ kind: 'translated', by: 'deepl' }, { kind: 'translated', by: 'opus-mt' }, { kind: 'translated', by: 'constructor' }, { kind: 'lyrics', by: 'google' }, { kind: 'corrected' }, undefined])
    assert.equal(attributionOf(s as never), null, JSON.stringify(s))
  for (const line of fixture('hindi').lines) assert.equal(attributionOf(line.translation?.source), null)
})
