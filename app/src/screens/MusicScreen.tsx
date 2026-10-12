import { AnimatePresence, motion } from 'motion/react'
import { forwardRef, useMemo, useState } from 'react'
import { createPortal } from 'react-dom'
import type { AlbumSummary, HealthReport } from '../lib/engine'
import { AlbumEditor } from './AlbumEditor'
import { setAccent, useCovers } from '../lib/covers'
import { IconSearch } from '../components/Icons'

const spring = { type: 'spring', stiffness: 300, damping: 30 } as const

/** "01_Indian_Film_Music/…" → "Indian Film Music": your own top-level folders as filters. */
function shelfOf(dir: string): string {
  const top = dir.split('/')[0] ?? ''
  return top.replace(/^\d+[_\s-]*/, '').replace(/_/g, ' ').trim() || 'Other'
}

const fmtNum = (n: number) => n.toLocaleString('en-IN')

/*
  Problems with the sound files themselves, which a build cannot fix. Everything else the
  quality report lists (volume levels, cover format, missing composer…) is fixed by a build.
*/
const SOUND_ISSUES = new Set([
  'damaged_audio',
  'lossy_files',
  'upsampled_hires',
  'padded_24bit',
  'limited_highs',
])

/** Your albums as a wall of covers, with search and filters. */
export const MusicWall = forwardRef<HTMLInputElement, { root: string; report: HealthReport | null; busy: string }>(
  function MusicWall({ root, report, busy }, searchRef) {
    const cover = useCovers(320)
    const [query, setQuery] = useState('')
    const [shelf, setShelf] = useState<string>('all')
    const [selected, setSelected] = useState<AlbumSummary | null>(null)
    const albums = useMemo(() => report?.albums ?? [], [report])

    // Albums whose sound files have a problem, with what it is.
    const flagged = useMemo(() => {
      const m = new Map<string, string[]>()
      for (const i of report?.issues ?? []) {
        if (!SOUND_ISSUES.has(i.id)) continue
        for (const d of i.albums) m.set(d, [...(m.get(d) ?? []), i.title])
      }
      return m
    }, [report])

    const shelves = useMemo(() => {
      const m = new Map<string, number>()
      for (const a of albums) m.set(shelfOf(a.dir), (m.get(shelfOf(a.dir)) ?? 0) + 1)
      return [...m.entries()].sort((a, b) => b[1] - a[1])
    }, [albums])

    const shown = useMemo(() => {
      const q = query.trim().toLowerCase()
      return albums
        .filter((a) => shelf === 'all' || (shelf === 'issues' ? flagged.has(a.dir) : shelfOf(a.dir) === shelf))
        .filter((a) => !q || `${a.title} ${a.album_artist} ${a.year ?? ''}`.toLowerCase().includes(q))
        .sort((a, b) => a.title.localeCompare(b.title))
    }, [albums, query, shelf, flagged])

    const chip = (id: string, label: string, count: number) => (
      <button
        key={id}
        onClick={() => setShelf(id)}
        aria-pressed={shelf === id}
        className={`relative shrink-0 rounded-full px-3.5 py-1.5 text-[12.5px] ${shelf === id ? 'text-ink' : 'text-ink/60 hover:text-ink'}`}
      >
        {shelf === id && (
          <motion.span
            layoutId="shelf-pill"
            transition={spring}
            className="absolute inset-0 rounded-full"
            style={{ background: 'color-mix(in oklab, var(--accent) 26%, color-mix(in oklab, var(--ink) 6%, transparent))' }}
          />
        )}
        <span className="relative">
          {label} <span className="text-ink/45">{fmtNum(count)}</span>
        </span>
      </button>
    )

    return (
      <div className="pb-16">
        <div className="sticky top-0 z-10 -mx-2 px-2 pb-3 pt-1">
          <div className="flex flex-wrap items-center gap-3">
            <div className="glass flex min-w-[240px] flex-1 items-center gap-2 rounded-full px-4">
              <IconSearch className="shrink-0 text-ink/45" />
              <input
                ref={searchRef}
                id="music-search"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => e.key === 'Escape' && (setQuery(''), (e.target as HTMLInputElement).blur())}
                placeholder="Search albums, artists, years"
                className="min-w-0 flex-1 bg-transparent py-2.5 text-[14px] text-ink outline-none placeholder:text-ink/45"
              />
              <kbd>/</kbd>
            </div>
          </div>
          <div className="no-scrollbar mt-3 flex gap-1 overflow-x-auto">
            {chip('all', 'All', albums.length)}
            {shelves.map(([s, n]) => chip(s, s, n))}
            {flagged.size > 0 && chip('issues', 'Sound quality', flagged.size)}
          </div>
        </div>

        {!report && <p className="py-20 text-center text-[14px] text-ink/55">{busy || 'Reading your music…'}</p>}
        {report && shown.length === 0 && (
          <p className="py-20 text-center text-[14px] text-ink/55">No albums match “{query}”.</p>
        )}

        <div className="mt-2 grid grid-cols-[repeat(auto-fill,minmax(168px,1fr))] gap-x-5 gap-y-7">
          {shown.map((a) => {
            const c = cover(a.dir)
            const issues = flagged.get(a.dir)
            return (
              <motion.button
                key={a.dir}
                onClick={() => {
                  setAccent(c?.accent)
                  setSelected(a)
                }}
                onHoverStart={() => setAccent(c?.accent)}
                whileHover={{ y: -6 }}
                transition={spring}
                className="group relative text-left"
              >
                <motion.div
                  layoutId={`cover-${a.dir}`}
                  className="aspect-square overflow-hidden rounded-2xl bg-ink/[0.06] shadow-xl shadow-black/30"
                >
                  {c && (
                    <img
                      src={c.src}
                      alt=""
                      draggable={false}
                      loading="lazy"
                      className="h-full w-full object-cover transition-transform duration-500 group-hover:scale-[1.04]"
                    />
                  )}
                </motion.div>
                {issues && (
                  <span
                    title={issues.join(', ')}
                    className="absolute right-2 top-2 max-w-[80%] truncate rounded-full bg-black/60 px-2 py-0.5 text-[10.5px] font-medium text-[#ffd180]"
                  >
                    {issues[0]}
                    {issues.length > 1 ? ` +${issues.length - 1}` : ''}
                  </span>
                )}
                <div className="mt-2.5 truncate text-[14px] font-medium">{a.title}</div>
                <div className="truncate text-[12.5px] text-ink/55">
                  {a.album_artist}
                  {a.year ? `, ${a.year}` : ''}
                </div>
              </motion.button>
            )
          })}
        </div>

        {createPortal(
          <AnimatePresence>
            {selected && (
              <AlbumEditor root={root} album={selected} src={cover(selected.dir)?.src} onClose={() => setSelected(null)} />
            )}
          </AnimatePresence>,
          document.body,
        )}
      </div>
    )
  },
)

