import { useEffect, useMemo } from 'react'
import { askAddSongs } from '../../components/AddToPlaylist'
import { AvailabilityBadge } from '../../components/AvailabilityBadge'
import { IconMore, IconPlay } from '../../components/Icons'
import { RemoteArt } from '../../components/RemoteArt'
import { quality, type OnlineAlbum, type OnlineArtist } from '../../lib/engine'
import { loadCovers, useCovers } from '../../lib/covers'
import { menuFor } from '../../lib/actions'
import { menuFromEvent } from '../../lib/menu'
import { enqueueItems, onlineEntryFrom, playItems, queueKey, songLength, toggle, usePlayerValue, current, type From } from '../../lib/player'
import { useNav } from './model'
import { fromOnline, itemOf, onlineItems, playOnlineArtistPage, withOnlineAlbum, type MixedSong } from './onlineSearch'
import { QualityBadge } from './parts'

/*
  The pieces of the All results that mix your library and the online catalog: the songs beside
  the Top result (yours first and marked so, then the catalog's you don't have), and cards for
  online albums and artists sized like the library's, so a row of both reads as one row.
*/

export type { MixedSong } from './onlineSearch'


/** Plays as a number the way the catalog writes it ("534M plays"). */
function playsText(n: number | null | undefined): string {
  if (!n) return ''
  const units: [number, string][] = [
    [1e9, 'B'],
    [1e6, 'M'],
    [1e3, 'K'],
  ]
  for (const [size, unit] of units) if (n >= size) return `${(n / size).toFixed(n >= size * 10 ? 0 : 1).replace(/\.0$/, '')}${unit} plays`
  return `${n} plays`
}

/**
 * The songs beside the Top result: a few, yours first. Playing one plays the list from it, so
 * the next is the next row whichever kind it is.
 */
export function MixedSongs(props: {
  query: string
  songs: MixedSong[]
  onSeeAll?: () => void
  /** "Songs" unless the page says better ("Top songs"). */
  title?: string
  /** What "See all" says ("Show fewer" once all are shown). */
  seeAllLabel?: string
  /** Where playing from here says it played from (the search, unless given). */
  from?: From
}) {
  const { root } = useNav()
  const cover = useCovers(96)
  const dirs = JSON.stringify([...new Set(props.songs.flatMap((s) => (s.where === 'library' ? [s.track.dir] : [])))])
  useEffect(() => {
    void loadCovers(root, JSON.parse(dirs) as string[], 96)
  }, [root, dirs])
  const items = useMemo(() => props.songs.map(itemOf), [props.songs])
  const nowKey = usePlayerValue((st) => {
    const c = current(st)
    return c ? queueKey(c) : ''
  })
  const from: From = props.from ?? fromOnline(props.query)
  const title = props.title ?? 'Songs'
  return (
    <section aria-label={title} className="min-w-0">
      <div className="mb-2 flex items-baseline gap-3">
        <h2 className="title text-[19px]">{title}</h2>
        {props.onSeeAll && (
          <button onClick={props.onSeeAll} className="ml-auto rounded-full px-3 py-1 text-[12.5px] text-ink/65 hover:bg-ink/[0.08] hover:text-ink">
            {props.seeAllLabel ?? 'See all'}
          </button>
        )}
      </div>
      <div className="grid gap-0.5">
        {props.songs.map((s, i) => {
          const item = items[i]
          const now = queueKey(item) === nowKey
          const t = s.track
          const art =
            s.where === 'library' ? (
              cover(s.track.dir) ? <img src={cover(s.track.dir)?.src} alt="" draggable={false} className="h-full w-full object-cover" /> : null
            ) : (
              <RemoteArt url={s.track.artwork_url} seed={s.track.album || s.track.title} size={40} imgClassName="h-full w-full object-cover" />
            )
          const menu = () =>
            menuFor(
              'song',
              { online: s.where === 'online', noArtist: true },
              {
                play: () => void playItems(root, items, i, from),
                next: () => void enqueueItems(root, [item], true),
                queue: () => void enqueueItems(root, [item], false),
                playlist: () => askAddSongs([s.where === 'library' ? { path: s.track.path } : { online: onlineEntryFrom(s.track) }], t.title),
              },
            )
          return (
            <div
              key={queueKey(item)}
              role="button"
              tabIndex={0}
              aria-current={now || undefined}
              onClick={(e) => !(e.target as HTMLElement).closest('button') && (now ? void toggle() : void playItems(root, items, i, from))}
              onKeyDown={(e) => e.key === 'Enter' && e.target === e.currentTarget && void playItems(root, items, i, from)}
              onContextMenu={(e) => menuFromEvent(e, menu())}
              className={`row group grid h-[52px] cursor-pointer grid-cols-[40px_minmax(0,1fr)_auto_44px_auto_32px] items-center gap-3 rounded-xl px-2 hover:bg-ink/[0.05] focus-visible:bg-ink/[0.06] active:bg-ink/[0.09] ${now ? 'text-accent-ink' : ''}`}
            >
              <span className="relative h-10 w-10 overflow-hidden rounded-lg bg-ink/[0.06]">
                {art}
                <span className="absolute inset-0 grid place-items-center bg-scrim text-ink opacity-0 transition-opacity duration-150 group-hover:opacity-100">
                  <IconPlay width={15} height={15} />
                </span>
              </span>
              <span className="min-w-0">
                <span className="block truncate text-[13.5px] font-medium">{t.title}</span>
                <span className="block truncate text-[12.5px] text-ink/55">{t.artists.join(', ') || 'Unknown artist'}</span>
              </span>
              <span className="text-right text-[12px] tabular-nums text-ink/50">
                {s.where === 'library' ? <QualityBadge text={quality(s.track)} /> : playsText(s.track.plays)}
              </span>
              <span className="text-right text-[12.5px] tabular-nums text-ink/55">{t.duration_ms ? songLength(t.duration_ms) : ''}</span>
              <AvailabilityBadge availability={s.where === 'library' ? 'local' : 'online'} />
              <button
                onClick={(e) => menuFromEvent(e, menu())}
                aria-label={`More for ${t.title}`}
                className="grid h-8 w-8 place-items-center rounded-full text-ink/60 opacity-0 hover:bg-ink/10 hover:text-ink focus-visible:opacity-100 group-hover:opacity-100"
              >
                <IconMore />
              </button>
            </div>
          )
        })}
      </div>
    </section>
  )
}

