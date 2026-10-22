// node --test tests/   (Node runs the TypeScript as is)
// Translation: on this device (the models, offered only for the song languages it translated
// well), or with your own key (DeepL, Microsoft, Google, Claude), sent only when you press
// Translate or turned on translating automatically. Nothing is downloaded or sent unless asked.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { modelFor, statusWords, sizeWords, translationStatus, type TranslationModel, type TranslationStatus } from '../src/lib/translation.ts'
import { DEFAULT_ENGINE, DEFAULT_MODEL, ENGINE_IDS, ENGINES, engineArg, readEngine, type EngineChoice } from '../src/lib/translationEngine.ts'

const models: TranslationModel[] = [
  { id: 'hi-en', from: 'hi', to: 'en', name: 'Hindi to English', size_bytes: 47_185_920, installed: true, progress: null, offered: false },
  { id: 'ja-en', from: 'ja', to: 'en', name: 'Japanese to English', size_bytes: 61_865_984, installed: false, progress: null, offered: false },
  { id: 'es-en', from: 'es', to: 'en', name: 'Spanish to English', size_bytes: 20_971_520, installed: false, progress: 0.4, offered: true },
  { id: 'fr-en', from: 'fr', to: 'en', name: 'French to English', size_bytes: 20_971_520, installed: false, progress: null, offered: true },
  { id: 'de-en', from: 'de', to: 'en', name: 'German to English', size_bytes: 20_971_520, installed: true, progress: null, offered: true },
]

const device = DEFAULT_ENGINE
const keyed = (engine: EngineChoice['engine'], auto = false): EngineChoice => ({ engine, auto, model: DEFAULT_MODEL })
const status = (a: Partial<Parameters<typeof translationStatus>[0]> & { from: string | null }): TranslationStatus =>
  translationStatus({ have: false, on: true, to: 'en', models, engine: device, keyKept: false, asked: false, error: null, ...a })

test('the model for a song: by its language and the one you read', () => {
  assert.equal(modelFor(models, 'hi', 'en')?.id, 'hi-en')
  assert.equal(modelFor(models, 'hi-Latn', 'en-GB')?.id, 'hi-en')
  assert.equal(modelFor(models, 'ko', 'en'), null)
  assert.equal(modelFor(models, null, 'en'), null)
})

test('sizes, as people read them', () => {
  assert.equal(sizeWords(47_185_920), '45 MB')
  assert.equal(sizeWords(900_000), '1 MB')
  assert.equal(sizeWords(1_610_612_736), '1.5 GB')
})

test('on this device: a model to download, downloading, translating', () => {
  // Already there: nothing to ask. Switched off: nothing to say.
  assert.deepEqual(status({ have: true, from: 'es' }), { kind: 'ready' })
  assert.deepEqual(status({ on: false, from: 'es' }), { kind: 'off' })
  assert.deepEqual(status({ from: 'fr' }), { kind: 'needs-model', model: models[3] })
  assert.deepEqual(status({ from: 'es' }), { kind: 'downloading', model: models[2] })
  assert.deepEqual(status({ from: 'de' }), { kind: 'translating', engine: 'device', model: models[4] })
  // It failed: said, with a way to try again.
  assert.deepEqual(status({ from: 'de', error: 'The translation came back with 2 lines for 3' }), { kind: 'failed', engine: 'device', error: 'The translation came back with 2 lines for 3' })
})

test('on this device, never for the languages it got wrong: said why, pointing to the key engines', () => {
  // A model there is (even one downloaded before), but not offered: Hindi, Japanese.
  assert.deepEqual(status({ from: 'hi' }), { kind: 'unreliable' })
  assert.deepEqual(status({ from: 'ja' }), { kind: 'unreliable' })
  // No model at all (Punjabi, Korean here).
  assert.deepEqual(status({ from: 'ko' }), { kind: 'none' })
  assert.deepEqual(status({ from: 'ja', models: [] }), { kind: 'none' })
  const why = statusWords({ kind: 'unreliable' }, 'hi', 'en')
  assert.equal(why, "Translation on this PC got Hindi lyrics wrong too often in our tests, so it isn't offered: choose DeepL, Microsoft, Google or AI with your own key in Settings > Lyrics.")
  assert.ok(!why.includes('—'))
  assert.match(statusWords({ kind: 'none' }, 'pa', 'en'), /Punjabi.*DeepL, Microsoft, Google or AI with your own key/)
})

