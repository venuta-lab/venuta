// node --test tests/   (Node runs the TypeScript as is)
// The two switches, Pronunciation and Translation: one choice for every lyrics view, remembered
// per song language, starting from the defaults in Settings > Lyrics. The per-window "Roman
// letters too" switch from before becomes the choice for the Indian languages it covered.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { choiceFor, keyAction, readChoices, readerLang, withChoice, withDefaults, CHOICES_KEY } from '../src/lib/secondChoice.ts'

const from = (kept: Record<string, string>) => (key: string) => kept[key] ?? null

test('nothing kept: both off, translations into English', () => {
  const c = readChoices(from({}))
  assert.deepEqual(choiceFor(c, 'hi'), { roman: false, translation: false })
  assert.equal(readerLang(c, 'de-DE'), 'en')
})

test('each song language keeps its own switches', () => {
  let c = readChoices(from({}))
  c = withChoice(c, 'hi', 'roman', true)
  c = withChoice(c, 'es', 'translation', true)
  assert.deepEqual(choiceFor(c, 'hi'), { roman: true, translation: false })
  assert.deepEqual(choiceFor(c, 'es'), { roman: false, translation: true })
  assert.deepEqual(choiceFor(c, 'ja'), { roman: false, translation: false }, 'a language never switched follows the defaults')
  // Region and script do not make another language.
  assert.deepEqual(choiceFor(c, 'hi-Latn'), { roman: true, translation: false })
  assert.deepEqual(choiceFor(c, 'es-MX'), { roman: false, translation: true })
})

test('the defaults are for languages never switched; a language switched keeps its choice', () => {
  let c = readChoices(from({}))
  c = withChoice(c, 'hi', 'roman', false)
  c = withDefaults(c, { roman: true, translation: true })
  assert.deepEqual(choiceFor(c, 'ko'), { roman: true, translation: true })
  assert.deepEqual(choiceFor(c, 'hi'), { roman: false, translation: true }, 'only the switch touched is kept')
})

test('kept, and read back', () => {
  let c = readChoices(from({}))
  c = withChoice(withDefaults(c, { roman: false, translation: true }), 'ja', 'roman', true)
  c = { ...c, into: 'es' }
  const back = readChoices(from({ [CHOICES_KEY]: JSON.stringify(c) }))
  assert.deepEqual(back, c)
  assert.deepEqual(readChoices(from({ [CHOICES_KEY]: '{nope' })).defaults, { roman: false, translation: false }, 'unreadable: the defaults')
  assert.deepEqual(readChoices(from({ [CHOICES_KEY]: JSON.stringify({ byLang: { hi: { roman: 'yes' } }, into: 7 }) })).byLang, { hi: {} }, 'only what is of its kind')
})

test('"Roman letters too" from before: on in either window, on for the Indian languages it covered', () => {
  for (const key of ['lyric-look-stage', 'lyric-look-overlay', 'lyric-look']) {
    const c = readChoices(from({ [key]: JSON.stringify({ look: 'glow', latin: true }) }))
    for (const lang of ['hi', 'mr', 'ne', 'sa', 'bn', 'gu', 'pa', 'or', 'ta', 'te', 'kn', 'ml']) assert.equal(choiceFor(c, lang).roman, true, `${key}: ${lang}`)
    assert.equal(choiceFor(c, 'ja').roman, false, 'Japanese was never read by it')
    assert.equal(choiceFor(c, 'hi').translation, false)
  }
  const off = readChoices(from({ 'lyric-look-stage': JSON.stringify({ latin: false }) }))
  assert.equal(choiceFor(off, 'hi').roman, false)
  // Once there is a choice of its own, the old switch is not read again.
  const own = readChoices(from({ [CHOICES_KEY]: JSON.stringify({ byLang: {} }), 'lyric-look-stage': JSON.stringify({ latin: true }) }))
  assert.equal(choiceFor(own, 'hi').roman, false)
})

test('the language translations are into: English, the system language, or one chosen', () => {
  const c = readChoices(from({}))
  assert.equal(readerLang({ ...c, into: 'system' }, 'hi-IN'), 'hi')
  assert.equal(readerLang({ ...c, into: 'system' }, ''), 'en', 'no system language known: English')
  assert.equal(readerLang({ ...c, into: 'es' }, 'hi-IN'), 'es')
})

test('the keys: T for pronunciation (as Roman letters had), Shift+T for translation', () => {
  const k = (key: string, more: Partial<KeyboardEvent> = {}) => keyAction({ key, shiftKey: false, ctrlKey: false, altKey: false, metaKey: false, ...more })
  assert.equal(k('t'), 'roman')
  assert.equal(k('T', { shiftKey: true }), 'translation')
  assert.equal(k('t', { ctrlKey: true }), null)
  assert.equal(k('t', { altKey: true }), null)
  assert.equal(k('r'), null)
})