/** An online album card the size of a library one: opens its page; the round button plays it. */
export function OnlineAlbumTile({ album: a, query }: { album: OnlineAlbum; query: string }) {
  const { root, openOnlineAlbum } = useNav()
  const from: From = { kind: 'search', ref: query, label: `Online: ${a.title}` }
  const playIt = () => void withOnlineAlbum(a, (t) => void playItems(root, onlineItems(t), 0, from))
  const menu = () =>
    menuFor(
      'album',
      { online: true },
      {
        open: () => openOnlineAlbum(a),
        play: playIt,
        next: () => void withOnlineAlbum(a, (t) => void enqueueItems(root, onlineItems(t), true)),
        queue: () => void withOnlineAlbum(a, (t) => void enqueueItems(root, onlineItems(t), false)),
        playlist: () => void withOnlineAlbum(a, (t) => askAddSongs(t.map((x) => ({ online: onlineEntryFrom(x) })), a.title)),
      },
    )
  const line = [a.kind, a.artists.join(', '), a.release_date?.slice(0, 4)].filter(Boolean).join(' · ')
  return (
    <div className="group relative">
      <button onClick={() => openOnlineAlbum(a)} onContextMenu={(e) => menuFromEvent(e, menu())} title={`Open “${a.title}”`} className="lift block w-full text-left">
        <span className="relative grid aspect-square w-full place-items-center overflow-hidden rounded-2xl bg-ink/[0.06] text-[28px] text-ink/25 shadow-xl shadow-black/25">
          <RemoteArt url={a.artwork_url} seed={a.title} size={168} imgClassName="h-full w-full object-cover" />
        </span>
        <span className="mt-3 block truncate text-[14px] font-medium">{a.title}</span>
        <span className="block truncate text-[12.5px] text-ink/55">{line}</span>
      </button>
      <button
        onClick={playIt}
        aria-label={`Play “${a.title}”`}
        title="Play"
        className="absolute right-2.5 top-[118px] grid h-10 w-10 place-items-center rounded-full bg-accent text-on-accent opacity-0 shadow-lg transition-opacity focus-visible:opacity-100 group-hover:opacity-100"
      >
        <IconPlay width={17} height={17} />
      </button>
    </div>
  )
}

/** An online artist card the size of a library one: a round picture, the name, the audience. */
export function OnlineArtistCard({ artist: ar }: { artist: OnlineArtist }) {
  const { root, openOnlineArtist } = useNav()
  const audience = ar.audience ? playsText(ar.audience).replace(' plays', ' monthly audience') : 'Artist'
  return (
    <button
      onClick={() => openOnlineArtist(ar)}
      onContextMenu={(e) =>
        menuFromEvent(e, menuFor('artist', { online: true }, { open: () => openOnlineArtist(ar), play: () => void playOnlineArtistPage(root, ar) }))
      }
      title={`Open ${ar.name}`}
      className="lift group block w-full min-w-0 text-center"
    >
      <span className="block aspect-square overflow-hidden rounded-full bg-ink/[0.06] text-[40px] text-ink/30 shadow-xl shadow-black/25">
        <RemoteArt url={ar.artwork_url} fallback={<span className="grid h-full w-full place-items-center">{ar.name.slice(0, 1)}</span>} size={168} imgClassName="h-full w-full object-cover" />
      </span>
      <span className="mt-3 block truncate text-[14px] font-medium">{ar.name}</span>
      <span className="block truncate text-[12.5px] text-ink/55">{audience}</span>
    </button>
  )
}
