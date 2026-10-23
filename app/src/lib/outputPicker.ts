// The output picker's logic (docs/design/devices.md): the devices grouped by kind with their
// clear names, which one is chosen and which one plays, the playing device's state (bit-perfect
// or shared, its format) and its delay, in words. Pure, so node tests run it; the component is
// components/player/OutputPicker.tsx.

export type DeviceKind = 'bluetooth' | 'usb' | 'wired' | 'hdmi' | 'virtual' | 'other'
export type DeviceForm = 'speakers' | 'headphones' | 'headset' | 'line_out' | 'digital' | 'display' | 'other'

/** One device as the engine lists it (a Bluetooth device's stereo and hands-free endpoints are one). */
export interface OutputDevice {
  /** The endpoint it plays on (the stereo one of a Bluetooth device that has both). */
  id: string
  name: string
  detail: string
  kind: DeviceKind
  form: DeviceForm
  /** Its hands-free (call) endpoint, never played while the stereo one is here. */
  hands_free: string | null
  /** Only its hands-free endpoint is connected: mono, call quality. */
  hands_free_only: boolean
  /** Windows' default output is this device. */
  default: boolean
  connected: boolean
  /** The extra delay set for it, ms (null: not set). */
  delay_ms: number | null
  /** A starting value to offer while not set (Bluetooth). */
  suggested_delay_ms: number | null
  /** Exclusive mode suits it (never Bluetooth or a virtual device). */
  exclusive_suits: boolean
  /** The rates it takes in exclusive mode and its deepest bits, once asked (empty before). */
  exclusive_rates: number[]
  exclusive_bits: number
}

export interface OutputChoices {
  devices: OutputDevice[]
  /** The chosen device's endpoint; null follows Windows' default. */
  device: string | null
  exclusive: boolean
  /** The endpoint the music is on now, once something has played. */
  playing: string | null
}

/** What the picker reads of the player's signal path. */
export interface PlayingSignal {
  mode: 'exclusive' | 'shared' | 'memory'
  bit_perfect: boolean
  output: { rate: number; valid_bits: number; container_bits: number; kind: 'int' | 'float' }
  source: { rate: number }
  latency_ms: number
  endpoint?: string | null
  kind?: DeviceKind | null
  note?: string | null
  delay_ms?: number | null
  suggested_delay_ms?: number | null
  follows_default?: boolean
}

export const KIND_ORDER: DeviceKind[] = ['bluetooth', 'usb', 'wired', 'hdmi', 'virtual', 'other']

export const KIND_TITLE: Record<DeviceKind, string> = {
  bluetooth: 'Bluetooth',
  usb: 'USB',
  wired: 'Wired',
  hdmi: 'HDMI and DisplayPort',
  virtual: 'Virtual devices',
  other: 'Other',
}

export interface Group {
  kind: DeviceKind
  title: string
  devices: OutputDevice[]
}

/** The devices by kind, in the picker's order; a kind with none is left out. */
export function groups(devices: OutputDevice[]): Group[] {
  return KIND_ORDER.map((kind) => ({ kind, title: KIND_TITLE[kind], devices: devices.filter((d) => d.kind === kind) })).filter((g) => g.devices.length > 0)
}

/** The device's endpoints (its stereo one and its hands-free one). */
const ids = (d: OutputDevice) => [d.id, ...(d.hands_free ? [d.hands_free] : [])]

/** This is the device chosen (by either of its endpoints). */
export const isChosen = (c: OutputChoices, d: OutputDevice) => c.device !== null && ids(d).includes(c.device)

/** The music is on this device now. */
export const isPlaying = (c: OutputChoices, d: OutputDevice) => c.playing !== null && ids(d).includes(c.playing)

/** The device the music is on, or would be on (the chosen one, else Windows' default). */
export function current(c: OutputChoices): OutputDevice | undefined {
  return c.devices.find((d) => isPlaying(c, d)) ?? c.devices.find((d) => (c.device === null ? d.default : isChosen(c, d)))
}

/** The choice when "Follow Windows" is turned on (null) or off (the device the music is on stays chosen). */
export function followChoice(c: OutputChoices, on: boolean): string | null {
  if (on) return null
  return current(c)?.id ?? c.devices.find((d) => d.connected)?.id ?? null
}

const khz = (r: number) => `${r % 1000 ? (r / 1000).toFixed(1) : r / 1000} kHz`

/** "44.1 to 96 kHz, up to 24-bit" for a device asked for its exclusive formats; "" before. */
export function formatsWords(rates: number[], bits: number): string {
  if (!rates.length) return ''
  const lo = Math.min(...rates)
  const hi = Math.max(...rates)
  const span = lo === hi ? khz(lo) : `${khz(lo).replace(' kHz', '')} to ${khz(hi)}`
  return bits ? `${span}, up to ${bits}-bit` : span
}

/** The line under a device's name. */
export function deviceLine(c: OutputChoices, d: OutputDevice): string {
  if (!d.connected) return 'Not connected'
  if (d.hands_free_only) return 'Hands-free only (for calls): mono, low quality'
  const parts = [d.detail]
  if (d.default) parts.push("Windows' default")
  if (c.exclusive && d.exclusive_suits) {
    const f = formatsWords(d.exclusive_rates, d.exclusive_bits)
    if (f) parts.push(`exclusive ${f}`)
  }
  return parts.filter(Boolean).join(', ')
}

/** The playing device's state: bit-perfect or shared, and the format it runs at. */
export function stateLine(s: PlayingSignal | null): string {
  if (!s) return 'Nothing playing'
  const out = s.output
  const depth = out.kind === 'float' ? '32-bit float' : `${out.valid_bits}-bit`
  const fmt = `${depth}, ${khz(out.rate)}`
  if (s.kind === 'bluetooth') return `Bluetooth, re-encoded by the link (${khz(out.rate)})`
  if (s.kind === 'virtual') return `Virtual device, shared (${khz(out.rate)})`
  if (s.bit_perfect) return `Bit-perfect, exclusive, ${fmt}`
  if (s.mode === 'exclusive') return `Exclusive, ${fmt}${out.rate !== s.source.rate ? ', resampled' : ''}`
  if (s.mode === 'shared') return `Shared with other apps, ${fmt}`
  return fmt
}

/** The playing device's delay: what the device measured, and the extra set for it. */
export function delayWords(measuredMs: number | null, setMs: number | null): string {
  const parts: string[] = []
  if (measuredMs !== null && measuredMs > 0) parts.push(`${Math.round(measuredMs)} ms measured`)
  if (setMs !== null) parts.push(setMs === 0 ? 'nothing extra set' : setMs > 0 ? `plus ${setMs} ms set` : `${-setMs} ms less set`)
  if (!parts.length) return 'Not measured yet'
  const s = parts.join(', ')
  return s[0].toUpperCase() + s.slice(1)
}

/** The note to show above the list: what Venuta did about the choice (a twin, a fallback), and
 *  whether it is a warning (poor sound, the chosen device away). */
export function noteOf(s: PlayingSignal | null): { text: string; warn: boolean } | null {
  if (!s?.note) return null
  const warn = /calls only|isn't connected|isn't answering/.test(s.note)
  return { text: s.note, warn }
}

/** The extra delay to offer as a one-press start, with its words (never applied silently). */
export function suggestion(d: OutputDevice | undefined): { ms: number; says: string } | null {
  if (!d || d.delay_ms !== null || d.suggested_delay_ms === null) return null
  return { ms: d.suggested_delay_ms, says: `Bluetooth is usually about ${d.suggested_delay_ms} ms late` }
}
