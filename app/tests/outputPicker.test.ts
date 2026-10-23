// The output picker's logic (lib/outputPicker.ts) over the owner's kinds of devices: grouped by
// kind in order with clear names, the Bluetooth twins as one device, which one is chosen and
// which one plays, Follow Windows, the playing device's state and delay in words, the note and
// the Bluetooth suggestion.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  current,
  delayWords,
  deviceLine,
  followChoice,
  formatsWords,
  groups,
  isChosen,
  isPlaying,
  noteOf,
  stateLine,
  suggestion,
  type OutputChoices,
  type OutputDevice,
  type PlayingSignal,
} from '../src/lib/outputPicker.ts'

const dev = (o: Partial<OutputDevice> & Pick<OutputDevice, 'id' | 'name' | 'kind'>): OutputDevice => ({
  detail: '',
  form: 'speakers',
  hands_free: null,
  hands_free_only: false,
  default: false,
  connected: true,
  delay_ms: null,
  suggested_delay_ms: null,
  exclusive_suits: o.kind !== 'bluetooth' && o.kind !== 'virtual',
  exclusive_rates: [],
  exclusive_bits: 0,
  ...o,
})

const devices: OutputDevice[] = [
  dev({ id: 'speakers', name: 'Speakers', kind: 'wired', detail: 'Realtek(R) Audio', default: true }),
  dev({ id: 'pods', name: 'AirPods Pro', kind: 'bluetooth', form: 'headphones', hands_free: 'pods-hf', suggested_delay_ms: 170 }),
  dev({ id: 'x600', name: 'soundcore Motion X600', kind: 'bluetooth', hands_free: 'x600-hf', suggested_delay_ms: 170, delay_ms: 180 }),
  dev({ id: 'dac', name: 'SNOWSKY DISC', kind: 'usb', form: 'headphones', exclusive_rates: [44_100, 48_000, 96_000, 192_000], exclusive_bits: 32 }),
  dev({ id: 'hdmi', name: 'XG32UCWG', kind: 'hdmi', form: 'display', detail: 'NVIDIA High Definition Audio' }),
  dev({ id: 'vad', name: 'Nahimic VAD', kind: 'virtual', detail: 'Virtual device' }),
]

const choices = (o: Partial<OutputChoices> = {}): OutputChoices => ({ devices, device: null, exclusive: true, playing: null, ...o })

test('devices are grouped by kind in the picker order, empty kinds left out', () => {
  const g = groups(devices)
  assert.deepEqual(
    g.map((x) => [x.title, x.devices.map((d) => d.name)]),
    [
      ['Bluetooth', ['AirPods Pro', 'soundcore Motion X600']],
      ['USB', ['SNOWSKY DISC']],
      ['Wired', ['Speakers']],
      ['HDMI and DisplayPort', ['XG32UCWG']],
      ['Virtual devices', ['Nahimic VAD']],
    ],
  )
})

test("a Bluetooth device is chosen and playing by either of its endpoints", () => {
  const pods = devices[1]
  assert.ok(isChosen(choices({ device: 'pods-hf' }), pods), 'chosen by its hands-free endpoint')
  assert.ok(isChosen(choices({ device: 'pods' }), pods))
  assert.ok(!isChosen(choices(), pods), 'following Windows chooses nothing')
  assert.ok(isPlaying(choices({ playing: 'pods' }), pods))
  assert.equal(current(choices({ device: 'pods-hf' }))?.id, 'pods')
})

test('Follow Windows: on chooses nothing, off keeps the device the music is on', () => {
  assert.equal(followChoice(choices({ device: 'dac' }), true), null)
  assert.equal(followChoice(choices({ playing: 'hdmi' }), false), 'hdmi')
  // Nothing played yet: Windows' default becomes the choice.
  assert.equal(followChoice(choices(), false), 'speakers')
  assert.equal(current(choices())?.id, 'speakers')
})

test('each device says what it is, its formats once known, and when it is away or hands-free only', () => {
  const c = choices()
  assert.equal(deviceLine(c, devices[0]), "Realtek(R) Audio, Windows' default")
  assert.equal(deviceLine(c, devices[3]), 'exclusive 44.1 to 192 kHz, up to 32-bit')
  assert.equal(deviceLine(choices({ exclusive: false }), devices[3]), '', 'formats only matter in exclusive mode')
  assert.equal(deviceLine(c, { ...devices[3], connected: false }), 'Not connected')
  assert.equal(deviceLine(c, { ...devices[1], hands_free_only: true }), 'Hands-free only (for calls): mono, low quality')
  assert.equal(formatsWords([48_000], 24), '48 kHz, up to 24-bit')
  assert.equal(formatsWords([], 0), '')
})

const sig = (o: Partial<PlayingSignal>): PlayingSignal => ({
  mode: 'exclusive',
  bit_perfect: true,
  output: { rate: 96_000, valid_bits: 24, container_bits: 32, kind: 'int' },
  source: { rate: 96_000 },
  latency_ms: 21,
  ...o,
})

test('the playing device says honestly whether it is bit-perfect, and why not', () => {
  assert.equal(stateLine(sig({})), 'Bit-perfect, exclusive, 24-bit, 96 kHz')
  assert.equal(stateLine(sig({ bit_perfect: false, source: { rate: 192_000 } })), 'Exclusive, 24-bit, 96 kHz, resampled')
  assert.equal(
    stateLine(sig({ mode: 'shared', bit_perfect: false, output: { rate: 48_000, valid_bits: 32, container_bits: 32, kind: 'float' } })),
    'Shared with other apps, 32-bit float, 48 kHz',
  )
  assert.equal(stateLine(sig({ mode: 'shared', bit_perfect: false, kind: 'bluetooth', output: { rate: 48_000, valid_bits: 32, container_bits: 32, kind: 'float' } })), 'Bluetooth, re-encoded by the link (48 kHz)')
  assert.equal(stateLine(null), 'Nothing playing')
})

test('the delay in words: measured by the device, and the extra set', () => {
  assert.equal(delayWords(74, null), '74 ms measured')
  assert.equal(delayWords(74.4, 170), '74 ms measured, plus 170 ms set')
  assert.equal(delayWords(74, 0), '74 ms measured, nothing extra set')
  assert.equal(delayWords(30, -20), '30 ms measured, 20 ms less set')
  assert.equal(delayWords(null, 170), 'Plus 170 ms set')
  assert.equal(delayWords(null, null), 'Not measured yet')
})

test('the Bluetooth suggestion is offered only while nothing is set, and the note warns when the sound is poor', () => {
  assert.deepEqual(suggestion(devices[1]), { ms: 170, says: 'Bluetooth is usually about 170 ms late' })
  assert.equal(suggestion(devices[2]), null, 'a value set: no suggestion')
  assert.equal(suggestion(devices[3]), null, 'not Bluetooth')
  assert.equal(noteOf(sig({ note: null })), null)
  assert.deepEqual(noteOf(sig({ note: "AirPods Pro is connected for calls only (hands-free: mono, 8 to 16 kHz), so music sounds poor." })), {
    text: "AirPods Pro is connected for calls only (hands-free: mono, 8 to 16 kHz), so music sounds poor.",
    warn: true,
  })
  assert.equal(noteOf(sig({ note: "Playing on AirPods Pro's stereo connection: its hands-free one is for calls (mono, 8 to 16 kHz)." }))?.warn, false)
  assert.equal(noteOf(sig({ note: "SNOWSKY DISC isn't connected: playing on Speakers until it's back." }))?.warn, true)
})
