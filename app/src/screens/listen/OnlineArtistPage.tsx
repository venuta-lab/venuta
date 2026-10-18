import { motion } from 'motion/react'
import { useEffect, useMemo, useState } from 'react'
import { askAddSongs } from '../../components/AddToPlaylist'
import { AvailabilityBadge } from '../../components/AvailabilityBadge'
import { IconPause, IconPlay, IconPlaylistAdd, IconShuffle } from '../../components/Icons'
import { RemoteArt, artUrl } from '../../components/RemoteArt'
import { Detail } from '../../components/ui'
import { onlineArtistPage, type OnlineArtistPage as Page, type OnlineTrack } from '../../lib/engine'
import { onlineProblem } from '../../lib/online'
import { current, enqueueItems, fromIs, playAllItems, playItems, queueKey, toRemoteItem, toggle, usePlayerValue, type From } from '../../lib/player'
import { rankOnlineTracks } from '../../lib/searchRank'
import { artistFor, plural, spring, useNav, type Library, type OnlineArtistRef } from './model'
import { OnlineAlbumCard, OnlineTrackHeader, OnlineTrackRow } from './OnlineResults'
import { onlineSongs, withOnlineAlbum } from './onlineSearch'

/*
  An online catalog artist as its own page, the way an online album has one: their picture,
  name and audience, Play / Shuffle, their top songs, more songs, albums, singles and artists
  like them. Songs are ranked for the artist's name, so their own recordings lead (not covers).
  Back returns to where it was opened from (the search, an album, another artist).
*/

const keyOf = (t: OnlineTrack) => `online:${t.provider}:${t.track_id}`
/** Songs shown before "Show all" in More songs. */
const MORE_SHOWN = 10

