import { AnimatePresence, motion, Reorder, useDragControls } from 'motion/react'
import { Fragment, memo, useCallback, useEffect, useMemo, useState } from 'react'
import { listeningSummary, playlistShelf, type ListeningSummary, type PlaylistSummary } from '../../lib/engine'
import { PlaylistCard } from '../playlists/PlaylistsHome'
import { loadCovers, useCovers } from '../../lib/covers'
import {
  appearsOn,
  artistFor,
  albumFrom,
  CardWidth,
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
import { Detail, InBar, Segmented, Switch } from '../../components/ui'
import { arrange, CARD_WIDTHS, move, readLayout, reorder, saveLayout, toggle as toggleHome, type CardSize, type HomeLayout } from '../../lib/homeLayout'
import { useShortcuts } from '../../lib/shortcuts'
import { FamiliarArtists, HomeChips, OnlineShelves, QuickPicks, useOnlineHome, useRecentOnline, useRecentPlays } from './HomeOnline'
import { AlbumGrid, Art, Avatar, Chip, HiResPill, PlayButtons, QualityBadge, Shelf } from './parts'
import { SongList } from './SongList'
import { albumWork, artistWork, menuFor } from '../../lib/actions'
import { menuFromEvent } from '../../lib/menu'
import { dragSource } from '../../lib/drag'
import { albumHasGenre, catalogGenreFor, GENRE_FILTER } from '../../lib/yourMusic'
import { useCatalogTiles } from '../../lib/catalogTiles'
import { useLocalOnly } from '../../lib/localOnly'
import { YourMusic } from './YourMusic'
import { WindowedGrid } from '../../components/WindowedGrid'
import { useYtmLibraryShown } from '../../lib/ytmChoices'
import { useYtmCount } from '../../lib/ytmLibrary'
import { YtmGrid, YtmSongs } from './YtmLibrary'
import { availabilityOf, enqueueItems, onlineEntryFrom, play, queueKey, toggle, toItem, toRemoteItem, type QueueItem } from '../../lib/player'
import { artUrl, RemoteArt } from '../../components/RemoteArt'
import { AvailabilityBadge } from '../../components/AvailabilityBadge'
import { IconPause, IconPlay } from '../../components/Icons'
import { PlaylistShelf } from '../playlists/PlaylistShelf'
import { OnlineResults, OnlineTrackHeader, OnlineTrackRow } from './OnlineResults'
import { splitOnlineSongs, useOnlineExtras, useOnlineSearch } from './onlineSearch'
import { TopResult, type TopSources } from './TopResult'
import { pickTop, type TopPick } from '../../lib/searchRank'

import { askAddSongs } from '../../components/AddToPlaylist'
import { ownedIndex, titleCore } from '../../lib/owned'
import { searchKey } from '../../lib/textsearch'
import { MixedSongs, OnlineAlbumTile, OnlineArtistCard, type MixedSong } from './SearchAll'
import { CollectionCard } from './OnlineCards'
import { playWithRadio } from '../../lib/radio'
import { OnlineArtistSections } from './OnlineArtistSections'
import { useArtistSongs, useOnlineArtistFor } from './artistSongs'
import type { CatalogTrack, OnlineTrack } from '../../lib/engine'
export const HI_RES = 'Hi-Res'

// --- Home ------------------------------------------------------------------------------------

/** What is playing (or an album for today), then shelves: hi-res first, then your categories. */
export const HomeView = memo(function HomeView(props: {
  root: string
  lib: Library
  hasCover: (dir: string) => boolean
  onSeeAll: (filter: string) => void
  /** Your music's tiles: a Library view, Albums with a filter. */
  onLibrary: (view: 'albums' | 'artists' | 'songs', filter?: string) => void
  onOpenPlaylist: (name: string) => void
  onPlaylists: () => void
  /** No music of your own yet: a way to add it, at the top of Home. */
  onAddMusic?: () => void
  /** Edit Home open, when the page around Home holds its button (the greeting is then the page's title). */
  editing?: boolean
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
  // The online catalog's part: its mood chips (one picked filters its rows), Quick picks, rows.
  const [chip, setChip] = useState<string | null>(null)
  const localOnly = useLocalOnly()
  const online = useOnlineHome(chip)
  const quick = online.shelves?.find((s) => s.kind === 'quick_picks')
  const onlineRows = (online.shelves ?? []).filter((s) => s !== quick)
  // Your listening lately: the artists you play, and online songs for Jump back in.
  const plays = useRecentPlays(props.root, now.path)
  const recentOnline = useRecentOnline(plays)
  // What you pinned to the top of Home, in the order you pinned it.
  const pins = useShortcuts()
  const pinnedAlbums = useMemo(
    () => pins.flatMap((p) => (p.kind === 'album' ? [lib.byDir.get(p.dir)].filter((a): a is Album => !!a) : [])),
    [pins, lib],
  )
  const [layout, setLayout] = useState(readLayout)
  const [ownEditing, setOwnEditing] = useState(false)
  // The page's own Edit Home button, when it has one; otherwise Home's own, beside the greeting.
  const editing = props.editing ?? ownEditing
  const change = (l: HomeLayout) => {
    setLayout(l)
    saveLayout(l)
  }

  // Every part of Home, by id, in its own order (Edit Home puts them in yours).
  const parts: { id: string; label: string; node: React.ReactNode }[] = [
    {
      id: 'hero',
      label: 'Playing now, or picked for today',
      node: (
        <AnimatePresence mode="wait" initial={false}>
          {!playing && now.online && !localOnly ? (
            <OnlineHero key={queueKey(now.online)} song={now.online} playing={now.playing} />
          ) : (
            hero && <Hero key={hero.dir} album={hero} label={playing ? (now.playing ? 'Playing now' : 'Paused') : 'Picked for today'} />
          )}
        </AnimatePresence>
      ),
    },
    {
      id: 'shortcuts',
      label: 'Shortcuts',
      node: pins.length > 0 && (
        <Shelf
          title="Shortcuts"
          albums={pinnedAlbums}
          origin="shelf-shortcuts"
          at={(d) => pins.length - pins.findIndex((p) => p.kind === 'album' && p.dir === d)}
          more={pins.flatMap((p, i): { key: string; at: number; node: React.ReactNode }[] => {
            const at = pins.length - i
            if (p.kind === 'artist') {
              const ar = artistFor(lib, p.name)
              return ar.albums.length ? [{ key: 'artist:' + p.name, at, node: <ArtistCard artist={ar} /> }] : []
            }
            if (p.kind === 'playlist') {
              const pl = lists.find((x) => x.name === p.name)
              return pl ? [{ key: 'playlist:' + p.name, at, node: <PlaylistCard root={props.root} p={pl} onOpen={() => props.onOpenPlaylist(p.name)} /> }] : []
            }
            if (p.kind === 'online-album') return [{ key: 'oa:' + p.album.album_id, at, node: <OnlineAlbumTile album={p.album} query={p.album.title} /> }]
            if (p.kind === 'online-artist')
              return [{ key: 'oar:' + p.artist.artist_id, at, node: <OnlineArtistCard artist={{ provider: 'remote_catalog', audience: null, artwork_url: null, ...p.artist }} /> }]
            if (p.kind === 'online-collection') return [{ key: 'oc:' + p.collection.collection_id, at, node: <CollectionCard collection={p.collection} /> }]
            return []
          })}
        />
      ),
    },
    {
      id: 'jump',
      label: 'Jump back in',
      node: recent.length + playedLists.length + recentOnline.length > 0 && (
        <Shelf title="Jump back in" albums={recent} origin="shelf-recent" more={[...playedLists, ...recentOnline]} at={(d) => heard?.last_played[d] ?? 0} />
      ),
    },
    { id: 'quick', label: 'Quick picks', node: quick && <QuickPicks shelf={quick} /> },
    { id: 'familiar', label: 'Familiar artists', node: <FamiliarArtists lib={lib} plays={plays} /> },
    { id: 'playlists', label: 'Your playlists', node: <PlaylistShelf root={props.root} onOpen={props.onOpenPlaylist} onSeeAll={props.onPlaylists} /> },
    { id: 'yours', label: 'Your music', node: <YourMusic root={props.root} lib={lib} onOpen={props.onLibrary} /> },
    { id: 'most', label: 'Most played', node: top.length > 2 && <Shelf title="Most played" albums={top} origin="shelf-top" /> },
    { id: 'rediscover', label: 'Rediscover', node: rediscover.length > 0 && <Shelf title="Rediscover" albums={rediscover} origin="shelf-rediscover" /> },
    { id: 'hires', label: 'Hi-Res', node: <Shelf title="Hi-Res" albums={lib.hiRes} origin="shelf-hires" onSeeAll={() => props.onSeeAll(HI_RES)} /> },
    {
      id: 'categories',
      label: 'Your categories',
      node: shelves.map(([c, albums]) => <Shelf key={c} title={c} albums={albums} origin={`shelf-${c}`} onSeeAll={() => props.onSeeAll(c)} />),
    },
    { id: 'online', label: 'From the online catalog', node: <OnlineShelves shelves={onlineRows} more={online.more} label="Home" /> },
  ]
  const known = parts.map((p) => p.id)
  const shown = arrange(layout, known)
  const chips = <HomeChips chips={online.chips} chip={chip} onChip={setChip} />
  // The first part shown is the card for what is playing: the chips go right under it.
  const heroFirst = shown.find((s) => s.on)?.id === 'hero'
  return (
    <div className="pb-16">
      {props.editing === undefined && (
        <div className="mb-4 flex flex-wrap items-center gap-x-4 gap-y-2">
          <h2 className="title text-[24px] leading-tight">{greeting()}</h2>
          <button onClick={() => setOwnEditing((e) => !e)} aria-expanded={editing} className="btn-ghost btn-sm ml-auto">
            {editing ? 'Done' : 'Edit Home'}
          </button>
        </div>
      )}
      {!heroFirst && chips}
      {lib.albums.length === 0 && props.onAddMusic && (
        <section aria-label="Add your music" className="glass mt-5 flex flex-wrap items-center gap-x-6 gap-y-3 rounded-[24px] p-6">
          <div className="min-w-[260px] flex-1">
            <h3 className="text-[17px] font-semibold">Add your music</h3>
            <p className="mt-1 max-w-[60ch] text-[13.5px] leading-relaxed text-ink/60">
              Your FLACs and other files, beside the catalog and played bit-perfect. Choose the folder your albums are in; it is
              only read, never changed.
            </p>
          </div>
          <button onClick={props.onAddMusic} className="btn-primary">
            Add your music
          </button>
        </section>
      )}
      <AnimatePresence>
        {editing && (
          <EditHome
            key="edit"
            parts={shown.map((s) => ({ ...s, label: parts.find((p) => p.id === s.id)?.label ?? s.id }))}
            onToggle={(id) => change(toggleHome(layout, id))}
            onMove={(id, by) => change(move(layout, known, id, by))}
            onOrder={(ids) => change(reorder(layout, known, ids))}
            onReset={() => change({ order: [], hidden: [] })}
            size={layout.size ?? 'm'}
            onSize={(size) => change({ ...layout, size })}
          />
        )}
      </AnimatePresence>
      <CardWidth value={CARD_WIDTHS[layout.size ?? 'm']}>
        <div className="mt-4">
          {shown
            .filter((s) => s.on)
            .map((s) => (
              <Fragment key={s.id}>
                {parts.find((p) => p.id === s.id)?.node}
                {heroFirst && s.id === 'hero' && <div className="mt-4">{chips}</div>}
              </Fragment>
            ))}
        </div>
      </CardWidth>
    </div>
  )
})

/** "Good morning", "Good afternoon", "Good evening", by the hour. */
export function greeting() {
  const h = new Date().getHours()
  return h < 5 ? 'Good night' : h < 12 ? 'Good morning' : h < 17 ? 'Good afternoon' : 'Good evening'
}

/** Edit Home: each part on or off, and where it goes. Kept on this computer. */
function EditHome(props: {
  parts: { id: string; label: string; on: boolean }[]
  onToggle: (id: string) => void
  onMove: (id: string, by: -1 | 1) => void
  /** Dragged into this order. */
  onOrder: (ids: string[]) => void
  onReset: () => void
  size: CardSize
  onSize: (s: CardSize) => void
}) {
  return (
    <motion.section
      aria-label="Edit Home"
      initial={{ opacity: 0, height: 0 }}
      animate={{ opacity: 1, height: 'auto' }}
      exit={{ opacity: 0, height: 0 }}
      transition={spring}
      className="overflow-hidden"
    >
      <div className="glass mt-3 rounded-2xl p-3">
        <div className="flex items-baseline gap-3 px-2 pb-2 pt-1">
          <h3 className="text-[14px] font-semibold">What Home shows, in this order</h3>
          <Segmented
            size="sm"
            label="Card size"
            value={props.size}
            onChange={props.onSize}
            options={[
              { value: 's', label: 'Small' },
              { value: 'm', label: 'Medium' },
              { value: 'l', label: 'Large' },
            ]}
          />
          <button onClick={props.onReset} className="ml-auto rounded-full px-2.5 py-0.5 text-[12px] text-ink/60 hover:bg-ink/[0.08] hover:text-ink">
            As it was
          </button>
        </div>
        <Reorder.Group as="ol" axis="y" values={props.parts.map((p) => p.id)} onReorder={props.onOrder} className="grid gap-0.5">
          {props.parts.map((p, i) => (
            <HomeSectionRow key={p.id} part={p} first={i === 0} last={i === props.parts.length - 1} onToggle={props.onToggle} onMove={props.onMove} />
          ))}
        </Reorder.Group>
      </div>
    </motion.section>
  )
}

/** One section in Edit Home: dragged by its grip, or moved with the arrows (and the keyboard). */
function HomeSectionRow(props: {
  part: { id: string; label: string; on: boolean }
  first: boolean
  last: boolean
  onToggle: (id: string) => void
  onMove: (id: string, by: -1 | 1) => void
}) {
  const { part: p } = props
  const drag = useDragControls()
  const [dragging, setDragging] = useState(false)
  return (
    <Reorder.Item
      as="li"
      value={p.id}
      dragListener={false}
      dragControls={drag}
      onDragStart={() => setDragging(true)}
      onDragEnd={() => setDragging(false)}
      whileDrag={{ scale: 1.02, boxShadow: '0 12px 30px rgb(0 0 0 / 0.3)' }}
      className={`relative flex items-center gap-3 rounded-xl px-2 py-1.5 ${dragging ? 'z-10 bg-[var(--solid)]' : 'hover:bg-ink/[0.04]'}`}
    >
      <span
        onPointerDown={(e) => drag.start(e)}
        title="Drag to move"
        aria-hidden
        className="grid h-7 w-5 cursor-grab touch-none place-items-center text-ink/35 hover:text-ink/70 active:cursor-grabbing"
      >
        <svg width="10" height="14" viewBox="0 0 10 14" fill="currentColor">
          {[2, 7, 12].flatMap((y) => [<circle key={`a${y}`} cx="2.5" cy={y} r="1.3" />, <circle key={`b${y}`} cx="7.5" cy={y} r="1.3" />])}
        </svg>
      </span>
      <Switch on={p.on} onChange={() => props.onToggle(p.id)} label={p.label} />
      <span className={`min-w-0 flex-1 truncate text-[13.5px] ${p.on ? '' : 'text-ink/45'}`}>{p.label}</span>
      <button disabled={props.first} onClick={() => props.onMove(p.id, -1)} aria-label={`Move ${p.label} up`} className="grid h-7 w-7 place-items-center rounded-full text-ink/60 hover:bg-ink/[0.1] hover:text-ink disabled:opacity-30">
        ↑
      </button>
      <button disabled={props.last} onClick={() => props.onMove(p.id, 1)} aria-label={`Move ${p.label} down`} className="grid h-7 w-7 place-items-center rounded-full text-ink/60 hover:bg-ink/[0.1] hover:text-ink disabled:opacity-30">
        ↓
      </button>
    </Reorder.Item>
  )
}

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
      className="glass relative overflow-hidden rounded-[24px] p-4"
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
      <div className="relative flex flex-wrap items-center gap-x-5 gap-y-3">
        <div className="aspect-square w-[104px] shrink-0 overflow-hidden rounded-xl bg-ink/[0.06] shadow-xl shadow-black/35">
          <RemoteArt url={song.artwork_url} seed={song.album || song.title} size={200} imgClassName="h-full w-full object-cover" />
        </div>
        <div className="min-w-[220px] flex-1">
          <p className="text-[12.5px] text-ink/60">{playing ? 'Playing now' : 'Paused'}</p>
          <h2 className="title mt-1 truncate text-[22px] leading-[1.15]">{song.title}</h2>
          <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-[13px] text-ink/65">
            {artist && <span className="text-ink/80">{artist}</span>}
            {song.album && <span>{song.album}</span>}
            <AvailabilityBadge availability={availabilityOf(song)} />
          </div>
        </div>
        <div className="shrink-0 pr-2">
          <div>
            <button onClick={() => void toggle()} className="btn-primary play-main">
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
      className="glass relative overflow-hidden rounded-[24px] p-4"
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
      <div className="relative flex flex-wrap items-center gap-x-5 gap-y-3">
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
            className="aspect-square w-[104px] overflow-hidden rounded-xl bg-ink/[0.06] shadow-xl shadow-black/35"
          >
            <Art cover={cover} album={a} />
          </motion.div>
        </button>
        <div className="min-w-[220px] flex-1">
          <p className="text-[12.5px] text-ink/60">{label}</p>
          <h2 className="title mt-1 truncate text-[22px] leading-[1.15]">
            <button onClick={() => nav.openAlbum(a.dir, 'hero')} className="rounded-md text-left hover:underline">
              {a.title}
            </button>
          </h2>
          <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-[13px] text-ink/60">
            <button onClick={() => nav.openArtist(a.artist)} className="rounded-md text-left text-ink/80 hover:text-ink hover:underline">
              {a.artist}
            </button>
            {a.year && <span>{a.year}</span>}
            <QualityBadge text={a.quality} />
            {a.hiRes && <HiResPill />}
          </div>
        </div>
        <div className="shrink-0 pr-2">
          <PlayButtons tracks={a.tracks} from={albumFrom(a)} dir={a.dir} />
        </div>
      </div>
    </motion.section>
  )
}

