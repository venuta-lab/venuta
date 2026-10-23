// Development only (devices-preview.html, served by `vite`, never part of a build): the output
// picker (components/player/OutputPicker.tsx) with the owner's kinds of devices as fixtures, in a
// plain browser, for screenshots (scripts/devices-shots.mjs). Nothing here reaches the engine.
//
//   /devices-preview.html?case=twin&theme=dark&view=popover
//
//   case   twin       a Bluetooth device chosen by its hands-free endpoint: its stereo one plays
//          handsfree  only the hands-free endpoint connected: plays, with the warning
//          dac        a USB DAC, exclusive, bit-perfect at 96 kHz
//          away       the chosen DAC unplugged: on the speakers until it is back
//          follow     following Windows' default (the laptop's speakers), shared mode
//   theme  dark | light
//   view   popover (the quality badge's, 360 wide) | settings (Settings > Playback's)

import type { OutputChoices, OutputDevice } from '../lib/outputPicker'
import type { Signal } from '../lib/player'

const q = new URLSearchParams(location.search)
const which = q.get('case') ?? 'twin'
const theme = q.get('theme') === 'light' ? 'light' : 'dark'
const view = q.get('view') ?? 'popover'

localStorage.clear()
localStorage.setItem('appearance', JSON.stringify({ theme }))

await import('@fontsource-variable/sora')
await import('@fontsource-variable/figtree')
await import('@fontsource-variable/inter')
await import('../index.css')
const [{ createRoot }, { createElement }, prefs, picker] = await Promise.all([
  import('react-dom/client'),
  import('react'),
  import('../lib/prefs'),
  import('../components/player/OutputPicker'),
])
prefs.applyPrefs()

const dev = (o: Partial<OutputDevice> & Pick<OutputDevice, 'id' | 'name' | 'kind'>): OutputDevice => ({
  detail: '',
  form: 'speakers',
  hands_free: null,
  hands_free_only: false,
  default: false,
  connected: true,
  delay_ms: null,
  suggested_delay_ms: o.kind === 'bluetooth' && !o.hands_free_only ? 170 : null,
  exclusive_suits: o.kind !== 'bluetooth' && o.kind !== 'virtual',
  exclusive_rates: [],
  exclusive_bits: 0,
  ...o,
})

const pods = dev({ id: 'pods', name: 'AirPods Pro', kind: 'bluetooth', form: 'headphones', hands_free: 'pods-hf' })
const fiio = dev({ id: 'fiio', name: 'FIIO EH11', kind: 'bluetooth', form: 'headphones', hands_free: 'fiio-hf' })
const x600 = dev({ id: 'x600', name: 'soundcore Motion X600', kind: 'bluetooth', hands_free: 'x600-hf', delay_ms: 180 })
const dac = dev({ id: 'dac', name: 'SNOWSKY DISC', kind: 'usb', form: 'headphones', exclusive_rates: [44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000], exclusive_bits: 32 })
const speakers = dev({ id: 'speakers', name: 'Speakers', kind: 'wired', detail: 'Realtek(R) Audio', default: true, exclusive_rates: [44_100, 48_000], exclusive_bits: 24 })
const hdmi = dev({ id: 'hdmi', name: 'XG32UCWG', kind: 'hdmi', form: 'display', detail: 'NVIDIA High Definition Audio' })
const mirror = dev({ id: 'mirror', name: 'Nahimic mirroring device', kind: 'virtual', detail: 'Virtual device' })
const vad = dev({ id: 'vad', name: 'Nahimic VAD', kind: 'virtual', detail: 'Virtual device' })

const signal = (o: Partial<Signal>): Signal => ({
  device: 'Speakers',
  mode: 'shared',
  source: { rate: 44_100, channels: 2, bits: 16, lossless: true, codec: 'FLAC' },
  file: { size_bytes: 31_000_000, bitrate_kbps: 980, decoder: 'Symphonia FLAC' },
  buffer_ms: 2000,
  latency_ms: 74,
  output: { rate: 48_000, channels: 2, container_bits: 32, valid_bits: 32, kind: 'float' },
  bit_perfect: false,
  steps: [],
  endpoint: 'speakers',
  kind: 'wired',
  note: null,
  delay_ms: null,
  suggested_delay_ms: null,
  follows_default: false,
  ...o,
})

