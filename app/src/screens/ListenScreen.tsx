import { useEscape } from '../lib/layers'
import { motion } from 'motion/react'
import { useCallback, useDeferredValue, useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from 'react'
import { type HealthReport } from '../lib/engine'
import { useCatalog } from '../lib/catalog'
import { current, usePlayer } from '../lib/player'
import { IconSearch } from '../components/Icons'
import { Page, Segmented } from '../components/ui'
import { Reading } from './MusicScreen'
import { AlbumPage } from './listen/AlbumPage'
import { SongList } from './listen/SongList'
import { AlbumsView, ArtistPage, ArtistsView, HomeView, SearchResults } from './listen/views'
import { buildLibrary, fmtNum, NavContext, NowContext, scrollerOf, search, spring, type Nav } from './listen/model'
import { PlaylistsScreen, showPlaylists, usePlaylistsKind } from './PlaylistsScreen'
import { useLyricHits } from '../lib/lyricsSearch'

export type Tab = 'home' | 'albums' | 'artists' | 'songs' | 'playlists'
const TABS: [Tab, string][] = [
  ['home', 'Home'],
  ['albums', 'Albums'],
  ['artists', 'Artists'],
  ['songs', 'Songs'],
  ['playlists', 'Playlists'],
]

/*
  A view of Music asked for from outside (a playlist to open, the palette's Playlists): taken
  when the screen shows, or at once if it is showing.
*/
let askedTab: Tab | null = null
const tabListeners = new Set<() => void>()
export function showMusicTab(tab: Tab) {
  askedTab = tab
  kept.tab = tab
  tabListeners.forEach((l) => l())
}
const onTabAsked = (l: () => void) => {
  tabListeners.add(l)
  return () => {
    tabListeners.delete(l)
  }
}

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
  onOpenPlaylist: (name: string) => void
  onPlaylists: () => void
  /** Bumped when Music is chosen again while already open: back to its start. */
  reset?: number
}

// The view you were on, kept while you visit other places (the screen itself is remounted).
const kept: { tab: Tab; filter: string } = { tab: 'home', filter: 'all' }

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
  const [tab, setTab] = useState<Tab>(kept.tab)
  const [stack, setStack] = useState<Entry[]>([])
  const [query, setQuery] = useState('')
  const [filter, setFilter] = useState(kept.filter)
  useEffect(() => {
    kept.tab = tab
    kept.filter = filter
  }, [tab, filter])
  const [fadeIn, setFadeIn] = useState(true)
  const [shown, setShown] = useState<string | null>(null)
  const typed = useDeferredValue(query)
  const playlistsAt = usePlaylistsKind()

  // A new report means the folder was read again: the catalog is read again with it (shared
  // with Playlists and the add-to-playlist picker).
  const tracks = useCatalog(root, report)

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

  // Escape (when nothing is open over it) or the mouse's back button closes the page on top.
  useEscape(stack.length > 0, back)
  useEffect(() => {
    if (!stack.length) return
    const onMouse = (e: MouseEvent) => {
      if (e.button === 3) {
        e.preventDefault()
        back()
      }
    }
    window.addEventListener('mouseup', onMouse)
    return () => window.removeEventListener('mouseup', onMouse)
  }, [stack, back])

  // Asked from outside for a view (Playlists, with a playlist open).
  const tabAsked = useSyncExternalStore(onTabAsked, () => askedTab)
  useEffect(() => {
    if (!tabAsked) return
    askedTab = null
    restore.current = 0
    setStack([])
    setQuery('')
    setFadeIn(true)
    setTab(tabAsked)
  }, [tabAsked])

  // Music chosen again in the rail: close pages and search, back to the top.
  const resets = useRef(props.reset)
  useEffect(() => {
    if (props.reset === resets.current) return
    resets.current = props.reset
    restore.current = 0
    setStack([])
    setQuery('')
    if (tab === 'playlists') showPlaylists({ kind: 'home' })
    scrollerOf(box.current)?.scrollTo({ top: 0, behavior: 'smooth' })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.reset])

  const results = useMemo(() => search(lib, typed), [lib, typed])
  const lyricHits = useLyricHits(root, typed)
  // Songs found by their words, apart from the ones already found by name.
  const inLyrics = useMemo(() => {
    const byName = new Set(results.tracks.map((t) => t.path))
    const byPath = new Map(lib.tracks.map((t) => [t.path, t]))
    return lyricHits.flatMap((h) => {
      const track = byPath.get(h.path)
      return track && !byName.has(h.path) ? [{ ...h, track }] : []
    })
  }, [lyricHits, results.tracks, lib.tracks])
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

  // Playlists live here too: their home among the tabs, a playlist (or an import) as its own page.
  const playlists = <PlaylistsScreen root={root} report={report} embedded onAlbum={(dir) => push({ kind: 'album', dir })} />
  const playlistOpen = tab === 'playlists' && playlistsAt !== 'home' && !typed.trim() && !query.trim()

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
      <SearchResults query={typed} albums={results.albums} artists={results.artists} tracks={results.tracks} lyrics={inLyrics} />
    ) : tab === 'home' ? (
      <HomeView
        root={root}
        lib={lib}
        hasCover={hasCover}
        onSeeAll={seeAll}
        onOpenPlaylist={props.onOpenPlaylist}
        onPlaylists={props.onPlaylists}
      />
    ) : tab === 'playlists' ? (
      playlists
    ) : tab === 'albums' ? (
      <AlbumsView lib={lib} filter={filter} onFilter={setFilter} />
    ) : tab === 'artists' ? (
      <ArtistsView artists={lib.artists} />
    ) : (
      <div className="pb-16">
        <SongList tracks={lib.tracks} from={{ kind: 'songs', ref: '', label: 'Songs' }} sortBy="title" pin />
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

  // An album or artist open: its own page, with Back.
  if (report && tracks && top)
    return (
      <NavContext value={nav}>
        <div ref={box}>{body}</div>
      </NavContext>
    )
  // A playlist, an import or To get: its own page, with Back to Playlists.
  if (report && playlistOpen)
    return (
      <NavContext value={nav}>
        <div ref={box}>{playlists}</div>
      </NavContext>
    )

  return (
    <NavContext value={nav}>
      <div ref={box}>
        <Page
          title="Music"
          line={line}
          bar={
            <>
              <Segmented
                tabs
                label="Music views"
                value={query.trim() ? ('' as Tab) : tab}
                options={TABS.map(([value, label]) => ({ value, label }))}
                onChange={(id) => {
                  setQuery('')
                  leavePages()
                  setFadeIn(true)
                  if (id === 'albums' && tab !== 'albums') setFilter('all')
                  setTab(id)
                }}
              />
          <div className="ml-auto flex min-w-[240px] max-w-[380px] flex-1 items-center gap-2 rounded-full bg-ink/[0.07] px-4 shadow-[inset_0_0_0_1px_var(--edge)] focus-within:bg-ink/[0.1] focus-within:shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--accent)_60%,transparent)]">
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
              className="min-w-0 flex-1 bg-transparent py-2 text-[13.5px] text-ink outline-none placeholder:text-ink/45 [&::-webkit-search-cancel-button]:hidden"
            />
            <kbd>/</kbd>
          </div>
            </>
          }
        >
          {body}
        </Page>
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
