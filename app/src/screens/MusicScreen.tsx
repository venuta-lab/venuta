import { AnimatePresence, motion } from 'motion/react'
import { useMemo, useState, useEffect } from 'react'
import { createPortal } from 'react-dom'
import type { AlbumSummary, HealthReport } from '../lib/engine'
import { AlbumEditor } from './AlbumEditor'
import { useCovers } from '../lib/covers'
import { IconSearch } from '../components/Icons'
import { Bar } from '../components/Activity'
import { Chip, Page } from '../components/ui'
import { counted, fraction, timeLeft, useJob, useNow } from '../lib/jobs'
import { keyFor, matcher } from '../lib/textsearch'
import { WindowedList, type Line } from '../components/WindowedList'

const fmtNum = (n: number) => n.toLocaleString('en-IN')

/**
 * The details an album can be missing, from the health report's findings. (Volume levelling
 * is measured from the sound by Health's quality check, not written in, so it isn't here.)
 */
const GAPS: { issue: string; label: string }[] = [
  { issue: 'missing_album_artist', label: 'Album artist' },
  { issue: 'missing_genre', label: 'Genre' },
  { issue: 'missing_composer', label: 'Composer' },
  { issue: 'no_lyrics', label: 'Synced lyrics' },
  { issue: 'art_small', label: 'Sharp cover' },
]
const ROW = 'grid grid-cols-[48px_minmax(0,1.3fr)_56px_minmax(0,1.7fr)_16px] items-center gap-4 px-2'
const NEEDS = 'needs'
/** A row: the 48 px cover with 8 px above and below, and 2 px between rows. */
const ROW_H = 64
const ROW_STEP = 66

/**
 * Metadata: every album as a row with what it is missing, to find the ones that need work and
 * open them in the editor. Music is the place to browse and play; this is the workbench.
 */
