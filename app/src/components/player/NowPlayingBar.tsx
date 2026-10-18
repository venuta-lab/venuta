import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useCovers } from '../../lib/covers'
import { useThemeInfo } from '../../lib/prefs'
import type { SeekBar as BarDesign } from '../../lib/themes'
import { LcdSpectrum } from './Spectrum'
import { openBigScreen } from '../../lib/lyricWindows'
import {
  clock,
  songLength,
  enqueueItems,
  toggleMiniPlayer,
  current,
  isOnline,
  next,
  outputChoices,
  previous,
  remoteArtUrl,
  seek,
  setOutput,
  setRepeat,
  setShuffle,
  setVolume,
  onlineQuality,
  sourceLabel,
  toggle,
  usePlayer,
  type OutputChoices,
  type PlayerState,
  type Signal,
  type Verification,
  useWaveform,
  verifyCurrent,
} from '../../lib/player'
import { artUrl, OnlineCover } from '../RemoteArt'
import { SteadyArt } from '../SteadyArt'
import { useDropTarget } from '../../lib/drag'
import { notify } from '../../lib/notify'
import { LikeButton } from '../LikeButton'
import { useLikes } from '../../lib/likes'
import {
  IconExpand,
  IconLyrics,
  IconMini,
  IconMute,
  IconNext,
  IconPause,
  IconPlay,
  IconPrevious,
  IconQueue,
  IconRepeat,
  IconRepeatOne,
  IconShuffle,
  IconVolume,
} from '../Icons'

const spring = { type: 'spring', stiffness: 420, damping: 34 } as const

const typing = (t: EventTarget | null) =>
  t instanceof HTMLElement && (t.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName))

/**
 * The song's loudness as one smooth shape, mirrored about the middle (Poweramp's waveform
 * seek bar): what has played in the accent, what's to come dimmed, and a glowing playhead.
 */
function Wave(props: { wave: number[]; pct: number; height: number }) {
  const path = useMemo(() => {
    const n = 160
    const h = props.height
    const mid = h / 2
    // Peaks folded to n points, then softened so the edge flows instead of stepping.
    const raw = Array.from({ length: n }, (_, i) => {
      const a = Math.floor((i * props.wave.length) / n)
      const b = Math.max(a + 1, Math.floor(((i + 1) * props.wave.length) / n))
      return Math.max(...props.wave.slice(a, b)) / 255
    })
    const soft = raw.map((_, i) => {
      let sum = 0
      let w = 0
      for (let k = -2; k <= 2; k++) {
        const v = raw[i + k]
        if (v === undefined) continue
        const weight = 3 - Math.abs(k)
        sum += v * weight
        w += weight
      }
      return sum / w
    })
    // Loud masters peak near full scale almost everywhere: stretch this song's own range
    // (quietest tenth to loudest) so its quiet and loud passages read as shape.
    const sorted = [...soft].sort((a, b) => a - b)
    const lo = sorted[Math.floor(n * 0.1)] ?? 0
    const hi = sorted[n - 1] || 1
    const amp = soft.map((v) => {
      const t = hi > lo ? Math.min(1, Math.max(0, (v - lo) / (hi - lo))) : 0.5
      return (0.12 + 0.88 * Math.pow(t, 1.4)) * (mid - 0.5)
    })
    const x = (i: number) => (i / (n - 1)) * 600
    // Top edge left to right as smooth curves through midpoints, then the mirror back.
    let d = `M0 ${mid} L0 ${mid - amp[0]}`
    for (let i = 1; i < n; i++) {
      const cx = x(i - 1)
      const cy = mid - amp[i - 1]
      d += ` Q${cx.toFixed(1)} ${cy.toFixed(2)} ${((cx + x(i)) / 2).toFixed(1)} ${(mid - (amp[i - 1] + amp[i]) / 2).toFixed(2)}`
    }
    d += ` L600 ${mid - amp[n - 1]} L600 ${mid + amp[n - 1]}`
    for (let i = n - 2; i >= 0; i--) {
      const cx = x(i + 1)
      const cy = mid + amp[i + 1]
      d += ` Q${cx.toFixed(1)} ${cy.toFixed(2)} ${((cx + x(i)) / 2).toFixed(1)} ${(mid + (amp[i + 1] + amp[i]) / 2).toFixed(2)}`
    }
    return d + ` L0 ${mid + amp[0]} Z`
  }, [props.wave, props.height])
  return (
    <>
      <svg viewBox={`0 0 600 ${props.height}`} preserveAspectRatio="none" className="absolute inset-0 h-full w-full" aria-hidden>
        <defs>
          <linearGradient id="wave-played" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="var(--accent)" stopOpacity="1" />
            <stop offset="0.5" stopColor="var(--accent)" stopOpacity="0.85" />
            <stop offset="1" stopColor="var(--accent)" stopOpacity="1" />
          </linearGradient>
        </defs>
        <path d={path} className="fill-ink/[0.18] transition-[fill] group-hover:fill-ink/25" />
        <path d={path} fill="url(#wave-played)" style={{ clipPath: `inset(0 ${100 - props.pct}% 0 0)` }} />
      </svg>
      {/* The playhead: a thin line with the accent's glow. */}
      <div
        className="pointer-events-none absolute -top-0.5 -bottom-0.5 w-[2px] -translate-x-1/2 rounded-full bg-ink shadow-[0_0_8px_var(--accent)]"
        style={{ left: `${props.pct}%` }}
      />
    </>
  )
}

