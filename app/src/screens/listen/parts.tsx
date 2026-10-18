import { motion } from 'motion/react'
import { memo, useLayoutEffect, useRef, useState } from 'react'
import type { CatalogTrack } from '../../lib/engine'
import { useCovers, type Cover } from '../../lib/covers'
import { playAll, toItem, toggle, type From } from '../../lib/player'
import { dragSource } from '../../lib/drag'
import { albumWork, menuFor, songFacts, songWork } from '../../lib/actions'
import { closeMenu, menuFromEvent, useMenuOpen, type MenuGroups } from '../../lib/menu'
import { useJob } from '../../lib/jobs'
import { IconChevron, IconMore, IconPause, IconPlay, IconShuffle } from '../../components/Icons'
import { CoverImage, NoCover } from '../MusicScreen'
import { WindowedGrid } from '../../components/WindowedGrid'
import { albumFrom, coverId, smooth, spring, useCardWidth, useNav, useNearView, useNowPlaying, type Album } from './model'

export function QualityBadge({ text }: { text: string }) {
  if (!text) return null
  return (
    <span className="inline-flex shrink-0 items-center rounded-md bg-ink/[0.08] px-1.5 py-[3px] text-[11px] font-medium leading-none text-ink/70">
      {text}
    </span>
  )
}

/** Above CD quality. `onArt` sits over a cover, so it brings its own surface. */
export function HiResPill({ onArt = false }: { onArt?: boolean }) {
  return (
    <span
      className={`inline-flex shrink-0 items-center rounded-full px-2 py-[3px] text-[11px] font-medium leading-none ${onArt ? 'on-art' : 'text-accent-ink'}`}
      style={onArt ? undefined : { background: 'color-mix(in oklab, var(--accent) 20%, transparent)' }}
    >
      Hi-Res
    </span>
  )
}

/** Three bars in the accent colour: bouncing while it plays, still when paused. */
export function Equalizer({ playing, className = '' }: { playing: boolean; className?: string }) {
  return (
    <span aria-hidden className={`inline-flex h-3.5 items-end gap-[2px] ${playing ? 'eq-on' : ''} ${className}`}>
      {[0, 1, 2].map((i) => (
        <span key={i} className="eq-bar h-full w-[3px] rounded-full bg-accent" />
      ))}
    </span>
  )
}

/** The primary action. Plays the album, or pauses and resumes it when it is the one playing. */
export function PlayButtons(props: { tracks: CatalogTrack[]; from: From; dir?: string; label?: string; size?: 'sm' | 'lg' }) {
  const { tracks, from, dir, label = 'Play' } = props
  const sm = props.size === 'sm'
  const { root } = useNav()
  const now = useNowPlaying()
  const isNow = !!dir && now.dir === dir
  return (
    <div className="flex flex-wrap items-center gap-2.5">
      <button
        disabled={!tracks.length}
        onClick={() => (isNow ? void toggle() : void playAll(root, tracks, from))}
        className={`btn-primary play-main ${sm ? 'btn-sm' : 'btn-lg'}`}
      >
        {isNow && now.playing ? <IconPause /> : <IconPlay />}
        {isNow ? (now.playing ? 'Pause' : 'Resume') : label}
      </button>
      {!sm && (
        <button disabled={tracks.length < 2} onClick={() => void playAll(root, tracks, from, 'shuffle')} className="btn btn-lg shuffle-main">
          <IconShuffle />
          Shuffle
        </button>
      )}
    </div>
  )
}

export { Chip } from '../../components/ui'

/** The cover itself, or a shimmer while covers are still being made, or the lettered tile. */
export function Art({ cover, album, round = false }: { cover: Cover | undefined; album: { title: string }; round?: boolean }) {
  const making = !!useJob('covers')
  if (cover) return <CoverImage src={cover.src} title={album.title} />
  if (making) return <div className={`shimmer h-full w-full ${round ? 'rounded-full' : ''}`} />
  return <NoCover title={album.title} />
}

/**
 * An album on a shelf or in a grid. Hovering lifts it, tints the app with its cover and shows a
 * Play button; clicking opens the album.
 */
