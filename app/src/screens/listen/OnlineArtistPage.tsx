import { motion } from 'motion/react'
import { FollowButton } from '../../components/YtmButtons'
import { useEffect, useState } from 'react'
import { askAddSongs } from '../../components/AddToPlaylist'
import { AvailabilityBadge } from '../../components/AvailabilityBadge'
import { IconPause, IconPlay, IconPlaylistAdd, IconRadio, IconShuffle } from '../../components/Icons'
import { playRadioList } from '../../lib/radio'
import { RemoteArt, artUrl, cleanArtUrl } from '../../components/RemoteArt'
import { Detail } from '../../components/ui'
import { onlineArtistPage, type OnlineArtistPage as Page } from '../../lib/engine'
import { onlineProblem } from '../../lib/online'
import { current, fromIs, onlineEntryFrom, playAllItems, queueKey, toggle, usePlayerValue } from '../../lib/player'
import { artistFor, plural, spring, useNav, type Library, type OnlineArtistRef } from './model'
import { artistShelves, useArtistSongs } from './artistSongs'
import { OnlineArtistSections } from './OnlineArtistSections'
import { Shelf } from './parts'

/*
  An online artist's page, the way the catalog lays it out: their banner with name, audience and
  who they are; Play, Shuffle, Add to playlist; their top songs; then every row of their page in
  its order (Albums, Singles & EPs, Videos, Live performances, Featured on, Playlists, Fans might
  also like, Appears on). Yours comes first: what you own by them is a row of its own on top, and
  a song you have plays your file and says LOCAL. Songs are ranked for the artist's name, so their
  own recordings lead (not covers). Back returns to where it was opened from.
*/

/** Songs "Add to playlist" adds: their top ones. */
const TOP_SHOWN = 5

export function OnlineArtistPage({ artist, lib, onBack }: { artist: OnlineArtistRef; lib: Library; onBack: () => void }) {
  const nav = useNav()
  const { root } = nav
  const [page, setPage] = useState<Page | null>(null)
  const [problem, setProblem] = useState('')
  const [aboutOpen, setAboutOpen] = useState(false)
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
  const { songs, items, from, topCount } = useArtistSongs(page, name, lib)

  const nowKey = usePlayerValue((s) => {
    const c = current(s)
    return c ? queueKey(c) : ''
  })
  const playing = usePlayerValue((s) => s.playing)
  const nowFrom = usePlayerValue((s) => s.from)
  const isNow = fromIs(nowFrom, 'search', name) && items.some((it) => queueKey(it) === nowKey)
  // The same artist in your library, when there is one by this name.
  const mine = artistFor(lib, name)
  const mineSongs = mine.albums.reduce((n, a) => n + a.tracks.length, 0) || mine.songs

  const addTop = () => {
    const list = songs.slice(0, Math.max(TOP_SHOWN, topCount))
    if (list.length) askAddSongs(list.map((s) => (s.where === 'library' ? { path: s.track.path } : { online: onlineEntryFrom(s.track) })), name)
  }
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

  // The banner is wide: used at the size it comes (the cover sizer would square it).
  const banner = cleanArtUrl(page?.banner_url)
  const counts = [
    page?.subscribers ?? '',
    page?.audience ? `${compact(page.audience)} monthly audience` : '',
    page === null && !problem ? 'Reading their page…' : '',
  ].filter(Boolean)
  const about = page?.description?.trim()

  const heading = (
    <motion.div className="min-w-[260px] flex-1 pb-1" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ ...spring, delay: 0.05 }}>
      <p className="flex items-center gap-2 text-[13px] font-medium text-ink/60">
        Artist <AvailabilityBadge availability="online" />
      </p>
      <h1 className={`title hero-title mt-1 leading-[1.05] ${banner ? 'text-[52px]' : 'text-[34px]'}`}>{name}</h1>
      {counts.length > 0 && <p className="mt-2.5 text-[13px] text-ink/65">{counts.join(' · ')}</p>}
      {mine.albums.length > 0 && (
        <button onClick={() => nav.openArtist(mine.name)} className="mt-1 text-left text-[13px] text-ink/65 underline-offset-4 hover:text-ink hover:underline">
          In your library: {plural(mineSongs, 'song')}
        </button>
      )}
      {about && (
        <div className="mt-3 max-w-[68ch]">
          <p className={`whitespace-pre-line text-[13.5px] leading-relaxed text-ink/70 ${aboutOpen ? '' : 'line-clamp-2'}`}>{about}</p>
          <button onClick={() => setAboutOpen((o) => !o)} className="mt-1 text-[12.5px] font-medium text-ink/60 hover:text-ink">
            {aboutOpen ? 'Less' : 'More'}
          </button>
        </div>
      )}
      <div className="mt-5 flex flex-wrap items-center gap-2.5">
        {playButton('lg')}
        <button disabled={items.length < 2} onClick={() => void playAllItems(root, items, from, 'shuffle')} className="btn btn-lg shuffle-main">
          <IconShuffle />
          Shuffle
        </button>
        {page?.radio_id && (
          <button onClick={() => page.radio_id && void playRadioList(root, page.radio_id, `${name} radio`)} className="btn-ghost btn-lg" title={`Songs by ${name} and artists like them, without end`}>
            <IconRadio width={18} height={18} />
            Radio
          </button>
        )}
        <button disabled={!songs.length} onClick={addTop} className="btn-ghost btn-lg" title="Add their top songs to a playlist">
          <IconPlaylistAdd width={18} height={18} />
          Add to playlist
        </button>
        <FollowButton artistId={artist.artist_id} />
      </div>
    </motion.div>
  )

  return (
    <Detail title={name} onBack={onBack} glow={banner ? undefined : artUrl(page?.artwork_url ?? artist.artwork_url, 226)} banner={banner} tools={items.length ? playButton('sm') : undefined}>
      {banner ? (
        <div className="relative pt-[150px]">{heading}</div>
      ) : (
        <div className="relative flex flex-wrap items-end gap-8">
          <div className="grid aspect-square w-[220px] shrink-0 place-items-center overflow-hidden rounded-full bg-ink/[0.06] text-[56px] text-ink/25 shadow-2xl shadow-black/40">
            <RemoteArt url={page?.artwork_url ?? artist.artwork_url} fallback={name.slice(0, 1)} size={220} imgClassName="h-full w-full object-cover" />
          </div>
          {heading}
        </div>
      )}

      {problem ? (
        <p role="status" className="mt-9 py-6 text-[13px] text-ink/55">
          {problem}
        </p>
      ) : page === null ? (
        <p className="mt-9 py-6 text-[13px] text-ink/45">Reading {name}’s page from the online catalog…</p>
      ) : (
        <>
          {mine.albums.length > 0 && (
            <div className="mt-10">
              <Shelf title="In your library" albums={mine.albums} origin="online-artist" spaced={false} onSeeAll={() => nav.openArtist(mine.name)} />
            </div>
          )}
          <OnlineArtistSections page={page} name={name} songs={songs} from={from} />
          {!songs.length && !artistShelves(page).length && <p className="mt-9 py-6 text-[13px] text-ink/55">The online catalog lists nothing more for {name} yet.</p>}
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
