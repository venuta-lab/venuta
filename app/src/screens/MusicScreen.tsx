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

/** Your albums as a wall of covers, with search and filters. */
export const MusicWall = forwardRef<HTMLInputElement, { root: string; report: HealthReport | null; busy: string }>(
  function MusicWall({ root, report, busy }, searchRef) {
    const cover = useCovers(320)
    const [query, setQuery] = useState('')
    const [shelf, setShelf] = useState<string>('all')
    const [selected, setSelected] = useState<AlbumSummary | null>(null)
    const albums = useMemo(() => report?.albums ?? [], [report])

    const shelves = useMemo(() => {
      const m = new Map<string, number>()
      for (const a of albums) m.set(shelfOf(a.dir), (m.get(shelfOf(a.dir)) ?? 0) + 1)
      return [...m.entries()].sort((a, b) => b[1] - a[1])
    }, [albums])

    const shown = useMemo(() => {
      const q = query.trim().toLowerCase()
      return albums
        .filter((a) => shelf === 'all' || shelfOf(a.dir) === shelf)
        .filter((a) => !q || `${a.title} ${a.album_artist} ${a.year ?? ''}`.toLowerCase().includes(q))
        .sort((a, b) => a.title.localeCompare(b.title))
    }, [albums, query, shelf])

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
          </div>
        </div>

        {!report && <p className="py-20 text-center text-[14px] text-ink/55">{busy || 'Reading your music…'}</p>}
        {report && shown.length === 0 && (
          <p className="py-20 text-center text-[14px] text-ink/55">No albums match “{query}”.</p>
        )}

        <div className="mt-2 grid grid-cols-[repeat(auto-fill,minmax(168px,1fr))] gap-x-5 gap-y-7">
          {shown.map((a) => {
            const c = cover(a.dir)
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
                  {c ? (
                    <img
                      src={c.src}
                      alt=""
                      draggable={false}
                      loading="lazy"
                      className="h-full w-full object-cover transition-transform duration-500 group-hover:scale-[1.04]"
                    />
                  ) : (
                    <NoCover title={a.title} />
                  )}
                </motion.div>
                <div className="mt-2.5 truncate text-[14px] font-medium">{a.title}</div>
                <div className="truncate text-[12.5px] text-ink/55">
                  {a.album_artist || (a.untagged ? 'No tags yet' : 'Unknown artist')}
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

/** Music: every album, to find, play, edit and rate. */
export function MusicScreen(props: {
  root: string
  report: HealthReport | null
  busy: string
  searchRef: React.Ref<HTMLInputElement>
}) {
  const s = props.report?.summary
  return (
    <div className="mx-auto max-w-[1180px]">
      <header className="pb-4 pt-6">
        <h1 className="text-[40px] font-semibold leading-none tracking-[-0.035em]">Music</h1>
        <p className="mt-2.5 text-[14px] text-ink/60">
          {s ? `${fmtNum(s.albums)} albums, ${fmtNum(s.tracks)} songs` : props.busy || 'Reading your music…'}
        </p>
      </header>
      <MusicWall ref={props.searchRef} root={props.root} report={props.report} busy={props.busy} />
    </div>
  )
}

/** A cover for albums without art: the first letters on a soft tint of the accent. */
function NoCover({ title }: { title: string }) {
  const letters = title
    .split(/s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]?.toUpperCase())
    .join('')
  return (
    <div
      className="grid h-full w-full place-items-center text-[38px] font-semibold tracking-[-0.03em] text-ink/45"
      style={{ background: 'linear-gradient(135deg, color-mix(in oklab, var(--accent) 22%, transparent), color-mix(in oklab, var(--ink) 4%, transparent))' }}
    >
      {letters || '♪'}
    </div>
  )
}