export const AlbumCard = memo(function AlbumCard(props: {
  album: Album
  cover: Cover | undefined
  origin: string
  /** The album playing now, and whether it is playing. */
  now: 'no' | 'playing' | 'paused'
  subtitle?: 'artist' | 'year'
}) {
  const { album: a, cover: c, origin } = props
  const nav = useNav()
  const { root, openAlbum } = nav
  const sub = props.subtitle === 'year' ? a.year : `${a.artist}${a.year ? `, ${a.year}` : ''}`
  // The lift on hover and the press on Play are CSS (.card, .card-play in index.css), so a page
  // of cards is plain elements; motion is kept for the cover's flight into the album page.
  return (
    <div
      onContextMenu={(e) => menuFromEvent(e, menuFor('album', {}, albumWork(nav, a)))}
      {...dragSource(() => ({ label: a.title, count: a.tracks.length, cover: c?.src, songs: async () => a.tracks.map(toItem) }))}
      className="card group relative"
    >
      <button
        onClick={() => {
          openAlbum(a.dir, origin)
        }}
        className="block w-full text-left"
      >
        <motion.div
          layoutId={coverId(origin, a.dir)}
          transition={spring}
          className="card-art aspect-square overflow-hidden rounded-2xl bg-ink/[0.06] shadow-xl shadow-black/30"
        >
          <Art cover={c} album={a} />
        </motion.div>
        <div className={`mt-2.5 truncate text-[14px] font-medium ${props.now !== 'no' ? 'text-accent-ink' : ''}`}>{a.title}</div>
        <div className="truncate text-[12.5px] text-ink/55">{sub || ' '}</div>
      </button>
      <div className="pointer-events-none absolute inset-x-0 top-0 aspect-square">
        {a.hiRes && (
          <span className="absolute left-2.5 top-2.5">
            <HiResPill onArt />
          </span>
        )}
        <button
          aria-label={props.now === 'playing' ? `Pause ${a.title}` : `Play ${a.title}`}
          onClick={() => (props.now !== 'no' ? void toggle() : void playAll(root, a.tracks, albumFrom(a)))}
          className={`card-play accent-fill pointer-events-auto absolute bottom-3 right-3 grid h-11 w-11 place-items-center rounded-full text-on-accent shadow-lg shadow-black/30 transition-[opacity,translate,scale] duration-200 focus-visible:translate-y-0 focus-visible:opacity-100 group-hover:translate-y-0 group-hover:opacity-100 active:scale-[0.92] ${
            props.now === 'no' ? 'translate-y-1.5 opacity-0' : 'opacity-100'
          }`}
        >
          {props.now === 'playing' ? <IconPause /> : <IconPlay />}
        </button>
      </div>
    </div>
  )
})

/** Cards for many albums: one cover lookup and one "now" check for the whole set. */
function useCardState() {
  const cover = useCovers(320)
  const now = useNowPlaying()
  const state = (dir: string): 'no' | 'playing' | 'paused' => (now.dir !== dir ? 'no' : now.playing ? 'playing' : 'paused')
  return { cover, state }
}

export function AlbumGrid({ albums, origin, subtitle }: { albums: Album[]; origin: string; subtitle?: 'artist' | 'year' }) {
  const { cover, state } = useCardState()
  // A few rows at a time: thousands of albums open as fast as a dozen.
  return (
    <WindowedGrid
      items={albums}
      keyOf={(a) => a.dir}
      min={168}
      gapX={20}
      gapY={28}
      below={50}
      label="Albums"
      render={(a) => <AlbumCard album={a} cover={cover(a.dir)} origin={origin} now={state(a.dir)} subtitle={subtitle} />}
    />
  )
}

/** How many cards a shelf draws at first. A card and the gap after it is its width + 20 px. */
const FIRST_CARDS = 8
const GAP = 20

