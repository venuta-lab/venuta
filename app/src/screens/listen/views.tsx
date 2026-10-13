import { AnimatePresence, motion } from 'motion/react'
import { memo, useEffect, useMemo, useState } from 'react'
import { listeningSummary, type ListeningSummary } from '../../lib/engine'
import { setAccent, useCovers } from '../../lib/covers'
import {
  appearsOn,
  artistFor,
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
import { Detail, InBar } from '../../components/ui'
import { AlbumGrid, Art, Avatar, Chip, HiResPill, PlayButtons, QualityBadge, Shelf } from './parts'
import { SongList } from './SongList'

export const HI_RES = 'Hi-Res'

// --- Home ------------------------------------------------------------------------------------

/** What is playing (or an album for today), then shelves: hi-res first, then your categories. */
export const HomeView = memo(function HomeView(props: {
  root: string
  lib: Library
  hasCover: (dir: string) => boolean
  onSeeAll: (filter: string) => void
}) {
  const { lib } = props
  const now = useNowPlaying()
  // What you've played, fetched again when the album playing changes.
  const [heard, setHeard] = useState<ListeningSummary | null>(null)
  useEffect(() => {
    let live = true
    void listeningSummary(props.root)
      .then((h) => live && setHeard(h))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [props.root, now.dir])
  const recent = useMemo(
    () => (heard?.recent ?? []).map((d) => lib.byDir.get(d)).filter((a): a is Album => !!a).slice(0, 24),
    [heard, lib],
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
        {hero && (
          <Hero
            key={hero.dir}
            album={hero}
            label={playing ? (now.playing ? 'Playing now' : 'Paused') : 'Picked for today'}
          />
        )}
      </AnimatePresence>
      {recent.length > 0 && <Shelf title="Jump back in" albums={recent} origin="shelf-recent" />}
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

function Hero({ album: a, label }: { album: Album; label: string }) {
  const nav = useNav()
  const cover = useCovers(320)(a.dir)
  return (
    <motion.section
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -6, transition: { duration: 0.15 } }}
      transition={spring}
      onHoverStart={() => setAccent(cover?.accent)}
      className="glass relative overflow-hidden rounded-[28px] p-6"
    >
      {cover && (
        <img
          aria-hidden
          src={cover.src}
          alt=""
          className="pointer-events-none absolute inset-0 h-full w-full scale-150 object-cover opacity-45 blur-3xl"
        />
      )}
      <div className="relative flex flex-wrap items-center gap-7">
        <button
          onClick={() => {
            setAccent(cover?.accent)
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
          <h2 className="mt-1.5 text-[30px] font-semibold leading-[1.1] tracking-[-0.03em]">
            <button onClick={() => nav.openAlbum(a.dir, 'hero')} className="rounded-md text-left hover:underline">
              {a.title}
            </button>
          </h2>
          <button onClick={() => nav.openArtist(a.artist)} className="mt-1.5 rounded-md text-left text-[15px] text-accent-ink hover:underline">
            {a.artist}
          </button>
          <div className="mt-2.5 flex flex-wrap items-center gap-2 text-[13px] text-ink/60">
            {a.year && <span>{a.year}</span>}
            <QualityBadge text={a.quality} />
            {a.hiRes && <HiResPill />}
          </div>
          <div className="mt-5">
            <PlayButtons tracks={a.tracks} from={a.title} dir={a.dir} />
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
    <div className="grid grid-cols-[repeat(auto-fill,minmax(150px,1fr))] gap-x-5 gap-y-7 pb-16">
      {artists.map((ar) => (
        <ArtistCard key={ar.key} artist={ar} />
      ))}
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

function ArtistCard({ artist: ar }: { artist: Artist }) {
  const nav = useNav()
  const c = useArtistCover(ar)
  return (
    <motion.button
      whileHover={{ y: -5 }}
      transition={spring}
      onHoverStart={() => setAccent(c?.accent)}
      onFocus={() => setAccent(c?.accent)}
      onClick={() => nav.openArtist(ar.name)}
      className="group block w-full min-w-0 text-center"
    >
      <Avatar name={ar.name} cover={c} />
      <div className="mt-3 truncate text-[14px] font-medium">{ar.name}</div>
      <div className="text-[12.5px] text-ink/55">{plural(ar.albums.length, 'album')}</div>
    </motion.button>
  )
}

/** One artist: their albums, newest first, and the songs they sing on other people's albums. */
export function ArtistPage({ lib, name, onBack }: { lib: Library; name: string; onBack: () => void }) {
  const ar = useMemo(() => artistFor(lib, name), [lib, name])
  const also = useMemo(() => appearsOn(lib, ar.key), [lib, ar.key])
  const own = useMemo(() => ar.albums.flatMap((a) => a.tracks), [ar])
  const cover = useArtistCover(ar)
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
      tools={<PlayButtons tracks={own.length ? own : also} from={ar.name} label="Play all" size="sm" />}
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
          <h1 className="text-[40px] font-semibold leading-none tracking-[-0.035em]">{ar.name}</h1>
          <p className="mt-2.5 text-[14px] text-ink/60">{counts.join(', ') || 'Nothing by them in your library yet.'}</p>
          <div className="mt-5">
            <PlayButtons tracks={own.length ? own : also} from={ar.name} label="Play all" />
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
          <SongList tracks={also} from={ar.name} />
        </section>
      )}
    </Detail>
  )
}

// --- Search ----------------------------------------------------------------------------------

/** Everything that matches, together: albums, artists, then songs. */
export function SearchResults(props: { query: string; albums: Album[]; artists: Artist[]; tracks: Library['tracks'] }) {
  const nothing = !props.albums.length && !props.artists.length && !props.tracks.length
  if (nothing) return <p className="py-20 text-center text-[14px] text-ink/55">Nothing matches “{props.query.trim()}”.</p>
  return (
    <div className="pb-16">
      {props.artists.length > 0 && <ArtistRow artists={props.artists} />}
      <Shelf title="Albums" albums={props.albums} origin="search" limit={40} spaced={props.artists.length > 0} />
      {props.tracks.length > 0 && (
        <section className={props.albums.length || props.artists.length ? 'mt-10' : ''}>
          <div className="mb-3 flex items-baseline gap-3">
            <h2 className="text-[19px] font-semibold tracking-[-0.02em]">Songs</h2>
            <span className="text-[12.5px] text-ink/50">{props.tracks.length.toLocaleString('en-IN')}</span>
          </div>
          <SongList tracks={props.tracks} from={`Search: ${props.query.trim()}`} pin />
        </section>
      )}
    </div>
  )
}

function ArtistRow({ artists }: { artists: Artist[] }) {
  return (
    <section>
      <h2 className="mb-3.5 text-[19px] font-semibold tracking-[-0.02em]">Artists</h2>
      <div className="no-scrollbar -mx-3 -my-3 flex gap-5 overflow-x-auto px-3 py-3">
        {artists.slice(0, 30).map((ar) => (
          <div key={ar.key} className="w-[128px] shrink-0">
            <ArtistCard artist={ar} />
          </div>
        ))}
      </div>
    </section>
  )
}
