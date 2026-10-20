import { motion } from 'motion/react'
import { SaveAlbumButton } from '../../components/YtmButtons'
import { useEffect, useMemo, useState } from 'react'
import { askAddSongs } from '../../components/AddToPlaylist'
import { AvailabilityBadge } from '../../components/AvailabilityBadge'
import { IconPause, IconPlay, IconPlaylistAdd, IconRadio, IconSelect, IconShuffle } from '../../components/Icons'
import { SelectBar } from '../../components/SelectBar'
import { NO_PICKS, pick, pickAll, picked, type Picks } from '../../lib/selection'
import { RemoteArt, artUrl } from '../../components/RemoteArt'
import { Detail } from '../../components/ui'
import { onlineAlbumPage, type OnlineAlbum, type OnlineAlbumPage as Page } from '../../lib/engine'
import { onlineProblem } from '../../lib/online'
import { ownedIndex, titleCore } from '../../lib/owned'
import { current, fromIs, onlineEntryFrom, playAllItems, queueKey, toggle, usePlayerValue, type From } from '../../lib/player'
import { playRadioAt } from '../../lib/radio'
import { listRadio } from '../../lib/radioSession'
import { matcher, searchKey } from '../../lib/textsearch'
import { spring, useNav, type Library } from './model'
import { OnlineShelfRow } from './OnlineCards'
import { itemOf, type MixedSong } from './onlineSearch'
import { MixedSongs } from './SearchAll'

/*
  An online album as its own page, the way the catalog lays it out: its cover, what it is and
  when, who made it (each a link to their page), how long, and what it is about; Play, Shuffle,
  Add to playlist and a search of its songs; its songs, numbered, with their plays; then the
  rows under them (other versions, more by the artist, releases like it). A song you have says
  LOCAL and plays your file, and an album you have links to yours. Back returns to where it was
  opened from.
*/