test('with your key: only when you press Translate, unless you turned on translating automatically', () => {
  // No key yet: nothing is sent, and you're told where to add one.
  assert.deepEqual(status({ from: 'hi', engine: keyed('deepl') }), { kind: 'needs-key', engine: 'deepl' })
  // A key, automatic off (the default): waits for Translate.
  assert.deepEqual(status({ from: 'hi', engine: keyed('deepl'), keyKept: true }), { kind: 'ask', engine: 'deepl' })
  // Pressed, or automatic on: translating with it.
  assert.deepEqual(status({ from: 'hi', engine: keyed('deepl'), keyKept: true, asked: true }), { kind: 'translating', engine: 'deepl' })
  assert.deepEqual(status({ from: 'pa', engine: keyed('llm', true), keyKept: true }), { kind: 'translating', engine: 'llm' })
  // Never without a key, even automatic.
  assert.deepEqual(status({ from: 'hi', engine: keyed('google', true) }), { kind: 'needs-key', engine: 'google' })
  // A failure in plain words, with a way to try again.
  assert.deepEqual(status({ from: 'hi', engine: keyed('microsoft'), keyKept: true, asked: true, error: 'Your Microsoft Translator quota is used up.' }), {
    kind: 'failed',
    engine: 'microsoft',
    error: 'Your Microsoft Translator quota is used up.',
  })
  // The song's own translation still comes first; off is off.
  assert.deepEqual(status({ have: true, from: 'hi', engine: keyed('deepl', true), keyKept: true }), { kind: 'ready' })
  assert.deepEqual(status({ on: false, from: 'hi', engine: keyed('deepl', true), keyKept: true }), { kind: 'off' })
  assert.match(statusWords({ kind: 'ask', engine: 'deepl' }, 'hi', 'en'), /DeepL/)
  assert.match(statusWords({ kind: 'needs-key', engine: 'llm' }, 'hi', 'en'), /Claude.*Settings > Lyrics/)
})

test('the engine choice: on this PC at first, automatic off, the default model', () => {
  assert.deepEqual(readEngine(() => null), { engine: 'device', auto: false, model: 'claude-sonnet-5-5' })
  assert.deepEqual(readEngine(() => '{"engine":"llm","auto":true,"model":"claude-opus-5-5"}'), { engine: 'llm', auto: true, model: 'claude-opus-5-5' })
  assert.deepEqual(readEngine(() => '{"engine":"nope","auto":"yes","model":"  "}'), { engine: 'device', auto: false, model: 'claude-sonnet-5-5' })
  assert.deepEqual(readEngine(() => 'not json'), DEFAULT_ENGINE)
  assert.deepEqual(engineArg(keyed('llm')), { engine: 'llm', model: 'claude-sonnet-5-5' })
  assert.deepEqual(engineArg(keyed('deepl')), { engine: 'deepl' })
})

test("Google's terms for its translations: the attribution, the disclaimer, and the page to link to", () => {
  // As https://docs.cloud.google.com/translate/attribution asks (checked 2026-10-22).
  const g = ENGINES.google
  assert.equal(g.attribution, 'Powered by Google Translate')
  assert.match(g.notice ?? '', /^THIS SERVICE MAY CONTAIN TRANSLATIONS POWERED BY GOOGLE\. GOOGLE DISCLAIMS ALL WARRANTIES RELATED TO THE TRANSLATIONS,/)
  assert.equal(g.page, 'https://cloud.google.com/translate')
  // No other engine asks for any.
  for (const id of ENGINE_IDS.filter((e) => e !== 'google')) assert.deepEqual([ENGINES[id].attribution, ENGINES[id].notice, ENGINES[id].page], [undefined, undefined, undefined], id)
})