/** A row of albums that scrolls sideways, with arrows on hover and a way to see them all. */
export function Shelf(props: {
  title: string
  albums: Album[]
  origin: string
  /** Other cards among the albums (playlists you played), placed by when (see `at`). */
  more?: { key: string; at: number; node: React.ReactNode }[]
  /** When an album was played, to place it among `more`. */
  at?: (dir: string) => number
  onSeeAll?: () => void
  limit?: number
  /** Space above, between shelves; off for the first thing on a page. */
  spaced?: boolean
}) {
  const { cover, state } = useCardState()
  const width = useCardWidth()
  const step = width + GAP
  const row = useRef<HTMLDivElement>(null)
  // Home holds a dozen shelves; each fills in just before it scrolls into view.
  const box = useRef<HTMLElement>(null)
  const near = useNearView(box)
  const [ends, setEnds] = useState({ start: true, end: false })
  const shown = [
    ...props.albums.map((a) => ({ key: a.dir, at: props.at?.(a.dir) ?? 0, album: a as Album | undefined, node: null as React.ReactNode })),
    ...(props.more ?? []).map((m) => ({ ...m, album: undefined })),
  ]
    .sort((x, y) => (props.more?.length ? y.at - x.at : 0))
    .slice(0, props.limit ?? 24)
  // The cards that fit and a few more are drawn; the rest of the row holds their room and they
  // come in a few at a time as it is scrolled toward them. A shelf of forty (search results,
  // Home) never mounts forty cards at once.
  const [upto, setUpto] = useState(FIRST_CARDS)
  const reach = (el: HTMLElement) => {
    const want = Math.ceil((el.scrollLeft + el.clientWidth) / step) + 4
    setUpto((n) => (want > n ? Math.min(shown.length, want) : n))
  }
  const drawn = shown.slice(0, upto)

  const measure = () => {
    const el = row.current
    if (!el) return
    const start = el.scrollLeft < 4
    const end = el.scrollLeft + el.clientWidth >= el.scrollWidth - 4
    setEnds((e) => (e.start === start && e.end === end ? e : { start, end }))
  }
  useLayoutEffect(() => {
    const el = row.current
    if (!el) return
    const ro = new ResizeObserver(measure)
    ro.observe(el)
    return () => ro.disconnect()
  }, [near])

  const by = (dir: 1 | -1) => {
    const el = row.current
    if (el) el.scrollBy({ left: dir * el.clientWidth * 0.85, behavior: smooth() })
  }

  const arrow = (dir: 1 | -1) => (
    <button
      aria-label={dir < 0 ? 'Scroll left' : 'Scroll right'}
      tabIndex={-1}
      onClick={() => by(dir)}
      style={{ top: Math.round(width * 0.38) }}
      className={`popover absolute z-10 grid h-10 w-10 place-items-center rounded-full text-ink/80 opacity-0 transition-opacity duration-200 hover:text-ink group-hover/shelf:opacity-100 focus-visible:opacity-100 ${
        dir < 0 ? '-left-4' : '-right-4'
      }`}
    >
      <IconChevron className={dir < 0 ? 'rotate-180' : ''} />
    </button>
  )

  if (!shown.length) return null
  return (
    <section ref={box} className={props.spaced === false ? '' : 'mt-10'}>
      <div className="mb-3.5 flex items-baseline gap-3">
        <h2 className="title text-[19px]">{props.title}</h2>
        <span className="text-[12.5px] text-ink/50">{(props.albums.length + (props.more?.length ?? 0)).toLocaleString('en-IN')}</span>
        {props.onSeeAll && (
          <button onClick={props.onSeeAll} className="ml-auto rounded-full px-3 py-1 text-[12.5px] text-ink/65 hover:bg-ink/[0.08] hover:text-ink">
            See all
          </button>
        )}
      </div>
      {!near ? (
        // A card's height (cover, title, line) holds the place until the row is drawn.
        <div style={{ height: width + 50 }} />
      ) : (
      <div className="group/shelf relative">
        {!ends.start && arrow(-1)}
        <div
          ref={row}
          onScroll={(e) => {
            reach(e.currentTarget)
            measure()
          }}
          className="no-scrollbar -mx-3 -my-3 flex snap-x gap-5 overflow-x-auto px-3 py-3"
          style={{ scrollPaddingInline: 12 }}
        >
          {drawn.map(({ key, album: a, node }) => (
            <div key={key} className="shrink-0 snap-start" style={{ width }}>
              {a ? <AlbumCard album={a} cover={cover(a.dir)} origin={props.origin} now={state(a.dir)} /> : node}
            </div>
          ))}
          {/* The room of the cards not drawn yet, so the row scrolls (and says there is more) as if they were. */}
          {shown.length > drawn.length && (
            <div aria-hidden className="shrink-0" style={{ width: (shown.length - drawn.length) * step - GAP }} />
          )}
        </div>
        {!ends.end && arrow(1)}
      </div>
      )}
    </section>
  )
}

/** A round picture for an artist: their first album's cover, or their initials. */
export function Avatar({ name, cover }: { name: string; cover: Cover | undefined }) {
  return (
    <div className="aspect-square overflow-hidden rounded-full bg-ink/[0.06] shadow-xl shadow-black/25">
      {cover ? <CoverImage src={cover.src} title={name} /> : <NoCover title={name} />}
    </div>
  )
}

// --- Menus ------------------------------------------------------------------------------------

/**
 * A right-click menu (and a "…" button's): opens the window's one menu (components/ContextMenu)
 * with these items, at the pointer or under the button. Items are groups from lib/actions.ts,
 * or a plain list of [label, run] for menus outside the music (themes).
 */
export function useContextMenu(items: () => MenuGroups | [string, () => void][]) {
  const onContextMenu = (e: React.MouseEvent) => {
    const list = items()
    const groups: MenuGroups = typeof list[0]?.[0] === 'string'
      ? [(list as [string, () => void][]).map(([label, run]) => ({ label, run }))]
      : (list as MenuGroups)
    menuFromEvent(e, groups)
  }
  return { onContextMenu, menu: null }
}

/** "…" on a song: the same menu as right-clicking it. `play`: play it within its list. */
export function TrackMenu({ track, label, play }: { track: CatalogTrack; label: string; play?: () => void }) {
  const nav = useNav()
  // The button itself (state, not a ref: whether its menu is open is read while drawing).
  const [button, setButton] = useState<HTMLButtonElement | null>(null)
  const open = useMenuOpen()
  const shown = !!open && !!button && open.back === button
  return (
    <button
      ref={setButton}
      aria-label={`More for ${label}`}
      aria-haspopup="menu"
      aria-expanded={shown}
      onClick={(e) => {
        e.stopPropagation()
        if (shown) closeMenu()
        else menuFromEvent(e, menuFor('song', songFacts(track), songWork(nav, track, play)))
      }}
      onDoubleClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => e.key === 'Enter' && e.stopPropagation()}
      className={`grid h-8 w-8 place-items-center rounded-full text-ink/60 hover:bg-ink/[0.1] hover:text-ink focus-visible:opacity-100 group-hover:opacity-100 ${shown ? 'opacity-100' : 'opacity-0'}`}
    >
      <IconMore />
    </button>
  )
}