/** The retro themes' seek bars (research §3): each its own track and playhead. */
export function ThemedTrack({ bar, pct }: { bar: BarDesign; pct: number }) {
  // The players people know: a thin line that thickens or lights up under the pointer.
  if (bar === 'pulse')
    return (
      <>
        <div className="absolute inset-x-0 top-1/2 h-[4px] -translate-y-1/2 rounded-full bg-[#4d4d4d]" />
        <div className="absolute left-0 top-1/2 h-[4px] -translate-y-1/2 rounded-full bg-white group-hover:bg-[#1ed760]" style={{ width: `${pct}%` }} />
        <span className="absolute top-1/2 h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white opacity-0 shadow-[0_2px_4px_rgb(0_0_0/0.5)] group-hover:opacity-100" style={{ left: `${pct}%` }} />
      </>
    )
  if (bar === 'slim')
    return (
      <>
        <div className="absolute inset-x-0 top-1/2 h-[4px] -translate-y-1/2 rounded-full bg-ink/20 transition-[height] group-hover:h-[6px]" />
        <div className="absolute left-0 top-1/2 h-[4px] -translate-y-1/2 rounded-full bg-ink/70 transition-[height] group-hover:h-[6px]" style={{ width: `${pct}%` }} />
      </>
    )
  if (bar === 'red')
    return (
      <>
        <div className="absolute inset-x-0 top-1/2 h-[3px] -translate-y-1/2 bg-white/20 transition-[height] group-hover:h-[5px]" />
        <div className="absolute left-0 top-1/2 h-[3px] -translate-y-1/2 bg-[#f03] transition-[height] group-hover:h-[5px]" style={{ width: `${pct}%` }} />
        <span className="absolute top-1/2 h-[13px] w-[13px] -translate-x-1/2 -translate-y-1/2 scale-0 rounded-full bg-[#f03] transition-transform group-hover:scale-100" style={{ left: `${pct}%` }} />
      </>
    )
  if (bar === 'segments') {
    const lit = Math.round((pct / 100) * 24)
    return (
      <div className="absolute inset-x-0 top-1/2 flex h-[10px] -translate-y-1/2 gap-[2px]">
        {Array.from({ length: 24 }, (_, i) => (
          <span
            key={i}
            className="flex-1"
            style={{ background: i < lit ? (pct > 90 ? '#f66e25' : '#8bd450') : 'rgb(246 110 37 / 0.14)' }}
          />
        ))}
      </div>
    )
  }
  if (bar === 'cursor')
    return (
      <>
        <div className="absolute inset-x-0 top-1/2 h-px bg-ink/25" />
        <div className="absolute left-0 top-1/2 h-px bg-accent" style={{ width: `${pct}%` }} />
        <span className="fx-blink absolute top-1/2 -translate-y-1/2 text-[15px] leading-none text-accent" style={{ left: `${pct}%` }}>
          ▌
        </span>
      </>
    )
  if (bar === 'hud')
    return (
      <>
        <div className="absolute inset-x-0 top-1/2 h-[2px] -translate-y-1/2 bg-ink/15" />
        {Array.from({ length: 11 }, (_, i) => (
          <span key={i} className="absolute top-1/2 h-[6px] w-px -translate-y-1/2 bg-ink/30" style={{ left: `${i * 10}%` }} />
        ))}
        <div className="absolute left-0 top-1/2 h-[2px] -translate-y-1/2 bg-accent shadow-[0_0_8px_var(--accent)]" style={{ width: `${pct}%` }} />
        <span className="absolute top-1/2 h-[12px] w-px -translate-y-1/2 bg-accent" style={{ left: `${pct}%` }} />
      </>
    )
  if (bar === 'stamp')
    return (
      <>
        <div className="absolute inset-x-0 top-1/2 h-[6px] -translate-y-1/2 border-2 border-ink" />
        <div className="absolute left-0 top-1/2 h-[6px] -translate-y-1/2 bg-accent" style={{ width: `${pct}%` }} />
        <span className="absolute top-1/2 h-3 w-3 -translate-x-1/2 -translate-y-1/2 border-2 border-ink bg-[var(--app-bg)]" style={{ left: `${pct}%` }} />
      </>
    )
  if (bar === 'bevel')
    return (
      <>
        <div className="absolute inset-x-0 top-1/2 h-[6px] -translate-y-1/2 bg-[#14161c] shadow-[inset_1px_1px_0_#0d0f14,inset_-1px_-1px_0_#5a6070]" />
        <div className="absolute left-px top-1/2 h-[4px] -translate-y-1/2 bg-accent" style={{ width: `calc(${pct}% - 2px)` }} />
        <span
          className="absolute top-1/2 h-[10px] w-[28px] -translate-x-1/2 -translate-y-1/2 bg-[linear-gradient(#5a6070,#3a3f4c)] shadow-[inset_1px_1px_0_#8c93a6,inset_-1px_-1px_0_#14161c]"
          style={{ left: `${pct}%` }}
        />
      </>
    )
  // 'trail' (Neo-Tokyo): a red bar whose fill trails a fading streak behind the playhead.
  return (
    <>
      <div className="absolute inset-x-0 top-1/2 h-[4px] -translate-y-1/2 bg-ink/12" />
      <div
        className="absolute left-0 top-1/2 h-[4px] -translate-y-1/2"
        style={{ width: `${pct}%`, background: 'linear-gradient(90deg, var(--accent) 0, var(--accent) calc(100% - 40px), var(--trail-tip, #ff6a4a))' }}
      />
      <span className="absolute top-1/2 h-[6px] w-[14px] -translate-x-1/2 -translate-y-1/2 bg-[var(--accent-ink)]" style={{ left: `${pct}%`, boxShadow: '0 0 12px var(--trail-glow, rgb(255 42 42 / 0.6))' }} />
    </>
  )
}

