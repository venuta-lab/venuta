import { useState } from 'react'
import { askAddSongs } from '../../components/AddToPlaylist'
import { AvailabilityBadge } from '../../components/AvailabilityBadge'
import { IconMore, IconPlay, IconPlaylistAdd } from '../../components/Icons'
import { RemoteArt } from '../../components/RemoteArt'
import { Segmented } from '../../components/ui'
import type { OnlineAlbum, OnlineTrack } from '../../lib/engine'
import { OFFLINE } from '../../lib/online'
import { songLength, enqueueItems, playItems, toRemoteItem } from '../../lib/player'
import { useNav } from './model'
import { menuFor } from '../../lib/actions'
import { menuFromEvent } from '../../lib/menu'
import { dragSource } from '../../lib/drag'
import { fromOnline, onlineSongs, splitOnlineSongs, withOnlineAlbum, type OnlineSearch } from './onlineSearch'

/*
  The online catalog under your library's search results: songs, albums and artists for the
  same words. Rows match the local Songs list columns so local and online read as one family.
  While a new query is in flight we keep the last hits dimmed and show a skeleton, so the page
  never flashes empty for a few seconds.
*/

const items = (tracks: OnlineTrack[]) => tracks.map(toRemoteItem)
const songs = onlineSongs

/** Which online songs show: the recordings people mean, or the covers and other uploads. */
type SongView = 'best' | 'covers'

/** The Online catalog's label, the same as "In your library" above it, with how it's doing. */
function SectionLabel({ status }: { status: React.ReactNode }) {
  return (
    <div className="mb-4 flex flex-wrap items-baseline gap-x-3 gap-y-1">
      <span className="text-[12px] font-semibold uppercase tracking-[0.08em] text-ink/45">Online catalog</span>
      <span className="text-[12.5px] text-ink/50">{status}</span>
    </div>
  )
}

/** Same column rhythm as `SongList` so online and local search feel like one list. */
const COLS = 'grid-cols-[36px_minmax(0,1.7fr)_minmax(0,1.1fr)_minmax(0,1.1fr)_52px_96px_32px]'


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
 * The online catalog's part of a search, from the search state the page holds (one search for
 * the Top result, the chips and this section).
 *
 * - `libraryEmpty`: nothing in the library matched, so this section also says that, in one
 *   line that agrees with what the online search is doing (searching, found, nothing, offline).
 * - `parts`: which of songs, albums and artists show (all by default).
 * - `covers`: "tabs" (Best | Covers & other, for the Songs chip) or "fold" (Best rows, then a
 *   folded "Covers & versions" row, for All).
 * - `songLimit` / `onSeeAllSongs`: Best rows shown before "See all".
 */