const all = [pods, fiio, x600, dac, speakers, hdmi, mirror, vad]
const CASES: Record<string, { choices: OutputChoices; signal: Signal }> = {
  twin: {
    choices: { devices: all, device: 'pods-hf', exclusive: true, playing: 'pods' },
    signal: signal({
      device: 'AirPods Pro',
      endpoint: 'pods',
      kind: 'bluetooth',
      latency_ms: 112,
      suggested_delay_ms: 170,
      note: "Playing on AirPods Pro's stereo connection: its hands-free one is for calls (mono, 8 to 16 kHz).",
    }),
  },
  handsfree: {
    choices: { devices: [{ ...pods, id: 'pods-hf', hands_free: null, hands_free_only: true, suggested_delay_ms: null }, fiio, dac, speakers, hdmi, mirror], device: 'pods', exclusive: true, playing: 'pods-hf' },
    signal: signal({
      device: 'AirPods Pro',
      endpoint: 'pods-hf',
      kind: 'bluetooth',
      output: { rate: 16_000, channels: 1, container_bits: 16, valid_bits: 16, kind: 'int' },
      latency_ms: 64,
      note: "AirPods Pro is connected for calls only (hands-free: mono, 8 to 16 kHz), so music sounds poor. Reconnect it, or check its stereo (audio) service in Windows' Bluetooth settings.",
    }),
  },
  dac: {
    choices: { devices: all, device: 'dac', exclusive: true, playing: 'dac' },
    signal: signal({
      device: 'SNOWSKY DISC',
      endpoint: 'dac',
      kind: 'usb',
      mode: 'exclusive',
      bit_perfect: true,
      source: { rate: 96_000, channels: 2, bits: 24, lossless: true, codec: 'FLAC' },
      output: { rate: 96_000, channels: 2, container_bits: 32, valid_bits: 24, kind: 'int' },
      latency_ms: 21,
    }),
  },
  away: {
    choices: { devices: [pods, fiio, x600, { ...dac, connected: false, detail: 'Not connected' }, speakers, hdmi, mirror, vad], device: 'dac', exclusive: true, playing: 'speakers' },
    signal: signal({
      mode: 'exclusive',
      output: { rate: 44_100, channels: 2, container_bits: 32, valid_bits: 24, kind: 'int' },
      bit_perfect: true,
      latency_ms: 112,
      note: "SNOWSKY DISC isn't connected: playing on Speakers until it's back.",
    }),
  },
  follow: {
    choices: { devices: all, device: null, exclusive: false, playing: 'speakers' },
    signal: signal({ follows_default: true, delay_ms: 0 }),
  },
}
const c = CASES[which] ?? CASES.twin
if (which === 'follow') c.choices.devices = c.choices.devices.map((d) => (d.id === 'speakers' ? { ...d, delay_ms: 0 } : d))

const P = picker.OutputPicker
const root = createRoot(document.getElementById('root')!)
document.body.style.margin = '0'
root.render(
  view === 'settings'
    ? createElement(
        'div',
        { style: { padding: '24px 32px', maxWidth: 640 }, className: 'min-h-screen text-ink' },
        createElement('div', { className: 'text-[14px] font-medium' }, 'Play through'),
        createElement('div', { className: 'mt-3' }, createElement(P, { signal: c.signal, choices: c.choices, roomy: true })),
      )
    : createElement(
        'div',
        { style: { padding: 16 }, className: 'min-h-screen text-ink' },
        createElement(
          'div',
          { role: 'dialog', 'aria-label': 'Signal path', className: 'popover w-[360px] rounded-2xl p-4 text-[12.5px]' },
          createElement('div', { className: 'mb-1 text-[12px] text-ink/55' }, 'Play through'),
          createElement(P, { signal: c.signal, choices: c.choices }),
        ),
      ),
)