/** Music: your albums, and the quality of your files. */
export function MusicScreen(props: {
  root: string
  report: HealthReport | null
  busy: string
  view: 'albums' | 'quality'
  onView: (v: 'albums' | 'quality') => void
  searchRef: React.Ref<HTMLInputElement>
  actions: React.ReactNode
  progress: React.ReactNode
  quality: React.ReactNode
}) {
  const s = props.report?.summary
  return (
    <div className="mx-auto max-w-[1180px]">
      <header className="flex flex-wrap items-end gap-x-6 gap-y-3 pb-4 pt-6">
        <div className="min-w-0 flex-1">
          <h1 className="text-[40px] font-semibold leading-none tracking-[-0.035em]">Music</h1>
          <p className="mt-2.5 text-[14px] text-ink/60">
            {s
              ? `${fmtNum(s.albums)} albums, ${fmtNum(s.tracks)} songs, ${Math.round(s.hours)} hours, ${(s.bytes / 1e9).toFixed(1)} GB`
              : props.busy || 'Reading your music…'}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">{props.actions}</div>
      </header>
      {props.progress}
      <div role="tablist" aria-label="Music views" className="mb-4 inline-flex rounded-full bg-ink/[0.07] p-1">
        {(['albums', 'quality'] as const).map((v) => (
          <button
            key={v}
            role="tab"
            aria-selected={props.view === v}
            onClick={() => props.onView(v)}
            className={`relative rounded-full px-4 py-1.5 text-[13px] ${props.view === v ? 'text-ink' : 'text-ink/60 hover:text-ink'}`}
          >
            {props.view === v && (
              <motion.span layoutId="music-view" transition={spring} className="absolute inset-0 rounded-full bg-ink/[0.14]" />
            )}
            <span className="relative">{v === 'albums' ? 'Albums' : 'Quality'}</span>
          </button>
        ))}
      </div>
      {props.view === 'albums' ? (
        <MusicWall ref={props.searchRef} root={props.root} report={props.report} busy={props.busy} />
      ) : (
        props.quality
      )}
    </div>
  )
}
