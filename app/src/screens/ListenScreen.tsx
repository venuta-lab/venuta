import { useEscape } from '../lib/layers'
import { motion } from 'motion/react'
import { useCallback, useDeferredValue, useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from 'react'
import { isSearchCancelled, onlineSuggest, type HealthReport, type MoodTile, type OnlineAlbum, type OnlineCollection, type OnlineMore, type OnlineSuggestion } from '../lib/engine'
import { loadCovers, useCovers } from '../lib/covers'
import { RemoteArt } from '../components/RemoteArt'
import { findOnlineArtist, fromOnline } from './listen/onlineSearch'
import { notify } from '../lib/notify'
import { useLikes } from '../lib/likes'
import { onlineProblem } from '../lib/online'
import { useCatalog } from '../lib/catalog'
import { current, isOnline, play, playItems, queueKey, readFrom, songItemOf, toRemoteItem, usePlayer, usePlayerValue } from '../lib/player'
import { askAddSongs } from '../components/AddToPlaylist'
import type { Target } from '../lib/drag'
import { recentSearches } from '../lib/recentSearches'
import { SearchBox, type OnlineSuggestions, type Suggestion } from '../components/search/SearchBox'
import { createPortal } from 'react-dom'
import { TITLE_SEARCH } from '../components/TitleBar'
import { useLocalOnly } from '../lib/localOnly'
import { Page, Segmented } from '../components/ui'
import { Reading } from './MusicScreen'
import { AlbumPage } from './listen/AlbumPage'
import { OnlineAlbumPage } from './listen/OnlineAlbumPage'
import { OnlineArtistPage } from './listen/OnlineArtistPage'
import { OnlineCollectionPage } from './listen/OnlineCollectionPage'
import { MoodPage } from './listen/MoodPage'
import { OnlineShelfPage } from './listen/OnlineShelfPage'
import { SearchHome } from './listen/SearchHome'
import { YourMusic } from './listen/YourMusic'
import { ListeningView } from './listen/ListeningView'
import { SongList } from './listen/SongList'
import { AlbumsView, ArtistPage, ArtistsView, HomeView, SearchResults } from './listen/views'
import { artistKey, indexTracks, fmtNum, NavContext, NowContext, scrollerOf, search, spring, useNav, type Nav, type OnlineArtistRef } from './listen/model'
import { PlaylistsScreen, showPlaylists, usePlaylistsKind } from './PlaylistsScreen'
import { useLyricHits } from '../lib/lyricsSearch'

export type Tab = 'home' | 'search' | 'albums' | 'artists' | 'songs' | 'playlists' | 'listening'
/** The library's own views, as chips under Library. */
type LibraryView = 'albums' | 'artists' | 'songs' | 'playlists'
const LIBRARY: [LibraryView, string][] = [
  ['albums', 'Albums'],
  ['artists', 'Artists'],
  ['songs', 'Songs'],
  ['playlists', 'Playlists'],
]
/** The tabs along the top: Home, Search, Library (which opens its last view) and Listening
 *  (your stats and history). */
type Top = 'home' | 'search' | 'library' | 'listening'
const TOPS: [Top, string][] = [
  ['home', 'Home'],
  ['search', 'Search'],
  ['library', 'Library'],
  ['listening', 'Listening'],
]
const isLibrary = (t: Tab): t is LibraryView => t !== 'home' && t !== 'search' && t !== 'listening'
const topOf = (t: Tab): Top => (isLibrary(t) ? 'library' : t)

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

/* A page of Music, or a search, asked for from outside Music (Now Playing, the palette): shown
   when the screen shows, or at once if it is showing. */
type Asked = { page: Page } | { words: string }
let outside: Asked | null = null
const askedListeners = new Set<() => void>()
const ask = (a: Asked) => {
  outside = a
  askedListeners.forEach((l) => l())
}
/** Open a page of Music: an online artist, album, playlist or podcast, a mood. */
export const showMusicPage = (page: MusicPage) => ask({ page })
/** Search Music for these words (the Search tab, with their results). */
export const searchMusic = (words: string) => ask({ words })
export function showOnlineArtist(artist: OnlineArtistRef) {
  showMusicPage({ kind: 'online-artist', artist })
}
const onAsked = (l: () => void) => {
  askedListeners.add(l)
  return () => {
    askedListeners.delete(l)
  }
}

type Page =
  | { kind: 'album'; dir: string; origin?: string }
  | { kind: 'artist'; name: string }
  | { kind: 'online-album'; album: OnlineAlbum }
  | { kind: 'online-artist'; artist: OnlineArtistRef }
  | { kind: 'online-collection'; collection: OnlineCollection }
  | { kind: 'mood'; tile: MoodTile }
  | { kind: 'online-shelf'; title: string; owner: string; more: OnlineMore }
/** A page of Music that can be asked for from outside (see `showMusicPage`). */
export type MusicPage = Page
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
  /** An artist to show (from Now Playing); cleared with `onOpenedArtist` once shown. */
  openArtist?: string | null
  onOpenedArtist?: () => void
  onEditDetails: (dir: string) => void
  onOpenPlaylist: (name: string) => void
  onPlaylists: () => void
  /** No music of your own yet: add it (Home offers it). */
  onAddMusic?: () => void
  /** Bumped when Music is chosen again while already open: back to its start. */
  reset?: number
}