export function SeekBar(props: {
  position: number
  duration: number
  className?: string
  wave?: number[] | null
  waveHeight?: number
  /** Still opening (an online song): shown, but not draggable until it plays. */
  disabled?: boolean
}) {
  const design = useThemeInfo().seekBar
  const bar = useRef<HTMLDivElement>(null)
  const [drag, setDrag] = useState<number | null>(null)
  const [hover, setHover] = useState<number | null>(null)
  const at = (x: number) => {
    const r = bar.current?.getBoundingClientRect()
    if (!r || !props.duration || props.disabled) return 0
    return Math.min(1, Math.max(0, (x - r.left) / r.width)) * props.duration
  }
  // The bar's width in device pixels: the playhead moves a pixel at a time, so it is drawn again
  // only when it reaches the next one (a song's playhead crawls a few pixels a second; the
  // player reports ten times a second).
  const [px, setPx] = useState(0)
  useLayoutEffect(() => {
    const el = bar.current
    if (!el) return
    const read = () => setPx(Math.round(el.getBoundingClientRect().width * window.devicePixelRatio))
    read()
    const ro = new ResizeObserver(read)
    ro.observe(el)
    return () => ro.disconnect()
  }, [])
  const shown = drag ?? props.position
  const exact = props.duration ? Math.min(100, (shown / props.duration) * 100) : 0
  const pct = drag !== null || px < 2 ? exact : (Math.round((exact / 100) * px) / px) * 100
  return (
    <div className={`flex items-center gap-3 text-[11.5px] tabular-nums text-ink/55 ${props.className ?? ''}`}>
      <span className="readout w-10 text-right">{clock(shown)}</span>
      <div
        ref={bar}
        role="slider"
        aria-label="Position in the song"
        aria-valuemin={0}
        aria-valuemax={Math.round(props.duration / 1000)}
        aria-valuenow={Math.round(shown / 1000)}
        aria-valuetext={props.disabled ? 'Opening…' : props.duration ? `${clock(shown)} of ${clock(props.duration)}` : clock(shown)}
        aria-disabled={props.disabled || undefined}
        title={props.disabled ? 'Opening…' : undefined}
        tabIndex={0}
        onKeyDown={(e) => {
          if (props.disabled) return
          if (e.key === 'ArrowRight') void seek(props.position + 5000)
          if (e.key === 'ArrowLeft') void seek(props.position - 5000)
        }}
        onPointerDown={(e) => {
          if (props.disabled) return
          ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
          setDrag(at(e.clientX))
        }}
        onPointerMove={(e) => {
          if (props.disabled) return
          setHover(at(e.clientX))
          if (drag !== null) setDrag(at(e.clientX))
        }}
        onPointerUp={(e) => {
          if (drag !== null) void seek(at(e.clientX))
          setDrag(null)
        }}
        onPointerLeave={() => setHover(null)}
        className={`group relative flex-1 touch-none ${props.disabled ? 'cursor-progress opacity-50' : 'cursor-pointer'}`}
        style={{ height: design === 'wave' && props.wave ? (props.waveHeight ?? 22) : 20 }}
      >
        {design !== 'wave' ? (
          <ThemedTrack bar={design} pct={pct} />
        ) : props.wave ? (
          <Wave wave={props.wave} pct={pct} height={props.waveHeight ?? 22} />
        ) : (
          <div className="absolute inset-x-0 top-1/2 h-[4px] -translate-y-1/2 overflow-hidden rounded-full bg-ink/12 transition-[height] group-hover:h-[6px]">
            <div
              className={`h-full rounded-full bg-accent ${drag === null ? 'transition-[width] duration-150 ease-linear' : ''}`}
              style={{ width: `${pct}%` }}
            />
          </div>
        )}
        <div
          className={`absolute top-1/2 h-3.5 w-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-ink shadow transition-opacity ${props.wave || design !== 'wave' ? 'hidden' : ''} ${drag !== null ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'}`}
          style={{ left: `${pct}%` }}
        />
        {hover !== null && drag === null && props.duration > 0 && (
          <div
            className="popover pointer-events-none absolute -top-8 -translate-x-1/2 rounded-lg px-2 py-0.5 text-[11px] text-ink/85"
            style={{ left: `${(hover / props.duration) * 100}%` }}
          >
            {clock(hover)}
          </div>
        )}
      </div>
      <span className="readout w-10">{songLength(props.duration)}</span>
    </div>
  )
}

