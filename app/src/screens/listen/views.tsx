import { AnimatePresence, motion } from 'motion/react'
import { memo, useCallback, useEffect, useMemo, useState } from 'react'
import { listeningSummary, playlistShelf, type ListeningSummary, type PlaylistSummary } from '../../lib/engine'
import { PlaylistCard } from '../playlists/PlaylistsHome'
import { loadCovers, useCovers } from '../../lib/covers'
import {
  appearsOn,
  artistFor,
  albumFrom,
  coverId,
  featured,
  plural,
  spring,
  useNav,
  useNowPlaying,
  type Album,
  type Artist,
  type Library,
} from './model'
import { Detail, InBar, Segmented } from '../../components/ui'
import { AlbumGrid, Art, Avatar, Chip, HiResPill, PlayButtons, QualityBadge, Shelf } from './parts'
import { SongList } from './SongList'
import { albumWork, artistWork, menuFor } from '../../lib/actions'
import { menuFromEvent } from '../../lib/menu'
import { dragSource } from '../../lib/drag'
import { WindowedGrid } from '../../components/WindowedGrid'
import { enqueueItems, onlineEntryFrom, play, playItems, queueKey, toggle, toItem, toRemoteItem, type QueueItem } from '../../lib/player'
import { artUrl, RemoteArt } from '../../components/RemoteArt'
import { AvailabilityBadge } from '../../components/AvailabilityBadge'
import { IconPause, IconPlay } from '../../components/Icons'
import { PlaylistShelf } from '../playlists/PlaylistShelf'
import { OnlineResults, OnlineTrackHeader, OnlineTrackRow } from './OnlineResults'
import { fromOnline, splitOnlineSongs, useOnlineExtras, useOnlineSearch } from './onlineSearch'
import { TopResult, type TopSources } from './TopResult'
import { pickTop, type TopPick } from '../../lib/searchRank'

import { askAddSongs } from '../../components/AddToPlaylist'
import { ownedIndex, titleCore } from '../../lib/owned'
import { searchKey } from '../../lib/textsearch'
import { MixedSongs, OnlineAlbumTile, OnlineArtistCard, type MixedSong } from './SearchAll'
import { CollectionCard } from './OnlineCards'
import { OnlineArtistSections } from './OnlineArtistSections'
import { useArtistSongs, useOnlineArtistFor } from './artistSongs'
import type { OnlineTrack } from '../../lib/engine'
export const HI_RES = 'Hi-Res'

// --- Home ------------------------------------------------------------------------------------

