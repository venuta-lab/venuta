import { useEffect, useState } from 'react'
import { askAddSongs } from '../../components/AddToPlaylist'
import { AvailabilityBadge } from '../../components/AvailabilityBadge'
import { IconMore, IconPlay, IconPlaylistAdd } from '../../components/Icons'
import { RemoteArt } from '../../components/RemoteArt'
import { onlineAlbumTracks, onlineSearch, type OnlineAlbum, type OnlineArtist, type OnlineResults as Results, type OnlineTrack } from '../../lib/engine'
import { notify } from '../../lib/notify'
import { OFFLINE, onlineProblem, useNetworkOnline } from '../../lib/online'
import { songLength, enqueueItems, onlineEntryFrom, playItems, toRemoteItem, type From } from '../../lib/player'
import { useNav } from './model'
import { menuFor } from '../../lib/actions'
import { menuFromEvent } from '../../lib/menu'
import { dragSource } from '../../lib/drag'

/*
  The online catalog under your library's search results: songs, albums and artists for the
  same words. Rows match the local Songs list columns so local and online read as one family.
  While a new query is in flight we keep the last hits dimmed and show a skeleton, so the page
  never flashes empty for a few seconds.
*/

const fromOnline = (query: string): From => ({ kind: 'search', ref: query, label: `Online: ${query}` })
const items = (tracks: OnlineTrack[]) => tracks.map(toRemoteItem)
const songs = (tracks: OnlineTrack[]) => tracks.map((t) => ({ online: onlineEntryFrom(t) }))
const say = onlineProblem

/** Same column rhythm as `SongList` so online and local search feel like one list. */
const COLS = 'grid-cols-[36px_minmax(0,1.7fr)_minmax(0,1.1fr)_minmax(0,1.1fr)_52px_96px_32px]'

export type OnlineSearchState = {
  /** Words currently requested (the search box). */
  query: string
  /** Which query `results` answered; may lag while loading. */
  resultsFor: string
  results: Results | null
  error: string
  /** True while a request for the current query is outstanding (including debounce). */
  loading: boolean
  /** The computer is offline: nothing was asked, and `error` says so. */
  offline: boolean
}

/** The online search for `query` (asked a moment after typing stops). Keeps prior hits while loading. */
export function useOnlineSearch(query: string): OnlineSearchState {
  const connected = useNetworkOnline()
  const [state, setState] = useState<OnlineSearchState>({
    query: '',
    resultsFor: '',
    results: null,
    error: '',
    loading: false,
    offline: false,
  })
  useEffect(() => {
    const q = query.trim()
    // Nothing typed; or offline, when nothing is asked (asked again by itself once connected).
    if (!q || !connected) {
      const offline = !!q && !connected
      setState({ query: q, resultsFor: q, results: null, error: offline ? OFFLINE : '', loading: false, offline })
      return
    }
    // Mark loading immediately; keep last results (resultsFor) until the new ones land.
    setState((s) => ({ ...s, query: q, loading: true, error: '', offline: false }))
    let live = true
    const t = window.setTimeout(() => {
      onlineSearch(q)
        .then((results) => live && setState({ query: q, resultsFor: q, results, error: '', loading: false, offline: false }))
        .catch((e) => live && setState({ query: q, resultsFor: q, results: null, error: say(e), loading: false, offline: false }))
    }, 350)
    return () => {
      live = false
      window.clearTimeout(t)
    }
  }, [query, connected])
  return state
}

function OnlineSkeleton({ rows = 5 }: { rows?: number }) {
  return (
    <div aria-busy="true" aria-live="polite" className="grid gap-0.5">
      <p className="sr-only">Searching online…</p>
      {Array.from({ length: rows }, (_, i) => (
        <div key={i} className={`grid ${COLS} h-11 items-center gap-3 px-2`} style={{ opacity: 1 - i * 0.08 }}>
          <span className="shimmer h-8 w-8 rounded-md" />
          <span className="shimmer h-3 w-[70%] rounded-full" />
          <span className="shimmer h-3 w-[55%] rounded-full" />
          <span className="shimmer h-3 w-[50%] rounded-full" />
          <span className="shimmer ml-auto h-3 w-8 rounded-full" />
          <span className="shimmer h-4 w-14 rounded-full" />
          <span />
        </div>
      ))}
    </div>
  )
}

