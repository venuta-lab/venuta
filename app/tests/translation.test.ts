// node --test tests/   (Node runs the TypeScript as is)
// Translation models on this device: the stub the translation package (L5) fills. Nothing is
// downloaded or sent anywhere unless asked; without the engine's commands, there are no models.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { modelFor, sizeWords, translationStatus, type TranslationModel } from '../src/lib/translation.ts'

const models: TranslationModel[] = [
  { id: 'hi-en', from: 'hi', to: 'en', name: 'Hindi to English', size_bytes: 47_185_920, installed: true, progress: null },
  { id: 'ja-en', from: 'ja', to: 'en', name: 'Japanese to English', size_bytes: 61_865_984, installed: false, progress: null },
  { id: 'es-en', from: 'es', to: 'en', name: 'Spanish to English', size_bytes: 20_971_520, installed: false, progress: 0.4 },
]

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

test("what Translation can do for a song that doesn't carry one in your language", () => {
  // Already there: nothing to ask.
  assert.deepEqual(translationStatus({ have: true, on: true, from: 'ja', to: 'en', models }), { kind: 'ready' })
  // Switched off: nothing to say.
  assert.deepEqual(translationStatus({ have: false, on: false, from: 'ja', to: 'en', models }), { kind: 'off' })
  // A model to download, only when asked.
  assert.deepEqual(translationStatus({ have: false, on: true, from: 'ja', to: 'en', models }), { kind: 'needs-model', model: models[1] })
  assert.deepEqual(translationStatus({ have: false, on: true, from: 'es', to: 'en', models }), { kind: 'downloading', model: models[2] })
  // Installed: translating on this PC.
  assert.deepEqual(translationStatus({ have: false, on: true, from: 'hi', to: 'en', models }), { kind: 'translating', model: models[0] })
  // No model for it (or no engine yet).
  assert.deepEqual(translationStatus({ have: false, on: true, from: 'ko', to: 'en', models }), { kind: 'none' })
  assert.deepEqual(translationStatus({ have: false, on: true, from: 'ja', to: 'en', models: [] }), { kind: 'none' })
})