/** What is playing (or an album for today), then shelves: hi-res first, then your categories. */
export const HomeView = memo(function HomeView(props: {
  root: string
  lib: Library
  hasCover: (dir: string) => boolean
  onSeeAll: (filter: string) => void
  onOpenPlaylist: (name: string) => void
  onPlaylists: () => void
}) {
  const { lib } = props
  const now = useNowPlaying()
  // What you've played, fetched again when the song playing changes.
  const [heard, setHeard] = useState<ListeningSummary | null>(null)
  useEffect(() => {
    let live = true
    void listeningSummary(props.root)
      .then((h) => live && setHeard(h))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [props.root, now.path])
  const recent = useMemo(
    () => (heard?.recent ?? []).map((d) => lib.byDir.get(d)).filter((a): a is Album => !!a).slice(0, 24),
    [heard, lib],
  )
  // Playlists you played, beside the albums in Jump back in.
  const [lists, setLists] = useState<PlaylistSummary[]>([])
  useEffect(() => {
    let live = true
    const load = () =>
      playlistShelf(props.root)
        .then((l) => live && setLists(l))
        .catch(() => {})
    void load()
    window.addEventListener('venuta:playlists-changed', load)
    return () => {
      live = false
      window.removeEventListener('venuta:playlists-changed', load)
    }
  }, [props.root])
  const playedLists = useMemo(
    () =>
      (heard?.recent_playlists ?? []).flatMap(([name, at]) => {
        const p = lists.find((x) => x.name === name)
        return p && p.tracks - p.missing > 0
          ? [{ key: `playlist:${name}`, at, node: <PlaylistCard root={props.root} p={p} onOpen={() => props.onOpenPlaylist(name)} /> }]
          : []
      }),
    [heard, lists, props],
  )
  const top = useMemo(
    () => (heard?.top ?? []).filter(([, n]) => n >= 2).map(([d]) => lib.byDir.get(d)).filter((a): a is Album => !!a),
    [heard, lib],
  )
  // Albums with songs you rated 4 or 5 stars, best first.
  const rated = useMemo(() => {
    const score = (a: Album) => {
      const r = a.tracks.map((t) => t.stars).filter((x) => x > 0)
      return r.length ? r.reduce((x, y) => x + y, 0) / r.length : 0
    }
    return lib.albums
      .filter((a) => a.tracks.some((t) => t.stars >= 4))
      .sort((a, b) => score(b) - score(a))
      .slice(0, 30)
  }, [lib])
  // Played before, but not in the last month: oldest first.
  const [openedAt] = useState(() => Date.now())
  const rediscover = useMemo(() => {
    const month = 30 * 24 * 3600 * 1000
    return Object.entries(heard?.last_played ?? {})
      .filter(([, at]) => openedAt - at > month)
      .sort((a, b) => a[1] - b[1])
      .map(([d]) => lib.byDir.get(d))
      .filter((a): a is Album => !!a)
      .slice(0, 24)
  }, [heard, lib, openedAt])
  const today = useMemo(() => featured(lib.albums, props.hasCover), [lib, props.hasCover])
  const playing = now.dir ? lib.byDir.get(now.dir) : undefined
  const hero = playing ?? today
  const shelves = useMemo(
    () => lib.categories.map(([c]) => [c, lib.albums.filter((a) => a.category === c)] as const),
    [lib],
  )
  return (
    <div className="pb-16">
      <AnimatePresence mode="wait" initial={false}>
        {!playing && now.online ? (
          <OnlineHero key={queueKey(now.online)} song={now.online} playing={now.playing} />
        ) : hero && (
          <Hero
            key={hero.dir}
            album={hero}
            label={playing ? (now.playing ? 'Playing now' : 'Paused') : 'Picked for today'}
          />
        )}
      </AnimatePresence>
      {recent.length + playedLists.length > 0 && (
        <Shelf title="Jump back in" albums={recent} origin="shelf-recent" more={playedLists} at={(d) => heard?.last_played[d] ?? 0} />
      )}
      <PlaylistShelf root={props.root} onOpen={props.onOpenPlaylist} onSeeAll={props.onPlaylists} />
      {top.length > 2 && <Shelf title="Most played" albums={top} origin="shelf-top" />}
      {rated.length > 0 && <Shelf title="Top rated" albums={rated} origin="shelf-rated" />}
      {rediscover.length > 0 && <Shelf title="Rediscover" albums={rediscover} origin="shelf-rediscover" />}
      <Shelf title="Hi-Res" albums={lib.hiRes} origin="shelf-hires" onSeeAll={() => props.onSeeAll(HI_RES)} />
      {shelves.map(([c, albums]) => (
        <Shelf key={c} title={c} albums={albums} origin={`shelf-${c}`} onSeeAll={() => props.onSeeAll(c)} />
      ))}
    </div>
  )
})

/**
 * Home's top card while an online song plays: the song as the card shows an album of yours, its
 * cover (or the online cover), Online, and Pause/Resume. It has no page of its own to open.
 */
function OnlineHero({ song, playing }: { song: QueueItem; playing: boolean }) {
  const art = artUrl(song.artwork_url, 200)
  const artist = song.artists.join(', ') || song.album_artist
  return (
    <motion.section
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -6, transition: { duration: 0.15 } }}
      transition={spring}
      className="glass relative overflow-hidden rounded-[28px] p-6"
    >
      {art && (
        <img
          aria-hidden
          src={art}
          alt=""
          className="pointer-events-none absolute inset-0 h-full w-full scale-150 object-cover opacity-45"
          style={{ filter: 'blur(64px) var(--art-tint)' }}
        />
      )}
      <div className="relative flex flex-wrap items-center gap-7">
        <div className="aspect-square w-[200px] shrink-0 overflow-hidden rounded-2xl bg-ink/[0.06] shadow-2xl shadow-black/40">
          <RemoteArt url={song.artwork_url} seed={song.album || song.title} size={200} imgClassName="h-full w-full object-cover" />
        </div>
        <div className="min-w-[240px] flex-1">
          <p className="text-[13px] text-ink/60">{playing ? 'Playing now' : 'Paused'}</p>
          <h2 className="title mt-1.5 text-[30px] leading-[1.1]">{song.title}</h2>
          {artist && <p className="mt-1.5 text-[15px] text-ink/80">{artist}</p>}
          <div className="mt-2.5 flex flex-wrap items-center gap-2 text-[13px] text-ink/60">
            {song.album && <span>{song.album}</span>}
            <AvailabilityBadge availability="online" />
          </div>
          <div className="mt-5">
            <button onClick={() => void toggle()} className="btn-primary play-main btn-lg">
              {playing ? <IconPause /> : <IconPlay />}
              {playing ? 'Pause' : 'Resume'}
            </button>
          </div>
        </div>
      </div>
    </motion.section>
  )
}

