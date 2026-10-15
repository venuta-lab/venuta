// node --test tests/   (Node runs the TypeScript as is)
// An online song in words: its quality, a length not known yet, its cover's colour.
import { describe, it } from 'node:test'
import assert from 'node:assert/strict'
import { hueOf, onlineQuality, songLength } from '../src/lib/onlineSong.ts'

const signal = (codec: string, bitrate_kbps: number) => ({
  device: 'Speakers',
  mode: 'shared',
  source: { rate: 44_100, channels: 2, bits: 16, lossless: false, codec },
  file: { size_bytes: 1, bitrate_kbps, decoder: '' },
  buffer_ms: 0,
  latency_ms: 0,
  output: { rate: 48_000, channels: 2, container_bits: 32, valid_bits: 32, kind: 'float' },
  bit_perfect: false,
  steps: [],
})

describe('an online song in words', () => {
  it('says the codec and bitrate the stream has', () => {
    assert.equal(onlineQuality(signal('AAC', 129.4)), 'AAC 129 kbps')
  })
  it('says nothing until the stream has opened', () => {
    assert.equal(onlineQuality(null), '')
  })
  it('leaves out a kind it does not know, and never says "Remote"', () => {
    assert.equal(onlineQuality(signal('Audio', 160)), '160 kbps')
    assert.equal(onlineQuality(signal('Remote', 0)), '')
  })
})

describe('a length not known yet', () => {
  it('reads as a dash, never 0:00', () => {
    assert.equal(songLength(0), '–')
    assert.equal(songLength(undefined), '–')
    assert.equal(songLength(null), '–')
    assert.equal(songLength(214_000), '3:34')
  })
})

describe('the online cover', () => {
  it('gets the same colour for the same album, whatever its case or spacing', () => {
    assert.equal(hueOf('Hybrid Theory'), hueOf(' hybrid theory '))
    assert.ok(hueOf('Hybrid Theory') >= 0)
    assert.ok(hueOf('Hybrid Theory') < 360)
  })
})
