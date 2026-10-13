import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useCovers } from '../../lib/covers'
import {
  clock,
  current,
  next,
  outputChoices,
  previous,
  seek,
  setOutput,
  setRepeat,
  setShuffle,
  setVolume,
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
import {
  IconExpand,
  IconLyrics,
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

/** Drag or click to move through the song; hover shows where you'd land. */
/** The song's shape: bars, dim ahead and accent-coloured behind where you are. */
function Wave(props: { wave: number[]; pct: number; height: number }) {
  // About one bar per 4 px of a 600 px bar: fold the 600 peaks to 150.
  const bars = useMemo(() => {
    const n = 150
    return Array.from({ length: n }, (_, i) => {
      const a = Math.floor((i * props.wave.length) / n)
      const b = Math.max(a + 1, Math.floor(((i + 1) * props.wave.length) / n))
      return Math.max(...props.wave.slice(a, b)) / 255
    })
  }, [props.wave])
  const shape = (
    <>
      {bars.map((v, i) => {
        const h = Math.max(0.08, v) * props.height
        return <rect key={i} x={i * 4 + 0.5} y={(props.height - h) / 2} width={2.6} height={h} rx={1.3} />
      })}
    </>
  )
  return (
    <svg viewBox={`0 0 600 ${props.height}`} preserveAspectRatio="none" className="absolute inset-0 h-full w-full" aria-hidden>
      <g className="fill-ink/20">{shape}</g>
      <g className="fill-accent" style={{ clipPath: `inset(0 ${100 - props.pct}% 0 0)` }}>
        {shape}
      </g>
    </svg>
  )
}

export function SeekBar(props: { position: number; duration: number; className?: string; wave?: number[] | null; waveHeight?: number }) {
  const bar = useRef<HTMLDivElement>(null)
  const [drag, setDrag] = useState<number | null>(null)
  const [hover, setHover] = useState<number | null>(null)
  const at = (x: number) => {
    const r = bar.current?.getBoundingClientRect()
    if (!r || !props.duration) return 0
    return Math.min(1, Math.max(0, (x - r.left) / r.width)) * props.duration
  }
  const shown = drag ?? props.position
  const pct = props.duration ? Math.min(100, (shown / props.duration) * 100) : 0
  return (
    <div className={`flex items-center gap-3 text-[11.5px] tabular-nums text-ink/55 ${props.className ?? ''}`}>
      <span className="w-10 text-right">{clock(shown)}</span>
      <div
        ref={bar}
        role="slider"
        aria-label="Position in the song"
        aria-valuemin={0}
        aria-valuemax={Math.round(props.duration / 1000)}
        aria-valuenow={Math.round(shown / 1000)}
        aria-valuetext={`${clock(shown)} of ${clock(props.duration)}`}
        tabIndex={0}
        onKeyDown={(e) => {
          if (e.key === 'ArrowRight') void seek(props.position + 5000)
          if (e.key === 'ArrowLeft') void seek(props.position - 5000)
        }}
        onPointerDown={(e) => {
          ;(e.target as HTMLElement).setPointerCapture(e.pointerId)
          setDrag(at(e.clientX))
        }}
        onPointerMove={(e) => {
          setHover(at(e.clientX))
          if (drag !== null) setDrag(at(e.clientX))
        }}
        onPointerUp={(e) => {
          if (drag !== null) void seek(at(e.clientX))
          setDrag(null)
        }}
        onPointerLeave={() => setHover(null)}
        className="group relative flex-1 cursor-pointer touch-none"
        style={{ height: props.wave ? (props.waveHeight ?? 22) : 20 }}
      >
        {props.wave ? (
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
          className={`absolute top-1/2 h-3.5 w-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-ink shadow transition-opacity ${props.wave ? 'hidden' : ''} ${drag !== null ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'}`}
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
      <span className="w-10">{clock(props.duration)}</span>
    </div>
  )
}

function Volume(props: { volume: number }) {
  const [before, setBefore] = useState(1)
  const muted = props.volume === 0
  return (
    <div
      className="flex items-center gap-2"
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
        className="h-1 w-24 accent-[var(--accent)]"
      />
    </div>
  )
}

/** "FLAC 24/96, Bit-perfect": opens the signal path and where the sound goes. */
function SignalBadge(props: { signal: Signal | null; fallback: string }) {
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
  const label = s ? sourceLabel(s.source) : props.fallback
  return (
    <div ref={box} className="relative">
      <button
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className="flex items-center gap-1.5 whitespace-nowrap rounded-full bg-ink/[0.07] px-3 py-1 text-[11.5px] text-ink/75 hover:bg-ink/[0.12]"
        title="Where the sound goes"
      >
        {s && <span className={`h-1.5 w-1.5 rounded-full ${s.bit_perfect ? 'bg-good' : 'bg-fix'}`} aria-hidden />}
        <span>{label}</span>
        {s && <span className="text-ink/50">{s.bit_perfect ? 'Bit-perfect' : s.mode === 'shared' ? 'Shared' : 'Changed'}</span>}
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
            className="popover absolute bottom-11 right-0 z-50 w-[340px] rounded-2xl p-4 text-[12.5px]"
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
  if (!s) return <p className="text-ink/55">Play something to see where the sound goes.</p>
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
      className={`relative grid h-8 w-8 place-items-center rounded-full hover:bg-ink/10 ${props.on ? 'text-accent-ink' : 'text-ink/70 hover:text-ink'}`}
    >
      {props.children}
      {props.on && <span className="absolute bottom-0.5 h-1 w-1 rounded-full bg-accent" aria-hidden />}
    </motion.button>
  )
}

/** Space, arrows, Ctrl+arrows, L, Q: the player from the keyboard, from any screen. */
function useShortcuts(s: PlayerState, open: { lyrics: () => void; queue: () => void }) {
  const ref = useRef({ s, open })
  useEffect(() => {
    ref.current = { s, open }
  })
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (typing(e.target) || e.altKey || e.metaKey) return
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
  const song = current(s)
  const cover = useCovers(320)
  useShortcuts(s, { lyrics: props.onLyrics, queue: props.onQueue })
  const art = song ? cover(song.dir) : undefined
  const duration = s.duration_ms || song?.duration_ms || 0
  const wave = useWaveform(song?.path)
  return (
    <AnimatePresence initial={false}>
      {song && (
        <motion.section
          aria-label="Now playing"
          initial={{ y: 90, opacity: 0 }}
          animate={{ y: 0, opacity: 1 }}
          exit={{ y: 90, opacity: 0 }}
          transition={spring}
          className="glass relative z-30 mx-3 mb-3 grid grid-cols-[minmax(0,1fr)_minmax(0,1.5fr)_minmax(0,1fr)] items-center gap-6 rounded-[22px] px-4 py-3"
        >
          <div className="flex min-w-0 items-center gap-3">
            <button
              onClick={props.onExpand}
              aria-label="Open Now Playing"
              className="group relative h-14 w-14 shrink-0 overflow-hidden rounded-xl bg-ink/[0.08] shadow-md"
            >
              {!props.expanded && (
                <motion.div layoutId="now-playing-cover" className="h-full w-full" transition={spring}>
                  {art ? <img src={art.src} alt="" className="h-full w-full object-cover" draggable={false} /> : null}
                </motion.div>
              )}
              <span className="absolute inset-0 grid place-items-center bg-black/40 text-white opacity-0 transition-opacity group-hover:opacity-100">
                <IconExpand />
              </span>
            </button>
            <div className="min-w-0">
              <div className="truncate text-[14px] font-medium">{song.title}</div>
              <div className="truncate text-[12.5px] text-ink/60">
                {song.artists.length
                  ? song.artists.map((a, i) => (
                      <span key={a}>
                        {i > 0 && ', '}
                        <button onClick={() => props.onArtist(a)} className="hover:text-ink hover:underline">
                          {a}
                        </button>
                      </span>
                    ))
                  : song.album_artist}
              </div>
            </div>
          </div>

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
                aria-label={s.playing ? 'Pause (Space)' : 'Play (Space)'}
                title={s.playing ? 'Pause (Space)' : 'Play (Space)'}
                className="grid h-10 w-10 place-items-center rounded-full bg-accent text-on-accent shadow-lg"
              >
                <AnimatePresence mode="wait" initial={false}>
                  <motion.span
                    key={s.playing ? 'pause' : 'play'}
                    initial={{ scale: 0.6, opacity: 0 }}
                    animate={{ scale: 1, opacity: 1 }}
                    exit={{ scale: 0.6, opacity: 0 }}
                    transition={{ duration: 0.12 }}
                    className={s.playing ? '' : 'pl-0.5'}
                  >
                    {s.playing ? <IconPause /> : <IconPlay />}
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
            <SeekBar position={s.position_ms} duration={duration} wave={wave} className="w-full max-w-[560px]" />
          </div>

          <div className="flex min-w-0 items-center justify-end gap-2">
            <SignalBadge signal={s.signal} fallback={song.format} />
            {song.lyrics && (
              <Round label="Lyrics (L)" onClick={props.onLyrics}>
                <IconLyrics />
              </Round>
            )}
            <Round label="Queue (Q)" on={props.queueOpen} onClick={props.onQueue}>
              <IconQueue />
            </Round>
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