function Hero({ album: a, label }: { album: Album; label: string }) {
  const nav = useNav()
  const cover = useCovers(320)(a.dir)
  return (
    <motion.section
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -6, transition: { duration: 0.15 } }}
      transition={spring}
      onContextMenu={(e) => menuFromEvent(e, menuFor('album', {}, albumWork(nav, a)))}
      className="glass relative overflow-hidden rounded-[28px] p-6"
    >
      {cover && (
        <img
          aria-hidden
          src={cover.src}
          alt=""
          className="pointer-events-none absolute inset-0 h-full w-full scale-150 object-cover opacity-45"
          style={{ filter: "blur(64px) var(--art-tint)" }}
        />
      )}
      <div className="relative flex flex-wrap items-center gap-7">
        <button
          onClick={() => {
            nav.openAlbum(a.dir, 'hero')
          }}
          aria-label={`Open ${a.title}`}
          className="group shrink-0"
        >
          <motion.div
            layoutId={coverId('hero', a.dir)}
            transition={spring}
            className="aspect-square w-[200px] overflow-hidden rounded-2xl bg-ink/[0.06] shadow-2xl shadow-black/40"
          >
            <Art cover={cover} album={a} />
          </motion.div>
        </button>
        <div className="min-w-[240px] flex-1">
          <p className="text-[13px] text-ink/60">{label}</p>
          <h2 className="title mt-1.5 text-[30px] leading-[1.1]">
            <button onClick={() => nav.openAlbum(a.dir, 'hero')} className="rounded-md text-left hover:underline">
              {a.title}
            </button>
          </h2>
          <button onClick={() => nav.openArtist(a.artist)} className="mt-1.5 rounded-md text-left text-[15px] text-ink/80 hover:text-ink hover:underline">
            {a.artist}
          </button>
          <div className="mt-2.5 flex flex-wrap items-center gap-2 text-[13px] text-ink/60">
            {a.year && <span>{a.year}</span>}
            <QualityBadge text={a.quality} />
            {a.hiRes && <HiResPill />}
          </div>
          <div className="mt-5">
            <PlayButtons tracks={a.tracks} from={albumFrom(a)} dir={a.dir} />
          </div>
        </div>
      </div>
    </motion.section>
  )
}

// --- Albums ----------------------------------------------------------------------------------

export const AlbumsView = memo(function AlbumsView(props: { lib: Library; filter: string; onFilter: (f: string) => void }) {
  const { lib, filter } = props
  const shown = useMemo(
    () => (filter === 'all' ? lib.albums : filter === HI_RES ? lib.hiRes : lib.albums.filter((a) => a.category === filter)),
    [lib, filter],
  )
  const chip = (id: string, label: string, count: number) => (
    <Chip key={id} group="listen-album-filter" on={filter === id} label={label} count={count} onClick={() => props.onFilter(id)} />
  )
  return (
    <div className="pb-16">
      <InBar>
        <div className="no-scrollbar -mx-1 flex gap-1 overflow-x-auto px-1">
          {chip('all', 'All', lib.albums.length)}
          {lib.hiRes.length > 0 && chip(HI_RES, 'Hi-Res', lib.hiRes.length)}
          {lib.categories.map(([c, n]) => chip(c, c, n))}
        </div>
      </InBar>
      <AlbumGrid albums={shown} origin="albums" />
    </div>
  )
})

