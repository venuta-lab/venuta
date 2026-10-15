import { useEffect } from 'react'
import { AvailabilityBadge } from '../../components/AvailabilityBadge'
import { IconPlay } from '../../components/Icons'
import { RemoteArt } from '../../components/RemoteArt'
import { loadCovers, useCovers } from '../../lib/covers'
import type { CatalogTrack, OnlineAlbum, OnlineArtist, OnlineTrack } from '../../lib/engine'
import { play, playItems } from '../../lib/player'
import { recentSearches } from '../../lib/recentSearches'
import type { TopPick } from '../../lib/searchRank'
import { albumFrom, plural, useNav, type Album, type Artist } from './model'
import { fromOnline, onlineItems, playOnlineArtistPage, withOnlineAlbum } from './onlineSearch'
import { Art } from './parts'

/*
  The Top result on Music search: the one thing the words most likely mean (see `pickTop` in
  lib/searchRank), as a card with Play, and Open where there's a page to open. Your own file,
  album or artist is preferred; an online one shows the Online badge.
*/

export interface TopSources {
  query: string
  /** Library matches, in the lists the page shows. */
  tracks: CatalogTrack[]
  albums: Album[]
  artists: Artist[]
  /** Online matches: the Best songs (covers set aside), albums and artists. */
  songs: OnlineTrack[]
  onlineAlbums: OnlineAlbum[]
  onlineArtists: OnlineArtist[]
}

