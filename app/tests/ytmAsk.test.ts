// node --test tests/   (Node runs the TypeScript as is)
// What Venuta asks before a change to your YouTube Music account that can't be taken back
// (lib/ytmAsk.ts): every one in the same plain words, each allowed, none blocked.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { ACCOUNT_ASKS, askDeletePlaylist, askDeleteUpload, askRemoveFromHistory, askRemovePastSearch, isUploadAlbum } from '../src/lib/ytmAsk.ts'

test('deleting a playlist asks with its name, on YouTube Music', () => {
  const a = askDeletePlaylist('Road trip')
  assert.equal(a.title, 'Delete “Road trip” on YouTube Music?')
  assert.equal(a.confirm, 'Delete playlist')
})

test('deleting an upload says the file on this computer stays', () => {
  const song = askDeleteUpload('Home Recording', 'song')
  assert.equal(song.title, 'Delete “Home Recording” on YouTube Music?')
  assert.match(song.text, /file on this computer stays/)
  assert.equal(song.confirm, 'Delete upload')
  assert.match(askDeleteUpload('Demos', 'album').text, /album/)
})

test('taking a play or a search out of the history asks too', () => {
  assert.equal(askRemoveFromHistory('Kesariya').title, 'Remove “Kesariya” from your YouTube Music history?')
  assert.equal(askRemovePastSearch('arijit singh').title, 'Remove “arijit singh” from your YouTube Music search history?')
})

test('every ask is the same plain shape: a question, why, and that Venuta can’t undo it', () => {
  for (const ask of ACCOUNT_ASKS('X')) {
    assert.ok(ask.title.endsWith('?'), ask.title)
    assert.ok(ask.title.includes('“X”'), ask.title)
    assert.ok(ask.text.endsWith('This can’t be undone from Venuta.'), ask.text)
    assert.equal(ask.danger, true)
    assert.ok(ask.confirm && !ask.confirm.includes('…'))
    assert.ok(!/—/.test(ask.title + ask.text + ask.confirm), 'no em dashes')
  }
})

test('an uploaded album is told by its page id, with or without VL', () => {
  assert.equal(isUploadAlbum('FEmusic_library_privately_owned_release_detailb_po_X'), true)
  assert.equal(isUploadAlbum('VLFEmusic_library_privately_owned_release_detailb_po_X'), true)
  assert.equal(isUploadAlbum('MPREb_X'), false)
  assert.equal(isUploadAlbum('PLX'), false)
})