// --- Artists ---------------------------------------------------------------------------------

export const ArtistsView = memo(function ArtistsView({ artists }: { artists: Artist[] }) {
  return (
    <div className="pb-16">
      <WindowedGrid
        items={artists}
        keyOf={(ar) => ar.key}
        min={150}
        gapX={20}
        gapY={28}
        below={44}
        label="Artists"
        render={(ar) => <ArtistCard artist={ar} />}
      />
    </div>
  )
})

/** The first album of theirs that has a cover. */
function useArtistCover(ar: Artist) {
  const cover = useCovers(320)
  for (const a of ar.albums) {
    const c = cover(a.dir)
    if (c) return c
  }
  return undefined
}

export function ArtistCard({ artist: ar }: { artist: Artist }) {
  const nav = useNav()
  const c = useArtistCover(ar)
  return (
    <button
      onClick={() => nav.openArtist(ar.name)}
      onContextMenu={(e) => menuFromEvent(e, menuFor('artist', {}, artistWork(nav, ar)))}
      {...dragSource(() => {
        const tracks = ar.albums.flatMap((a) => a.tracks)
        return { label: ar.name, count: tracks.length, cover: c?.src, songs: async () => tracks.map(toItem) }
      })}
      className="lift group block w-full min-w-0 text-center"
    >
      <Avatar name={ar.name} cover={c} />
      <div className="mt-3 truncate text-[14px] font-medium">{ar.name}</div>
      <div className="text-[12.5px] text-ink/55">{plural(ar.albums.length, 'album')}</div>
    </button>
  )
}

/** One artist: their albums, newest first, and the songs they sing on other people's albums. */
export function ArtistPage({ lib, name, onBack }: { lib: Library; name: string; onBack: () => void }) {
  const ar = useMemo(() => artistFor(lib, name), [lib, name])
  const also = useMemo(() => appearsOn(lib, ar.key), [lib, ar.key])
  const own = useMemo(() => ar.albums.flatMap((a) => a.tracks), [ar])
  const cover = useArtistCover(ar)
  // Their page in the online catalog, when there is one: shown under what you have.
  const online = useOnlineArtistFor(ar.name)
  const onlineName = online?.name || ar.name
  const onlineSongs = useArtistSongs(online, onlineName, lib)
  const nav = useNav()
  const counts = [
    ar.albums.length ? plural(ar.albums.length, 'album') : '',
    own.length ? plural(own.length, 'song') : '',
    also.length ? `on ${plural(also.length, 'more song')}` : '',
  ].filter(Boolean)

  return (
    <Detail
      title={ar.name}
      onBack={onBack}
      glow={cover?.src}
      tools={<PlayButtons tracks={own.length ? own : also} from={{ kind: 'artist', ref: ar.name, label: ar.name }} label="Play all" size="sm" />}
    >
      <div className="flex flex-wrap items-end gap-7">
        <div className="w-[168px] shrink-0">
          <Avatar name={ar.name} cover={cover} />
        </div>
        <motion.div
          className="min-w-[240px] flex-1 pb-1"
          initial={{ opacity: 0, y: 12 }}
          animate={{ opacity: 1, y: 0 }}
          transition={spring}
        >
          <h1 className="title hero-title text-[40px] leading-none">{ar.name}</h1>
          <p className="mt-2.5 text-[14px] text-ink/60">{counts.join(', ') || 'Nothing by them in your library yet.'}</p>
          <div className="mt-5">
            <PlayButtons tracks={own.length ? own : also} from={{ kind: 'artist', ref: ar.name, label: ar.name }} label="Play all" />
          </div>
        </motion.div>
      </div>
      {ar.albums.length > 0 && (
        <section className="mt-10">
          <h2 className="mb-4 text-[19px] font-semibold tracking-[-0.02em]">Albums</h2>
          <AlbumGrid albums={ar.albums} origin="artist" subtitle="year" />
        </section>
      )}
      {also.length > 0 && (
        <section className="mt-10">
          <h2 className="mb-3 text-[19px] font-semibold tracking-[-0.02em]">Also appears on</h2>
          <SongList tracks={also} from={{ kind: 'artist', ref: ar.name, label: ar.name }} />
        </section>
      )}
      {online && (
        <section className="mt-14" aria-label={`${onlineName} online`}>
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
            <h2 className="title text-[22px]">More by {onlineName} online</h2>
            <AvailabilityBadge availability="online" />
            <button
              onClick={() => nav.openOnlineArtist({ artist_id: online.artist_id, name: onlineName, artwork_url: online.artwork_url })}
              className="ml-auto rounded-full px-3 py-1 text-[12.5px] text-ink/65 hover:bg-ink/[0.08] hover:text-ink"
            >
              Open their online page
            </button>
          </div>
          <OnlineArtistSections page={online} name={onlineName} songs={onlineSongs.songs} from={onlineSongs.from} />
        </section>
      )}
    </Detail>
  )
}