export function OnlineAlbumPage({ album: a, lib, onBack }: { album: OnlineAlbum; lib: Library; onBack: () => void }) {
  const nav = useNav()
  const { root } = nav
  const [page, setPage] = useState<Page | null>(null)
  const [problem, setProblem] = useState('')
  const [aboutOpen, setAboutOpen] = useState(false)
  const [words, setWords] = useState('')
  // Picking songs to act on them together (Select).
  const [picking, setPicking] = useState(false)
  const [picks, setPicks] = useState<Picks>(NO_PICKS)
  useEffect(() => {
    let live = true
    onlineAlbumPage(a.album_id)
      .then((p) => live && setPage(p))
      .catch((e) => live && setProblem(onlineProblem(e)))
    return () => {
      live = false
    }
  }, [a.album_id])

  const album = page?.album ?? a
  const title = album.title || a.title
  // Each song once, as yours when you have it (it plays your file), numbered as on the album.
  const owns = useMemo(() => ownedIndex(lib.tracks), [lib.tracks])
  const songs: MixedSong[] = useMemo(
    () =>
      (page?.tracks ?? []).map((t): MixedSong => {
        const mine = owns(t)
        return mine ? { where: 'library', track: mine } : { where: 'online', track: t }
      }),
    [page, owns],
  )
  const items = useMemo(() => songs.map(itemOf), [songs])
  // "Playing from" names the album; choosing it there searches for it again. Its own list in the
  // catalog is what Autoplay asks YouTube Music to go on with when it ends.
  const list = page?.playlist_id ?? null
  const from: From = useMemo(() => ({ kind: 'search', ref: title, label: `Online: ${title}`, list }), [title, list])
  // Start radio: the radio its page's button names, else the album's list's.
  const radio = page ? listRadio(page.radio, list) : null
  // The songs that hold the words typed in "Search this album", with their album numbers.
  const shown = useMemo(() => {
    const all = songs.map((s, i) => ({ s, n: i + 1 }))
    const q = words.trim()
    if (!q) return all
    const has = matcher(q)
    return all.filter(({ s }) => has(searchKey(`${s.track.title} ${s.track.artists.join(' ')}`)))
  }, [songs, words])

  const shownKeys = useMemo(() => shown.map((x) => queueKey(itemOf(x.s))), [shown])
  const byKey = useMemo(() => new Map(items.map((it) => [queueKey(it), it])), [items])
  const pickedItems = picked(picks, items.map(queueKey)).flatMap((k) => byKey.get(k) ?? [])
  const stopPicking = () => {
    setPicking(false)
    setPicks(NO_PICKS)
  }

  const nowKey = usePlayerValue((s) => {
    const c = current(s)
    return c ? queueKey(c) : ''
  })
  const playing = usePlayerValue((s) => s.playing)
  const nowFrom = usePlayerValue((s) => s.from)
  const isNow = fromIs(nowFrom, 'search', title) && items.some((it) => queueKey(it) === nowKey)

  // The same album in your library, when you have it.
  const mine = useMemo(() => {
    const key = titleCore(title)
    const theirs = (page?.artists.map((x) => x.name) ?? a.artists).map(searchKey)
    return lib.albums.find((m) => titleCore(m.title) === key && (!theirs.length || theirs.includes(searchKey(m.artist))))
  }, [lib.albums, title, page, a.artists])

  const kicker = [album.kind || 'Album', album.release_date?.slice(0, 4)].filter(Boolean).join(' · ')
  const length = page?.length?.replaceAll(' • ', ' · ')
  const about = page?.description?.trim()
  const artists = page?.artists.length ? page.artists : a.artists.map((name) => ({ name, artist_id: null }))
  // The album's own list goes in one copy on YouTube Music when every song of it is online.
  const addAll = () => songs.length && askAddSongs(songs.map((s) => (s.where === 'library' ? { path: s.track.path } : { online: onlineEntryFrom(s.track) })), title, undefined, list ?? undefined)

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

  return (
    <Detail title={title} onBack={onBack} glow={artUrl(album.artwork_url ?? a.artwork_url, 240)} tools={items.length ? playButton('sm') : undefined}>
      <div className="relative flex flex-wrap items-end gap-8">
        <div className="grid aspect-square w-[240px] shrink-0 place-items-center overflow-hidden rounded-3xl bg-ink/[0.06] text-[56px] text-ink/25 shadow-2xl shadow-black/40">
          <RemoteArt url={album.artwork_url ?? a.artwork_url} seed={title} size={240} imgClassName="h-full w-full object-cover" />
        </div>
        <motion.div className="min-w-[260px] flex-1 pb-1" initial={{ opacity: 0, y: 12 }} animate={{ opacity: 1, y: 0 }} transition={{ ...spring, delay: 0.05 }}>
          <p className="flex items-center gap-2 text-[13px] font-medium text-ink/60">
            {kicker} <AvailabilityBadge availability="online" />
            {page?.explicit && (
              <span title="Explicit" className="rounded bg-ink/[0.12] px-1 text-[10.5px] font-semibold leading-[16px] text-ink/70">
                E
              </span>
            )}
          </p>
          <h1 className="title hero-title mt-1 text-[34px] leading-[1.1]">{title}</h1>
          {artists.length > 0 && (
            <p className="mt-2 text-[16px] font-medium text-ink/85">
              {artists.map((ar, i) => (
                <span key={`${ar.name}:${i}`}>
                  {i > 0 && ', '}
                  {ar.artist_id ? (
                    <button
                      onClick={() => ar.artist_id && nav.openOnlineArtist({ artist_id: ar.artist_id, name: ar.name })}
                      className="underline-offset-4 hover:text-ink hover:underline"
                    >
                      {ar.name}
                    </button>
                  ) : (
                    ar.name
                  )}
                </span>
              ))}
            </p>
          )}
          <p className="mt-2.5 text-[13px] text-ink/60">{page === null && !problem ? 'Reading its songs…' : length}</p>
          {mine && (
            <button onClick={() => nav.openAlbum(mine.dir)} className="mt-1 text-left text-[13px] text-ink/65 underline-offset-4 hover:text-ink hover:underline">
              In your library: open your copy
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
            {radio && (
              <button onClick={() => void playRadioAt(root, radio, `${title} radio`)} className="btn-ghost btn-lg" title={`Play YouTube Music's radio of “${title}”`}>
                <IconRadio width={18} height={18} />
                Start radio
              </button>
            )}
            <button disabled={!items.length} onClick={addAll} className="btn-ghost btn-lg">
              <IconPlaylistAdd width={18} height={18} />
              Add to playlist
            </button>
            <button disabled={!items.length} onClick={() => (picking ? stopPicking() : setPicking(true))} aria-pressed={picking} className="btn-ghost btn-lg">
              <IconSelect width={18} height={18} />
              Select
            </button>
            <SaveAlbumButton albumId={a.album_id} />
          </div>
        </motion.div>
      </div>

      <div className="mt-9">
        {problem ? (
          <p role="status" className="py-6 text-[13px] text-ink/55">
            {problem}
          </p>
        ) : page === null ? (
          <p className="py-6 text-[13px] text-ink/45">Reading the album’s songs from the online catalog…</p>
        ) : !songs.length ? (
          <p className="py-6 text-[13px] text-ink/55">The online catalog lists no songs for this album.</p>
        ) : (
          <>
            {songs.length > 4 && (
              <div className="mb-3 flex justify-end">
                <input
                  type="search"
                  value={words}
                  onChange={(e) => setWords(e.target.value)}
                  onKeyDown={(e) => e.key === 'Escape' && words && (e.stopPropagation(), setWords(''))}
                  placeholder="Search this album"
                  aria-label="Search this album"
                  className="field h-9 w-[240px] rounded-full px-4 text-[13px]"
                />
              </div>
            )}
            {shown.length ? (
              <MixedSongs
                query={title}
                from={from}
                title=""
                songs={shown.map((x) => x.s)}
                numbers={shown.map((x) => x.n)}
                pick={picking ? { keys: picks.keys, onPick: (key, range) => setPicks((p) => pick(p, shownKeys, key, { range })) } : undefined}
              />
            ) : (
              <p className="py-6 text-[13px] text-ink/55">No song on this album has “{words.trim()}”.</p>
            )}
          </>
        )}
      </div>
      {/* Room under the songs, so the bar never covers the last of them. */}
      {picking && <div aria-hidden className="h-24" />}
      <SelectBar
        open={picking}
        root={root}
        items={pickedItems}
        total={shownKeys.length}
        from={from}
        label={title}
        onAll={() => setPicks((p) => pickAll(p, shownKeys))}
        onDone={stopPicking}
      />
      {page?.shelves.map((s) => <OnlineShelfRow key={s.title} shelf={s} label={title} onSeeAll={s.more ? () => s.more && nav.openOnlineShelf(s.title, title, s.more) : undefined} />)}
    </Detail>
  )
}