export function MusicScreen({
  root,
  report,
  busy,
  searchRef,
  openDir = null,
  onOpened,
}: {
  root: string
  report: HealthReport | null
  busy: string
  searchRef: React.Ref<HTMLInputElement>
  /** An album to open in the editor (from its page's Edit details); cleared with onOpened. */
  openDir?: string | null
  onOpened?: () => void
}) {
  const cover = useCovers(320)
  const making = !!useJob('covers')
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState<string>('all')
  const [selected, setSelected] = useState<AlbumSummary | null>(null)
  const albums = useMemo(() => report?.albums ?? [], [report])
  useEffect(() => {
    if (!openDir) return
    const a = albums.find((x) => x.dir === openDir)
    if (!a) return
    setSelected(a)
    onOpened?.()
  }, [openDir, albums, onOpened])

  // Which details each album lacks.
  const gaps = useMemo(() => {
    const m = new Map<string, string[]>()
    for (const g of GAPS) {
      const issue = report?.issues.find((i) => i.id === g.issue)
      for (const dir of issue?.albums ?? []) m.set(dir, [...(m.get(dir) ?? []), g.label])
    }
    return m
  }, [report])
  const counts = useMemo(
    () => GAPS.map((g) => ({ ...g, n: report?.issues.find((i) => i.id === g.issue)?.albums.length ?? 0 })).filter((g) => g.n > 0),
    [report],
  )
  const needs = (a: AlbumSummary) => !!a.untagged || (gaps.get(a.dir)?.length ?? 0) > 0
  const needing = albums.filter(needs).length

  const shown = useMemo(() => {
    const has = matcher(query)
    return albums
      .filter((a) =>
        filter === 'all'
          ? true
          : filter === NEEDS
            ? !!a.untagged || (gaps.get(a.dir)?.length ?? 0) > 0
            : (gaps.get(a.dir) ?? []).includes(filter),
      )
      .filter((a) => has(keyFor(a, () => `${a.title} ${a.album_artist} ${a.year ?? ''}`)))
      .sort((a, b) => a.title.localeCompare(b.title))
  }, [albums, query, filter, gaps])
  const lines = useMemo(() => shown.map((a): Line<AlbumSummary> => ({ kind: 'row', key: a.dir, row: a })), [shown])

  const s = report?.summary
  return (
    <Page
      title="Metadata"
      line={
        s
          ? `${fmtNum(s.albums)} albums, ${fmtNum(s.tracks)} songs. ${needing ? `${fmtNum(needing)} could use more details; open one to edit it.` : 'Every album has its details.'}`
          : busy || 'Reading your music…'
      }
      bar={
        <div className="flex min-w-[240px] max-w-[420px] flex-1 items-center gap-2 rounded-full bg-ink/[0.07] px-4 shadow-[inset_0_0_0_1px_var(--edge)] focus-within:bg-ink/[0.1]">
          <IconSearch className="shrink-0 text-ink/45" />
          <input
            ref={searchRef}
            id="music-search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === 'Escape' && (setQuery(''), (e.target as HTMLInputElement).blur())}
            placeholder="Search albums, artists, years"
            className="min-w-0 flex-1 bg-transparent py-2 text-[13.5px] text-ink outline-none placeholder:text-ink/45"
          />
          <kbd>/</kbd>
        </div>
      }
      under={
        albums.length > 0 && (
          <>
          <div className="no-scrollbar -mx-1 flex gap-1 overflow-x-auto px-1">
            <Chip group="meta-filter" on={filter === 'all'} label="All" count={albums.length} onClick={() => setFilter('all')} />
            <Chip group="meta-filter" on={filter === NEEDS} label="Needs details" count={needing} onClick={() => setFilter(NEEDS)} />
            {counts.map((g) => (
              <Chip
                key={g.issue}
                group="meta-filter"
                on={filter === g.label}
                label={`No ${g.label.toLowerCase()}`}
                count={g.n}
                onClick={() => setFilter(g.label)}
              />
            ))}
          </div>
          {shown.length > 0 && (
            <div className={`${ROW} mt-2 border-b border-edge pb-1.5 text-[12px] text-ink/55`}>
              <span />
              <span>Album</span>
              <span>Year</span>
              <span>Missing</span>
              <span />
            </div>
          )}
          </>
        )
      }
    >
      {!report && <Reading busy={busy} />}
      {report && shown.length === 0 && (
        <p className="py-20 text-center text-[14px] text-ink/55">{query ? `No albums match “${query}”.` : 'None: every album has this.'}</p>
      )}
      {shown.length > 0 && (
        // A screen of rows at a time: the whole library opens here as fast as a dozen albums.
        <WindowedList
          label="Albums"
          lines={lines}
          rowHeight={ROW_STEP}
          movable={false}
          onMove={() => {}}
          render={(a, _i, style) => {
            const c = cover(a.dir)
            const missing = gaps.get(a.dir) ?? []
            return (
              <button
                role="listitem"
                style={{ ...style, height: ROW_H }}
                onClick={() => {
                  setSelected(a)
                }}
                className={`group ${ROW} absolute inset-x-0 rounded-2xl text-left transition-colors hover:bg-ink/[0.06] focus-visible:bg-ink/[0.07] active:bg-ink/[0.1]`}
              >
                <motion.div layoutId={`cover-${a.dir}`} className="h-12 w-12 overflow-hidden rounded-[10px] bg-ink/[0.06] shadow-md shadow-black/25">
                  {c ? (
                    <CoverImage src={c.src} title={a.title} />
                  ) : making && a.cover_track ? (
                    <div className="shimmer h-full w-full" />
                  ) : (
                    <NoCover title={a.title} small />
                  )}
                </motion.div>
                <span className="min-w-0">
                  <span className="block truncate text-[14px] font-medium">{a.title}</span>
                  <span className="block truncate text-[12.5px] text-ink/55">{a.album_artist || (a.untagged ? 'No tags yet' : 'Unknown artist')}</span>
                </span>
                <span className="text-[13px] tabular-nums text-ink/55">{a.year ?? ''}</span>
                <span className="flex h-[22px] min-w-0 flex-wrap gap-1.5 overflow-hidden">
                  {a.untagged ? (
                    <span className="rounded-full bg-problem/15 px-2 py-0.5 text-[11.5px] text-problem">No tags</span>
                  ) : missing.length ? (
                    missing.map((m) => (
                      <span key={m} className="rounded-full bg-ink/[0.07] px-2 py-0.5 text-[11.5px] text-ink/65">
                        {m}
                      </span>
                    ))
                  ) : (
                    <span className="py-0.5 text-[12px] text-good">✓ Complete</span>
                  )}
                </span>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" className="text-ink/30 transition-colors group-hover:text-ink/70" aria-hidden>
                  <path d="m9 6 6 6-6 6" />
                </svg>
              </button>
            )
          }}
        />
      )}

      {createPortal(
        <AnimatePresence>
          {selected && <AlbumEditor root={root} album={selected} src={cover(selected.dir)?.src} onClose={() => setSelected(null)} />}
        </AnimatePresence>,
        document.body,
      )}
    </Page>
  )
}

