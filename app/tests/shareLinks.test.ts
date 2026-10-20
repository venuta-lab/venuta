// node --test tests/   (Node runs the TypeScript as is)
// The YouTube Music links Copy link puts on the clipboard (lib/shareLinks.ts).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { albumLink, artistLink, collectionLink } from '../src/lib/shareLinks.ts'

test('an album opens by its page', () => {
  assert.equal(albumLink('MPREb_abc'), 'https://music.youtube.com/browse/MPREb_abc')
})

test('a playlist or a podcast opens by its list', () => {
  assert.equal(collectionLink('VLPLabc'), 'https://music.youtube.com/playlist?list=PLabc')
  assert.equal(collectionLink('PLabc'), 'https://music.youtube.com/playlist?list=PLabc')
  assert.equal(collectionLink('MPSPPLpod'), 'https://music.youtube.com/playlist?list=PLpod')
  assert.equal(collectionLink('RDCLAK5uy_mix'), 'https://music.youtube.com/playlist?list=RDCLAK5uy_mix')
})

test('your own uploads have no link to share', () => {
  assert.equal(collectionLink('FEmusic_library_privately_owned_release_detailb_po_x'), null)
  assert.equal(albumLink(''), null)
})

test('an artist opens by their channel', () => {
  assert.equal(artistLink('UC123'), 'https://music.youtube.com/channel/UC123')
  assert.equal(artistLink(' '), null)
})