/**
 * `libraryEmpty`: nothing in the library matched, so this section also says that, in one line
 * that agrees with what the online search is doing (searching, found, found nothing, offline).
 */
export function OnlineResults(props: { query: string; spaced: boolean; libraryEmpty?: boolean }) {
  const { root, openOnlineAlbum } = useNav()
  const { results, error, loading, resultsFor, offline } = useOnlineSearch(props.query)
  const q = props.query.trim()
  if (!q) return null

  const fresh = resultsFor === q
  const stale = loading && !!results && !fresh
  const showResults = !!results && (fresh || loading)
  const empty = fresh && !loading && results && !results.tracks.length && !results.albums.length && !results.artists.length
  const from = fromOnline(q)

  if (props.libraryEmpty && empty)
    return <p className="py-20 text-center text-[14px] text-ink/55">Nothing matches “{q}” in your library or the online catalog.</p>
  const headline = props.libraryEmpty && (
    <p className="mb-6 pt-8 text-center text-[13.5px] text-ink/55">
      Nothing in your library for “{q}”{loading ? '. Searching the online catalog…' : '.'}
    </p>
  )

  /** An album's songs, then play them or add them. */
  const withAlbum = async (a: OnlineAlbum, then: (tracks: OnlineTrack[]) => void) => {
    try {
      const tracks = await onlineAlbumTracks(a.album_id)
      if (!tracks.length) return notify(`“${a.title}” has no songs online.`)
      then(tracks)
    } catch (e) {
      notify(`Couldn't open “${a.title}”: ${say(e)}`)
    }
  }
  /** An artist's songs (a search by their name), played. */
  const playArtist = async (ar: OnlineArtist) => {
    try {
      const found = await onlineSearch(ar.name)
      if (!found.tracks.length) return notify(`No songs by ${ar.name} online.`)
      void playItems(root, items(found.tracks), 0, fromOnline(ar.name))
    } catch (e) {
      notify(`Couldn't play ${ar.name}: ${say(e)}`)
    }
  }

  if (offline)
    return (
      <>
        {headline}
        <section className={`${props.spaced ? 'mt-12' : ''} opacity-70`} aria-label="Online">
          <div className="mb-2 flex items-baseline gap-3">
            <h2 className="text-[19px] font-semibold tracking-[-0.02em] text-ink/60">Online</h2>
            <span className="text-[12.5px] text-ink/50">{OFFLINE}</span>
          </div>
          <p className="text-[13px] leading-relaxed text-ink/55">
            Songs in your library play as always. Online results come back by themselves when you’re connected.
          </p>
        </section>
      </>
    )

  return (
    <>
      {headline}
      <section className={props.spaced ? 'mt-12' : ''} aria-label="Online">
        <div className="mb-3 flex items-baseline gap-3">
          <h2 className="text-[19px] font-semibold tracking-[-0.02em]">Online</h2>
          <span className="text-[12.5px] text-ink/50">
            {loading ? (
              <span className="inline-flex items-center gap-2">
                <span className="spin inline-block h-3 w-3 rounded-full border-2 border-ink/20 border-t-ink/60" aria-hidden />
                Searching online…
              </span>
            ) : (
              'Plays online until you have the file'
            )}
          </span>
        </div>
        {error && !loading ? (
          <p role="status" className="py-3 text-[13px] text-ink/55">
            {error}
          </p>
        ) : loading && !showResults ? (
          <>
            <OnlineTrackHeader />
            <div className="mt-1">
              <OnlineSkeleton />
            </div>
          </>
        ) : empty ? (
          <p className="py-3 text-[13px] text-ink/55">Nothing in the online catalog for “{q}”.</p>
        ) : showResults && results ? (
          <div className={`grid gap-8 transition-opacity duration-200 ${stale || loading ? 'opacity-55' : 'opacity-100'}`}>
            {results.tracks.length > 0 && (
              <div>
                <OnlineTrackHeader />
                <div className="mt-1 grid gap-0.5">
                  {results.tracks.map((t, i) => (
                    <OnlineTrackRow
                    key={`${t.provider}:${t.track_id}`}
                      track={t}
                      onPlay={() => void playItems(root, items(results.tracks), i, from)}
                      onQueue={(next) => void enqueueItems(root, items([t]), next)}
                      onAdd={() => askAddSongs(songs([t]), t.title)}
                    />
                  ))}
                </div>
                {loading && (
                  <div className="mt-1">
                    <OnlineSkeleton rows={2} />
                  </div>
                )}
              </div>
            )}
            {results.albums.length > 0 && (
              <div>
                <h3 className="mb-2.5 text-[14.5px] font-semibold">Albums</h3>
                <div className="no-scrollbar -mx-3 -my-2 flex gap-4 overflow-x-auto px-3 py-2">
                  {results.albums.map((a) => (
                    <AlbumCard
                    key={`${a.provider}:${a.album_id}`}
                    album={a}
                    onOpen={() => openOnlineAlbum(a)}
                      onPlay={() => void withAlbum(a, (t) => void playItems(root, items(t), 0, { kind: 'search', ref: q, label: `Online: ${a.title}` }))}
                      onQueue={(next) => void withAlbum(a, (t) => void enqueueItems(root, items(t), next))}
                      onAdd={() => void withAlbum(a, (t) => askAddSongs(songs(t), a.title))}
                    />
                  ))}
                </div>
              </div>
            )}
            {results.artists.length > 0 && (
              <div>
                <h3 className="mb-2.5 text-[14.5px] font-semibold">Artists</h3>
                <div className="flex flex-wrap gap-2">
                  {results.artists.map((ar) => (
                    <button
                      key={`${ar.provider}:${ar.artist_id}`}
                      onClick={() => void playArtist(ar)}
                      title={`Play songs by ${ar.name}`}
                      className="flex items-center gap-2 rounded-full bg-ink/[0.06] py-1 pl-1 pr-3.5 text-[13px] transition-colors hover:bg-ink/[0.1] active:bg-ink/[0.14]"
                    >
                      <span className="grid h-7 w-7 place-items-center overflow-hidden rounded-full bg-ink/[0.08] text-[12px] text-ink/50">
                        <RemoteArt url={ar.artwork_url} fallback={ar.name.slice(0, 1)} imgClassName="h-full w-full object-cover" />
                      </span>
                      {ar.name}
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>
        ) : null}
      </section>
    </>
  )
}

/** The column names over online song rows (the same rhythm as the library's song lists). */
export function OnlineTrackHeader() {
  return (
    <div className={`grid ${COLS} items-center gap-3 border-b border-edge px-2 pb-1.5 text-[12px] text-ink/55`}>
      <span />
      <span>Title</span>
      <span>Artist</span>
      <span>Album</span>
      <span className="text-right">Time</span>
      <span>Where</span>
      <span />
    </div>
  )
}

/** One online song: click plays it; Add to playlist keeps its id. `now`: it is the song playing. */
export function OnlineTrackRow(props: {
  track: OnlineTrack
  now?: boolean
  onPlay: () => void
  onQueue: (next: boolean) => void
  onAdd: () => void
}) {
  const t = props.track
  const artists = t.artists.join(', ') || 'Unknown artist'
  const menu = () =>
    menuFor(
      'song',
      { online: true, noArtist: true },
      { play: props.onPlay, next: () => props.onQueue(true), queue: () => props.onQueue(false), playlist: props.onAdd },
    )
  return (
    <div
      role="button"
      tabIndex={0}
      onContextMenu={(e) => menuFromEvent(e, menu())}
      {...dragSource(() => ({ label: t.title, count: 1, cover: t.artwork_url ?? undefined, songs: async () => [toRemoteItem(t)] }))}
      onClick={(e) => !(e.target as HTMLElement).closest('button') && props.onPlay()}
      onKeyDown={(e) => e.key === 'Enter' && e.target === e.currentTarget && props.onPlay()}
      aria-current={props.now || undefined}
      className={`row group grid ${COLS} h-11 cursor-pointer items-center gap-3 rounded-xl px-2 hover:bg-ink/[0.05] focus-visible:bg-ink/[0.06] active:bg-ink/[0.09] ${props.now ? 'text-accent-ink' : ''}`}
    >
      <button
        tabIndex={-1}
        aria-label={`Play ${t.title}`}
        onClick={props.onPlay}
        className="relative h-8 w-8 overflow-hidden rounded-md bg-ink/[0.06] text-ink/30"
      >
        <RemoteArt url={t.artwork_url} seed={t.album || t.title} imgClassName="h-full w-full object-cover" />
        <span className="absolute inset-0 grid place-items-center bg-scrim text-ink opacity-0 transition-opacity duration-150 group-hover:opacity-100">
          <IconPlay width={14} height={14} />
        </span>
      </button>
      <span className="truncate text-[13.5px] font-medium">{t.title}</span>
      <span className="truncate text-[13px] text-ink/60">{artists}</span>
      <span className="truncate text-[13px] text-ink/60">{t.album ?? ''}</span>
      <span className="text-right text-[12.5px] tabular-nums text-ink/55">{songLength(t.duration_ms)}</span>
      <span className="min-w-0">
        <AvailabilityBadge availability="online" />
      </span>
      <span className="flex items-center justify-end gap-0.5 opacity-0 transition-opacity focus-within:opacity-100 group-hover:opacity-100">
        <button
          onClick={props.onAdd}
          aria-label={`Add ${t.title} to a playlist`}
          title="Add to playlist"
          className="grid h-8 w-8 place-items-center rounded-full text-ink/60 hover:bg-ink/10 hover:text-ink"
        >
          <IconPlaylistAdd width={17} height={17} />
        </button>
        <button onClick={(e) => menuFromEvent(e, menu())} aria-label={`More for ${t.title}`} className="grid h-8 w-8 place-items-center rounded-full text-ink/60 hover:bg-ink/10 hover:text-ink">
          <IconMore />
        </button>
      </span>
    </div>
  )
}

function AlbumCard(props: { album: OnlineAlbum; onOpen: () => void; onPlay: () => void; onQueue: (next: boolean) => void; onAdd: () => void }) {
  const a = props.album
  const menu = () =>
    menuFor(
      'album',
      { online: true },
      { open: props.onOpen, play: props.onPlay, next: () => props.onQueue(true), queue: () => props.onQueue(false), playlist: props.onAdd },
    )
  return (
    <div className="group relative w-[148px] shrink-0">
      <button onClick={props.onOpen} onContextMenu={(e) => menuFromEvent(e, menu())} title={`Open “${a.title}”`} className="block w-full text-left">
        <span className="relative grid aspect-square w-full place-items-center overflow-hidden rounded-xl bg-ink/[0.06] text-[28px] text-ink/25">
          <RemoteArt url={a.artwork_url} seed={a.title} imgClassName="h-full w-full object-cover" />
        </span>
        <span className="mt-2 block truncate text-[13px] font-medium">{a.title}</span>
        <span className="block truncate text-[12px] text-ink/55">{[a.artists.join(', '), a.release_date?.slice(0, 4)].filter(Boolean).join(' · ')}</span>
      </button>
      <button
        onClick={props.onPlay}
        aria-label={`Play “${a.title}”`}
        title="Play"
        className="absolute right-2 top-[104px] grid h-9 w-9 place-items-center rounded-full bg-accent text-on-accent opacity-0 shadow-lg transition-opacity focus-visible:opacity-100 group-hover:opacity-100"
      >
        <IconPlay width={16} height={16} />
      </button>
    </div>
  )
}