// --- Albums ----------------------------------------------------------------------------------

export const AlbumsView = memo(function AlbumsView(props: { lib: Library; filter: string; onFilter: (f: string) => void }) {
  const { lib, filter } = props
  const signed_in = useYtmLibraryShown()
  const ytmAlbums = useYtmCount('albums', signed_in)
  const ytmUploads = useYtmCount('upload_albums', signed_in)
  // One of your genres or categories: the catalog's genre of the same name, to go on exploring.
  const nav = useNav()
  const catalog = useCatalogTiles()
  const yours = filter.startsWith(GENRE_FILTER) ? filter.slice(GENRE_FILTER.length) : filter !== 'all' && filter !== HI_RES ? filter : ''
  const online = yours ? catalogGenreFor(catalog, yours) : undefined
  const shown = useMemo(
    () =>
      filter === 'all'
        ? lib.albums
        : filter === HI_RES
          ? lib.hiRes
          : filter.startsWith(GENRE_FILTER)
            ? lib.albums.filter((a) => albumHasGenre(a, filter.slice(GENRE_FILTER.length)))
            : lib.albums.filter((a) => a.category === filter),
    [lib, filter],
  )
  const chip = (id: string, label: string, count: number) => (
    <Chip key={id} group="listen-album-filter" on={filter === id} label={label} count={count} onClick={() => props.onFilter(id)} />
  )
  const chipMaybe = (id: string, label: string, count: number | null) => (
    <Chip key={id} group="listen-album-filter" on={filter === id} label={label} count={count ?? undefined} onClick={() => props.onFilter(id)} />
  )
  if (signed_in && (filter === 'ytm:albums' || filter === 'ytm:upload_albums'))
    return (
      <div>
        <InBar>
          <div className="no-scrollbar -mx-1 flex gap-1 overflow-x-auto px-1">
            {chip('all', 'All', lib.albums.length)}
            {chipMaybe('ytm:albums', 'On YouTube Music', ytmAlbums)}
            {!!ytmUploads && chipMaybe('ytm:upload_albums', 'Your uploads', ytmUploads)}
          </div>
        </InBar>
        {filter === 'ytm:albums' ? (
          <YtmGrid part="albums" label="Your YouTube Music albums" empty="No albums saved on YouTube Music yet. Save one from its page." />
        ) : (
          <YtmGrid part="upload_albums" label="Your uploads" empty="No uploads on YouTube Music." />
        )}
      </div>
    )
  return (
    <div className="pb-16">
      <InBar>
        <div className="no-scrollbar -mx-1 flex gap-1 overflow-x-auto px-1">
          {chip('all', 'All', lib.albums.length)}
          {lib.hiRes.length > 0 && chip(HI_RES, 'Hi-Res', lib.hiRes.length)}
          {lib.categories.map(([c, n]) => chip(c, c, n))}
          {/* A genre opened from Your music: its own chip while it shows. */}
          {filter.startsWith(GENRE_FILTER) && chip(filter, filter.slice(GENRE_FILTER.length), shown.length)}
          {/* Your YouTube Music library, beside your own: online, so not in All. */}
          {signed_in && chipMaybe('ytm:albums', 'On YouTube Music', ytmAlbums)}
          {signed_in && !!ytmUploads && chipMaybe('ytm:upload_albums', 'Your uploads', ytmUploads)}
        </div>
      </InBar>
      {online && (
        <div className="mb-5 flex items-center gap-3 text-[13px] text-ink/60">
          <span>More {yours} in the online catalog:</span>
          <button onClick={() => nav.openMood(online)} className="btn btn-sm">
            {online.title}
          </button>
        </div>
      )}
      <AlbumGrid albums={shown} origin="albums" />
    </div>
  )
})

