import { memo, useCallback, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { isHiRes, quality, type CatalogTrack } from '../../lib/engine'
import { useCovers, type Cover } from '../../lib/covers'
import { clock, play, toggle } from '../../lib/player'
import { IconPause, IconPlay } from '../../components/Icons'
import { compareText, scrollerOf, useNav, useNowPlaying } from './model'
import { InBar } from '../../components/ui'
import { Equalizer, QualityBadge, TrackMenu, trackActions, useContextMenu } from './parts'

const ROW = 44
const OVERSCAN = 10
const COLS = 'grid-cols-[36px_minmax(0,1.7fr)_minmax(0,1.1fr)_minmax(0,1.1fr)_52px_96px_32px]'

type SortKey = 'title' | 'artist' | 'album' | 'time' | 'quality'
const HEADERS: [SortKey, string][] = [
  ['title', 'Title'],
  ['artist', 'Artist'],
  ['album', 'Album'],
  ['time', 'Time'],
  ['quality', 'Quality'],
]

const artistsOf = (t: CatalogTrack) => (t.artists.length ? t.artists.join(', ') : t.album_artist)
const inAlbum = (a: CatalogTrack, b: CatalogTrack) =>
  compareText(a.album, b.album) || (a.disc || 1) - (b.disc || 1) || a.track - b.track
const score = (t: CatalogTrack) => (isHiRes(t) ? 2e9 : 0) + (t.bit_depth ? 1e9 : 0) + t.bit_depth * 1e6 + t.sample_rate

const ORDER: Record<SortKey, (a: CatalogTrack, b: CatalogTrack) => number> = {
  title: (a, b) => compareText(a.title, b.title) || compareText(artistsOf(a), artistsOf(b)),
  artist: (a, b) => compareText(artistsOf(a), artistsOf(b)) || inAlbum(a, b),
  album: (a, b) => inAlbum(a, b),
  time: (a, b) => a.duration_ms - b.duration_ms,
  quality: (a, b) => score(a) - score(b) || compareText(a.title, b.title),
}

/**
 * Songs as a long list: only the rows on screen are drawn, so thousands scroll smoothly. Click a
 * heading to sort by it (again to reverse). Double-click or Enter plays the list as shown, from
 * that song.
 */
export const SongList = memo(function SongList(props: {
  tracks: CatalogTrack[]
  /** Where the queue says it came from ("Songs", "Search: rahman"). */
  from: string
  /** Start sorted, or keep the order given. */
  sortBy?: SortKey
  /** Keep the column heads in the page's pinned bar while the list scrolls. */
  pin?: boolean
}) {
  const { root } = useNav()
  const now = useNowPlaying()
  const cover = useCovers(320)
  const [sort, setSort] = useState<{ key: SortKey | null; desc: boolean }>({ key: props.sortBy ?? null, desc: false })
  const list = useRef<HTMLDivElement>(null)
  const [range, setRange] = useState({ start: 0, end: 40 })

  const sorted = useMemo(() => {
    if (!sort.key) return props.tracks
    const by = ORDER[sort.key]
    const out = props.tracks.slice().sort(by)
    return sort.desc ? out.reverse() : out
  }, [props.tracks, sort])

  // Which rows are on screen: measured against the screen's own scroll area.
  useLayoutEffect(() => {
    const el = list.current
    const sc = scrollerOf(el)
    if (!el || !sc) return
    const update = () => {
      const top = el.getBoundingClientRect().top - sc.getBoundingClientRect().top
      const start = Math.max(0, Math.floor(-top / ROW) - OVERSCAN)
      const end = Math.max(start, Math.min(sorted.length, Math.ceil((sc.clientHeight - top) / ROW) + OVERSCAN))
      setRange((r) => (r.start === start && r.end === end ? r : { start, end }))
    }
    const raf = requestAnimationFrame(update)
    sc.addEventListener('scroll', update, { passive: true })
    const ro = new ResizeObserver(update)
    ro.observe(sc)
    return () => {
      cancelAnimationFrame(raf)
      sc.removeEventListener('scroll', update)
      ro.disconnect()
    }
  }, [sorted.length])

  const onPlay = useCallback((i: number) => void play(root, sorted, i, props.from), [root, sorted, props.from])
  const onMove = useCallback(
    (i: number, by: number) => {
      const to = Math.min(sorted.length - 1, Math.max(0, i + by))
      list.current?.querySelector<HTMLElement>(`[data-index="${to}"]`)?.focus()
    },
    [sorted.length],
  )

  const heading = (key: SortKey, label: string, end = false) => {
    const on = sort.key === key
    return (
      <button
        key={key}
        onClick={() => setSort((s) => (s.key === key ? { key, desc: !s.desc } : { key, desc: false }))}
        aria-sort={on ? (sort.desc ? 'descending' : 'ascending') : 'none'}
        className={`flex min-w-0 items-center gap-1 truncate rounded-md py-1 text-[12px] ${end ? 'justify-end' : ''} ${on ? 'text-ink' : 'text-ink/55 hover:text-ink'}`}
      >
        {label}
        {on && (
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden className={sort.desc ? 'rotate-180' : ''}>
            <path d="M2 6.5 5 3.5 8 6.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        )}
      </button>
    )
  }

  if (!sorted.length) return null
  const head = (
    <div className={`grid ${COLS} items-center gap-3 border-b border-edge px-2 pb-1.5`}>
      <span />
      {HEADERS.map(([k, l]) => heading(k, l, k === 'time'))}
      <span />
    </div>
  )
  return (
    <div>
      {props.pin ? <InBar>{head}</InBar> : head}
      <div ref={list} role="list" className="relative mt-1" style={{ height: sorted.length * ROW }}>
        {sorted.slice(range.start, range.end).map((t, k) => {
          const i = range.start + k
          return (
            <SongRow
              key={t.path}
              track={t}
              index={i}
              cover={cover(t.dir)}
              now={t.path === now.path ? (now.playing ? 'playing' : 'paused') : 'no'}
              onPlay={onPlay}
              onMove={onMove}
            />
          )
        })}
      </div>
    </div>
  )
})

const SongRow = memo(function SongRow(props: {
  track: CatalogTrack
  index: number
  cover: Cover | undefined
  now: 'no' | 'playing' | 'paused'
  onPlay: (i: number) => void
  onMove: (i: number, by: number) => void
}) {
  const { track: t, index: i, now } = props
  const nav = useNav()
  const { openAlbum } = nav
  const isNow = now !== 'no'
  const ctx = useContextMenu(() => trackActions(nav, t))
  return (
    <div
      onContextMenu={ctx.onContextMenu}
      role="listitem"
      tabIndex={0}
      data-index={i}
      aria-current={isNow || undefined}
      onClick={(e) => {
        if ((e.target as HTMLElement).closest('button, a, input') && (e.target as HTMLElement).closest('button, a, input') !== e.currentTarget) return
        if (isNow) void toggle()
        else props.onPlay(i)
      }}
      onKeyDown={(e) => {
        if (e.target !== e.currentTarget) return
        if (e.key === 'Enter') {
          e.preventDefault()
          props.onPlay(i)
        } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
          e.preventDefault()
          props.onMove(i, e.key === 'ArrowDown' ? 1 : -1)
        }
      }}
      className={`group absolute inset-x-0 grid ${COLS} scroll-mb-4 scroll-mt-16 items-center gap-3 rounded-xl px-2 hover:bg-ink/[0.05] focus-visible:bg-ink/[0.06] ${isNow ? 'text-accent-ink' : ''}`}
      style={{ top: i * ROW, height: ROW }}
    >
      <button
        tabIndex={-1}
        aria-label={now === 'playing' ? `Pause ${t.title}` : `Play ${t.title}`}
        onClick={() => (isNow ? void toggle() : props.onPlay(i))}
        className="relative h-8 w-8 overflow-hidden rounded-md bg-ink/[0.06]"
      >
        {props.cover ? (
          <img src={props.cover.src} alt="" draggable={false} loading="lazy" className="h-full w-full object-cover" />
        ) : (
          <span
            className="grid h-full w-full place-items-center text-[12px] font-semibold text-ink/45"
            style={{ background: 'linear-gradient(135deg, color-mix(in oklab, var(--accent) 22%, transparent), color-mix(in oklab, var(--ink) 4%, transparent))' }}
          >
            {/^\p{L}/u.test((t.album || t.title).trim()) ? (t.album || t.title).trim()[0].toUpperCase() : '♪'}
          </span>
        )}
        <span
          className={`absolute inset-0 grid place-items-center bg-scrim text-ink transition-opacity duration-150 ${
            isNow ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'
          }`}
        >
          {isNow ? (
            <>
              <span className="group-hover:hidden">
                <Equalizer playing={now === 'playing'} />
              </span>
              <span className="hidden group-hover:block">{now === 'playing' ? <IconPause width={14} height={14} /> : <IconPlay width={14} height={14} />}</span>
            </>
          ) : (
            <IconPlay width={14} height={14} />
          )}
        </span>
      </button>
      <span className="truncate text-[13.5px] font-medium">{t.title}</span>
      <span className={`truncate text-[13px] ${isNow ? '' : 'text-ink/60'}`}>{artistsOf(t)}</span>
      <span className="min-w-0 truncate">
        <button
          tabIndex={-1}
          onClick={() => openAlbum(t.dir)}
          onDoubleClick={(e) => e.stopPropagation()}
          className={`max-w-full truncate text-left text-[13px] hover:underline ${isNow ? '' : 'text-ink/60'}`}
        >
          {t.album}
        </button>
      </span>
      <span className={`text-right text-[12.5px] ${isNow ? '' : 'text-ink/55'}`}>{clock(t.duration_ms)}</span>
      <span className="min-w-0">
        <QualityBadge text={quality(t)} />
      </span>
      <TrackMenu track={t} label={t.title} />
      {ctx.menu}
    </div>
  )
})