function Volume(props: { volume: number }) {
  const [before, setBefore] = useState(1)
  const muted = props.volume === 0
  return (
    <div
      className="group/vol relative flex items-center gap-2"
      onWheel={(e) => void setVolume(props.volume + (e.deltaY < 0 ? 0.05 : -0.05))}
      title="Volume (Ctrl ↑ / ↓). At 100% nothing changes the sound."
    >
      <button
        aria-label={muted ? 'Unmute' : 'Mute'}
        onClick={() => {
          if (muted) void setVolume(before || 1)
          else {
            setBefore(props.volume)
            void setVolume(0)
          }
        }}
        className="grid h-8 w-8 place-items-center rounded-full text-ink/70 hover:bg-ink/10 hover:text-ink"
      >
        {muted ? <IconMute /> : <IconVolume />}
      </button>
      <input
        type="range"
        min={0}
        max={100}
        value={Math.round(props.volume * 100)}
        onChange={(e) => void setVolume(Number(e.target.value) / 100)}
        aria-label="Volume"
        className="hidden h-1 w-24 accent-[var(--accent)] @[1000px]:block"
      />
      <div className="popover absolute bottom-full right-0 mb-2 hidden rounded-xl px-3 py-2.5 group-hover/vol:block group-focus-within/vol:block @[1000px]:!hidden">
        <input
          type="range"
          min={0}
          max={100}
          value={Math.round(props.volume * 100)}
          onChange={(e) => void setVolume(Number(e.target.value) / 100)}
          aria-label="Volume"
          className="block h-1 w-28 accent-[var(--accent)]"
        />
      </div>
    </div>
  )
}

/** "FLAC 24/96, Bit-perfect": opens the signal path and where the sound goes. */
/** The sound in a word or two, and the way to where it goes. Your file: "FLAC 24/96". An online
 *  song: "Online · AAC 129 kbps", the one Online label it gets here (just "Online" until it opens). */