export function TopResult({ pick, from }: { pick: TopPick; from: TopSources }) {
  const nav = useNav()
  const { root } = nav
  const cover = useCovers(320)
  const q = from.query.trim()

  // What the pick is, and what it does.
  let art: React.ReactNode = null
  let kind = ''
  let title = ''
  let line = ''
  let online = false
  let round = false
  let onPlay: () => void = () => {}
  let onOpen: (() => void) | null = null
  let dir = ''

  if (pick.kind === 'artist' && pick.where === 'library') {
    const ar = from.artists[pick.index]
    dir = ar.albums[0]?.dir ?? ''
    kind = 'Artist'
    title = ar.name
    line = [plural(ar.albums.length, 'album'), plural(ar.songs, 'song')].join(', ')
    round = true
    onPlay = () => void play(root, ar.albums.flatMap((a) => a.tracks), 0, { kind: 'artist', ref: ar.name, label: ar.name })
    onOpen = () => nav.openArtist(ar.name)
  } else if (pick.kind === 'artist') {
    const ar = from.onlineArtists[pick.index]
    kind = 'Artist'
    title = ar.name
    line = ar.audience ? `${compact(ar.audience)} monthly listeners` : 'Plays their songs from the online catalog'
    online = true
    round = true
    art = <RemoteArt url={ar.artwork_url} fallback={ar.name.slice(0, 1)} imgClassName="h-full w-full object-cover" />
    onPlay = () => void playOnlineArtistPage(root, ar)
    onOpen = () => nav.openOnlineArtist(ar)
  } else if (pick.kind === 'song' && pick.where === 'library') {
    const t = from.tracks[pick.index]
    dir = t.dir
    kind = 'Song'
    title = t.title
    line = [t.artists.join(', '), t.album].filter(Boolean).join(' · ')
    onPlay = () => void play(root, from.tracks, pick.index, { kind: 'search', ref: q, label: `Search: ${q}` })
    onOpen = () => nav.openAlbum(t.dir)
  } else if (pick.kind === 'song') {
    const t = from.songs[pick.index]
    kind = 'Song'
    title = t.title
    line = [t.artists.join(', '), t.album].filter(Boolean).join(' · ')
    online = true
    art = <RemoteArt url={t.artwork_url} fallback="♪" imgClassName="h-full w-full object-cover" />
    // From here down the Best list, the order the Online catalog section shows.
    onPlay = () => void playItems(root, onlineItems(from.songs), pick.index, fromOnline(q))
    if (t.album_id)
      onOpen = () =>
        nav.openOnlineAlbum({
          provider: t.provider,
          album_id: t.album_id!,
          title: t.album ?? t.title,
          artists: t.artists,
          track_count: null,
          release_date: null,
          artwork_url: t.artwork_url,
        })
  } else if (pick.where === 'library') {
    const a = from.albums[pick.index]
    dir = a.dir
    kind = 'Album'
    title = a.title
    line = [a.artist, a.year, plural(a.tracks.length, 'song')].filter(Boolean).join(' · ')
    onPlay = () => void play(root, a.tracks, 0, albumFrom(a))
    onOpen = () => nav.openAlbum(a.dir)
  } else {
    const a = from.onlineAlbums[pick.index]
    kind = a.kind ?? 'Album'
    title = a.title
    line = [a.artists.join(', '), a.release_date?.slice(0, 4)].filter(Boolean).join(' · ')
    online = true
    art = <RemoteArt url={a.artwork_url} fallback="♪" imgClassName="h-full w-full object-cover" />
    onPlay = () => void withOnlineAlbum(a, (t) => void playItems(root, onlineItems(t), 0, { kind: 'search', ref: q, label: `Online: ${a.title}` }))
    onOpen = () => nav.openOnlineAlbum(a)
  }

  useEffect(() => {
    if (root && dir) void loadCovers(root, [dir])
  }, [root, dir])
  // Playing or opening the Top result means the search: it's remembered.
  const played = onPlay
  onPlay = () => {
    recentSearches.push(q)
    played()
  }
  if (!online) art = <Art cover={dir ? cover(dir) : undefined} album={{ title }} round={round} />

  return (
    <section aria-label="Top result" className="mb-10">
      <h2 className="mb-3 text-[19px] font-semibold tracking-[-0.02em]">Top result</h2>
      <div className="group flex max-w-[640px] items-center gap-5 rounded-2xl bg-ink/[0.045] p-4 shadow-[inset_0_0_0_1px_var(--edge)] transition-colors hover:bg-ink/[0.07]">
        <button
          // An artist's picture opens their page; any other plays.
          onClick={pick.kind === 'artist' && onOpen ? onOpen : onPlay}
          aria-label={pick.kind === 'artist' && onOpen ? `Open ${title}` : `Play ${title}`}
          className={`relative grid h-24 w-24 shrink-0 place-items-center overflow-hidden bg-ink/[0.06] text-[28px] text-ink/30 shadow-lg shadow-black/25 ${round ? 'rounded-full' : 'rounded-xl'}`}
        >
          {art}
          <span className="absolute inset-0 grid place-items-center bg-black/35 text-white opacity-0 transition-opacity group-hover:opacity-100">
            <IconPlay width={22} height={22} />
          </span>
        </button>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 text-[12px] font-medium text-ink/55">
            {kind}
            {online ? <AvailabilityBadge availability="online" /> : <span className="text-ink/45">· In your library</span>}
          </div>
          <div className="mt-0.5 truncate text-[22px] font-semibold leading-tight tracking-[-0.01em]">{title}</div>
          {line && <div className="mt-0.5 truncate text-[13px] text-ink/60">{line}</div>}
          <div className="mt-3 flex items-center gap-2">
            <button onClick={onPlay} className="btn-primary btn-sm">
              <IconPlay width={14} height={14} />
              Play
            </button>
            {onOpen && (
              <button onClick={onOpen} className="btn-ghost btn-sm">
                {pick.kind === 'artist' ? 'Open artist' : 'Open album'}
              </button>
            )}
          </div>
        </div>
      </div>
    </section>
  )
}

/** 642000000 → "642M". */
function compact(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(n >= 1e10 ? 0 : 1)}B`
  if (n >= 1e6) return `${(n / 1e6).toFixed(n >= 1e7 ? 0 : 1)}M`
  if (n >= 1e3) return `${Math.round(n / 1e3)}K`
  return String(n)
}
