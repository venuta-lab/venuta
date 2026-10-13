import { motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { albumSpectrogram, type AlbumSummary, type HealthReport } from '../lib/engine'

/*
  Small pieces several screens share: a job's progress bar with Stop, an album's spectrogram,
  the formats bar and a pill button.
*/
const spring = { type: 'spring', stiffness: 380, damping: 34 } as const

export function DeepProgressBar(props: {
  deep: { done: number; total: number; note?: string }
  label: string
  unit: string
  onCancel: () => void
}) {
  const { done, total } = props.deep
  const pct = total ? (done / total) * 100 : 0
  return (
    <motion.div
      initial={{ opacity: 0, height: 0 }}
      animate={{ opacity: 1, height: 'auto' }}
      exit={{ opacity: 0, height: 0 }}
      transition={spring}
      className="overflow-hidden"
    >
      <div className="glass mt-6 flex items-center gap-4 rounded-2xl px-5 py-3.5">
        <div className="flex-1">
          <div className="text-[13.5px] text-ink/85">
            {total ? `${props.label}: ${fmt(done)} of ${fmt(total)} ${props.unit}` : 'Getting ready…'}
          </div>
          {props.deep.note && (
            <motion.div key={props.deep.note} initial={{ opacity: 0, y: 3 }} animate={{ opacity: 1, y: 0 }} className="mt-0.5 truncate text-[12.5px] text-ink/55">
              {props.deep.note}
            </motion.div>
          )}
          <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-ink/10">
            <motion.div
              className="h-full rounded-full"
              style={{ background: 'var(--accent)' }}
              animate={{ width: `${pct}%` }}
              transition={{ type: 'spring', stiffness: 120, damping: 24 }}
            />
          </div>
        </div>
        <button onClick={props.onCancel} className="btn-ghost">
          Stop
        </button>
      </div>
    </motion.div>
  )
}

export function SpectrogramView(props: { root: string; album: AlbumSummary; onClose: () => void }) {
  const [result, setResult] = useState<{ dir: string; src?: string; error?: string } | null>(null)
  useEffect(() => {
    let live = true
    albumSpectrogram(props.root, props.album.dir).then(
      (src) => live && setResult({ dir: props.album.dir, src }),
      (e) => live && setResult({ dir: props.album.dir, error: e instanceof Error ? e.message : String(e) }),
    )
    return () => {
      live = false
    }
  }, [props.root, props.album.dir])
  const current = result?.dir === props.album.dir ? result : null
  const src = current?.src
  const error = current?.error
  return createPortal(
    <motion.div
      className="fixed inset-0 z-50 grid place-items-center p-10"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={props.onClose}
    >
      <div className="absolute inset-0 bg-black/50 backdrop-blur-md" />
      <motion.div
        onClick={(e) => e.stopPropagation()}
        initial={{ y: 24, scale: 0.98 }}
        animate={{ y: 0, scale: 1 }}
        exit={{ y: 16, scale: 0.98 }}
        transition={spring}
        className="popover relative w-full max-w-[1040px] rounded-[26px] p-7"
      >
        <h2 className="text-[22px] font-semibold tracking-[-0.02em]">{props.album.title}</h2>
        <p className="mt-1 text-[13.5px] text-ink/60">First track, from start to end.</p>
        <div className="mt-5 flex gap-3">
          <div className="flex flex-col justify-between py-1 text-right text-[11px] text-ink/45">
            <span>top of range</span>
            <span>0 Hz</span>
          </div>
          <div className="relative aspect-[960/360] flex-1 overflow-hidden rounded-xl bg-black/40">
            {src ? (
              <motion.img
                src={src}
                alt={`Spectrogram of ${props.album.title}`}
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                className="h-full w-full"
              />
            ) : (
              <div className="grid h-full place-items-center text-[13px] text-ink/55">
                {error || 'Drawing the spectrogram…'}
              </div>
            )}
          </div>
        </div>
        <p className="mt-5 max-w-[72ch] text-[13.5px] leading-relaxed text-ink/65">
          Time runs left to right; higher rows are higher frequencies, brighter is louder. A hard, flat
          edge with darkness above it means the sound was cut off there: typical of MP3 or AAC sources
          (around 16 to 20 kHz) or of upsampling (around 22 kHz in a 96 kHz file). A soft, uneven fade is
          usually just the music or an old master.
        </p>
      </motion.div>
    </motion.div>,
    document.body,
  )
}

export function FormatBar(props: { formats: HealthReport['summary']['formats']; total: number }) {
  const label = (f: { bit_depth: number; sample_rate: number }) =>
    `${f.bit_depth}-bit ${f.sample_rate / 1000} kHz`
  const shades = [0.9, 0.62, 0.42, 0.28, 0.18, 0.12]
  return (
    <div className="mt-4">
      <div className="flex h-2.5 overflow-hidden rounded-full bg-ink/5">
        {props.formats.map((f, i) => (
          <motion.div
            key={label(f)}
            initial={{ width: 0 }}
            animate={{ width: `${(f.tracks / props.total) * 100}%` }}
            transition={{ ...spring, delay: 0.1 + i * 0.05 }}
            style={{ background: `color-mix(in oklab, var(--accent) ${shades[i] * 100}%, transparent)` }}
          />
        ))}
      </div>
      <div className="mt-2.5 flex flex-wrap gap-x-5 gap-y-1 text-[12px] text-ink/60">
        {props.formats.map((f, i) => (
          <span key={label(f)} className="flex items-center gap-1.5">
            <span
              className="h-2 w-2 rounded-full"
              style={{ background: `color-mix(in oklab, var(--accent) ${shades[i] * 100}%, transparent)` }}
            />
            {label(f)} <span className="text-ink/40">{fmt(f.tracks)}</span>
          </span>
        ))}
      </div>
    </div>
  )
}

export function PillButton(props: {
  onClick: () => void
  disabled?: boolean
  accent?: boolean
  title?: string
  children: React.ReactNode
}) {
  return (
    <button onClick={props.onClick} disabled={props.disabled} title={props.title} className={`${props.accent ? 'btn-primary' : 'btn'} btn-lg`}>
      {props.children}
    </button>
  )
}

const fmt = (n: number) => n.toLocaleString()