function SignalBadge(props: { signal: Signal | null; online: boolean; fallback: string }) {
  const [open, setOpen] = useState(false)
  const [choices, setChoices] = useState<OutputChoices | null>(null)
  const box = useRef<HTMLDivElement>(null)
  const s = props.signal
  useEffect(() => {
    if (!open) return
    void outputChoices().then(setChoices)
    const close = (e: PointerEvent) => !box.current?.contains(e.target as Node) && setOpen(false)
    window.addEventListener('pointerdown', close)
    return () => window.removeEventListener('pointerdown', close)
  }, [open])
  const label = props.online ? ['Online', onlineQuality(s)].filter(Boolean).join(' · ') : s ? sourceLabel(s.source) : props.fallback
  return (
    <div ref={box} className="relative">
      <button
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className="flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-full bg-ink/[0.07] px-2 py-[3px] text-[10.5px] font-medium leading-none text-ink/70 transition-colors hover:bg-ink/[0.13] hover:text-ink"
        title={s ? `${label}, ${s.bit_perfect ? 'bit-perfect' : s.mode === 'shared' ? 'shared mode' : 'changed on the way'}. Where the sound goes.` : 'Where the sound goes'}
      >
        {s && <span className={`keep-round h-1.5 w-1.5 rounded-full ${s.bit_perfect ? 'bg-good' : 'bg-fix'}`} aria-hidden />}
        {props.online ? (
          <span className="readout">
            <span className="text-[10px] font-semibold uppercase tracking-[0.04em] text-accent-ink">Online</span>
            {label.slice('Online'.length)}
          </span>
        ) : (
          <span className="readout">{label}</span>
        )}
        {s && <span className="hidden font-normal text-ink/50 @[1240px]:inline">{s.bit_perfect ? 'Bit-perfect' : s.mode === 'shared' ? 'Shared' : 'Changed'}</span>}
      </button>
      <AnimatePresence>
        {open && (
          <motion.div
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 8 }}
            transition={{ duration: 0.16 }}
            role="dialog"
            aria-label="Signal path"
            className="popover absolute bottom-9 left-0 z-50 w-[340px] rounded-2xl p-4 text-[12.5px]"
          >
            <SignalPath signal={s} />
            {choices && (
              <div className="mt-4 border-t border-edge pt-3">
                <div className="mb-2 text-[12px] text-ink/55">Play through</div>
                {[{ id: null as string | null, name: 'Windows default', default: false }, ...choices.devices].map((d) => (
                  <button
                    key={d.id ?? 'default'}
                    onClick={() => {
                      void setOutput(d.id, choices.exclusive)
                      setChoices({ ...choices, device: d.id })
                    }}
                    className={`flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left hover:bg-ink/[0.06] ${choices.device === d.id ? 'text-accent-ink' : 'text-ink/80'}`}
                  >
                    <span className="flex-1 truncate">{d.name}</span>
                    {choices.device === d.id && <span aria-hidden>✓</span>}
                  </button>
                ))}
                <label className="mt-2 flex cursor-pointer items-start gap-2 rounded-lg px-2 py-1.5 hover:bg-ink/[0.06]">
                  <input
                    type="checkbox"
                    checked={choices.exclusive}
                    onChange={(e) => {
                      void setOutput(choices.device, e.target.checked)
                      setChoices({ ...choices, exclusive: e.target.checked })
                    }}
                    className="mt-0.5 accent-[var(--accent)]"
                  />
                  <span>
                    <span className="block text-ink/85">Exclusive (bit-perfect)</span>
                    <span className="block text-[11.5px] text-ink/50">
                      Each song at its own rate and depth, untouched. Other apps can't play sound meanwhile.
                    </span>
                  </span>
                </label>
              </div>
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}

/** From the file to the device, step by step. */
export function SignalPath(props: { signal: Signal | null }) {
  const s = props.signal
  const [check, setCheck] = useState<{ state: 'running' } | { state: 'done'; v: Verification } | { state: 'error'; message: string } | null>(null)
  if (!s) return <p className="text-ink/55">Press play to see each step the sound takes, from the file to your speakers.</p>
  const out = s.output
  const khz = (r: number) => `${r % 1000 ? (r / 1000).toFixed(1) : r / 1000} kHz`
  const channels = s.source.channels === 1 ? 'mono' : s.source.channels === 2 ? 'stereo' : `${s.source.channels} channels`
  const mb = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(2)} GB` : `${(b / 1e6).toFixed(1)} MB`)
  const outFormat =
    out.kind === 'float'
      ? `32-bit float, ${khz(out.rate)}`
      : `${out.valid_bits}-bit${out.container_bits !== out.valid_bits ? ` in ${out.container_bits}` : ''}, ${khz(out.rate)}`
  const stages: { name: string; lines: string[]; warn?: boolean }[] = [
    {
      name: 'File',
      lines: [
        `${s.source.codec}${s.source.lossless ? `, ${s.source.bits}-bit` : ''}, ${khz(s.source.rate)}, ${channels}`,
        [s.file.bitrate_kbps ? `${s.file.bitrate_kbps.toLocaleString('en-IN')} kbit/s` : '', s.file.size_bytes ? mb(s.file.size_bytes) : '']
          .filter(Boolean)
          .join(', '),
      ].filter(Boolean),
    },
    { name: 'Decoder', lines: [s.file.decoder, `${(s.buffer_ms / 1000).toFixed(1)} s decoded ahead`] },
    {
      name: 'Processing',
      lines: s.steps.length ? s.steps : ['Nothing changes the samples'],
      warn: s.steps.length > 0,
    },
    {
      name: 'Output',
      lines: [
        `${s.mode === 'exclusive' ? 'WASAPI exclusive' : s.mode === 'shared' ? 'WASAPI shared (Windows mixer)' : 'Memory'}`,
        `${outFormat}${s.latency_ms ? `, ${s.latency_ms.toFixed(1)} ms latency` : ''}`,
      ],
    },
    { name: 'Device', lines: [s.device] },
  ]
  return (
    <div>
      <div className={`text-[14px] font-medium ${s.bit_perfect ? 'text-good' : 'text-ink'}`}>
        {s.bit_perfect ? 'Bit-perfect: the device gets the exact samples in the file' : 'The sound is changed on the way'}
      </div>
      <ol className="mt-3">
        {stages.map((st, i) => (
          <li key={st.name} className="relative flex gap-3 pb-3 last:pb-0">
            {/* The chain: a dot per stage, joined by a line. */}
            <span className="relative flex w-3 shrink-0 justify-center" aria-hidden>
              <span className={`mt-1.5 h-2.5 w-2.5 rounded-full ${st.warn ? 'bg-fix' : i === stages.length - 1 && s.bit_perfect ? 'bg-good' : 'bg-ink/35'}`} />
              {i < stages.length - 1 && <span className="absolute top-5 bottom-[-2px] w-px bg-ink/15" />}
            </span>
            <span className="min-w-0">
              <span className={`block text-[11.5px] ${st.warn ? 'text-fix' : 'text-ink/45'}`}>{st.name}</span>
              {st.lines.map((l) => (
                <span key={l} className="block text-ink/85">
                  {l}
                </span>
              ))}
            </span>
          </li>
        ))}
      </ol>
      {s.source.codec === 'FLAC' && (
        <div className="mt-4 rounded-xl bg-ink/[0.05] px-3 py-2.5 text-[12px]">
          {!check && (
            <button
              onClick={() => {
                setCheck({ state: 'running' })
                verifyCurrent()
                  .then((v) => setCheck({ state: 'done', v }))
                  .catch((e) => setCheck({ state: 'error', message: e instanceof Error ? e.message : String(e) }))
              }}
              className="text-accent-ink hover:underline"
            >
              Check that this song comes out exactly as stored
            </button>
          )}
          {check?.state === 'running' && <span className="text-ink/60">Playing it through into memory and comparing…</span>}
          {check?.state === 'done' &&
            (check.v.bit_perfect ? (
              <span className="text-good">
                Verified: every sample matches the fingerprint stored in the file (MD5 {check.v.file_md5.slice(0, 8)}…).
              </span>
            ) : (
              <span className="text-problem">The samples don't match the file's fingerprint; the file may be damaged.</span>
            ))}
          {check?.state === 'error' && <span className="text-ink/60">{check.message}</span>}
        </div>
      )}
    </div>
  )
}

function Round(props: { label: string; onClick: () => void; on?: boolean; children: React.ReactNode }) {
  return (
    <motion.button
      whileTap={{ scale: 0.9 }}
      aria-label={props.label}
      aria-pressed={props.on}
      title={props.label}
      onClick={props.onClick}
      className={`relative grid h-8 w-8 place-items-center rounded-full transition-[background-color,color,transform] duration-150 hover:bg-ink/10 active:scale-90 ${props.on ? 'text-accent-ink' : 'text-ink/70 hover:text-ink'}`}
    >
      {props.children}
      {props.on && <span className="absolute bottom-0.5 h-1 w-1 rounded-full bg-accent" aria-hidden />}
    </motion.button>
  )
}

/** Space, arrows, Ctrl+arrows, L, Q: the player from the keyboard, from any screen. */
/** The volume before muting with M, to go back to. */
let lastVolume = 1

function useShortcuts(s: PlayerState, open: { lyrics: () => void; queue: () => void; now: () => void }) {
  const ref = useRef({ s, open })
  useEffect(() => {
    ref.current = { s, open }
  })
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // A key something else already took (Organize's Space plays its clips).
      if (e.defaultPrevented || typing(e.target) || e.altKey || e.metaKey) return
      const { s, open } = ref.current
      if (s.index < 0) return
      if (e.key === ' ' && !e.ctrlKey) {
        // Space on a focused button presses it; only take it when nothing else would.
        if (e.target instanceof HTMLButtonElement) return
        e.preventDefault()
        void toggle()
      } else if (e.key === 'ArrowRight' && e.ctrlKey) {
        e.preventDefault()
        void next()
      } else if (e.key === 'ArrowLeft' && e.ctrlKey) {
        e.preventDefault()
        void previous()
      } else if (e.key === 'ArrowUp' && e.ctrlKey) {
        e.preventDefault()
        void setVolume(s.volume + 0.05)
      } else if (e.key === 'ArrowDown' && e.ctrlKey) {
        e.preventDefault()
        void setVolume(s.volume - 0.05)
      } else if ((e.key === 'ArrowRight' || e.key === 'ArrowLeft') && !e.ctrlKey && document.activeElement === document.body) {
        e.preventDefault()
        void seek(s.position_ms + (e.key === 'ArrowRight' ? 1 : -1) * (e.shiftKey ? 30_000 : 5_000))
      } else if (!e.ctrlKey && (e.key === 'l' || e.key === 'L')) {
        open.lyrics()
      } else if (!e.ctrlKey && (e.key === 'q' || e.key === 'Q')) {
        open.queue()
      } else if (e.ctrlKey && (e.key === 'm' || e.key === 'M')) {
        e.preventDefault()
        void toggleMiniPlayer()
      } else if (e.key === 'F11') {
        // The big screen finds the words online too, so every song has it.
        e.preventDefault()
        void openBigScreen()
      } else if (e.ctrlKey && !e.shiftKey && (e.key === 's' || e.key === 'S')) {
        e.preventDefault()
        void setShuffle(!s.shuffle)
      } else if (e.ctrlKey && !e.shiftKey && (e.key === 'r' || e.key === 'R')) {
        e.preventDefault()
        void setRepeat(s.repeat === 'off' ? 'all' : s.repeat === 'all' ? 'one' : 'off')
      } else if (!e.ctrlKey && (e.key === 'm' || e.key === 'M')) {
        // Mute, and back to the volume it had.
        if (s.volume > 0) {
          lastVolume = s.volume
          void setVolume(0)
        } else void setVolume(lastVolume || 1)
      } else if (e.ctrlKey && !e.shiftKey && (e.key === 'e' || e.key === 'E')) {
        e.preventDefault()
        open.now()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])
}

/**
 * The bar at the bottom of every screen once something has played: the song, the controls,
 * where you are in it, the volume, and how it reaches your ears.
 */
export function NowPlayingBar(props: {
  onExpand: () => void
  onQueue: () => void
  onLyrics: () => void
  onArtist: (name: string) => void
  queueOpen: boolean
  expanded: boolean
}) {
  const s = usePlayer()
  // Hearts on the playing song are right wherever the window opens.
  useLikes(s.root ?? '')
  const song = current(s)
  const cover = useCovers(320)
  useShortcuts(s, { lyrics: props.onLyrics, queue: props.onQueue, now: props.onExpand })
  const info = useThemeInfo()
  const localArt = song && !isOnline(song) ? cover(song.dir) : undefined
  const onlineArt = remoteArtUrl(song)
  const duration = s.duration_ms || song?.duration_ms || 0
  const wave = useWaveform(song?.path)
  // Music dropped on the bar plays next.
  const drop = useDropTarget(
    song
      ? {
          label: 'Play next',
          drop: async (c) => {
            await enqueueItems(s.root ?? '', await c.songs(), true)
            notify(`${c.label} plays next.`)
          },
        }
      : null,
  )
  return (
    <AnimatePresence initial={false}>
      {song && (
        <motion.section
          {...drop.props}
          aria-label="Now playing"
          initial={{ y: 90, opacity: 0 }}
          animate={{ y: 0, opacity: 1 }}
          exit={{ y: 90, opacity: 0 }}
          transition={spring}
          className="np np-bar @container relative mx-3 mb-3 grid grid-cols-[minmax(0,1fr)_minmax(280px,1.5fr)_minmax(0,1fr)] items-center gap-4 px-4 py-3 @[1100px]:gap-6"
        >
          {/* Shown large in Now Playing while that is open, so it steps aside here. */}
          <motion.div
            animate={{ opacity: props.expanded ? 0 : 1, x: props.expanded ? -8 : 0 }}
            transition={{ duration: 0.2 }}
            className={`flex min-w-0 items-center gap-3 ${props.expanded ? 'pointer-events-none' : ''}`}
            aria-hidden={props.expanded}
          >
            <button
              onClick={props.onExpand}
              aria-label="Open Now Playing"
              className="group relative h-14 w-14 shrink-0 overflow-hidden rounded-xl bg-ink/[0.08] shadow-md"
            >
              {!props.expanded && (
                <motion.div layoutId="now-playing-cover" className="grid h-full w-full place-items-center" transition={spring}>
                  {/* Held steady across a change of song (no blank frame); online or local alike. */}
                  <SteadyArt src={onlineArt ? artUrl(onlineArt, 56) : localArt?.src} fallback={isOnline(song) ? <OnlineCover seed={song.album || song.title} /> : <span className="text-ink/30">♪</span>} />
                </motion.div>
              )}
              <span className="absolute inset-0 grid place-items-center bg-black/40 text-white opacity-0 transition-opacity group-hover:opacity-100">
                <IconExpand />
              </span>
            </button>
            <div className="min-w-0">
              <div className="flex min-w-0 items-center gap-2">
                <span className="truncate text-[14px] font-medium">{song.title}</span>
                <SignalBadge signal={s.signal} online={isOnline(song)} fallback={song.format} />
              </div>
              <div className="truncate text-[12.5px] text-ink/60">
                {s.buffering ? (
                  <span className="inline-flex items-center gap-1.5 text-ink/55">
                    <span className="spin h-2.5 w-2.5 rounded-full border-2 border-ink/20 border-t-ink/55" aria-hidden />
                    Opening…
                  </span>
                ) : song.artists.length ? (
                  song.artists.map((a, i) => (
                    <span key={a}>
                      {i > 0 && ', '}
                      <button onClick={() => props.onArtist(a)} className="hover:text-ink hover:underline">
                        {a}
                      </button>
                    </span>
                  ))
                ) : (
                  song.album_artist
                )}
              </div>
            </div>
            {s.root && <LikeButton root={s.root} item={song} />}
          </motion.div>

          <div className="flex min-w-0 flex-col items-center gap-1">
            <div className="flex items-center gap-2">
              <Round label={s.shuffle ? 'Shuffle is on' : 'Shuffle'} on={s.shuffle} onClick={() => void setShuffle(!s.shuffle)}>
                <IconShuffle />
              </Round>
              <Round label="Previous (Ctrl ←)" onClick={() => void previous()}>
                <IconPrevious />
              </Round>
              <motion.button
                whileTap={{ scale: 0.92 }}
                onClick={() => void toggle()}
                aria-label={s.buffering ? 'Opening…' : s.playing ? 'Pause (Space)' : 'Play (Space)'}
                title={s.buffering ? 'Opening the stream…' : s.playing ? 'Pause (Space)' : 'Play (Space)'}
                className="np-play accent-fill grid h-10 w-10 place-items-center rounded-full text-on-accent"
              >
                <AnimatePresence mode="wait" initial={false}>
                  <motion.span
                    key={s.buffering ? 'buf' : s.playing ? 'pause' : 'play'}
                    initial={{ scale: 0.6, opacity: 0 }}
                    animate={{ scale: 1, opacity: 1 }}
                    exit={{ scale: 0.6, opacity: 0 }}
                    transition={{ duration: 0.12 }}
                    className="grid place-items-center"
                  >
                    {s.buffering ? (
                      <span className="spin h-4 w-4 rounded-full border-2 border-on-accent/30 border-t-on-accent" aria-hidden />
                    ) : s.playing ? (
                      <IconPause />
                    ) : (
                      <IconPlay />
                    )}
                  </motion.span>
                </AnimatePresence>
              </motion.button>
              <Round label="Next (Ctrl →)" onClick={() => void next()}>
                <IconNext />
              </Round>
              <Round
                label={s.repeat === 'off' ? 'Repeat' : s.repeat === 'all' ? 'Repeating all' : 'Repeating this song'}
                on={s.repeat !== 'off'}
                onClick={() => void setRepeat(s.repeat === 'off' ? 'all' : s.repeat === 'all' ? 'one' : 'off')}
              >
                {s.repeat === 'one' ? <IconRepeatOne /> : <IconRepeat />}
              </Round>
            </div>
            <SeekBar position={s.position_ms} duration={duration} wave={wave} disabled={s.buffering} className="w-full max-w-[560px]" />
          </div>

          <div className="flex min-w-0 items-center justify-end gap-2">
            {info.lcd && <LcdSpectrum className="hidden h-9 w-[132px] shrink-0 @[1180px]:flex" />}
            {song.lyrics && (
              <Round label="Lyrics (L)" onClick={props.onLyrics}>
                <IconLyrics />
              </Round>
            )}
            <Round label="Queue (Q)" on={props.queueOpen} onClick={props.onQueue}>
              <IconQueue />
            </Round>
            <span className="hidden @[900px]:contents">
            <Round label="Mini player (Ctrl M)" onClick={() => void toggleMiniPlayer()}>
              <IconMini />
            </Round>
            </span>
            <Volume volume={s.volume} />
          </div>
          {s.error && (
            <div className="absolute -top-7 left-4 right-4 truncate text-center text-[12px] text-problem" role="status">
              {s.error}
            </div>
          )}
        </motion.section>
      )}
    </AnimatePresence>
  )
}
