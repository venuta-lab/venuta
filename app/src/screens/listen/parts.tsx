import { AnimatePresence, motion } from 'motion/react'
import { memo, useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { CatalogTrack } from '../../lib/engine'
import { setAccent, useCovers, type Cover } from '../../lib/covers'
import { enqueue, play, toggle } from '../../lib/player'
import { useJob } from '../../lib/jobs'
import { IconChevron, IconMore, IconPause, IconPlay, IconShuffle } from '../../components/Icons'
import { CoverImage, NoCover } from '../MusicScreen'
import { coverId, shuffled, smooth, spring, useNav, useNowPlaying, type Album } from './model'

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
export function PlayButtons(props: { tracks: CatalogTrack[]; from: string; dir?: string; label?: string; size?: 'sm' | 'lg' }) {
  const { tracks, from, dir, label = 'Play' } = props
  const sm = props.size === 'sm'
  const { root } = useNav()
  const now = useNowPlaying()
  const isNow = !!dir && now.dir === dir
  return (
    <div className="flex flex-wrap items-center gap-2.5">
      <button
        disabled={!tracks.length}
        onClick={() => (isNow ? void toggle() : void play(root, tracks, 0, from))}
        className={`btn-primary ${sm ? 'btn-sm' : 'btn-lg'}`}
      >
        {isNow && now.playing ? <IconPause /> : <IconPlay />}
        {isNow ? (now.playing ? 'Pause' : 'Resume') : label}
      </button>
      {!sm && (
        <button disabled={tracks.length < 2} onClick={() => void play(root, shuffled(tracks), 0, from)} className="btn btn-lg">
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
  const ctx = useContextMenu(() => albumActions(nav, a))
  return (
    <motion.div
      whileHover={{ y: -6 }}
      transition={spring}
      onHoverStart={() => setAccent(c?.accent)}
      onContextMenu={ctx.onContextMenu}
      className="group relative"
    >
      {ctx.menu}
      <button
        onClick={() => {
          setAccent(c?.accent)
          openAlbum(a.dir, origin)
        }}
        onFocus={() => setAccent(c?.accent)}
        className="block w-full text-left"
      >
        <motion.div
          layoutId={coverId(origin, a.dir)}
          transition={spring}
          className="aspect-square overflow-hidden rounded-2xl bg-ink/[0.06] shadow-xl shadow-black/30"
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
        <motion.button
          whileTap={{ scale: 0.92 }}
          transition={spring}
          aria-label={props.now === 'playing' ? `Pause ${a.title}` : `Play ${a.title}`}
          onClick={() => (props.now !== 'no' ? void toggle() : void play(root, a.tracks, 0, a.title))}
          className={`pointer-events-auto absolute bottom-3 right-3 grid h-11 w-11 place-items-center rounded-full bg-accent text-on-accent shadow-lg shadow-black/30 transition-[opacity,translate] duration-200 focus-visible:translate-y-0 focus-visible:opacity-100 group-hover:translate-y-0 group-hover:opacity-100 ${
            props.now === 'no' ? 'translate-y-1.5 opacity-0' : 'opacity-100'
          }`}
        >
          {props.now === 'playing' ? <IconPause /> : <IconPlay />}
        </motion.button>
      </div>
    </motion.div>
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
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(168px,1fr))] gap-x-5 gap-y-7">
      {albums.map((a) => (
        <AlbumCard key={a.dir} album={a} cover={cover(a.dir)} origin={origin} now={state(a.dir)} subtitle={subtitle} />
      ))}
    </div>
  )
}

/** A row of albums that scrolls sideways, with arrows on hover and a way to see them all. */
export function Shelf(props: {
  title: string
  albums: Album[]
  origin: string
  onSeeAll?: () => void
  limit?: number
  /** Space above, between shelves; off for the first thing on a page. */
  spaced?: boolean
}) {
  const { cover, state } = useCardState()
  const row = useRef<HTMLDivElement>(null)
  const [ends, setEnds] = useState({ start: true, end: false })
  const shown = props.albums.slice(0, props.limit ?? 24)

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
  }, [])

  const by = (dir: 1 | -1) => {
    const el = row.current
    if (el) el.scrollBy({ left: dir * el.clientWidth * 0.85, behavior: smooth() })
  }

  const arrow = (dir: 1 | -1) => (
    <button
      aria-label={dir < 0 ? 'Scroll left' : 'Scroll right'}
      tabIndex={-1}
      onClick={() => by(dir)}
      className={`popover absolute top-[64px] z-10 grid h-10 w-10 place-items-center rounded-full text-ink/80 opacity-0 transition-opacity duration-200 hover:text-ink group-hover/shelf:opacity-100 focus-visible:opacity-100 ${
        dir < 0 ? '-left-4' : '-right-4'
      }`}
    >
      <IconChevron className={dir < 0 ? 'rotate-180' : ''} />
    </button>
  )

  if (!shown.length) return null
  return (
    <section className={props.spaced === false ? '' : 'mt-10'}>
      <div className="mb-3.5 flex items-baseline gap-3">
        <h2 className="title text-[19px]">{props.title}</h2>
        <span className="text-[12.5px] text-ink/50">{props.albums.length.toLocaleString('en-IN')}</span>
        {props.onSeeAll && (
          <button onClick={props.onSeeAll} className="ml-auto rounded-full px-3 py-1 text-[12.5px] text-ink/65 hover:bg-ink/[0.08] hover:text-ink">
            See all
          </button>
        )}
      </div>
      <div className="group/shelf relative">
        {!ends.start && arrow(-1)}
        <div
          ref={row}
          onScroll={measure}
          className="no-scrollbar -mx-3 -my-3 flex snap-x gap-5 overflow-x-auto px-3 py-3"
          style={{ scrollPaddingInline: 12 }}
        >
          {shown.map((a) => (
            <div key={a.dir} className="w-[168px] shrink-0 snap-start">
              <AlbumCard album={a} cover={cover(a.dir)} origin={props.origin} now={state(a.dir)} />
            </div>
          ))}
        </div>
        {!ends.end && arrow(1)}
      </div>
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

// --- The "…" menu on a song ------------------------------------------------------------------

/** "…" on a song: play next, add to queue, go to artist, edit details. */
/** A menu opened with the right mouse button, at the pointer. */
export function useContextMenu(items: () => [string, () => void][]) {
  const [at, setAt] = useState<{ x: number; y: number; up: boolean; items: [string, () => void][] } | null>(null)
  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault()
    const up = e.clientY + 230 > window.innerHeight
    setAt({ x: Math.min(e.clientX + 180, window.innerWidth - 8), y: e.clientY, up, items: items() })
  }
  const close = useCallback(() => setAt(null), [])
  const menu = createPortal(<AnimatePresence>{at && <Menu at={at} items={at.items} onClose={close} />}</AnimatePresence>, document.body)
  return { onContextMenu, menu }
}

/** What can be done with a whole album (right-click a cover). */
export function albumActions(nav: ReturnType<typeof useNav>, a: Album): [string, () => void][] {
  return [
    ['Play', () => void play(nav.root, a.tracks, 0, a.title)],
    ['Shuffle', () => void play(nav.root, shuffled(a.tracks), 0, a.title)],
    ['Play next', () => void enqueue(nav.root, a.tracks, true)],
    ['Add to queue', () => void enqueue(nav.root, a.tracks, false)],
    ['Go to artist', () => nav.openArtist(a.artist)],
    ['Edit details', () => nav.editDetails(a.dir)],
  ]
}

/** What can be done with one song. */
export function trackActions(nav: ReturnType<typeof useNav>, track: CatalogTrack): [string, () => void][] {
  const artist = track.artists[0] || track.album_artist
  return [
    ['Play next', () => void enqueue(nav.root, [track], true)],
    ['Add to queue', () => void enqueue(nav.root, [track], false)],
    ...(artist ? ([['Go to artist', () => nav.openArtist(artist)]] as [string, () => void][]) : []),
    ['Go to album', () => nav.openAlbum(track.dir)],
    ['Edit details', () => nav.editDetails(track.dir)],
  ]
}

export function TrackMenu({ track, label }: { track: CatalogTrack; label: string }) {
  const nav = useNav()
  const button = useRef<HTMLButtonElement>(null)
  const [at, setAt] = useState<{ x: number; y: number; up: boolean } | null>(null)

  const openMenu = () => {
    const r = button.current?.getBoundingClientRect()
    if (!r) return
    const up = r.bottom + 190 > window.innerHeight
    setAt({ x: Math.min(r.right, window.innerWidth - 8), y: up ? r.top - 6 : r.bottom + 6, up })
  }
  const close = useCallback((refocus = true) => {
    setAt(null)
    if (refocus) button.current?.focus()
  }, [])
  const items = trackActions(nav, track)

  return (
    <>
      <button
        ref={button}
        aria-label={`More for ${label}`}
        aria-haspopup="menu"
        aria-expanded={!!at}
        onClick={(e) => {
          e.stopPropagation()
          if (at) close()
          else openMenu()
        }}
        onDoubleClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === 'Enter' && e.stopPropagation()}
        className={`grid h-8 w-8 place-items-center rounded-full text-ink/60 hover:bg-ink/[0.1] hover:text-ink focus-visible:opacity-100 group-hover:opacity-100 ${at ? 'opacity-100' : 'opacity-0'}`}
      >
        <IconMore />
      </button>
      {createPortal(
        <AnimatePresence>
          {at && <Menu at={at} items={items} onClose={close} />}
        </AnimatePresence>,
        document.body,
      )}
    </>
  )
}

