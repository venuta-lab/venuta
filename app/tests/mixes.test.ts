// node --test tests/   (Node runs the TypeScript as is)
// A mix card on Home plays as YouTube Music's own play button plays it.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { radioOfCard } from '../src/lib/mixes.ts'

const card = (collection_id: string, play: { playlist_id?: string; params?: string; video_id?: string } | null = null) => ({
  kind: 'playlist' as const,
  collection_id,
  play,
})

test('a mix plays through its own play button, params and all', () => {
  const play = { playlist_id: 'RDTMAK5uy_super', params: 'wAEB8gECKAE%3D' }
  assert.deepEqual(radioOfCard(card('VLRDTMAK5uy_super', play)), play)
  const mix = { video_id: 'seedvideo01', playlist_id: 'RDTMAK5uy_mix1', params: 'wAEB' }
  assert.deepEqual(radioOfCard(card('VLRDTMAK5uy_mix1', mix)), mix)
})

test('a mix with no button kept still plays as the radio its id names', () => {
  assert.deepEqual(radioOfCard(card('VLRDCLAK5uy_x')), { playlist_id: 'RDCLAK5uy_x' })
})

test('a playlist or a podcast is not a radio, whatever its button', () => {
  assert.equal(radioOfCard(card('VLPLhits', { playlist_id: 'PLhits' })), null)
  assert.equal(radioOfCard({ kind: 'podcast', collection_id: 'MPSPPL1', play: null }), null)
})