export function OnlineArtistPage({ artist, lib, onBack }: { artist: OnlineArtistRef; lib: Library; onBack: () => void }) {
  const nav = useNav()
  const { root } = nav
  const [page, setPage] = useState<Page | null>(null)
  const [problem, setProblem] = useState('')
  const [allMore, setAllMore] = useState(false)
  useEffect(() => {
    let live = true
    onlineArtistPage(artist.artist_id)
      .then((p) => live && setPage(p))
      .catch((e) => live && setProblem(onlineProblem(e)))
    return () => {
      live = false
    }
  }, [artist.artist_id])

  const name = page?.name || artist.name
  // Their own recordings first: covers and uploads by others don't lead their page.
  // Top songs, then the rest, each ranked: the order `artistSongs` gives the Top result's Play.
  const top = useMemo(() => rankOnlineTracks(name, page?.top_songs ?? []), [name, page])
  const more = useMemo(() => rankOnlineTracks(name, page?.songs ?? []), [name, page])
  const all = useMemo(() => [...top, ...more], [top, more])
  const items = useMemo(() => all.map(toRemoteItem), [all])
  const from: From = useMemo(() => ({ kind: 'search', ref: name, label: `Online: ${name}` }), [name])

  const nowKey = usePlayerValue((s) => {
    const c = current(s)
    return c ? queueKey(c) : ''
  })
  const playing = usePlayerValue((s) => s.playing)
  const nowFrom = usePlayerValue((s) => s.from)
  const isNow = fromIs(nowFrom, 'search', name) && all.some((t) => keyOf(t) === nowKey)
  // The same artist in your library, when there is one by this name.
  const mine = artistFor(lib, name)
  const mineSongs = mine.albums.reduce((n, a) => n + a.tracks.length, 0) || mine.songs

  const playButton = (size: 'sm' | 'lg') => (
    <button
      disabled={!items.length}
      onClick={() => (isNow ? void toggle() : void playAllItems(root, items, from))}
      className={size === 'lg' ? 'btn-primary btn-lg play-main' : 'btn-primary btn-sm'}
    >
      {isNow && playing ? <IconPause width={size === 'sm' ? 15 : undefined} height={size === 'sm' ? 15 : undefined} /> : <IconPlay width={size === 'sm' ? 15 : undefined} height={size === 'sm' ? 15 : undefined} />}
      {isNow ? (playing ? 'Pause' : 'Resume') : 'Play'}
    </button>
  )

  /** Song rows of a list: play goes on through all their songs, from this one. */
  const rows = (list: OnlineTrack[], offset: number) =>
    list.map((t, i) => (
      <OnlineTrackRow
        key={keyOf(t)}
        track={t}
        now={isNow && keyOf(t) === nowKey}
        onPlay={() => (isNow && keyOf(t) === nowKey ? void toggle() : void playItems(root, items, offset + i, from))}
        onQueue={(next) => void enqueueItems(root, [toRemoteItem(t)], next)}
        onAdd={() => askAddSongs(onlineSongs([t]), t.title)}
      />
    ))
  const moreShown = allMore || more.length <= MORE_SHOWN + 2 ? more : more.slice(0, MORE_SHOWN)
  const records = (title: string, list: Page['albums']) =>
    list.length > 0 && (
      <section className="mt-12">
        <h2 className="mb-3 text-[19px] font-semibold tracking-[-0.02em]">{title}</h2>
        <div className="no-scrollbar -mx-3 -my-2 flex gap-4 overflow-x-auto px-3 py-2">
          {list.map((a) => (
            <OnlineAlbumCard
              key={`${a.provider}:${a.album_id}`}
              album={a}
              onOpen={() => nav.openOnlineAlbum(a)}
              onPlay={() => void withOnlineAlbum(a, (t) => void playItems(root, t.map(toRemoteItem), 0, { kind: 'search', ref: a.title, label: `Online: ${a.title}` }))}
              onQueue={(next) => void withOnlineAlbum(a, (t) => void enqueueItems(root, t.map(toRemoteItem), next))}
              onAdd={() => void withOnlineAlbum(a, (t) => askAddSongs(onlineSongs(t), a.title))}
            />
          ))}
        </div>
      </section>
    )

  return (
    <Detail title={name} onBack={onBack} glow={artUrl(page?.artwork_url ?? artist.artwork_url, 226)} tools={items.length ? playButton('sm') : undefined}>
      <div className="relative flex flex-wrap items-end gap-8">
        <div className="grid aspect-square w-[220px] shrink-0 place-items-center overflow-hidden rounded-full bg-ink/[0.06] text-[56px] text-ink/25 shadow-2xl shadow-black/40">
          <RemoteArt url={page?.artwork_url ?? artist.artwork_url} fallback={name.slice(0, 1)} size={220} imgClassName="h-full w-full object-cover" />
        </div>
        <motion.div className="min-w-[260px] flex-1 pb-1" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ ...spring, delay: 0.05 }}>
          <p className="flex items-center gap-2 text-[13px] font-medium text-ink/60">
            Artist <AvailabilityBadge availability="online" />
          </p>
          <h1 className="title hero-title mt-1 text-[34px] leading-[1.1]">{name}</h1>
          <p className="mt-2.5 text-[13px] text-ink/60">
            {[page?.audience ? `${compact(page.audience)} monthly listeners` : '', page === null && !problem ? 'Reading their page…' : all.length ? plural(all.length, 'song') : '']
              .filter(Boolean)
              .join(' · ')}
          </p>
          {mine.albums.length > 0 && (
            <button onClick={() => nav.openArtist(mine.name)} className="mt-1 text-left text-[13px] text-ink/60 underline-offset-4 hover:text-ink hover:underline">
              In your library: {plural(mineSongs, 'song')}
            </button>
          )}
          <div className="mt-5 flex flex-wrap items-center gap-2.5">
            {playButton('lg')}
            <button disabled={items.length < 2} onClick={() => void playAllItems(root, items, from, 'shuffle')} className="btn btn-lg shuffle-main">
              <IconShuffle />
              Shuffle
            </button>
            <button disabled={!all.length} onClick={() => askAddSongs(onlineSongs(top.length ? top : all), name)} className="btn-ghost btn-lg" title="Add their top songs to a playlist">
              <IconPlaylistAdd width={18} height={18} />
              Add to playlist
            </button>
          </div>
        </motion.div>
      </div>

      {problem ? (
        <p role="status" className="mt-9 py-6 text-[13px] text-ink/55">
          {problem}
        </p>
      ) : page === null ? (
        <p className="mt-9 py-6 text-[13px] text-ink/45">Reading {name}’s page from the online catalog…</p>
      ) : (
        <>
          {top.length > 0 && (
            <section className="mt-9">
              <h2 className="mb-3 text-[19px] font-semibold tracking-[-0.02em]">Top songs</h2>
              <OnlineTrackHeader />
              <div className="mt-1 grid gap-0.5">{rows(top, 0)}</div>
            </section>
          )}
          {more.length > 0 && (
            <section className="mt-12">
              <div className="mb-3 flex items-baseline gap-3">
                <h2 className="text-[19px] font-semibold tracking-[-0.02em]">More songs</h2>
                <span className="text-[12.5px] text-ink/50">{more.length.toLocaleString('en-IN')}</span>
              </div>
              <OnlineTrackHeader />
              <div className="mt-1 grid gap-0.5">{rows(moreShown, top.length)}</div>
              {moreShown.length < more.length && (
                <button onClick={() => setAllMore(true)} className="btn-ghost btn-sm mt-2">
                  Show all {more.length} songs
                </button>
              )}
            </section>
          )}
          {records('Albums', page.albums)}
          {records('Singles & EPs', page.singles)}
          {page.similar.length > 0 && (
            <section className="mt-12">
              <h2 className="mb-3 text-[19px] font-semibold tracking-[-0.02em]">Fans also like</h2>
              <div className="flex flex-wrap gap-2">
                {page.similar.map((ar) => (
                  <button
                    key={`${ar.provider}:${ar.artist_id}`}
                    onClick={() => nav.openOnlineArtist(ar)}
                    title={`Open ${ar.name}`}
                    className="flex items-center gap-2 rounded-full bg-ink/[0.06] py-1 pl-1 pr-3.5 text-[13px] transition-colors hover:bg-ink/[0.1] active:bg-ink/[0.14]"
                  >
                    <span className="grid h-7 w-7 place-items-center overflow-hidden rounded-full bg-ink/[0.08] text-[12px] text-ink/50">
                      <RemoteArt url={ar.artwork_url} fallback={ar.name.slice(0, 1)} size={28} imgClassName="h-full w-full object-cover" />
                    </span>
                    {ar.name}
                  </button>
                ))}
              </div>
            </section>
          )}
          {!all.length && !page.albums.length && !page.singles.length && (
            <p className="mt-9 py-6 text-[13px] text-ink/55">The online catalog lists nothing more for {name} yet.</p>
          )}
        </>
      )}
    </Detail>
  )
}

/** 642000000 → "642M". */
function compact(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(n >= 1e10 ? 0 : 1)}B`
  if (n >= 1e6) return `${(n / 1e6).toFixed(n >= 1e7 ? 0 : 1)}M`
  if (n >= 1e3) return `${Math.round(n / 1e3)}K`
  return String(n)
}