// --- Search ----------------------------------------------------------------------------------

/** A song found by its words, and the line that holds them. */
export interface LyricResult {
  track: Library['tracks'][number]
  line: string
  latin?: string
}

/** Everything that matches, together: albums, artists, songs, then songs by their words. */
/** Which part of the search results shows. */
type ResultChip = 'all' | 'songs' | 'albums' | 'artists' | 'playlists' | 'videos' | 'podcasts' | 'lyrics'

/** Songs beside the Top result. */
const SONGS_BESIDE_TOP = 5
const CARDS_ON_ALL = 16
const VIDEOS_ON_ALL = 5

/**
 * Search results, in the order a music app lays them out: the Top result with the songs beside
 * it, then albums, artists, playlists, videos and podcasts, then songs found by their lyrics.
 * Your library and the online catalog are one list each: what you have comes first, marked so,
 * and plays from your file; the catalog fills in the rest, without repeating what you have.
 * Chips narrow it to one kind (back to All when the words change).
 */
export function SearchResults(props: {
  query: string
  albums: Album[]
  artists: Artist[]
  tracks: Library['tracks']
  lyrics: LyricResult[]
}) {
  const q = props.query.trim()
  // The words were searched for (not typed): asked at once.
  const online = useOnlineSearch(props.query, 'music', true)
  const extras = useOnlineExtras(props.query)
  const [picked, setPicked] = useState<{ q: string; chip: ResultChip }>({ q: '', chip: 'all' })
  const chip: ResultChip = picked.q === q ? picked.chip : 'all'
  const setChip = (c: ResultChip) => setPicked({ q, chip: c })

  const onlineDone = online.offline || (online.resultsFor === q && !online.loading)
  const answer = online.resultsFor === q && !online.loading ? online.results : null
  const best = useMemo(() => splitOnlineSongs(answer?.tracks ?? [], q).best, [answer, q])

  // What of the catalog's you already have: it shows once, as yours.
  const owns = useMemo(() => ownedIndex(props.tracks), [props.tracks])
  const owned = useCallback((t: { title: string; artists: string[]; duration_ms?: number | null }) => !!owns(t), [owns])
  const ownAlbum = useMemo(() => {
    const mine = props.albums.map((a) => ({ key: titleCore(a.title), artist: searchKey(a.artist) }))
    return (a: { title: string; artists: string[] }) => {
      const key = titleCore(a.title)
      const theirs = a.artists.map(searchKey)
      return mine.some((m) => m.key === key && (!theirs.length || theirs.some((t) => t === m.artist)))
    }
  }, [props.albums])
  const ownArtist = useMemo(() => {
    const mine = new Set(props.artists.map((a) => searchKey(a.name)))
    return (name: string) => mine.has(searchKey(name))
  }, [props.artists])

  // The Top result: from the library now, once more when the online catalog has answered.
  const phase = `${q}|${onlineDone ? 'online' : 'library'}`
  const [held, setHeld] = useState<{ phase: string; pick: TopPick | null }>({ phase: '', pick: null })
  let top = held.pick
  if (held.phase !== phase) {
    top = pickTop(q, { tracks: props.tracks, albums: props.albums, artists: props.artists }, answer ? { songs: best, albums: answer.albums, artists: answer.artists } : null)
    setHeld({ phase, pick: top })
  }
  const sources: TopSources = {
    query: q,
    tracks: props.tracks,
    albums: props.albums,
    artists: props.artists,
    songs: best,
    onlineAlbums: answer?.albums ?? [],
    onlineArtists: answer?.artists ?? [],
  }
  // A pick whose row is gone (the library was read again): none rather than a wrong one.
  const len = { library: { song: props.tracks.length, album: props.albums.length, artist: props.artists.length }, online: { song: best.length, album: sources.onlineAlbums.length, artist: sources.onlineArtists.length } }
  if (top && top.index >= len[top.where][top.kind]) top = null

  // The mixed lists: yours, then the catalog's you don't have.
  const songs: MixedSong[] = useMemo(
    () => [
      ...props.tracks.map((track) => ({ where: 'library' as const, track })),
      ...best.filter((t) => !owned(t)).map((track) => ({ where: 'online' as const, track })),
    ],
    [props.tracks, best, owned],
  )
  const onlineAlbums = useMemo(() => (answer?.albums ?? []).filter((a) => !ownAlbum(a)), [answer, ownAlbum])
  const onlineArtists = useMemo(() => (answer?.artists ?? []).filter((a) => !ownArtist(a.name)), [answer, ownArtist])
  const fresh = extras.for === q
  const playlists = fresh ? extras.playlists : []
  const podcasts = fresh ? extras.podcasts : []
  const videos = fresh ? extras.videos : []

  const from = { kind: 'search' as const, ref: q, label: `Search: ${q}` }
  const none = (have: number, there: number, loading = !onlineDone) => !have && !loading && !there
  const chips: { value: ResultChip; label: string; disabled?: boolean }[] = [
    { value: 'all', label: 'All' },
    { value: 'songs', label: 'Songs', disabled: none(props.tracks.length, answer?.tracks.length ?? 0) },
    { value: 'albums', label: 'Albums', disabled: none(props.albums.length, answer?.albums.length ?? 0) },
    { value: 'artists', label: 'Artists', disabled: none(props.artists.length, answer?.artists.length ?? 0) },
    { value: 'playlists', label: 'Playlists', disabled: none(0, playlists.length, extras.loading) },
    { value: 'videos', label: 'Videos', disabled: none(0, videos.length, extras.loading) },
    { value: 'podcasts', label: 'Podcasts', disabled: none(0, podcasts.length, extras.loading) },
    ...(props.lyrics.length ? [{ value: 'lyrics' as const, label: 'Lyrics' }] : []),
  ]
  const eyebrow = (text: string, count?: number) => (
    <div className="mb-4 flex items-baseline gap-2 text-[12px] font-semibold uppercase tracking-[0.08em] text-ink/45">
      {text}
      {count != null && <span className="font-normal normal-case tracking-normal">{count.toLocaleString('en-IN')}</span>}
    </div>
  )
  const lyricSection = (spaced: boolean) =>
    props.lyrics.length > 0 && (
      <section className={spaced ? 'mt-12' : ''}>
        <div className="mb-3 flex items-baseline gap-3">
          <h2 className="title text-[19px]">In the lyrics</h2>
          <span className="text-[12.5px] text-ink/50">{props.lyrics.length.toLocaleString('en-IN')}</span>
        </div>
        <LyricResults hits={props.lyrics} query={q} />
      </section>
    )
  const cardGrid = (nodes: React.ReactNode[]) => <div className="grid grid-cols-[repeat(auto-fill,minmax(168px,1fr))] gap-x-5 gap-y-8">{nodes}</div>
  const videoRows = (list: OnlineTrack[]) => (
    <>
      <OnlineTrackHeader />
      <div className="mt-1 grid gap-0.5">
        {list.map((t, i) => (
          <OnlineTrackRow
            key={`${t.provider}:${t.track_id}`}
            track={t}
            onPlay={() => void playItems(nav.root, list.map(toRemoteItem), i, fromOnline(q))}
            onQueue={(next) => void enqueueItems(nav.root, [toRemoteItem(t)], next)}
            onAdd={() => askAddSongs([{ online: onlineEntryFrom(t) }], t.title)}
          />
        ))}
      </div>
    </>
  )
  const nav = useNav()
  const waiting = (what: string) => <p className="py-6 text-[13px] text-ink/45">Looking for {what}…</p>
  const nothing = (what: string) => <p className="py-6 text-[13px] text-ink/55">No {what} for “{q}”.</p>

  let body: React.ReactNode
  if (chip === 'songs')
    body = (
      <>
        {props.tracks.length > 0 && (
          <section>
            {eyebrow('In your library', props.tracks.length)}
            <SongList tracks={props.tracks} from={from} pin />
          </section>
        )}
        <OnlineResults query={props.query} online={online} spaced={props.tracks.length > 0} parts={{ songs: true, albums: false, artists: false }} covers="tabs" skip={owned} />
      </>
    )
  else if (chip === 'albums')
    body = (
      <>
        {props.albums.length > 0 && (
          <section>
            {eyebrow('In your library', props.albums.length)}
            <AlbumGrid albums={props.albums} origin="search" />
          </section>
        )}
        {onlineAlbums.length > 0 && (
          <section className={props.albums.length ? 'mt-12' : ''}>
            {eyebrow('Online catalog', onlineAlbums.length)}
            {cardGrid(onlineAlbums.map((a) => <OnlineAlbumTile key={a.album_id} album={a} query={q} />))}
          </section>
        )}
        {!onlineDone && waiting('albums online')}
      </>
    )
  else if (chip === 'artists')
    body = (
      <>
        {props.artists.length > 0 && (
          <section>
            {eyebrow('In your library', props.artists.length)}
            <div className="grid grid-cols-[repeat(auto-fill,minmax(128px,1fr))] gap-x-5 gap-y-7">
              {props.artists.map((ar) => (
                <ArtistCard key={ar.key} artist={ar} />
              ))}
            </div>
          </section>
        )}
        {onlineArtists.length > 0 && (
          <section className={props.artists.length ? 'mt-12' : ''}>
            {eyebrow('Online catalog', onlineArtists.length)}
            <div className="grid grid-cols-[repeat(auto-fill,minmax(128px,1fr))] gap-x-5 gap-y-7">
              {onlineArtists.map((ar) => (
                <OnlineArtistCard key={ar.artist_id} artist={ar} />
              ))}
            </div>
          </section>
        )}
        {!onlineDone && waiting('artists online')}
      </>
    )
  else if (chip === 'playlists')
    body = playlists.length ? cardGrid(playlists.map((c) => <CollectionCard key={c.collection_id} collection={c} width={168} />)) : extras.loading ? waiting('playlists') : nothing('playlists')
  else if (chip === 'podcasts')
    body = podcasts.length ? cardGrid(podcasts.map((c) => <CollectionCard key={c.collection_id} collection={c} width={168} />)) : extras.loading ? waiting('podcasts') : nothing('podcasts')
  else if (chip === 'videos') body = videos.length ? videoRows(videos) : extras.loading ? waiting('videos') : nothing('videos')
  else if (chip === 'lyrics') body = lyricSection(false)
  else {
    const besideTop = songs.slice(0, SONGS_BESIDE_TOP)
    const albumCards = [
      ...props.albums.slice(0, CARDS_ON_ALL).map((a) => ({ key: a.dir, at: 2, node: null as React.ReactNode, album: a })),
      ...onlineAlbums.slice(0, CARDS_ON_ALL).map((a) => ({ key: `online:${a.album_id}`, at: 1, node: <OnlineAlbumTile album={a} query={q} />, album: undefined })),
    ]
    const artistCards = [
      ...props.artists.slice(0, CARDS_ON_ALL).map((ar) => ({ key: ar.key, at: 2, node: <ArtistCard artist={ar} /> })),
      ...onlineArtists.slice(0, CARDS_ON_ALL).map((ar) => ({ key: `online:${ar.artist_id}`, at: 1, node: <OnlineArtistCard artist={ar} /> })),
    ]
    const nothingAtAll =
      onlineDone && !extras.loading && !top && !songs.length && !albumCards.length && !artistCards.length && !playlists.length && !videos.length && !podcasts.length && !props.lyrics.length
    body = nothingAtAll ? (
      <p className="py-20 text-center text-[14px] text-ink/55">Nothing matches “{q}” in your library or the online catalog.</p>
    ) : (
      <>
        {(top || besideTop.length > 0) && (
          <div className="grid items-start gap-x-10 gap-y-8 xl:grid-cols-[minmax(0,5fr)_minmax(0,7fr)]">
            {top ? (
              <TopResult pick={top} from={sources} />
            ) : (
              <div />
            )}
            {besideTop.length > 0 && <MixedSongs query={q} songs={besideTop} onSeeAll={songs.length > SONGS_BESIDE_TOP ? () => setChip('songs') : undefined} />}
          </div>
        )}
        {!onlineDone && !top && !besideTop.length && waiting('music')}
        <Shelf
          title="Albums"
          albums={props.albums.slice(0, CARDS_ON_ALL)}
          origin="search"
          at={() => 2}
          more={albumCards.filter((c) => !c.album).map(({ key, at, node }) => ({ key, at, node }))}
          onSeeAll={() => setChip('albums')}
        />
        <Shelf title="Artists" albums={[]} origin="search" more={artistCards} onSeeAll={() => setChip('artists')} />
        <Shelf title="Playlists" albums={[]} origin="search" more={playlists.slice(0, CARDS_ON_ALL).map((c) => ({ key: c.collection_id, at: 0, node: <CollectionCard collection={c} width={168} /> }))} onSeeAll={() => setChip('playlists')} />
        {videos.length > 0 && (
          <section className="mt-10">
            <div className="mb-3 flex items-baseline gap-3">
              <h2 className="title text-[19px]">Videos</h2>
              <button onClick={() => setChip('videos')} className="ml-auto rounded-full px-3 py-1 text-[12.5px] text-ink/65 hover:bg-ink/[0.08] hover:text-ink">
                See all
              </button>
            </div>
            {videoRows(videos.slice(0, VIDEOS_ON_ALL))}
          </section>
        )}
        <Shelf title="Podcasts" albums={[]} origin="search" more={podcasts.slice(0, CARDS_ON_ALL).map((c) => ({ key: c.collection_id, at: 0, node: <CollectionCard collection={c} width={168} /> }))} onSeeAll={() => setChip('podcasts')} />
        {lyricSection(true)}
      </>
    )
  }

  return (
    <div className="pb-16">
      <div className="mb-8">
        <Segmented tabs size="sm" label="Search results" value={chip} options={chips} onChange={setChip} />
      </div>
      {body}
    </div>
  )
}

