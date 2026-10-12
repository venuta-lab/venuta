import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import {
  albumSpectrogram,
  SPECTRAL_ISSUES,
  type AlbumSummary,
  type HealthReport,
  type Issue,
  type Severity,
} from '../lib/engine'
import { setAccent, useCovers } from '../lib/covers'
import { IconChevron, IconFolder, IconRefresh } from '../components/Icons'

const spring = { type: 'spring', stiffness: 380, damping: 34 } as const

const SEVERITY: Record<Severity, { color: string; group: string }> = {
  problem: { color: 'var(--color-problem)', group: 'Problems' },
  fix: { color: 'var(--color-fix)', group: 'Worth fixing' },
  note: { color: 'var(--color-note)', group: 'Good to know' },
}

export function LibraryScreen(props: {
  root: string
  report: HealthReport | null
  busy: string
  deep: { done: number; total: number } | null
  lookup: { done: number; total: number } | null
  onLookup: () => void
  onCancelLookup: () => void
  onChoose: () => void
  onRescan: () => void
  onDeepCheck: () => void
  onCancelDeep: () => void
}) {
  const { root, report, busy, deep } = props
  const [spectrum, setSpectrum] = useState<AlbumSummary | null>(null)
  const name = root.split(/[\\/]/).filter(Boolean).pop() ?? root
  const [open, setOpen] = useState<string | null>(null)

  if (!root) return <EmptyState onChoose={props.onChoose} />

  const albumsByDir = new Map((report?.albums ?? []).map((a) => [a.dir, a]))
  const groups = (['problem', 'fix', 'note'] as Severity[])
    .map((s) => ({ s, issues: report?.issues.filter((i) => i.severity === s) ?? [] }))
    .filter((g) => g.issues.length)

  return (
    <div className="mx-auto max-w-[920px] pb-16">
      <header className="flex items-end gap-6 pt-6">
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-[56px] font-semibold leading-none tracking-[-0.035em]">{name}</h1>
          <p className="mt-4 max-w-[60ch] text-[15px] leading-relaxed text-ink/70">
            {report
              ? `${fmt(report.summary.tracks)} tracks in ${fmt(report.summary.albums)} albums. ${Math.round(
                  report.summary.hours,
                )} hours of music, ${(report.summary.bytes / 1e9).toFixed(1)} GB on disk.`
              : busy || 'Reading your library…'}
          </p>
        </div>
        <div className="flex shrink-0 gap-2 pb-1">
          <PillButton onClick={props.onChoose} title="Choose another folder">
            <IconFolder />
          </PillButton>
          <PillButton
            onClick={props.onLookup}
            disabled={!!busy || !!props.lookup}
            title="Find original years, composer and lyricist credits, sharper covers and missing lyrics online. Changes nothing until you approve it in Review."
          >
            Look up online
          </PillButton>
          <PillButton
            onClick={props.onDeepCheck}
            disabled={!!busy || !!deep}
            title="Decode every new or changed track: integrity, quality and loudness. Changes nothing."
          >
            Deep check
          </PillButton>
          <PillButton onClick={props.onRescan} disabled={!!busy} accent>
            <motion.span
              animate={busy ? { rotate: 360 } : { rotate: 0 }}
              transition={busy ? { repeat: Infinity, duration: 1, ease: 'linear' } : { duration: 0.3 }}
              className="grid"
            >
              <IconRefresh />
            </motion.span>
            {busy ? 'Scanning' : 'Rescan'}
          </PillButton>
        </div>
      </header>

      <AnimatePresence>
        {deep && <DeepProgressBar deep={deep} label="Deep check" unit="tracks" onCancel={props.onCancelDeep} />}
      </AnimatePresence>
      <AnimatePresence>
        {props.lookup && (
          <DeepProgressBar deep={props.lookup} label="Looking up online" unit="steps" onCancel={props.onCancelLookup} />
        )}
      </AnimatePresence>

      {report && <FormatBar formats={report.summary.formats} total={report.summary.tracks} />}

      {groups.map(({ s, issues }) => (
        <section key={s} className="mt-10">
          <h2 className="mb-3 flex items-center gap-2 text-[15px] font-medium text-ink/80">
            <span className="h-2 w-2 rounded-full" style={{ background: SEVERITY[s].color }} />
            {SEVERITY[s].group}
          </h2>
          <div className="flex flex-col gap-2">
            {issues.map((issue) => (
              <IssueRow
                key={issue.id}
                issue={issue}
                open={open === issue.id}
                onToggle={() => setOpen(open === issue.id ? null : issue.id)}
                albums={albumsByDir}
                onAlbum={SPECTRAL_ISSUES.has(issue.id) ? setSpectrum : undefined}
              />
            ))}
          </div>
        </section>
      ))}
      <AnimatePresence>
        {spectrum && <SpectrogramView root={root} album={spectrum} onClose={() => setSpectrum(null)} />}
      </AnimatePresence>
    </div>
  )
}