function Menu(props: { at: { x: number; y: number; up: boolean }; items: [string, () => void][]; onClose: (refocus?: boolean) => void }) {
  const ref = useRef<HTMLDivElement>(null)
  const { onClose } = props
  useEffect(() => {
    ref.current?.querySelector<HTMLButtonElement>('button')?.focus()
  }, [])
  useEffect(() => {
    const away = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose(false)
    }
    const gone = () => onClose(false)
    document.addEventListener('pointerdown', away, true)
    window.addEventListener('scroll', gone, true)
    window.addEventListener('resize', gone)
    window.addEventListener('blur', gone)
    return () => {
      document.removeEventListener('pointerdown', away, true)
      window.removeEventListener('scroll', gone, true)
      window.removeEventListener('resize', gone)
      window.removeEventListener('blur', gone)
    }
  }, [onClose])

  const onKey = (e: React.KeyboardEvent) => {
    const buttons = [...(ref.current?.querySelectorAll<HTMLButtonElement>('button') ?? [])]
    const i = buttons.indexOf(document.activeElement as HTMLButtonElement)
    if (e.key === 'Escape' || e.key === 'Tab') {
      e.preventDefault()
      e.stopPropagation()
      onClose()
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      const n = buttons.length
      buttons[(i + (e.key === 'ArrowDown' ? 1 : -1) + n) % n]?.focus()
    } else if (e.key === 'Home' || e.key === 'End') {
      e.preventDefault()
      buttons[e.key === 'Home' ? 0 : buttons.length - 1]?.focus()
    }
    // Keys used here never reach the rows or the screen behind the menu.
    if (e.key === 'Enter') e.stopPropagation()
  }

  return (
    <motion.div
      ref={ref}
      role="menu"
      onKeyDown={onKey}
      initial={{ opacity: 0, scale: 0.96, y: props.at.up ? 4 : -4 }}
      animate={{ opacity: 1, scale: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.96, transition: { duration: 0.12 } }}
      transition={spring}
      className="popover fixed z-50 min-w-[180px] rounded-2xl p-1.5"
      style={{
        left: props.at.x,
        top: props.at.y,
        translate: props.at.up ? '-100% -100%' : '-100% 0',
        transformOrigin: props.at.up ? 'bottom right' : 'top right',
      }}
    >
      {props.items.map(([label, run]) => (
        <button
          key={label}
          role="menuitem"
          onClick={() => {
            props.onClose()
            run()
          }}
          className="block w-full rounded-xl px-3 py-2 text-left text-[13px] text-ink/85 hover:bg-ink/[0.08] hover:text-ink focus-visible:bg-ink/[0.08] focus-visible:outline-none"
        >
          {label}
        </button>
      ))}
    </motion.div>
  )
}