export function OnlineResults(props: {
  query: string
  online: OnlineSearch
  spaced: boolean
  libraryEmpty?: boolean
  parts?: { songs?: boolean; albums?: boolean; artists?: boolean }
  covers?: 'tabs' | 'fold'
  songLimit?: number
  onSeeAllSongs?: () => void
}) {
  const { root, openOnlineAlbum, openOnlineArtist } = useNav()
  const { results, error, loading, resultsFor, offline, loadingMore, moreError, canLoadMore, loadMore } = props.online
  // Best | Covers & other (or the fold open), for the words it was chosen for: new words start
  // at Best again, folded.
  const [picked, setPicked] = useState<{ q: string; view: SongView }>({ q: '', view: 'best' })
  const q = props.query.trim()
  if (!q) return null
  const show = { songs: props.parts?.songs ?? true, albums: props.parts?.albums ?? true, artists: props.parts?.artists ?? true }
  const fold = props.covers === 'fold'

  const fresh = resultsFor === q
  const stale = loading && !!results && !fresh
  const showResults = !!results && (fresh || loading)
  const empty =
    fresh &&
    !loading &&
    results &&
    !(show.songs && results.tracks.length) &&
    !(show.albums && results.albums.length) &&
    !(show.artists && results.artists.length)
  const from = fromOnline(q)

  // The ranked songs, split: the recordings people mean (Best), and covers, karaoke, slowed,
  // loops and uploads (Covers & other). Words that ask for covers keep them all in Best.
  const splitFor = resultsFor || q
  const { best, covers } = splitOnlineSongs(results?.tracks ?? [], splitFor)
  const asked = picked.q === q ? picked.view : 'best'
  // Nothing but covers: show them rather than an empty Best (tabs), or open the fold.
  const view: SongView = asked === 'covers' || !best.length ? 'covers' : 'best'
  const shown = fold ? best : view === 'covers' ? covers : best
  const capped = props.songLimit && shown.length > props.songLimit + 2 ? shown.slice(0, props.songLimit) : shown
  const foldOpen = fold && covers.length > 0 && view === 'covers'

  if (props.libraryEmpty && empty)
    return <p className="py-20 text-center text-[14px] text-ink/55">Nothing matches “{q}” in your library or the online catalog.</p>
  const headline = props.libraryEmpty && (
    <p className="mb-6 pt-8 text-center text-[13.5px] text-ink/55">
      Nothing in your library for “{q}”{loading ? '. Searching the online catalog…' : '.'}
    </p>
  )

  const withAlbum = withOnlineAlbum

  if (offline)
    return (
      <>
        {headline}
        <section className={`${props.spaced ? 'mt-12' : ''} opacity-70`} aria-label="Online">
          <SectionLabel status={OFFLINE} />
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
        <SectionLabel
          status={
            loading ? (
              <span className="inline-flex items-center gap-2">
                <span className="spin inline-block h-3 w-3 rounded-full border-2 border-ink/20 border-t-ink/60" aria-hidden />
                Searching…
              </span>
            ) : (
              'Plays without the file; yours always plays first'
            )
          }
        />
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
          // One column that can shrink: sized by its content, long names would push the
          // song columns past the window.
          <div className={`grid grid-cols-[minmax(0,1fr)] gap-8 transition-opacity duration-200 ${stale || loading ? 'opacity-55' : 'opacity-100'}`}>
            {show.songs && results.tracks.length > 0 && (
              <div>
                <div className="mb-3 flex flex-wrap items-center gap-x-3 gap-y-2">
                  <h2 className="text-[19px] font-semibold tracking-[-0.02em]">Songs</h2>
                  {!fold && covers.length > 0 && (
                    <>
                      <Segmented
                        tabs
                        size="sm"
                        label="Which online songs"
                        value={view}
                        onChange={(v) => setPicked({ q, view: v })}
                        options={[
                          { value: 'best', label: `Best ${best.length}`, disabled: !best.length },
                          { value: 'covers', label: `Covers & other ${covers.length}` },
                        ]}
                      />
                      <span className="text-[12.5px] text-ink/50">
                        {view === 'best' ? 'The recordings, first.' : 'Covers, karaoke, slowed and other uploads.'}
                      </span>
                    </>
                  )}
                </div>
                {capped.length > 0 && <OnlineTrackHeader />}
                <div className="mt-1 grid gap-0.5">
                  {/* Play goes through the list as shown: its order, from this row (on, past a
                      "See all" cut, through the same list). */}
                  {capped.map((t, i) => (
                    <OnlineTrackRow
                      key={`${t.provider}:${t.track_id}`}
                      track={t}
                      onPlay={() => void playItems(root, items(shown), i, from)}
                      onQueue={(next) => void enqueueItems(root, items([t]), next)}
                      onAdd={() => askAddSongs(songs([t]), t.title)}
                    />
                  ))}
                </div>
                <div className="mt-2 flex flex-wrap items-center gap-3">
                  {capped.length < shown.length && props.onSeeAllSongs ? (
                    <button onClick={props.onSeeAllSongs} className="btn-ghost btn-sm">
                      See all {shown.length} online songs
                    </button>
                  ) : canLoadMore ? (
                    // The rows shown stay while more come; the list is ranked again with them.
                    <button onClick={loadMore} disabled={loadingMore} aria-busy={loadingMore} className="btn-ghost btn-sm">
                      {loadingMore && <span className="spin inline-block h-3 w-3 rounded-full border-2 border-ink/20 border-t-ink/60" aria-hidden />}
                      {loadingMore ? 'Loading more…' : 'More online songs'}
                    </button>
                  ) : (
                    !fold &&
                    fresh &&
                    !loading && <span className="text-[12.5px] text-ink/45">That’s everything the online catalog found for these words.</span>
                  )}
                  {moreError && (
                    <span role="status" className="text-[12.5px] text-ink/55">
                      {moreError}
                    </span>
                  )}
                </div>
                {fold && covers.length > 0 && (
                  <div className={best.length ? 'mt-3' : ''}>
                    <button
                      onClick={() => setPicked({ q, view: foldOpen ? 'best' : 'covers' })}
                      aria-expanded={foldOpen}
                      className="flex w-full items-center gap-2 rounded-xl px-2 py-2 text-left text-[13px] text-ink/60 transition-colors hover:bg-ink/[0.05] hover:text-ink"
                    >
                      <span aria-hidden className={`inline-block transition-transform ${foldOpen ? 'rotate-90' : ''}`}>
                        ›
                      </span>
                      <span className="font-medium">Covers & versions · {covers.length}</span>
                      <span className="text-ink/45">karaoke, slowed, lofi and other uploads</span>
                    </button>
                    {foldOpen && (
                      <div className="mt-1 grid gap-0.5">
                        {covers.map((t, i) => (
                          <OnlineTrackRow
                            key={`${t.provider}:${t.track_id}`}
                            track={t}
                            onPlay={() => void playItems(root, items(covers), i, from)}
                            onQueue={(next) => void enqueueItems(root, items([t]), next)}
                            onAdd={() => askAddSongs(songs([t]), t.title)}
                          />
                        ))}
                      </div>
                    )}
                  </div>
                )}
                {loading && (
                  <div className="mt-1">
                    <OnlineSkeleton rows={2} />
                  </div>
                )}
              </div>
            )}
            {show.albums && results.albums.length > 0 && (
              <div>
                <h2 className="mb-3 text-[19px] font-semibold tracking-[-0.02em]">Albums</h2>
                <div className="no-scrollbar -mx-3 -my-2 flex gap-4 overflow-x-auto px-3 py-2">
                  {results.albums.map((a) => (
                    <OnlineAlbumCard
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
            {show.artists && results.artists.length > 0 && (
              <div>
                <h2 className="mb-3 text-[19px] font-semibold tracking-[-0.02em]">Artists</h2>
                <div className="flex flex-wrap gap-2">
                  {results.artists.map((ar) => (
                    <button
                      key={`${ar.provider}:${ar.artist_id}`}
                      onClick={() => openOnlineArtist(ar)}
                      title={`Open ${ar.name}`}
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

/** An online album card: the card opens its page; its round button plays it. */
export function OnlineAlbumCard(props: { album: OnlineAlbum; onOpen: () => void; onPlay: () => void; onQueue: (next: boolean) => void; onAdd: () => void }) {
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
