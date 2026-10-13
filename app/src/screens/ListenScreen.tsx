import { motion } from 'motion/react'
import { useCallback, useDeferredValue, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { musicCatalog, type CatalogTrack, type HealthReport } from '../lib/engine'
import { current, usePlayer } from '../lib/player'
import { IconSearch } from '../components/Icons'
import { Reading } from './MusicScreen'
import { AlbumPage } from './listen/AlbumPage'
import { SongList } from './listen/SongList'
import { AlbumsView, ArtistPage, ArtistsView, HomeView, SearchResults } from './listen/views'
import { buildLibrary, fmtNum, NavContext, NowContext, scrollerOf, search, spring, type Nav } from './listen/model'

type Tab = 'home' | 'albums' | 'artists' | 'songs'
const TABS: [Tab, string][] = [
  ['home', 'Home'],
  ['albums', 'Albums'],
  ['artists', 'Artists'],
  ['songs', 'Songs'],
]

type Page = { kind: 'album'; dir: string; origin?: string } | { kind: 'artist'; name: string }
/** A page opened over the tabs, with where the screen was scrolled when it opened. */
type Entry = { page: Page; scroll: number }

interface Props {
  root: string
  report: HealthReport | null
  busy: string
  searchRef: React.Ref<HTMLInputElement>
  /** An album to show (from the palette); cleared with `onOpenedAlbum` once shown. */
  openAlbum: string | null
  onOpenedAlbum: () => void
  onEditDetails: (dir: string) => void
}

/** Music: your library to listen to. Home, albums, artists and songs, and search across them. */
export function ListenScreen(props: Props) {
  return (
    <NowPlaying>
      <Listen {...props} />
    </NowPlaying>
  )
}

/**
 * What is playing, for the rows and covers. The player streams its position many times a
 * second; only this wrapper hears that, and the lists re-render only when the song changes.
 */
function NowPlaying({ children }: { children: ReactNode }) {
  const player = usePlayer()
  const song = current(player)
  const path = song?.path ?? ''
  const dir = song?.dir ?? ''
  const playing = !!song && player.playing
  const value = useMemo(() => ({ path, dir, playing }), [path, dir, playing])
  return <NowContext value={value}>{children}</NowContext>
}

function Listen(props: Props) {
  const { root, report, busy, searchRef, openAlbum, onOpenedAlbum } = props
  const box = useRef<HTMLDivElement>(null)
  const [tracks, setTracks] = useState<CatalogTrack[] | null>(null)
  const [tab, setTab] = useState<Tab>('home')
  const [stack, setStack] = useState<Entry[]>([])
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState('all')
  const [fadeIn, setFadeIn] = useState(true)
  const [shown, setShown] = useState<string | null>(null)
  const typed = useDeferredValue(query)

  // A new report means the folder was read again: read the catalog again with it.
  useEffect(() => {
    let live = true
    musicCatalog(root)
      .then((t) => live && setTracks(t))
      .catch(() => live && setTracks([]))
    return () => {
      live = false
    }
  }, [root, report])

  const lib = useMemo(() => buildLibrary(tracks ?? []), [tracks])
  const withCover = useMemo(() => new Set((report?.albums ?? []).filter((a) => a.cover_track).map((a) => a.dir)), [report])
  const hasCover = useCallback((dir: string) => withCover.has(dir), [withCover])

  // Pages over the tabs: going in starts at the top, coming back returns to where you were.
  const restore = useRef(0)
  const depth = useRef(0)
  const top = stack.at(-1)
  useLayoutEffect(() => {
    const sc = scrollerOf(box.current)
    if (sc) sc.scrollTop = stack.length < depth.current ? restore.current : 0
    depth.current = stack.length
  }, [stack, top])
  useLayoutEffect(() => {
    const sc = scrollerOf(box.current)
    if (sc) sc.scrollTop = 0
  }, [tab])

  const push = useCallback((page: Page) => {
    const scroll = scrollerOf(box.current)?.scrollTop ?? 0
    setStack((s) => {
      const last = s.at(-1)?.page
      const same =
        last &&
        last.kind === page.kind &&
        (page.kind === 'album' ? last.kind === 'album' && last.dir === page.dir : last.kind === 'artist' && last.name.toLowerCase() === page.name.toLowerCase())
      return same ? s : [...s, { page, scroll }]
    })
  }, [])
  const back = useCallback(() => {
    const last = stack.at(-1)
    if (!last) return
    restore.current = last.scroll
    setFadeIn(false)
    setStack(stack.slice(0, -1))
  }, [stack])
  const leavePages = () => {
    restore.current = 0
    setStack([])
  }

  const edit = useRef(props.onEditDetails)
  useEffect(() => {
    edit.current = props.onEditDetails
  })
  const nav: Nav = useMemo(
    () => ({
      root,
      openAlbum: (dir, origin) => push({ kind: 'album', dir, origin }),
      openArtist: (name) => push({ kind: 'artist', name }),
      editDetails: (dir) => edit.current(dir),
    }),
    [root, push],
  )

  // An album asked for from elsewhere (the palette): show it once the catalog has it.
  if (openAlbum !== shown) {
    if (!openAlbum) setShown(null)
    else if (lib.byDir.has(openAlbum)) {
      setShown(openAlbum)
      setQuery('')
      setStack([{ page: { kind: 'album', dir: openAlbum }, scroll: 0 }])
    }
  }
  useEffect(() => {
    if (openAlbum && shown === openAlbum) onOpenedAlbum()
  }, [openAlbum, shown, onOpenedAlbum])

  // Escape (or the mouse's back button) closes the page on top.
  useEffect(() => {
    if (!stack.length) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || e.defaultPrevented) return
      const t = e.target
      if (t instanceof HTMLElement && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return
      if (document.querySelector('[role="dialog"], [role="menu"]')) return
      back()
    }
    const onMouse = (e: MouseEvent) => {
      if (e.button === 3) {
        e.preventDefault()
        back()
      }
    }
    window.addEventListener('keydown', onKey)
    window.addEventListener('mouseup', onMouse)
    return () => {
      window.removeEventListener('keydown', onKey)
      window.removeEventListener('mouseup', onMouse)
    }
  }, [stack, back])

  const results = useMemo(() => search(lib, typed), [lib, typed])
  const seeAll = useCallback((f: string) => {
    setFilter(f)
    setFadeIn(true)
    setTab('albums')
  }, [])

  const s = report?.summary
  const line = tracks
    ? `${fmtNum(lib.albums.length)} albums, ${fmtNum(lib.tracks.length)} songs, ${fmtNum(lib.hiRes.length)} hi-res`
    : s
      ? `${fmtNum(s.albums)} albums, ${fmtNum(s.tracks)} songs`
      : busy || 'Reading your music…'

  const body = (() => {
    if (!report) return <Reading busy={busy} />
    if (!tracks) return <Loading />
    if (!lib.tracks.length)
      return (
        <p className="py-20 text-center text-[14px] text-ink/55">
          No songs to play here yet. Bring music in from Organize and it shows up here.
        </p>
      )
    const page = top?.page
    if (page?.kind === 'album') {
      const album = lib.byDir.get(page.dir)
      return album ? (
        <AlbumPage key={album.dir} album={album} origin={page.origin} onBack={back} />
      ) : (
        <p className="py-20 text-center text-[14px] text-ink/55">That album is no longer in this library.</p>
      )
    }
    if (page?.kind === 'artist') return <ArtistPage key={page.name} lib={lib} name={page.name} onBack={back} />
    const view = typed.trim() ? (
      <SearchResults query={typed} albums={results.albums} artists={results.artists} tracks={results.tracks} />
    ) : tab === 'home' ? (
      <HomeView root={root} lib={lib} hasCover={hasCover} onSeeAll={seeAll} />
    ) : tab === 'albums' ? (
      <AlbumsView lib={lib} filter={filter} onFilter={setFilter} />
    ) : tab === 'artists' ? (
      <ArtistsView artists={lib.artists} />
    ) : (
      <div className="pb-16">
        <SongList tracks={lib.tracks} from="Songs" sortBy="title" />
      </div>
    )
    return (
      <motion.div
        key={typed.trim() ? 'search' : tab}
        initial={fadeIn ? { opacity: 0, y: 8 } : false}
        animate={{ opacity: 1, y: 0 }}
        transition={spring}
      >
        {view}
      </motion.div>
    )
  })()

  return (
    <NavContext value={nav}>
      <div ref={box} className="mx-auto max-w-[1180px]">
        <header className="pb-5 pt-6">
          <h1 className="text-[40px] font-semibold leading-none tracking-[-0.035em]">Music</h1>
          <p className="mt-2.5 text-[14px] text-ink/60">{line}</p>
        </header>

        <div className="mb-6 flex flex-wrap items-center gap-3">
          <div role="tablist" aria-label="Music views" className="glass flex rounded-full p-1">
            {TABS.map(([id, label]) => {
              const on = tab === id && !query.trim() && !top
              return (
                <button
                  key={id}
                  role="tab"
                  aria-selected={on}
                  onClick={() => {
                    setQuery('')
                    leavePages()
                    setFadeIn(true)
                    if (id === 'albums' && tab !== 'albums') setFilter('all')
                    setTab(id)
                  }}
                  className={`relative rounded-full px-4 py-1.5 text-[13px] ${on ? 'text-ink' : 'text-ink/60 hover:text-ink'}`}
                >
                  {tab === id && (
                    <motion.span
                      layoutId="listen-tab"
                      transition={spring}
                      className={`absolute inset-0 rounded-full ${on ? 'bg-ink/[0.14]' : 'bg-ink/[0.06]'}`}
                    />
                  )}
                  <span className="relative">{label}</span>
                </button>
              )
            })}
          </div>
          <div className="glass ml-auto flex min-w-[240px] max-w-[380px] flex-1 items-center gap-2 rounded-full px-4">
            <IconSearch className="shrink-0 text-ink/45" />
            <input
              ref={searchRef}
              id="listen-search"
              type="search"
              value={query}
              onChange={(e) => {
                setQuery(e.target.value)
                setFadeIn(true)
                if (stack.length) leavePages()
              }}
              onKeyDown={(e) => {
                if (e.key === 'Escape') {
                  setQuery('')
                  e.currentTarget.blur()
                }
              }}
              placeholder="Search albums, artists, songs"
              aria-label="Search your music"
              className="min-w-0 flex-1 bg-transparent py-2.5 text-[14px] text-ink outline-none placeholder:text-ink/45 [&::-webkit-search-cancel-button]:hidden"
            />
            <kbd>/</kbd>
          </div>
        </div>

        {body}
      </div>
    </NavContext>
  )
}

/** The catalog is a moment behind the report: the wall's shape while it arrives. */
function Loading() {
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(168px,1fr))] gap-x-5 gap-y-7" aria-hidden>
      {Array.from({ length: 10 }, (_, i) => (
        <div key={i}>
          <div className="shimmer aspect-square rounded-2xl" style={{ animationDelay: `${i * 80}ms` }} />
          <div className="shimmer mt-3 h-3 w-3/4 rounded-full" />
          <div className="shimmer mt-2 h-2.5 w-1/2 rounded-full" />
        </div>
      ))}
    </div>
  )
}