/** While the folder is read: what is happening, how far along, and the wall's shape to come. */
export function Reading({ busy }: { busy: string }) {
  const job = useJob('scan')
  const now = useNow()
  const count = job ? counted(job) : ''
  const eta = job ? timeLeft(job, now) : ''
  return (
    <div className="mt-2">
      <div className="glass rounded-[22px] px-6 py-5">
        <div className="flex items-baseline gap-4">
          <span className="flex-1 text-[15px] font-medium">{job?.label ?? (busy.replace(/…$/, '') || 'Reading your music')}</span>
          <span className="text-[12.5px] text-ink/55">{[count, eta].filter(Boolean).join(', ')}</span>
        </div>
        <div className="mt-3">
          <Bar value={job ? fraction(job) : null} />
        </div>
        <p className="mt-3 text-[12.5px] text-ink/55">
          The first time takes a few minutes for a big collection; after that only new or changed files are read. Your files are only
          read, never changed.
        </p>
      </div>
      <div className="mt-6 grid grid-cols-[repeat(auto-fill,minmax(168px,1fr))] gap-x-5 gap-y-7" aria-hidden>
        {Array.from({ length: 10 }, (_, i) => (
          <div key={i}>
            <div className="shimmer aspect-square rounded-2xl" style={{ animationDelay: `${i * 80}ms` }} />
            <div className="shimmer mt-3 h-3 w-3/4 rounded-full" />
            <div className="shimmer mt-2 h-2.5 w-1/2 rounded-full" />
          </div>
        ))}
      </div>
    </div>
  )
}

/**
 * A cover thumbnail. If it fails to load it tries once more, then falls back to the lettered
 * tile, so a hiccup never leaves a broken-image icon on the wall.
 */
export function CoverImage({ src, title }: { src: string; title: string }) {
  const [attempt, setAttempt] = useState(0)
  if (attempt > 1) return <NoCover title={title} />
  return (
    <img
      key={attempt}
      src={attempt ? `${src}?retry=${attempt}` : src}
      alt=""
      draggable={false}
      loading="lazy"
      onError={() => window.setTimeout(() => setAttempt((n) => n + 1), attempt ? 0 : 600)}
      className="h-full w-full object-cover transition-transform duration-500 group-hover:scale-[1.04]"
    />
  )
}

/** A cover for albums without art: the first letters on a soft tint of the accent. */
export function NoCover({ title, small = false }: { title: string; small?: boolean }) {
  const letters = title
    .split(/\s+/)
    .filter((w) => /^\p{L}/u.test(w))
    .slice(0, 2)
    .map((w) => w[0]?.toUpperCase())
    .join('')
  return (
    <div
      className={`grid h-full w-full place-items-center font-semibold tracking-[-0.03em] text-ink/45 ${small ? 'text-[15px]' : 'text-[38px]'}`}
      style={{ background: 'linear-gradient(135deg, color-mix(in oklab, var(--accent) 22%, transparent), color-mix(in oklab, var(--ink) 4%, transparent))' }}
    >
      {letters || '♪'}
    </div>
  )
}
