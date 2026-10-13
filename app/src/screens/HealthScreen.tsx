import { useEscape } from '../lib/layers'
import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useState } from 'react'
import { createPortal } from 'react-dom'
import {
  inApp,
  review,
  SPECTRAL_ISSUES,
  wantAdd,
  type AlbumSummary,
  type AlbumView,
  type HealthReport,
} from '../lib/engine'
import { setAccent, useCovers } from '../lib/covers'
import { AlbumEditor } from './AlbumEditor'
import { NoCover } from './MusicScreen'
import { Page } from '../components/ui'
import { FormatBar, SpectrogramView } from './LibraryScreen'

/*
  Health: how complete and how sound your library is, as a dashboard of tiles, each with
  something to do. Browsing lives in Music; questions live in Organize.
*/

const fmt = (n: number) => n.toLocaleString('en-IN')
const spring = { type: 'spring', stiffness: 360, damping: 32 } as const

/** Whether the plan gives an album this detail. */
const has = (a: AlbumView, field: string) =>
  a.changes.some((c) => c.field.toUpperCase() === field && c.after.some((v) => v.trim() !== ''))

type Fix = 'build' | 'lookup'
const DETAILS: { label: string; issue: string; fix: Fix; filled: (a: AlbumView) => boolean }[] = [
  { label: 'Composer', issue: 'missing_composer', fix: 'lookup', filled: (a) => has(a, 'COMPOSER') },
  { label: 'Synced lyrics', issue: 'no_lyrics', fix: 'lookup', filled: (a) => has(a, 'LYRICS') },
  { label: 'Genre', issue: 'missing_genre', fix: 'build', filled: (a) => has(a, 'GENRE') },
  { label: 'Album artist', issue: 'missing_album_artist', fix: 'build', filled: (a) => has(a, 'ALBUMARTIST') },
  { label: 'Sharp cover', issue: 'art_small', fix: 'lookup', filled: (a) => !!a.cover_upgrade },
  { label: 'Cover every player shows', issue: 'art_progressive', fix: 'build', filled: (a) => a.art.kind === 'reencode' },
  { label: 'Volume levelling', issue: 'no_replaygain', fix: 'build', filled: (a) => has(a, 'REPLAYGAIN_ALBUM_GAIN') },
]

/** Problems in the sound files themselves; only a better copy fixes them. */
const SOUND: { issue: string; label: string; short: string }[] = [
  { issue: 'damaged_audio', label: 'Damaged files', short: 'The sound no longer matches its checksum.' },
  { issue: 'lossy_files', label: 'Lossy files', short: 'MP3 or AAC in a lossless library.' },
  { issue: 'upsampled_hires', label: 'Hi-res in name only', short: 'Upsampled from a lower rate.' },
  { issue: 'padded_24bit', label: '24-bit in name only', short: '16-bit sound padded to 24 bits.' },
  { issue: 'limited_highs', label: 'Highs cut off', short: 'Often a sign of a lossy source.' },
  { issue: 'incomplete_albums', label: 'Missing songs', short: 'Track numbers have gaps.' },
]

type Sheet = { title: string; text: string; albums: string[]; issue?: string; sound?: boolean }