// --- Songs -----------------------------------------------------------------------------------

/** Your songs, and (signed in) the account's library songs and uploads beside them. The account's
 *  lists are read when chosen: they can be thousands long. */
export const SongsView = memo(function SongsView({ tracks }: { tracks: CatalogTrack[] }) {
  const signed_in = useYtmLibraryShown()
  const [view, setView] = useState<'all' | 'songs' | 'upload_songs'>('all')
  const chip = (id: typeof view, label: string, count?: number) => (
    <Chip key={id} group="listen-song-filter" on={view === id} label={label} count={count} onClick={() => setView(id)} />
  )
  const chips = signed_in && (
    <InBar>
      <div className="no-scrollbar -mx-1 flex gap-1 overflow-x-auto px-1">
        {chip('all', 'All', tracks.length)}
        {chip('songs', 'On YouTube Music')}
        {chip('upload_songs', 'Your uploads')}
      </div>
    </InBar>
  )
  if (signed_in && view !== 'all')
    return (
      <div>
        {chips}
        <YtmSongs
          part={view}
          label={view === 'songs' ? 'Your YouTube Music songs' : 'Your uploads'}
          empty={view === 'songs' ? 'No songs saved to your YouTube Music library yet.' : 'No songs uploaded to YouTube Music.'}
        />
      </div>
    )
  return (
    <div className="pb-16">
      {chips}
      <SongList tracks={tracks} from={{ kind: 'songs', ref: '', label: 'Songs' }} sortBy="title" pin />
    </div>
  )
})