/** Songs found by a line of their lyrics: the song, and the line (in Roman letters too). */
function LyricResults(props: { hits: LyricResult[]; query: string }) {
  const { root } = useNav()
  const cover = useCovers(320)
  const tracks = props.hits.map((h) => h.track)
  // The albums' covers, asked for once per set of albums.
  const dirs = JSON.stringify([...new Set(tracks.map((t) => t.dir))])
  useEffect(() => {
    void loadCovers(root, JSON.parse(dirs) as string[])
  }, [root, dirs])
  return (
    <div className="grid gap-1">
      {props.hits.map((h, i) => {
        const art = cover(h.track.dir)
        return (
          <button
            key={h.track.path}
            onClick={() => void play(root, tracks, i, { kind: 'search', ref: props.query, label: `Lyrics: ${props.query}` })}
            className="row group grid grid-cols-[40px_minmax(0,1fr)] items-center gap-3 rounded-xl px-2 py-2 text-left hover:bg-ink/[0.05] focus-visible:bg-ink/[0.06] active:bg-ink/[0.09]"
          >
            <span className="h-10 w-10 overflow-hidden rounded-md bg-ink/[0.06]">
              {art && <img src={art.src} alt="" draggable={false} loading="lazy" className="h-full w-full object-cover" />}
            </span>
            <span className="min-w-0">
              <span className="block truncate text-[13.5px] font-medium">
                {h.track.title}
                <span className="font-normal text-ink/55"> · {h.track.artists.join(', ')}</span>
              </span>
              <span className="block truncate text-[13px] text-ink/70">“{h.line}”</span>
              {h.latin && <span className="block truncate text-[12.5px] text-ink/50">{h.latin}</span>}
            </span>
          </button>
        )
      })}
    </div>
  )
}