export function HealthScreen(props: {
  root: string
  report: HealthReport | null
  busy: string
  progress: React.ReactNode
  onLookup: () => void
  onCheck: () => void
  onRescan: () => void
  onOrganize: () => void
  running: { lookup: boolean; check: boolean }
}) {
  const { report } = props
  const cover = useCovers(320)
  const [sheet, setSheet] = useState<Sheet | null>(null)
  const s = report?.summary
  const byId = useMemo(() => new Map((report?.issues ?? []).map((i) => [i.id, i])), [report])

  // The plan, to show where each detail lands after you build.
  const [plan, setPlan] = useState<AlbumView[] | null>(null)
  useEffect(() => {
    if (!props.root || !report) return
    let live = true
    review(props.root)
      .then((r) => live && setPlan(r.plan.albums))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [props.root, report])
  const planBySource = useMemo(() => {
    const m = new Map<string, AlbumView[]>()
    for (const a of plan ?? []) m.set(a.source_dir, [...(m.get(a.source_dir) ?? []), a])
    return m
  }, [plan])

  const details = DETAILS.map((d) => {
    const issue = byId.get(d.issue)
    const total = s?.albums ?? 0
    const missing = issue?.albums ?? []
    const fixed = plan
      ? missing.filter((dir) => {
          const dest = planBySource.get(dir)
          return !!dest?.length && dest.every(d.filled)
        })
      : []
    // Never shows 100% while something is still missing.
    const pct = (have: number) => (total ? (have >= total ? 100 : Math.min(99, Math.round((have / total) * 100))) : 100)
    const now = pct(total - missing.length)
    const after = pct(total - missing.length + fixed.length)
    return { ...d, missing, left: missing.filter((m) => !fixed.includes(m)), now, after }
  })
  const overallNow = details.length ? Math.round(details.reduce((x, d) => x + d.now, 0) / details.length) : 0
  const overallAfter = details.length ? Math.round(details.reduce((x, d) => x + d.after, 0) / details.length) : 0
  const sound = SOUND.map((x) => ({ ...x, found: byId.get(x.issue) })).filter((x) => x.found)
  const dupes = (byId.get('duplicate_audio')?.albums.length ?? 0) + (byId.get('duplicate_recording')?.albums.length ?? 0)

  const stack = (dirs: string[]) => (
    <div className="flex -space-x-2.5">
      {dirs.slice(0, 4).map((d) => {
        const c = cover(d)
        return (
          <div key={d} className="h-9 w-9 overflow-hidden rounded-lg bg-ink/[0.1] shadow-md shadow-black/25 ring-2 ring-[var(--glass)]">
            {c ? <img src={c.src} alt="" className="h-full w-full object-cover" /> : <NoCover title="" small />}
          </div>
        )
      })}
      {dirs.length > 4 && (
        <div className="grid h-9 w-9 place-items-center rounded-lg bg-ink/[0.12] text-[11px] text-ink/70 ring-2 ring-[var(--glass)]">
          +{dirs.length - 4}
        </div>
      )}
    </div>
  )

  if (!s)
    return (
      <Page title="Health" line={props.busy || 'Reading your music…'}>
        <span />
      </Page>
    )

  return (
    <Page title="Health" line={`${fmt(s.albums)} albums, ${fmt(s.tracks)} songs, ${Math.round(s.hours)} hours, ${(s.bytes / 1e9).toFixed(1)} GB`}>
      {props.progress}

      {/* The overview and the tools. */}
      <div className="grid gap-3 lg:grid-cols-[1.25fr_1fr]">
        <section className="glass flex flex-wrap items-center gap-6 rounded-[24px] p-6">
          <Ring now={overallNow} after={plan ? overallAfter : overallNow} />
          <div className="min-w-[200px] flex-1">
            <div className="text-[13px] font-medium text-accent-ink">How complete your library is</div>
            <div className="mt-1 text-[22px] font-semibold leading-tight tracking-[-0.02em]">
              {overallNow}% today{plan && overallAfter > overallNow ? `, ${overallAfter}% after you build` : ''}
            </div>
            <p className="mt-1.5 text-[13px] leading-relaxed text-ink/60">
              Credits, lyrics, genres, covers and volume levels, across all albums. The build fills most gaps; looking up
              details online finds more.
            </p>
            <div className="mt-4">
              <button onClick={props.onOrganize} className="btn-primary">
                Review and build
              </button>
            </div>
          </div>
        </section>

        <section className="glass rounded-[24px] px-5 pb-2 pt-4">
          <h2 className="text-[15px] font-semibold">Tools</h2>
          <Tool
            title="Look up details online"
            text="Original years, composers and lyricists, sharper covers, synced lyrics."
            action={props.running.lookup ? 'Running…' : 'Run'}
            disabled={props.running.lookup}
            onClick={props.onLookup}
          />
          <Tool
            title="Check quality"
            text="Decodes every new or changed song: damage, lossy sources, fake hi-res, loudness."
            action={props.running.check ? 'Running…' : 'Run'}
            disabled={props.running.check}
            onClick={props.onCheck}
          />
          <Tool title="Look for changes" text="Reads the folder again for new or changed files." action="Rescan" onClick={props.onRescan} />
        </section>
      </div>

      {/* Details: one tile each. */}
      <Section title="Details" />
      <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-3">
        {details.map((d) => (
          <section key={d.label} className="glass flex flex-col rounded-[20px] p-4">
            <div className="flex items-baseline gap-2">
              <span className="flex-1 text-[13.5px] font-medium">{d.label}</span>
              <span className="text-[22px] font-semibold tabular-nums tracking-[-0.02em]">{d.now}%</span>
            </div>
            <div className="relative mt-2 h-1.5 overflow-hidden rounded-full bg-ink/10">
              <motion.div
                className="absolute inset-y-0 left-0 rounded-full"
                style={{ background: 'color-mix(in oklab, var(--color-good) 50%, transparent)' }}
                initial={{ width: 0 }}
                animate={{ width: `${d.after}%` }}
                transition={{ type: 'spring', stiffness: 70, damping: 20 }}
              />
              <motion.div
                className="absolute inset-y-0 left-0 rounded-full"
                style={{ background: d.now === 100 ? 'var(--color-good)' : 'var(--accent)' }}
                initial={{ width: 0 }}
                animate={{ width: `${d.now}%` }}
                transition={{ type: 'spring', stiffness: 90, damping: 20 }}
              />
            </div>
            <div className="mt-1.5 text-[12px] text-ink/55">
              {d.missing.length === 0
                ? 'Every album has it.'
                : plan && d.after > d.now
                  ? `${d.after}% after you build${d.left.length ? `; ${fmt(d.left.length)} still without` : ''}`
                  : `${fmt(d.missing.length)} albums without`}
            </div>
            <div className="mt-auto flex items-center gap-2 pt-3">
              {d.left.length > 0 && stack(d.left)}
              <span className="flex-1" />
              {d.left.length > 0 && (
                <button
                  onClick={() =>
                    setSheet({
                      title: `${d.label}: ${fmt(d.left.length)} albums still without`,
                      text:
                        d.fix === 'lookup'
                          ? 'Looking up details online may find these. You can also add them yourself: open an album and edit it.'
                          : 'Open an album to add this yourself.',
                      albums: d.left,
                    })
                  }
                  className="btn-ghost btn-sm"
                >
                  See albums
                </button>
              )}
            </div>
          </section>
        ))}
      </div>

      {/* Sound: problems a better copy fixes. */}
      {sound.length > 0 && (
        <>
          <Section
            title="Sound"
            text="Problems in the files themselves. Open one to see the albums and their spectrograms, or put an album on “To get” to find a better copy."
          />
          <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-3">
            {sound.map((x) => (
              <button
                key={x.issue}
                onClick={() => setSheet({ title: x.label, text: x.found!.why, albums: x.found!.albums, issue: x.issue, sound: true })}
                className="glass flex flex-col rounded-[20px] p-4 text-left transition-transform hover:-translate-y-0.5"
              >
                <div className="flex items-baseline gap-2">
                  <span className="flex-1 text-[13.5px] font-medium">{x.label}</span>
                  <span className="text-[22px] font-semibold tabular-nums tracking-[-0.02em]">{fmt(x.found!.albums.length)}</span>
                </div>
                <div className="mt-1 text-[12px] text-ink/55">
                  {x.short} {fmt(x.found!.tracks)} songs.
                </div>
                <div className="mt-auto pt-3">{stack(x.found!.albums)}</div>
              </button>
            ))}
          </div>
        </>
      )}

      {/* Formats and duplicates. */}
      <div className="mt-10 grid gap-3 lg:grid-cols-[1.6fr_1fr]">
        <section className="glass rounded-[22px] p-5">
          <h2 className="text-[15px] font-semibold">Formats</h2>
          <FormatBar formats={s.formats} total={s.tracks} />
        </section>
        <section className="glass flex flex-col rounded-[22px] p-5">
          <h2 className="text-[15px] font-semibold">Duplicates</h2>
          <div className="mt-1 text-[12.5px] text-ink/60">
            {dupes
              ? `${fmt(dupes)} albums hold songs that exist elsewhere. Organize compares them and sets copies aside, with proof.`
              : 'No duplicates found.'}
          </div>
          {dupes > 0 && (
            <div className="mt-auto pt-3">
              <button onClick={props.onOrganize} className="btn btn-sm">
                Open Organize
              </button>
            </div>
          )}
        </section>
      </div>

      {createPortal(
        <AnimatePresence>
          {sheet && <AlbumsSheet root={props.root} sheet={sheet} albums={report?.albums ?? []} onClose={() => setSheet(null)} />}
        </AnimatePresence>,
        document.body,
      )}
    </Page>
  )
}

/** A heading for a group of tiles, with a line about it. */
function Section(props: { title: string; text?: string }) {
  return (
    <div className="mb-3 mt-10">
      <h2 className="text-[17px] font-semibold tracking-[-0.01em]">{props.title}</h2>
      {props.text && <p className="mt-1 max-w-[70ch] text-[13px] leading-relaxed text-ink/55">{props.text}</p>}
    </div>
  )
}

/** Overall completeness: today (accent) and after the build (green), as two arcs. */
function Ring({ now, after }: { now: number; after: number }) {
  const r = 46
  const c = 2 * Math.PI * r
  return (
    <svg width="120" height="120" viewBox="0 0 120 120" role="img" aria-label={`${now}% today, ${after}% after you build`}>
      <circle cx="60" cy="60" r={r} fill="none" stroke="color-mix(in oklab, var(--ink) 10%, transparent)" strokeWidth="10" />
      <motion.circle
        cx="60"
        cy="60"
        r={r}
        fill="none"
        stroke="color-mix(in oklab, var(--color-good) 55%, transparent)"
        strokeWidth="10"
        strokeLinecap="round"
        transform="rotate(-90 60 60)"
        strokeDasharray={c}
        initial={{ strokeDashoffset: c }}
        animate={{ strokeDashoffset: c * (1 - after / 100) }}
        transition={{ type: 'spring', stiffness: 50, damping: 18 }}
      />
      <motion.circle
        cx="60"
        cy="60"
        r={r}
        fill="none"
        stroke="var(--accent)"
        strokeWidth="10"
        strokeLinecap="round"
        transform="rotate(-90 60 60)"
        strokeDasharray={c}
        initial={{ strokeDashoffset: c }}
        animate={{ strokeDashoffset: c * (1 - now / 100) }}
        transition={{ type: 'spring', stiffness: 60, damping: 18 }}
      />
      <text x="60" y="58" textAnchor="middle" fontSize="24" fontWeight="600" fill="var(--ink)">
        {now}%
      </text>
      {after > now && (
        <text x="60" y="78" textAnchor="middle" fontSize="12" fill="var(--color-good)">
          → {after}%
        </text>
      )}
    </svg>
  )
}

function Tool(props: { title: string; text: string; action: string; disabled?: boolean; onClick: () => void }) {
  return (
    <div className="flex items-center gap-3 border-b border-edge py-3 last:border-b-0">
      <div className="min-w-0 flex-1">
        <div className="text-[13.5px] font-medium">{props.title}</div>
        <div className="truncate text-[12px] text-ink/50" title={props.text}>
          {props.text}
        </div>
      </div>
      <button
        onClick={props.onClick}
        disabled={props.disabled}
        className="btn btn-sm min-w-[72px]"
      >
        {props.action}
      </button>
    </div>
  )
}

/** A sheet of albums for one finding, each with what can be done about it. */
function AlbumsSheet(props: { root: string; sheet: Sheet; albums: AlbumSummary[]; onClose: () => void }) {
  const cover = useCovers(320)
  const byDir = new Map(props.albums.map((a) => [a.dir, a]))
  const list = props.sheet.albums.map((d) => byDir.get(d)).filter((a): a is AlbumSummary => !!a)
  const [open, setOpen] = useState<AlbumSummary | null>(null)
  const [spectrum, setSpectrum] = useState<AlbumSummary | null>(null)
  const [wanted, setWanted] = useState<Set<string>>(new Set())
  const spectral = !!props.sheet.issue && SPECTRAL_ISSUES.has(props.sheet.issue)
  const { onClose } = props
  useEscape(!open && !spectrum, onClose)
  return (
    <>
      <motion.div className="fixed inset-0 z-40 bg-black/35" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} onClick={onClose} />
      <motion.aside
        role="dialog"
        aria-label={props.sheet.title}
        initial={{ x: '100%' }}
        animate={{ x: 0 }}
        exit={{ x: '100%' }}
        transition={spring}
        className="popover no-scrollbar fixed bottom-3 right-3 top-12 z-40 w-[min(720px,92vw)] overflow-y-auto rounded-[24px] p-6"
      >
        <div className="flex items-start gap-3">
          <div className="min-w-0 flex-1">
            <h2 className="text-[22px] font-semibold tracking-[-0.02em]">{props.sheet.title}</h2>
            <p className="mt-1.5 max-w-[60ch] text-[13px] leading-relaxed text-ink/65">{props.sheet.text}</p>
          </div>
          <button onClick={onClose} aria-label="Close" className="grid h-8 w-8 place-items-center rounded-full text-[18px] text-ink/60 hover:bg-ink/10">
            ×
          </button>
        </div>
        <div className="mt-5 grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-x-4 gap-y-5">
          {list.map((a) => {
            const c = cover(a.dir)
            const isWanted = wanted.has(a.dir)
            return (
              <div key={a.dir} className="min-w-0">
                <button onClick={() => setOpen(a)} onMouseEnter={() => setAccent(c?.accent)} className="group block w-full text-left">
                  <div className="aspect-square overflow-hidden rounded-xl bg-ink/[0.06] shadow-lg shadow-black/25">
                    {c && <img src={c.src} alt="" loading="lazy" className="h-full w-full object-cover transition-transform group-hover:scale-[1.04]" />}
                  </div>
                  <div className="mt-1.5 truncate text-[12.5px] font-medium">{a.title}</div>
                  <div className="truncate text-[11.5px] text-ink/50">{a.album_artist}</div>
                </button>
                {props.sheet.sound && (
                  <div className="mt-1.5 flex flex-wrap gap-1">
                    {spectral && (
                      <button onClick={() => setSpectrum(a)} className="rounded-full bg-ink/[0.08] px-2.5 py-0.5 text-[11.5px] hover:bg-ink/[0.14]">
                        Spectrogram
                      </button>
                    )}
                    <button
                      disabled={isWanted || !inApp()}
                      onClick={async () => {
                        await wantAdd(a.title, a.album_artist, props.sheet.title.toLowerCase())
                        setWanted(new Set([...wanted, a.dir]))
                      }}
                      className="rounded-full bg-ink/[0.08] px-2.5 py-0.5 text-[11.5px] hover:bg-ink/[0.14] disabled:opacity-60"
                    >
                      {isWanted ? 'On To get ✓' : 'Add to To get'}
                    </button>
                  </div>
                )}
              </div>
            )
          })}
        </div>
      </motion.aside>
      <AnimatePresence>
        {open && <AlbumEditor root={props.root} album={open} src={cover(open.dir)?.src} onClose={() => setOpen(null)} />}
      </AnimatePresence>
      <AnimatePresence>{spectrum && <SpectrogramView root={props.root} album={spectrum} onClose={() => setSpectrum(null)} />}</AnimatePresence>
    </>
  )
}