// --- Artists ---------------------------------------------------------------------------------

export const ArtistsView = memo(function ArtistsView({ artists }: { artists: Artist[] }) {
  const signed_in = useYtmLibraryShown()
  const [view, setView] = useState<'all' | 'artists' | 'subscriptions'>('all')
  const ytmArtists = useYtmCount('artists', signed_in)
  const following = useYtmCount('subscriptions', signed_in)
  const chip = (id: typeof view, label: string, count: number | null) => (
    <Chip key={id} group="listen-artist-filter" on={view === id} label={label} count={count ?? undefined} onClick={() => setView(id)} />
  )
  const chips = signed_in && (
    <InBar>
      <div className="no-scrollbar -mx-1 flex gap-1 overflow-x-auto px-1">
        {chip('all', 'All', artists.length)}
        {chip('artists', 'On YouTube Music', ytmArtists)}
        {chip('subscriptions', 'Following', following)}
      </div>
    </InBar>
  )
  if (signed_in && view !== 'all')
    return (
      <div>
        {chips}
        <YtmGrid
          part={view}
          label={view === 'artists' ? 'Your YouTube Music artists' : 'Artists you follow'}
          empty={view === 'artists' ? 'No artists in your YouTube Music library yet.' : 'You follow no artists on YouTube Music yet.'}
        />
      </div>
    )
  return (
    <div className="pb-16">
      {chips}
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
  // The words were searched for (not typed): asked at once. Your own words, so with Signed-in
  // search on they go as your account.
  const online = useOnlineSearch(props.query, 'music', true, true)
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
        {list.map((t) => (
          <OnlineTrackRow
            key={`${t.provider}:${t.track_id}`}
            track={t}
            onPlay={() => void playWithRadio(nav.root, toRemoteItem(t))}
            onQueue={(next) => void enqueueItems(nav.root, [toRemoteItem(t)], next)}
            onAdd={() => askAddSongs([{ online: onlineEntryFrom(t) }], t.title)}
          />
        ))}
      </div>
    </>
  )
  const nav = useNav()
  // A song of yours found here plays as YouTube Music plays a song found: it, then (Autoplay on,
  // and only when it is the same song there) its YouTube Music radio.
  const playFound = (t: CatalogTrack) => void playWithRadio(nav.root, toItem(t))
  const waiting = (what: string) => <p className="py-6 text-[13px] text-ink/45">Looking for {what}…</p>
  const nothing = (what: string) => <p className="py-6 text-[13px] text-ink/55">No {what} for “{q}”.</p>

  let body: React.ReactNode
  if (chip === 'songs')
    body = (
      <>
        {props.tracks.length > 0 && (
          <section>
            {eyebrow('In your library', props.tracks.length)}
            <SongList tracks={props.tracks} from={from} pin onPlay={playFound} />
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
            {besideTop.length > 0 && (
              <MixedSongs
                query={q}
                songs={besideTop}
                onPlay={(item) => void playWithRadio(nav.root, item)}
                onSeeAll={songs.length > SONGS_BESIDE_TOP ? () => setChip('songs') : undefined}
              />
            )}
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