function IssueRow(props: {
  issue: Issue
  open: boolean
  onToggle: () => void
  albums: Map<string, AlbumSummary>
  onAlbum?: (a: AlbumSummary) => void
}) {
  const { issue, open } = props
  return (
    <motion.div layout transition={spring} className="glass overflow-hidden rounded-[18px]">
      <motion.button
        layout="position"
        onClick={props.onToggle}
        aria-expanded={open}
        className="flex w-full items-center gap-4 px-5 py-4 text-left"
      >
        <span
          className="h-9 w-1 shrink-0 rounded-full"
          style={{ background: SEVERITY[issue.severity].color, opacity: 0.85 }}
        />
        <span className="flex-1 text-[15px] font-medium">{issue.title}</span>
        <span className="text-[13px] text-ink/60">
          {fmt(issue.tracks)} tracks in {fmt(issue.albums.length)} albums
        </span>
        <motion.span animate={{ rotate: open ? 90 : 0 }} transition={spring} className="text-ink/50">
          <IconChevron />
        </motion.span>
      </motion.button>
      <AnimatePresence initial={false}>
        {open && (
          <motion.div
            initial={{ opacity: 0, y: -6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            transition={{ duration: 0.22 }}
            className="px-5 pb-5 pl-[45px]"
          >
            <p className="max-w-[64ch] text-[14px] leading-relaxed text-ink/70">{issue.why}</p>
            {issue.examples.length > 0 && (
              <div className="mt-3 flex flex-wrap gap-2">
                {issue.examples.slice(0, 6).map((e) => (
                  <span key={e} className="rounded-full bg-white/[0.07] px-3 py-1 text-[12px] text-ink/80">
                    {e}
                  </span>
                ))}
              </div>
            )}
            {props.onAlbum && (
              <p className="mt-3 text-[12.5px] text-ink/50">Click an album to see its spectrogram.</p>
            )}
            <CoverStrip dirs={issue.albums} albums={props.albums} onAlbum={props.onAlbum} />
          </motion.div>
        )}
      </AnimatePresence>
    </motion.div>
  )
}

/** Affected albums as a strip of covers you can drag sideways, like flicking through a crate. */
function CoverStrip(props: {
  dirs: string[]
  albums: Map<string, AlbumSummary>
  onAlbum?: (a: AlbumSummary) => void
}) {
  const cover = useCovers(320)
  const dirs = props.dirs.slice(0, 40)
  return (
    <div className="no-scrollbar -mx-2 mt-4 overflow-x-auto px-2 pb-1">
      <motion.div className="flex w-max gap-3">
        {dirs.map((d, i) => {
          const c = cover(d)
          const a = props.albums.get(d)
          return (
            <motion.div
              key={d}
              initial={{ opacity: 0, scale: 0.92 }}
              animate={{ opacity: 1, scale: 1 }}
              transition={{ ...spring, delay: Math.min(i, 12) * 0.025 }}
              whileHover={{ y: -4, scale: 1.04 }}
              onHoverStart={() => setAccent(c?.accent)}
              onClick={() => a && props.onAlbum?.(a)}
              className={`w-[112px] shrink-0 ${props.onAlbum ? 'cursor-pointer' : ''}`}
              title={a ? `${a.title}  (${a.album_artist})` : d}
            >
              <div className="aspect-square overflow-hidden rounded-xl bg-white/5 shadow-lg shadow-black/30">
                {c && <img src={c.src} alt="" className="h-full w-full object-cover" draggable={false} />}
              </div>
              <div className="mt-1.5 truncate text-[11.5px] text-ink/75">{a?.title ?? d}</div>
            </motion.div>
          )
        })}
        {props.dirs.length > dirs.length && (
          <div className="grid w-[112px] shrink-0 place-items-center text-[12px] text-ink/50">
            +{props.dirs.length - dirs.length} more
          </div>
        )}
      </motion.div>
    </div>
  )
}

export function DeepProgressBar(props: {
  deep: { done: number; total: number }
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
          <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-white/10">
            <motion.div
              className="h-full rounded-full"
              style={{ background: 'var(--accent)' }}
              animate={{ width: `${pct}%` }}
              transition={{ type: 'spring', stiffness: 120, damping: 24 }}
            />
          </div>
        </div>
        <button onClick={props.onCancel} className="rounded-full px-3 py-1.5 text-[13px] text-ink/70 hover:bg-white/10 hover:text-ink">
          Stop
        </button>
      </div>
    </motion.div>
  )
}

function SpectrogramView(props: { root: string; album: AlbumSummary; onClose: () => void }) {
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
        className="glass relative w-full max-w-[1040px] rounded-[26px] p-7"
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

function FormatBar(props: { formats: HealthReport['summary']['formats']; total: number }) {
  const label = (f: { bit_depth: number; sample_rate: number }) =>
    `${f.bit_depth}-bit ${f.sample_rate / 1000} kHz`
  const shades = [0.9, 0.62, 0.42, 0.28, 0.18, 0.12]
  return (
    <div className="mt-8">
      <div className="flex h-2.5 overflow-hidden rounded-full bg-white/5">
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

function EmptyState(props: { onChoose: () => void }) {
  return (
    <div className="grid h-full place-items-center">
      <div className="text-center">
        <h1 className="text-[44px] font-semibold tracking-[-0.03em]">Point it at your music</h1>
        <p className="mx-auto mt-3 max-w-[46ch] text-[15px] leading-relaxed text-ink/65">
          Choose the folder that holds your library. It is only read. Nothing changes until you
          approve it.
        </p>
        <div className="mt-8 flex justify-center">
          <PillButton onClick={props.onChoose} accent>
            <IconFolder /> Choose library folder
          </PillButton>
        </div>
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
    <motion.button
      whileTap={{ scale: 0.95 }}
      whileHover={{ scale: 1.03 }}
      transition={spring}
      onClick={props.onClick}
      disabled={props.disabled}
      title={props.title}
      className={`flex items-center gap-2 rounded-full px-4 py-2.5 text-[13.5px] font-medium disabled:opacity-60 ${
        props.accent ? 'text-[#0d0d10]' : 'glass text-ink'
      }`}
      style={props.accent ? { background: 'var(--accent)' } : undefined}
    >
      {props.children}
    </motion.button>
  )
}

const fmt = (n: number) => n.toLocaleString()