// The view you were on, kept while you visit other places (the screen itself is remounted).
const kept: { tab: Tab; filter: string; library: LibraryView; asked: string } = { tab: 'home', filter: 'all', library: 'albums', asked: '' }

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
  const online = song && isOnline(song) ? song : undefined
  const onlineKey = online ? queueKey(online) : ''
  // Keyed by the online song, not the row object, so a status that brings the queue again changes nothing.
  const value = useMemo(() => ({ path, dir, playing, online }), [path, dir, playing, onlineKey])
  return <NowContext value={value}>{children}</NowContext>
}

function Listen(props: Props) {
  const { root, report, busy, searchRef, openAlbum, onOpenedAlbum, openArtist = null, onOpenedArtist } = props
  const box = useRef<HTMLDivElement>(null)
  const [tab, setTab] = useState<Tab>(kept.tab)
  const [stack, setStack] = useState<Entry[]>([])
  // What is typed (the dropdown follows it), and what was searched for (Enter, or a pick from
  // the dropdown): the results are for the words searched, as on the Search page.
  const [query, setQuery] = useState(kept.asked)
  const localOnly = useLocalOnly()
  // The search lives in the title bar while Music shows (one search, top center).
  const [titleSlot, setTitleSlot] = useState<HTMLElement | null>(null)
  useLayoutEffect(() => setTitleSlot(document.getElementById(TITLE_SEARCH)), [])
  const [asked, setAsked] = useState(kept.asked)
  const searchForRef = useRef<(words: string) => void>(() => {})
  // The words now, for remembering a search when something found by it is played or opened.
  const words = useRef('')
  words.current = asked.trim()
  const [filter, setFilter] = useState(kept.filter)
  useEffect(() => {
    kept.tab = tab
    kept.filter = filter
    kept.asked = asked
    if (isLibrary(tab)) kept.library = tab
  }, [tab, filter, asked])
  const [fadeIn, setFadeIn] = useState(true)
  const [shown, setShown] = useState<string | null>(null)
  const typed = useDeferredValue(asked)
  const playlistsAt = usePlaylistsKind()

  // A new report means the folder was read again: the catalog is read again with it (shared
  // with Playlists and the add-to-playlist picker).
  const tracks = useCatalog(root, report)

  const lib = useMemo(() => indexTracks(tracks ?? []), [tracks])
  // What is liked, read again whenever the library is.
  useLikes(root, tracks)
  // The library as of now, for opening an artist by name from a menu.
  const libRef = useRef(lib)
  useEffect(() => {
    libRef.current = lib
  })
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
    // Opening something found by a search means the search: it's remembered.
    if (words.current) recentSearches.push(words.current)
    const scroll = scrollerOf(box.current)?.scrollTop ?? 0
    setStack((s) => {
      const last = s.at(-1)?.page
      const same =
        last &&
        last.kind === page.kind &&
        (page.kind === 'album'
          ? last.kind === 'album' && last.dir === page.dir
          : page.kind === 'online-album'
            ? last.kind === 'online-album' && last.album.album_id === page.album.album_id
            : page.kind === 'online-artist'
              ? last.kind === 'online-artist' && last.artist.artist_id === page.artist.artist_id
              : page.kind === 'online-collection'
                ? last.kind === 'online-collection' && last.collection.collection_id === page.collection.collection_id
                : page.kind === 'mood'
                  ? last.kind === 'mood' && last.tile.params === page.tile.params
                  : page.kind === 'online-shelf'
                    ? last.kind === 'online-shelf' && last.more.browse_id === page.more.browse_id && last.more.params === page.more.params
                    : last.kind === 'artist' && last.name.toLowerCase() === page.name.toLowerCase())
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
      openArtistNamed: (name) => {
        if (libRef.current?.byArtist.has(artistKey(name))) return push({ kind: 'artist', name })
        void findOnlineArtist(name, 'more-by')
          .then(({ artist }) => (artist ? push({ kind: 'online-artist', artist }) : notify(`No artist page for ${name} in the online catalog.`)))
          .catch((e) => !isSearchCancelled(e) && notify(onlineProblem(e)))
      },
      openOnlineAlbum: (album) => push({ kind: 'online-album', album }),
      openOnlineArtist: (artist) => push({ kind: 'online-artist', artist }),
      openOnlineCollection: (collection) => push({ kind: 'online-collection', collection }),
      openMood: (tile) => push({ kind: 'mood', tile }),
      openOnlineShelf: (title, owner, more) => push({ kind: 'online-shelf', title, owner, more }),
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
      setAsked('')
      setStack([{ page: { kind: 'album', dir: openAlbum }, scroll: 0 }])
    }
  }
  useEffect(() => {
    if (openAlbum && shown === openAlbum) onOpenedAlbum()
  }, [openAlbum, shown, onOpenedAlbum])

  // An artist asked for from elsewhere (Now Playing), the same way.
  const [shownArtist, setShownArtist] = useState<string | null>(null)
  // An artist you have no music by is looked up online (after this render).
  const [lookUp, setLookUp] = useState<string | null>(null)
  if (openArtist !== shownArtist) {
    if (!openArtist) setShownArtist(null)
    else if (lib.byDir.size) {
      setShownArtist(openArtist)
      setQuery('')
      setAsked('')
      if (lib.byArtist.has(artistKey(openArtist))) setStack([{ page: { kind: 'artist', name: openArtist }, scroll: 0 }])
      else {
        setStack([])
        setLookUp(openArtist)
      }
    }
  }
  useEffect(() => {
    if (!lookUp) return
    setLookUp(null)
    nav.openArtistNamed(lookUp)
  }, [lookUp, nav])
  useEffect(() => {
    if (openArtist && shownArtist === openArtist) onOpenedArtist?.()
  }, [openArtist, shownArtist, onOpenedArtist])

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

  // Asked from outside for a page (Now Playing's "More online by…", the palette) or a search.
  const wanted = useSyncExternalStore(onAsked, () => outside)
  useEffect(() => {
    if (!wanted) return
    outside = null
    if ('page' in wanted) push(wanted.page)
    else {
      // After this render's effects, so the search runs with this screen's state.
      const words = wanted.words
      window.setTimeout(() => searchForRef.current(words), 0)
    }
  }, [wanted, push])

  // Asked from outside for a view (Playlists, with a playlist open).
  const tabAsked = useSyncExternalStore(onTabAsked, () => askedTab)
  useEffect(() => {
    if (!tabAsked) return
    askedTab = null
    restore.current = 0
    setStack([])
    setQuery('')
    setAsked('')
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
    setAsked('')
    if (tab === 'playlists') showPlaylists({ kind: 'home' })
    scrollerOf(box.current)?.scrollTo({ top: 0, behavior: 'smooth' })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.reset])

  const choose = (id: Tab) => {
    // Search keeps its words; the other tabs are left without them.
    if (id !== 'search') {
      setQuery('')
      setAsked('')
    }
    leavePages()
    setFadeIn(true)
    if (id === 'albums' && tab !== 'albums') setFilter('all')
    setTab(id)
  }
  useEffect(() => {
    searchForRef.current = searchFor
  })
  const chooseTop = (t: Top) => choose(t === 'library' ? (isLibrary(tab) ? tab : kept.library) : t)
  // Your music's tiles (Home, Search): a Library view, Albums with a filter.
  const openLibrary = (view: 'albums' | 'artists' | 'songs', f?: string) => {
    choose(view)
    if (view === 'albums') setFilter(f ?? 'all')
  }
  /** Search for these words: the Search tab, with their results. */
  const searchFor = (words: string) => {
    const w = words.trim()
    setQuery(words)
    setAsked(w)
    if (w) recentSearches.push(w)
    leavePages()
    setFadeIn(true)
    setTab('search')
  }
  // Music dropped on the Playlists tab: pick the playlist. Held over it: the playlists show, to
  // drop onto one of them.
  const toPlaylists: Target = {
    label: 'Add to a playlist…',
    drop: async (c) => askAddSongs((await c.songs()).map(songItemOf), c.label),
    dwell: () => {
      if (tab !== 'playlists') choose('playlists')
      showPlaylists({ kind: 'home' })
    },
  }

  const results = useMemo(() => search(lib, typed), [lib, typed])
  // Playing from the results of these words (a song, an online row, a lyric hit) means them.
  const playingFrom = usePlayerValue((s) => s.from)
  useEffect(() => {
    const f = readFrom(playingFrom)
    if (f?.kind === 'search' && words.current && f.ref.trim().toLowerCase() === words.current.toLowerCase()) recentSearches.push(words.current)
  }, [playingFrom])
  // What the dropdown shows from the library: an artist, an album, a couple of songs, each with
  // its picture; picking one opens it (or plays the song), as the catalog's suggestions do.
  const suggest = useCallback(
    (text: string): Suggestion[] => {
      const r = search(lib, text)
      const from = { kind: 'search' as const, ref: text, label: `Search: ${text}` }
      return [
        ...r.artists.slice(0, 1).map((a) => ({
          key: `artist:${a.key}`,
          kind: 'Artist' as const,
          label: a.name,
          detail: `${a.albums.length} ${a.albums.length === 1 ? 'album' : 'albums'}`,
          value: a.name,
          art: a.albums[0] ? <LocalCover dir={a.albums[0].dir} /> : undefined,
          round: true,
          where: 'library' as const,
          onPick: () => nav.openArtist(a.name),
        })),
        ...r.albums.slice(0, 1).map((a) => ({
          key: `album:${a.dir}`,
          kind: 'Album' as const,
          label: a.title,
          detail: [a.artist, a.year].filter(Boolean).join(' · '),
          value: a.title,
          art: <LocalCover dir={a.dir} />,
          where: 'library' as const,
          onPick: () => nav.openAlbum(a.dir),
        })),
        ...r.tracks.slice(0, 2).map((t, i) => ({
          key: `song:${t.path}`,
          kind: 'Song' as const,
          label: t.title,
          detail: t.artists.join(', '),
          value: t.title,
          art: <LocalCover dir={t.dir} />,
          where: 'library' as const,
          onPick: () => void play(root, r.tracks.slice(0, 2), i, from),
        })),
      ]
    },
    [lib, nav, root],
  )
  // The online catalog's suggestions, as dropdown rows that play or open what they are.
  const suggestOnline = useCallback(
    (text: string): Promise<OnlineSuggestions> =>
      onlineSuggest(text).then((found) => ({
        queries: found.queries,
        items: found.items.flatMap((item): Suggestion[] => suggestionRow(item, text, { root, nav })),
      })),
    [root, nav],
  )
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
  const playlistOpen = tab === 'playlists' && playlistsAt !== 'home'

  const body = (() => {
    if (!report) return <Reading busy={busy} />
    if (!tracks) return <Loading />
    if (top?.page.kind === 'online-album') return <OnlineAlbumPage key={top.page.album.album_id} album={top.page.album} lib={lib} onBack={back} />
    if (top?.page.kind === 'online-artist')
      return <OnlineArtistPage key={top.page.artist.artist_id} artist={top.page.artist} lib={lib} onBack={back} />
    if (top?.page.kind === 'online-collection')
      return <OnlineCollectionPage key={top.page.collection.collection_id} collection={top.page.collection} onBack={back} />
    if (top?.page.kind === 'mood') return <MoodPage key={top.page.tile.params} tile={top.page.tile} lib={lib} onBack={back} />
    if (top?.page.kind === 'online-shelf')
      return <OnlineShelfPage key={`${top.page.more.browse_id}:${top.page.more.params ?? ''}`} title={top.page.title} owner={top.page.owner} more={top.page.more} onBack={back} />
    if (!lib.tracks.length && tab !== 'search')
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
    const view = tab === 'search' ? (
      typed.trim() ? (
        <SearchResults query={typed} albums={results.albums} artists={results.artists} tracks={results.tracks} lyrics={inLyrics} />
      ) : (
        <SearchHome onSearch={searchFor} yours={<YourMusic root={root} lib={lib} onOpen={openLibrary} className="mt-10" />} />
      )
    ) : tab === 'home' ? (
      <HomeView
        root={root}
        lib={lib}
        hasCover={hasCover}
        onSeeAll={seeAll}
        onLibrary={openLibrary}
        onOpenPlaylist={props.onOpenPlaylist}
        onPlaylists={props.onPlaylists}
        onAddMusic={props.onAddMusic}
      />
    ) : tab === 'listening' ? (
      <ListeningView lib={lib} root={root} />
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
        key={tab === 'search' ? `search:${typed.trim() ? 'results' : 'home'}` : tab}
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
      {titleSlot &&
        createPortal(
          <SearchBox
            id="listen-search"
            inputRef={searchRef}
            value={query}
            onChange={(v) => {
              setQuery(v)
              // Cleared: back to the Search page before anything was searched.
              if (!v.trim()) setAsked('')
              if (stack.length) leavePages()
              if (tab !== 'search' && v.trim()) {
                setFadeIn(true)
                setTab('search')
              }
            }}
            onSubmit={searchFor}
            onEscape={() => {
              setQuery('')
              setAsked('')
            }}
            suggest={lib.tracks.length ? suggest : undefined}
            onlineSuggest={localOnly ? undefined : suggestOnline}
            placeholder="Search songs, albums, artists, playlists"
            label="Search your music"
            hint={<kbd>/</kbd>}
            compact
          />,
          titleSlot,
        )}
      <div ref={box}>
        <Page
          title="Music"
          line={line}
          under={
            isLibrary(tab) ? (
              <Segmented
                tabs
                size="sm"
                label="Library views"
                value={tab}
                options={LIBRARY.map(([value, label]) => ({ value, label, drop: value === 'playlists' ? toPlaylists : null }))}
                onChange={choose}
              />
            ) : undefined
          }
          bar={
            <>
              <Segmented
                tabs
                label="Music views"
                value={topOf(tab)}
                options={TOPS.map(([value, label]) => ({ value, label }))}
                onChange={chooseTop}
              />
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

/** A library album's cover, small, for a dropdown row (asked for when shown). */
function LocalCover({ dir }: { dir: string }) {
  const { root } = useNav()
  const cover = useCovers(96)
  useEffect(() => {
    void loadCovers(root, [dir], 96)
  }, [root, dir])
  const art = cover(dir)
  return art ? <img src={art.src} alt="" draggable={false} className="h-full w-full object-cover" /> : null
}

/** One catalog suggestion as a dropdown row: a song plays, the rest open their page. */
function suggestionRow(item: OnlineSuggestion, text: string, ctx: { root: string; nav: Nav }): Suggestion[] {
  switch (item.kind) {
    case 'song': {
      const t = item.track
      return [
        {
          key: `online-song:${t.track_id}`,
          kind: 'Song',
          label: t.title,
          detail: t.artists.join(', '),
          value: t.title,
          art: <RemoteArt url={t.artwork_url} seed={t.album || t.title} size={40} imgClassName="h-full w-full object-cover" />,
          where: 'online',
          onPick: () => void playItems(ctx.root, [toRemoteItem(t)], 0, fromOnline(text)),
        },
      ]
    }
    case 'album':
      return [
        {
          key: `online-album:${item.album.album_id}`,
          kind: 'Album',
          label: item.album.title,
          detail: stripKind(item.subtitle, 'Album') ?? item.album.artists.join(', '),
          value: item.album.title,
          art: <RemoteArt url={item.album.artwork_url} seed={item.album.title} size={40} imgClassName="h-full w-full object-cover" />,
          where: 'online',
          onPick: () => ctx.nav.openOnlineAlbum(item.album),
        },
      ]
    case 'artist':
      return [
        {
          key: `online-artist:${item.artist.artist_id}`,
          kind: 'Artist',
          label: item.artist.name,
          detail: stripKind(item.subtitle, 'Artist') ?? undefined,
          value: item.artist.name,
          art: <RemoteArt url={item.artist.artwork_url} fallback={item.artist.name.slice(0, 1)} size={40} imgClassName="h-full w-full object-cover" />,
          round: true,
          where: 'online',
          onPick: () => ctx.nav.openOnlineArtist(item.artist),
        },
      ]
    case 'playlist':
      return [
        {
          key: `online-list:${item.collection.collection_id}`,
          kind: 'Playlist',
          label: item.collection.title,
          detail: stripKind(item.collection.subtitle, 'Playlist') ?? undefined,
          value: item.collection.title,
          art: <RemoteArt url={item.collection.artwork_url} seed={item.collection.title} size={40} imgClassName="h-full w-full object-cover" />,
          where: 'online',
          onPick: () => ctx.nav.openOnlineCollection(item.collection),
        },
      ]
  }
}

/** A catalog line without its leading kind ("Album • Arijit Singh • 2023" → "Arijit Singh · 2023"). */
function stripKind(line: string | null | undefined, kind: string): string | null {
  if (!line) return null
  const parts = line.split('•').map((p) => p.trim()).filter(Boolean)
  if (parts[0]?.toLowerCase() === kind.toLowerCase()) parts.shift()
  return parts.length ? parts.join(' · ') : null
}
